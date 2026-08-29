#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/envir.c — environment utility functions.
//!
//! This module ports the standalone utility functions from R's environment
//! management that don't require SEXP.
//!
//! Ported standalone functions:
//!   char_hash (djb2 string hash)
//!
//! Ported SEXP-dependent functions:
//!   do_attach, do_detach, do_search

// ---------------------------------------------------------------------------
// String hash function (djb2)
// ---------------------------------------------------------------------------

/// Default hash table size (must be a power of 2).
pub const CHAR_HASH_SIZE: u32 = 65536;

/// Hash mask (size - 1).
pub const CHAR_HASH_MASK: u32 = CHAR_HASH_SIZE - 1;

use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::ffi::{FALSE, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_NilValue};
use crate::sexp::protect::protect;
use std::os::raw::{c_char, c_int};

/// djb2 string hash function.
///
/// Computes a hash of the first `len` bytes of string `s`.
/// Uses the djb2 algorithm from http://www.cse.yorku.ca/~oz/hash.html
///
/// # Parameters
/// - `s`: pointer to string data
/// - `len`: number of bytes to hash
///
/// # Safety
/// `s` must point to at least `len` valid bytes.
pub(crate) unsafe fn char_hash(s: *const u8, len: std::os::raw::c_int) -> u32 {
    unsafe {
        let mut h: u32 = 5381;
        for i in 0..len as isize {
            let byte = *s.add(i as usize);
            h = h.wrapping_mul(33).wrapping_add(byte as u32);
        }
        h
    }
}

/// Safe Rust wrapper for `char_hash`.
pub fn char_hash_str(s: &[u8]) -> u32 {
    let mut h: u32 = 5381;
    for &byte in s {
        h = h.wrapping_mul(33).wrapping_add(byte as u32);
    }
    h
}

// ---------------------------------------------------------------------------
// Local helper functions
// ---------------------------------------------------------------------------

unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

/// Check if a CHARSXP is non-null and non-empty.
unsafe fn isValidStringF(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::CHARSXP && t != SEXPTYPE::STRSXP && t != SEXPTYPE::SYMSXP {
            return false;
        }
        if t == SEXPTYPE::STRSXP {
            if LENGTH(x) < 1 {
                return false;
            }
            let elt = STRING_ELT(x, 0);
            if elt.is_null() || elt == R_NilValue() {
                return false;
            }
            let p = CHAR(elt);
            return !p.is_null() && *p != 0;
        }
        // CHARSXP or SYMSXP
        let p = CHAR(x);
        !p.is_null() && *p != 0
    }
}

/// Check if x is a string (STRSXP).
unsafe fn isString(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP }
}

/// Check if x is an environment (ENVSXP).
unsafe fn isEnvironment(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::ENVSXP }
}

/// Check if x is a list/vector (VECSXP).
unsafe fn isNewList(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::VECSXP }
}

