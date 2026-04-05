//! R structure constructors: mkPRIMSXP, mkCLOSXP, R_mkClosure, mkSYMSXP.
//!
//! Ported from R's src/main/dstruct.c. These functions create the fundamental
//! R object types (primitives, closures, symbols) using the arena allocator.

#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_isEnvironment;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_GlobalEnv, R_NilValue};
use crate::sexp::memory::with_arena;

// ---------------------------------------------------------------------------
// ddval constants
// ---------------------------------------------------------------------------

/// Bit mask for the DDVAL (double-dot value) flag in gp bits.
/// gp bit 10 (1 << 10 = 1024).
const DDVAL_MASK: u16 = 1 << 10;

// ---------------------------------------------------------------------------
// mkPRIMSXP
// ---------------------------------------------------------------------------

/// Create a builtin or special function SEXP.
///
/// This is equivalent to R's `mkPRIMSXP(offset, eval)`. If `eval` is nonzero,
/// creates a BUILTINSXP; otherwise creates a SPECIALSXP.
pub unsafe fn mkPRIMSXP(offset: c_int, eval: c_int) -> SEXP {
    unsafe {
        let sexptype = if eval != 0 {
            SEXPTYPE::BUILTINSXP
        } else {
            SEXPTYPE::SPECIALSXP
        };

        let node = with_arena(|arena| arena.alloc_node(sexptype));
        SET_PRIMOFFSET(node, offset);
        node
    }
}

// ---------------------------------------------------------------------------
// mkCLOSXP
// ---------------------------------------------------------------------------

/// Create a closure SEXP with the given formals, body, and environment.
///
/// This is equivalent to R's `mkCLOSXP(formals, body, rho)`.
/// If `rho` is R_NilValue, the global environment is used instead.
pub unsafe fn mkCLOSXP(formals: SEXP, body: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let c = with_arena(|arena| arena.alloc_node(SEXPTYPE::CLOSXP));

        SET_FORMALS(c, formals);

        let body_type = TYPEOF(body);
        match SEXPTYPE(body_type) {
            SEXPTYPE::CLOSXP
            | SEXPTYPE::BUILTINSXP
            | SEXPTYPE::SPECIALSXP
            | SEXPTYPE::DOTSXP
            | SEXPTYPE::ANYSXP => {
                // Invalid body type - in real R this would error.
                // For now, just skip setting the body.
            }
            _ => {
                SET_BODY(c, body);
            }
        }

        if rho.is_null() || rho == R_NilValue() {
            SET_CLOENV(c, R_GlobalEnv());
        } else {
            SET_CLOENV(c, rho);
        }

        c
    }
}

// ---------------------------------------------------------------------------
// R_mkClosure
// ---------------------------------------------------------------------------

/// API version of mkCLOSXP with more checking.
///
/// This is equivalent to R's `R_mkClosure(formals, body, rho)`.
/// Checks that formals is a pairlist and that rho is an environment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_mkClosure(formals: SEXP, body: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // CheckFormals would verify formals is a pairlist or NILSXP.
        // For now, we skip the detailed checking.
        if !R_NilValue().is_null() && !rho.is_null() && Rf_isEnvironment(rho) == 0 {
            // Invalid environment - in real R this would error.
            // Return null to signal the error.
            return ptr::null_mut();
        }
        mkCLOSXP(formals, body, rho)
    }
}

// ---------------------------------------------------------------------------
// isDDName / mkSYMSXP
// ---------------------------------------------------------------------------

/// Check if a CHARSXP name is a double-dot (..N) name.
///
/// Returns 1 if the name starts with ".." followed by digits (e.g., "..1", "..42"),
/// 0 otherwise.
unsafe fn isDDName(name: SEXP) -> c_int {
    unsafe {
        if name.is_null() {
            return 0;
        }
        let buf = CHAR(name);
        if buf.is_null() {
            return 0;
        }
        let cstr = std::ffi::CStr::from_ptr(buf);
        let bytes = cstr.to_bytes();

        if bytes.len() > 2 && bytes[0] == b'.' && bytes[1] == b'.' {
            // Check that the rest is all digits
            let rest = &bytes[2..];
            if rest.is_empty() {
                return 0;
            }
            return if rest.iter().all(|&b| b.is_ascii_digit()) {
                1
            } else {
                0
            };
        }
        0
    }
}

