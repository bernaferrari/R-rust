//! Crate-internal mutation guard for an R SEXP.
//!
//! [`Sexp`] reads are shared and safe, but in-place mutation of R object
//! payloads (`SET_*` element writes, attribute reassignment) goes through
//! raw pointers, and a write observed through a shared slice view derived
//! from another handle is exactly the aliasing-undefined-behavior class
//! this crate forbids. [`SexpMut`] is the crate-internal guard that
//! centralizes those writes on one auditable path. It is NOT a uniqueness
//! proof:
//!
//! * [`Sexp`] is `Clone`, and every clone is an alias over the same R
//!   memory (see the [`Clone`] impl). Consuming one clone in
//!   [`SexpMut::from_owned`] proves nothing about sibling clones minted
//!   earlier, nor about `&'a [T]` slice views derived from them: a live
//!   shared real slice observes a guard write. The
//!   `documents_shared_slice_aliasing` test below pins this hole.
//! * The quarantine is therefore containment, not exclusivity:
//!   [`SexpMut::from_owned`] is `pub(crate)`, so outside the crate the
//!   pattern is unrepresentable — external code cannot mint a guard and
//!   hence cannot perform in-place mutation through the safe API at all.
//!   Inside the crate every guard construction site is auditable, and no
//!   long-lived shared slice view may be retained across a mutation
//!   window.
//!
//! What the guard does provide:
//!
//! * A single mutation path: every `set_*` / `try_set_*` element setter
//!   for new code lives here and takes `&mut self`, excluding concurrent
//!   borrows of the guard itself.
//! * Move discipline: construction takes the caller's handle by value and
//!   [`SexpMut::freeze`] hands it back, so at least the moved-from value
//!   cannot spawn new readers while the guard is live. The guard itself
//!   is not `Clone`, so the guard cannot be duplicated while it is live.
//! * Reads stay available through [`Deref`] to [`Sexp`]: type predicates,
//!   element reads, and child-handle accessors are shared-borrow safe.
//!   A small set of hot reads ([`SexpMut::len`], [`SexpMut::typeof_`],
//!   [`SexpMut::is_empty`]) is also forwarded as inherent methods so
//!   callers do not need a `Deref` step (or a premature [`SexpMut::freeze`])
//!   for the common length/type checks.
//! * Hand the value back to read-only code with [`SexpMut::freeze`], which
//!   converts the guard into a plain `Sexp` handle.
//!
//! # Examples
//!
//! The mutation flow (`from_owned` -> `set_*` -> `freeze()`). Guard
//! construction is crate-internal (`pub(crate)`), so this is illustrative
//! only and ignored as a doctest:
//!
//! ```ignore
//! use rmath::sexp::memory::RArena;
//! use rmath::sexp::object::SexpMut;
//! use rmath::sexp::SEXPTYPE;
//!
//! let mut arena = RArena::new();
//! let sexp = arena
//!     .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
//!     .expect("arena allocation failed");
//! let mut mutable = SexpMut::from_owned(sexp);
//! assert!(mutable.set_integer_elt(0, 7));
//!
//! // Shared reads resolve through Deref while the guard is live.
//! assert_eq!(mutable.integer_elt(0), Some(7));
//!
//! let readback = mutable.freeze();
//! assert_eq!(readback.integer_elt(0), Some(7));
//! ```
//!
//! Mutating through a non-`mut` binding is rejected at compile time, and
//! the forbidden aliasing patterns remain pinned by the
//! `tests/compile_fail` suite.
//!
//! # Full split contract
//!
//! This module ships the interim step of a `SexpRef` / `SexpMut` split.
//! [`SexpRef`] exists as a shared-handle alias for [`Sexp`] (see
//! `sexp/object/mod.rs`); every shared handle is still the same
//! pointer-identity `Sexp` underneath.
//!
//! ## (a) Quarantine: non-`Copy` `Sexp` + crate-internal `SexpMut` guard
//!
//! * [`Sexp`] is intentionally not `Copy` (`sexp/object/mod.rs` documents
//!   the rationale and pins it with a trait-ambiguity compile-time guard
//!   plus `tests/compile_fail` move-semantics cases). Cloning is explicit
//!   and cheap (same pointer, same owner token), and every clone is
//!   understood to be an alias.
//! * [`SexpMut::from_owned`] consumes the caller's handle by value. Once
//!   moved in, no second handle is reachable *from that value* — but that
//!   is all it proves. Sibling clones minted before the move, and shared
//!   `&'a [T]` slice views derived from them, keep aliasing the same R
//!   memory, and a guard write is visible through them (pinned by the
//!   `documents_shared_slice_aliasing` test below). Do NOT reason about
//!   `SexpMut` as uniqueness: it is a crate-internal mutation guard that
//!   centralizes writes on one auditable path, nothing more.
//! * The guard itself is not `Clone`, so the guard value cannot be
//!   duplicated while it is live; [`SexpMut::freeze`] consumes the guard
//!   and returns the plain `Sexp` handle to read-only code.
//! * Why `Deref` is still present: `Deref` only yields `&Sexp` shared
//!   reads (type predicates, `*_elt` element reads, child-handle
//!   accessors), which never write through the raw pointer. All writes
//!   require `&mut self` on the guard and therefore exclude any
//!   concurrent borrow of the guard itself. Borrow-exclusion on the
//!   guard is real; memory-exclusion on the R object is not claimed.
//! * The deprecated by-value `set_*` / `try_set_*` shims on `Sexp` (twelve
//!   in `sexp/object/vector.rs`: logical, integer, real, raw, string, and
//!   vector element setters plus their `try_*` variants; two more in
//!   `sexp/object/slots.rs`: the complex pair) remain only as
//!   translation-compat shims for already-ported C-shaped code. Their
//!   discipline story is move semantics: each takes `self` by value.
//!   New in-crate code must use `SexpMut::from_owned(..)` -> `set_*` ->
//!   `freeze()`.
//! * Quarantine boundary: [`SexpMut::from_owned`] is `pub(crate)`, so
//!   external crates cannot construct the guard and cannot perform
//!   in-place mutation through the safe API. Audit rule for in-crate
//!   construction sites: never retain a live clone or a derived shared
//!   slice view across a guard mutation window.
//!
//! ## (b) Target: `SexpRef` / `SexpMut` with no `Deref` short-circuit
//!
//! * `SexpRef` is the shared borrow handle (`&`-like): freely
//!   reborrowable, exposing the full read surface (`typeof_`, `len`,
//!   predicates, `*_elt` / `try_*_elt` element reads, slice views,
//!   child-handle accessors) and no mutation surface.
//! * `SexpMut` is the crate-internal mutation handle: constructed by
//!   moving a handle in (`from_owned`, `pub(crate)`), mutating through
//!   `&mut self` setters, and handing back via `freeze()`. Its own small
//!   inherent read surface (length/type checks such as `len` / `typeof_`)
//!   exists so write loops can inspect the object without freezing early.
//! * No `Deref` between the two: reads will be spelled explicitly on each
//!   handle instead of falling through from `SexpMut` to `Sexp`. Method
//!   resolution will therefore tell the reader whether a call ran against
//!   the guard (inherent method) or a shared handle, and removing
//!   the shared reborrow will be a visible code change rather than a silent
//!   deref-chain edit.
//!
//! ## (c) Migration steps and done-condition
//!
//! Ordered steps:
//!
//! 1. Migrate the remaining deprecated call sites to the guard: the
//!    translated write loops under `#![allow(deprecated)]` in
//!    `eval/arithmetic.rs` and `eval/bytecode.rs` (plus any stragglers in
//!    tests still exercising the shims) become
//!    `SexpMut::from_owned(..)` -> `set_*` / `try_set_*` -> `freeze()`.
//!    (Done: both modules build through the guard on same-crate paths.)
//! 2. Remove `impl Deref for SexpMut`, extending the inherent
//!    `Deref`-free forwarding reads (seeded here with `len`, `is_empty`,
//!    and `typeof_`) to cover whatever shared reads guard-held write loops
//!    still need, so no caller must `freeze()` prematurely just to read.
//! 3. Keep `from_owned` `pub(crate)` and keep the `SexpRef` shared-handle
//!    alias pointed at the read-only surface; a future true-uniqueness
//!    design (if any) must first close the sibling-clone / shared-slice
//!    hole documented by `documents_shared_slice_aliasing`.
//!
//! Done-condition: [`SexpMut::from_owned`] is `pub(crate)` (external
//! construction unrepresentable — `tests/compile_fail/
//! sexp_mut_requires_mutable_binding.rs` must be reworked to expect E0624
//! instead of E0596; it is outside this change's file set), no
//! `#![allow(deprecated)]` / `#[allow(deprecated)]` kept alive for the
//! `Sexp` `set_*` shims outside this module's internal delegation, no
//! `Deref` impl on `SexpMut`, the `SexpRef` shared-handle alias present
//! with read-only code using it, the `documents_shared_slice_aliasing`
//! regression test green (documenting the quarantined hole), and `cargo
//! build -p rmath` plus the `tests/compile_fail` suite green once that
//! case file is updated.

