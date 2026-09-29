//! Checked element mutation over a crate-internal SEXP handle.
//!
//! Checked handles support safe mutation with copied reads. Acquiring a guard
//! from an unknown raw handle is unsafe: consuming a clone does not prove
//! uniqueness or revoke borrowed slices. The caller must exclude borrowed
//! payload views throughout the guard's lifetime and keep the object alive.
//! Setters then check type and bounds and take `&mut self`.
//!
//! The guard has no `Deref` to `Sexp`, so it cannot create shared slice views
//! or clone the underlying handle during the mutation window. `freeze()`
//! consumes the guard and returns a read handle. Existing raw C accessors
//! remain a separate unsafe boundary.

use std::os::raw::{c_double, c_int};

use super::{Sexp, SexpResult};
use crate::sexp::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};

/// Mutation authority acquired after the caller excludes borrowed payloads.
/// This is not a uniqueness proof over sibling raw handles.
#[derive(Debug)]
pub struct SexpMut<'a> {
    inner: Sexp<'a>,
}

// The delegating setters call the deprecated `Sexp` compat shims because
// they are the single source of truth for the bounds/type checks. SexpMut
// is the blessed mutation path, so the deprecation lint is noise here.
#[allow(deprecated)]
impl<'a> SexpMut<'a> {
    /// Mutate a checked arena or session value using copied element access.
    ///
    /// Checked factories retain the allocation and safe reads never lend Rust
    /// payload references. Unsafe payload loans must exclude all mutation,
    /// including this API. Unknown raw handles and immutable sentinels fail.
    pub fn try_from_checked(sexp: Sexp<'a>) -> SexpResult<Self> {
        if matches!(
            sexp.owner(),
            super::SexpOwner::Arena(_) | super::SexpOwner::Session(_)
        ) {
            Ok(Self { inner: sexp })
        } else {
            Err(super::SexpError::UncheckedMutation)
        }
    }

    /// Acquire mutation authority over an existing handle.
    ///
    /// # Safety
    /// The object must remain live for `'a`. No Rust reference into its
    /// payload may exist or be created while this guard is held. Raw aliases
    /// may exist, but cannot access the payload concurrently with a write.
    #[inline]
    pub(crate) unsafe fn from_owned(sexp: Sexp<'a>) -> Self {
        assert!(
            !matches!(sexp.owner(), super::SexpOwner::Static),
            "immutable singleton cannot be mutated"
        );
        Self { inner: sexp }
    }

