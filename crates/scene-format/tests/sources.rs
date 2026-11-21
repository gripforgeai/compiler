//! `sources` of a project (the Rust files of the game's code, declared by the project: crate::sources): accepted by
//! the tolerant validator, the same here as in the compiler, and never packed into the game the players load (the code
//! is compiled into game.main, or not run in the development mode).

use gripforge_scene_format::{project::pack, project::validate_project, Registry};
use serde_json::{json, Value as Json};

fn project(sources: Json) -> Json {
    json!({ "id": "arena", "sources": sources, "requires": ["game.engine"],
        "installed": [{ "id": "game.engine", "version": "1.0.0", "config": { "startScene": "level" } }],
        "data": { "scenes": [{ "id": "level", "role": "level", "requires": ["game.engine"],
            "nodes": [{ "name": "Spawn", "type": "entry" }] }] } })
}

fn codes(p: &Json) -> Vec<&'static str> {
    validate_project(p, &|_| None, &Registry::base())
        .iter()
        .map(|i| i.code)
        .collect()
}

#[test]
fn declared_sources_are_a_known_key_checked_and_not_packed() {
    let p = project(json!(["rules.rs", "code/weapons.rs"]));
    assert!(codes(&p).is_empty(), "{:?}", codes(&p));
    let game = pack(&p, &|_| None, &Registry::base(), &|_| None).expect("packed");
    assert!(
        game["project"].get("sources").is_none(),
        "{}",
        game["project"]
    );
    assert_eq!(codes(&project(json!(["../x.rs"]))), ["source_outside"]);
    assert_eq!(
        codes(&project(json!(["a.rs", "a.rs"]))),
        ["source_duplicate"]
    );
    assert_eq!(codes(&project(json!(["a.txt"]))), ["source_not_rust"]);
    assert_eq!(codes(&project(json!({ "a": 1 }))), ["sources_invalid"]);
}

/// The digest of the game's code this crate names (`sources_digest`, the re-export of gripforge-sources-digest) gives
/// the shared values of that crate (the page's binding checks the same).
#[test]
fn the_digest_of_the_sources_is_the_one_of_the_shared_values() {
    let cases: Json = serde_json::from_str(gripforge_scene_format::sources_digest::VALUES).unwrap();
    for c in cases["cases"].as_array().unwrap() {
        let files: Vec<(String, Vec<u8>)> = c["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap().to_string(),
                    f["text"].as_str().unwrap().as_bytes().to_vec(),
                )
            })
            .collect();
        let pairs: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(p, b)| (p.as_str(), b.as_slice()))
            .collect();
        let d = gripforge_scene_format::sources_digest::digest(&pairs);
        assert_eq!(
            std::str::from_utf8(&d).unwrap(),
            c["digest"].as_str().unwrap(),
            "{c}"
        );
    }
    // A const fn: what the crate of game.main computes at compile time.
    const AT_COMPILE: [u8; 24] = gripforge_scene_format::sources_digest::digest(&[("ab.rs", b"c")]);
    assert_eq!(&AT_COMPILE, b"fnv1a64:26edf0643d65c4a9");
}

/// The compiled module of a project with code joins the packed game last in the lock (its id given by the module); the
/// development mode refuses the project without it (`game_main_missing`) or with a module of other files (`game_main_stale`).
#[test]
fn the_development_mode_plays_a_project_with_code_only_with_its_compiled_module() {
    use gripforge_scene_format::sources::{dev_module_issue, digest_of, with_game_module};
    let p = project(json!(["rules.rs", "./code/weapons.rs"]));
    let mut game = pack(&p, &|_| None, &Registry::base(), &|_| None).expect("packed");
    assert_eq!(game["lock"]["order"], json!(["game.engine"]));
    with_game_module(&mut game, "game.main", "0.1.0");
    with_game_module(&mut game, "game.main", "0.1.0");
    assert_eq!(game["lock"]["order"], json!(["game.engine", "game.main"]));
    assert_eq!(game["lock"]["kits"]["game.main"]["enabled"], json!(true));
    assert_eq!(game["project"]["installed"][1]["id"], json!("game.main"));

    let disk = |path: &str| match path {
        "rules.rs" => Some(b"pub struct G;\n".to_vec()),
        "code/weapons.rs" => Some("// é\nfn x() {}\n".as_bytes().to_vec()),
        _ => None,
    };
    let now = digest_of(&p, &disk).unwrap();
    assert_eq!(now, "fnv1a64:0d990bb6333e6744");
    assert!(dev_module_issue(&p, Some(&now), &disk).is_none());
    assert_eq!(
        dev_module_issue(&p, None, &disk).unwrap().code,
        "game_main_missing"
    );
    assert_eq!(
        dev_module_issue(&p, Some("fnv1a64:0000000000000000"), &disk)
            .unwrap()
            .code,
        "game_main_stale"
    );
    assert_eq!(
        dev_module_issue(&p, Some(&now), &|_| None).unwrap().code,
        "source_missing"
    );
    assert!(dev_module_issue(&project(json!([])), None, &disk).is_none());
}
