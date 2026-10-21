#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
#![deny(unsafe_op_in_unsafe_fn)]

//! R's PROTECT/UNPROTECT mechanism, split into two storages.
//!
//! R maintains a protection stack to prevent GC from collecting objects that
//! are only referenced by local variables. This port keeps TWO storages with
//! deliberately different disciplines:
//!
//! # Two storages, two disciplines
//!
//! * **[`LegacyProtectionStack`]** — the C-port stack: a strict LIFO
//!   ordered, checked allocations for translated `Rf_protect` / `Rf_unprotect` /
//!   `UNPROTECT_PTR` / `R_ProtectCount` code ([`protect_raw_pointer`],
//!   [`unprotect_count`], [`unprotect_ptr`], [`protect_n`]). Release is
//!   COUNT-BASED: `Rf_unprotect(n)` truncates the top `n` entries and later
//!   entries shift down, exactly like C R. [`R_ProtectCount`] reflects ONLY
//!   this stack's depth. RAII wrappers over it ([`protect_n`]) must unwind
//!   in LIFO order — the same discipline as the C code they translate; a
//!   violation is a caller bug, not a detected error (as in C).
//! * **[`RootTable`]** — the Rust root table: stable, generational slots for
//!   Rust-side handles ([`ProtectGuard`] from [`protect_sexp`] / [`protect`],
//!   [`IndexedProtectGuard`] / [`protect_sexp_with_index`], [`RootedSexp`],
//!   and the `R_ProtectWithIndex` shim). Slots are released BY SLOT ID +
//!   GENERATION in ANY order; a released slot becomes vacant in place and its
//!   index is recycled via a free list,
//!   so surviving guards keep resolving to their own entries and a stale
//!   handle is detectable ([`ProtectionSlot::is_stale`]) instead of silently
//!   aliasing another entry's protection.
//!
//! The two storages never alias: count-based legacy ops cannot truncate root
//! slots and root release never shifts legacy entries. The collector (gengc)
//! explicitly scans BOTH storages at mark and update time.
//!
//! # Ownership model
//!
//! * [`Sexp`](super::object::Sexp) handles are **non-`Copy`**: assigning a
//!   handle moves it, and aliasing the same R object requires an explicit
//!   [`Clone`](Sexp::clone) (a cheap second handle over identical memory,
//!   never a deep copy).
//! * Holding a handle alone does **not** root the object: the non-moving /
//!   generational GC may collect anything only reachable from Rust locals
//!   once an R evaluation re-enters. To retain a value across a GC point,
//!   push it on a protection storage.
//! * [`RootedSexp`] is the ergonomic RAII rooting layer: it clones the
//!   handle, roots it on creation, and releases the root on [`Drop`],
//!   exposing reads through [`RootedSexp::get`]. Lower-level callers can use
//!   [`protect_sexp`]/[`ProtectGuard`] or the replaceable-slot
//!   [`protect_sexp_with_index`]/[`IndexedProtectGuard`] directly.
//! * Handles to objects that may be *replaced* during evaluation (grown
//!   vectors, PROMSXP re-promises) must be refreshed through the write
//!   barrier — re-derive the handle from its owner or use
//!   [`IndexedProtectGuard::reprotect_sexp`] on a slot-protected value —
//!   never by mutating a raw pointer in place.
//!
//! # Slot-stability contract (root table)
//!
//! The root table is a `Vec` with stable slot indices. Releasing a slot
//! (`IndexedProtectGuard` / [`RootedSexp`] / [`ProtectGuard`] drop)
//! makes its entry vacant and pushes the
//! index onto a per-instance free list instead of shifting later entries, so
//! guards may be dropped in **any order** — surviving guards keep resolving
//! to their own entries. A later slot push reuses a freed index with a fresh
//! generation. Every pushed entry is tagged with a generation from a
//! monotonic per-instance counter, so a slot handle whose entry was released
//! is detectable via [`ProtectionSlot::is_stale`] /
//! [`RootedSexp::is_stale`] instead of silently resolving to another entry's
//! protection. Freed TAIL slots collapse off the table (popped together with
//! their generation tags) so the physical Vec stays contiguous; interior
//! frees stay tombstoned beneath live entries.

use std::cell::RefCell;
use std::marker::PhantomData;

use super::ffi::SEXP;
use super::instance::{RInstance, with_required_current_instance};
use super::object::{Sexp, SexpOwner};

#[path = "../root_storage.rs"]
mod root_storage;
pub(crate) use root_storage::{RootStorage, SlotId, StorageError};
#[path = "../root_sequence.rs"]
mod root_sequence;
pub(crate) use root_sequence::{RootSequence, SequenceLease};

mod guards;
#[cfg(kani)]
mod root_table_kani;
mod stacks;
#[cfg(test)]
mod tests;

pub(crate) use guards::*;
pub(crate) use stacks::*;

#[cfg(test)]
mod sequence_budget_tests;
