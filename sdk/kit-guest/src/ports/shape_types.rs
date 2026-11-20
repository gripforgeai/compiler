//! The shapes of the physics (box, sphere, capsule, mesh, hull, compound, heightfield, plane, cylinder, cone, triangle) and
//! their geometry (half height,
//! triangles, rotation helpers), shared by the kit façade and the core like `port_types.rs` (compiled once in
//! `gripforge-abi`, `ports/mod.rs`). Split out of `port_types.rs` as is.

use super::{Quat, V3};

/// Capsule `height` is the total height (caps included) along local +Y. Meshes are static only.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Box {
        size: V3,
    },
    Sphere {
        radius: f64,
    },
    Capsule {
        radius: f64,
        height: f64,
    },
    Mesh {
        vertices: Vec<f32>,
        indices: Vec<u32>,
    },
    /// A convex hull (the collision tool's `convex`, docs/usage/collision.md): its points, and its faces as
    /// triangles over them (what a host whose physics only takes meshes builds). `border`: the radius it is rounded
    /// by, metres, added around the hull (0: sharp). A rounded box, a wedge, a pyramid, a tetrahedron are hulls.
    Convex {
        vertices: Vec<f32>,
        indices: Vec<u32>,
        border: f64,
    },
    /// Several shapes in one body (`compound`, a convex decomposition), each at its place in the body. Kept packed
    /// (`Shape::compound`, `Shape::parts`) so the scalar wires pass it by address like a mesh.
    Compound {
        floats: Vec<f32>,
        indices: Vec<u32>,
    },
    /// A heightfield (static): `heights` row by row along +X then +Z, `cols` per row, cells of `cell` (x, z) metres,
    /// its first corner at the body's origin; each cell cut along its (x0,z0)–(x1,z1) diagonal. Said as such, never
    /// guessed from a mesh.
    HeightField {
        cols: u32,
        cell: [f64; 2],
        heights: Vec<f32>,
    },
    /// A plane (static) through the body's origin, solid under it along -`normal` (a half-space).
    Plane {
        normal: V3,
    },
    /// A cylinder along local +Y, centred: `radius`, total `height`, rounded by `border` (0: sharp edges; the
    /// rounding is inside the given size).
    Cylinder {
        radius: f64,
        height: f64,
        border: f64,
    },
    /// A cone along local +Y, centred: its base of `radius` at -height/2, its apex at +height/2, rounded by `border`
    /// (inside the given size).
    Cone {
        radius: f64,
        height: f64,
        border: f64,
    },
    /// A triangle of three points in the body's space (x, y, z of each, its float stream on the wires), thickened
    /// by `border` (0: a flat face, no volume).
    Triangle {
        points: [f32; 9],
        border: f64,
    },
}

/// Sides of the polygon a cylinder or a cone is drawn with by a physics or a view that only takes meshes.
pub const ROUND_SIDES: u32 = 16;

/// Half the side of the slab a physics or a view that only takes boxes or meshes gives a plane, metres.
pub const PLANE_HALF: f64 = 5000.0;

/// Words of a part's header in `Shape::Compound::floats`: code, position, rotation, three numbers, float count,
/// index count.
const PART_HEAD: usize = 13;

/// What is wrong with the data of a triangle mesh, said in a line (`None` when it can be built): no triangle, a half
/// triangle, a vertex that is not a number, an index past the last vertex. The physics asks it before it builds a
/// mesh: parry3d indexes its vertices with what it is given and aborts the whole process on a bad index.
pub fn mesh_problem(vertices: &[f32], indices: &[u32]) -> Option<String> {
    if !vertices.len().is_multiple_of(3) {
        return Some(format!(
            "the vertices are {} numbers, not a multiple of 3",
            vertices.len()
        ));
    }
    if indices.is_empty() {
        return Some("the mesh has no triangle (no indices)".into());
    }
    if !indices.len().is_multiple_of(3) {
        return Some(format!(
            "the indices are {} numbers, not a multiple of 3 (a half triangle)",
            indices.len()
        ));
    }
    if let Some(i) = vertices.iter().position(|v| !v.is_finite()) {
        return Some(format!("vertex number {} is not a finite number", i / 3));
    }
    let points = vertices.len() / 3;
    indices
        .iter()
        .find(|&&i| i as usize >= points)
        .map(|i| format!("index {i} is past the last vertex (the mesh has {points} vertices)"))
}

