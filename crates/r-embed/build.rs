fn main() {
    // Native-extension tests dlopen a fixture that calls back into the
    // engine's registration ABI (R_registerRoutines / R_useDynamicSymbols,
    // exported from rmath via no_mangle). On Linux, the executable must
    // re-export its symbols for dlopen(RTLD_NOW) to resolve them; macOS
    // resolves lazily through the flat namespace. Export-dynamic the test
    // binaries on ELF platforms so the fixture links against the host.
    println!("cargo:rustc-link-arg-tests=-rdynamic");
}
