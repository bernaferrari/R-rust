use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var_os("CARGO_CFG_TARGET_ARCH").as_deref() == Some(std::ffi::OsStr::new("wasm32")) {
        pack_upstream_base_sources().expect("pack embedded GNU base sources");
        if env::var_os("CARGO_FEATURE_BROWSER_ANALYSIS_TOOLS").is_some() {
            pack_browser_analysis_tools().expect("apply the pinned browser analysis tools overlay");
        }
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

fn pack_browser_analysis_tools() -> io::Result<()> {
    let assets = Path::new("src/library/portable_package/assets/tools");
    let read = |file: &str| {
        let path = assets.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        fs::read(path)
    };
    let invalid = || io::Error::other("invalid browser analysis tools overlay");
    let original = read("wasm.data")?;
    let mut source_crc = flate2::Crc::new();
    source_crc.update(&original);
    source_crc.update(&read("wasm.blocks")?);
    source_crc.update(&read("wasm.index.rds")?);
    let overlay = read("browser-analysis/wasm.replacements")?;
    let (expected_crc, mut entries) = overlay.split_first_chunk::<4>().ok_or_else(invalid)?;
    if source_crc.sum() != u32::from_le_bytes(*expected_crc) {
        return Err(io::Error::other(
            "browser analysis tools overlay is stale. Regenerate it from the pinned package assets",
        ));
    }

    let mut output = Vec::with_capacity(original.len());
    let mut cursor = 0;
    // Each entry gives the original packed byte range followed by a complete
    // replacement gzip block. Copy every intervening original block as-is.
    while !entries.is_empty() {
        let (header, payload) = entries.split_first_chunk::<12>().ok_or_else(invalid)?;
        let mut fields = header
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_be_bytes(*bytes) as usize);
        let (start, end, length) = (
            fields.next().unwrap(),
            fields.next().unwrap(),
            fields.next().unwrap(),
        );
        if start >= end || end > original.len() || length >= end - start {
            return Err(invalid());
        }
        output.extend_from_slice(original.get(cursor..start).ok_or_else(invalid)?);
        output.extend_from_slice(payload.get(..length).ok_or_else(invalid)?);
        cursor = end;
        entries = &payload[length..];
    }
    output.extend_from_slice(&original[cursor..]);
    let directory = read("browser-analysis/wasm.blocks")?;
    let final_end = directory.last_chunk::<4>().ok_or_else(invalid)?;
    if output.len() != u32::from_be_bytes(*final_end) as usize {
        return Err(invalid());
    }
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output"));
    fs::write(out_dir.join("tools-analysis.data"), output)
}
