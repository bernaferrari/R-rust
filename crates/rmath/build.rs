use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var_os("CARGO_CFG_TARGET_ARCH").as_deref() == Some(std::ffi::OsStr::new("wasm32")) {
        pack_upstream_base_sources().expect("pack embedded GNU base sources");
    }
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

fn pack_upstream_base_sources() -> io::Result<()> {
    let source_dir = Path::new("upstream/base/R");
    println!("cargo:rerun-if-changed={}", source_dir.display());
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output"));
    pack_r_directory(source_dir, &output.join("upstream-base"))
}

fn pack_r_directory(source_dir: &Path, packed_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(packed_dir)?;
    for file in fs::read_dir(source_dir)? {
        let file = file?;
        let path = file.path();
        let out = packed_dir.join(file.file_name());
        let file_type = file.file_type()?;
        if file_type.is_dir() {
            pack_r_directory(&path, &out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "R") {
            let source = fs::read(path)?;
            let stream = fs::File::create(out)?;
            let mut encoder = flate2::write::ZlibEncoder::new(stream, flate2::Compression::best());
            encoder.write_all(&source)?;
            encoder.finish()?;
        }
    }
    Ok(())
}