impl Shape {
    /// The code of the shape on the scalar wires (0 box, 1 sphere, 2 capsule, 3 mesh, 4 convex, 5 compound,
    /// 6 heightfield, 7 plane, 8 cylinder, 9 cone, 10 triangle).
    pub fn code(&self) -> u32 {
        match self {
            Shape::Box { .. } => 0,
            Shape::Sphere { .. } => 1,
            Shape::Capsule { .. } => 2,
            Shape::Mesh { .. } => 3,
            Shape::Convex { .. } => 4,
            Shape::Compound { .. } => 5,
            Shape::HeightField { .. } => 6,
            Shape::Plane { .. } => 7,
            Shape::Cylinder { .. } => 8,
            Shape::Cone { .. } => 9,
            Shape::Triangle { .. } => 10,
        }
    }
    /// The highest code a reader takes ([`Shape::code`]); a code past it is not a shape.
    pub const LAST_CODE: u32 = 10;
    /// The three numbers of a primitive on the scalar wires (box size, sphere radius, capsule radius and height;
    /// cylinder and cone radius, height, border; a hull's and a triangle's border first).
    pub fn numbers(&self) -> V3 {
        match self {
            Shape::Box { size } => *size,
            Shape::Sphere { radius } => [*radius, 0.0, 0.0],
            Shape::Capsule { radius, height } => [*radius, *height, 0.0],
            Shape::HeightField { cols, cell, .. } => [cell[0], cell[1], *cols as f64],
            Shape::Plane { normal } => *normal,
            Shape::Cylinder {
                radius,
                height,
                border,
            }
            | Shape::Cone {
                radius,
                height,
                border,
            } => [*radius, *height, *border],
            Shape::Convex { border, .. } | Shape::Triangle { border, .. } => [*border, 0.0, 0.0],
            _ => [0.0; 3],
        }
    }
    /// What is wrong with the data of a triangle mesh ([`mesh_problem`]); `None` for a mesh that can be built and for
    /// every other shape (they have their own checks).
    pub fn mesh_problem(&self) -> Option<String> {
        match self {
            Shape::Mesh { vertices, indices } => mesh_problem(vertices, indices),
            _ => None,
        }
    }
    /// The float and index streams a mesh, a hull or a compound carries (passed by address on the scalar wires).
    pub fn streams(&self) -> Option<(&[f32], &[u32])> {
        match self {
            Shape::Mesh { vertices, indices }
            | Shape::Convex {
                vertices, indices, ..
            } => Some((vertices, indices)),
            Shape::Compound { floats, indices } => Some((floats, indices)),
            Shape::HeightField { heights, .. } => Some((heights, &[])),
            Shape::Triangle { points, .. } => Some((points, &[])),
            _ => None,
        }
    }
    /// The shape of a code, its three numbers and its streams (the inverse of `code`, `numbers`, `streams`).
    pub fn from_wire(code: u32, n: V3, floats: &[f32], indices: &[u32]) -> Shape {
        match code {
            1 => Shape::Sphere { radius: n[0] },
            2 => Shape::Capsule {
                radius: n[0],
                height: n[1],
            },
            3 => Shape::Mesh {
                vertices: floats.to_vec(),
                indices: indices.to_vec(),
            },
            4 => Shape::Convex {
                vertices: floats.to_vec(),
                indices: indices.to_vec(),
                border: n[0],
            },
            5 => Shape::Compound {
                floats: floats.to_vec(),
                indices: indices.to_vec(),
            },
            6 => Shape::HeightField {
                cols: n[2].max(0.0) as u32,
                cell: [n[0], n[1]],
                heights: floats.to_vec(),
            },
            7 => Shape::Plane { normal: n },
            8 => Shape::Cylinder {
                radius: n[0],
                height: n[1],
                border: n[2],
            },
            9 => Shape::Cone {
                radius: n[0],
                height: n[1],
                border: n[2],
            },
            10 => Shape::Triangle {
                points: std::array::from_fn(|i| floats.get(i).copied().unwrap_or(0.0)),
                border: n[0],
            },
            _ => Shape::Box { size: n },
        }
    }
    /// A compound of parts (`shape`, position and rotation in the body). A compound part is flattened into its own
    /// parts, placed.
    pub fn compound(parts: &[(Shape, V3, Quat)]) -> Shape {
        let (mut floats, mut indices) = (Vec::new(), Vec::new());
        for (shape, p, q) in parts {
            if let Shape::Compound { .. } = shape {
                let inner: Vec<(Shape, V3, Quat)> = shape
                    .parts()
                    .into_iter()
                    .map(|(s, lp, lq)| (s, add3(*p, rotate(*q, lp)), qmul(*q, lq)))
                    .collect();
                if let Shape::Compound {
                    floats: f,
                    indices: i,
                } = Shape::compound(&inner)
                {
                    floats.extend_from_slice(&f);
                    indices.extend_from_slice(&i);
                }
                continue;
            }
            let n = shape.numbers();
            let (sf, si): (&[f32], &[u32]) = shape.streams().unwrap_or((&[], &[]));
            floats.extend_from_slice(&[
                shape.code() as f32,
                p[0] as f32,
                p[1] as f32,
                p[2] as f32,
                q[0] as f32,
                q[1] as f32,
                q[2] as f32,
                q[3] as f32,
                n[0] as f32,
                n[1] as f32,
                n[2] as f32,
                sf.len() as f32,
                si.len() as f32,
            ]);
            floats.extend_from_slice(sf);
            indices.extend_from_slice(si);
        }
        Shape::Compound { floats, indices }
    }
    /// The parts of a compound (the shape alone, at the origin, for any other shape).
    pub fn parts(&self) -> Vec<(Shape, V3, Quat)> {
        let Shape::Compound { floats, indices } = self else {
            return vec![(self.clone(), [0.0; 3], [0.0, 0.0, 0.0, 1.0])];
        };
        let (mut out, mut f, mut i) = (Vec::new(), 0usize, 0usize);
        while f + PART_HEAD <= floats.len() {
            let h = &floats[f..f + PART_HEAD];
            let (nf, ni) = (h[11].max(0.0) as usize, h[12].max(0.0) as usize);
            if f + PART_HEAD + nf > floats.len() || i + ni > indices.len() || h[0] as u32 == 5 {
                break;
            }
            let d = |x: f32| x as f64;
            let shape = Shape::from_wire(
                h[0] as u32,
                [d(h[8]), d(h[9]), d(h[10])],
                &floats[f + PART_HEAD..f + PART_HEAD + nf],
                &indices[i..i + ni],
            );
            out.push((
                shape,
                [d(h[1]), d(h[2]), d(h[3])],
                [d(h[4]), d(h[5]), d(h[6]), d(h[7])],
            ));
            f += PART_HEAD + nf;
            i += ni;
        }
        out
    }
    /// The half height of the shape over its centre (the reach of `grounded()`): 0 for meshes and hulls.
    pub fn half_height(&self) -> f64 {
        match self {
            Shape::Box { size } => size[1] / 2.0,
            Shape::Sphere { radius } => *radius,
            Shape::Capsule { height, .. }
            | Shape::Cylinder { height, .. }
            | Shape::Cone { height, .. } => height / 2.0,
            _ => 0.0,
        }
    }
    /// The shape as triangles in the body's space (vertices xyz, indices), for a physics or a view that only takes
    /// meshes: a box its 12 triangles, a sphere and a capsule a coarse polyhedron, a hull its faces, a cylinder and a
    /// cone a prism and a pyramid of [`ROUND_SIDES`] sides, a triangle itself (both faces), a compound its parts
    /// placed. A border (a rounding) is not drawn: the triangles are of the shape it rounds.
    pub fn triangles(&self) -> (Vec<f32>, Vec<u32>) {
        let (mut v, mut t) = (Vec::new(), Vec::new());
        for (shape, p, q) in self.parts() {
            let base = (v.len() / 3) as u32;
            let (lv, lt) = match &shape {
                Shape::Mesh { vertices, indices }
                | Shape::Convex {
                    vertices, indices, ..
                } => (vertices.clone(), indices.clone()),
                Shape::Cylinder { radius, height, .. } => ring_triangles(*radius, *radius, *height),
                Shape::Cone { radius, height, .. } => ring_triangles(*radius, 0.0, *height),
                Shape::Triangle { points, .. } => (points.to_vec(), vec![0, 1, 2, 0, 2, 1]),
                Shape::Box { size } => box_triangles(*size),
                Shape::Sphere { radius } => round_triangles(*radius, 0.0),
                Shape::Capsule { radius, height } => {
                    round_triangles(*radius, (height / 2.0 - radius).max(0.0))
                }
                Shape::HeightField {
                    cols,
                    cell,
                    heights,
                } => grid_triangles(*cols as usize, *cell, heights),
                Shape::Plane { .. } => {
                    let half = PLANE_HALF as f32;
                    let v = vec![
                        -half, 0., -half, half, 0., -half, -half, 0., half, half, 0., half,
                    ];
                    (v, vec![0, 2, 3, 0, 3, 1])
                }
                Shape::Compound { .. } => continue,
            };
            for c in lv.as_chunks::<3>().0 {
                let w = add3(p, rotate(q, [c[0] as f64, c[1] as f64, c[2] as f64]));
                v.extend_from_slice(&[w[0] as f32, w[1] as f32, w[2] as f32]);
            }
            t.extend(lt.iter().map(|i| i + base));
        }
        (v, t)
    }
}

