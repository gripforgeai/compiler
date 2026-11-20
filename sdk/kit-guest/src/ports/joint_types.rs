//! The joints of the physics port (brique 2, decision of 12/11: joints now): two bodies held together — fixed, a
//! ball (rotation free), a hinge (rotation about one axis), a slider (translation along one axis), a spring (a
//! distance pulled back to its rest) or a cone (a ball whose swing away from its axis is held within an angle, its
//! twist about the axis within [min, max] if given: the joints of a ragdoll). A joint is named (`id`), its bodies by their entities; its anchors are in each
//! body's own frame. One description for every wire: the kitcall codec, the orders of the link, the module's entry.

use super::V3;

/// The kind of a joint (its number on every wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointKind {
    Fixed = 0,
    Ball = 1,
    Hinge = 2,
    Slider = 3,
    Spring = 4,
    Cone = 5,
}

impl JointKind {
    /// The kind of a name (`fixed`, `ball`, `hinge`, `slider`, `spring`, `cone`).
    pub fn named(name: &str) -> Option<JointKind> {
        Some(match name {
            "fixed" => JointKind::Fixed,
            "ball" => JointKind::Ball,
            "hinge" => JointKind::Hinge,
            "slider" => JointKind::Slider,
            "spring" => JointKind::Spring,
            "cone" => JointKind::Cone,
            _ => return None,
        })
    }
    fn of(n: f64) -> Option<JointKind> {
        [
            JointKind::Fixed,
            JointKind::Ball,
            JointKind::Hinge,
            JointKind::Slider,
            JointKind::Spring,
            JointKind::Cone,
        ]
        .get(n as usize)
        .copied()
        .filter(|_| n >= 0.0)
    }
}

/// A joint between two bodies.
#[derive(Clone, Debug, PartialEq)]
pub struct JointDesc {
    pub kind: JointKind,
    /// Where the joint holds each body, in that body's frame.
    pub anchor_a: V3,
    pub anchor_b: V3,
    /// The axis of a hinge, a slider or a cone, in the frame of the first body (+Y by default).
    pub axis: V3,
    /// The limits of a hinge (radians) or a slider (metres), none by default.
    pub limits: Option<[f64; 2]>,
    /// A spring: its length at rest, how hard it pulls back and how it damps.
    pub rest_length: f64,
    pub stiffness: f64,
    pub damping: f64,
    /// Whether the two bodies still collide with each other (no by default).
    pub collide: bool,
    /// A cone: the largest angle (radians) between the axis seen by each body, 0 by default (the axis held).
    pub swing: f64,
    /// A cone: the twist of the second body about the axis (radians, [min, max]), free by default.
    pub twist: Option<[f64; 2]>,
}

/// The words of a joint description on the link and at the module's entry (contract `abi.physics.joint`, core/abi/
/// contracts/abi.physics.joint.json: every index generated).
pub use gripforge_contracts::abi_physics_joint as joint_words;
/// The words of a joint description.
pub const JOINT_WORDS: usize = joint_words::WORDS;

impl JointDesc {
    pub fn new(kind: JointKind) -> JointDesc {
        JointDesc {
            kind,
            anchor_a: [0.0; 3],
            anchor_b: [0.0; 3],
            axis: [0.0, 1.0, 0.0],
            limits: None,
            rest_length: 0.0,
            stiffness: 1.0,
            damping: 0.0,
            collide: false,
            swing: 0.0,
            twist: None,
        }
    }

    /// Its words: kind, anchor a (3), anchor b (3), axis (3), limited (0 or 1), low, high, rest length, stiffness,
    /// damping, collide (0 or 1), swing, twisted (0 or 1), twist min, twist max.
    pub fn words(&self) -> [f64; JOINT_WORDS] {
        use joint_words::at;
        let [lo, hi] = self.limits.unwrap_or([0.0; 2]);
        let [t0, t1] = self.twist.unwrap_or([0.0; 2]);
        let mut w = [0.0; JOINT_WORDS];
        w[at::KIND] = self.kind as u8 as f64;
        w[at::ANCHOR_A..at::ANCHOR_A + 3].copy_from_slice(&self.anchor_a);
        w[at::ANCHOR_B..at::ANCHOR_B + 3].copy_from_slice(&self.anchor_b);
        w[at::AXIS..at::AXIS + 3].copy_from_slice(&self.axis);
        w[at::LIMITED] = self.limits.is_some() as u8 as f64;
        (w[at::LOW], w[at::HIGH]) = (lo, hi);
        w[at::REST_LENGTH] = self.rest_length;
        w[at::STIFFNESS] = self.stiffness;
        w[at::DAMPING] = self.damping;
        w[at::COLLIDE] = self.collide as u8 as f64;
        w[at::SWING] = self.swing;
        w[at::TWISTED] = self.twist.is_some() as u8 as f64;
        (w[at::TWIST_MIN], w[at::TWIST_MAX]) = (t0, t1);
        w
    }

    /// The description of its words (`None`: too few words or an unknown kind).
    pub fn of_words(w: &[f64]) -> Option<JointDesc> {
        use joint_words as j;
        j::check(w).ok()?;
        Some(JointDesc {
            kind: JointKind::of(j::kind(w))?,
            anchor_a: j::anchor_a(w),
            anchor_b: j::anchor_b(w),
            axis: j::axis(w),
            limits: (j::limited(w) != 0.0).then_some([j::low(w), j::high(w)]),
            rest_length: j::rest_length(w),
            stiffness: j::stiffness(w),
            damping: j::damping(w),
            collide: j::collide(w) != 0.0,
            swing: j::swing(w),
            twist: (j::twisted(w) != 0.0).then_some([j::twist_min(w), j::twist_max(w)]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_joint_reads_back_from_its_words() {
        let mut j = JointDesc::new(JointKind::Hinge);
        j.anchor_a = [1.0, 2.0, 3.0];
        j.axis = [1.0, 0.0, 0.0];
        j.limits = Some([-0.5, 0.5]);
        j.collide = true;
        assert_eq!(JointDesc::of_words(&j.words()), Some(j));
        let mut c = JointDesc::new(JointKind::Cone);
        (c.swing, c.twist) = (0.6, Some([-0.2, 0.4]));
        assert_eq!(JointDesc::of_words(&c.words()), Some(c));
        assert_eq!(JointDesc::of_words(&[9.0; JOINT_WORDS]), None);
        assert_eq!(JointDesc::of_words(&[0.0; 3]), None);
    }
}
