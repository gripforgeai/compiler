//! Seeded mulberry32 with `fork(label)`, bit for bit the generator of runtime/rng.ts. game.engine 1.3:
//! `state()` / `restore(state)` (`{ seed, a }`, two unsigned 32-bit integers), what a kit saves in its own section.
#![allow(
    clippy::should_implement_trait,
    reason = "a seeded random stream named like the web code's; it never ends, it is not an Iterator"
)]

use crate::js::hash32;
use serde_json::Value as Json;

#[derive(Clone, Debug)]
pub struct Rng {
    seed32: u32,
    a: u32,
}

/// The seed of a host: a number or a string, as `createRng(seed)` takes it.
#[derive(Clone, Debug)]
pub enum Seed {
    Number(f64),
    Text(String),
}

impl Seed {
    pub fn from_json(v: Option<&Json>) -> Seed {
        match v {
            Some(Json::String(s)) => Seed::Text(s.clone()),
            Some(Json::Number(n)) => Seed::Number(n.as_f64().unwrap_or(1.0)),
            _ => Seed::Number(1.0),
        }
    }
}

impl Rng {
    pub fn new(seed: &Seed) -> Rng {
        let s = match seed {
            // `Math.floor(seed) >>> 0`
            Seed::Number(n) => {
                let f = n.floor();
                if f.is_finite() {
                    (f.rem_euclid(4_294_967_296.0)) as u32
                } else {
                    0
                }
            }
            Seed::Text(t) => hash32(t),
        };
        let seed32 = if s == 0 { 0x9e37_79b9 } else { s };
        Rng { seed32, a: seed32 }
    }

    pub fn from_u32(n: u32) -> Rng {
        Rng::new(&Seed::Number(n as f64))
    }

    pub fn next(&mut self) -> f64 {
        self.a = self.a.wrapping_add(0x6d2b_79f5);
        let a = self.a;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
    }

    pub fn range(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min) * self.next()
    }

    pub fn int(&mut self, min: i64, max_inclusive: i64) -> i64 {
        min + (self.next() * ((max_inclusive - min + 1) as f64)).floor() as i64
    }

    pub fn pick<'a, T>(&mut self, list: &'a [T]) -> Option<&'a T> {
        let i = (self.next() * list.len() as f64).floor() as usize;
        list.get(i)
    }

    /// An independent, reproducible stream: `createRng(hash32(`${seed32}:${label}`))`.
    pub fn fork(&self, label: &str) -> Rng {
        Rng::from_u32(hash32(&format!("{}:{}", self.seed32, label)))
    }

    /// `rng.state()`: the seed the forks derive from and the position in the stream, as JSON.
    pub fn state(&self) -> Json {
        serde_json::json!({ "seed": self.seed32, "a": self.a })
    }

    /// `rng.restore(state)`: puts back a state `state()` gave; false (nothing changes) for anything else.
    pub fn restore(&mut self, state: &Json) -> bool {
        let part = |k: &str| {
            state
                .get(k)
                .and_then(Json::as_f64)
                .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= 4_294_967_295.0)
                .map(|n| n as u32)
        };
        match (part("seed"), part("a")) {
            (Some(seed), Some(a)) => {
                self.seed32 = seed;
                self.a = a;
                true
            }
            _ => false,
        }
    }
}
