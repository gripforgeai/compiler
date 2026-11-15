//! The joints of the physics port (brique 2, decision of 12/11: joints now): two bodies held together — fixed, a
//! ball (rotation free), a hinge (rotation about one axis), a slider (translation along one axis) or a spring (a
//! distance pulled back to its rest). A joint is named (`id`), its bodies by their entities; its anchors are in each
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
}

impl JointKind {
    /// The kind of a name (`fixed`, `ball`, `hinge`, `slider`, `spring`).
    pub fn named(name: &str) -> Option<JointKind> {
        Some(match name {
            "fixed" => JointKind::Fixed,
            "ball" => JointKind::Ball,
            "hinge" => JointKind::Hinge,
            "slider" => JointKind::Slider,
            "spring" => JointKind::Spring,
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
    /// The axis of a hinge or a slider, in the frame of the first body (+Y by default).
    pub axis: V3,
    /// The limits of a hinge (radians) or a slider (metres), none by default.
    pub limits: Option<[f64; 2]>,
    /// A spring: its length at rest, how hard it pulls back and how it damps.
    pub rest_length: f64,
    pub stiffness: f64,
    pub damping: f64,
    /// Whether the two bodies still collide with each other (no by default).
    pub collide: bool,
}

/// The words of a joint description on the link and at the module's entry.
pub const JOINT_WORDS: usize = 17;

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
        }
    }

    /// Its words: kind, anchor a (3), anchor b (3), axis (3), limited (0 or 1), low, high, rest length, stiffness,
    /// damping, collide (0 or 1).
    pub fn words(&self) -> [f64; JOINT_WORDS] {
        let [lo, hi] = self.limits.unwrap_or([0.0; 2]);
        let (a, b, x) = (self.anchor_a, self.anchor_b, self.axis);
        [
            self.kind as u8 as f64,
            a[0],
            a[1],
            a[2],
            b[0],
            b[1],
            b[2],
            x[0],
            x[1],
            x[2],
            self.limits.is_some() as u8 as f64,
            lo,
            hi,
            self.rest_length,
            self.stiffness,
            self.damping,
            self.collide as u8 as f64,
        ]
    }

    /// The description of its words (`None`: too few words or an unknown kind).
    pub fn of_words(w: &[f64]) -> Option<JointDesc> {
        let w: &[f64; JOINT_WORDS] = w.get(..JOINT_WORDS)?.try_into().ok()?;
        Some(JointDesc {
            kind: JointKind::of(w[0])?,
            anchor_a: [w[1], w[2], w[3]],
            anchor_b: [w[4], w[5], w[6]],
            axis: [w[7], w[8], w[9]],
            limits: (w[10] != 0.0).then_some([w[11], w[12]]),
            rest_length: w[13],
            stiffness: w[14],
            damping: w[15],
            collide: w[16] != 0.0,
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
        assert_eq!(JointDesc::of_words(&[9.0; JOINT_WORDS]), None);
        assert_eq!(JointDesc::of_words(&[0.0; 3]), None);
    }
}
