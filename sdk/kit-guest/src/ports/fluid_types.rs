//! What floats and what it floats in, for the physics port (the floating of core.physics, provisional names): a body's
//! **buoyancy** (its density or its volume, how the fluid brakes it, the cells that feel the fluid) and a **fluid
//! source** (a surface: a level, up to four Gerstner waves of world.water's swell, a footprint — everywhere, a box, a
//! disc or a path —, a density, a drag, a flow, a clock). Data only, one description for every wire (the kitcall
//! codec, the orders of the link, the module's entry: words of f64); the arithmetic is gripforge-kit-common's `fluid`
//! (one function of the waves, shared by the port and world.water).
//!
//! The fluid source is internal to core.physics: no service, no event, no column names it (the catalogue of surface
//! behaviours of core.material plugs in there later). A fluid source the port is given is evaluated by the port
//! itself at each of its cells and substeps; it is given again only when it changes (a wind), never per tick. A port
//! that does not float bodies answers false (said by its name by core.physics): there is no second floating.

use super::{Shape, V3};

/// The most cells of one floating body.
pub const BUOYANCY_MAX_POINTS: usize = 8;
/// The words of a floating body, its cells, a fluid, a wave: contracts `abi.physics.buoyancy`, `buoyancy_cell`,
/// `fluid`, `wave` (core/abi/contracts/), every index generated.
pub use gripforge_contracts::{
    abi_physics_buoyancy as buoyancy_words, abi_physics_buoyancy_cell as buoyancy_cell_words,
    abi_physics_fluid as fluid_words, abi_physics_wave as wave_words,
};
/// The words of a buoyancy before its cells: density, volume, drag, flow drag, side drag.
pub const BUOYANCY_HEAD_WORDS: usize = buoyancy_words::WORDS;
/// The words of one cell: its position, its half sizes.
pub const BUOYANCY_POINT_WORDS: usize = buoyancy_cell_words::WORDS;
/// The most waves of a fluid's surface (world.water's swell has four).
pub const FLUID_MAX_WAVES: usize = 4;
/// The most points of a fluid's path (a river).
pub const FLUID_MAX_PATH: usize = 32;
/// The words of a fluid before its waves: level, density, drag, flow (2), flow along, depth, clock, footprint, its 4
/// numbers, the count of waves, the count of path points.
pub const FLUID_HEAD_WORDS: usize = fluid_words::WORDS;
/// The words of one wave: dirX, dirZ, amplitude, length, omega, phase, steepness (world.water's `waves_json`).
pub const WAVE_WORDS: usize = wave_words::WORDS;
/// The density of water, kg/m³ (what a fluid that says none is).
pub const WATER_DENSITY: f64 = 1000.0;

/// One cell of a floating body, in the body's frame: a small box of half sizes `half` centred on `at`, which carries
/// an equal share of the body's mass. How high it stands in the world is read from its half sizes turned by the
/// body (a cell of three equal half sizes is round: the same whatever the turn), so that a body floats the same on
/// any of its faces. A cell gives only the place and the height of what is under water, never the volume: the volume
/// is the body's mass over its density, or its declared volume (a hollow hull is not its box).
#[derive(Clone, Debug, PartialEq)]
pub struct BuoyancyPoint {
    pub at: V3,
    pub half: V3,
}

impl BuoyancyPoint {
    /// A round cell of `radius` at `at` (a vehicle's float, a sphere).
    pub fn round(at: V3, radius: f64) -> BuoyancyPoint {
        BuoyancyPoint {
            at,
            half: [radius; 3],
        }
    }
}

