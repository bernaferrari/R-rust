//! Compile-fail harness for the `Sexp` handle safety model.
//!
//! This is a dependency-free equivalent of `trybuild` (which is not in the
//! dependency graph). Each sibling `*.rs` file in this directory is a
//! standalone case compiled with the freshly built `librmath.rlib`:
//!
//! * A file containing `//~ ERROR: <pattern>` markers (compiletest
//!   convention) must FAIL to compile, and every marked pattern must
//!   appear in the compiler diagnostics. These files document the
//!   forbidden aliasing and construction patterns the safety model rules
//!   out at compile time.
//! * A file without markers must compile, link, and RUN to exit status 0.
//!   These files document the allowed patterns (rooting across GC,
//!   explicit cloning).
//!
//! The sibling case files are deliberately NOT cargo test targets (cargo
//! only builds `tests/*.rs` and `tests/*/main.rs`), so the forbidden code
//! never enters the normal build; only this harness feeds it to `rustc`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Directory holding the compile-fail/positive case files.
fn case_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("compile_fail")
}

/// Scratch directory for harness outputs (rmeta, case binaries).
fn out_dir() -> PathBuf {
    if let Some(dir) = option_env!("CARGO_TARGET_TMPDIR") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
        return PathBuf::from(dir);
    }
    std::env::temp_dir()
}

/// The `<target>/<profile>` directory of this very test run, derived from
/// the test binary path (`<target>/<profile>/deps/<test>-<hash>`).
fn current_profile_dir() -> Option<PathBuf> {
    let deps = std::env::current_exe().ok()?.parent()?.to_path_buf();
    deps.parent().map(|dir| dir.to_path_buf())
}

/// Locate `librmath.rlib` plus its `deps` search directory.
///
/// Prefers the profile directory of the running test binary (guaranteed to
/// match this build's profile), then falls back to candidate target roots
/// picking the most recently built rlib.
fn locate_rmath() -> (PathBuf, PathBuf) {
    if let Some(profile_dir) = current_profile_dir() {
        let rlib = profile_dir.join("librmath.rlib");
        if rlib.exists() {
            return (rlib, profile_dir.join("deps"));
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    // Workspace root target dir (rmath-rs/rmath -> rmath-rs -> repo root).
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
    {
        candidates.push(root.join("target"));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("target"));

    let mut best: Option<(SystemTime, PathBuf, PathBuf)> = None;
    for base in &candidates {
        for profile in ["debug", "release"] {
            let profile_dir = base.join(profile);
            let rlib = profile_dir.join("librmath.rlib");
            let Ok(metadata) = fs::metadata(&rlib) else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            if best.as_ref().is_none_or(|(time, _, _)| modified > *time) {
                best = Some((modified, rlib, profile_dir.join("deps")));
            }
        }
    }

    match best {
        Some((_, rlib, deps)) => (rlib, deps),
        None => panic!(
            "librmath.rlib not found under {candidates:?}; build the rmath crate before running this test"
        ),
    }
}

/// A `rustc` invocation preconfigured to resolve `rmath` from this build.
fn rustc(rlib: &Path, deps: &Path) -> Command {
    let mut cmd = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()));
    cmd.arg("--edition=2024")
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg("--extern")
        .arg(format!("rmath={}", rlib.display()));
    cmd
}

/// Extract the `<pattern>` of every `//~ ERROR: <pattern>` marker.
fn error_markers(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let start = line.find("//~ ERROR:")?;
            Some(line[start + "//~ ERROR:".len()..].trim().to_string())
        })
        .collect()
}

/// Compile `path` expecting failure; assert every marker appears in stderr.
fn check_compile_fail(path: &Path, markers: &[String], rlib: &Path, deps: &Path, out_dir: &Path) {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let output = rustc(rlib, deps)
        .arg("--crate-type=lib")
        .arg("--emit=metadata")
        .arg("-o")
        .arg(out_dir.join(format!("{stem}.rmeta")))
        .arg(path)
        .output()
        .unwrap_or_else(|error| panic!("{stem}: failed to spawn rustc: {error}"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        panic!(
            "{stem}: FORBIDDEN PATTERN COMPILED — the safety model does not hold.\nstderr:\n{stderr}"
        );
    }
    for marker in markers {
        assert!(
            stderr.contains(marker.as_str()),
            "{stem}: expected {marker:?} in diagnostics, got:\n{stderr}"
        );
    }
}

/// Compile `path` as a binary and run it, requiring exit status 0.
fn check_compile_and_run(path: &Path, rlib: &Path, deps: &Path, out_dir: &Path) {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let binary = out_dir.join(format!("{stem}-case"));
    let output = rustc(rlib, deps)
        .arg("-o")
        .arg(&binary)
        .arg(path)
        .output()
        .unwrap_or_else(|error| panic!("{stem}: failed to spawn rustc: {error}"));
    assert!(
        output.status.success(),
        "{stem}: allowed pattern must compile, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let run = Command::new(&binary)
        .output()
        .unwrap_or_else(|error| panic!("{stem}: failed to run compiled case: {error}"));
    assert!(
        run.status.success(),
        "{stem}: allowed pattern must run to completion, got status {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn compile_fail_safety_model() {
    let (rlib, deps) = locate_rmath();
    let dir = case_dir();

    let mut cases: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .filter(|path| path.file_name().is_some_and(|name| name != "main.rs"))
        .collect();
    cases.sort();
    assert!(
        !cases.is_empty(),
        "no case files found in {}",
        dir.display()
    );

    let out_dir = out_dir();
    let mut failures: Vec<String> = Vec::new();
    for case in &cases {
        let name = case.file_stem().unwrap_or_default().to_string_lossy();
        let source = match fs::read_to_string(case) {
            Ok(source) => source,
            Err(error) => {
                failures.push(format!("{name}: cannot read case: {error}"));
                continue;
            }
        };
        let markers = error_markers(&source);
        let result = std::panic::catch_unwind(|| {
            if markers.is_empty() {
                check_compile_and_run(case, &rlib, &deps, &out_dir);
            } else {
                check_compile_fail(case, &markers, &rlib, &deps, &out_dir);
            }
        });
        if let Err(panic) = result {
            let detail = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".to_string());
            failures.push(detail);
        }
        println!(
            "[compile_fail] {name}: ok ({})",
            if markers.is_empty() {
                "allowed pattern compiled, linked, ran".to_string()
            } else {
                format!("refused to compile as required; markers: {markers:?}")
            }
        );
    }

    assert!(
        failures.is_empty(),
        "compile-fail suite failed:\n\n{}",
        failures.join("\n\n")
    );
}
