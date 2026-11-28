//! A project resolved before anything else reads it (docs/design/banc-de-demo-gripforge.md § 5): the `.gripforge`
//! files it includes (`includes`: `file`, `as`, `variables`, `hide`, `overrides`) merged under the master, the
//! variables of its nodes made game variables (`<id>.<name>`), and every `{ "$var": "<path>" }` read where the format
//! allows it (`visible`, a variable's start value, `data.scenes[].enabled`).
//!
//! One function for everyone ([`resolve_project`]): the validator and the pack ([`crate::project`]), dev.builder (the
//! project it is given is already resolved) and gripforge-build. Resolving a resolved project changes nothing, byte
//! for byte. A `visible` bound to a variable becomes a state derived from it and two reflection rules, so the render
//! follows the variable (resolved by game.engine's reflection, never copied at the tick).

use crate::issue::Issue;
use crate::tree::{merge, Files};
use serde_json::{json, Map, Value as Json};

mod instances;

/// The depth of nested includes allowed by default.
pub const MAX_DEPTH: usize = 8;
/// The keys only the master gives (an included file's are dropped when the master has its own).
const MASTER_KEYS: &[&str] = &["$schema", "id", "name", "run", "meta", "preset", "schema"];

/// [`resolve_project_with`] at the default depth ([`MAX_DEPTH`]). `files` reads a path relative to the project file.
pub fn resolve_project(project: &Json, files: Files) -> (Json, Vec<Issue>) {
    resolve_project_with(project, files, MAX_DEPTH)
}

/// The project with its includes merged, its nodes' variables hoisted and its `$var` resolved, and what went wrong.
pub fn resolve_project_with(project: &Json, files: Files, max_depth: usize) -> (Json, Vec<Issue>) {
    let mut issues = Vec::new();
    if !project.is_object() {
        return (project.clone(), issues);
    }
    let mut stack = Vec::new();
    let mut p = expand(project, "", files, &mut stack, 0, max_depth, &mut issues);
    let from_instances = instances::instances(&mut p, &mut issues);
    hoist_node_variables(&mut p, &mut issues);
    instances::add_variables(&mut p, from_instances);
    check_var_places(&p, "", None, &mut issues);
    resolve_relays(&mut p, &mut issues);
    enabled_scenes(&mut p, &mut issues);
    bind_visible(&mut p, &mut issues);
    (p, issues)
}

/// The resolution over files given as a map (path → content), for a reader that fetches them (`devGame` in the web
/// page, through the wasm of `core/scene-format-wasm`): `{ project, issues, missing }`, `missing` the paths asked and
/// not in the map. The reader fetches them and calls again until `missing` is empty or nothing more can be fetched.
pub fn resolve_with_files(project: &Json, files: &Map<String, Json>) -> Json {
    let missing = std::cell::RefCell::new(std::collections::BTreeSet::new());
    let read = |p: &str| {
        let f = files.get(p).cloned();
        if f.is_none() {
            missing.borrow_mut().insert(p.to_string());
        }
        f
    };
    let (p, issues) = resolve_project(project, &read);
    json!({
        "project": p,
        "issues": issues.iter().map(Issue::to_json).collect::<Vec<_>>(),
        "missing": missing.into_inner().into_iter().collect::<Vec<_>>(),
    })
}

fn dir_of(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..=i])
}

