use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

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
