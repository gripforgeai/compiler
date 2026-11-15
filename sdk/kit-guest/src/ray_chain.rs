//! Questions to the physics that take several rays in a row (skip a character, slide along a wall), asked and
//! settled one answer at a time (docs/kit-modules.md §7.17).
//!
//! A kit keeps a [`Chains`] in its state. [`Chains::run`] drives the chain of a key: it takes the answers that
//! are ready, asks the next ray of the chain, and gives the verdict of the last chain that finished. On the thread
//! of the game the physics answers at once, so the whole chain settles in the call and the verdict is the one a
//! blocking loop gives; when the physics runs on a loop of its own the chain goes on one answer per tick and the
//! verdict is the last one that arrived (`fallback` until the first).
use crate::host::Kx;
use crate::ports::{RayHit, Ticket};
use std::collections::{HashMap, VecDeque};
use std::task::Poll;

type Vec3 = [f64; 3];

/// Default concurrency. Queued observations keep their turn instead of cancelling live questions.
pub const IN_FLIGHT: usize = 8;

/// What a kit decides of one ray of a chain.
pub enum Verdict {
    /// The chain is over: this is its answer.
    Done(bool),
    /// Go on from further along the segment, no longer excluding `exclude` when given.
    Skip {
        advance: f64,
        exclude: Option<String>,
    },
}

/// How a chain is bounded.
#[derive(Clone, Copy)]
pub struct Rule {
    /// Rays at most.
    pub max_rays: u32,
    /// The answer when the segment is used up (`travelled`, `total`) before the kit decided.
    pub used_up: fn(f64, f64) -> bool,
    /// Added to the length of every ray (what is looked for beyond the segment).
    pub extra: f64,
}

pub struct Chain {
    pub origin: Vec3,
    pub dir: Vec3,
    pub total: f64,
    pub travelled: f64,
    pub exclude: Option<String>,
    /// A number the kit leaves for itself when it decides (the distance it stopped at).
    pub note: f64,
    rays: u32,
    ticket: Ticket,
    seq: u64,
    touched: f64,
}

pub type Classify<'a> = &'a mut dyn FnMut(&mut Kx, Option<&RayHit>, &mut Chain) -> Verdict;

#[derive(Default)]
pub struct Chains {
    flight: HashMap<String, Chain>,
    last: HashMap<String, bool>,
    seq: u64,
    limit: usize,
    waiting: HashMap<String, f64>,
    queue: VecDeque<String>,
    prune_at: f64,
}

impl Chains {
    /// Scale a caller's bounded observation budget without changing other kits.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit: limit.clamp(IN_FLIGHT, 128),
            ..Self::default()
        }
    }

    fn capacity(&self) -> usize {
        self.limit.max(IN_FLIGHT)
    }

    fn admit(&mut self, kx: &mut Kx, key: &str) -> bool {
        let now = kx.elapsed();
        if now >= self.prune_at {
            self.prune_at = now + 0.25;
            // A caller that disappears must not hold a slot or the head of the queue forever.
            self.flight.retain(|_, c| {
                if now - c.touched > 2.0 {
                    kx.physics().forget(c.ticket);
                    false
                } else {
                    true
                }
            });
            self.waiting.retain(|_, asked| now - *asked <= 2.0);
            self.queue.retain(|key| self.waiting.contains_key(key));
        }
        if let Some(asked) = self.waiting.get_mut(key) {
            *asked = now;
        } else {
            // The caller can retry when a place opens; no pending physical question is discarded.
            if self.waiting.len() >= 4096 {
                return false;
            }
            self.waiting.insert(key.to_owned(), now);
            self.queue.push_back(key.to_owned());
        }
        if self.flight.len() >= self.capacity() || self.queue.front().is_none_or(|next| next != key)
        {
            return false;
        }
        self.queue.pop_front();
        self.waiting.remove(key);
        true
    }

    /// The chain of `key` toward `dir`: `classify` decides each ray (`None` is a miss). `fallback` is the verdict
    /// while no chain of this key has finished.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        kx: &mut Kx,
        key: &str,
        origin: Vec3,
        dir: Vec3,
        total: f64,
        exclude: Option<&str>,
        rule: Rule,
        fallback: bool,
        classify: Classify,
    ) -> bool {
        if let Some((done, _)) = self.step(kx, key, rule, classify) {
            self.last.insert(key.to_owned(), done);
        }
        if !self.flight.contains_key(key) && self.admit(kx, key) {
            if let Some((done, _)) =
                self.start(kx, key, origin, dir, total, exclude, rule, classify)
            {
                self.last.insert(key.to_owned(), done);
            }
        }
        self.last.get(key).copied().unwrap_or(fallback)
    }

    /// Takes the answers that came for the chain of `key`: `Some((verdict, note))` when it is over.
    pub fn step(
        &mut self,
        kx: &mut Kx,
        key: &str,
        rule: Rule,
        classify: Classify,
    ) -> Option<(bool, f64)> {
        let mut c = self.flight.remove(key)?;
        c.touched = kx.elapsed();
        match drive(kx, &mut c, rule, classify) {
            Some(done) => Some((done, c.note)),
            None => {
                self.flight.insert(key.to_owned(), c);
                None
            }
        }
    }

    /// Begins a chain for `key` (none in flight) and takes the answers that are already there.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        kx: &mut Kx,
        key: &str,
        origin: Vec3,
        dir: Vec3,
        total: f64,
        exclude: Option<&str>,
        rule: Rule,
        classify: Classify,
    ) -> Option<(bool, f64)> {
        let mut c = Chain {
            origin,
            dir,
            total,
            travelled: 0.0,
            exclude: exclude.map(str::to_owned),
            note: 0.0,
            rays: 0,
            ticket: Ticket(0),
            seq: self.seq,
            touched: kx.elapsed(),
        };
        self.seq += 1;
        c.ticket = ask(kx, &c, rule);
        match drive(kx, &mut c, rule, classify) {
            Some(done) => Some((done, c.note)),
            None => {
                if self.flight.len() >= self.capacity() {
                    if let Some(old) = self
                        .flight
                        .iter()
                        .min_by_key(|(_, c)| c.seq)
                        .map(|(k, _)| k.clone())
                    {
                        if let Some(o) = self.flight.remove(&old) {
                            kx.physics().forget(o.ticket);
                        }
                    }
                }
                self.flight.insert(key.to_owned(), c);
                None
            }
        }
    }

    /// Is a chain of `key` waiting for its answers?
    pub fn pending(&self, key: &str) -> bool {
        self.flight.contains_key(key)
    }

    /// Gives back everything in flight (a scene change, a teardown).
    pub fn clear(&mut self, kx: &mut Kx) {
        for (_, c) in self.flight.drain() {
            kx.physics().forget(c.ticket);
        }
        self.last.clear();
        self.waiting.clear();
        self.queue.clear();
        self.prune_at = 0.0;
    }
}