/// `dir` + `file`, with `.` and `..` folded (leading `..` kept: the reader resolves them from the project's folder). A
/// `file` that starts with a root alias (`#tests/…`, crate::path::ROOT_ALIASES) starts from the root, whatever `dir`;
/// a `..` never folds the alias away (it stays, and [`crate::path::root_alias_error`] refuses the path).
fn join(dir: &str, file: &str) -> String {
    let whole = if file.starts_with('#') {
        file.to_string()
    } else {
        format!("{dir}{file}")
    };
    let mut out: Vec<&str> = Vec::new();
    for seg in whole.split('/') {
        match seg {
            "" | "." => {}
            ".." if out.last().is_some_and(|s| *s != "..")
                && !(out.len() == 1 && out[0].starts_with('#')) =>
            {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

fn stem(file: &str) -> String {
    let base = file.rsplit('/').next().unwrap_or(file);
    base.split('.').next().unwrap_or(base).to_string()
}

/// A project with its includes merged in order, then itself over them.
fn expand(
    doc: &Json,
    path: &str,
    files: Files,
    stack: &mut Vec<String>,
    depth: usize,
    max: usize,
    issues: &mut Vec<Issue>,
) -> Json {
    let mut acc = json!({});
    for entry in doc
        .get("includes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let Some(file) = entry.get("file").and_then(Json::as_str) else {
            issues.push(
                Issue::error(
                    "include_missing",
                    "an include has no file",
                    "write { \"file\": \"<path>.gripforge\", \"as\": \"<prefix>\" }",
                )
                .in_file(path)
                .at("", "includes"),
            );
            continue;
        };
        let target = join(dir_of(path), file);
        if let Some(why) = crate::path::root_alias_error(&target) {
            issues.push(
                Issue::error(
                    "include_missing",
                    format!("the included file {target} {why}"),
                    "write the path relative to the file that includes it, or from the root with a root alias (#kits/, #tools/, #core/, #tests/)",
                )
                .in_file(path)
                .at("", "includes"),
            );
            continue;
        }
        if stack.contains(&target) || depth + 1 > max {
            issues.push(
                Issue::error(
                    "include_cycle",
                    format!("{target} is included in a loop or deeper than {max} levels"),
                    "remove the include that comes back to a file already included",
                )
                .in_file(&target),
            );
            continue;
        }
        let Some(sub) = files(&target) else {
            issues.push(
                Issue::error(
                    "include_missing",
                    format!("the included file {target} is not found"),
                    "write the path of the .gripforge relative to the file that includes it, or from the root with a root alias (#kits/, #tools/, #core/, #tests/)",
                )
                .in_file(&target),
            );
            continue;
        };
        stack.push(target.clone());
        let sub = expand(&sub, &target, files, stack, depth + 1, max, issues);
        stack.pop();
        let prefix = entry
            .get("as")
            .and_then(Json::as_str)
            .map_or_else(|| stem(file), str::to_string);
        let mut sub = prefixed(sub, &prefix, dir_of(&target), files);
        apply_entry(&mut sub, entry, &prefix, &target, issues);
        merge_project(&mut acc, &sub, false, &target, issues);
    }
    let mut own = doc.clone();
    if let Some(o) = own.as_object_mut() {
        o.remove("includes");
    }
    merge_project(&mut acc, &own, true, path, issues);
    acc
}

/// An included project with its root variables named `<as>.<name>` (and every read of them), its scene files carried
/// in `data.scene_files` under `<as>/`.
fn prefixed(mut sub: Json, prefix: &str, dir: &str, files: Files) -> Json {
    let names: Vec<String> = sub
        .get("variables")
        .and_then(Json::as_object)
        .map(|v| v.keys().cloned().collect())
        .unwrap_or_default();
    rename_vars(&mut sub, &names, prefix);
    if let Some(v) = sub.get_mut("variables").and_then(Json::as_object_mut) {
        *v = std::mem::take(v)
            .into_iter()
            .map(|(k, x)| (format!("{prefix}.{k}"), x))
            .collect();
    }
    let carried = sub
        .pointer("/data/scene_files")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let mut out = Map::new();
    let mut todo: Vec<String> = Vec::new();
    for s in sub
        .pointer_mut("/data/scenes")
        .and_then(Json::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(f) = s.get("file").and_then(Json::as_str).map(str::to_string) {
            todo.push(f.clone());
            s["file"] = json!(format!("{prefix}/{f}"));
        }
        if let Some(n) = s.get_mut("nodes") {
            rename_scene_refs(n, prefix, &mut todo);
        }
    }
    // Every sub-scene the file carries, its own scenes using it or not: a library of sub-scenes is for who includes it.
    todo.extend(carried.keys().cloned());
    while let Some(f) = todo.pop() {
        let key = format!("{prefix}/{f}");
        if out.contains_key(&key) {
            continue;
        }
        let Some(mut d) = carried.get(&f).cloned().or_else(|| {
            let p = join(dir, &f);
            crate::path::root_alias_error(&p)
                .is_none()
                .then(|| files(&p))
                .flatten()
        }) else {
            continue;
        };
        if let Some(n) = d.get_mut("nodes") {
            rename_scene_refs(n, prefix, &mut todo);
        }
        out.insert(key, d);
    }
    if sub.get("data").is_none() {
        sub["data"] = json!({});
    }
    sub["data"]["scene_files"] = Json::Object(out);
    sub
}

/// The reads of a root variable (`$var`, `var`, the `path` of `set_var` / `inc_var`) renamed `<as>.<name>`.
fn rename_vars(v: &mut Json, names: &[String], prefix: &str) {
    match v {
        Json::Object(o) => {
            for (k, x) in o.iter_mut() {
                if let (true, Json::String(f)) = (k == "calc", &*x) {
                    *x = json!(rename_calc(f, names, prefix));
                    continue;
                }
                let reads = matches!(k.as_str(), "$var" | "var");
                if let (true, Json::String(s)) = (reads, &*x) {
                    if names.contains(s) {
                        *x = json!(format!("{prefix}.{s}"));
                    }
                    continue;
                }
                if matches!(k.as_str(), "set_var" | "inc_var") {
                    if let Some(Json::String(s)) = x.get("path") {
                        if names.contains(s) {
                            x["path"] = json!(format!("{prefix}.{s}"));
                        }
                    }
                }
                rename_vars(x, names, prefix);
            }
        }
        Json::Array(a) => a.iter_mut().for_each(|x| rename_vars(x, names, prefix)),
        _ => {}
    }
}

/// The sub-scenes a list of nodes instances, renamed `<as>/<path>` and noted to be carried.
fn rename_scene_refs(nodes: &mut Json, prefix: &str, todo: &mut Vec<String>) {
    for n in nodes.as_array_mut().into_iter().flatten() {
        if n.get("type") == Some(&json!("scene")) {
            if let Some(s) = n.get("scene").and_then(Json::as_str).map(str::to_string) {
                todo.push(s.clone());
                n["scene"] = json!(format!("{prefix}/{s}"));
            }
        }
        if let Some(c) = n.get_mut("children") {
            rename_scene_refs(c, prefix, todo);
        }
    }
}

/// The parameters of an include entry: its variables' start values, its `hide` and its `overrides`.
fn apply_entry(sub: &mut Json, entry: &Json, prefix: &str, file: &str, issues: &mut Vec<Issue>) {
    for (k, v) in entry
        .get("variables")
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
    {
        let name = format!("{prefix}.{k}");
        match sub
            .get_mut("variables")
            .and_then(Json::as_object_mut)
            .filter(|m| m.contains_key(&name))
        {
            Some(m) => {
                m.insert(name, v.clone());
            }
            None => issues.push(
                Issue::error(
                    "include_unknown_variable",
                    format!("{file} has no variable {k}"),
                    format!(
                        "give a variable {file} declares in its variables, or declare {k} there"
                    ),
                )
                .in_file(file)
                .at("", &format!("includes.variables.{k}")),
            ),
        }
    }
    let hide = entry.get("hide");
    let hidden: Vec<Json> = hide
        .and_then(|h| h.get("scenes"))
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    if !hidden.is_empty() {
        if let Some(l) = sub.pointer_mut("/data/scenes").and_then(Json::as_array_mut) {
            l.retain(|s| !s.get("id").is_some_and(|id| hidden.contains(id)));
        }
        for k in sub
            .get_mut("installed")
            .and_then(Json::as_array_mut)
            .into_iter()
            .flatten()
        {
            if k.get("id") == Some(&json!("game.engine"))
                && k.pointer("/config/startScene")
                    .is_some_and(|s| hidden.contains(s))
            {
                k["config"].as_object_mut().map(|c| c.remove("startScene"));
            }
        }
    }
    let mut patches: Vec<(String, Json)> = hide
        .and_then(|h| h.get("nodes"))
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .map(|t| (t.to_string(), json!({ "visible": false })))
        .collect();
    for (t, v) in entry
        .get("overrides")
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
    {
        patches.push((t.clone(), v.clone()));
    }
    for (target, patch) in patches {
        if !override_node(sub, &target, &patch) {
            issues.push(
                Issue::warning(
                    "override_invalid",
                    format!("{target} names no node of {file}"),
                    "address a node as <scene id>:<path of names> or <scene id>:#<id>",
                )
                .in_file(file)
                .at("", &format!("includes.overrides.{target}")),
            );
        }
    }
}

/// Merge `patch` into the node `scene:path` or `scene:#id`; whether it was found.
fn override_node(project: &mut Json, target: &str, patch: &Json) -> bool {
    let (Some((scene, at)), Some(patch)) = (target.split_once(':'), patch.as_object()) else {
        return false;
    };
    let Some(s) = project
        .pointer("/data/scenes")
        .and_then(Json::as_array)
        .and_then(|l| l.iter().position(|s| s.get("id") == Some(&json!(scene))))
    else {
        return false;
    };
    let file = project["data"]["scenes"][s]
        .get("file")
        .and_then(Json::as_str)
        .map(str::to_string);
    let nodes = match file {
        Some(f) => project
            .pointer_mut(&format!(
                "/data/scene_files/{}",
                f.replace('~', "~0").replace('/', "~1")
            ))
            .and_then(|d| d.get_mut("nodes")),
        None => project["data"]["scenes"][s].get_mut("nodes"),
    };
    let Some(n) = nodes.and_then(|n| find_node(n, at)) else {
        return false;
    };
    match n.as_object_mut() {
        Some(o) => {
            merge(o, patch);
            true
        }
        None => false,
    }
}

fn find_node<'a>(nodes: &'a mut Json, at: &str) -> Option<&'a mut Json> {
    if let Some(id) = at.strip_prefix('#') {
        return find_by_id(nodes, id);
    }
    let mut cur = nodes;
    let mut segs = at.split('/').peekable();
    while let Some(seg) = segs.next() {
        let n = cur
            .as_array_mut()?
            .iter_mut()
            .find(|n| n.get("name") == Some(&json!(seg)))?;
        if segs.peek().is_none() {
            return Some(n);
        }
        cur = n.get_mut("children")?;
    }
    None
}

fn find_by_id<'a>(nodes: &'a mut Json, id: &str) -> Option<&'a mut Json> {
    for n in nodes.as_array_mut()? {
        if n.get("id") == Some(&json!(id)) {
            return Some(n);
        }
        if let Some(found) = n.get_mut("children").and_then(|c| find_by_id(c, id)) {
            return Some(found);
        }
    }
    None
}

