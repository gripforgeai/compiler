//! The section written at compile time, read back, and the rules of the check (the vectors shared with the web host
//! are in tests/contracts_vectors.rs).
use super::*;

/// One entry as the generated crate writes it (`DECL`, role 0): a field is `(name, type, offset, size, default)`.
fn decl(
    name: &str,
    major: u16,
    head: &[(&str, u8, u16, u16, bool)],
    row: &[(&str, u8, u16, u16, bool)],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&id_of(name).to_le_bytes());
    out.extend_from_slice(&major.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(head.len() as u16).to_le_bytes());
    out.extend_from_slice(&(row.len() as u16).to_le_bytes());
    out.push(name.len() as u8);
    out.extend_from_slice(name.as_bytes());
    for (n, ty, offset, size, def) in head.iter().chain(row) {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&[*ty, u8::from(*def), n.len() as u8]);
        out.extend_from_slice(n.as_bytes());
    }
    out
}

const ROW4: [(&str, u8, u16, u16, bool); 4] = [
    ("flags", 3, 0, 4, false),
    ("stamp", 3, 4, 4, false),
    ("p", 11, 8, 24, false),
    ("rev", 3, 32, 4, true),
];
const HEAD: [(&str, u8, u16, u16, bool); 2] = [("seq", 3, 32, 4, false), ("time", 8, 40, 8, false)];

// What `contracts_section!` expands to, at compile time, for one contract read.
const PING: &[u8] = &[
    0x34, 0x12, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 4, b'p', b'i', b'n', b'g', 0, 0, 4, 0, 3, 0, 1, b'n',
];
const ENTRIES: &[(&[u8], u8)] = &[(PING, READ)];
const LEN: usize = section_len(ENTRIES);
static SECTION_BYTES: [u8; LEN] = section::<LEN>(ENTRIES);

#[test]
fn the_section_written_at_compile_time_is_read_back() {
    let d = read(&SECTION_BYTES).unwrap();
    assert_eq!(&SECTION_BYTES[..4], b"GFCS");
    assert_eq!(d.len(), 1);
    assert_eq!(
        (d[0].id, d[0].major, d[0].role, d[0].name.as_str()),
        (0x1234, 1, READ, "ping")
    );
    assert_eq!(
        d[0].row,
        vec![Field {
            offset: 0,
            size: 4,
            ty: 3,
            default: false,
            name: "n".into()
        }]
    );
    assert!(read(&SECTION_BYTES[..LEN - 1])
        .unwrap_err()
        .contains("cut short"));
    assert_eq!(read(&section::<8>(&[])).unwrap(), vec![]);
}

#[test]
fn the_bytes_are_those_of_the_web_host() {
    // The section of `kit.reader` in the vector `minor_up_loads` (written by core/gateway-web/contracts.mjs).
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("../tests/contracts-vectors.json")).unwrap();
    let case = doc["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "minor_up_loads")
        .unwrap();
    let hex = case["steps"][1]["hex"].as_str().unwrap();
    let mine = decl("abi.physics.bodies", 1, &HEAD, &ROW4);
    let mut want = b"GFCS".to_vec();
    want.extend_from_slice(&[1, 0, 1, 0]);
    want.extend_from_slice(&mine);
    want[8 + ROLE_AT] = READ;
    let got: String = want.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(got, hex);
    assert_eq!(
        id_of("abi.physics.bodies"),
        u32::from_le_bytes(mine[..4].try_into().unwrap())
    );
}

fn one(role: u8, major: u16, row: &[(&str, u8, u16, u16, bool)]) -> Vec<Decl> {
    let mut s = b"GFCS".to_vec();
    s.extend_from_slice(&[1, 0, 1, 0]);
    let mut e = decl("abi.physics.bodies", major, &HEAD, row);
    e[7] = role;
    s.extend_from_slice(&e);
    read(&s).unwrap()
}

#[test]
fn a_minor_raised_loads_a_major_changed_is_refused_by_name() {
    let mut more = ROW4.to_vec();
    more.push(("contacts", 3, 36, 4, true));
    let mut reg = Registry::default();
    reg.admit("host", one(WRITE, 1, &more)).unwrap();
    reg.admit("old.reader", one(READ, 1, &ROW4)).unwrap();
    let why = reg.admit("other.reader", one(READ, 2, &ROW4)).unwrap_err();
    assert!(
        why.starts_with("contract_refused: other.reader reads abi.physics.bodies 2.4"),
        "{why}"
    );
    assert!(why.contains("another major"), "{why}");
    // Refused means not admitted: the registry still has two modules.
    assert_eq!(reg.loaded().len(), 2);
    // The same module again, same declarations (a second thread): accepted.
    reg.admit("old.reader", one(READ, 1, &ROW4)).unwrap();
}

#[test]
fn a_module_that_does_not_declare_the_contract_loads() {
    let mut reg = Registry::default();
    reg.admit("host", one(WRITE, 2, &ROW4)).unwrap();
    reg.admit("kit.none", Vec::new()).unwrap();
    assert_eq!(type_name(14), "str");
}
