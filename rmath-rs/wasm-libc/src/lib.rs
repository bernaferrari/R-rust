//! The `libc` dependency of `rmath`.
//!
//! `rmath`'s C-ported sources call the C library through the `libc` crate
//! surface (`libc::c_int`, `libc::snprintf`, `libc::FILE`, ...). This crate
//! provides that surface with one source per target:
//!
//! * **Native targets** re-export the crates.io [`libc`] unchanged — zero
//!   code, zero behavior difference (every item is the real system binding).
//! * **`wasm32-unknown-unknown`** has no operating system, so the crates.io
//!   crate exposes almost none of that surface. The `facade` module —
//!   implemented in pure Rust — provides exactly the items the rmath tree
//!   uses, with documented policy: real computation for string/math/printf
//!   semantics, allocator-backed `malloc`, clean failures for filesystem,
//!   environment, process, and socket services.
//!
//! This crate is a private path dependency of `rmath` (never published) and
//! exists so `rmath/Cargo.toml` can keep a single `libc` dependency whose
//! source is target-independent (a cargo requirement).

#[cfg(not(target_arch = "wasm32"))]
pub use real_libc::*;

#[cfg(target_arch = "wasm32")]
pub use facade::*;
#[cfg(target_arch = "wasm32")]
mod facade;