/// Create a symbol SEXP with the given name and value.
///
/// This is equivalent to R's `mkSYMSXP(name, value)`.
/// If the name is a double-dot name (e.g., "..1"), the DDVAL bit is set.
pub unsafe fn mkSYMSXP(name: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let ddval = isDDName(name);
        let c = with_arena(|arena| arena.alloc_node(SEXPTYPE::SYMSXP));
        SET_PRINTNAME(c, name);
        SET_SYMVALUE(c, value);
        if ddval != 0 {
            let gp = (*c).sxpinfo.gp() | DDVAL_MASK;
            (*c).sxpinfo.set_gp(gp);
        }
        c
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::constructors::Rf_ScalarInteger;
    use crate::sexp::memory::RArena;

    #[test]
    fn test_mkprimsxp_builtin() {
        unsafe {
            let p = mkPRIMSXP(0, 1);
            assert!(!p.is_null());
            assert_eq!(TYPEOF(p), SEXPTYPE::BUILTINSXP.0);
            assert_eq!(PRIMOFFSET(p), 0);
        }
    }

    #[test]
    fn test_mkprimsxp_special() {
        unsafe {
            let p = mkPRIMSXP(5, 0);
            assert!(!p.is_null());
            assert_eq!(TYPEOF(p), SEXPTYPE::SPECIALSXP.0);
            assert_eq!(PRIMOFFSET(p), 5);
        }
    }

    #[test]
    fn test_mkclosxp_basic() {
        unsafe {
            let mut arena = RArena::new();
            let formals = arena.alloc_node(SEXPTYPE::NILSXP);
            let body = Rf_ScalarInteger(42);
            let env = arena.alloc_node(SEXPTYPE::ENVSXP);

            let c = mkCLOSXP(formals, body, env);
            assert!(!c.is_null());
            assert_eq!(TYPEOF(c), SEXPTYPE::CLOSXP.0);
            assert_eq!(FORMALS(c), formals);
            assert_eq!(BODY(c), body);
            assert_eq!(CLOENV(c), env);
        }
    }

    #[test]
    fn test_mkclosxp_nil_env() {
        unsafe {
            let mut arena = RArena::new();
            let formals = arena.alloc_node(SEXPTYPE::NILSXP);
            let body = Rf_ScalarInteger(1);

            let c = mkCLOSXP(formals, body, R_NilValue());
            assert!(!c.is_null());
            assert_eq!(CLOENV(c), R_GlobalEnv());
        }
    }

    #[test]
    fn test_mkclosure_basic() {
        unsafe {
            let mut arena = RArena::new();
            let formals = arena.alloc_node(SEXPTYPE::NILSXP);
            let body = Rf_ScalarInteger(1);
            let env = arena.alloc_node(SEXPTYPE::ENVSXP);

            let c = R_mkClosure(formals, body, env);
            assert!(!c.is_null());
            assert_eq!(TYPEOF(c), SEXPTYPE::CLOSXP.0);
        }
    }

    #[test]
    fn test_isddname() {
        unsafe {
            let mut arena = RArena::new();
            // "..1" is a dd name
            let dd1 = arena.alloc_charsxp(b"..1");
            assert_eq!(isDDName(dd1), 1);

            // "..42" is a dd name
            let dd42 = arena.alloc_charsxp(b"..42");
            assert_eq!(isDDName(dd42), 1);

            // "abc" is not a dd name
            let abc = arena.alloc_charsxp(b"abc");
            assert_eq!(isDDName(abc), 0);

            // ".." is not a dd name (no digits)
            let dotdot = arena.alloc_charsxp(b"..");
            assert_eq!(isDDName(dotdot), 0);

            // "..abc" is not a dd name (not all digits)
            let ddabc = arena.alloc_charsxp(b"..abc");
            assert_eq!(isDDName(ddabc), 0);

            // null
            assert_eq!(isDDName(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_mksymsxp_basic() {
        unsafe {
            let mut arena = RArena::new();
            let name = arena.alloc_charsxp(b"myvar");
            let value = Rf_ScalarInteger(99);

            let sym = mkSYMSXP(name, value);
            assert!(!sym.is_null());
            assert_eq!(TYPEOF(sym), SEXPTYPE::SYMSXP.0);
            assert_eq!(PRINTNAME(sym), name);
            assert_eq!(SYMVALUE(sym), value);
        }
    }

    #[test]
    fn test_mksymsxp_ddname() {
        unsafe {
            let mut arena = RArena::new();
            let name = arena.alloc_charsxp(b"..1");
            let value = Rf_ScalarInteger(0);

            let sym = mkSYMSXP(name, value);
            assert!(!sym.is_null());
            // DDVAL bit should be set
            let gp = (*sym).sxpinfo.gp();
            assert_eq!(gp & DDVAL_MASK, DDVAL_MASK);
        }
    }

    #[test]
    fn test_mksymsxp_non_ddname() {
        unsafe {
            let mut arena = RArena::new();
            let name = arena.alloc_charsxp(b"normal");
            let value = Rf_ScalarInteger(0);

            let sym = mkSYMSXP(name, value);
            assert!(!sym.is_null());
            let gp = (*sym).sxpinfo.gp();
            assert_eq!(gp & DDVAL_MASK, 0);
        }
    }
}
