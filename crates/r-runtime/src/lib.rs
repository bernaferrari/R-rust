//! rmath: Rust port of R's core runtime.
//!
//! This crate is a full Rust port of R's core runtime — parser, evaluator,
//! SEXP arena and object model, base/stats library slices, and the
//! Android/embedding facade. The faithful translations of the nmath
//! statistical routines (`dist`, `dpq`, `special`, `nmath`) live in the
//! dedicated `r-math` crate and are re-exported here as `r_runtime::nmath`.
//! See `docs/rust-r-port-architecture.md` for the layering and ownership
//! model.

// C-to-Rust translation conventions (R's nmath library uses C naming)
#![forbid(unsafe_code)]
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
// Pointer operations: inherent to C FFI and memory allocation patterns
// Math code style: single-char variables, precise constants, float comparisons
// Older C-port tests assert panic paths without stable panic strings.
// Documentation: C code doesn't follow Rust doc conventions
// Safety docs: every unsafe fn in this C FFI port operates on raw SEXP pointers;
// adding 1600+ boilerplate "# Safety" sections adds no real safety value.
// C control flow idioms: direct translations from C if/else/match patterns
// C expression/closure patterns
// Naming and structure: C conventions don't follow Rust idioms
// Function size: C translation produces long functions with many params
// Container and loop forms: retain C/Fortran-translated storage and iteration
// shapes where pointer identity, stable addresses, or source diffability matter.
// Miscellaneous pervasive C-port patterns
#![warn(unused_unsafe)]

pub mod embed;
pub mod host;

pub use r_math as nmath;
pub use value::support::{MAX_BROWSER_FILE_BYTES, MAX_BROWSER_FILE_PATH_BYTES};

pub mod value;

