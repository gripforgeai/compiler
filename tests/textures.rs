//! `--compress-textures` against a local simulated server (no real network, no real compression).

use base64::Engine;
use gripforge_compiler::build::{build, BuildOptions};
use gripforge_compiler::cli::parse;
use gripforge_compiler::textures::{has_basisu, DEFAULT_URL};
use serde_json::{json, Value as Json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn glb(json: &Json) -> Vec<u8> {
    let mut j = json.to_string().into_bytes();
    while !j.len().is_multiple_of(4) {
        j.push(b' ');
    }
    let mut v = b"glTF".to_vec();
    v.extend(2u32.to_le_bytes());
    v.extend(((12 + 8 + j.len()) as u32).to_le_bytes());
    v.extend((j.len() as u32).to_le_bytes());
    v.extend(b"JSON");
    v.extend(j);
    v
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// A server answering `status` + `body` to every request; counts them and keeps the last body received.
fn server(status: u16, body: String) -> (String, Arc<AtomicUsize>, Arc<std::sync::Mutex<String>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/api/v1/public/textures/compress",
        l.local_addr().unwrap()
    );
    let n = Arc::new(AtomicUsize::new(0));
    let last = Arc::new(std::sync::Mutex::new(String::new()));
    let (n2, last2) = (n.clone(), last.clone());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { break };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 8192];
            let (head_end, len) = loop {
                let k = s.read(&mut tmp).unwrap_or(0);
                buf.extend_from_slice(&tmp[..k]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let h = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                    let len = h
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map(|v| v.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    break (i + 4, len);
                }
                if k == 0 {
                    return;
                }
            };
            while buf.len() < head_end + len {
                let k = s.read(&mut tmp).unwrap_or(0);
                if k == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..k]);
            }
            *last2.lock().unwrap() = String::from_utf8_lossy(&buf[head_end..]).into_owned();
            n2.fetch_add(1, Ordering::SeqCst);
            let _ = write!(s, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        }
    });
    (url, n, last)
}

fn project() -> (PathBuf, PathBuf) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "gfc-tex-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    let room = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/room");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::copy(room.join("project.gripforge"), d.join("project.gripforge")).unwrap();
    (d, room.join("kits"))
}

fn run(
    d: &Path,
    kits: &Path,
    url: Option<&str>,
    on: bool,
) -> gripforge_compiler::build::BuildResult {
    build(&BuildOptions {
        project: Some(d.to_path_buf()),
        kits: Some(kits.to_path_buf()),
        sdk: Some(d.join("sdk")),
        compress_textures: on,
        compress_textures_url: url.map(String::from),
        ..Default::default()
    })
}

#[test]
fn detects_basisu() {
    assert!(has_basisu(&glb(
        &json!({"extensionsUsed": ["KHR_texture_basisu"]})
    )));
    assert!(!has_basisu(&glb(&json!({"asset": {"version": "2.0"}}))));
    assert!(!has_basisu(b"not a glb"));
}

#[test]
fn command_line() {
    let a = parse(&[
        "--compress-textures".into(),
        "--compress-textures-url".into(),
        "http://x/y".into(),
    ])
    .unwrap();
    assert!(a.compress_textures);
    assert_eq!(a.compress_textures_url.as_deref(), Some("http://x/y"));
    let a = parse(&[
        "--compress-textures".into(),
        "--no-compress-textures".into(),
    ])
    .unwrap();
    assert!(!a.compress_textures);
    assert!(DEFAULT_URL.ends_with("/api/v1/public/textures/compress"));
}

#[test]
fn compresses_writes_a_copy_and_caches() {
    let (d, kits) = project();
    std::fs::create_dir_all(d.join("assets")).unwrap();
    let orig = glb(&json!({"asset": {"version": "2.0"}}));
    let done = glb(&json!({"asset": {"version": "2.0"}, "extensionsUsed": ["KHR_texture_basisu"]}));
    std::fs::write(d.join("assets/a.glb"), &orig).unwrap();
    std::fs::write(
        d.join("assets/b.glb"),
        glb(&json!({"extensionsUsed": ["KHR_texture_basisu"]})),
    )
    .unwrap();
    let body = json!({"applied": true, "glbBase64": b64(&done), "stats": {"bytesBefore": 1000, "bytesAfter": 400, "gpuBytesBefore": 4000, "gpuBytesAfter": 1000}}).to_string();
    let (url, n, last) = server(200, body);
    let r = run(&d, &kits, Some(&url), true);
    assert!(r.ok, "{:?}", r.issues);
    assert_eq!(
        n.load(Ordering::SeqCst),
        1,
        "b.glb is already compressed: only a.glb is sent"
    );
    assert!(last.lock().unwrap().contains(&b64(&orig)));
    let t = r.textures.clone().unwrap();
    assert_eq!(t["compressed"], 1);
    assert_eq!(t["alreadyCompressed"], 1);
    assert_eq!(t["gpuBytesBefore"], 4000);
    assert_eq!(t["gpuBytesAfter"], 1000);
    let out = d.join(".build/textures/assets/a.glb.ktx2.glb");
    assert_eq!(std::fs::read(out).unwrap(), done);
    assert_eq!(
        std::fs::read(d.join("assets/a.glb")).unwrap(),
        orig,
        "the original is never touched"
    );
    let rep: Json =
        serde_json::from_str(&std::fs::read_to_string(d.join(".build/build.json")).unwrap())
            .unwrap();
    assert_eq!(rep["textures"]["compressed"], 1);
    // second build: from the local cache, no request
    let r = run(&d, &kits, Some(&url), true);
    assert!(r.ok);
    assert_eq!(n.load(Ordering::SeqCst), 1);
    assert_eq!(r.textures.unwrap()["cached"], 1);
}

#[test]
fn failures_are_warnings_and_the_build_goes_on() {
    let (d, kits) = project();
    std::fs::write(d.join("a.glb"), glb(&json!({}))).unwrap();
    for (status, body, code) in [
        (503, "{}".to_string(), "textures_service_failed"),
        (200, json!({"applied": false, "warnings": [{"code": "x", "message": "no textures", "howToFix": "add some"}]}).to_string(), "textures_not_applied"),
    ] {
        let (url, _, _) = server(status, body);
        let r = run(&d, &kits, Some(&url), true);
        assert!(r.ok, "{:?}", r.issues);
        assert!(r.issues.iter().any(|i| i["level"] == "warning" && i["code"] == code), "{:?}", r.issues);
    }
    // nothing listens
    let r = run(&d, &kits, Some("http://127.0.0.1:1/x"), true);
    assert!(r.ok);
    assert!(r
        .issues
        .iter()
        .any(|i| i["code"] == "textures_service_failed"));
}

#[test]
fn off_by_default_sends_nothing() {
    let (d, kits) = project();
    std::fs::write(d.join("a.glb"), glb(&json!({}))).unwrap();
    let (url, n, _) = server(200, "{}".into());
    let r = run(&d, &kits, Some(&url), false);
    assert!(r.ok);
    assert_eq!(n.load(Ordering::SeqCst), 0);
    assert!(r.textures.is_none());
}
