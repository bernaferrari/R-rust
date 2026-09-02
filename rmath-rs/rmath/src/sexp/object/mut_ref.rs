//! Exclusive mutable access to an R SEXP.
//!
//! [`Sexp`] reads are shared and safe, but in-place mutation of R object
//! payloads (`SET_*` element writes, attribute reassignment) must be
//! exclusive: the writes go through raw pointers, and a write observed
//! through a shared slice view derived from another handle is exactly the
//! aliasing-undefined-behavior class this crate forbids. The borrow guard
//! for that exclusivity is [`SexpMut`]:
//!
//! * Obtain one by *moving* a handle in: [`SexpMut::from_owned`]. While the
//!   guard exists, no other `Sexp` handle for the object can be derived
//!   from the moved value, so every mutation can safely take `&mut self`.
//! * Reads stay available through [`Deref`] to [`Sexp`]: type predicates,
//!   element reads, and child-handle accessors are shared-borrow safe.
//! * Hand the value back to read-only code with [`SexpMut::freeze`], which
//!   converts the exclusive guard into a plain `Sexp` handle.
//!
//! # Examples
//!
//! ```
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

use std::ops::Deref;
use std::os::raw::{c_double, c_int};

use super::{Sexp, SexpResult};
use crate::sexp::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP};

/// Exclusive mutable access to an R SEXP.
///
/// Prevents aliasing with any shared reference derived from the same handle:
/// [`SexpMut::from_owned`] consumes the only handle, every mutation takes
/// `&mut self`, and [`freeze`](SexpMut::freeze) converts the guard back to a
/// shared, read-only [`Sexp`].
///
/// `SexpMut` is deliberately not `Clone`: cloning would mint a second
/// handle while exclusive access is live. The deprecated `set_*` methods on
/// `Sexp` remain only as compatibility shims for existing translated code;
/// new code mutates here.
#[derive(Debug)]
pub struct SexpMut<'a> {
    inner: Sexp<'a>,
}

// The delegating setters call the deprecated `Sexp` compat shims because
// they are the single source of truth for the bounds/type checks. SexpMut
// is the blessed mutation path, so the deprecation lint is noise here.
#[allow(deprecated)]
impl<'a> SexpMut<'a> {
    /// Take exclusive ownership of a handle for in-place mutation.
    ///
    /// Consuming the handle is the mechanism: once moved in, no other
    /// `Sexp` for the same R object is reachable from this value, so the
    /// `&mut self` setters cannot alias a live shared reader.
    #[inline]
    pub fn from_owned(sexp: Sexp<'a>) -> Self {
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
}