    /// Consume the guard and return a read handle.
    #[inline]
    pub fn freeze(self) -> Sexp<'a> {
        self.inner
    }

    /// Get the underlying raw `SEXP` pointer for FFI handoff.
    #[inline]
    pub fn as_raw(&self) -> SEXP {
        self.inner.clone().as_raw()
    }

    /// Copy the guarded object's type tag.
    #[inline]
    pub fn typeof_(&self) -> SEXPTYPE {
        self.inner.typeof_()
    }

    /// Copy the vector length (zero for non-vector types).
    #[inline]
    pub fn len(&self) -> R_xlen_t {
        self.inner.len()
    }

    /// Check whether the guarded value has length zero.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Copy an element without lending a reference into the payload.
    pub fn integer_elt(&self, i: R_xlen_t) -> Option<c_int> {
        self.inner.integer_elt(i)
    }

    /// Copy an element without lending a reference into the payload.
    pub fn try_real_elt(&self, i: R_xlen_t) -> SexpResult<c_double> {
        self.inner.try_real_elt(i)
    }

    pub fn is_vector(&self) -> bool {
        self.inner.is_vector()
    }

    /// Set the i-th logical value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_logical_elt(&mut self, i: R_xlen_t, v: c_int) -> bool {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().set_logical_elt(i, v) }
    }

    /// Set the i-th logical value with typed error reporting.
    #[inline]
    pub fn try_set_logical_elt(&mut self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().try_set_logical_elt(i, v) }
    }

    /// Set the i-th integer value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_integer_elt(&mut self, i: R_xlen_t, v: c_int) -> bool {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().set_integer_elt(i, v) }
    }

    /// Set the i-th integer value with typed error reporting.
    #[inline]
    pub fn try_set_integer_elt(&mut self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().try_set_integer_elt(i, v) }
    }

    /// Set the i-th real (double) value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_real_elt(&mut self, i: R_xlen_t, v: c_double) -> bool {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().set_real_elt(i, v) }
    }

    /// Set the i-th real value with typed error reporting.
    #[inline]
    pub fn try_set_real_elt(&mut self, i: R_xlen_t, v: c_double) -> SexpResult<()> {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().try_set_real_elt(i, v) }
    }

    /// Set the i-th raw byte.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_raw_elt(&mut self, i: R_xlen_t, v: Rbyte) -> bool {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().set_raw_elt(i, v) }
    }

    /// Set the i-th raw byte with typed error reporting.
    #[inline]
    pub fn try_set_raw_elt(&mut self, i: R_xlen_t, v: Rbyte) -> SexpResult<()> {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().try_set_raw_elt(i, v) }
    }

    /// Set the i-th complex value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    #[inline]
    pub fn set_complex_elt(&mut self, i: R_xlen_t, v: Rcomplex) -> bool {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().set_complex_elt(i, v) }
    }

    /// Set the i-th complex value with typed error reporting.
    #[inline]
    pub fn try_set_complex_elt(&mut self, i: R_xlen_t, v: Rcomplex) -> SexpResult<()> {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().try_set_complex_elt(i, v) }
    }

    /// Set the i-th string element.
    ///
    /// Returns `false` if this is not a string vector, `v` is not CHARSXP,
    /// the index is out of bounds, or data pointer is null.
    #[inline]
    pub fn set_string_elt(&mut self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().set_string_elt(i, v) }
    }

    /// Set the i-th string element with typed error reporting.
    #[inline]
    pub fn try_set_string_elt(&mut self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().try_set_string_elt(i, v) }
    }

    /// Set the i-th vector element.
    ///
    /// Returns `false` if this is not a generic/expression vector, the
    /// index is out of bounds, or data pointer is null.
    #[inline]
    pub fn set_vector_elt(&mut self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().set_vector_elt(i, v) }
    }

    /// Set the i-th generic/expression vector element with typed error
    /// reporting.
    #[inline]
    pub fn try_set_vector_elt(&mut self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        // SAFETY: the acquisition contract excludes payload borrows.
        unsafe { self.inner.clone().try_set_vector_elt(i, v) }
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
        // SAFETY: fresh fixture, with no live borrowed payload views.
        let mut mutable = unsafe { SexpMut::from_owned(sexp) };

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
        // SAFETY: fresh fixture, with no live borrowed payload views.
        let frozen = unsafe { SexpMut::from_owned(sexp) }.freeze();
        assert_eq!(frozen.typeof_(), SEXPTYPE::REALSXP);
        assert_eq!(frozen.as_raw(), raw);
    }

    #[test]
    fn try_set_reports_typed_errors() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 1)
            .expect("arena allocation failed");
        // SAFETY: fresh fixture, with no live borrowed payload views.
        let mut mutable = unsafe { SexpMut::from_owned(sexp) };

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
    fn guard_reads_copy_without_exposing_a_handle() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 2)
            .expect("arena allocation failed");
        // SAFETY: fresh fixture, with no live borrowed payload views.
        let mut mutable = unsafe { SexpMut::from_owned(sexp) };
        mutable.set_integer_elt(0, 1);

        // Explicit forwarding reads return scalar copies, never slices.
        assert_eq!(mutable.len(), 2);
        assert!(mutable.is_vector());
        assert_eq!(mutable.integer_elt(0), Some(1));

        assert_eq!(mutable.integer_elt(1), Some(0));
    }

    #[test]
    fn inherent_reads_match_shared_surface() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 2)
            .expect("arena allocation failed");
        // SAFETY: fresh fixture, with no live borrowed payload views.
        let mutable = unsafe { SexpMut::from_owned(sexp) };

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
        // SAFETY: fresh fixture, with no live borrowed payload views.
        let mut mutable = unsafe { SexpMut::from_owned(sexp) };
        mutable.set_integer_elt(0, 3);
        assert_eq!(mutable.integer_elt(0), Some(3));
    }

    #[test]
    fn frozen_handle_mutates_only_through_reconversion() {
        let mut arena = RArena::new();
        let sexp = arena
            .alloc_vector_sexp(SEXPTYPE::INTSXP, 1)
            .expect("arena allocation failed");

        // SAFETY: fresh fixture, with no live borrowed payload views.

        let frozen = unsafe { SexpMut::from_owned(sexp) }.freeze();
        // SAFETY: fresh fixture, with no live borrowed payload views.
        let mut again = unsafe { SexpMut::from_owned(frozen) };
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
        let ok = unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(0, 9)
        };
        assert!(ok);
        assert_eq!(sexp.integer_elt(0), Some(9));
    }

    #[test]
    fn shared_slice_borrow_ends_before_mutation() {
        let mut arena = RArena::new();
        let sexp = arena.alloc_vector_sexp(SEXPTYPE::REALSXP, 2).unwrap();
        let alias = sexp.clone();
        {
            let shared = unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                alias.as_real_slice()
            }
            .unwrap();
            assert_eq!(shared, &[0.0, 0.0]);
        }
        // SAFETY: all payload borrows have ended. Arena remains borrowed
        // by the handle, so the allocation is live through this write.
        let mut guard = unsafe { SexpMut::from_owned(sexp) };
        assert!(guard.set_real_elt(0, 42.5));
        drop(guard);
        assert_eq!(alias.real_elt(0), Some(42.5));
        assert_eq!(alias.real_elt(1), Some(0.0));
    }
}
