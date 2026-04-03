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
//!   do_ls, do_get, do_assign, do_remove, do_attach, do_detach, do_search

// ---------------------------------------------------------------------------
// String hash function (djb2)
// ---------------------------------------------------------------------------

/// Default hash table size (must be a power of 2).
pub const CHAR_HASH_SIZE: u32 = 65536;

/// Hash mask (size - 1).
pub const CHAR_HASH_MASK: u32 = CHAR_HASH_SIZE - 1;

use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_ScalarLogical;
use crate::sexp::ffi::{FALSE, R_xlen_t, Rboolean, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_BaseEnv, R_EmptyEnv, R_GlobalEnv, R_NilValue, R_UnboundValue};
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn char_hash(s: *const u8, len: std::os::raw::c_int) -> u32 {
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

/// Check if a CHARSXP is non-null and non-empty.
unsafe fn isValidStringF(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::CHARSXP.0 && t != SEXPTYPE::STRSXP.0 && t != SEXPTYPE::SYMSXP.0 {
            return false;
        }
        if t == SEXPTYPE::STRSXP.0 {
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

/// Install a symbol from a CHARSXP (install the string of the CHARSXP).
unsafe fn installTrChar(x: SEXP) -> SEXP {
    unsafe {
        let p = CHAR(x);
        if p.is_null() {
            return R_NilValue();
        }
        crate::sexp::symbol::Rf_install(p)
    }
}

/// Check if x is a string (STRSXP).
unsafe fn isString(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP.0 }
}

/// Check if x is an environment (ENVSXP).
unsafe fn isEnvironment(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::ENVSXP.0 }
}

/// Check if x is a list/vector (VECSXP).
unsafe fn isNewList(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::VECSXP.0 }
}

/// Check if x is NULL (R_NilValue).
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

/// Get the "R_NameSymbol" — a pre-interned symbol for the "name" attribute.
unsafe fn R_NameSymbol() -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(b"name\0".as_ptr() as *const c_char) }
}

/// Fourth element of a list (CAD4R).
unsafe fn CAD4R(x: SEXP) -> SEXP {
    unsafe { CDR(CDR(CDR(CDR(x)))) }
}

/// Remove a binding from an environment's frame (pairlist).
/// Returns the new frame head.
unsafe fn RemoveFromList(name: SEXP, list: SEXP, found: *mut c_int) -> SEXP {
    unsafe {
        if list.is_null() || list == R_NilValue() {
            *found = 0;
            return R_NilValue();
        }
        // If the first element matches, skip it
        if TAG(list) == name {
            *found = 1;
            return CDR(list);
        }
        // Walk the list looking for the match
        let mut prev = list;
        let mut current = CDR(list);
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == name {
                *found = 1;
                // Unlink current from the list
                SETCDR(prev, CDR(current));
                return list;
            }
            prev = current;
            current = CDR(current);
        }
        *found = 0;
        list
    }
}

/// Simple string comparison from C string pointers.
unsafe fn strcmp_c(a: *const c_char, b: *const c_char) -> bool {
    unsafe {
        if a.is_null() || b.is_null() {
            return a == b;
        }
        std::ffi::CStr::from_ptr(a) == std::ffi::CStr::from_ptr(b)
    }
}

/// Check if a printname starts with '.' (i.e., is hidden).
unsafe fn name_starts_with_dot(frame_cell: SEXP) -> bool {
    unsafe {
        let pn = PRINTNAME(TAG(frame_cell));
        if pn.is_null() {
            return false;
        }
        let p = CHAR(pn);
        !p.is_null() && *p == '.' as c_char
    }
}

/// Sort a STRSXP vector in-place (simple insertion sort for correctness).
/// Equivalent of R's sortVector(ans, FALSE) for STRSXP.
unsafe fn sortStringVector(ans: SEXP) {
    unsafe {
        let n = LENGTH(ans);
        if n <= 1 {
            return;
        }
        // Insertion sort
        for i in 1..n {
            let key = STRING_ELT(ans, i as R_xlen_t);
            let key_ptr = CHAR(key);
            let mut j = i - 1;
            while j >= 0 {
                let j_elt = STRING_ELT(ans, j as R_xlen_t);
                let j_ptr = CHAR(j_elt);
                // Compare strings: if j_ptr > key_ptr, shift right
                if !j_ptr.is_null() && !key_ptr.is_null() {
                    let j_cstr = std::ffi::CStr::from_ptr(j_ptr);
                    let k_cstr = std::ffi::CStr::from_ptr(key_ptr);
                    if j_cstr <= k_cstr {
                        break;
                    }
                }
                SET_STRING_ELT(ans, (j + 1) as R_xlen_t, j_elt);
                j -= 1;
            }
            SET_STRING_ELT(ans, (j + 1) as R_xlen_t, key);
        }
    }
}

