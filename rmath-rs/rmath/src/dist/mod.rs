// Re-export the canonical `nmath::dist` implementations.
//
// R's distribution math lives in `src/nmath/`; `crate::nmath::dist` is the
// faithful upstream mirror and is canonical. The top-level `dist` module
// historically held a divergent copy (it lacked qnbinom_inner, which nmath
// imported back from here; and beta.rs had a truncated M_LN4 literal, fixed in
// 2bd4d41c). Callers keep using `crate::dist::*`; symbols now resolve to the
// single canonical source. The dist/*.rs bodies are no longer compiled.
//
// Refs rport-vy1h, rport-x89b.
pub use crate::nmath::dist::*;
