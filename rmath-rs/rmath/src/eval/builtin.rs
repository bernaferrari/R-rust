#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Built-in function table and management — ports R's builtin.c.
//!
//! Provides:
//! - R_FunTab: the master function table for all builtins/specials
//! - PRIMFUN/PRIMNAME/PRIMPRINT accessors
//! - R_InitBuiltinSlots: initialize builtin function slots

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{CADR, CAR, CDDR, CDR, PRIMOFFSET, Rf_isNull, SET_PRIMOFFSET, TYPEOF};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory;

// ---------------------------------------------------------------------------
// Function table entry
// ---------------------------------------------------------------------------

/// Entry in R's function table (R_FunTab).
#[derive(Clone, Copy, Debug)]
pub struct FunTabEntry {
    /// Function name.
    pub name: &'static str,
    /// Number of arguments (-1 = variable).
    pub nargs: c_int,
    /// Function pointer (for C builtins/specials).
    pub fun: Option<unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP>,
    /// Offset in the function table.
    pub offset: c_int,
    /// Evaluation type (0 = special, 1 = builtin).
    pub kind: c_int,
    /// Print level (0 = visible, 1 = invisible).
    pub ppkind: c_int,
    /// Group (for group generics).
    pub group: &'static str,
}

// ---------------------------------------------------------------------------
// Function table (stub — populated at runtime)
// ---------------------------------------------------------------------------

/// R's master function table.
///
/// In the full implementation, this contains all ~400+ builtins and specials.
/// For now, we provide a minimal set.
static mut FUN_TAB: [FunTabEntry; 0] = [];

/// Get the function table.
pub unsafe fn R_FunTab() -> *const FunTabEntry {
    std::ptr::addr_of!(FUN_TAB) as *const FunTabEntry
}

/// Get the function table length.
pub unsafe fn R_FunTabSize() -> usize {
    unsafe { (*std::ptr::addr_of!(FUN_TAB)).len() }
}

// ---------------------------------------------------------------------------
// PRIMFUN — get the function pointer for a builtin/special
// ---------------------------------------------------------------------------

/// Get the function pointer for a primitive (SPECIAL or BUILTIN).
///
/// This is the equivalent of R's `PRIMFUN()` macro.
#[inline]
pub unsafe fn PRIMFUN(op: SEXP) -> Option<unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP> {
    unsafe {
        if op.is_null() {
            return None;
        }
        let t = TYPEOF(op);
        if t != SEXPTYPE::SPECIALSXP.0 && t != SEXPTYPE::BUILTINSXP.0 {
            return None;
        }
        let offset = PRIMOFFSET(op);
        if offset < 0 || offset as usize >= (*std::ptr::addr_of!(FUN_TAB)).len() {
            return None;
        }
        (*std::ptr::addr_of!(FUN_TAB))[offset as usize].fun
    }
}

/// Get the name of a primitive function.
///
/// This is the equivalent of R's `PRIMNAME()` macro.
pub unsafe fn PRIMNAME(op: SEXP) -> &'static str {
    unsafe {
        if op.is_null() {
            return "unknown";
        }
        let t = TYPEOF(op);
        if t != SEXPTYPE::SPECIALSXP.0 && t != SEXPTYPE::BUILTINSXP.0 {
            return "unknown";
        }
        let offset = PRIMOFFSET(op);
        if offset < 0 || offset as usize >= (*std::ptr::addr_of!(FUN_TAB)).len() {
            return "unknown";
        }
        (*std::ptr::addr_of!(FUN_TAB))[offset as usize].name
    }
}

// ---------------------------------------------------------------------------
// R_InitBuiltinSlots — initialize builtin function slots
// ---------------------------------------------------------------------------

/// Initialize the builtin function slots.
///
/// This is the equivalent of R's `R_InitBuiltinSlots()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitBuiltinSlots() {
    // In the full implementation, this walks R_FunTab and
    // creates SPECIALSXP/BUILTINSXP nodes for each entry.
    // For now, this is a stub.
}

// ---------------------------------------------------------------------------
// Create a primitive function SEXP
// ---------------------------------------------------------------------------

/// Create a SPECIALSXP or BUILTINSXP from a function table offset.
pub unsafe fn R_mkPrim(name: *const std::os::raw::c_char, offset: c_int, kind: c_int) -> SEXP {
    unsafe {
        let sexptype = if kind == 0 {
            SEXPTYPE::SPECIALSXP
        } else {
            SEXPTYPE::BUILTINSXP
        };

        memory::with_arena(|arena| {
            let prim = arena.alloc_node(sexptype);
            if !prim.is_null() {
                SET_PRIMOFFSET(prim, offset);
            }
            prim
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fun_tab_empty() {
        unsafe {
            assert_eq!(R_FunTabSize(), 0);
        }
    }

    #[test]
    fn test_primfun_null() {
        unsafe {
            assert!(PRIMFUN(ptr::null_mut()).is_none());
        }
    }

    #[test]
    fn test_primname_null() {
        unsafe {
            assert_eq!(PRIMNAME(ptr::null_mut()), "unknown");
        }
    }
}