// ---------------------------------------------------------------------------
// R_lsInternal3 — core listing logic
// ---------------------------------------------------------------------------

/// List variable names in an environment.
///
/// Simplified version of R's `R_lsInternal3`. Walks the frame pairlist
/// and collects names (optionally filtering hidden names starting with '.').
/// Does not handle hash tables or builtin environments in this simplified port.
unsafe fn R_lsInternal3(env: SEXP, all: c_int, sorted: c_int) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_allocVector;

        if env.is_null() || !isEnvironment(env) {
            // Return empty vector for non-environments
            return Rf_allocVector(SEXPTYPE::STRSXP.0, 0);
        }

        // Step 1: Count names
        let frame = FRAME(env);
        let mut count: c_int = 0;
        let mut f = frame;
        while !f.is_null() && f != R_NilValue() {
            if all != 0 || !name_starts_with_dot(f) {
                count += 1;
            }
            f = CDR(f);
        }

        // Step 2: Allocate and fill
        let ans = Rf_allocVector(SEXPTYPE::STRSXP.0, count);
        let mut idx: c_int = 0;
        f = frame;
        while !f.is_null() && f != R_NilValue() {
            if all != 0 || !name_starts_with_dot(f) {
                let pn = PRINTNAME(TAG(f));
                SET_STRING_ELT(ans, idx as R_xlen_t, pn);
                idx += 1;
            }
            f = CDR(f);
        }

        // Step 3: Sort if requested
        if sorted != 0 && count > 1 {
            sortStringVector(ans);
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_ls — list variables in an environment (like R's ls())
// ---------------------------------------------------------------------------

/// List variables in an environment.
///
/// Port of R's `do_ls` from envir.c.
/// `.Internal(ls(envir, all.names, sorted))`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_ls(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::mainutils::coerce::asLogical;
        use crate::sexp::constructors::Rf_allocVector;
        use crate::sexp::protect::Rf_protect;
        use crate::sexp::protect::Rf_unprotect;

        if args.is_null() || args == R_NilValue() {
            return Rf_allocVector(SEXPTYPE::STRSXP.0, 0);
        }

        let env = CAR(args);
        let all = asLogical(CADR(args));
        let all = if all == c_int::MIN { 0 } else { all };

        let sort_nms = asLogical(CADDR(args));
        let sort_nms = if sort_nms == c_int::MIN { 0 } else { sort_nms };

        R_lsInternal3(env, all, sort_nms)
    }
}

// ---------------------------------------------------------------------------
// do_get — get a variable from an environment (like R's get())
// ---------------------------------------------------------------------------

