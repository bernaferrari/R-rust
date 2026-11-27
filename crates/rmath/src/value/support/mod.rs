#![forbid(unsafe_code)]
//! Leaf support implementations canonical to the pure world — numeric
//! parsing, Unicode identifier classes, and GNU's permutation heapsort.
//! The old world re-exports these until its deletion.

pub(crate) mod identifier;
pub(crate) mod number_parse;
pub(crate) mod sort;
