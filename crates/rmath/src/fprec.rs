// Re-export the canonical `nmath::fprec` implementation.
//
// The top-level `fprec` module was a byte-identical (after import-prefix
// normalization) copy of `crate::nmath::fprec`. Collapse to a single source.
//
// Refs rport-vy1h, rport-ee8d.
pub use crate::nmath::fprec::*;