/// Get a variable from an environment.
///
/// Port of R's `do_get` from envir.c.
/// `get(x, envir, mode, inherits)` — returns the value of x found in envir.
///
/// This is a simplified port that handles the core case:
/// - x is a SYMSXP or string
/// - envir is an environment
/// - mode checking is done for common types
/// - inherits controls parent environment search
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_get(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_ScalarString;
        use crate::sexp::envir::R_findVarInFrame;
        use crate::sexp::symbol::Rf_install;

        if args.is_null() || args == R_NilValue() {
            return R_UnboundValue();
        }

        // First arg: the object name (SYMSXP or string)
        let raw = CAR(args);
        let t1 = if TYPEOF(raw) == SEXPTYPE::SYMSXP.0 {
            raw
        } else if isValidStringF(raw) && TYPEOF(raw) == SEXPTYPE::STRSXP.0 {
            let s = STRING_ELT(raw, 0);
            if s.is_null() || s == R_NilValue() {
                return R_UnboundValue();
            }
            Rf_install(CHAR(s))
        } else {
            return R_UnboundValue();
        };

        // Second arg: envir (environment)
        let genv = CADR(args);
        if !isEnvironment(genv) {
            return R_UnboundValue();
        }

        // Third arg: mode (string, e.g. "any", "function", "numeric")
        let mode_arg = CADDR(args);

        // Fourth arg: inherits
        let rest4 = CDR(CDR(CDR(args)));
        let ginherits = if rest4.is_null() || rest4 == R_NilValue() {
            1
        } else {
            let v = CAR(rest4);
            if !v.is_null() && TYPEOF(v) == SEXPTYPE::LGLSXP.0 {
                let lv = *LOGICAL(v);
                if lv == FALSE { 0 } else { 1 }
            } else {
                1
            }
        };

        // Search for the object in the environment chain
        let mut rval = R_UnboundValue();
        let mut current_env = genv;
        loop {
            if current_env.is_null() || !isEnvironment(current_env) {
                break;
            }
            let val = R_findVarInFrame(current_env, t1);
            if val != R_UnboundValue() {
                // Check mode if specified
                let mode_ok = if !isString(mode_arg) || LENGTH(mode_arg) < 1 {
                    true
                } else {
                    let mode_str = CHAR(STRING_ELT(mode_arg, 0));
                    if mode_str.is_null() {
                        true
                    } else {
                        let cs = std::ffi::CStr::from_ptr(mode_str);
                        let vt = TYPEOF(val);
                        match cs.to_str() {
                            Ok("any") => true,
                            Ok("function") => {
                                vt == SEXPTYPE::CLOSXP.0
                                    || vt == SEXPTYPE::BUILTINSXP.0
                                    || vt == SEXPTYPE::SPECIALSXP.0
                            }
                            Ok("numeric") | Ok("double") => {
                                vt == SEXPTYPE::REALSXP.0 || vt == SEXPTYPE::INTSXP.0
                            }
                            Ok("integer") => vt == SEXPTYPE::INTSXP.0,
                            Ok("complex") => vt == SEXPTYPE::CPLXSXP.0,
                            Ok("logical") => vt == SEXPTYPE::LGLSXP.0,
                            Ok("list") => vt == SEXPTYPE::VECSXP.0 || vt == SEXPTYPE::LISTSXP.0,
                            Ok("character") | Ok("string") => {
                                vt == SEXPTYPE::STRSXP.0 || vt == SEXPTYPE::CHARSXP.0
                            }
                            Ok("environment") => vt == SEXPTYPE::ENVSXP.0,
                            _ => true,
                        }
                    }
                };
                if mode_ok {
                    rval = val;
                }
                break;
            }
            if ginherits == 0 {
                break;
            }
            current_env = ENCLOS(current_env);
        }

        rval
    }
}

// ---------------------------------------------------------------------------
// do_assign — assign a value in an environment (like R's assign())
// ---------------------------------------------------------------------------

/// Assign a value to a variable in an environment.
///
/// Port of R's `do_assign` from envir.c.
/// `.Internal(assign(x, value, envir, inherits))`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_assign(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::mainutils::coerce::asLogical;
        use crate::sexp::envir::{defineVar, setVar};
        use crate::sexp::protect::Rf_protect;
        use crate::sexp::protect::Rf_unprotect;
        use crate::sexp::symbol::Rf_install;

        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        // First arg: variable name (string)
        let name_arg = CAR(args);
        let sym = if isString(name_arg) && LENGTH(name_arg) >= 1 {
            let s = STRING_ELT(name_arg, 0);
            if s.is_null() || s == R_NilValue() {
                return R_NilValue();
            }
            Rf_install(CHAR(s))
        } else if TYPEOF(name_arg) == SEXPTYPE::SYMSXP.0 {
            name_arg
        } else {
            return R_NilValue();
        };

        // Second arg: value
        let val = CADR(args);
        Rf_protect(val);

        // Third arg: envir
        let aenv = CADDR(args);
        if !isEnvironment(aenv) {
            Rf_unprotect(1);
            return R_NilValue();
        }

        // Fourth arg: inherits
        let rest4 = CDR(CDR(CDR(args)));
        let ginherits = if rest4.is_null() || rest4 == R_NilValue() {
            1
        } else {
            let v = CAR(rest4);
            if !v.is_null() && TYPEOF(v) == SEXPTYPE::LGLSXP.0 {
                let lv = *LOGICAL(v);
                if lv == FALSE { 0 } else { 1 }
            } else {
                1
            }
        };

        if ginherits != 0 {
            setVar(sym, val, aenv);
        } else {
            defineVar(sym, val, aenv);
        }

        Rf_unprotect(1);
        val
    }
}

// ---------------------------------------------------------------------------
// do_remove — remove a variable from an environment (like R's rm())
// ---------------------------------------------------------------------------

