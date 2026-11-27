#![forbid(unsafe_code)]
//! The pure-Rust object layer: generational `ValueId` handles into a direct
//! O(1) `Heap<Slot>` with typed `RObject` payloads, unified rooting, and
//! mark-and-sweep collection.
//!
//! This module is the target object model of the pure-Rust foundation plan
//! (docs/pure-rust-foundation-plan.md section 2.1). It shares no code with
//! the legacy `sexp` machinery and forbids unsafe code throughout.


pub mod id { pub use r_core::id::*; }
pub mod intern { pub use r_core::intern::*; }
pub mod object { pub use r_core::object::*; }
pub mod heap { pub use r_core::heap::*; }
pub mod gc { pub use r_core::gc::*; }
pub mod roots { pub use r_core::roots::*; }
pub mod rng { pub use r_core::rng::*; }
pub mod path_policy { pub use r_core::path_policy::*; }
pub mod graphics { pub use r_core::graphics::*; }
pub mod parser { pub use r_core::parser::*; }
pub mod env { pub use r_core::env::*; }
pub mod support { pub use r_core::support::*; }
pub mod eval { pub use r_core::eval::*; }
pub mod print { pub use r_core::print::*; }
pub mod regex { pub use r_core::regex::*; }
pub mod civil { pub use r_core::civil::*; }
pub mod rbg1 { pub use r_core::rbg1::*; }
pub mod vfile { pub use r_core::vfile::*; }
pub mod attrib { pub use r_core::attrib::*; }
pub mod primitives { pub use r_base::primitives::*; }
pub mod datasets { pub use r_base::datasets::*; }
pub mod session { pub use r_base::session::*; }
pub mod eval_tests;
pub mod conformance_sweep;

pub use heap::Heap;
pub use id::ValueId;
pub use intern::{InternedStr, SymId, SymbolInterner};
pub use object::{ObjectFlags, RObject, Slot};
pub use roots::{RootStack, RootTable, RootToken};
