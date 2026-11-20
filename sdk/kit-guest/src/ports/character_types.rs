//! The options of a character controller and the frame of a character (brique 2: the origin of what is followed, plus
//! an offset only when the game asks for one), shared by
//! the kit façade and the core like `port_types.rs` (compiled once in `gripforge-abi`, `ports/mod.rs`).

use super::V3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterOptions {
    pub max_slope_deg: f64,
    pub step_height: f64,
    pub snap_distance: f64,
    /// Gap kept from what the controller touches (`core.physics` config `character.skinRatio`), read by ONE rule,
    /// [`CharacterOptions::ground_gap`]: the default [`DEFAULT_SKIN_RATIO`] keeps the physics' own fixed gap (5 cm),
    /// another value of 0.001..0.5 is a fraction of the capsule radius (at least 1 cm).
    pub skin_ratio: f64,
    /// The offset from the origin of the followed object to the point the kits give and read (m); `None`: the
    /// config's `character.originOffset`, else 0.
    pub origin_offset: Option<V3>,
}

/// The gap a character controller keeps from what it touches when no ratio is asked (m): the physics' own fixed one.
pub const DEFAULT_GROUND_GAP: f64 = 0.05;

/// The default of `character.skinRatio` (the manifest's): the physics' own fixed gap, [`DEFAULT_GROUND_GAP`].
pub const DEFAULT_SKIN_RATIO: f64 = 0.05;

impl CharacterOptions {
    /// The gap (m) the controller of a capsule of `radius` really keeps from the ground, the one rule of `skinRatio`:
    /// the default ([`DEFAULT_SKIN_RATIO`]) or no ratio (0, NaN, out of 0.001..0.5) keeps [`DEFAULT_GROUND_GAP`];
    /// another ratio is that fraction of the radius, at least 1 cm. The physics keeps it and core.physics publishes
    /// it (`physics.ground_gap`) from this same function, for a game to put the body's feet where they really rest.
    pub fn ground_gap(&self, radius: f64) -> f64 {
        let r = self.skin_ratio;
        let ratio =
            r.is_finite() && (0.001..=0.5).contains(&r) && (r - DEFAULT_SKIN_RATIO).abs() > 1e-9;
        if ratio && radius > 0.0 {
            (r * radius).max(0.01)
        } else {
            DEFAULT_GROUND_GAP
        }
    }

    /// The controller's numbers, the frame of the config.
    pub fn of(max_slope_deg: f64, step_height: f64, snap_distance: f64, skin_ratio: f64) -> Self {
        CharacterOptions {
            max_slope_deg,
            step_height,
            snap_distance,
            skin_ratio,
            ..Default::default()
        }
    }
}

impl Default for CharacterOptions {
    fn default() -> Self {
        CharacterOptions {
            max_slope_deg: 45.0,
            step_height: 0.4,
            snap_distance: 0.3,
            skin_ratio: DEFAULT_SKIN_RATIO,
            origin_offset: None,
        }
    }
}

/// The frame of a character (brique 2, decision of the owner): the origin of what is followed, as it is, offset 0 by
/// default on every engine; an offset only when the game gives one (a value, or the value its function answers to the
/// hook `core.physics.character.offset`, asked by the kit when the character is made). The kit computes nothing, reads
/// no bone. The physics keeps the capsule's centre; [`CharacterFrame::lift`] goes from the point to the centre.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CharacterFrame {
    pub offset: V3,
}

impl CharacterFrame {
    /// From the point a kit gives to the capsule's centre: the offset taken back. The capsule's centre is the origin
    /// of the followed object unless the game said otherwise.
    pub fn lift(&self, _height: f64) -> V3 {
        [-self.offset[0], -self.offset[1], -self.offset[2]]
    }
    /// The frame of `opts` over this one (the config's).
    pub fn with(self, opts: &CharacterOptions) -> CharacterFrame {
        CharacterFrame {
            offset: opts.origin_offset.unwrap_or(self.offset),
        }
    }
}