fn add3(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `q` applied to `v`.
pub fn rotate(q: Quat, v: V3) -> V3 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// `a * b`.
pub fn qmul(a: Quat, b: Quat) -> Quat {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The triangles of a heightfield: its grid, each cell cut along its (x0,z0)–(x1,z1) diagonal as `[a, c, d, a, d, b]`
/// (what a physics that only takes meshes builds; nothing for a grid that is not one).
fn grid_triangles(cols: usize, cell: [f64; 2], heights: &[f32]) -> (Vec<f32>, Vec<u32>) {
    let rows = heights.len().checked_div(cols).unwrap_or(0);
    if cols < 2 || rows < 2 {
        return (Vec::new(), Vec::new());
    }
    let mut v = Vec::with_capacity(cols * rows * 3);
    for j in 0..rows {
        for i in 0..cols {
            let (x, z) = (i as f64 * cell[0], j as f64 * cell[1]);
            v.extend_from_slice(&[x as f32, heights[j * cols + i], z as f32]);
        }
    }
    let mut t = Vec::with_capacity((cols - 1) * (rows - 1) * 6);
    for j in 0..rows - 1 {
        for i in 0..cols - 1 {
            let a = (j * cols + i) as u32;
            let (b, c) = (a + 1, a + cols as u32);
            t.extend([a, c, c + 1, a, c + 1, b]);
        }
    }
    (v, t)
}

/// A prism (`top` = `bottom`: a cylinder) or a pyramid (`top` = 0: a cone) of [`ROUND_SIDES`] sides along Y, centred,
/// `height` tall, its bottom ring of `bottom`, its top ring of `top` (a point when 0), both ends closed.
fn ring_triangles(bottom: f64, top: f64, height: f64) -> (Vec<f32>, Vec<u32>) {
    let n = ROUND_SIDES;
    let h = (height / 2.0) as f32;
    let mut v = Vec::with_capacity((2 * n as usize + 2) * 3);
    for (r, y) in [(bottom, -h), (top, h)] {
        for s in 0..n {
            let a = std::f64::consts::TAU * s as f64 / n as f64;
            v.extend_from_slice(&[(r * a.cos()) as f32, y, (r * a.sin()) as f32]);
        }
    }
    // The centres of both ends.
    let (cb, ct) = (2 * n, 2 * n + 1);
    v.extend_from_slice(&[0.0, -h, 0.0, 0.0, h, 0.0]);
    let mut t = Vec::with_capacity(n as usize * 12);
    for s in 0..n {
        let (a, b) = (s, (s + 1) % n);
        let (c, d) = (a + n, b + n);
        t.extend_from_slice(&[a, c, b, b, c, d, cb, a, b, ct, d, c]);
    }
    (v, t)
}

fn box_triangles(size: V3) -> (Vec<f32>, Vec<u32>) {
    let h = [
        size[0] as f32 / 2.0,
        size[1] as f32 / 2.0,
        size[2] as f32 / 2.0,
    ];
    let mut v = Vec::with_capacity(24);
    for i in 0..8 {
        v.extend_from_slice(&[
            if i & 1 == 0 { -h[0] } else { h[0] },
            if i & 2 == 0 { -h[1] } else { h[1] },
            if i & 4 == 0 { -h[2] } else { h[2] },
        ]);
    }
    let t = vec![
        0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6,
        1, 3, 5, 3, 7, 5,
    ];
    (v, t)
}

/// A sphere of `radius` (8 sides, 4 rings per half), its halves `half` apart along Y (a capsule's segment).
fn round_triangles(radius: f64, half: f64) -> (Vec<f32>, Vec<u32>) {
    const SIDES: u32 = 8;
    const RINGS: u32 = 4;
    let mut v = Vec::new();
    // Rings from the top pole to the bottom one; the upper half is raised by `half`, the lower lowered.
    let rows = 2 * RINGS + 1;
    for r in 0..rows {
        let a = std::f64::consts::PI * r as f64 / (rows - 1) as f64;
        let (y, ring) = (radius * a.cos(), radius * a.sin());
        let lift = if r < RINGS {
            half
        } else if r > RINGS {
            -half
        } else {
            0.0
        };
        for s in 0..SIDES {
            let b = std::f64::consts::TAU * s as f64 / SIDES as f64;
            v.extend_from_slice(&[
                (ring * b.cos()) as f32,
                (y + lift) as f32,
                (ring * b.sin()) as f32,
            ]);
        }
    }
    // The equator ring is doubled for a capsule: its two halves joined by a band.
    let mut t = Vec::new();
    for r in 0..rows - 1 {
        for s in 0..SIDES {
            let (a, b) = (r * SIDES + s, r * SIDES + (s + 1) % SIDES);
            let (c, d) = (a + SIDES, b + SIDES);
            t.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }
    let _ = half;
    (v, t)
}
