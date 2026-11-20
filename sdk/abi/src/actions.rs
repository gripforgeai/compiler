//! The state of the actions written OUTSIDE every tick (decision of 20/11, docs/design/etat-des-actions-hors-tick.md):
//! the kit that owns the inputs publishes its bindings as DATA ([`crate::zone::action_bindings`]: control → action,
//! dead zone, saturation, curve, factors), and the host applies them AT EACH EVENT, where it reads the devices
//! (`InputPort`, core/kit-api), into one neutral block per action ([`crate::zone::action_state`]): the cumulated value,
//! the host's time of its last change, and its integral (value × seconds, for a stick read as a speed). Every loop reads
//! it in place at its own instant (the physics at its step, the render at its frame, the tick at its tick). No kit
//! reads a device; the host knows no action and no kit: an entry names a control (its key) and the block it drives.
//!
//! The arithmetic is the one of the owner's table (core.input `table.rs` `read_binding`, `response.rs`), in `f32` as
//! there, so the value the host writes is the value the table reads at the same instant (guarded on core.input's side
//! by `tests/model/host_state.rs`). Written new.

use crate::zone::action_bindings as b;

/// One entry of the bindings (`zone::action_bindings`), decoded.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Entry {
    /// The control's key: [`control_key`] of its code (`KeyW`, `StickLeft`, a virtual control's name).
    pub control: u64,
    /// The device: its kind ([`b::ANY`]: any) and its index ([`b::ANY`]: any).
    pub kind: u8,
    pub index: u8,
    /// What the control gives the group ([`b::role`]).
    pub role: u8,
    /// [`b::flag`].
    pub flags: u8,
    /// Where the action's block ([`crate::zone::action_state`]) is in the zone.
    pub state: u32,
    /// The entries of one binding share their group (a binding of four keys is one group).
    pub group: u32,
    /// The dead zone, the saturation, the curve (`response.rs`); for a [`b::role::WITH`], the threshold.
    pub deadzone: f32,
    pub saturation: f32,
    pub curve: f32,
    /// What multiplies the group's value (the binding's response: sensitivity, inversion; its scale).
    pub gain: [f32; 2],
    /// What multiplies the action's sum (the player's settings), after the clamp of an axis.
    pub post: [f32; 2],
}

/// The key of a control's code: FNV-1a 64 of its UTF-8 bytes (the same as `zone::published::hash`).
pub const fn control_key(code: &str) -> u64 {
    crate::zone::published::hash(code)
}

fn rd<const N: usize>(at: &[u8], o: u32) -> [u8; N] {
    let o = o as usize;
    at[o..o + N].try_into().unwrap_or([0; N])
}

impl Entry {
    /// Decodes the entry at the start of `at` ([`b::ENTRY`] bytes at least).
    pub fn read(at: &[u8]) -> Entry {
        let f = |o| f32::from_le_bytes(rd(at, o));
        Entry {
            control: u64::from_le_bytes(rd(at, b::E_CONTROL)),
            kind: at[b::E_KIND as usize],
            index: at[b::E_INDEX as usize],
            role: at[b::E_ROLE as usize],
            flags: at[b::E_FLAGS as usize],
            state: u32::from_le_bytes(rd(at, b::E_STATE)),
            group: u32::from_le_bytes(rd(at, b::E_GROUP)),
            deadzone: f(b::E_DEADZONE),
            saturation: f(b::E_SATURATION),
            curve: f(b::E_CURVE),
            gain: [f(b::E_GAIN), f(b::E_GAIN + 4)],
            post: [f(b::E_POST), f(b::E_POST + 4)],
        }
    }

    /// Encodes the entry into the start of `out` ([`b::ENTRY`] bytes).
    pub fn write(&self, out: &mut [u8]) {
        let mut put = |o: u32, v: &[u8]| out[o as usize..o as usize + v.len()].copy_from_slice(v);
        put(b::E_CONTROL, &self.control.to_le_bytes());
        put(b::E_KIND, &[self.kind, self.index, self.role, self.flags]);
        put(b::E_STATE, &self.state.to_le_bytes());
        put(b::E_GROUP, &self.group.to_le_bytes());
        put(b::E_DEADZONE, &self.deadzone.to_le_bytes());
        put(b::E_SATURATION, &self.saturation.to_le_bytes());
        put(b::E_CURVE, &self.curve.to_le_bytes());
        put(b::E_GAIN, &self.gain[0].to_le_bytes());
        put(b::E_GAIN + 4, &self.gain[1].to_le_bytes());
        put(b::E_POST, &self.post[0].to_le_bytes());
        put(b::E_POST + 4, &self.post[1].to_le_bytes());
    }

    /// Whether a record of the device `(kind, index)` on the control `control` is this entry's.
    pub fn matches(&self, control: u64, kind: u8, index: u8) -> bool {
        self.control == control
            && (self.kind == b::ANY || self.kind == kind)
            && (self.index == b::ANY || self.index == index)
    }
}

