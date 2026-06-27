// Re-export the canonical `nmath::dpq` (dpq.h macro translations).
//
// The top-level `dpq` module was a divergent copy (it defined a local M_LN2 and
// used `libm::*` imports where nmath uses explicit imports). Callers keep using
// `crate::dpq::*` and symbols now resolve to the single canonical source.
//
// Refs rport-vy1h, rport-ee8d.
pub use crate::nmath::dpq::*;
