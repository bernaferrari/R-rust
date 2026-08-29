// Re-export the canonical `nmath::utils` implementations.
//
// These small numeric helpers (fmax2, fmin2, imax2, imin2, sign, fsign,
// ftrunc, r_forceint, r_nonint) live canonically in `rmath-nmath`. The
// top-level copy had diverged: its `r_forceint` used `libm::round`
// (round-half-away-from-zero) instead of C's `nearbyint`
// (round-half-to-even). Callers keep using `crate::utils::*`; symbols now
// resolve to the single canonical source; the duplicate body is deleted.
pub use crate::nmath::utils::*;