/// One analog axis: dead zone, saturation, curve, sign kept (`response.rs` `shape1`).
fn shape1(e: &Entry, v: f32) -> f32 {
    let a = v.abs();
    if a <= e.deadzone {
        return 0.0;
    }
    let span = (e.saturation - e.deadzone).max(1e-6);
    let n = ((a - e.deadzone) / span).min(1.0);
    n.powf(e.curve).copysign(v)
}

/// A stick: radial or axial dead zone, the length within 1 (`response.rs` `shape2`).
fn shape2(e: &Entry, x: f32, y: f32) -> (f32, f32) {
    if e.flags & b::flag::AXIAL != 0 {
        return (shape1(e, x), shape1(e, y));
    }
    let len = (x * x + y * y).sqrt();
    if len <= e.deadzone || len == 0.0 {
        return (0.0, 0.0);
    }
    let k = shape1(e, len.min(1.0)) / len;
    (x * k, y * k)
}

/// The value of the action whose block is at `state`, from `entries` and the controls held now: `held(control, kind,
/// index_filter)` answers the sum of `(x, y)` over the devices that hold the control and pass the entry's device filter
/// (the owner's `value`). Every group gives its value × its gain, summed; an axis is clamped; then the player's factor.
pub fn evaluate(
    entries: &[Entry],
    state: u32,
    mut held: impl FnMut(&Entry) -> (f32, f32),
) -> (f32, f32) {
    let mut sum = (0.0f32, 0.0f32);
    let mut post = [1.0f32, 1.0];
    let mut clamp = false;
    let mut i = 0;
    let mine: Vec<&Entry> = entries.iter().filter(|e| e.state == state).collect();
    while i < mine.len() {
        let g = mine[i].group;
        let mut j = i;
        while j < mine.len() && mine[j].group == g {
            j += 1;
        }
        let group = &mine[i..j];
        i = j;
        post = group[0].post;
        clamp |= group[0].flags & b::flag::CLAMP_X != 0;
        // A combination not held: the group gives nothing.
        if group
            .iter()
            .filter(|e| e.role == b::role::WITH)
            .any(|e| held(e).0.abs() < e.deadzone)
        {
            continue;
        }
        let mut keys = [0.0f32; 4];
        let mut v = (0.0f32, 0.0f32);
        let mut composite = false;
        for e in group {
            let (x, y) = held(e);
            match e.role {
                b::role::VECTOR => v = shape2(e, x, y),
                b::role::AXIS => v = (shape1(e, x), 0.0),
                b::role::RAW => v = (x, 0.0),
                b::role::POS_X | b::role::NEG_X | b::role::POS_Y | b::role::NEG_Y => {
                    composite = true;
                    keys[usize::from(e.role - b::role::POS_X)] = x.abs().min(1.0);
                }
                _ => {}
            }
        }
        if composite {
            let (mut x, mut y) = (keys[0] - keys[1], keys[2] - keys[3]);
            let len = (x * x + y * y).sqrt();
            if len > 1.0 {
                x /= len;
                y /= len;
            }
            v = (x, y);
        }
        let gain = group[0].gain;
        sum.0 += v.0 * gain[0];
        sum.1 += v.1 * gain[1];
    }
    if clamp {
        sum = (sum.0.clamp(-1.0, 1.0), 0.0);
    }
    (sum.0 * post[0], sum.1 * post[1])
}

/// Reads the entries of a bindings table (contract `abi.input.bindings`) from `bytes`, the copy of its block taken
/// while `seq` was `seq` (even: not being written), `bound` its head read once (where the rows are, their stride);
/// `count` the entries in use. `None` when the rows are not whole in `bytes`.
pub fn read_table(
    bytes: &[u8],
    bound: &crate::zone::table::Bound,
    count: u32,
) -> Option<Vec<Entry>> {
    let rows = (bound.rows - bound.at) as usize;
    let stride = bound.stride as usize;
    let n = count.min(bound.slots) as usize;
    if stride < b::ENTRY as usize || rows + n * stride > bytes.len() {
        return None;
    }
    Some(
        (0..n)
            .map(|i| Entry::read(&bytes[rows + i * stride..]))
            .collect(),
    )
}

