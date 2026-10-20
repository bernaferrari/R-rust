#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Memory allocation for R objects.
//!
//! # Arena + generational GC
//!
//! Objects are allocated from a slab arena ([`RArena`]) that owns node pages
//! and vector payloads. That arena is **not** a "no-GC forever" pool: unreachable
//! objects are reclaimed by the generational mark-sweep collector in
//! [`super::gengc`]. Allocation may trigger collection when node/byte
//! thresholds trip; survivors promote young→old under write-barrier tracking.
//!
//! # Mandatory rooting
//!
//! Raw `SEXP` values held only in Rust locals are invisible to the collector.
//! Any pointer that must survive an allocating call (or an explicit `gc()`)
//! has to be rooted via [`super::protect`] (legacy PROTECT stack or the Rust
//! root table) or another traced root (environments, the bytecode operand
//! stack, remembered-set edges). Dropping roots because "there is no GC" is
//! use-after-free — do not treat this module as GC-free.

mod bytecode_strings;
mod characters;

use super::ffi::{NodeBody, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, SexprecCore};
use super::heap::{
    CheckedNode, HeapBackingOwners, HeapIdentity, NodeId, NodeLink, NodePage, NodeProjection,
    PageMetadata,
};
use super::object::Sexp;
use super::payload::{OwnedPayload, PayloadError, PayloadLease, PayloadLink};
use std::alloc::Layout;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ptr::{self};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod retention_tests;

#[cfg(test)]
#[path = "memory/character_cache_tests.rs"]
mod character_cache_tests;

#[cfg(test)]
#[path = "memory/bytecode_string_tests.rs"]
mod bytecode_string_tests;

mod arena;
mod budget;
mod checked;
mod gc;
mod layout;
mod ledger;
mod page_store;
mod slab;
mod slab_slots;

#[cfg(test)]
mod tests_arena;

#[cfg(test)]
mod tests_physical_headers;

#[cfg(test)]
mod tests_payload;

#[cfg(test)]
mod tests_payload_index;

#[cfg(test)]
mod tests_bookkeeping;

#[cfg(test)]
mod tests_page_descriptors;

#[allow(unused_imports)]
pub use self::arena::*;
#[allow(unused_imports)]
pub use self::budget::*;
#[allow(unused_imports)]
pub use self::checked::*;
#[allow(unused_imports)]
pub use self::gc::*;
#[allow(unused_imports)]
pub use self::layout::*;
pub(crate) use self::ledger::*;
#[allow(unused_imports)]
pub use self::slab::*;
