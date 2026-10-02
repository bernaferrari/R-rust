use std::os::raw::{c_double, c_int};

use super::{Sexp, SexpError, SexpResult};
use crate::sexp::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NaString, R_NilValue};

#[allow(deprecated)] // deprecated Sexp set_* shims delegate to try_set_* shims
impl<'a> Sexp<'a> {
    // --- Vector element access with bounds checking ---

    /// Get the i-th logical value with bounds checking.
    ///
    /// Returns `None` if this is not a logical vector, the index is out of
    /// bounds, or the data pointer is null.
    #[inline]
    pub fn logical_elt(&self, i: R_xlen_t) -> Option<c_int> {
        self.try_logical_elt(i).ok()
    }

    /// Get the i-th logical value with typed error reporting.
    #[inline]
    pub fn try_logical_elt(&self, i: R_xlen_t) -> SexpResult<c_int> {
        let data = self.try_typed_data::<c_int>(SEXPTYPE::LGLSXP, "logical vector")?;
        let i = self.try_index(i)?;
        Ok(unsafe { *data.add(i) })
    }

    /// Get the i-th integer value with bounds checking.
    ///
    /// A compact sequence is computed from its formula and stays unallocated.
    /// Returns `None` if this is not an integer vector, the index is out of
    /// bounds, or a plain vector has no data pointer.
    #[inline]
    pub fn integer_elt(&self, i: R_xlen_t) -> Option<c_int> {
        self.try_integer_elt(i).ok()
    }

    /// Get the i-th integer value with typed error reporting.
    ///
    /// Compact sequences answer here, before any buffer allocation. Pointer
    /// access still goes through [`Self::try_typed_data`] and expands them.
    #[inline]
    pub fn try_integer_elt(&self, i: R_xlen_t) -> SexpResult<c_int> {
        match self.read_compact_int(i, false) {
            crate::sexp::altseq::LazyRead::Ready(value) => return Ok(value),
            crate::sexp::altseq::LazyRead::OutOfRange => {
                return Err(SexpError::OutOfBounds {
                    index: i,
                    len: self.len(),
                });
            }
            crate::sexp::altseq::LazyRead::Absent => {}
        }
        let data = self.try_typed_data::<c_int>(SEXPTYPE::INTSXP, "integer vector")?;
        let i = self.try_index(i)?;
        // SAFETY: `try_typed_data` returned the live buffer and `i` is in range.
        // The element is copied; no payload reference is returned.
        Ok(unsafe { data.add(i).read() })
    }

    /// Get the i-th real (double) value with bounds checking.
    ///
    /// A compact sequence is computed from its formula and stays unallocated.
    /// Returns `None` if this is not a real vector, the index is out of bounds,
    /// or a plain vector has no data pointer.
    #[inline]
    pub fn real_elt(&self, i: R_xlen_t) -> Option<c_double> {
        self.try_real_elt(i).ok()
    }

    /// Get the i-th real value with typed error reporting.
    ///
    /// Compact sequences answer here, before any buffer allocation.
    #[inline]
    pub fn try_real_elt(&self, i: R_xlen_t) -> SexpResult<c_double> {
        match self.read_compact_real(i, false) {
            crate::sexp::altseq::LazyRead::Ready(value) => return Ok(value),
            crate::sexp::altseq::LazyRead::OutOfRange => {
                return Err(SexpError::OutOfBounds {
                    index: i,
                    len: self.len(),
                });
            }
            crate::sexp::altseq::LazyRead::Absent => {}
        }
        let data = self.try_typed_data::<c_double>(SEXPTYPE::REALSXP, "real vector")?;
        let i = self.try_index(i)?;
        // SAFETY: `try_typed_data` returned the live buffer and `i` is in range.
        // The element is copied; no payload reference is returned.
        Ok(unsafe { data.add(i).read() })
    }

    /// Get the i-th raw byte with bounds checking.
    ///
    /// Returns `None` if this is not a raw vector, the index is out of bounds,
    /// or the data pointer is null.
    #[inline]
    pub fn raw_elt(&self, i: R_xlen_t) -> Option<Rbyte> {
        self.try_raw_elt(i).ok()
    }

