//! `--compress-textures`: the GLB of the project without a compressed copy are sent to the public texture route of
//! GripForge (no key), and the compressed copy is written in `<out>/textures/`. This module only CALLS the API: the
//! compression itself (basisu, KTX2) is done by the service, nothing of it lives in this repository.
//!
//!   <out>/textures/<path of the glb>.ktx2.glb      the compressed copy (the original is never touched)
//!   <out>/textures/.cache/<sha256 of the glb>.glb   the same copy, kept by the hash of the original: a GLB already
//!                                                   sent is not sent again
//!
//! A GLB that already declares `KHR_texture_basisu` is left alone. A failure (network, server, `applied:false`) is a
//! warning (`textures_*`): the build goes on without the copy.

use base64::Engine;
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_URL: &str = "https://gripforge.ai/api/v1/public/textures/compress";
/// The route refuses more (413): not even sent.
const MAX_BYTES: usize = 32 * 1024 * 1024;

/// The GLB of a project folder (without the build folders, hidden folders and the copies already made).
pub fn find_glbs(dir: &Path, skip: &Path) -> Vec<PathBuf> {
    fn walk(d: &Path, skip: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut es: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        es.sort();
        for p in es {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if p.is_dir() {
                if name.starts_with('.')
                    || name == "target"
                    || name == "node_modules"
                    || p.starts_with(skip)
                {
                    continue;
                }
                walk(&p, skip, out);
            } else if name.to_lowercase().ends_with(".glb")
                && !name.to_lowercase().ends_with(".ktx2.glb")
            {
                out.push(p);
            }
        }
    }
    let mut v = Vec::new();
    walk(dir, skip, &mut v);
    v
}

/// Does the JSON chunk of this GLB name `KHR_texture_basisu` (used or required)?
pub fn has_basisu(glb: &[u8]) -> bool {
    if glb.len() < 20 || &glb[0..4] != b"glTF" {
        return false;
    }
    let len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
    if &glb[16..20] != b"JSON" || glb.len() < 20 + len {
        return false;
    }
    let Ok(j) = serde_json::from_slice::<Json>(&glb[20..20 + len]) else {
        return false;
    };
    ["extensionsUsed", "extensionsRequired"].iter().any(|k| {
        j[k].as_array()
            .is_some_and(|a| a.iter().any(|e| e == "KHR_texture_basisu"))
    })
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn warn(code: &str, file: &Path, message: String, fix: &str) -> Json {
    json!({ "level": "warning", "code": code, "file": file.to_string_lossy(), "message": message, "fix": fix })
}

/// Asks the route for the compressed copy: Ok(response `value`) or a message.
fn call(url: &str, glb: &[u8]) -> Result<Json, String> {
    let body = json!({ "glbBase64": base64::engine::general_purpose::STANDARD.encode(glb) });
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build();
    match agent
        .post(url)
        .set("content-type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(r) => {
            use std::io::Read;
            let mut text = Vec::new();
            r.into_reader()
                .take(128 * 1024 * 1024)
                .read_to_end(&mut text)
                .map_err(|e| format!("the answer cannot be read: {e}"))?;
            serde_json::from_slice::<Json>(&text)
                .map_err(|e| format!("the answer is not JSON: {e}"))
        }
        Err(ureq::Error::Status(code, r)) => {
            let wait = r
                .header("retry-after")
                .map(|w| format!(", retry after {w} s"))
                .unwrap_or_default();
            Err(format!("the service answered HTTP {code}{wait}"))
        }
        Err(e) => Err(format!("the service cannot be reached: {e}")),
    }
}