use std::ops::Deref;
use std::os::raw::{c_double, c_int};

use super::{Sexp, SexpResult};
use crate::sexp::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};

/// Crate-internal mutation guard for an R SEXP.
///
/// Centralizes in-place element writes on one auditable path; it is NOT a
/// uniqueness proof. [`Sexp`] is `Clone` and every clone aliases the same
/// R memory, so consuming one clone in [`SexpMut::from_owned`] says
/// nothing about sibling clones or shared `&[T]` slice views derived from
/// them — a guard write is observable through such a view (see the
/// `documents_shared_slice_aliasing` test). Construction is therefore
/// `pub(crate)`: outside the crate the mutating pattern is
/// unrepresentable, and inside the crate no live clone or derived shared
/// slice may be retained across a mutation window.
///
/// Every mutation takes `&mut self`, and [`freeze`](SexpMut::freeze)
/// converts the guard back to a shared, read-only [`Sexp`].
///
/// `SexpMut` is deliberately not `Clone`: cloning would mint a second
/// guard while a mutation window is live. The deprecated `set_*` methods on
/// `Sexp` remain only as compatibility shims for existing translated code;
/// new in-crate code mutates here.
#[derive(Debug)]
pub struct SexpMut<'a> {
    inner: Sexp<'a>,
}

