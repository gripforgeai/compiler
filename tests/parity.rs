//! The parity with the development mode of the kits repository: each case of tests/parity (copied with its expected
//! resolution from module-kits tools/parity/cases by tools/sync-scene-format) read by `gripforge-build --resolve` gives
//! the project and the resolution issues the kits repository's reference reader wrote, compared canonically (keys
//! sorted, whole numbers without a fraction). The full corpus (every `.gripforge` of the kits repository, with its
//! kits) is compared by the CI of module-kits (tools/parity/parity.mjs --compiler).

use gripforge_compiler::cli::main as cli;
use serde_json::{Map, Value as Json};
use std::path::Path;

/// `v` with the keys of every object sorted.
fn sorted(v: &Json) -> Json {
    match v {
        Json::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            Json::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), sorted(&o[k])))
                    .collect::<Map<_, _>>(),
            )
        }
        Json::Array(a) => Json::Array(a.iter().map(sorted).collect()),
        _ => v.clone(),
    }
}

fn canonical(v: &Json) -> String {
    gripforge_compiler::json::pretty(&sorted(v))
}

#[test]
fn every_case_of_the_kits_repository_resolves_as_its_reference_reader_does() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let list: Json = serde_json::from_str(
        &std::fs::read_to_string(root.join("tests/parity/cases.json")).unwrap(),
    )
    .unwrap();
    let cases = list["cases"].as_array().expect("cases");
    assert!(!cases.is_empty());
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let project = root.join(c["project"].as_str().unwrap());
        let mut text = String::new();
        let code = cli(
            &[
                project.to_string_lossy().into_owned(),
                "--resolve".into(),
                "--kits-path".into(),
                root.join("examples/room/kits").to_string_lossy().into_owned(),
            ],
            &mut |s| text.push_str(s),
            &mut |_| {},
        );
        let got: Json = serde_json::from_str(&text).unwrap();
        assert!(code <= 1, "{name}: exit {code}");
        let mut mine = Map::new();
        mine.insert("project".into(), got["project"].clone());
        mine.insert("resolveIssues".into(), got["resolveIssues"].clone());
        let expected: Json = serde_json::from_str(
            &std::fs::read_to_string(root.join(format!("tests/parity/{name}.expected.json")))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            canonical(&Json::Object(mine)),
            canonical(&expected),
            "{name}: gripforge-build --resolve differs from the reference reader of the kits repository"
        );
    }
}
