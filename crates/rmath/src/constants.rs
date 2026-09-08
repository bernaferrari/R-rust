// Re-export the canonical `nmath::constants`.
//
// `crate::nmath::constants` is the centralized constants module (it superseded
// the older per-file duplicated constants). The top-level `constants` module
// historically held a divergent copy; callers keep using `crate::constants::*`
// and symbols now resolve to the single canonical source.
//
// Refs rport-vy1h, rport-ee8d.
pub use crate::nmath::constants::*;