// The delegating setters call the deprecated `Sexp` compat shims because
// they are the single source of truth for the bounds/type checks. SexpMut
// is the blessed mutation path, so the deprecation lint is noise here.
#[allow(deprecated)]
impl<'a> SexpMut<'a> {
    /// Take a handle for in-place mutation (crate-internal).
    ///
    /// Consuming the handle only removes *that value* from the caller's
    /// reach: sibling clones minted earlier, and shared `&[T]` slice views
    /// derived from them, still alias the same R memory, so this is NOT a
    /// uniqueness proof. `pub(crate)` visibility is the quarantine — only
    /// audited in-crate construction sites may hold a guard, and they must
    /// not retain a live clone or derived shared slice across the
    /// mutation window.
    #[inline]
    pub(crate) fn from_owned(sexp: Sexp<'a>) -> Self {
        Self { inner: sexp }
    }

    /// Convert the exclusive guard back into a shared, read-only handle.
    #[inline]
    pub fn freeze(self) -> Sexp<'a> {
        self.inner
    }

    /// Get the underlying raw `SEXP` pointer for FFI handoff.
    #[inline]
    pub fn as_raw(&self) -> SEXP {
        self.inner.clone().as_raw()
    }

    /// Get the type of the guarded SEXP without going through [`Deref`].
    ///
    /// Thin shared-borrow forward to [`Sexp::typeof_`]: write loops that
    /// need a type check while the guard is live can call this instead of
    /// freezing early (or relying on the `Deref` short-circuit that the
    /// full `SexpRef` / `SexpMut` split will remove).
    #[inline]
    pub fn typeof_(&self) -> SEXPTYPE {
        self.inner.typeof_()
    }

    /// Get the length of the guarded vector SEXP without going through
    /// [`Deref`].
    ///
    /// Thin shared-borrow forward to [`Sexp::len`] (0 for non-vector
    /// types); exists so result-fill loops can bound themselves while the
    /// guard is live.
    #[inline]
    pub fn len(&self) -> R_xlen_t {
        self.inner.len()
    }

    /// Check whether the guarded SEXP is empty without going through
    /// [`Deref`].
    ///
    /// Thin shared-borrow forward to [`Sexp::is_empty`].
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Set the i-th logical value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_logical_elt(&mut self, i: R_xlen_t, v: c_int) -> bool {
        self.inner.clone().set_logical_elt(i, v)
    }

    /// Set the i-th logical value with typed error reporting.
    #[inline]
    pub fn try_set_logical_elt(&mut self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        self.inner.clone().try_set_logical_elt(i, v)
    }

    /// Set the i-th integer value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_integer_elt(&mut self, i: R_xlen_t, v: c_int) -> bool {
        self.inner.clone().set_integer_elt(i, v)
    }

    /// Set the i-th integer value with typed error reporting.
    #[inline]
    pub fn try_set_integer_elt(&mut self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        self.inner.clone().try_set_integer_elt(i, v)
    }

    /// Set the i-th real (double) value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_real_elt(&mut self, i: R_xlen_t, v: c_double) -> bool {
        self.inner.clone().set_real_elt(i, v)
    }

    /// Set the i-th real value with typed error reporting.
    #[inline]
    pub fn try_set_real_elt(&mut self, i: R_xlen_t, v: c_double) -> SexpResult<()> {
        self.inner.clone().try_set_real_elt(i, v)
    }

    /// Set the i-th raw byte.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_raw_elt(&mut self, i: R_xlen_t, v: Rbyte) -> bool {
        self.inner.clone().set_raw_elt(i, v)
    }

    /// Set the i-th raw byte with typed error reporting.
    #[inline]
    pub fn try_set_raw_elt(&mut self, i: R_xlen_t, v: Rbyte) -> SexpResult<()> {
        self.inner.clone().try_set_raw_elt(i, v)
    }

    /// Set the i-th complex value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_complex_elt(&mut self, i: R_xlen_t, v: Rcomplex) -> bool {
        self.inner.clone().set_complex_elt(i, v)
    }

    /// Set the i-th complex value with typed error reporting.
    #[inline]
    pub fn try_set_complex_elt(&mut self, i: R_xlen_t, v: Rcomplex) -> SexpResult<()> {
        self.inner.clone().try_set_complex_elt(i, v)
    }

    /// Set the i-th string element.
    ///
    /// Returns `false` if this is not a string vector, `v` is not CHARSXP,
    /// the index is out of bounds, or data pointer is null.
    #[inline]
    pub fn set_string_elt(&mut self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        self.inner.clone().set_string_elt(i, v)
    }

    /// Set the i-th string element with typed error reporting.
    #[inline]
    pub fn try_set_string_elt(&mut self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        self.inner.clone().try_set_string_elt(i, v)
    }

    /// Set the i-th vector element.
    ///
    /// Returns `false` if this is not a generic/expression vector, the
    /// index is out of bounds, or data pointer is null.
    #[inline]
    pub fn set_vector_elt(&mut self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        self.inner.clone().set_vector_elt(i, v)
    }

    /// Set the i-th generic/expression vector element with typed error
    /// reporting.
    #[inline]
    pub fn try_set_vector_elt(&mut self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        self.inner.clone().try_set_vector_elt(i, v)
    }
}

