use super::{Sexp, SexpResult, SexpView};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

/// Return whether a raw pointer is an atomic vector.
///
/// This keeps legacy C-shaped entry points from open-coding numeric type tags
/// while preserving their null-tolerant predicate semantics.
#[inline]
pub(crate) fn raw_is_atomic_vector(ptr: SEXP) -> bool {
    Sexp::from_raw(ptr).is_some_and(|sexp| sexp.is_atomic())
}

/// Return whether a raw pointer is any R vector type.
///
/// This is the raw-boundary companion to [`Sexp::is_vector`].
#[inline]
pub(crate) fn raw_is_vector(ptr: SEXP) -> bool {
    Sexp::from_raw(ptr).is_some_and(|sexp| sexp.is_vector())
}

impl<'a> Sexp<'a> {
    /// Return a Rust-shaped borrowed view for this SEXP.
    pub fn view(self) -> SexpResult<SexpView<'a>> {
        if self.clone().is_nil() {
            return Ok(SexpView::Nil);
        }
        match self.clone().typeof_() {
            SEXPTYPE::LGLSXP => Ok(SexpView::Logical(self.try_as_logical_slice()?)),
            SEXPTYPE::INTSXP => Ok(SexpView::Integer(self.try_as_integer_slice()?)),
            SEXPTYPE::REALSXP => Ok(SexpView::Real(self.try_as_real_slice()?)),
            SEXPTYPE::CPLXSXP => Ok(SexpView::Complex(self.try_as_complex_slice()?)),
            SEXPTYPE::RAWSXP => Ok(SexpView::Raw(self.try_as_raw_slice()?)),
            SEXPTYPE::CHARSXP => Ok(SexpView::Char(self.try_as_bytes()?)),
            SEXPTYPE::STRSXP => Ok(SexpView::StringVector(self)),
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => Ok(SexpView::GenericVector(self)),
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => Ok(SexpView::Pairlist(self)),
            SEXPTYPE::ENVSXP => Ok(SexpView::Environment(self)),
            SEXPTYPE::SYMSXP => Ok(SexpView::Symbol(self)),
            SEXPTYPE::CLOSXP | SEXPTYPE::SPECIALSXP | SEXPTYPE::BUILTINSXP => {
                Ok(SexpView::Function(self))
            }
            _ => Ok(SexpView::Other(self)),
        }
    }

    /// Get the type of this SEXP.
    #[inline]
    pub fn typeof_(&self) -> SEXPTYPE {
        unsafe { (*self.ptr).sxpinfo.type_of() }
    }

    /// Get the length of a vector SEXP.
    ///
    /// Returns 0 for non-vector types.
    #[inline]
    pub fn len(&self) -> R_xlen_t {
        if self.typeof_().is_vector_type() {
            unsafe { (*self.ptr).vecsxp_length() }
        } else {
            0
        }
    }

    /// Check if this SEXP is empty (length 0 or nil).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if this is R_NilValue.
    #[inline]
    pub fn is_nil(&self) -> bool {
        self.ptr == unsafe { R_NilValue() }
    }

    /// Check if this is a null value (R_NilValue).
    #[inline]
    pub fn is_null_value(&self) -> bool {
        self.is_nil()
    }

    /// Check if this is a symbol (SYMSXP).
    #[inline]
    pub fn is_symbol(&self) -> bool {
        self.typeof_() == SEXPTYPE::SYMSXP
    }

    /// Check if this is a closure (CLOSXP, i.e., a user-defined function).
    #[inline]
    pub fn is_closure(&self) -> bool {
        self.typeof_() == SEXPTYPE::CLOSXP
    }

    /// Check if this is an environment (ENVSXP).
    #[inline]
    pub fn is_environment(&self) -> bool {
        self.typeof_() == SEXPTYPE::ENVSXP
    }

    /// Check if this is a pairlist (LISTSXP or LANGSXP).
    #[inline]
    pub fn is_pairlist(&self) -> bool {
        matches!(self.typeof_(), SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP)
    }

    /// Check if this is an atomic vector.
    ///
    /// Atomic vectors hold primitive data directly (LGLSXP, INTSXP,
    /// REALSXP, CPLXSXP, STRSXP, RAWSXP).
    #[inline]
    pub fn is_atomic(&self) -> bool {
        self.typeof_().is_atomic_type()
    }

    /// Check if this is a vector type.
    ///
    /// Includes all atomic vectors plus VECSXP, EXPRSXP, and RAWSXP.
    #[inline]
    pub fn is_vector(&self) -> bool {
        self.typeof_().is_vector_type()
    }
}
