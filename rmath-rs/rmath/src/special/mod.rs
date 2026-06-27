// Re-export the canonical `nmath::special` implementations.
//
// `crate::nmath::special` is the faithful upstream `src/nmath/special/` mirror
// and is a superset of the top-level `special` copy (it additionally has
// `beta_util.rs`). There are no cross-tree import entanglements here (each side
// references its own tree), so callers keep using `crate::special::*` and the
// symbols now resolve to the single canonical source. The special/*.rs bodies
// are no longer compiled.
//
// Refs rport-vy1h, rport-ee8d.
pub use crate::nmath::special::*;
