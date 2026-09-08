use std::os::raw::{c_double, c_int};

use super::super::ffi::{Rbyte, Rcomplex};
use super::Sexp;

/// Borrowed, type-directed view over a `Sexp`.
#[derive(Debug, Clone)]
pub enum SexpView<'a> {
    Nil,
    Logical(&'a [c_int]),
    Integer(&'a [c_int]),
    Real(&'a [c_double]),
    Complex(&'a [Rcomplex]),
    Raw(&'a [Rbyte]),
    Char(&'a [u8]),
    StringVector(Sexp<'a>),
    GenericVector(Sexp<'a>),
    Pairlist(Sexp<'a>),
    Environment(Sexp<'a>),
    Symbol(Sexp<'a>),
    Function(Sexp<'a>),
    Other(Sexp<'a>),
}
