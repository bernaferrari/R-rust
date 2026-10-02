use std::os::raw::c_int;

use super::header::NodeBody;
use super::{Sexp, SexpError, SexpResult};
use crate::sexp::ffi::SEXPTYPE;

impl<'a> Sexp<'a> {
    // --- Primitive/Builtin/Special accessors ---

    #[inline]
    pub fn is_special(&self) -> bool {
        self.typeof_() == SEXPTYPE::SPECIALSXP
    }

    #[inline]
    pub fn is_builtin(&self) -> bool {
        self.typeof_() == SEXPTYPE::BUILTINSXP
    }

    #[inline]
    pub fn is_primitive(&self) -> bool {
        matches!(self.typeof_(), SEXPTYPE::SPECIALSXP | SEXPTYPE::BUILTINSXP)
    }

    pub fn primoffset(&self) -> Option<c_int> {
        match self.header().body {
            NodeBody::Primitive(slot) => Some(slot.offset),
            _ => None,
        }
    }

    /// Get the primitive table index with typed error reporting.
    pub fn try_primoffset(&self) -> SexpResult<c_int> {
        match self.header().body {
            NodeBody::Primitive(slot) => Ok(slot.offset),
            _ => Err(SexpError::TypeMismatch {
                expected: "special or builtin primitive",
                actual: self.typeof_(),
            }),
        }
    }
}