/// How a body floats: what the port needs once (given again only when it changes).
#[derive(Clone, Debug, PartialEq)]
pub struct BuoyancyDesc {
    /// The body's density, kg/m³ (500: half under in water), used when `volume` is 0.
    pub density: f64,
    /// The body's volume, m³ (0: the mass over `density`).
    pub volume: f64,
    /// How hard the fluid damps the up and down of each cell against the surface's own (per kg of the body, times
    /// the immersion).
    pub drag: f64,
    /// How hard the fluid pulls each cell along with its flow, sideways and forward (per kg, times the immersion).
    pub flow_drag: f64,
    /// How hard a hull's sideways slide is damped while wet (per kg; 0: none).
    pub side_drag: f64,
    /// The cells that feel the fluid (empty: taken from the body's shape, [`BuoyancyDesc::points_of`]).
    pub points: Vec<BuoyancyPoint>,
}

impl Default for BuoyancyDesc {
    fn default() -> Self {
        BuoyancyDesc {
            density: 500.0,
            volume: 0.0,
            // A crate's heave settles in about 3 s (damping ratio near 0.3 for a 1 m crate half under), a barrel takes
            // 90 % of a river's speed within 3 s.
            drag: 6.0,
            flow_drag: 2.0,
            side_drag: 0.0,
            points: Vec::new(),
        }
    }
}

impl BuoyancyDesc {
    /// The density of the body: its mass over its volume when a volume is given, else `density`.
    pub fn body_density(&self, mass: f64) -> f64 {
        if self.volume > 0.0 && mass > 0.0 {
            mass / self.volume
        } else {
            self.density.max(1e-3)
        }
    }

    /// The cells of a shape: the eight octants of its box (a box; a hull, a mesh, a compound by their bounds — their
    /// place only, never their volume), one round cell for a sphere, two stacked along its axis for a capsule.
    /// Nothing for a shape that cannot move (a plane, a heightfield).
    pub fn points_of(shape: &Shape) -> Vec<BuoyancyPoint> {
        let octants = |c: V3, h: V3| {
            let q = h.map(|x| (x / 2.0).max(1e-3));
            let mut out = Vec::with_capacity(8);
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    for sz in [-1.0, 1.0] {
                        out.push(BuoyancyPoint {
                            at: [c[0] + sx * q[0], c[1] + sy * q[1], c[2] + sz * q[2]],
                            half: q,
                        });
                    }
                }
            }
            out
        };
        match shape {
            Shape::Box { size } => octants([0.0; 3], size.map(|s| s / 2.0)),
            Shape::Sphere { radius } => vec![BuoyancyPoint::round([0.0; 3], radius.max(1e-3))],
            Shape::Capsule { radius, height } => {
                let q = (height / 4.0).max(1e-3);
                let r = radius.max(1e-3);
                vec![
                    BuoyancyPoint {
                        at: [0.0, q, 0.0],
                        half: [r, q, r],
                    },
                    BuoyancyPoint {
                        at: [0.0, -q, 0.0],
                        half: [r, q, r],
                    },
                ]
            }
            Shape::Plane { .. } | Shape::HeightField { .. } => Vec::new(),
            other => {
                let (v, _) = other.triangles();
                if v.len() < 3 {
                    return Vec::new();
                }
                let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for p in v.as_chunks::<3>().0 {
                    for i in 0..3 {
                        lo[i] = lo[i].min(f64::from(p[i]));
                        hi[i] = hi[i].max(f64::from(p[i]));
                    }
                }
                let c = [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0);
                octants(c, [0, 1, 2].map(|i| (hi[i] - lo[i]) / 2.0))
            }
        }
    }

    /// Its cells, or those of `shape` when it gives none.
    pub fn points_or(&self, shape: &Shape) -> Vec<BuoyancyPoint> {
        if self.points.is_empty() {
            Self::points_of(shape)
        } else {
            self.points.clone()
        }
    }

    /// Its words: density, volume, drag, flow drag, side drag; then per cell: at (3), half sizes (3).
    pub fn words(&self) -> Vec<f64> {
        use buoyancy_words::at;
        let n = self.points.len().min(BUOYANCY_MAX_POINTS);
        let mut w = vec![0.0; BUOYANCY_HEAD_WORDS + n * BUOYANCY_POINT_WORDS];
        w[at::DENSITY] = self.density;
        w[at::VOLUME] = self.volume;
        w[at::DRAG] = self.drag;
        w[at::FLOW_DRAG] = self.flow_drag;
        w[at::SIDE_DRAG] = self.side_drag;
        for (p, o) in self.points.iter().zip(
            w[BUOYANCY_HEAD_WORDS..]
                .as_chunks_mut::<{ BUOYANCY_POINT_WORDS }>()
                .0
                .iter_mut(),
        ) {
            use buoyancy_cell_words::at;
            o[at::POSITION..at::POSITION + 3].copy_from_slice(&p.at);
            o[at::HALF..at::HALF + 3].copy_from_slice(&p.half);
        }
        w
    }

    /// The description of its words (`None`: too few words, or a number that is not finite).
    pub fn of_words(w: &[f64]) -> Option<BuoyancyDesc> {
        use buoyancy_words as b;
        if b::check(w).is_err() || w.iter().any(|x| !x.is_finite()) {
            return None;
        }
        let points = w[BUOYANCY_HEAD_WORDS..]
            .as_chunks::<{ BUOYANCY_POINT_WORDS }>()
            .0
            .iter()
            .take(BUOYANCY_MAX_POINTS)
            .map(|p| BuoyancyPoint {
                at: buoyancy_cell_words::position(p),
                half: buoyancy_cell_words::half(p).map(|x| x.max(1e-3)),
            })
            .collect();
        Some(BuoyancyDesc {
            density: b::density(w).max(1e-3),
            volume: b::volume(w).max(0.0),
            drag: b::drag(w).max(0.0),
            flow_drag: b::flow_drag(w).max(0.0),
            side_drag: b::side_drag(w).max(0.0),
            points,
        })
    }
}

