use std::os::raw::c_int;

use super::{Sexp, SexpResult};
use crate::sexp::ffi::SEXPTYPE;

impl<'a> Sexp<'a> {
    // --- Primitive/Builtin/Special accessors ---

    #[inline]
    pub fn is_special(self) -> bool {
        self.typeof_() == SEXPTYPE::SPECIALSXP
    }

    #[inline]
    pub fn is_builtin(self) -> bool {
        self.typeof_() == SEXPTYPE::BUILTINSXP
    }

    #[inline]
    pub fn is_primitive(self) -> bool {
        matches!(self.typeof_(), SEXPTYPE::SPECIALSXP | SEXPTYPE::BUILTINSXP)
    }

    pub fn primoffset(self) -> Option<c_int> {
        if self.clone().is_primitive() {
            Some(unsafe { (*self.ptr).data.primsxp.offset })
        } else {
            None
        }
    }

    /// Get the primitive table index with typed error reporting.
    pub fn try_primoffset(self) -> SexpResult<c_int> {
        self.clone().expect_any_type(
            "special or builtin primitive",
            &[SEXPTYPE::SPECIALSXP, SEXPTYPE::BUILTINSXP],
        ).clone()?;
        Ok(unsafe { (*self.ptr).data.primsxp.offset })
    }
}