fn conflict(file: &str, what: String) -> Issue {
    Issue::error("include_conflict", format!("{what} is given twice"), "rename one of them, or write \"replace\": true on the master's document to replace the included one").in_file(file)
}

/// Merge a project into what is gathered: requires and kits united (the master's kit config over), variables, data
/// documents by id and carried files united (twice = `include_conflict`, unless the master writes `replace: true`).
fn merge_project(acc: &mut Json, doc: &Json, master: bool, file: &str, issues: &mut Vec<Issue>) {
    let Some(o) = doc.as_object() else { return };
    for (k, v) in o {
        match k.as_str() {
            "requires" => {
                let l = acc.as_object_mut().unwrap().entry(k).or_insert(json!([]));
                for r in v.as_array().into_iter().flatten() {
                    if let Some(a) = l.as_array_mut().filter(|a| !a.contains(r)) {
                        a.push(r.clone());
                    }
                }
            }
            "installed" => {
                let l = acc.as_object_mut().unwrap().entry(k).or_insert(json!([]));
                let Some(a) = l.as_array_mut() else { continue };
                for kit in v.as_array().into_iter().flatten() {
                    match a
                        .iter_mut()
                        .find(|x| x.get("id").is_some() && x.get("id") == kit.get("id"))
                    {
                        Some(Json::Object(x)) => {
                            if let Some(p) = kit.as_object() {
                                merge(x, p);
                            }
                        }
                        _ => a.push(kit.clone()),
                    }
                }
            }
            "variables" | "bindings" => {
                let m = acc.as_object_mut().unwrap().entry(k).or_insert(json!({}));
                for (name, x) in v.as_object().into_iter().flatten() {
                    match m.get(name) {
                        Some(old) if old != x => {
                            issues.push(conflict(file, format!("the {k} {name}")))
                        }
                        _ => {
                            m[name] = x.clone();
                        }
                    }
                }
            }
            "data" => merge_data(acc, v, master, file, issues),
            _ if master || acc.get(k).is_none() => {
                acc[k] = v.clone();
            }
            _ if MASTER_KEYS.contains(&k.as_str()) => {}
            _ if acc.get(k) != Some(v) => issues.push(conflict(file, format!("the key {k}"))),
            _ => {}
        }
    }
}