/// One Gerstner wave of a surface: direction (unit, XZ), amplitude (m), length (m), angular frequency (rad/s), phase,
/// steepness (0 to 1: how far the crests are drawn together). world.water's swell, the same numbers as its shader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wave {
    pub dir: [f64; 2],
    pub amplitude: f64,
    pub length: f64,
    pub omega: f64,
    pub phase: f64,
    pub steepness: f64,
}

impl Wave {
    /// The wave number (rad/m).
    pub fn k(&self) -> f64 {
        std::f64::consts::TAU / self.length
    }
    /// Its words: dirX, dirZ, amplitude, length, omega, phase, steepness.
    pub fn words(&self) -> [f64; WAVE_WORDS] {
        use wave_words::at;
        let mut w = [0.0; WAVE_WORDS];
        w[at::DIR..at::DIR + 2].copy_from_slice(&self.dir);
        w[at::AMPLITUDE] = self.amplitude;
        w[at::LENGTH] = self.length;
        w[at::OMEGA] = self.omega;
        w[at::PHASE] = self.phase;
        w[at::STEEPNESS] = self.steepness;
        w
    }

    /// The wave of its words (`None`: too few, not finite, or a length that is not positive).
    pub fn of_words(w: &[f64]) -> Option<Wave> {
        use wave_words as v;
        let w = w.get(..WAVE_WORDS)?;
        if w.iter().any(|x| !x.is_finite()) || v::length(w) <= 0.0 {
            return None;
        }
        Some(Wave {
            dir: v::dir(w),
            amplitude: v::amplitude(w),
            length: v::length(w),
            omega: v::omega(w),
            phase: v::phase(w),
            steepness: v::steepness(w),
        })
    }
}

/// Where a fluid is, seen from above (world.water's bodies of water: a box, a disc, a path).
#[derive(Clone, Debug, PartialEq)]
pub enum Footprint {
    /// Everywhere (a level of the whole world).
    Everywhere,
    /// A box aligned on X and Z: its corners `[minX, minZ]`, `[maxX, maxZ]`.
    Rect { min: [f64; 2], max: [f64; 2] },
    /// A disc: its centre (X, Z) and radius.
    Disc { center: [f64; 2], radius: f64 },
    /// A path (a river): its points (X, Z), in the direction it flows, and its whole width.
    Path { points: Vec<[f64; 2]>, width: f64 },
}

