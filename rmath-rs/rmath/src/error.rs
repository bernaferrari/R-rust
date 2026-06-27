// Re-export the canonical `nmath::error`.
//
// `crate::nmath::error` is the faithful upstream `nmath.h` error-handling
// mirror and is a superset: it includes the MATH_ERROR_CODE `ME_DOMAIN`
// early-return guard that the divergent top-level copy lacked, so the builtin
// error path now matches upstream R more closely. Callers keep using
// `crate::error::*` and symbols resolve to the single canonical source.
//
// Refs rport-vy1h, rport-ee8d.
pub use crate::nmath::error::*;
