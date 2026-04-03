fn main() {
    cc::Build::new()
        .file("src/shim.c")
        .include("include")
        .warnings(false)
        .compile("r_shim");

    #[cfg(target_os = "linux")]
    println!("cargo:rustc-cdylib-link-arg=-Wl,--version-script=src/version.map");

    println!("cargo:rerun-if-changed=include/R.h");
    println!("cargo:rerun-if-changed=include/Rinternals.h");
    println!("cargo:rerun-if-changed=src/shim.c");
}