/// A fluid source: a surface over a footprint (provisional; the catalogue of surface behaviours gives them later).
#[derive(Clone, Debug, PartialEq)]
pub struct FluidDesc {
    /// The level of the surface at rest (m).
    pub level: f64,
    /// kg/m³ (water 1000).
    pub density: f64,
    /// A factor on every body's drag (water 1; mud more).
    pub drag: f64,
    /// A flow the same everywhere (m/s along X and Z).
    pub flow: [f64; 2],
    /// A flow along a path (m/s), in the direction of its nearest segment (a river that turns): added to `flow`.
    pub flow_along: f64,
    /// How deep the fluid goes under its level (m; a point deeper is not in it).
    pub depth: f64,
    /// The time of the surface (s) when the fluid is given: the port adds its own substeps to it.
    pub clock: f64,
    pub footprint: Footprint,
    pub waves: Vec<Wave>,
}

impl FluidDesc {
    /// A flat fluid of water at `level`, everywhere, deep without end.
    pub fn plane(level: f64) -> FluidDesc {
        FluidDesc {
            level,
            density: WATER_DENSITY,
            drag: 1.0,
            flow: [0.0; 2],
            flow_along: 0.0,
            depth: f64::MAX,
            clock: 0.0,
            footprint: Footprint::Everywhere,
            waves: Vec::new(),
        }
    }

    /// Its words: level, density, drag, flow (2), flow along, depth, clock, footprint (0 everywhere, 1 box, 2 disc,
    /// 3 path), its 4 numbers (box: minX, minZ, maxX, maxZ; disc: centre X, Z, radius; path: width), the count of
    /// waves, the count of path points; then per wave its 7, then per path point X, Z.
    pub fn words(&self) -> Vec<f64> {
        let waves = &self.waves[..self.waves.len().min(FLUID_MAX_WAVES)];
        let (kind, a, path): (f64, [f64; 4], &[[f64; 2]]) = match &self.footprint {
            Footprint::Everywhere => (0.0, [0.0; 4], &[]),
            Footprint::Rect { min, max } => (1.0, [min[0], min[1], max[0], max[1]], &[]),
            Footprint::Disc { center, radius } => (2.0, [center[0], center[1], *radius, 0.0], &[]),
            Footprint::Path { points, width } => (
                3.0,
                [*width, 0.0, 0.0, 0.0],
                &points[..points.len().min(FLUID_MAX_PATH)],
            ),
        };
        use fluid_words::at;
        let mut w = vec![0.0; FLUID_HEAD_WORDS];
        w[at::LEVEL] = self.level;
        w[at::DENSITY] = self.density;
        w[at::DRAG] = self.drag;
        w[at::FLOW..at::FLOW + 2].copy_from_slice(&self.flow);
        w[at::FLOW_ALONG] = self.flow_along;
        w[at::DEPTH] = self.depth;
        w[at::CLOCK] = self.clock;
        w[at::FOOTPRINT] = kind;
        w[at::AREA..at::AREA + 4].copy_from_slice(&a);
        w[at::WAVES] = waves.len() as f64;
        w[at::PATH] = path.len() as f64;
        for wave in waves {
            w.extend(wave.words());
        }
        for p in path {
            w.extend(p);
        }
        w
    }

