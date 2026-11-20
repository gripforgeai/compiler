//! One command written back to the project where its scene lives (docs/dev-builder.md « Appliquer au projet »): a scene
//! written inline in the project (`data.scenes[i].nodes`, also inside the `project` of a packed `game.json`) or a
//! sub-scene file it names (`file`). The command is applied by [`crate::ops::apply_command`], so only the changed values
//! move; for an inline scene only the bytes of that scene are handed to it, and put back in place (the rest of the
//! project is not even re-read). With the revision of a text (to refuse a write made on a text that changed since) and
//! the lines that moved (what a person or an AI reviews).
//!
//! Used by the dev.builder kit (`applyToProject`) and by any tool (CLI, MCP) that holds the texts. Never touches a disk:
//! texts in, texts out.

#![allow(
    clippy::result_large_err,
    reason = "a refusal is rare and handed to a person or an AI as it is (as ops.rs)"
)]

use crate::build::inline_label;
use crate::issue::Issue;
use crate::jsontext::{self, Seg};
use crate::ops::apply_command;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

/// The revision of a text: FNV-1a 64 bits of its bytes, in hex. A write names the revision it was made on
/// (`baseRevision`); another revision means the file changed in between, and the write is refused.
pub fn revision(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The lines that differ between two texts, as one block: `{ file, line (1-based, in before), before: [..], after: [..] }`;
/// `None` when they are equal. The common head and tail are left out, so a value changed on one line gives one line.
pub fn line_diff(file: &str, before: &str, after: &str) -> Option<Json> {
    if before == after {
        return None;
    }
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    Some(json!({ "file": file, "line": head + 1,
        "before": &a[head..a.len() - tail], "after": &b[head..b.len() - tail] }))
}

/// Where a project keeps its scenes: `data.scenes` of a project file, or `project.data.scenes` of a packed `game.json`.
fn scenes_path(doc: &Json) -> Option<Vec<Seg>> {
    if doc.pointer("/data/scenes").is_some_and(Json::is_array) {
        return Some(vec![Seg::Key("data".into()), Seg::Key("scenes".into())]);
    }
    doc.pointer("/project/data/scenes")
        .filter(|s| s.is_array())
        .map(|_| {
            vec![
                Seg::Key("project".into()),
                Seg::Key("data".into()),
                Seg::Key("scenes".into()),
            ]
        })
}

fn bad(code: &'static str, message: impl Into<String>, fix: impl Into<String>) -> Issue {
    Issue::error(code, message, fix)
}

/// What a write changed: the file (a project path), its text before and after.
#[derive(Clone, Debug)]
pub struct Written {
    /// The project path of the file written (the project file itself for an inline scene).
    pub file: String,
    /// Its text before.
    pub before: String,
    /// Its text after (the same when nothing had to change).
    pub after: String,
}

/// Applies `cmd` (a command of the site's scene patch, ops.rs) to the scene `scene` of the project whose text is
/// `texts[project]`: in the project itself when the scene is written inline, else in the file it names (`texts[file]`).
/// The other texts are the sub-scenes a command may need (a game.engine id inside an instanced sub-scene); the scene
/// files the project carries (`data.scene_files`) are read when a text is not given.
pub fn apply_in_project(
    texts: &BTreeMap<String, String>,
    project: &str,
    scene: &str,
    cmd: &Json,
) -> Result<Written, Issue> {
    let text = texts.get(project).ok_or_else(|| {
        bad(
            "project_text_missing",
            format!("the text of the project {project} was not given"),
            "give the texts of the project: { \"<project path>\": \"<its content>\" } (a kit never reads the disk)",
        )
    })?;
    let doc: Json = serde_json::from_str(text).map_err(|e| {
        bad(
            "file_invalid",
            format!("{project} is not valid JSON ({e})"),
            "fix the JSON first",
        )
    })?;
    let at = scenes_path(&doc).ok_or_else(|| {
        bad(
            "scene_unknown",
            format!("{project} has no data.scenes"),
            "write the scenes in data.scenes",
        )
    })?;
    let list = doc
        .pointer(if at.len() == 2 {
            "/data/scenes"
        } else {
            "/project/data/scenes"
        })
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(i) = list
        .iter()
        .position(|s| s.get("id").and_then(Json::as_str) == Some(scene))
    else {
        return Err(bad(
            "scene_unknown",
            format!("no scene {scene} in {project}"),
            "name a scene of data.scenes",
        ));
    };
    let carried = list.get(i).and(doc.pointer(if at.len() == 2 {
        "/data/scene_files"
    } else {
        "/project/data/scene_files"
    }));
    let read = |p: &str| {
        texts
            .get(p)
            .cloned()
            .or_else(|| carried.and_then(|c| c.get(p)).map(jsontext::to_text))
    };
    if let Some(file) = list[i].get("file").and_then(Json::as_str) {
        let before = read(file).ok_or_else(|| {
            bad(
                "scene_file_missing",
                format!("the text of {file} (scene {scene}) was not given"),
                "give it with the texts: { \"<its project path>\": \"<its content>\" }",
            )
            .in_file(file)
        })?;
        let after = apply_command(&before, file, cmd, &read).map_err(|e| e.in_file(file))?;
        return Ok(Written {
            file: file.to_string(),
            before,
            after,
        });
    }
    // Inline: only the bytes of this scene go to the command, then back in place.
    let root = jsontext::parse(text).map_err(|e| bad("file_invalid", e, "fix the JSON first"))?;
    let mut segs = at;
    segs.push(Seg::Index(i));
    let span = jsontext::find(&root, &segs).ok_or_else(|| {
        bad(
            "scene_unknown",
            format!("scene {scene} not found in the text of {project}"),
            "check the project",
        )
    })?;
    let (start, end) = (span.start, span.end);
    let piece = apply_command(&text[start..end], &inline_label(scene), cmd, &read)
        .map_err(|e| e.in_file(project))?;
    Ok(Written {
        file: project.to_string(),
        before: text.clone(),
        after: format!("{}{piece}{}", &text[..start], &text[end..]),
    })
}
