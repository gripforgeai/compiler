//! The command line of `gripforge-build` (`gripforge project build`): [`main`] answers the exit code (0 built or
//! checked, 1 the project has errors, 2 a wrong command line) and writes through `out` / `err`, so that the tests run
//! it in process.
//!
//!   gripforge-build [<project.gripforge | folder>] [--out <dir>] [--kits-path <dir>] [--sdk <dir>] [--sdk-rev <rev>]
//!                   [--check | --resolve] [--dev] [--lenient] [--json] [--compress-textures | --no-compress-textures] [--compress-textures-url <url>]

use crate::build::{build, BuildOptions};
use std::path::PathBuf;

pub const USAGE: &str = "usage: gripforge-build [<project.gripforge | folder>] [--out <dir>] [--kits-path <dir>] [--sdk <dir>] [--sdk-rev <rev>] [--check | --resolve] [--dev] [--lenient] [--json] [--compress-textures | --no-compress-textures] [--compress-textures-url <url>]";

/// What a command line asks.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub project: Option<String>,
    pub out: Option<String>,
    pub kits_path: Option<String>,
    pub sdk: Option<String>,
    pub sdk_rev: Option<String>,
    pub check: bool,
    pub resolve: bool,
    pub dev: bool,
    pub lenient: bool,
    pub json: bool,
    pub compress_textures: bool,
    pub compress_textures_url: Option<String>,
    pub help: bool,
    pub version: bool,
}

/// The options of a command line, or what is wrong with it.
pub fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(x) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match x.as_str() {
            "--out" => a.out = Some(value(x)?),
            "--kits-path" => a.kits_path = Some(value(x)?),
            "--sdk" => a.sdk = Some(value(x)?),
            "--sdk-rev" => a.sdk_rev = Some(value(x)?),
            "--check" => a.check = true,
            "--resolve" => a.resolve = true,
            "--dev" => a.dev = true,
            "--lenient" => a.lenient = true,
            "--json" => a.json = true,
            "--compress-textures" => a.compress_textures = true,
            "--no-compress-textures" => a.compress_textures = false,
            "--compress-textures-url" => a.compress_textures_url = Some(value(x)?),
            "--help" | "-h" => a.help = true,
            "--version" | "-V" => a.version = true,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => match &a.project {
                None => a.project = Some(s.to_string()),
                Some(p) => return Err(format!("one project only (got {p} and {s})")),
            },
        }
    }
    Ok(a)
}

/// Runs the command; answers its exit code.
pub fn main(argv: &[String], out: &mut dyn FnMut(&str), err: &mut dyn FnMut(&str)) -> i32 {
    let a = match parse(argv) {
        Ok(a) => a,
        Err(e) => {
            err(&format!("gripforge-build: {e}"));
            err(USAGE);
            return 2;
        }
    };
    if a.help {
        out(USAGE);
        return 0;
    }
    if a.version {
        out(crate::core::VERSION);
        return 0;
    }
    if a.resolve {
        // The project resolved and the tolerant validator's issues, as JSON: what the development mode of the kits
        // makes of it (the parity test of the kits repository compares the two).
        let answer = crate::build::resolved(&BuildOptions {
            project: a.project.map(PathBuf::from),
            kits_path: a.kits_path.map(PathBuf::from),
            ..Default::default()
        });
        out(&crate::json::pretty(&answer));
        return if answer["ok"] == true { 0 } else { 1 };
    }
    let r = build(&BuildOptions {
        project: a.project.map(PathBuf::from),
        out: a.out.map(PathBuf::from),
        kits_path: a.kits_path.map(PathBuf::from),
        sdk: a.sdk.map(PathBuf::from),
        sdk_rev: a.sdk_rev,
        lenient: a.lenient,
        check: a.check,
        dev: a.dev,
        compress_textures: a.compress_textures,
        compress_textures_url: a.compress_textures_url,
    });
    let name = r.project_name.clone().unwrap_or_else(|| "project".into());
    if a.json {
        out(&crate::json::pretty(&crate::report::report(&r)));
    } else {
        for i in &r.issues {
            err(&crate::report::line(i, &name));
        }
        let errors = r.issues.iter().filter(|i| i["level"] == "error").count();
        let warnings = r.issues.len() - errors;
        if r.ok {
            let what = if a.check {
                "checked".to_string()
            } else {
                format!(
                    "generated in {}",
                    r.out
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                )
            };
            let steps: Vec<String> = r
                .steps
                .iter()
                .map(|s| format!("{} {} ms", s["name"].as_str().unwrap_or(""), s["ms"]))
                .collect();
            err(&format!(
                "{name}: {what} ({warnings} warning(s)) — {}",
                steps.join(", ")
            ));
        } else {
            err(&format!("{name}: {errors} error(s), {warnings} warning(s) — nothing generated; fix the project and build again"));
        }
    }
    if r.ok {
        0
    } else {
        1
    }
}