    /// The fluid of its words (`None`: too few words, a number that is not finite, an unknown footprint).
    pub fn of_words(w: &[f64]) -> Option<FluidDesc> {
        if fluid_words::check(w).is_err() || w.iter().any(|x| !x.is_finite()) {
            return None;
        }
        use fluid_words as h;
        let n_waves = (h::waves(w).max(0.0) as usize).min(FLUID_MAX_WAVES);
        let n_path = (h::path(w).max(0.0) as usize).min(FLUID_MAX_PATH);
        let area = h::area(w);
        let path_at = FLUID_HEAD_WORDS + n_waves * WAVE_WORDS;
        if w.len() < path_at + 2 * n_path {
            return None;
        }
        let waves = (0..n_waves)
            .map(|i| Wave::of_words(&w[FLUID_HEAD_WORDS + i * WAVE_WORDS..]))
            .collect::<Option<Vec<Wave>>>()?;
        let footprint = match h::footprint(w) as u32 {
            0 => Footprint::Everywhere,
            1 => Footprint::Rect {
                min: [area[0], area[1]],
                max: [area[2], area[3]],
            },
            2 => Footprint::Disc {
                center: [area[0], area[1]],
                radius: area[2].max(0.0),
            },
            3 => Footprint::Path {
                points: (0..n_path)
                    .map(|i| [w[path_at + 2 * i], w[path_at + 2 * i + 1]])
                    .collect(),
                width: area[0].max(0.0),
            },
            _ => return None,
        };
        Some(FluidDesc {
            level: h::level(w),
            density: h::density(w).max(1e-3),
            drag: h::drag(w).max(0.0),
            flow: h::flow(w),
            flow_along: h::flow_along(w),
            depth: h::depth(w).max(0.0),
            clock: h::clock(w),
            footprint,
            waves,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buoyancy_and_a_fluid_read_back_from_their_words() {
        let mut b = BuoyancyDesc {
            volume: 0.4,
            side_drag: 1.5,
            ..BuoyancyDesc::default()
        };
        b.points = vec![BuoyancyPoint::round([0.5, -0.2, 1.0], 0.3)];
        assert_eq!(BuoyancyDesc::of_words(&b.words()), Some(b));
        assert_eq!(BuoyancyDesc::of_words(&[1.0; 3]), None);
        let wave = Wave {
            dir: [1.0, 0.0],
            amplitude: 0.2,
            length: 8.0,
            omega: 2.7,
            phase: 0.3,
            steepness: 0.5,
        };
        for footprint in [
            Footprint::Everywhere,
            Footprint::Rect {
                min: [-5.0, -4.0],
                max: [5.0, 6.0],
            },
            Footprint::Disc {
                center: [1.0, 2.0],
                radius: 8.0,
            },
            Footprint::Path {
                points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
                width: 3.0,
            },
        ] {
            let f = FluidDesc {
                depth: 3.0,
                flow_along: 1.5,
                footprint,
                waves: vec![wave],
                ..FluidDesc::plane(2.0)
            };
            assert_eq!(FluidDesc::of_words(&f.words()), Some(f));
        }
        let mut cut = FluidDesc::plane(0.0).words();
        cut[8] = 9.0;
        assert_eq!(FluidDesc::of_words(&cut), None, "an unknown footprint");
    }

    #[test]
    fn the_cells_of_a_shape_are_its_octants_its_centre_or_its_axis() {
        let p = BuoyancyDesc::points_of(&Shape::Box {
            size: [2.0, 1.0, 4.0],
        });
        assert_eq!(p.len(), 8);
        assert!(p.iter().all(|q| q.half == [0.5, 0.25, 1.0]));
        assert!(p.iter().any(|q| q.at == [0.5, -0.25, 1.0]));
        assert_eq!(
            BuoyancyDesc::points_of(&Shape::Sphere { radius: 0.3 }),
            vec![BuoyancyPoint::round([0.0; 3], 0.3)]
        );
        assert_eq!(
            BuoyancyDesc::points_of(&Shape::Capsule {
                radius: 0.3,
                height: 2.0
            })
            .len(),
            2
        );
        assert!(BuoyancyDesc::points_of(&Shape::Plane {
            normal: [0.0, 1.0, 0.0]
        })
        .is_empty());
    }
}
