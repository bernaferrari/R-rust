use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let backend = if env::var_os("CARGO_FEATURE_FORTRAN_BACKEND").is_some() {
        "system-fortran"
    } else {
        "faer-pure-rust"
    };
    if env::var_os("CARGO_FEATURE_FORTRAN_BACKEND").is_some() {
        println!("cargo:rerun-if-env-changed=RPORT_LAPACK_LIB_DIR");
        println!("cargo:rerun-if-env-changed=RPORT_LAPACK_LIB_NAME");
        if let Ok(directory) = env::var("RPORT_LAPACK_LIB_DIR") {
            println!("cargo:rustc-link-search=native={directory}");
        }
        if let Ok(library) = env::var("RPORT_LAPACK_LIB_NAME") {
            println!("cargo:rustc-link-lib={library}");
        } else if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
            println!("cargo:rustc-link-lib=framework=Accelerate");
        } else {
            println!("cargo:rustc-link-lib=lapack");
            println!("cargo:rustc-link-lib=blas");
        }
    }
    println!("cargo:rustc-env=RUST_LAPACK_BACKEND={backend}");

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
