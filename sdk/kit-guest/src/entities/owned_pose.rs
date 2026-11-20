// How the façade reads a pose the physics owns (docs/decisions/2026-11-18-proprietaire-position.md): a kept copy
// follows the revision of the body (given by the direct link with the pose), and a module with neither rows nor a
// direct link asks the core's accessor by the bridge. Part of `entities.rs`, which declares it (`mod owned_pose;`).
// Used by: `Entities::get`, `get_mut`, `component` (through `fresh`), `Entities::transform`.

use super::*;

impl Entities {
    pub(super) fn fresh(&self, id: &str) -> Option<*mut Kept> {
        // Whether a reference to the copy held was handed out in this era already (then it is not changed in place).
        let held = self.reads.borrow().kept.get(id).copied();
        // SAFETY: a live box of `kept`.
        let lent_before = held.map(|q| (q, unsafe { (*q).lent }));
        let p = self.fresh_copy(id)?;
        let era = self.reads.borrow().era;
        let lent = lent_before.is_some_and(|(q, l)| q == p && l == era);
        Some(self.follow_body(p, lent))
    }

    /// The pose of the kept copy `p` (current at this check): its transform, which follows the body that owns it (its
    /// revision), else is the entity's own.
    pub(super) fn kept_pose(&self, p: *mut Kept) -> Option<Pos> {
        // SAFETY: a live box of `kept`.
        let lent = unsafe { (*p).lent } == self.reads.borrow().era;
        let p = self.follow_body(p, lent);
        // SAFETY: as above.
        Pos::of(unsafe { (*p).entity.component("transform") })
    }

    /// The kept copy `p`, its transform following the body that owns its pose
    /// (docs/decisions/2026-11-18-proprietaire-position.md): once per check of the world, the core says the body's
    /// revision (direct link, with the pose); the copy keeps its value while that revision did not change, else the
    /// body's pose is written in it. Nothing when no body owns it (the row says so, or the core did) or without the
    /// direct link (the copy then comes from the core at each check).
    pub(super) fn follow_body(&self, mut p: *mut Kept, lent: bool) -> *mut Kept {
        if !crate::direct::LINKED {
            return p;
        }
        let (check, era) = {
            let r = self.reads.borrow();
            (r.check, r.era)
        };
        // SAFETY: a live box of `kept` (changed in place only when no reference to it was handed out in this era,
        // else a changed copy takes its place, as in `apply`).
        unsafe {
            if (*p).body_seen == check {
                return p;
            }
            (*p).body_seen = check;
            let id = (*p).entity.id.clone();
            if crate::columns::table().is_some() && !crate::columns::owned(&id) {
                (*p).body = hot::NO_BODY;
                return p;
            }
            let Some(Some((pose, body))) = crate::direct::transform_body(&id) else {
                return p;
            };
            let Some(body) = body else {
                (*p).body = hot::NO_BODY;
                return p;
            };
            if u64::from(body) == (*p).body {
                return p;
            }
            if lent {
                let copy = Box::into_raw(Box::new(Kept {
                    entity: (*p).entity.clone(),
                    ..*p
                }));
                let mut r = self.reads.borrow_mut();
                r.grave.push(p);
                r.kept.insert(id, copy);
                p = copy;
            }
            if let Some((_, Json::Object(t))) = (*p)
                .entity
                .components
                .iter_mut()
                .find(|(k, _)| k == "transform")
            {
                for (k, v) in ["x", "y", "z", "yaw"].into_iter().zip(pose) {
                    if let Some(n) = serde_json::Number::from_f64(v) {
                        t.insert(k.to_string(), Json::Number(n));
                    }
                }
            }
            (*p).body = u64::from(body);
            (*p).lent = era;
        }
        p
    }

    /// `transform(id)` asked of the core by the bridge (`f::TRANSFORM`): its accessor, the cascade of an owned pose. What
    /// a module with neither rows nor a direct link (the bridge alone, another thread) reads, never its copies.
    pub(super) fn transform_by_bridge(&self, id: &str) -> Option<Pos> {
        let bytes = call(crate::func!(f::TRANSFORM), |w| {
            w.str(id);
        });
        let mut r = Reader::new(&bytes);
        if !r.bool().unwrap_or(false) {
            return None;
        }
        Some(Pos {
            x: r.f64().ok()?,
            y: r.f64().ok()?,
            z: r.f64().ok()?,
            yaw: r.f64().ok()?,
        })
    }
}
