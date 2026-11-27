#![forbid(unsafe_code)]
//! Leaf support implementations canonical to the pure world — numeric
//! parsing, Unicode identifier classes, and GNU's permutation heapsort.
//! The old world re-exports these until its deletion.

pub(crate) mod colors;
pub(crate) mod identifier;
pub(crate) mod number_parse;
pub(crate) mod sort;

/// Browser-mode virtual file store limits — the contract wasm consumers
/// validate against before writes reach the interpreter.
pub const MAX_BROWSER_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_BROWSER_FILE_PATH_BYTES: usize = 4096;
