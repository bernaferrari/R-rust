#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Builtin/primitive compatibility entrypoints.
//!
//! The real primitive metadata lives in [`super::primitive`]. This module keeps
//! the historical names used by older translated code while delegating to the
//! Rust-shaped descriptor layer.

use std::os::raw::c_int;

use crate::mainutils::names::FunTabEntry;
use crate::sexp::accessors::SET_PRIMOFFSET;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::memory;

use super::primitive;
pub use super::primitive::{PRIMNAME, PrimFun};

/// Get the canonical R function table.
pub fn R_FunTab() -> *const FunTabEntry {
    crate::mainutils::names::R_FunTab.as_ptr()
}

/// Get the canonical R function table length.
pub fn R_FunTabSize() -> usize {
    primitive::fun_tab_len()
}

/// Get the function pointer for a primitive (SPECIAL or BUILTIN).
///
/// This is the equivalent of R's `PRIMFUN()` macro.
#[inline]
pub unsafe fn PRIMFUN(op: SEXP) -> Option<PrimFun> {
    unsafe { primitive::get_primfun(op) }
}

/// Initialize builtin slots.
///
/// Primitive SEXP nodes are created lazily through `R_Primitive` in this port,
/// so there is no process-global slot table to initialize here.
pub fn R_InitBuiltinSlots() {}

/// Create a SPECIALSXP or BUILTINSXP from a function table index.
pub unsafe fn R_mkPrim(_name: *const std::os::raw::c_char, offset: c_int, kind: c_int) -> SEXP {
    let sexptype = if kind == SEXPTYPE::SPECIALSXP.as_c_int() || kind == 0 {
        SEXPTYPE::SPECIALSXP
    } else {
        SEXPTYPE::BUILTINSXP
    };

    memory::with_arena(|arena| {
        let prim = arena.alloc_node(sexptype);
        if !prim.is_null() {
            unsafe { SET_PRIMOFFSET(prim, offset) };
        }
        prim
    })
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn fun_tab_points_to_canonical_table() {
        assert!(!R_FunTab().is_null());
        assert!(R_FunTabSize() > 100);
    }

    #[test]
    fn primfun_null() {
        unsafe {
            assert!(PRIMFUN(ptr::null_mut()).is_none());
        }
    }

    #[test]
    fn primname_uses_canonical_table() {
        let _session = RSession::new();
        let primitive = unsafe { crate::mainutils::names::R_Primitive(c"+".as_ptr()) };
        assert_eq!(unsafe { PRIMNAME(primitive) }, "+");
    }
}