/// Compresses what is missing. Answers the report (`gripforge.texture-compress/1`) and the warnings.
pub fn compress(project_dir: &Path, out: &Path, url: &str) -> (Json, Vec<Json>) {
    let tex = out.join("textures");
    let cache = tex.join(".cache");
    let (mut files, mut issues) = (Vec::new(), Vec::new());
    let (mut fb, mut fa, mut gb, mut ga) = (0u64, 0u64, 0u64, 0u64);
    for p in find_glbs(project_dir, out) {
        let rel = p
            .strip_prefix(project_dir)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        if has_basisu(&bytes) {
            files.push(json!({ "file": rel, "status": "already_compressed" }));
            continue;
        }
        let sha = sha_hex(&bytes);
        let cached = cache.join(format!("{sha}.glb"));
        let dest = tex.join(format!("{rel}.ktx2.glb"));
        let write = |data: &[u8]| -> std::io::Result<()> {
            if let Some(d) = dest.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&dest, data)
        };
        if let Ok(c) = std::fs::read(&cached) {
            if write(&c).is_ok() {
                files.push(json!({ "file": rel, "status": "cached", "copy": dest.to_string_lossy(), "sha256": sha }));
                continue;
            }
        }
        if bytes.len() > MAX_BYTES {
            issues.push(warn("textures_too_big", &p, format!("{rel} is {} bytes: the service takes {MAX_BYTES} at most, so it is not compressed", bytes.len()), "reduce the textures of this GLB, or leave it as it is"));
            files.push(json!({ "file": rel, "status": "skipped" }));
            continue;
        }
        let v = match call(url, &bytes) {
            Ok(v) => v,
            Err(m) => {
                issues.push(warn(
                    "textures_service_failed",
                    &p,
                    format!("{rel} is not compressed: {m} ({url})"),
                    "build again later, or build without --compress-textures",
                ));
                files.push(json!({ "file": rel, "status": "failed" }));
                continue;
            }
        };
        let copy = if v["applied"] == true {
            v["glbBase64"]
                .as_str()
                .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
        } else {
            None
        };
        let Some(copy) = copy else {
            let why: Vec<String> = v["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|w| {
                    format!(
                        "{}{}",
                        w["message"].as_str().unwrap_or(&w["code"].to_string()),
                        w["howToFix"]
                            .as_str()
                            .map(|h| format!(" ({h})"))
                            .unwrap_or_default()
                    )
                })
                .collect();
            let why = if why.is_empty() {
                "no reason given".to_string()
            } else {
                why.join("; ")
            };
            issues.push(warn(
                "textures_not_applied",
                &p,
                format!("{rel} is not compressed: {why}"),
                "the build goes on with the original GLB",
            ));
            files.push(json!({ "file": rel, "status": "not_applied" }));
            continue;
        };
        let saved = std::fs::create_dir_all(&cache)
            .and_then(|_| std::fs::write(&cached, &copy))
            .and_then(|_| write(&copy));
        if let Err(e) = saved {
            issues.push(warn(
                "textures_write_failed",
                &dest,
                format!("the copy of {rel} cannot be written: {e}"),
                "check the folder given by --out and its rights",
            ));
            files.push(json!({ "file": rel, "status": "failed" }));
            continue;
        }
        let s = &v["stats"];
        let n = |k: &str| s[k].as_u64();
        let (b0, b1) = (
            n("bytesBefore").unwrap_or(bytes.len() as u64),
            n("bytesAfter").unwrap_or(copy.len() as u64),
        );
        let (g0, g1) = (
            n("gpuBytesBefore").unwrap_or(0),
            n("gpuBytesAfter").unwrap_or(0),
        );
        fb += b0;
        fa += b1;
        gb += g0;
        ga += g1;
        files.push(json!({ "file": rel, "status": "compressed", "copy": dest.to_string_lossy(), "sha256": sha,
            "bytesBefore": b0, "bytesAfter": b1, "gpuBytesBefore": g0, "gpuBytesAfter": g1 }));
    }
    let count = |st: &str| files.iter().filter(|f| f["status"] == st).count();
    let report = json!({ "schema": "gripforge.texture-compress/1", "url": url, "compressed": count("compressed"),
        "cached": count("cached"), "alreadyCompressed": count("already_compressed"),
        "bytesBefore": fb, "bytesAfter": fa, "gpuBytesBefore": gb, "gpuBytesAfter": ga, "files": files });
    (report, issues)
}
