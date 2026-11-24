//! Rooted, copied element access for the QR wrappers.
use crate::sexp::context::r_error;
use crate::sexp::ffi::{R_xlen_t, Rcomplex, SEXP};
use crate::sexp::object::{Sexp, SexpMut, SexpResult};
use crate::sexp::owner::OwnerToken;

#[inline]
fn checked<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|error| r_error(error.to_string()))
}

pub(super) fn root(pointer: SEXP) -> Sexp<'static> {
    checked(checked(OwnerToken::current()).sexp(pointer))
}

pub(super) fn writable(pointer: SEXP) -> SexpMut<'static> {
    checked(SexpMut::try_from_checked(root(pointer)))
}

#[inline]
pub(super) fn real(value: &Sexp<'_>, index: usize) -> f64 {
    checked(value.try_real_elt(index as R_xlen_t))
}

#[inline]
pub(super) fn complex(value: &Sexp<'_>, index: usize) -> Rcomplex {
    checked(value.try_complex_elt(index as R_xlen_t))
}

#[inline]
pub(super) fn read_writable_real(value: &SexpMut<'_>, index: usize) -> f64 {
    checked(value.try_real_elt(index as R_xlen_t))
}

#[inline]
pub(super) fn write_real(value: &mut SexpMut<'_>, index: usize, element: f64) {
    checked(value.try_set_real_elt(index as R_xlen_t, element));
}

#[inline]
pub(super) fn write_complex(value: &mut SexpMut<'_>, index: usize, element: Rcomplex) {
    checked(value.try_set_complex_elt(index as R_xlen_t, element));
}