/// Remove variables from an environment.
///
/// Port of R's `do_remove` from envir.c.
/// `.Internal(remove(list, envir, inherits))`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_remove(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::mainutils::coerce::asLogical;
        use crate::sexp::accessors::SETCDR;
        use crate::sexp::symbol::Rf_install;

        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        // First arg: list of names to remove
        let name = CAR(args);
        if isNull(name) {
            return R_NilValue();
        }
        if !isString(name) {
            return R_NilValue();
        }

        let mut rest = CDR(args);

        // Second arg: envir
        let envarg = if !rest.is_null() && rest != R_NilValue() {
            CAR(rest)
        } else {
            R_GlobalEnv()
        };
        rest = CDR(rest);

        // Third arg: inherits
        let ginherits = if rest.is_null() || rest == R_NilValue() {
            1
        } else {
            let v = CAR(rest);
            if !v.is_null() && TYPEOF(v) == SEXPTYPE::LGLSXP.0 {
                let lv = *LOGICAL(v);
                if lv == FALSE { 0 } else { 1 }
            } else {
                1
            }
        };

        let n = LENGTH(name);
        for i in 0..n {
            let elt = STRING_ELT(name, i as R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            let tsym = Rf_install(CHAR(elt));

            // Search through environments
            let mut tenv = envarg;
            let mut done = false;
            while !tenv.is_null() && isEnvironment(tenv) && tenv != R_EmptyEnv() {
                // RemoveFromList from this environment's frame
                let mut found: c_int = 0;
                let new_frame = RemoveFromList(tsym, FRAME(tenv), &mut found);
                if found != 0 {
                    SET_FRAME(tenv, new_frame);
                    done = true;
                }
                if done || ginherits == 0 {
                    break;
                }
                tenv = ENCLOS(tenv);
            }
        }

        R_NilValue()
    }
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_attach(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::{getAttrib, setAttrib};
        use crate::mainutils::coerce::asInteger;
        use crate::sexp::accessors::SETCDR;
        use crate::sexp::constructors::Rf_cons;
        use crate::sexp::envir::defineVar;
        use crate::sexp::memory_ext::allocSExp;
        use crate::sexp::protect::Rf_protect;
        use crate::sexp::protect::Rf_unprotect;

        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        // pos argument
        let pos_arg = CADR(args);
        let pos = if !pos_arg.is_null()
            && TYPEOF(pos_arg) == SEXPTYPE::INTSXP.0
            && LENGTH(pos_arg) >= 1
        {
            *INTEGER(pos_arg)
        } else if !pos_arg.is_null()
            && TYPEOF(pos_arg) == SEXPTYPE::REALSXP.0
            && LENGTH(pos_arg) >= 1
        {
            let r = *crate::sexp::accessors::REAL(pos_arg);
            r as c_int
        } else {
            2 // default position
        };

        // name argument
        let name_arg = CADDR(args);

        // Create a new environment
        let s = allocSExp(SEXPTYPE {
            0: SEXPTYPE::ENVSXP.0,
        });
        if s.is_null() {
            return R_NilValue();
        }
        Rf_protect(s);

        // Copy bindings from the source (list or environment)
        let what = CAR(args);
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
            let name_str = if TYPEOF(name_arg) == SEXPTYPE::STRSXP.0 {
                STRING_ELT(name_arg, 0)
            } else {
                name_arg
            };
            setAttrib(s, R_NameSymbol(), name_str);
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

        Rf_unprotect(1);
        s
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_detach(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::mainutils::coerce::asInteger;
        use crate::sexp::accessors::SETCDR;
        use crate::sexp::protect::Rf_protect;
        use crate::sexp::protect::Rf_unprotect;

        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        let pos_arg = CAR(args);
        let pos = if !pos_arg.is_null()
            && TYPEOF(pos_arg) == SEXPTYPE::INTSXP.0
            && LENGTH(pos_arg) >= 1
        {
            *INTEGER(pos_arg)
        } else if !pos_arg.is_null()
            && TYPEOF(pos_arg) == SEXPTYPE::REALSXP.0
            && LENGTH(pos_arg) >= 1
        {
            let r = *crate::sexp::accessors::REAL(pos_arg);
            r as c_int
        } else {
            2 // default
        };

        let global_env = R_GlobalEnv();
        let base_env = R_BaseEnv();

        if global_env.is_null() || base_env.is_null() {
            return R_NilValue();
        }

        // Count the total number of environments in the search path
        let mut n: c_int = 2; // GlobalEnv(1) + BaseEnv(last)
        let mut t = ENCLOS(global_env);
        while !t.is_null() && t != base_env {
            n += 1;
            t = ENCLOS(t);
        }

        // Cannot detach base
        if pos == n {
            return R_NilValue();
        }

        // Walk to the position
        let mut t = global_env;
        let mut remaining = pos;
        while remaining > 2 && !ENCLOS(t).is_null() && ENCLOS(t) != base_env {
            t = ENCLOS(t);
            remaining -= 1;
        }

        if remaining != 2 {
            return R_NilValue();
        }

        // t now points to the environment before the one we want to detach
        let s = ENCLOS(t);
        if s.is_null() || s == base_env {
            return R_NilValue();
        }

        Rf_protect(s);
        let x = ENCLOS(s);
        SET_ENCLOS(t, x);
        SET_ENCLOS(s, base_env);

        Rf_unprotect(1);
        s
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_search(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::getAttrib;
        use crate::sexp::constructors::{Rf_allocVector, Rf_mkChar};
        use crate::sexp::protect::Rf_protect;
        use crate::sexp::protect::Rf_unprotect;

        let global_env = R_GlobalEnv();
        let base_env = R_BaseEnv();

        if global_env.is_null() || base_env.is_null() {
            return Rf_allocVector(SEXPTYPE::STRSXP.0, 0);
        }

        // Count environments in search path
        let mut n: c_int = 2; // .GlobalEnv + package:base
        let mut t = ENCLOS(global_env);
        while !t.is_null() && t != base_env {
            n += 1;
            t = ENCLOS(t);
        }

        let ans = Rf_allocVector(SEXPTYPE::STRSXP.0, n);
        if ans.is_null() {
            return R_NilValue();
        }
        Rf_protect(ans);

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

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_exists — check if a variable exists in an environment
// ---------------------------------------------------------------------------

/// Check if a variable exists in an environment.
///
/// Equivalent of R's `do_exists()` from envir.c.
/// `exists(x, envir, mode, inherits)` -> logical
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_exists(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::envir::R_findVarInFrame;
        use crate::sexp::symbol::Rf_install;

        if args.is_null() || args == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let x = CAR(args);
        let mut rest = CDR(args);

        // envir argument
        let envir = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            let e = CAR(rest);
            rest = CDR(rest);
            e
        };

        // mode argument (default "any")
        let mode = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            CAR(rest)
        };

        // inherits argument
        let inherits = if CDR(rest).is_null() || CDR(rest) == R_NilValue() {
            TRUE as c_int
        } else {
            let v = CAR(CDR(rest));
            if !v.is_null() && TYPEOF(v) == SEXPTYPE::LGLSXP.0 {
                let lv = *LOGICAL(v);
                if lv == FALSE { 0 } else { 1 }
            } else {
                1
            }
        };

        // Get variable name
        let sym = if TYPEOF(x) == SEXPTYPE::SYMSXP.0 {
            x
        } else {
            let s = CAR(x);
            if s.is_null() || s == R_NilValue() {
                return Rf_ScalarLogical(FALSE);
            }
            // Try to convert string to symbol
            let nm = CHAR(s);
            if nm.is_null() {
                return Rf_ScalarLogical(FALSE);
            }
            Rf_install(nm)
        };

        // Search for the variable
        let mut result = FALSE;
        let mut env = envir;
        loop {
            if env.is_null() || env == R_NilValue() {
                break;
            }
            let val = R_findVarInFrame(env, sym);
            if val != R_UnboundValue() {
                // Check mode
                let matches_mode = if mode.is_null() || mode == R_NilValue() {
                    true
                } else {
                    let mode_str = CHAR(mode);
                    if mode_str.is_null() {
                        true
                    } else {
                        let cs = std::ffi::CStr::from_ptr(mode_str);
                        let Ok(ms) = cs.to_str() else {
                            return Rf_ScalarLogical(TRUE);
                        };
                        let vt = TYPEOF(val);
                        match ms {
                            "function" => {
                                vt == SEXPTYPE::CLOSXP.0
                                    || vt == SEXPTYPE::BUILTINSXP.0
                                    || vt == SEXPTYPE::SPECIALSXP.0
                            }
                            "numeric" | "double" => {
                                vt == SEXPTYPE::REALSXP.0 || vt == SEXPTYPE::INTSXP.0
                            }
                            "integer" => vt == SEXPTYPE::INTSXP.0,
                            "complex" => vt == SEXPTYPE::CPLXSXP.0,
                            "logical" => vt == SEXPTYPE::LGLSXP.0,
                            "list" => vt == SEXPTYPE::VECSXP.0 || vt == SEXPTYPE::LISTSXP.0,
                            "environment" => vt == SEXPTYPE::ENVSXP.0,
                            _ => true,
                        }
                    }
                };
                if matches_mode {
                    result = TRUE;
                }
                break;
            }
            if inherits == 0 {
                break;
            }
            // Move to parent environment (simplified: check ENCLOS)
            env = ENCLOS(env);
        }
        Rf_ScalarLogical(result)
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