/// Get the "R_NameSymbol" — a pre-interned symbol for the "name" attribute.
unsafe fn R_NameSymbol() -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(b"name\0".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// do_attach — attach a data frame/list to search path
// ---------------------------------------------------------------------------

/// Attach a list or environment to the search path.
///
/// Simplified port of R's `do_attach` from envir.c.
/// `attach(what, pos = 2, name = deparse(substitute(what)))`
///
/// Creates a new environment from the list/environment's bindings and
/// inserts it into the search path at position `pos`.
pub unsafe fn do_attach(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::{getAttrib, setAttrib};
        use crate::sexp::envir::defineVar;
        use crate::sexp::memory_ext::allocSExp;

        if args.is_null() || args == R_NilValue() {
            error("invalid first argument");
        }

        let what = arg_by_name_or_position(args, "what", 0);

        // pos argument
        let pos_arg = arg_by_name_or_position(args, "pos", 1);
        let pos = if !pos_arg.is_null()
            && TYPEOF(pos_arg) == SEXPTYPE::INTSXP
            && LENGTH(pos_arg) >= 1
        {
            *INTEGER(pos_arg)
        } else if !pos_arg.is_null() && TYPEOF(pos_arg) == SEXPTYPE::REALSXP && LENGTH(pos_arg) >= 1
        {
            let r = *crate::sexp::accessors::REAL(pos_arg);
            r as c_int
        } else {
            2 // default position
        };

        // name argument
        let name_arg = arg_by_name_or_position(args, "name", 2);

        // Create a new environment
        let s = allocSExp(SEXPTYPE(SEXPTYPE::ENVSXP.as_c_int()));
        if s.is_null() {
            error("could not allocate environment");
        }
        let _s_guard = protect(s);

        // Copy bindings from the source (list or environment)
        if isNewList(what) {
            // It's a list/vector — walk its elements
            let names = getAttrib(what, R_NameSymbol());
            let n = LENGTH(what);
            for i in 0..n {
                let val = VECTOR_ELT(what, i as R_xlen_t);
                if !val.is_null() {
                    let sym_name = if !names.is_null() && isString(names) && LENGTH(names) > i {
                        let ns = STRING_ELT(names, i as R_xlen_t);
                        if !ns.is_null() && ns != R_NilValue() {
                            crate::sexp::symbol::Rf_install(CHAR(ns))
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    };
                    defineVar(sym_name, val, s);
                }
            }
        } else if isEnvironment(what) {
            // It's an environment — copy its frame bindings
            let frame = FRAME(what);
            let mut f = frame;
            while !f.is_null() && f != R_NilValue() {
                let sym = TAG(f);
                let val = CAR(f);
                if !sym.is_null() {
                    defineVar(sym, val, s);
                }
                f = CDR(f);
            }
        }

        // Set the name attribute on the new environment
        if isValidStringF(name_arg) {
            let name_str = if TYPEOF(name_arg) == SEXPTYPE::STRSXP {
                STRING_ELT(name_arg, 0)
            } else {
                name_arg
            };
            setAttrib(s, R_NameSymbol(), Rf_mkString(CHAR(name_str)));
        }

        // Insert into search path at position `pos`
        // Walk from R_GlobalEnv, counting down pos
        let mut t = R_GlobalEnv();
        if !t.is_null() && isEnvironment(t) {
            let base = R_BaseEnv();
            let mut remaining = pos;
            while remaining > 2 && !ENCLOS(t).is_null() && ENCLOS(t) != base {
                t = ENCLOS(t);
                remaining -= 1;
            }
            // Insert s between t and ENCLOS(t)
            let old_enclos = ENCLOS(t);
            SET_ENCLOS(t, s);
            SET_ENCLOS(s, old_enclos);
        }

        s
    }
}

unsafe fn arg_by_name_or_position(args: SEXP, name: &str, position: usize) -> SEXP {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if cell_tag_name(current).as_deref() == Some(name) {
                return CAR(current);
            }
            current = CDR(current);
        }

        let mut current = args;
        let mut index = 0usize;
        while !current.is_null() && current != R_NilValue() {
            if index == position {
                return CAR(current);
            }
            index += 1;
            current = CDR(current);
        }
        R_NilValue()
    }
}

unsafe fn cell_tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return None;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(chars)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

// ---------------------------------------------------------------------------
// do_detach — detach from search path
// ---------------------------------------------------------------------------

/// Detach an environment from the search path by position.
///
/// Simplified port of R's `do_detach` from envir.c.
/// `detach(name, pos = 2, unload = FALSE, character.only = FALSE)`
///
/// Removes the environment at position `pos` from the search list.
pub unsafe fn do_detach(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            error("invalid 'pos' argument");
        }

        let pos_arg = CAR(args);

        let global_env = R_GlobalEnv();
        let base_env = R_BaseEnv();

        if global_env.is_null() || base_env.is_null() {
            error("invalid 'pos' argument");
        }

        let pos = detach_position(pos_arg, global_env, base_env);

        // Count the total number of environments in the search path
        let mut n: c_int = 2; // GlobalEnv(1) + BaseEnv(last)
        let mut t = ENCLOS(global_env);
        while !t.is_null() && t != base_env {
            n += 1;
            t = ENCLOS(t);
        }

        // Cannot detach base
        if pos == n {
            error("detaching \"package:base\" is not allowed");
        }

        // Walk to the position
        let mut t = global_env;
        let mut remaining = pos;
        while remaining > 2 && !ENCLOS(t).is_null() && ENCLOS(t) != base_env {
            t = ENCLOS(t);
            remaining -= 1;
        }

        if remaining != 2 {
            error("invalid 'pos' argument");
        }

        // t now points to the environment before the one we want to detach
        let s = ENCLOS(t);
        if s.is_null() || s == base_env {
            error("invalid 'pos' argument");
        }

        let _s_guard = protect(s);
        let x = ENCLOS(s);
        SET_ENCLOS(t, x);
        SET_ENCLOS(s, base_env);

        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

