//! The digest gives the shared values, at run time and at compile time, and SOURCE is the function's own file.

use serde_json::Value as Json;

#[test]
fn the_digest_gives_the_shared_values() {
    let cases: Json = serde_json::from_str(gripforge_sources_digest::VALUES).unwrap();
    let cases = cases["cases"].as_array().unwrap();
    assert!(cases.len() >= 7);
    for c in cases {
        let files: Vec<(&str, &[u8])> = c["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap(),
                    f["text"].as_str().unwrap().as_bytes(),
                )
            })
            .collect();
        let d = gripforge_sources_digest::digest(&files);
        assert_eq!(
            std::str::from_utf8(&d).unwrap(),
            c["digest"].as_str().unwrap(),
            "{c}"
        );
    }
}

#[test]
fn the_digest_is_computed_at_compile_time() {
    const AT_COMPILE: [u8; gripforge_sources_digest::DIGEST_LEN] =
        gripforge_sources_digest::digest(&[("ab.rs", b"c")]);
    assert_eq!(&AT_COMPILE, b"fnv1a64:26edf0643d65c4a9");
}

/// What gripforge-build writes into game.main stands alone: no `use`, nothing outside the file.
#[test]
fn the_source_is_the_file_of_the_function_and_stands_alone() {
    let file = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sources_digest.rs"
    ))
    .unwrap();
    assert_eq!(gripforge_sources_digest::SOURCE, file);
    assert!(
        !file.lines().any(|l| l.trim_start().starts_with("use ")),
        "{file}"
    );
    assert!(file.contains("pub const fn digest("));
}
