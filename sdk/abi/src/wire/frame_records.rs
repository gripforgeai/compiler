//! The fixed parts of a wire frame (wire/frame.rs), by their contracts (core/abi/contracts/abi.wire.*.json, positions
//! generated): the header — its first words the contract's id and major, what tells a frame from the WORLD header
//! (magic `GFW1`) and refuses a writer of another major —, the head of a section, the records of TRANSFORM and
//! VELOCITY. History of the header (also in the contracts' `history`):
//! - the pose carries a quaternion (docs/design/pose-quaternion.md, #1739): a TRANSFORM record is x, y, z, qx, qy, qz,
//!   qw (32 bytes) instead of x, y, z, yaw (20 bytes). Rebuild the player (web and native) and the kit host.
//! - #1865: the magic `GFW1` and the version (1, compared for equality) give way to the contract's id and major.

/// The header of a frame (contract `abi.wire.frame`).
pub use gripforge_contracts::abi_wire_frame as header;
/// The head of a section (contract `abi.wire.section`).
pub use gripforge_contracts::abi_wire_section as section;
/// A record of TRANSFORM (contract `abi.wire.transform`).
pub use gripforge_contracts::abi_wire_transform as transform;
/// A record of VELOCITY (contract `abi.wire.velocity`).
pub use gripforge_contracts::abi_wire_velocity as velocity;
/// The bytes of the header (`abi.wire.frame`, computed by the tool).
pub const HEADER: usize = header::SIZE;

/// The fields of a header but its contract and major.
pub struct Head {
    pub flags: u16,
    pub seq: u32,
    pub length: u32,
    pub tick: u64,
    pub elapsed: f64,
    pub render_time: f64,
    pub entities: u32,
    pub sections: u32,
}

/// The bytes of a header, its contract and major those of this build.
pub fn header_bytes(h: &Head) -> [u8; HEADER] {
    let (flags, seq, length, tick) = (h.flags, h.seq, h.length, h.tick);
    let (elapsed, render_time, entities, sections) =
        (h.elapsed, h.render_time, h.entities, h.sections);
    let (contract, major) = (header::ID, header::MAJOR);
    let row = header::Row {
        contract,
        major,
        flags,
        seq,
        length,
        tick,
        elapsed,
        render_time,
        entities,
        sections,
    };
    let mut out = [0u8; HEADER];
    out.copy_from_slice(header::as_bytes(&row));
    out
}

/// The header of `bytes`: refused when it is too short, of another contract or of another major.
pub fn read_header(bytes: &[u8]) -> Result<Head, String> {
    // SAFETY: `bytes` holds its length while `h` is read (this call).
    let h = unsafe { header::Record::bind(bytes.as_ptr(), bytes.len()) }
        .map_err(|_| "wire frame: bad header".to_string())?;
    if h.contract() != header::ID {
        return Err("wire frame: bad contract (not a frame: abi.wire.frame)".into());
    }
    if h.major() != header::MAJOR {
        let (theirs, ours) = (h.major(), header::MAJOR);
        return Err(format!("contract_refused: abi.wire.frame — the writer has major {theirs}, this reader {ours}: rebuild the reader or the writer"));
    }
    let (flags, seq, length, tick) = (h.flags(), h.seq(), h.length(), h.tick());
    let (elapsed, render_time, entities, sections) =
        (h.elapsed(), h.render_time(), h.entities(), h.sections());
    Ok(Head {
        flags,
        seq,
        length,
        tick,
        elapsed,
        render_time,
        entities,
        sections,
    })
}

/// Writes the head of a section of `length` bytes of body.
pub fn push_section(out: &mut Vec<u8>, kind: u16, length: u32) {
    let row = section::Row {
        kind,
        reserved: 0,
        length,
    };
    out.extend_from_slice(section::as_bytes(&row));
}

/// The kind and the body's bytes of the section at `at`.
pub fn section_at(bytes: &[u8], at: usize) -> Option<(u16, usize)> {
    let head = bytes.get(at..at + section::SIZE)?;
    // SAFETY: `head` holds `section::SIZE` bytes for this read.
    let s = unsafe { section::Record::bind(head.as_ptr(), head.len()) }.ok()?;
    Some((s.kind(), s.length() as usize))
}

/// Writes a record of TRANSFORM: x, y, z, qx, qy, qz, qw.
pub fn push_transform(body: &mut Vec<u8>, slot: u32, t: [f32; 7]) {
    let (position, rotation) = ([t[0], t[1], t[2]], [t[3], t[4], t[5], t[6]]);
    body.extend_from_slice(transform::as_bytes(&transform::Row {
        slot,
        position,
        rotation,
    }));
}

/// A record of TRANSFORM: its slot and x, y, z, qx, qy, qz, qw.
pub fn transform_of(b: &[u8]) -> Option<(usize, [f32; 7])> {
    // SAFETY: `b` holds the record for this read.
    let r = unsafe { transform::Record::bind(b.as_ptr(), b.len()) }.ok()?;
    let ([x, y, z], [qx, qy, qz, qw]) = (r.position(), r.rotation());
    Some((r.slot() as usize, [x, y, z, qx, qy, qz, qw]))
}

/// Writes a record of VELOCITY.
pub fn push_velocity(body: &mut Vec<u8>, slot: u32, velocity: [f32; 3]) {
    body.extend_from_slice(velocity::as_bytes(&velocity::Row { slot, velocity }));
}

/// A record of VELOCITY: its slot and velocity.
pub fn velocity_of(b: &[u8]) -> Option<(usize, [f32; 3])> {
    // SAFETY: `b` holds the record for this read.
    let r = unsafe { velocity::Record::bind(b.as_ptr(), b.len()) }.ok()?;
    Some((r.slot() as usize, r.velocity()))
}