fn merge_data(acc: &mut Json, data: &Json, master: bool, file: &str, issues: &mut Vec<Issue>) {
    if acc.get("data").is_none() {
        acc["data"] = json!({});
    }
    for (c, v) in data.as_object().into_iter().flatten() {
        let into = &mut acc["data"];
        match (into.get_mut(c), v) {
            (None, _) => {
                let mut v = v.clone();
                strip_replace(&mut v);
                into[c] = v;
            }
            (Some(Json::Array(a)), Json::Array(docs)) => {
                for d in docs {
                    let mut d = d.clone();
                    let replace = master && d.get("replace") == Some(&json!(true));
                    if let Some(o) = d.as_object_mut() {
                        o.remove("replace");
                    }
                    match a
                        .iter_mut()
                        .find(|x| d.get("id").is_some() && x.get("id") == d.get("id"))
                    {
                        Some(old) if replace => *old = d,
                        Some(old) if *old == d => {}
                        Some(_) => issues.push(conflict(
                            file,
                            format!("the document {} of data.{c}", d["id"]),
                        )),
                        None => a.push(d),
                    }
                }
            }
            (Some(Json::Object(m)), Json::Object(add)) => {
                for (k, x) in add {
                    match m.get(k) {
                        Some(old) if old != x => {
                            issues.push(conflict(file, format!("data.{c}.{k}")))
                        }
                        _ => {
                            m.insert(k.clone(), x.clone());
                        }
                    }
                }
            }
            (Some(old), _) if master => *old = v.clone(),
            (Some(old), _) if old != v => issues.push(conflict(file, format!("data.{c}"))),
            _ => {}
        }
    }
}

