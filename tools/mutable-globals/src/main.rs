//! `mutable-globals [--root DIR] [--config FILE] [--report FILE] (--check | --write | --list)`
//!
//! `--check`: fail on any problem or when the report file is not current.
//! `--write`: regenerate the report (still fails on problems).
//! `--list`:  print every discovered declaration, one per line.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut root = PathBuf::from(".");
    let mut config = None;
    let mut report = None;
    let mut mode = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => root = args.next().map(PathBuf::from).unwrap_or(root),
            "--config" => config = args.next().map(PathBuf::from),
            "--report" => report = args.next().map(PathBuf::from),
            "--check" | "--write" | "--list" => mode = Some(a),
            other => {
                eprintln!("unknown argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(mode) = mode else {
        eprintln!("usage: mutable-globals [--root DIR] [--config FILE] [--report FILE] (--check | --write | --list)");
        return ExitCode::from(2);
    };
    let config_path = config.unwrap_or_else(|| root.join("scripts/mutable-globals.toml"));
    let report_path = report.unwrap_or_else(|| root.join("docs/generated/mutable-globals.md"));
    let cfg = match mutable_globals::load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let out = mutable_globals::check(&root, &cfg);
    if mode == "--list" {
        for d in &out.discovery.decls {
            println!(
                "{}\t{}:{}\t{:?}\t{:?}\trequired={}\t{}",
                d.key,
                d.file,
                d.line,
                d.kind,
                d.active,
                d.required(),
                d.ty
            );
        }
        return ExitCode::SUCCESS;
    }
    let mut failed = false;
    for p in &out.problems {
        eprintln!("mutable-globals: {p}");
        failed = true;
    }
    if mode == "--write" {
        if let Some(dir) = report_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&report_path, &out.report) {
            eprintln!("{}: {e}", report_path.display());
            return ExitCode::from(2);
        }
    } else if std::fs::read_to_string(&report_path).ok().as_deref() != Some(out.report.as_str()) {
        eprintln!("mutable-globals: {} is not current; rerun with --write", report_path.display());
        failed = true;
    }
    let required = out.discovery.decls.iter().filter(|d| d.required()).count();
    eprintln!(
        "mutable-globals: {} declarations, {} need review, {} problems",
        out.discovery.decls.len(),
        required,
        out.problems.len()
    );
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
