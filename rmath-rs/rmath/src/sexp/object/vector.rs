use std::os::raw::{c_double, c_int};

use super::{Sexp, SexpResult};
use crate::sexp::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NaString, R_NilValue};

impl<'a> Sexp<'a> {
    // --- Vector element access with bounds checking ---

    /// Get the i-th logical value with bounds checking.
    ///
    /// Returns `None` if this is not a logical vector, the index is out of
    /// bounds, or the data pointer is null.
    #[inline]
    pub fn logical_elt(self, i: R_xlen_t) -> Option<c_int> {
        self.try_logical_elt(i).ok()
    }

    /// Get the i-th logical value with typed error reporting.
    #[inline]
    pub fn try_logical_elt(self, i: R_xlen_t) -> SexpResult<c_int> {
        let data = self.try_typed_data::<c_int>(SEXPTYPE::LGLSXP, "logical vector")?;
        let i = self.try_index(i)?;
        Ok(unsafe { *data.add(i) })
    }

    /// Get the i-th integer value with bounds checking.
    ///
    /// Returns `None` if this is not an integer vector, the index is out of
    /// bounds, or the data pointer is null.
    #[inline]
    pub fn integer_elt(self, i: R_xlen_t) -> Option<c_int> {
        self.try_integer_elt(i).ok()
    }

    /// Get the i-th integer value with typed error reporting.
    #[inline]
    pub fn try_integer_elt(self, i: R_xlen_t) -> SexpResult<c_int> {
        let data = self.try_typed_data::<c_int>(SEXPTYPE::INTSXP, "integer vector")?;
        let i = self.try_index(i)?;
        Ok(unsafe { *data.add(i) })
    }

    /// Get the i-th real (double) value with bounds checking.
    ///
    /// Returns `None` if this is not a real vector, the index is out of bounds,
    /// or the data pointer is null.
    #[inline]
    pub fn real_elt(self, i: R_xlen_t) -> Option<c_double> {
        self.try_real_elt(i).ok()
    }

    /// Get the i-th real value with typed error reporting.
    #[inline]
    pub fn try_real_elt(self, i: R_xlen_t) -> SexpResult<c_double> {
        let data = self.try_typed_data::<c_double>(SEXPTYPE::REALSXP, "real vector")?;
        let i = self.try_index(i)?;
        Ok(unsafe { *data.add(i) })
    }

    /// Get the i-th raw byte with bounds checking.
    ///
    /// Returns `None` if this is not a raw vector, the index is out of bounds,
    /// or the data pointer is null.
    #[inline]
    pub fn raw_elt(self, i: R_xlen_t) -> Option<Rbyte> {
        self.try_raw_elt(i).ok()
    }

    /// Get the i-th raw byte with typed error reporting.
    #[inline]
    pub fn try_raw_elt(self, i: R_xlen_t) -> SexpResult<Rbyte> {
        let data = self.try_typed_data::<Rbyte>(SEXPTYPE::RAWSXP, "raw vector")?;
        let i = self.try_index(i)?;
        Ok(unsafe { *data.add(i) })
    }

    /// Get the i-th complex value with bounds checking.
    ///
    /// Returns `None` if this is not a complex vector, the index is out of
    /// bounds, or the data pointer is null.
    #[inline]
    pub fn complex_elt(self, i: R_xlen_t) -> Option<Rcomplex> {
        self.try_complex_elt(i).ok()
    }

    /// Get the i-th complex value with typed error reporting.
    #[inline]
    pub fn try_complex_elt(self, i: R_xlen_t) -> SexpResult<Rcomplex> {
        let data = self.try_typed_data::<Rcomplex>(SEXPTYPE::CPLXSXP, "complex vector")?;
        let i = self.try_index(i)?;
        Ok(unsafe { *data.add(i) })
    }

