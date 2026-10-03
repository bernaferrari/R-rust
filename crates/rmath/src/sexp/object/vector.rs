use std::os::raw::{c_double, c_int};

use super::{Sexp, SexpError, SexpResult};
use crate::sexp::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXPTYPE};

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
        self.expect_type(SEXPTYPE::LGLSXP, "logical vector")?;
        self.try_index(i)?;
        if self.typeof_() == SEXPTYPE::LGLSXP
            && let Some(result) = crate::sexp::altrep::lazy_element(self, i)
        {
            return match result? {
                crate::sexp::altrep::AltrepElement::Logical(v) => Ok(v),
                _ => Err(SexpError::Altrep {
                    reason: "element type mismatch",
                }),
            };
        }
        let lease = self.try_payload_lease(SEXPTYPE::LGLSXP, "logical vector")?;
        let index = self.try_index(i)?;
        lease.integer_elt(index).ok_or(SexpError::MissingData {
            sexptype: SEXPTYPE::LGLSXP,
        })
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
        self.expect_type(SEXPTYPE::INTSXP, "integer vector")?;
        self.try_index(i)?;
        if self.typeof_() == SEXPTYPE::INTSXP
            && let Some(result) = crate::sexp::altrep::lazy_element(self, i)
        {
            return match result? {
                crate::sexp::altrep::AltrepElement::Integer(v) => Ok(v),
                _ => Err(SexpError::Altrep {
                    reason: "element type mismatch",
                }),
            };
        }
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
        let lease = self.try_payload_lease(SEXPTYPE::INTSXP, "integer vector")?;
        let index = self.try_index(i)?;
        lease.integer_elt(index).ok_or(SexpError::MissingData {
            sexptype: SEXPTYPE::INTSXP,
        })
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
        self.expect_type(SEXPTYPE::REALSXP, "real vector")?;
        self.try_index(i)?;
        if self.typeof_() == SEXPTYPE::REALSXP
            && let Some(result) = crate::sexp::altrep::lazy_element(self, i)
        {
            return match result? {
                crate::sexp::altrep::AltrepElement::Real(v) => Ok(v),
                _ => Err(SexpError::Altrep {
                    reason: "element type mismatch",
                }),
            };
        }
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
        let lease = self.try_payload_lease(SEXPTYPE::REALSXP, "real vector")?;
        let index = self.try_index(i)?;
        lease.real_elt(index).ok_or(SexpError::MissingData {
            sexptype: SEXPTYPE::REALSXP,
        })
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
        self.expect_type(SEXPTYPE::RAWSXP, "raw vector")?;
        self.try_index(i)?;
        if self.typeof_() == SEXPTYPE::RAWSXP
            && let Some(result) = crate::sexp::altrep::lazy_element(self, i)
        {
            return match result? {
                crate::sexp::altrep::AltrepElement::Raw(v) => Ok(v),
                _ => Err(SexpError::Altrep {
                    reason: "element type mismatch",
                }),
            };
        }
        let lease = self.try_payload_lease(SEXPTYPE::RAWSXP, "raw vector")?;
        let index = self.try_index(i)?;
        lease.byte_elt(index).ok_or(SexpError::MissingData {
            sexptype: SEXPTYPE::RAWSXP,
        })
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
        self.expect_type(SEXPTYPE::CPLXSXP, "complex vector")?;
        self.try_index(i)?;
        if self.typeof_() == SEXPTYPE::CPLXSXP
            && let Some(result) = crate::sexp::altrep::lazy_element(self, i)
        {
            return match result? {
                crate::sexp::altrep::AltrepElement::Complex(v) => Ok(v),
                _ => Err(SexpError::Altrep {
                    reason: "element type mismatch",
                }),
            };
        }
        let lease = self.try_payload_lease(SEXPTYPE::CPLXSXP, "complex vector")?;
        let index = self.try_index(i)?;
        lease.complex_elt(index).ok_or(SexpError::MissingData {
            sexptype: SEXPTYPE::CPLXSXP,
        })
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
        self.expect_type(SEXPTYPE::STRSXP, "string vector")?;
        self.try_index(i)?;
        if self.typeof_() == SEXPTYPE::STRSXP
            && let Some(result) = crate::sexp::altrep::lazy_element(self, i)
        {
            return match result? {
                crate::sexp::altrep::AltrepElement::String(v) => Ok(v),
                _ => Err(SexpError::Altrep {
                    reason: "element type mismatch",
                }),
            };
        }
        self.expect_type(SEXPTYPE::STRSXP, "string vector")?;
        self.try_index(i)?;
        self.materialize_compact_payload()?;
        self.expect_type(SEXPTYPE::STRSXP, "string vector")?;
        let index = self.try_index(i)?;
        self.checked_child(self.reference_elt(index)?)
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
        // A lazy callback may create a fresh child on every read. Before a
        // borrowed string view escapes, make it a traced child of this parent.
        self.materialize_compact_payload()?;
        let chars = self.try_string_elt(i)?;
        if chars.is_na_string() {
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
        if chars.is_na_string() {
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
        self.expect_any_type(
            "generic or expression vector",
            &[SEXPTYPE::VECSXP, SEXPTYPE::EXPRSXP],
        )?;
        self.try_index(i)?;
        if self.typeof_() == SEXPTYPE::VECSXP
            && let Some(result) = crate::sexp::altrep::lazy_element(self, i)
        {
            return match result? {
                crate::sexp::altrep::AltrepElement::List(v) => Ok(v),
                _ => Err(SexpError::Altrep {
                    reason: "element type mismatch",
                }),
            };
        }
        self.expect_any_type(
            "generic or expression vector",
            &[SEXPTYPE::VECSXP, SEXPTYPE::EXPRSXP],
        )?;
        self.try_index(i)?;
        self.materialize_compact_payload()?;
        self.expect_any_type(
            "generic or expression vector",
            &[SEXPTYPE::VECSXP, SEXPTYPE::EXPRSXP],
        )?;
        let index = self.try_index(i)?;
        self.checked_child(self.reference_elt(index)?)
    }

    // --- Mutation methods ---

    /// Set the i-th logical value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    /// Copies into a bounded cell of the actual retained allocation.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn set_logical_elt(self, i: R_xlen_t, v: c_int) -> bool {
        self.try_set_logical_elt(i, v).is_ok()
    }

    /// Set the i-th logical value with typed error reporting.
    /// Copies into a bounded cell of the actual retained allocation.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn try_set_logical_elt(self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        self.expect_type(SEXPTYPE::LGLSXP, "logical vector")?;
        self.try_index(i)?;
        let lease = self.try_payload_lease(SEXPTYPE::LGLSXP, "logical vector")?;
        let index = self.try_index(i)?;
        lease
            .set_integer_elt(index, v)
            .ok_or(SexpError::MissingData {
                sexptype: SEXPTYPE::LGLSXP,
            })
    }

    /// Set the i-th integer value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    /// Copies into a bounded cell of the actual retained allocation.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn set_integer_elt(self, i: R_xlen_t, v: c_int) -> bool {
        self.try_set_integer_elt(i, v).is_ok()
    }

    /// Set the i-th integer value with typed error reporting.
    /// Copies into a bounded cell of the actual retained allocation.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn try_set_integer_elt(self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        self.expect_type(SEXPTYPE::INTSXP, "integer vector")?;
        self.try_index(i)?;
        let lease = self.try_payload_lease(SEXPTYPE::INTSXP, "integer vector")?;
        let index = self.try_index(i)?;
        lease
            .set_integer_elt(index, v)
            .ok_or(SexpError::MissingData {
                sexptype: SEXPTYPE::INTSXP,
            })
    }

    /// Set the i-th real (double) value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    /// Copies into a bounded cell of the actual retained allocation.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn set_real_elt(self, i: R_xlen_t, v: c_double) -> bool {
        self.try_set_real_elt(i, v).is_ok()
    }

    /// Set the i-th real value with typed error reporting.
    /// Copies into a bounded cell of the actual retained allocation.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn try_set_real_elt(self, i: R_xlen_t, v: c_double) -> SexpResult<()> {
        self.expect_type(SEXPTYPE::REALSXP, "real vector")?;
        self.try_index(i)?;
        let lease = self.try_payload_lease(SEXPTYPE::REALSXP, "real vector")?;
        let index = self.try_index(i)?;
        lease.set_real_elt(index, v).ok_or(SexpError::MissingData {
            sexptype: SEXPTYPE::REALSXP,
        })
    }

    /// Set the i-th raw byte.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    /// Copies into a bounded cell of the actual retained allocation.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn set_raw_elt(self, i: R_xlen_t, v: Rbyte) -> bool {
        self.try_set_raw_elt(i, v).is_ok()
    }

    /// Set the i-th raw byte with typed error reporting.
    /// Copies into a bounded cell of the actual retained allocation.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn try_set_raw_elt(self, i: R_xlen_t, v: Rbyte) -> SexpResult<()> {
        self.expect_type(SEXPTYPE::RAWSXP, "raw vector")?;
        self.try_index(i)?;
        let lease = self.try_payload_lease(SEXPTYPE::RAWSXP, "raw vector")?;
        let index = self.try_index(i)?;
        lease.set_byte_elt(index, v).ok_or(SexpError::MissingData {
            sexptype: SEXPTYPE::RAWSXP,
        })
    }

    /// Set the i-th string element.
    ///
    /// Returns `false` if this is not a string vector, `v` is not CHARSXP,
    /// the index is out of bounds, or typed storage is unavailable.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn set_string_elt(self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        self.try_set_string_elt(i, v).is_ok()
    }

    /// Set the i-th string element with typed error reporting.
    /// Reference cells expose copied reads and bounded writes, without lending
    /// Rust references into their storage.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn try_set_string_elt(self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        self.check_child_owner(&v)?;
        v.clone()
            .expect_type(SEXPTYPE::CHARSXP, "character scalar")
            .clone()?;
        self.expect_type(SEXPTYPE::STRSXP, "string vector")?;
        self.try_index(i)?;
        self.materialize_compact_payload()?;
        self.expect_type(SEXPTYPE::STRSXP, "string vector")?;
        let i = self.try_index(i)?;
        self.reference_elt(i)?;
        self.remember_child(&v)?;
        self.set_reference_elt(i, &v)
    }

    /// Set the i-th vector element.
    ///
    /// Returns `false` if this is not a generic/expression vector, the index is
    /// out of bounds, or typed storage is unavailable.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn set_vector_elt(self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        self.try_set_vector_elt(i, v).is_ok()
    }

    /// Set the i-th generic/expression vector element with typed error reporting.
    /// Exact child identities and actual typed capacity are checked before the
    /// canonical cell is changed.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) fn try_set_vector_elt(self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        self.check_child_owner(&v)?;
        self.expect_any_type(
            "generic or expression vector",
            &[SEXPTYPE::VECSXP, SEXPTYPE::EXPRSXP],
        )?;
        self.try_index(i)?;
        self.materialize_compact_payload()?;
        self.expect_any_type(
            "generic or expression vector",
            &[SEXPTYPE::VECSXP, SEXPTYPE::EXPRSXP],
        )?;
        let i = self.try_index(i)?;
        self.reference_elt(i)?;
        self.remember_child(&v)?;
        self.set_reference_elt(i, &v)
    }

    /// Copy integer payloads into caller-owned storage without lending a view.
    pub fn copy_integer_into(&self, output: &mut [c_int]) -> SexpResult<()> {
        self.expect_type(SEXPTYPE::INTSXP, "integer vector")?;
        let expected = self.len() as usize;
        if output.len() != expected {
            return Err(SexpError::LengthMismatch {
                expected,
                actual: output.len(),
            });
        }
        if output.is_empty() {
            return Ok(());
        }
        let lease = self.try_payload_lease(SEXPTYPE::INTSXP, "integer vector")?;
        let current = usize::try_from(self.len()).map_err(|_| SexpError::MissingData {
            sexptype: SEXPTYPE::INTSXP,
        })?;
        if current != expected {
            return Err(SexpError::LengthMismatch {
                expected: current,
                actual: output.len(),
            });
        }
        for (index, target) in output.iter_mut().enumerate() {
            *target = lease.integer_elt(index).ok_or(SexpError::MissingData {
                sexptype: SEXPTYPE::INTSXP,
            })?;
        }
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
        let len = if matches!(self.typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP) {
            self.len()
        } else {
            0
        };
        (0..len).map(move |i| self.vector_elt(i).unwrap_or_else(|| Sexp::nil()))
    }
}

#[cfg(test)]
mod altrep_borrow_tests {
    use super::*;
    use crate::sexp::{
        altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement, is_materialized},
        session::RSession,
    };
    struct Fresh;
    impl AltrepClass for Fresh {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::STRSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(2)
        }
        fn element<'s>(&self, c: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            c.gc()?;
            Ok(AltrepElement::String(c.string("fresh")?))
        }
    }
    #[test]
    fn altrep_borrowed_string_is_retained_by_parent() {
        let s = RSession::new_for_gc_tests();
        let class = s.register_altrep_class("borrowed-strings", Fresh).unwrap();
        let x = AltrepBuilder::new(class).build().unwrap();
        assert!(!is_materialized(&x));
        let text = unsafe { x.try_string_text_elt(1) }.unwrap();
        assert!(is_materialized(&x));
        assert_eq!(text, Some("fresh"));
        // The payload loan has ended before R execution resumes.
        s.gc();
        assert_eq!(x.try_string_value_elt(1).unwrap().as_deref(), Some("fresh"));
    }
}
