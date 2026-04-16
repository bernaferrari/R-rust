use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn detect_r_include_dir() -> Option<PathBuf> {
    if let Ok(home) = env::var("R_HOME") {
        let include = PathBuf::from(home).join("include");
        if include.exists() {
            return Some(include);
        }
    }

    let output = Command::new("R").arg("RHOME").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if home.is_empty() {
        return None;
    }
    let include = PathBuf::from(home).join("include");
    if include.exists() {
        Some(include)
    } else {
        None
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/cshim/grid_release.c");
    println!("cargo:rerun-if-changed=../../r-source/src/include/R_ext/GraphicsDevice.h");
    println!("cargo:rerun-if-changed=../../r-source/src/include/R_ext/GraphicsEngine.h");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let vendored_r_headers = manifest_dir.join("../../r-source/src/include");
    let system_r_headers = detect_r_include_dir();

    let mut build = cc::Build::new();
    build
        .file(manifest_dir.join("src/cshim/grid_release.c"))
        .warnings(false);
    if vendored_r_headers.exists() {
        build.include(&vendored_r_headers);
    }
    if let Some(system_headers) = system_r_headers {
        build.include(system_headers);
    }
    build.compile("rmath_grid_release");

    // After building, copy librmath.a -> libRmath.a for C compatibility
    let target_dir = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
            Path::new(&manifest_dir).join("../../target")
        });

    let profile = env::var("PROFILE").unwrap(); // "release" or "debug"
    let src = target_dir.join(&profile).join("librmath.a");
    let dst = target_dir.join(&profile).join("libRmath.a");

    if src.exists() {
        std::fs::copy(&src, &dst).ok();
        println!(
            "cargo:warning=Copied {} -> {}",
            src.display(),
            dst.display()
        );
    }
}