fn strip_replace(v: &mut Json) {
    for d in v.as_array_mut().into_iter().flatten() {
        if let Some(o) = d.as_object_mut() {
            o.remove("replace");
        }
    }
}

/// Every list of nodes of the project: the inline scenes' and the carried files'.
/// The files only instanced by a `scene` node are left out: their nodes are read per instance (`instances`).
fn node_lists(p: &mut Json) -> Vec<&mut Json> {
    let instanced = instances::instanced_files(p);
    let mut out = Vec::new();
    let Some(data) = p.get_mut("data").and_then(Json::as_object_mut) else {
        return out;
    };
    for (k, v) in data.iter_mut() {
        match k.as_str() {
            "scenes" => out.extend(
                v.as_array_mut()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s.get_mut("nodes")),
            ),
            "scene_files" => out.extend(
                v.as_object_mut()
                    .into_iter()
                    .flatten()
                    .filter(|(k, _)| !instanced.contains(k.as_str()))
                    .filter_map(|(_, d)| d.get_mut("nodes")),
            ),
            _ => {}
        }
    }
    out
}

pub(crate) fn walk_nodes(nodes: &mut Json, f: &mut dyn FnMut(&mut Map<String, Json>)) {
    for n in nodes.as_array_mut().into_iter().flatten() {
        if let Some(o) = n.as_object_mut() {
            f(o);
            if let Some(c) = o.get_mut("children") {
                walk_nodes(c, f);
            }
        }
    }
}

/// A node's `variables` become game variables `<id>.<name>` (a node without an id cannot have any).
fn hoist_node_variables(p: &mut Json, issues: &mut Vec<Issue>) {
    let mut found: Vec<(String, Json)> = Vec::new();
    for list in node_lists(p) {
        walk_nodes(list, &mut |o| {
            let Some(vars) = o.remove("variables") else {
                return;
            };
            let Some(id) = o.get("id").and_then(Json::as_str) else {
                let name = o.get("name").and_then(Json::as_str).unwrap_or("");
                issues.push(
                    Issue::error(
                        "node_variables_without_id",
                        format!("the node {name} has variables but no id"),
                        "give the node an id: its variables are named <id>.<name>",
                    )
                    .at(name, "variables"),
                );
                return;
            };
            for (k, v) in vars.as_object().into_iter().flatten() {
                found.push((format!("{id}.{k}"), v.clone()));
            }
        });
    }
    if found.is_empty() {
        return;
    }
    if !p.get("variables").is_some_and(Json::is_object) {
        p["variables"] = json!({});
    }
    for (k, v) in found {
        p["variables"]
            .as_object_mut()
            .unwrap()
            .entry(k)
            .or_insert(v);
    }
}

/// A `$var` outside its places (`visible`, a variable's value, a scene's `enabled`): `var_ref_not_allowed`.
fn check_var_places(v: &Json, at: &str, parent: Option<&str>, issues: &mut Vec<Issue>) {
    match v {
        Json::Object(o) => {
            if o.contains_key("$var") {
                let allowed = matches!(parent, Some("visible") | Some("enabled"))
                    || (at.starts_with("/variables/") && at.matches('/').count() == 2);
                if !allowed {
                    issues.push(Issue::error("var_ref_not_allowed", format!("{{ \"$var\" }} is read at {at}, where the format does not read it"), "read a variable only in visible, in a variable's start value or in data.scenes[].enabled").at("", at));
                }
                return;
            }
            for (k, x) in o {
                check_var_places(x, &format!("{at}/{k}"), Some(k), issues);
            }
        }
        Json::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                check_var_places(x, &format!("{at}/{i}"), None, issues);
            }
        }
        _ => {}
    }
}