    /// Get the i-th string element (CHARSXP) with bounds checking.
    ///
    /// Returns `None` if the index is out of bounds, the data pointer is null,
    /// or the element itself is null.
    #[inline]
    pub fn string_elt(self, i: R_xlen_t) -> Option<Sexp<'a>> {
        self.try_string_elt(i).ok()
    }

    /// Get the i-th string element with typed error reporting.
    #[inline]
    pub fn try_string_elt(self, i: R_xlen_t) -> SexpResult<Sexp<'a>> {
        let data = self.try_typed_data::<SEXP>(SEXPTYPE::STRSXP, "string vector")?;
        let i = self.try_index(i)?;
        Self::checked_child(unsafe { *data.add(i) })
    }

    /// Return the i-th string value as UTF-8 text, preserving R's `NA_STRING`.
    ///
    /// `Ok(None)` means the element is `NA_character_`; `Ok(Some(_))` is a
    /// present CHARSXP value. Type, bounds, missing-data, and UTF-8 failures are
    /// reported as [`SexpError`](super::SexpError).
    #[inline]
    pub fn try_string_text_elt(self, i: R_xlen_t) -> SexpResult<Option<&'a str>> {
        let chars = self.try_string_elt(i)?;
        if chars.as_raw() == unsafe { R_NaString() } {
            Ok(None)
        } else {
            chars.try_as_str().map(Some)
        }
    }

    /// Return the i-th string value as optional UTF-8 text.
    ///
    /// The outer `None` is an access/type error; the inner `None` is R's
    /// `NA_character_`.
    #[inline]
    pub fn string_text_elt(self, i: R_xlen_t) -> Option<Option<&'a str>> {
        self.try_string_text_elt(i).ok()
    }

    /// Get the i-th vector element with bounds checking.
    ///
    /// Returns `None` if the index is out of bounds, the data pointer is null,
    /// or the element itself is null.
    #[inline]
    pub fn vector_elt(self, i: R_xlen_t) -> Option<Sexp<'a>> {
        self.try_vector_elt(i).ok()
    }

    /// Get the i-th generic/expression vector element with typed error reporting.
    #[inline]
    pub fn try_vector_elt(self, i: R_xlen_t) -> SexpResult<Sexp<'a>> {
        let data = self.try_vector_sexp_data()?;
        let i = self.try_index(i)?;
        Self::checked_child(unsafe { *data.add(i) })
    }

    // --- Mutation methods ---

    /// Set the i-th logical value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    pub fn set_logical_elt(self, i: R_xlen_t, v: c_int) -> bool {
        self.try_set_logical_elt(i, v).is_ok()
    }

    /// Set the i-th logical value with typed error reporting.
    pub fn try_set_logical_elt(self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        let data = self.try_typed_data_mut::<c_int>(SEXPTYPE::LGLSXP, "logical vector")?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v;
        }
        Ok(())
    }

    /// Set the i-th integer value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    pub fn set_integer_elt(self, i: R_xlen_t, v: c_int) -> bool {
        self.try_set_integer_elt(i, v).is_ok()
    }

    /// Set the i-th integer value with typed error reporting.
    pub fn try_set_integer_elt(self, i: R_xlen_t, v: c_int) -> SexpResult<()> {
        let data = self.try_typed_data_mut::<c_int>(SEXPTYPE::INTSXP, "integer vector")?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v;
        }
        Ok(())
    }

    /// Set the i-th real (double) value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    pub fn set_real_elt(self, i: R_xlen_t, v: c_double) -> bool {
        self.try_set_real_elt(i, v).is_ok()
    }

    /// Set the i-th real value with typed error reporting.
    pub fn try_set_real_elt(self, i: R_xlen_t, v: c_double) -> SexpResult<()> {
        let data = self.try_typed_data_mut::<c_double>(SEXPTYPE::REALSXP, "real vector")?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v;
        }
        Ok(())
    }

    /// Set the i-th raw byte.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    pub fn set_raw_elt(self, i: R_xlen_t, v: Rbyte) -> bool {
        self.try_set_raw_elt(i, v).is_ok()
    }

    /// Set the i-th raw byte with typed error reporting.
    pub fn try_set_raw_elt(self, i: R_xlen_t, v: Rbyte) -> SexpResult<()> {
        let data = self.try_typed_data_mut::<Rbyte>(SEXPTYPE::RAWSXP, "raw vector")?;
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
    pub fn set_string_elt(self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        self.try_set_string_elt(i, v).is_ok()
    }

    /// Set the i-th string element with typed error reporting.
    pub fn try_set_string_elt(self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        v.expect_type(SEXPTYPE::CHARSXP, "character scalar")?;
        let data = self.try_typed_data_mut::<SEXP>(SEXPTYPE::STRSXP, "string vector")?;
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
    pub fn set_vector_elt(self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        self.try_set_vector_elt(i, v).is_ok()
    }

    /// Set the i-th generic/expression vector element with typed error reporting.
    pub fn try_set_vector_elt(self, i: R_xlen_t, v: Sexp<'a>) -> SexpResult<()> {
        let data = self.try_vector_sexp_data_mut()?;
        let i = self.try_index(i)?;
        unsafe {
            *data.add(i) = v.as_raw();
        }
        Ok(())
    }

    // --- Slice views ---

    /// Get a slice view of the logical data.
    ///
    /// Returns `None` if this is not a logical vector or the data pointer is null.
    /// The slice is valid for the lifetime `'a` of the `Sexp`.
    pub fn as_logical_slice(self) -> Option<&'a [c_int]> {
        self.try_as_logical_slice().ok()
    }

    /// Get a logical slice view with typed error reporting.
    pub fn try_as_logical_slice(self) -> SexpResult<&'a [c_int]> {
        self.try_typed_slice::<c_int>(SEXPTYPE::LGLSXP, "logical vector")
    }

    /// Get a slice view of the integer data.
    ///
    /// Returns `None` if this is not an integer vector or the data pointer is null.
    pub fn as_integer_slice(self) -> Option<&'a [c_int]> {
        self.try_as_integer_slice().ok()
    }

    /// Get an integer slice view with typed error reporting.
    pub fn try_as_integer_slice(self) -> SexpResult<&'a [c_int]> {
        self.try_typed_slice::<c_int>(SEXPTYPE::INTSXP, "integer vector")
    }

    /// Get a slice view of the real (double) data.
    ///
    /// Returns `None` if this is not a real vector or the data pointer is null.
    pub fn as_real_slice(self) -> Option<&'a [c_double]> {
        self.try_as_real_slice().ok()
    }

    /// Get a real slice view with typed error reporting.
    pub fn try_as_real_slice(self) -> SexpResult<&'a [c_double]> {
        self.try_typed_slice::<c_double>(SEXPTYPE::REALSXP, "real vector")
    }

    /// Get a slice view of the raw byte data.
    ///
    /// Returns `None` if this is not a raw vector or the data pointer is null.
    pub fn as_raw_slice(self) -> Option<&'a [Rbyte]> {
        self.try_as_raw_slice().ok()
    }

    /// Get a raw byte slice view with typed error reporting.
    pub fn try_as_raw_slice(self) -> SexpResult<&'a [Rbyte]> {
        self.try_typed_slice::<Rbyte>(SEXPTYPE::RAWSXP, "raw vector")
    }

    // --- Iterators ---

    /// Iterate over logical elements.
    pub fn iter_logical(self) -> impl Iterator<Item = c_int> + 'a {
        self.as_logical_slice().unwrap_or(&[]).iter().copied()
    }

    /// Iterate over integer elements.
    pub fn iter_integer(self) -> impl Iterator<Item = c_int> + 'a {
        self.as_integer_slice().unwrap_or(&[]).iter().copied()
    }

    /// Iterate over real (double) elements.
    pub fn iter_real(self) -> impl Iterator<Item = c_double> + 'a {
        self.as_real_slice().unwrap_or(&[]).iter().copied()
    }

    /// Iterate over raw byte elements.
    pub fn iter_raw(self) -> impl Iterator<Item = Rbyte> + 'a {
        self.as_raw_slice().unwrap_or(&[]).iter().copied()
    }

    /// Iterate over vector elements (for VECSXP/EXPRSXP).
    ///
    /// Null elements are replaced with `R_NilValue`.
    pub fn iter_vector(self) -> impl Iterator<Item = Sexp<'a>> + 'a {
        let data = self.vector_sexp_data();
        let len = if data.is_some() {
            self.len() as usize
        } else {
            0
        };
        (0..len).map(move |i| {
            let ptr = data.map_or(std::ptr::null_mut(), |data| unsafe { *data.add(i) });
            Sexp::from_raw(ptr).unwrap_or_else(|| unsafe { Sexp::from_raw_unchecked(R_NilValue()) })
        })
    }
}
