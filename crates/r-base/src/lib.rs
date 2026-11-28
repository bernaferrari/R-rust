#![forbid(unsafe_code)]
//! The primitive registry: base, stats, and graphics builtins over the
//! r-core substrate. Staging crate of the rmath monolith split
//! (rport-c2v5f.15) — the domain separation into r-stats/r-graphics
//! follows as the next internal wave.

pub mod assets;
pub mod cpoly;
pub mod datasets;
pub mod session;
pub mod strptime;
pub mod supsmu;
pub mod primitives;

pub use primitives::{call_primitive, primitive_id_for_name, primitive_ids, primitive_names};