pub(crate) fn var_ref(v: &Json) -> Option<&str> {
    v.as_object()
        .filter(|o| o.len() == 1)?
        .get("$var")?
        .as_str()
}

fn unknown(path: &str) -> Issue {
    Issue::error(
        "var_unknown",
        format!("the variable {path} is read but declared nowhere"),
        "declare it in the project's variables, or on the node whose id starts its path",
    )
}

/// A root variable whose start value reads another takes that value (relays of an include's parameters).
fn resolve_relays(p: &mut Json, issues: &mut Vec<Issue>) {
    let Some(vars) = p.get("variables").and_then(Json::as_object).cloned() else {
        return;
    };
    let mut out = vars.clone();
    for (k, v) in &vars {
        let mut cur = v;
        let mut hops = 0;
        while let Some(r) = var_ref(cur) {
            hops += 1;
            match vars.get(r) {
                Some(n) if hops <= vars.len() => cur = n,
                _ => {
                    issues.push(unknown(r).at("", &format!("variables.{k}")));
                    cur = &Json::Null;
                    break;
                }
            }
        }
        out.insert(k.clone(), cur.clone());
    }
    p["variables"] = Json::Object(out);
}

fn start_value<'a>(p: &'a Json, path: &str) -> Option<&'a Json> {
    p.get("variables")?.get(path)
}

/// `data.scenes[].enabled` read at the start: a disabled scene leaves the flow.
fn enabled_scenes(p: &mut Json, issues: &mut Vec<Issue>) {
    let Some(scenes) = p.pointer("/data/scenes").and_then(Json::as_array).cloned() else {
        return;
    };
    let mut keep = Vec::new();
    for mut s in scenes {
        let on = match s.get("enabled") {
            None => true,
            Some(e) => match var_ref(e) {
                Some(r) => match start_value(p, r) {
                    Some(v) => v != &json!(false),
                    None => {
                        issues.push(unknown(r).at("", "data.scenes.enabled"));
                        true
                    }
                },
                None => e != &json!(false),
            },
        };
        if let Some(o) = s.as_object_mut() {
            o.remove("enabled");
        }
        if on {
            keep.push(s);
        }
    }
    p["data"]["scenes"] = Json::Array(keep);
}

/// A `visible` bound to a variable: a state derived from the variable and its two reflection rules on the node.
fn bind_visible(p: &mut Json, issues: &mut Vec<Issue>) {
    let vars = p.get("variables").cloned().unwrap_or(json!({}));
    for list in node_lists(p) {
        walk_nodes(list, &mut |o| {
            let Some(r) = o.get("visible").and_then(var_ref).map(str::to_string) else {
                return;
            };
            o.remove("visible");
            if vars.get(&r).is_none() {
                let name = o.get("name").and_then(Json::as_str).unwrap_or("");
                issues.push(unknown(&r).at(name, "visible"));
                return;
            }
            bind_node_visible(o, &r);
        });
    }
}

/// The node's `visible` follows the variable `r`: a state derived from it and its two reflection rules, each setting the
/// node itself (the key `""` of `set`, as `states::check` and game.engine read a reflection).
pub(crate) fn bind_node_visible(o: &mut Map<String, Json>, r: &str) {
    let state: String = format!("var_{r}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let entity = o.entry("entity").or_insert(json!({}));
    if !entity.get("states").is_some_and(Json::is_object) {
        entity["states"] = json!({});
    }
    entity["states"][&state] = json!({ "derived": { "var": r } });
    let reflect = o.entry("reflect").or_insert(json!([]));
    if let Some(a) = reflect.as_array_mut() {
        a.push(json!({ "when": { &state: true }, "set": { "": { "visible": true } } }));
        a.push(json!({ "when": { &state: false }, "set": { "": { "visible": false } } }));
    }
}

/// `var.<name>` of a formula (`calc`) renamed `var.<as>.<name>` for the included file's root variables.
fn rename_calc(s: &str, names: &[String], prefix: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while i < s.len() {
        if s[i..].starts_with("var.") && (i == 0 || !(word(b[i - 1]) || b[i - 1] == b'.')) {
            let start = i + 4;
            let mut end = start;
            while end < s.len() && word(b[end]) {
                end += 1;
            }
            out.push_str("var.");
            if names.iter().any(|n| n == &s[start..end]) {
                out.push_str(prefix);
                out.push('.');
            }
            out.push_str(&s[start..end]);
            i = end;
        } else {
            let c = s[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}