    /// Get the i-th raw byte with typed error reporting.
    #[inline]
    pub fn try_raw_elt(&self, i: R_xlen_t) -> SexpResult<Rbyte> {
        let data = self.try_typed_data::<Rbyte>(SEXPTYPE::RAWSXP, "raw vector")?;
        let i = self.try_index(i)?;
        Ok(unsafe { *data.add(i) })
    }

    /// Get the i-th complex value with bounds checking.
    ///
    /// Returns `None` if this is not a complex vector, the index is out of
    /// bounds, or the data pointer is null.
    #[inline]
    pub fn complex_elt(&self, i: R_xlen_t) -> Option<Rcomplex> {
        self.try_complex_elt(i).ok()
    }

    /// Get the i-th complex value with typed error reporting.
    #[inline]
    pub fn try_complex_elt(&self, i: R_xlen_t) -> SexpResult<Rcomplex> {
        let data = self.try_typed_data::<Rcomplex>(SEXPTYPE::CPLXSXP, "complex vector")?;
        let i = self.try_index(i)?;
        Ok(unsafe { *data.add(i) })
    }

    /// Get the i-th string element (CHARSXP) with bounds checking.
    ///
    /// Returns `None` if the index is out of bounds, the data pointer is null,
    /// or the element itself is null.
    #[inline]
    pub fn string_elt(&self, i: R_xlen_t) -> Option<Sexp<'a>> {
        self.try_string_elt(i).ok()
    }

    /// Get the i-th string element with typed error reporting.
    #[inline]
    pub fn try_string_elt(&self, i: R_xlen_t) -> SexpResult<Sexp<'a>> {
        let data = self.try_typed_data::<SEXP>(SEXPTYPE::STRSXP, "string vector")?;
        let i = self.try_index(i)?;
        self.checked_child(unsafe { *data.add(i) })
    }

    /// Return the i-th string value as UTF-8 text, preserving R's `NA_STRING`.
    ///
    /// `Ok(None)` means the element is `NA_character_`; `Ok(Some(_))` is a
    /// present CHARSXP value. Type, bounds, missing-data, and UTF-8 failures are
    /// reported as [`SexpError`](super::SexpError).
    #[inline]
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn try_string_text_elt(&self, i: R_xlen_t) -> SexpResult<Option<&'_ str>> {
        let chars = self.try_string_elt(i)?;
        if chars.clone().as_raw() == unsafe { R_NaString() } {
            Ok(None)
        } else {
            // SAFETY: this borrow is tied to the parent vector. Its root
            // retains this child even after the temporary child lease drops.
            // The caller excludes mutation throughout the returned borrow.
            let text = unsafe { chars.try_as_str()? };
            Ok(Some(unsafe { &*(text as *const str) }))
        }
    }

    /// Return the i-th string value as optional UTF-8 text.
    ///
    /// The outer `None` is an access/type error; the inner `None` is R's
    /// `NA_character_`.
    #[inline]
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn string_text_elt(&self, i: R_xlen_t) -> Option<Option<&'_ str>> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_string_text_elt(i)
        }
        .ok()
    }

    /// Copy a string element, preserving `NA_character_` as `None`.
    pub fn try_string_value_elt(&self, i: R_xlen_t) -> SexpResult<Option<String>> {
        let chars = self.try_string_elt(i)?;
        if chars.clone().as_raw() == unsafe { R_NaString() } {
            Ok(None)
        } else {
            chars.try_as_string().map(Some)
        }
    }

    pub fn string_value_elt(&self, i: R_xlen_t) -> Option<Option<String>> {
        self.try_string_value_elt(i).ok()
    }

    /// Get the i-th vector element with bounds checking.
    ///
    /// Returns `None` if the index is out of bounds, the data pointer is null,
    /// or the element itself is null.
    #[inline]
    pub fn vector_elt(&self, i: R_xlen_t) -> Option<Sexp<'a>> {
        self.try_vector_elt(i).ok()
    }

    /// Get the i-th generic/expression vector element with typed error reporting.
    #[inline]
    pub fn try_vector_elt(&self, i: R_xlen_t) -> SexpResult<Sexp<'a>> {
        let data = self.try_vector_sexp_data()?;
        let i = self.try_index(i)?;
        self.checked_child(unsafe { *data.add(i) })
    }

    // --- Mutation methods ---

    /// Set the i-th logical value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn set_logical_elt(self, i: R_xlen_t, v: c_int) -> bool {
        unsafe {
            /* SAFETY: caller excludes borrowed payload views. */
            self.try_set_logical_elt(i, v)
        }
        .is_ok()
    }

    /// Set the i-th logical value with typed error reporting.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn try_set_logical_elt(self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        let data = self
            .clone()
            .try_typed_data_mut::<c_int>(SEXPTYPE::LGLSXP, "logical vector")
            .clone()?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v;
        }
        Ok(())
    }

    /// Set the i-th integer value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn set_integer_elt(self, i: R_xlen_t, v: c_int) -> bool {
        unsafe {
            /* SAFETY: caller excludes borrowed payload views. */
            self.try_set_integer_elt(i, v)
        }
        .is_ok()
    }

    /// Set the i-th integer value with typed error reporting.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn try_set_integer_elt(self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        let data = self
            .clone()
            .try_typed_data_mut::<c_int>(SEXPTYPE::INTSXP, "integer vector")
            .clone()?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v;
        }
        Ok(())
    }

    /// Set the i-th real (double) value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn set_real_elt(self, i: R_xlen_t, v: c_double) -> bool {
        unsafe {
            /* SAFETY: caller excludes borrowed payload views. */
            self.try_set_real_elt(i, v)
        }
        .is_ok()
    }

    /// Set the i-th real value with typed error reporting.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn try_set_real_elt(self, i: R_xlen_t, v: c_double) -> SexpResult<()> {
        let data = self
            .clone()
            .try_typed_data_mut::<c_double>(SEXPTYPE::REALSXP, "real vector")
            .clone()?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v;
        }
        Ok(())
    }

    /// Set the i-th raw byte.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn set_raw_elt(self, i: R_xlen_t, v: Rbyte) -> bool {
        unsafe {
            /* SAFETY: caller excludes borrowed payload views. */
            self.try_set_raw_elt(i, v)
        }
        .is_ok()
    }

    /// Set the i-th raw byte with typed error reporting.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn try_set_raw_elt(self, i: R_xlen_t, v: Rbyte) -> SexpResult<()> {
        let data = self
            .clone()
            .try_typed_data_mut::<Rbyte>(SEXPTYPE::RAWSXP, "raw vector")
            .clone()?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v;
        }
        Ok(())
    }

    /// Set the i-th string element.
    ///
    /// Returns `false` if this is not a string vector, `v` is not CHARSXP,
    /// the index is out of bounds, or data pointer is null.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn set_string_elt(self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        unsafe {
            /* SAFETY: caller excludes borrowed payload views. */
            self.try_set_string_elt(i, v)
        }
        .is_ok()
    }

    /// Set the i-th string element with typed error reporting.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn try_set_string_elt(self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        self.check_child_owner(&v)?;
        v.clone()
            .expect_type(SEXPTYPE::CHARSXP, "character scalar")
            .clone()?;
        let data = self
            .clone()
            .try_typed_data_mut::<SEXP>(SEXPTYPE::STRSXP, "string vector")
            .clone()?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v.as_raw();
        }
        Ok(())
    }

    /// Set the i-th vector element.
    ///
    /// Returns `false` if this is not a generic/expression vector, the index is
    /// out of bounds, or data pointer is null.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn set_vector_elt(self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        unsafe {
            /* SAFETY: caller excludes borrowed payload views. */
            self.try_set_vector_elt(i, v)
        }
        .is_ok()
    }

    /// Set the i-th generic/expression vector element with typed error reporting.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn try_set_vector_elt(self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        self.check_child_owner(&v)?;
        let data = self.clone().try_vector_sexp_data_mut().clone()?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v.as_raw();
        }
        Ok(())
    }

    /// Copy integer payloads into caller-owned storage without lending a view.
    pub fn copy_integer_into(&self, output: &mut [c_int]) -> SexpResult<()> {
        self.expect_type(SEXPTYPE::INTSXP, "integer vector")?;
        let expected = self.len() as usize;
        if output.len() != expected {
            return Err(SexpError::LengthMismatch { expected, actual: output.len() });
        }
        // SAFETY: copy synchronously without R callbacks; the handle retains
        // the allocation. Safe callers cannot obtain a Rust R-payload borrow.
        output.copy_from_slice(unsafe { self.try_as_integer_slice() }?);
        Ok(())
    }

    // --- Slice views ---

    /// Get a slice view of the logical data.
    ///
    /// Returns `None` if this is not a logical vector or the data pointer is null.
    /// The slice borrows this handle; it does not outlive the handle.
    #[allow(clippy::wrong_self_convention)]
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn as_logical_slice(&self) -> Option<&'_ [c_int]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_as_logical_slice()
        }
        .ok()
    }

    /// Get a logical slice view with typed error reporting.
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn try_as_logical_slice(&self) -> SexpResult<&'_ [c_int]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_typed_slice::<c_int>(SEXPTYPE::LGLSXP, "logical vector")
        }
    }

    /// Get a slice view of the integer data.
    ///
    /// Returns `None` if this is not an integer vector or the data pointer is null.
    #[allow(clippy::wrong_self_convention)]
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn as_integer_slice(&self) -> Option<&'_ [c_int]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_as_integer_slice()
        }
        .ok()
    }

    /// Get an integer slice view with typed error reporting.
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn try_as_integer_slice(&self) -> SexpResult<&'_ [c_int]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_typed_slice::<c_int>(SEXPTYPE::INTSXP, "integer vector")
        }
    }

    /// Get a slice view of the real (double) data.
    ///
    /// Returns `None` if this is not a real vector or the data pointer is null.
    #[allow(clippy::wrong_self_convention)]
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn as_real_slice(&self) -> Option<&'_ [c_double]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_as_real_slice()
        }
        .ok()
    }

    /// Get a real slice view with typed error reporting.
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn try_as_real_slice(&self) -> SexpResult<&'_ [c_double]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_typed_slice::<c_double>(SEXPTYPE::REALSXP, "real vector")
        }
    }

    /// Get a slice view of the raw byte data.
    ///
    /// Returns `None` if this is not a raw vector or the data pointer is null.
    #[allow(clippy::wrong_self_convention)]
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn as_raw_slice(&self) -> Option<&'_ [Rbyte]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_as_raw_slice()
        }
        .ok()
    }

    /// Get a raw byte slice view with typed error reporting.
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn try_as_raw_slice(&self) -> SexpResult<&'_ [Rbyte]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_typed_slice::<Rbyte>(SEXPTYPE::RAWSXP, "raw vector")
        }
    }

    // --- Iterators ---

    /// Iterate over logical elements.
    pub fn iter_logical(self) -> impl Iterator<Item = c_int> + 'a {
        (0..self.len()).filter_map(move |i| self.logical_elt(i))
    }

    /// Iterate over integer elements.
    pub fn iter_integer(self) -> impl Iterator<Item = c_int> + 'a {
        (0..self.len()).filter_map(move |i| self.integer_elt(i))
    }

    /// Iterate over real (double) elements.
    pub fn iter_real(self) -> impl Iterator<Item = c_double> + 'a {
        (0..self.len()).filter_map(move |i| self.real_elt(i))
    }

    /// Iterate over raw byte elements.
    pub fn iter_raw(self) -> impl Iterator<Item = Rbyte> + 'a {
        (0..self.len()).filter_map(move |i| self.raw_elt(i))
    }

    /// Iterate over vector elements (for VECSXP/EXPRSXP).
    ///
    /// Null elements are replaced with `R_NilValue`.
    pub fn iter_vector(self) -> impl Iterator<Item = Sexp<'a>> + 'a {
        let len = if self.vector_sexp_data().is_some() {
            self.len()
        } else {
            0
        };
        (0..len).map(move |i| self.vector_elt(i).unwrap_or_else(|| Sexp::nil()))
    }
}