unsafe fn detach_position(pos_arg: SEXP, global_env: SEXP, base_env: SEXP) -> c_int {
    unsafe {
        if !pos_arg.is_null() && TYPEOF(pos_arg) == SEXPTYPE::INTSXP && LENGTH(pos_arg) >= 1 {
            return *INTEGER(pos_arg);
        }
        if !pos_arg.is_null() && TYPEOF(pos_arg) == SEXPTYPE::REALSXP && LENGTH(pos_arg) >= 1 {
            return *crate::sexp::accessors::REAL(pos_arg) as c_int;
        }
        if !pos_arg.is_null() && TYPEOF(pos_arg) == SEXPTYPE::STRSXP && LENGTH(pos_arg) >= 1 {
            let target = string_elt(pos_arg, 0);
            if target.is_empty() {
                error("invalid 'name' argument");
            }
            let mut pos = 2;
            let mut env = ENCLOS(global_env);
            while !env.is_null() && env != base_env {
                let name = search_env_name(env);
                if name == target || name.strip_prefix("package:") == Some(target.as_str()) {
                    return pos;
                }
                pos += 1;
                env = ENCLOS(env);
            }
            error("invalid 'name' argument");
        }
        2
    }
}

unsafe fn search_env_name(env: SEXP) -> String {
    unsafe {
        let name = crate::eval::attrib_core::getAttrib(env, R_NameSymbol());
        if !isString(name) || LENGTH(name) < 1 {
            return String::new();
        }
        string_elt(name, 0)
    }
}

unsafe fn string_elt(x: SEXP, i: R_xlen_t) -> String {
    unsafe {
        let charsxp = STRING_ELT(x, i);
        if charsxp.is_null() || charsxp == R_NilValue() {
            return String::new();
        }
        let ptr = CHAR(charsxp);
        if ptr.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

// ---------------------------------------------------------------------------
// do_search — show search path
// ---------------------------------------------------------------------------

/// Show the search path as a character vector.
///
/// Port of R's `do_search` from envir.c.
/// Returns a STRSXP with the names of all environments on the search path,
/// from .GlobalEnv to package:base.
pub unsafe fn do_search(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::getAttrib;
        use crate::sexp::constructors::{Rf_allocVector, Rf_mkChar};

        let global_env = R_GlobalEnv();
        let base_env = R_BaseEnv();

        if global_env.is_null() || base_env.is_null() {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }

        // Count environments in search path
        let mut n: c_int = 2; // .GlobalEnv + package:base
        let mut t = ENCLOS(global_env);
        while !t.is_null() && t != base_env {
            n += 1;
            t = ENCLOS(t);
        }

        let ans = Rf_allocVector(SEXPTYPE::STRSXP, n);
        if ans.is_null() {
            error("could not allocate search result");
        }
        let _ans_guard = protect(ans);

        // First element is always ".GlobalEnv"
        SET_STRING_ELT(ans, 0, Rf_mkChar(b".GlobalEnv\0".as_ptr() as *const c_char));

        // Last element is always "package:base"
        SET_STRING_ELT(
            ans,
            (n - 1) as R_xlen_t,
            Rf_mkChar(b"package:base\0".as_ptr() as *const c_char),
        );

        // Fill in middle elements
        let mut i: c_int = 1;
        let mut t = ENCLOS(global_env);
        while !t.is_null() && t != base_env {
            let name = getAttrib(t, R_NameSymbol());
            if !isString(name) || LENGTH(name) < 1 {
                SET_STRING_ELT(
                    ans,
                    i as R_xlen_t,
                    Rf_mkChar(b"(unknown)\0".as_ptr() as *const c_char),
                );
            } else {
                let name_elt = STRING_ELT(name, 0);
                if !name_elt.is_null() && name_elt != R_NilValue() {
                    SET_STRING_ELT(ans, i as R_xlen_t, name_elt);
                } else {
                    SET_STRING_ELT(
                        ans,
                        i as R_xlen_t,
                        Rf_mkChar(b"(unknown)\0".as_ptr() as *const c_char),
                    );
                }
            }
            i += 1;
            t = ENCLOS(t);
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_hash_empty() {
        assert_eq!(char_hash_str(b""), 5381);
    }

    #[test]
    fn test_char_hash_deterministic() {
        let h1 = char_hash_str(b"hello");
        let h2 = char_hash_str(b"hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_char_hash_different_strings() {
        let h1 = char_hash_str(b"hello");
        let h2 = char_hash_str(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_char_hash_known_values() {
        // djb2 hash of "hello" should produce consistent results
        let h = char_hash_str(b"hello");
        // Known djb2 value for "hello"
        assert_eq!(h, 261238937);
    }

    #[test]
    fn test_char_hash_unicode() {
        // Hash should work on any byte sequence
        let h1 = char_hash_str("café".as_bytes());
        let h2 = char_hash_str("café".as_bytes());
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_char_hash_ffi() {
        let s = b"test";
        let h = unsafe { char_hash(s.as_ptr(), s.len() as i32) };
        assert_eq!(h, char_hash_str(s));
    }
}