impl<'a> Deref for SexpMut<'a> {
    type Target = Sexp<'a>;

    #[inline]
    fn deref(&self) -> &Sexp<'a> {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::SEXPTYPE;
    use crate::sexp::memory::RArena;
    use crate::sexp::object::SexpError;

    #[test]
    fn sexp_mut_mutates_in_place() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
            .expect("arena allocation failed");
        let mut mutable = SexpMut::from_owned(sexp);

        assert!(mutable.set_integer_elt(0, 42));
        assert!(mutable.set_integer_elt(2, -7));
        assert!(!mutable.set_integer_elt(3, 0), "out of bounds must fail");

        let readback = mutable.freeze();
        assert_eq!(readback.integer_elt(0), Some(42));
        assert_eq!(readback.integer_elt(1), Some(0));
        assert_eq!(readback.integer_elt(2), Some(-7));
    }

    #[test]
    fn freeze_preserves_object_identity() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::REALSXP, 1)
            .expect("arena allocation failed");
        let raw = sexp.clone().as_raw();
        let frozen = SexpMut::from_owned(sexp).freeze();
        assert_eq!(frozen.typeof_(), SEXPTYPE::REALSXP);
        assert_eq!(frozen.as_raw(), raw);
    }

    #[test]
    fn try_set_reports_typed_errors() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 1)
            .expect("arena allocation failed");
        let mut mutable = SexpMut::from_owned(sexp);

        assert!(matches!(
            mutable.try_set_real_elt(0, 1.0),
            Err(SexpError::TypeMismatch { .. })
        ));
        assert!(matches!(
            mutable.try_set_integer_elt(5, 1),
            Err(SexpError::OutOfBounds { .. })
        ));
        assert!(mutable.try_set_integer_elt(0, 5).is_ok());
    }

    #[test]
    fn deref_exposes_shared_reads() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 2)
            .expect("arena allocation failed");
        let mut mutable = SexpMut::from_owned(sexp);
        mutable.set_integer_elt(0, 1);

        // Method calls auto-deref to the shared `Sexp` read surface.
        assert_eq!(mutable.len(), 2);
        assert!(mutable.is_vector());
        assert_eq!(mutable.integer_elt(0), Some(1));

        // Explicit reborrow also reads through the guard.
        let shared: &Sexp<'_> = &mutable;
        assert_eq!(shared.len(), 2);
        assert_eq!(shared.integer_elt(1), Some(0));
        assert_eq!(*shared, *shared);
    }

    #[test]
    fn inherent_reads_match_shared_surface() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 2)
            .expect("arena allocation failed");
        let mutable = SexpMut::from_owned(sexp);

        // Inherent Deref-free forwards agree with the shared `Sexp` reads,
        // so write loops can check length/type without freezing early.
        assert_eq!(mutable.len(), mutable.inner.len());
        assert_eq!(mutable.typeof_(), SEXPTYPE::INTSXP);
        assert_eq!(mutable.typeof_(), mutable.inner.typeof_());
        assert_eq!(mutable.is_empty(), mutable.inner.is_empty());
        assert!(!mutable.is_empty());
    }

    #[test]
    fn mutation_requires_a_mutable_binding() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 1)
            .expect("arena allocation failed");
        let mut mutable = SexpMut::from_owned(sexp);
        mutable.set_integer_elt(0, 3);
        assert_eq!(mutable.integer_elt(0), Some(3));
    }

    #[test]
    fn frozen_handle_mutates_only_through_reconversion() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 1)
            .expect("arena allocation failed");

        let frozen = SexpMut::from_owned(sexp).freeze();
        let mut again = SexpMut::from_owned(frozen);
        assert!(again.set_integer_elt(0, 3));
        assert_eq!(again.freeze().integer_elt(0), Some(3));
    }

    #[test]
    fn deprecated_sexp_setters_remain_working_shims() {
        // The by-value `Sexp` setters are deprecated but still functional
        // for the internal translated code; the exclusivity story for them
        // is move semantics (see tests/compile_fail/).
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 1)
            .expect("arena allocation failed");
        #[allow(deprecated)]
        let ok = sexp.clone().set_integer_elt(0, 9);
        assert!(ok);
        assert_eq!(sexp.integer_elt(0), Some(9));
    }

    #[test]
    fn documents_shared_slice_aliasing() {
        // QUARANTINED HOLE: `Sexp` is `Clone` and every clone aliases the
        // same R memory, so consuming one clone in `SexpMut::from_owned`
        // proves nothing about sibling clones or the shared `&[T]` slice
        // views derived from them: a guard write is observable through a
        // live shared slice. This test pins that behavior. The quarantine
        // (`from_owned` is `pub(crate)`) makes this pattern
        // unrepresentable from outside the crate; in-crate construction
        // sites must never retain a live clone or a derived shared slice
        // across a mutation window.
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::REALSXP, 2)
            .expect("arena allocation failed");
        let alias = sexp.clone();
        let shared = alias.as_real_slice().expect("real vector");
        assert_eq!(shared, &[0.0, 0.0]);

        let mut guard = SexpMut::from_owned(sexp);
        assert!(guard.set_real_elt(0, 42.5));

        // The write through the guard is visible through the shared slice:
        // no uniqueness is enforced.
        assert_eq!(shared[0], 42.5);
        assert_eq!(shared[1], 0.0);
    }
}
