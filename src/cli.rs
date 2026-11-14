//! The command line of `gripforge-build` (`gripforge project build`): [`main`] answers the exit code (0 built or
//! checked, 1 the project has errors, 2 a wrong command line) and writes through `out` / `err`, so that the tests run
//! it in process.
//!
//!   gripforge-build [<project.gripforge | folder>] [--out <dir>] [--kits <dir>] [--sdk <dir>] [--sdk-rev <rev>]
//!                   [--check] [--lenient] [--json]

use crate::build::{build, BuildOptions};
use std::path::PathBuf;

pub const USAGE: &str = "usage: gripforge-build [<project.gripforge | folder>] [--out <dir>] [--kits <dir>] [--sdk <dir>] [--sdk-rev <rev>] [--check] [--lenient] [--json]";

/// What a command line asks.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub project: Option<String>,
    pub out: Option<String>,
    pub kits: Option<String>,
    pub sdk: Option<String>,
    pub sdk_rev: Option<String>,
    pub check: bool,
    pub lenient: bool,
    pub json: bool,
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
            "--kits" => a.kits = Some(value(x)?),
            "--sdk" => a.sdk = Some(value(x)?),
            "--sdk-rev" => a.sdk_rev = Some(value(x)?),
            "--check" => a.check = true,
            "--lenient" => a.lenient = true,
            "--json" => a.json = true,
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
    let r = build(&BuildOptions {
        project: a.project.map(PathBuf::from),
        out: a.out.map(PathBuf::from),
        kits: a.kits.map(PathBuf::from),
        sdk: a.sdk.map(PathBuf::from),
        sdk_rev: a.sdk_rev,
        lenient: a.lenient,
        check: a.check,
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
