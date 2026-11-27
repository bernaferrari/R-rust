#![forbid(unsafe_code)]
//! The pure-Rust object layer: generational `ValueId` handles into a direct
//! O(1) `Heap<Slot>` with typed `RObject` payloads, unified rooting, and
//! mark-and-sweep collection.
//!
//! This module is the target object model of the pure-Rust foundation plan
//! (docs/pure-rust-foundation-plan.md section 2.1). It shares no code with
//! the legacy `sexp` machinery and forbids unsafe code throughout.

pub mod env;
pub mod eval;
pub mod gc;
pub mod heap;
pub mod id;
pub mod intern;
pub mod object;
pub mod primitives;
pub mod print;
pub mod regex;
pub mod rng;
pub mod rbg1;
pub mod datasets;
pub mod graphics;
pub mod civil;
pub mod roots;
pub mod parser;
pub mod session;
pub mod path_policy;
pub mod vfile;

pub use heap::Heap;
pub use id::ValueId;
pub use intern::{InternedStr, SymId, SymbolInterner};
pub use object::{ObjectFlags, RObject, Slot};
pub use roots::{RootStack, RootTable, RootToken};