/// An action's block now, from its fields: `(x, y)` and the integral at `now` (host's clock, s): the integral to the
/// last change plus the value held since.
pub fn integral_at(t: f64, x: f64, y: f64, ix: f64, iy: f64, now: f64) -> (f64, f64) {
    let since = (now - t).max(0.0);
    (ix + x * since, iy + y * since)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: &str, role: u8, group: u32) -> Entry {
        Entry {
            control: control_key(code),
            kind: b::ANY,
            index: b::ANY,
            role,
            flags: 0,
            state: 64,
            group,
            deadzone: 0.15,
            saturation: 1.0,
            curve: 1.0,
            gain: [1.0, 1.0],
            post: [1.0, 1.0],
        }
    }

    fn held<'a>(on: &'a [(&'a str, f32, f32)]) -> impl FnMut(&Entry) -> (f32, f32) + 'a {
        move |e| {
            on.iter()
                .filter(|(c, ..)| control_key(c) == e.control)
                .fold((0.0, 0.0), |a, (_, x, y)| (a.0 + x, a.1 + y))
        }
    }

    /// Four keys of one binding: a diagonal stays 1 long, the opposite keys cancel.
    #[test]
    fn four_keys_make_one_vector_within_one() {
        let e = [
            key("KeyD", b::role::POS_X, 1),
            key("KeyA", b::role::NEG_X, 1),
            key("KeyW", b::role::POS_Y, 1),
            key("KeyS", b::role::NEG_Y, 1),
        ];
        assert_eq!(evaluate(&e, 64, held(&[("KeyW", 1.0, 0.0)])), (0.0, 1.0));
        let (x, y) = evaluate(&e, 64, held(&[("KeyW", 1.0, 0.0), ("KeyD", 1.0, 0.0)]));
        assert!(((x * x + y * y).sqrt() - 1.0).abs() < 1e-6 && x > 0.0 && y > 0.0);
        assert_eq!(
            evaluate(&e, 64, held(&[("KeyA", 1.0, 0.0), ("KeyD", 1.0, 0.0)])),
            (0.0, 0.0)
        );
        assert_eq!(
            evaluate(&e, 65, held(&[("KeyW", 1.0, 0.0)])),
            (0.0, 0.0),
            "another block"
        );
    }

    /// A stick: the radial dead zone, the curve, the gain (an inverted y), and an axis clamped before the player's factor.
    #[test]
    fn a_stick_is_shaped_and_an_axis_clamped() {
        let mut s = key("StickLeft", b::role::VECTOR, 1);
        s.gain = [1.0, -1.0];
        assert_eq!(
            evaluate(&[s], 64, held(&[("StickLeft", 0.1, 0.0)])),
            (0.0, 0.0)
        );
        let (x, y) = evaluate(&[s], 64, held(&[("StickLeft", 0.0, 1.0)]));
        assert!(x.abs() < 1e-6 && (y + 1.0).abs() < 1e-6);
        let mut a = key("KeyD", b::role::AXIS, 1);
        a.flags = b::flag::CLAMP_X;
        a.post = [2.0, 1.0];
        let mut c = a;
        c.control = control_key("PadAxis4");
        c.group = 2;
        assert_eq!(
            evaluate(
                &[a, c],
                64,
                held(&[("KeyD", 1.0, 0.0), ("PadAxis4", 1.0, 0.0)])
            ),
            (2.0, 0.0),
            "1 + 1 clamped to 1, then the player's factor"
        );
    }

    /// A combination: the group counts only while its modifier is held over the threshold.
    #[test]
    fn a_combination_gates_its_group() {
        let mut with = key("ShiftLeft", b::role::WITH, 1);
        with.deadzone = 0.5;
        let k = key("KeyW", b::role::POS_Y, 1);
        assert_eq!(
            evaluate(&[with, k], 64, held(&[("KeyW", 1.0, 0.0)])),
            (0.0, 0.0)
        );
        assert_eq!(
            evaluate(
                &[with, k],
                64,
                held(&[("KeyW", 1.0, 0.0), ("ShiftLeft", 1.0, 0.0)])
            ),
            (0.0, 1.0)
        );
    }

    #[test]
    fn an_entry_and_a_table_round_trip() {
        use crate::zone::table;
        let mut e = key("KeyW", b::role::POS_Y, 7);
        e.kind = 0;
        e.index = 2;
        e.gain = [0.5, -2.0];
        // At 8: an offset 0 is never a block of the zone.
        let mut bytes = vec![0u8; 8 + b::bytes(2) as usize];
        table::lay(&mut bytes, 8, &b::SPEC, 2, 2);
        let bound = table::bind(&b::SPEC, 8, |o| {
            u32::from_le_bytes(bytes[o as usize..o as usize + 4].try_into().unwrap())
        })
        .unwrap();
        e.write(&mut bytes[bound.row(0) as usize..]);
        e.write(&mut bytes[bound.row(1) as usize..]);
        let list = read_table(&bytes[8..], &bound, 2).unwrap();
        assert_eq!((list.len(), list[1]), (2, e));
        assert!(list[0].matches(control_key("KeyW"), 0, 2));
        assert!(!list[0].matches(control_key("KeyW"), 0, 1));
        assert!(read_table(&bytes[8..28], &bound, 2).is_none(), "not whole");
    }

    #[test]
    fn the_integral_holds_the_value_since_the_last_change() {
        assert_eq!(integral_at(10.0, 0.5, -1.0, 2.0, 0.0, 12.0), (3.0, -2.0));
        assert_eq!(
            integral_at(10.0, 0.5, -1.0, 2.0, 0.0, 9.0),
            (2.0, 0.0),
            "never backward"
        );
    }
}
