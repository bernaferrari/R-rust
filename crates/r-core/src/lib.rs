#![forbid(unsafe_code)]
//! The pure-Rust object model foundation: generational `ValueId` handles
//! into a direct O(1) `Heap<Slot>` with typed `RObject` payloads, unified
//! rooting, mark-and-sweep collection, the heap-owned session state types
//! (RNG, path policy, evaluation context frames), and the per-device
//! graphics parameter state machine.
//!
//! This crate is the target foundation of the pure-Rust foundation plan
//! (docs/pure-rust-foundation-plan.md section 2.1) — the first extraction
//! of the rmath monolith split (rport-c2v5f.15).

pub mod env;
pub mod eval;
pub mod gc;
pub mod graphics;
pub mod heap;
pub mod id;
pub mod intern;
pub mod object;
pub mod parser;
pub mod path_policy;
pub mod rng;
pub mod roots;
pub mod support;

pub use heap::Heap;
pub use id::ValueId;
pub use intern::{InternedStr, SymId, SymbolInterner};
pub use object::{ObjectFlags, RObject, Slot};
