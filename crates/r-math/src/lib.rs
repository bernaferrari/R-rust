//! rmath-nmath: Rust port of R's nmath statistical library.
//!
//! This crate is a standalone replacement for R's `libRmath.a`: faithful
//! translations of the nmath distribution routines (`dist`), special
//! functions (`special`), density/quantile/probability helpers (`dpq`,
//! `fprec`, `constants`, `error`, `utils`), and the standalone RNG facade,
//! with C-compatible FFI entrypoints.
//!
//! Session-owned state: the sampler continuation caches ([`MathState`]) and
//! the RNG seed stream ([`RngState`]) are owned by the host. The `rmath`
//! core-runtime crate embeds them in its per-session instance; standalone
//! consumers fall back to per-thread defaults.

#![forbid(unsafe_code)]
// C-to-Rust translation conventions (R's nmath library uses C naming)
// Enable all clippy lints including pedantic
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
// --- Pervasive C-port patterns (organized by category) ---
//
// This crate is a direct C-to-Rust translation of R's nmath statistical
// library. The following allows suppress lints that fire hundreds/thousands
// of times due to inherent C coding patterns. Safety/correctness lints
// (unwrap_used, transmutes, eq_op, etc.) are intentionally NOT suppressed.

// Numeric casts: pervasive in C math code (f64/i32/i64/usize interop)
// C-port completeness: the crate mirrors libRmath.a's full public surface;
// entrypoints not yet consumed by the rmath host remain as part of the
// standalone-library contract.
// Pointer operations: inherent to C FFI and memory allocation patterns
// Math code style: single-char variables, precise constants, float comparisons
// Older C-port tests assert panic paths without stable panic strings.
// Documentation: C code doesn't follow Rust doc conventions
// adding 1600+ boilerplate "# Safety" sections adds no real safety value.
// C control flow idioms: direct translations from C if/else/match patterns
// C expression/closure patterns
// Naming and structure: C conventions don't follow Rust idioms
// Function size: C translation produces long functions with many params
// Container and loop forms: retain C/Fortran-translated storage and iteration
// shapes where pointer identity, stable addresses, or source diffability matter.
// Miscellaneous pervasive C-port patterns

pub mod constants;
pub mod diagnostics;
pub mod dist;
pub mod dpq;
pub mod error;
pub mod fprec;
pub mod rng;
pub mod special;
pub mod state;
pub mod utils;

#[cfg(test)]

pub use rng::RngState;
pub use state::MathState;