fn ask(kx: &mut Kx, c: &Chain, rule: Rule) -> Ticket {
    let o = [
        c.origin[0] + c.dir[0] * c.travelled,
        c.origin[1] + c.dir[1] * c.travelled,
        c.origin[2] + c.dir[2] * c.travelled,
    ];
    kx.physics().ask_raycast(
        o,
        c.dir,
        c.total - c.travelled + rule.extra,
        c.exclude.as_deref(),
    )
}

/// Takes the answers that are ready; `Some` once the chain is over.
fn drive(kx: &mut Kx, c: &mut Chain, rule: Rule, classify: Classify) -> Option<bool> {
    loop {
        let Poll::Ready(hit) = kx.physics().poll_hit(c.ticket) else {
            return None;
        };
        c.rays += 1;
        match classify(kx, hit.as_ref(), c) {
            Verdict::Done(b) => return Some(b),
            Verdict::Skip { advance, exclude } => {
                c.travelled += advance;
                if let Some(e) = exclude {
                    c.exclude = Some(e);
                }
                if c.rays >= rule.max_rays || c.travelled >= c.total {
                    return Some((rule.used_up)(c.travelled, c.total));
                }
                c.ticket = ask(kx, c, rule);
            }
        }
    }
}

/// One ray of a fan.
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
    pub max_toi: f64,
    pub exclude: Option<String>,
}

/// Rays asked together whose answers are used together (the candidates of a decision). They are pure questions, so
/// asking them all at once gives what asking them one after the other gives.
pub struct Fan {
    tickets: Vec<Ticket>,
    got: Vec<Option<Option<RayHit>>>,
    seq: u64,
}

impl Fan {
    pub fn ask(kx: &mut Kx, rays: &[Ray]) -> Fan {
        let tickets = rays
            .iter()
            .map(|r| {
                kx.physics()
                    .ask_raycast(r.origin, r.dir, r.max_toi, r.exclude.as_deref())
            })
            .collect();
        Fan {
            tickets,
            got: vec![None; rays.len()],
            seq: 0,
        }
    }

    /// The answers once every ray is answered.
    pub fn poll(&mut self, kx: &mut Kx) -> Option<Vec<Option<RayHit>>> {
        for (i, t) in self.tickets.iter().enumerate() {
            if self.got[i].is_none() {
                if let Poll::Ready(h) = kx.physics().poll_hit(*t) {
                    self.got[i] = Some(h);
                }
            }
        }
        if self.got.iter().any(Option::is_none) {
            return None;
        }
        Some(self.got.iter_mut().map(|g| g.take().flatten()).collect())
    }

    /// Gives the tickets back without reading what is not read yet.
    pub fn forget(self, kx: &mut Kx) {
        for (i, t) in self.tickets.iter().enumerate() {
            if self.got[i].is_none() {
                kx.physics().forget(*t);
            }
        }
    }
}

/// The fans of a kit, one per key, at most [`IN_FLIGHT`] at once.
#[derive(Default)]
pub struct Fans {
    flight: HashMap<String, Fan>,
    seq: u64,
}

impl Fans {
    /// The answers of the fan of `key` when one finished in this call: the one in flight, or else a new one
    /// (`rays` builds it) when the physics answers at once. `None`: the answers come on a later call.
    pub fn run(
        &mut self,
        kx: &mut Kx,
        key: &str,
        rays: impl FnOnce() -> Vec<Ray>,
    ) -> Option<Vec<Option<RayHit>>> {
        let mut fan = match self.flight.remove(key) {
            Some(f) => f,
            None => {
                let mut f = Fan::ask(kx, &rays());
                f.seq = self.seq;
                self.seq += 1;
                f
            }
        };
        if let Some(done) = fan.poll(kx) {
            return Some(done);
        }
        if self.flight.len() >= IN_FLIGHT {
            if let Some(old) = self
                .flight
                .iter()
                .min_by_key(|(_, f)| f.seq)
                .map(|(k, _)| k.clone())
            {
                if let Some(o) = self.flight.remove(&old) {
                    o.forget(kx);
                }
            }
        }
        self.flight.insert(key.to_owned(), fan);
        None
    }

    pub fn clear(&mut self, kx: &mut Kx) {
        for (_, f) in self.flight.drain() {
            f.forget(kx);
        }
    }
}
