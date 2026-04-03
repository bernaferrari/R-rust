#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of language-related utility functions from R's src/main/.
//!
//! This module implements do_* functions for:
//! - Language constructs: nargs, missing, Recall, on.exit, forceAndCall, quote,
//!   substitute, call, switch, browser, declare
//! - Debug: undebug, isdebugged, debugonce, .primTrace, .primUntrace
//! - Lazy evaluation: delayedAssign, makeLazy
//! - Dots: ...elt, ...length, ...names
//! - Class: .cache_class, .class2, as.function.default
//! - Environments: get0, mget, list2env, new.env, pos.to.env, environment<-
//! - Replacement: length<-, storage.mode<-, substr<-
//! - Misc: vector, Version, internalsID, memory.profile, builtins, vhash,
//!   setFileTime, sample2, cat, do.call, str2lang, str2expression,
//!   match.call, strsplit, agrepl, xtfrm, inspect, system, quit, readline,
//!   parse, eval, sort, order, eapply, polyroot, compareNumericVersion,
//!   switch, .C, .Fortran, .Call, .External, .External2,
//!   .Call.graphics, .External.graphics, ::, :::, @<-
//! - Condition/error helpers: .addGlobHands, C_tryCatchHelper,
//!   getNamespaceValue

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::main::coerce::{asInteger, asLogical, coerceVector};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::forcePromise;
use crate::sexp::ffi::{
    FALSE, ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::*;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// nargs() — number of arguments supplied to the current function
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_nargs(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // In a full implementation, this would inspect the current call.
        // For now, return 0 (no args tracked in this stub interpreter).
        Rf_ScalarInteger(0)
    }
}

// ---------------------------------------------------------------------------
// missing(x) — test whether a function argument was supplied
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_missing(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut sym = CAR(args);
        // Allow string argument: missing("x")
        if TYPEOF(sym) == SEXPTYPE::STRSXP.0 && LENGTH(sym) == 1 {
            let s = CStr::from_ptr(CHAR(STRING_ELT(sym, 0)));
            let name = s.to_str().unwrap_or("");
            let cs = CString::new(name).unwrap();
            sym = Rf_install(cs.as_ptr());
        }
        if TYPEOF(sym) != SEXPTYPE::SYMSXP.0 {
            std::panic::panic_any(RError {
                message: "invalid use of 'missing'".to_string(),
            });
        }
        let is_missing = crate::sexp::envir::R_isMissing(sym, env);
        Rf_ScalarLogical(if is_missing != 0 { TRUE } else { FALSE })
    }
}

// ---------------------------------------------------------------------------
// Recall — recursive call to the current function
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_Recall(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Cannot implement without full evaluation context
        eprintln!("Warning: Recall not fully implemented");
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// on.exit(expr, add) — register expression to evaluate on exit
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_on_exit(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Simplified: just store the expression (no real cleanup mechanism)
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// forceAndCall(n, FUN, ...) — force n arguments then call FUN
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_forceAndCall(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        eprintln!("Warning: forceAndCall not fully implemented");
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// declare(name, expr) — declare a variable (no-op in this port)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_declare(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// quote(expr) — return its argument unevaluated
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_quote(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

// ---------------------------------------------------------------------------
// substitute(expr, env) — substitute variables in an expression
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_substitute(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Simplified: just return the expression as-is
        CAR(args)
    }
}

// ---------------------------------------------------------------------------
// call(name, ...) — construct a function call
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_call_fn(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let name = CAR(args);
        let dots = CDR(args);
        // Build a LANGSXP: (name . dots)
        let result = Rf_protect(Rf_cons(name, dots));
        Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// switch(EXPR, ...) — switch statement
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_switch(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Simplified: evaluate first arg, try to match against named alternatives
        let expr = CAR(args);
        let alts = CDR(args);

        if alts.is_null() || alts == R_NilValue() {
            return R_NilValue();
        }

        // Evaluate the expression
        let val = crate::eval::eval::Rf_eval(expr, env);

        // Try to match against alternatives
        let mut a = alts;
        while !a.is_null() && a != R_NilValue() {
            let alt = CAR(a);
            if TYPEOF(alt) == SEXPTYPE::LANGSXP.0 {
                // Named element: check if name matches val
                let name = CAR(alt);
                if TYPEOF(name) == SEXPTYPE::SYMSXP.0 {
                    let pname = PRINTNAME(name);
                    let pstr = CStr::from_ptr(CHAR(pname)).to_str().unwrap_or("");

                    let val_str = match TYPEOF(val) {
                        t if t == SEXPTYPE::STRSXP.0 => {
                            if LENGTH(val) > 0 {
                                CStr::from_ptr(CHAR(STRING_ELT(val, 0)))
                                    .to_str()
                                    .unwrap_or("")
                                    .to_string()
                            } else {
                                String::new()
                            }
                        }
                        t if t == SEXPTYPE::REALSXP.0 => {
                            format!("{}", *REAL(val))
                        }
                        t if t == SEXPTYPE::INTSXP.0 => {
                            format!("{}", *INTEGER(val))
                        }
                        t if t == SEXPTYPE::LGLSXP.0 => {
                            let v = *LOGICAL(val);
                            if v == TRUE {
                                "TRUE".to_string()
                            } else if v == FALSE {
                                "FALSE".to_string()
                            } else {
                                String::new()
                            }
                        }
                        _ => String::new(),
                    };

                    if pstr == val_str {
                        // Found match, evaluate the body
                        let body = CDR(alt);
                        if !body.is_null() && body != R_NilValue() {
                            let mut result = R_NilValue();
                            let mut b = body;
                            while !b.is_null() && b != R_NilValue() {
                                result = crate::eval::eval::Rf_eval(CAR(b), env);
                                b = CDR(b);
                            }
                            return result;
                        }
                    }
                }
            } else if TYPEOF(alt) == SEXPTYPE::REALSXP.0
                || TYPEOF(alt) == SEXPTYPE::INTSXP.0
                || TYPEOF(alt) == SEXPTYPE::LGLSXP.0
                || TYPEOF(alt) == SEXPTYPE::STRSXP.0
            {
                // Default (unnamed) case — if this is the last element
                let next = CDR(a);
                if next.is_null() || next == R_NilValue() {
                    // This is the default
                    if TYPEOF(alt) == SEXPTYPE::LANGSXP.0 {
                        let body = CDR(alt);
                        if !body.is_null() && body != R_NilValue() {
                            return crate::eval::eval::Rf_eval(CAR(body), env);
                        }
                    }
                    return crate::eval::eval::Rf_eval(alt, env);
                }
            }
            a = CDR(a);
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// browser — interactive debugger (no-op)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_browser(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Non-interactive: no-op
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// .primTrace(name) / .primUntrace(name) — trace/untrace primitives
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_primTrace(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_primUntrace(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// undebug(fun) / isdebugged(fun) / debugonce(fun)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_undebug(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _fun = CAR(args);
        Rf_ScalarLogical(FALSE)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_isdebugged(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _fun = CAR(args);
        Rf_ScalarLogical(FALSE)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_debugonce(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _fun = CAR(args);
        Rf_ScalarLogical(FALSE)
    }
}

// ---------------------------------------------------------------------------
// delayedAssign(x, value, env, assign.env)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_delayedAssign(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// makeLazy(name, value, env) — create a lazy binding
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_makeLazy(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// ...elt(i) — extract the i-th element from ...
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_dot_elt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let idx = CAR(args);
        let i = if TYPEOF(idx) == SEXPTYPE::INTSXP.0 {
            *INTEGER(idx)
        } else if TYPEOF(idx) == SEXPTYPE::REALSXP.0 {
            *REAL(idx) as c_int
        } else {
            NA_INTEGER
        };
        if i == NA_INTEGER || i < 1 {
            return R_NilValue();
        }
        // Look up the ... symbol in the environment
        let dots_sym = Rf_install(b"...\0".as_ptr() as *const c_char);
        let dots_val = crate::sexp::envir::findVar(dots_sym, _env);
        if dots_val.is_null() || dots_val == R_NilValue() || TYPEOF(dots_val) != SEXPTYPE::DOTSXP.0
        {
            return R_NilValue();
        }
        // Walk the dots list
        let mut d = dots_val;
        let mut count = 1i32;
        while !d.is_null() && d != R_NilValue() {
            if count == i {
                let prom = CAR(d);
                if TYPEOF(prom) == SEXPTYPE::PROMSXP.0 {
                    return forcePromise(prom);
                }
                return prom;
            }
            d = CDR(d);
            count += 1;
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// ...length() — number of arguments in ...
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_dot_length(_call: SEXP, _op: SEXP, _args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let dots_sym = Rf_install(b"...\0".as_ptr() as *const c_char);
        let dots_val = crate::sexp::envir::findVar(dots_sym, env);
        if dots_val.is_null() || dots_val == R_NilValue() || TYPEOF(dots_val) != SEXPTYPE::DOTSXP.0
        {
            return Rf_ScalarInteger(0);
        }
        let mut count = 0i32;
        let mut d = dots_val;
        while !d.is_null() && d != R_NilValue() {
            count += 1;
            d = CDR(d);
        }
        Rf_ScalarInteger(count)
    }
}

// ---------------------------------------------------------------------------
// ...names() — names of ... arguments
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_dot_names(_call: SEXP, _op: SEXP, _args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let dots_sym = Rf_install(b"...\0".as_ptr() as *const c_char);
        let dots_val = crate::sexp::envir::findVar(dots_sym, env);
        if dots_val.is_null() || dots_val == R_NilValue() || TYPEOF(dots_val) != SEXPTYPE::DOTSXP.0
        {
            return Rf_allocVector(SEXPTYPE::STRSXP.0, 0);
        }
        // Count dots
        let mut count = 0i32;
        let mut d = dots_val;
        while !d.is_null() && d != R_NilValue() {
            count += 1;
            d = CDR(d);
        }
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, count));
        d = dots_val;
        let mut i = 0i32;
        while !d.is_null() && d != R_NilValue() {
            let tag = TAG(d);
            if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP.0 {
                SET_STRING_ELT(ans, i as R_xlen_t, PRINTNAME(tag));
            } else {
                SET_STRING_ELT(
                    ans,
                    i as R_xlen_t,
                    Rf_mkChar(b"\0".as_ptr() as *const c_char),
                );
            }
            d = CDR(d);
            i += 1;
        }
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// length(x) <- value — set the length of an object
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_length_assign(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CADR(args);
        let newlen = asInteger(value);
        if newlen == NA_INTEGER || newlen < 0 {
            return x;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP.0
            || t == SEXPTYPE::EXPRSXP.0
            || t == SEXPTYPE::STRSXP.0
            || t == SEXPTYPE::LGLSXP.0
            || t == SEXPTYPE::INTSXP.0
            || t == SEXPTYPE::REALSXP.0
            || t == SEXPTYPE::CPLXSXP.0
            || t == SEXPTYPE::RAWSXP.0
        {
            let oldlen = LENGTH(x) as c_int;
            if newlen == oldlen {
                return x;
            }
            let new_vec = Rf_protect(Rf_allocVector(t, newlen));
            let copy_len = if newlen < oldlen { newlen } else { oldlen };
            if copy_len > 0 {
                if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0 {
                    for i in 0..copy_len as usize {
                        SET_VECTOR_ELT(new_vec, i as R_xlen_t, VECTOR_ELT(x, i as R_xlen_t));
                    }
                } else if t == SEXPTYPE::STRSXP.0 {
                    for i in 0..copy_len as usize {
                        SET_STRING_ELT(new_vec, i as R_xlen_t, STRING_ELT(x, i as R_xlen_t));
                    }
                } else if t == SEXPTYPE::REALSXP.0 {
                    let src = REAL(x);
                    let dst = REAL(new_vec);
                    for i in 0..copy_len as usize {
                        *dst.add(i) = *src.add(i);
                    }
                } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
                    let src = INTEGER(x);
                    let dst = INTEGER(new_vec);
                    for i in 0..copy_len as usize {
                        *dst.add(i) = *src.add(i);
                    }
                } else if t == SEXPTYPE::RAWSXP.0 {
                    let src = RAW(x);
                    let dst = RAW(new_vec);
                    for i in 0..copy_len as usize {
                        *dst.add(i) = *src.add(i);
                    }
                }
            }
            // Copy attributes (except dim, dimnames which would be invalid)
            let attr = ATTRIB(x);
            if !attr.is_null() && attr != R_NilValue() {
                let dim = crate::attrib_core::getAttrib(x, crate::attrib_core::R_DimSymbol());
                let dimnames =
                    crate::attrib_core::getAttrib(x, crate::attrib_core::R_DimNamesSymbol());
                if dim.is_null() || dim == R_NilValue() {
                    crate::attrib_core::setAttrib(
                        new_vec,
                        crate::attrib_core::R_NamesSymbol(),
                        crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol()),
                    );
                }
            }
            Rf_unprotect(1);
            return new_vec;
        }
        x
    }
}

// ---------------------------------------------------------------------------
// .cache_class(x, class) / .class2(x)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_cache_class(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(_args);
        // Just return x unchanged
        x
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_class2(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let cl = crate::attrib_core::getAttrib(x, crate::attrib_core::R_ClassSymbol());
        if cl.is_null() || cl == R_NilValue() || LENGTH(cl) == 0 {
            return R_NilValue();
        }
        cl
    }
}

// ---------------------------------------------------------------------------
// @<-  (slot assignment)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_at_assign(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // S4 slot assignment — simplified stub
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// vector(mode, length) — create a vector
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_vector(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let mode_arg = CAR(args);
        let length_arg = CADR(args);

        let mode = if TYPEOF(mode_arg) == SEXPTYPE::STRSXP.0 {
            let s = CStr::from_ptr(CHAR(STRING_ELT(mode_arg, 0)))
                .to_str()
                .unwrap_or("");
            match s {
                "logical" => SEXPTYPE::LGLSXP.0,
                "integer" => SEXPTYPE::INTSXP.0,
                "double" | "numeric" => SEXPTYPE::REALSXP.0,
                "complex" => SEXPTYPE::CPLXSXP.0,
                "character" | "string" => SEXPTYPE::STRSXP.0,
                "list" => SEXPTYPE::VECSXP.0,
                "raw" => SEXPTYPE::RAWSXP.0,
                "expression" => SEXPTYPE::EXPRSXP.0,
                _ => SEXPTYPE::LGLSXP.0,
            }
        } else {
            asInteger(mode_arg)
        };

        let len = asInteger(length_arg);
        if len == NA_INTEGER || len < 0 {
            return Rf_allocVector(mode, 0);
        }

        let ans = Rf_protect(Rf_allocVector(mode, len));
        // Initialize logical vector to NA
        if mode == SEXPTYPE::LGLSXP.0 {
            let p = LOGICAL(ans);
            for i in 0..len as usize {
                *p.add(i) = NA_LOGICAL;
            }
        } else if mode == SEXPTYPE::REALSXP.0 {
            let p = REAL(ans);
            for i in 0..len as usize {
                *p.add(i) = NA_REAL;
            }
        } else if mode == SEXPTYPE::INTSXP.0 {
            let p = INTEGER(ans);
            for i in 0..len as usize {
                *p.add(i) = NA_INTEGER;
            }
        } else if mode == SEXPTYPE::CPLXSXP.0 {
            let p = COMPLEX(ans);
            for i in 0..len as usize {
                (*p.add(i)).r = NA_REAL;
                (*p.add(i)).i = NA_REAL;
            }
        }
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// get0(x, envir, mode, inherits) — get with default NULL
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_get0(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let sym_arg = CAR(args);
        let _envir = CADR(args);

        if TYPEOF(sym_arg) == SEXPTYPE::SYMSXP.0 {
            let val = crate::sexp::envir::findVar(sym_arg, env);
            if val == R_UnboundValue() {
                return R_NilValue();
            }
            if TYPEOF(val) == SEXPTYPE::PROMSXP.0 {
                return forcePromise(val);
            }
            return val;
        }

        if TYPEOF(sym_arg) == SEXPTYPE::STRSXP.0 {
            let s = CStr::from_ptr(CHAR(STRING_ELT(sym_arg, 0)))
                .to_str()
                .unwrap_or("");
            let cs = CString::new(s).unwrap();
            let sym = Rf_install(cs.as_ptr());
            let val = crate::sexp::envir::findVar(sym, env);
            if val == R_UnboundValue() {
                return R_NilValue();
            }
            if TYPEOF(val) == SEXPTYPE::PROMSXP.0 {
                return forcePromise(val);
            }
            return val;
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// mget(x, envir, mode, ifnotfound)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_mget(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _envir = CADR(args);

        if TYPEOF(x) != SEXPTYPE::STRSXP.0 {
            return Rf_allocVector(SEXPTYPE::VECSXP.0, 0);
        }

        let n = LENGTH(x);
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, n));

        for i in 0..n as R_xlen_t {
            let name_cstr = CStr::from_ptr(CHAR(STRING_ELT(x, i)));
            let name = name_cstr.to_str().unwrap_or("");
            let cs = CString::new(name).unwrap();
            let sym = Rf_install(cs.as_ptr());
            let val = crate::sexp::envir::findVar(sym, env);
            if val == R_UnboundValue() {
                SET_VECTOR_ELT(ans, i, R_NilValue());
            } else if TYPEOF(val) == SEXPTYPE::PROMSXP.0 {
                SET_VECTOR_ELT(ans, i, forcePromise(val));
            } else {
                SET_VECTOR_ELT(ans, i, val);
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// list2env(x, envir) — convert a list to an environment
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_list2env(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let envir_arg = CADR(args);

        if TYPEOF(x) != SEXPTYPE::VECSXP.0 {
            return R_NilValue();
        }

        let envir = if !envir_arg.is_null()
            && envir_arg != R_NilValue()
            && TYPEOF(envir_arg) == SEXPTYPE::ENVSXP.0
        {
            envir_arg
        } else {
            crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_NilValue(), R_GlobalEnv())
        };

        let n = LENGTH(x);
        let names = crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol());

        for i in 0..n {
            let val = VECTOR_ELT(x, i as R_xlen_t);
            if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP.0 {
                let name_str = CStr::from_ptr(CHAR(STRING_ELT(names, i as R_xlen_t)));
                let name = name_str.to_str().unwrap_or("");
                if !name.is_empty() {
                    let cs = CString::new(name).unwrap();
                    let sym = Rf_install(cs.as_ptr());
                    crate::sexp::envir::defineVar(sym, val, envir);
                }
            }
        }

        envir
    }
}

// ---------------------------------------------------------------------------
// new.env(hash, parent, size) — create a new environment
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_new_env(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _hash = CAR(args);
        let parent = CADR(args);
        let _size = CADDR(args);

        let parent_env = if !parent.is_null()
            && parent != R_NilValue()
            && TYPEOF(parent) == SEXPTYPE::ENVSXP.0
        {
            parent
        } else {
            R_GlobalEnv()
        };

        crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_NilValue(), parent_env)
    }
}

// ---------------------------------------------------------------------------
// environment<- (fun, value) — set function environment
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_environment_assign(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        let fun = CAR(args);
        let value = CADR(args);

        if TYPEOF(fun) == SEXPTYPE::CLOSXP.0 {
            SET_CLOENV(fun, value);
        }

        fun
    }
}

// ---------------------------------------------------------------------------
// pos.to.env(pos) — convert position to environment
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_pos_to_env(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let pos = CAR(args);
        let pos_val = asInteger(pos);

        // Walk the environment chain
        let mut env = R_GlobalEnv();
        for _ in 0..pos_val.saturating_sub(1) {
            let parent = ENCLOS(env);
            if parent.is_null() || parent == R_NilValue() || parent == R_EmptyEnv() {
                break;
            }
            env = parent;
        }

        env
    }
}

// ---------------------------------------------------------------------------
// storage.mode<- (x, value) — change storage mode
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_storage_mode_assign(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mode = CADR(args);

        let target_type = if TYPEOF(mode) == SEXPTYPE::STRSXP.0 {
            let s = CStr::from_ptr(CHAR(STRING_ELT(mode, 0)))
                .to_str()
                .unwrap_or("");
            match s {
                "logical" => SEXPTYPE::LGLSXP.0,
                "integer" => SEXPTYPE::INTSXP.0,
                "double" => SEXPTYPE::REALSXP.0,
                "complex" => SEXPTYPE::CPLXSXP.0,
                "character" => SEXPTYPE::STRSXP.0,
                "raw" => SEXPTYPE::RAWSXP.0,
                _ => return x,
            }
        } else {
            return x;
        };

        if TYPEOF(x) == target_type {
            return x;
        }

        coerceVector(x, target_type)
    }
}

// ---------------------------------------------------------------------------
// Version() — return R version info
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_Version(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let n = 7i32;
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n));
        let cn = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n));

        let fields = [
            ("platform", "unix"),
            ("arch", "x86_64"),
            ("os", "darwin"),
            ("system", "x86_64, darwin"),
            ("status", ""),
            ("major", "4"),
            ("minor", "4.0"),
        ];

        for (i, (name, val)) in fields.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*val).unwrap().as_ptr()),
            );
            SET_STRING_ELT(
                cn,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap().as_ptr()),
            );
        }

        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), cn);
        Rf_unprotect(2);
        ans
    }
}

// ---------------------------------------------------------------------------
// internalsID() — return R internals version
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_internalsID(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { Rf_mkString(CString::new("R 4.4.0").unwrap().as_ptr()) }
}

// ---------------------------------------------------------------------------
// memory.profile() — return memory usage by type
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_memory_profile(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        // Return integer vector with approximate counts by type
        let ntypes = 25i32;
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, ntypes));
        let p = INTEGER(ans);
        for i in 0..ntypes as usize {
            *p.add(i) = 0;
        }
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// builtins() — return names of built-in functions
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_builtins(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _internal = CAR(args);
        // Return a character vector of builtin names
        let entries = crate::main::names::R_FunTab;
        let mut names: Vec<&[u8]> = Vec::new();
        for entry in entries {
            let name_bytes = entry.name;
            if name_bytes == b"\0" || name_bytes.len() <= 1 {
                break;
            }
            if entry.cfun.is_some() {
                // Find the null terminator
                let end = name_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(name_bytes.len());
                names.push(&name_bytes[..end]);
            }
        }
        names.sort_by(|a, b| a.cmp(b));

        let n = names.len() as c_int;
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n));
        for (i, name) in names.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as R_xlen_t,
                Rf_mkChar(name.as_ptr() as *const c_char),
            );
        }
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// vhash(x) — hash a vector (internal use)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_vhash(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Simplified: return 0
        Rf_ScalarInteger(0)
    }
}

// ---------------------------------------------------------------------------
// setFileTime(path, time) — set file modification time
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_setFileTime(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let path_arg = CAR(args);
        let _time_arg = CADR(args);

        if TYPEOF(path_arg) != SEXPTYPE::STRSXP.0 || LENGTH(path_arg) < 1 {
            return Rf_ScalarLogical(FALSE);
        }

        let path_cstr = CStr::from_ptr(CHAR(STRING_ELT(path_arg, 0)));
        let path = path_cstr.to_str().unwrap_or("");

        // Use touch (set mtime to now)
        match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(path)
        {
            Ok(_f) => Rf_ScalarLogical(TRUE),
            Err(_) => Rf_ScalarLogical(FALSE),
        }
    }
}

// ---------------------------------------------------------------------------
// sample2 — internal sampling (no replacement)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_sample2(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// cat(..., file, sep, fill, labels, append) — output to file/connection
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_cat(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let a = args;
        let mut sep = " ";

        // Navigate args: ... file sep fill labels append
        // Use a simple approach: walk the list
        let mut args_list = a;
        let mut _file_arg = R_NilValue();
        let mut sep_arg = R_NilValue();

        // cat args: ..., file, sep, fill, labels, append
        // We only care about the first arg (...) and sep
        let items = CAR(args_list);
        args_list = CDR(args_list); // file
        _file_arg = CAR(args_list);
        args_list = CDR(args_list); // sep
        sep_arg = CAR(args_list);

        if !sep_arg.is_null() && sep_arg != R_NilValue() && TYPEOF(sep_arg) == SEXPTYPE::STRSXP.0 {
            let s = CStr::from_ptr(CHAR(STRING_ELT(sep_arg, 0)));
            sep = s.to_str().unwrap_or(" ");
        }

        let mut first = true;
        let nitems = 5; // first 5 args are the items to cat

        // Process items (first 5 args before file/sep/fill/labels/append)
        let items = CAR(a);
        if TYPEOF(items) == SEXPTYPE::STRSXP.0 {
            let n = LENGTH(items);
            for i in 0..n {
                if !first && !sep.is_empty() {
                    print!("{}", sep);
                }
                let s = CStr::from_ptr(CHAR(STRING_ELT(items, i as R_xlen_t)));
                print!("{}", s.to_str().unwrap_or(""));
                first = false;
            }
        } else if TYPEOF(items) == SEXPTYPE::VECSXP.0 {
            let n = LENGTH(items);
            for i in 0..n {
                if !first && !sep.is_empty() {
                    print!("{}", sep);
                }
                let elt = VECTOR_ELT(items, i as R_xlen_t);
                let s = CStr::from_ptr(CHAR(elt));
                print!("{}", s.to_str().unwrap_or(""));
                first = false;
            }
        } else {
            // Single non-string item: format it
            let s = crate::main::printutils::EncodeElement(items, 0, 0, 0);
            let cs = CStr::from_ptr(s);
            print!("{}", cs.to_str().unwrap_or(""));
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do.call(fun, args, envir) — call a function with a list of arguments
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_do_call(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let fun = CAR(args);
        let mut call_args = CADR(args);
        let call_env = if CDR(CDR(args)) != R_NilValue() {
            CADDR(args)
        } else {
            env
        };

        // Build a call: (fun arg1 arg2 ...)
        let call = Rf_protect(Rf_cons(fun, R_NilValue()));
        let mut tail = call;
        if TYPEOF(call_args) == SEXPTYPE::VECSXP.0 || TYPEOF(call_args) == SEXPTYPE::LISTSXP.0 {
            let n = LENGTH(call_args);
            let arg_names =
                crate::attrib_core::getAttrib(call_args, crate::attrib_core::R_NamesSymbol());
            for i in 0..n {
                let val;
                if TYPEOF(call_args) == SEXPTYPE::VECSXP.0 {
                    val = VECTOR_ELT(call_args, i as R_xlen_t);
                } else {
                    val = CAR(call_args);
                    call_args = CDR(call_args);
                }
                let new_cons = Rf_protect(Rf_cons(val, R_NilValue()));
                // Set tag if named
                if !arg_names.is_null() && arg_names != R_NilValue() {
                    let name_str = CStr::from_ptr(CHAR(STRING_ELT(arg_names, i as R_xlen_t)));
                    let name = name_str.to_str().unwrap_or("");
                    if !name.is_empty() {
                        let cs = CString::new(name).unwrap();
                        SETTAG(new_cons, Rf_install(cs.as_ptr()));
                    }
                }
                SETCDR(tail, new_cons);
                tail = new_cons;
                Rf_unprotect(1);
            }
        }

        Rf_unprotect(1);
        crate::eval::eval::Rf_eval(call, call_env)
    }
}

// ---------------------------------------------------------------------------
// str2lang(text) — convert a string to a language object
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_str2lang(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let text = CAR(args);
        if TYPEOF(text) != SEXPTYPE::STRSXP.0 || LENGTH(text) < 1 {
            return R_NilValue();
        }
        // Simplified: return the string as a symbol
        let s = CStr::from_ptr(CHAR(STRING_ELT(text, 0)));
        let name = s.to_str().unwrap_or("");
        Rf_install(CString::new(name).unwrap().as_ptr()) as SEXP
    }
}

// ---------------------------------------------------------------------------
// str2expression(text) — convert strings to expression
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_str2expression(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let text = CAR(args);
        if TYPEOF(text) != SEXPTYPE::STRSXP.0 {
            return R_NilValue();
        }
        let n = LENGTH(text);
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::EXPRSXP.0, n));
        for i in 0..n {
            let s = CStr::from_ptr(CHAR(STRING_ELT(text, i as R_xlen_t)));
            let name = s.to_str().unwrap_or("");
            SET_VECTOR_ELT(
                ans,
                i as R_xlen_t,
                Rf_install(CString::new(name).unwrap().as_ptr()) as SEXP,
            );
        }
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// substr<- (x, start, stop) <- value
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_substr_assign(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let start = CADR(args);
        let stop = CADDR(args);
        let value = CAR(CDR(CDR(CDR(args))));

        if TYPEOF(x) != SEXPTYPE::STRSXP.0 {
            return x;
        }

        let n = LENGTH(x);
        let y = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n));
        let na = crate::main::relop::NA_STRING();

        let k = if TYPEOF(start) == SEXPTYPE::INTSXP.0 {
            LENGTH(start)
        } else {
            0
        };
        let l = if TYPEOF(stop) == SEXPTYPE::INTSXP.0 {
            LENGTH(stop)
        } else {
            0
        };

        let starts = if k > 0 { INTEGER(start) } else { ptr::null() };
        let stops = if l > 0 { INTEGER(stop) } else { ptr::null() };

        for i in 0..n {
            let el = STRING_ELT(x, i as R_xlen_t);
            if el.is_null() || el == na {
                SET_STRING_ELT(y, i as R_xlen_t, el);
                continue;
            }

            let xi = CHAR(el);
            let xi_bytes = CStr::from_ptr(xi).to_bytes();
            let slen = xi_bytes.len();

            let si = if !starts.is_null() {
                *starts.add((i as c_int % k) as usize)
            } else {
                1
            };
            let ei = if !stops.is_null() {
                *stops.add((i as c_int % l) as usize)
            } else {
                slen as c_int
            };

            if si == NA_INTEGER || ei == NA_INTEGER {
                SET_STRING_ELT(y, i as R_xlen_t, na);
                continue;
            }

            // Get replacement value
            let rep_val = STRING_ELT(value, i as R_xlen_t);
            if rep_val.is_null() || rep_val == na {
                SET_STRING_ELT(y, i as R_xlen_t, na);
                continue;
            }

            let rep_bytes = CStr::from_ptr(CHAR(rep_val)).to_bytes();

            let mut s = si;
            let e = ei;
            if s < 1 {
                s = 1;
            }
            if s > e {
                s = e;
            }

            let mut result: Vec<u8> = Vec::new();
            // Before replacement
            if s > 1 && (s - 1) as usize <= slen {
                result.extend_from_slice(&xi_bytes[..(s - 1) as usize]);
            }
            // Replacement
            result.extend_from_slice(rep_bytes);
            // After replacement
            if e as usize <= slen {
                result.extend_from_slice(&xi_bytes[e as usize..]);
            }

            let result_len = result.len();
            let cs = CString::new(result).unwrap();
            let ch = Rf_mkCharLen(cs.as_ptr(), result_len as c_int);
            SET_STRING_ELT(y, i as R_xlen_t, ch);
        }

        Rf_unprotect(1);
        y
    }
}

// ---------------------------------------------------------------------------
// strsplit(x, split, fixed, perl, useBytes) — split strings
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_strsplit(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let split_arg = CADR(args);
        let _fixed = CADDR(args);

        if TYPEOF(x) != SEXPTYPE::STRSXP.0 || TYPEOF(split_arg) != SEXPTYPE::STRSXP.0 {
            return Rf_allocVector(SEXPTYPE::VECSXP.0, 0);
        }

        let split_str = CStr::from_ptr(CHAR(STRING_ELT(split_arg, 0)))
            .to_str()
            .unwrap_or("");
        let n = LENGTH(x);
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, n));

        for i in 0..n {
            let el = STRING_ELT(x, i as R_xlen_t);
            let na = crate::main::relop::NA_STRING();
            if el.is_null() || el == na {
                let empty = Rf_allocVector(SEXPTYPE::STRSXP.0, 0);
                SET_VECTOR_ELT(ans, i as R_xlen_t, empty);
                continue;
            }

            let s = CStr::from_ptr(CHAR(el)).to_str().unwrap_or("");

            // Simple split on exact match
            let parts: Vec<String> = if split_str.is_empty() {
                // Split into individual characters
                s.chars()
                    .map(|c| {
                        let mut buf = [0u8; 4];
                        c.encode_utf8(&mut buf);
                        String::from_utf8_lossy(&buf[..c.len_utf8()]).into_owned()
                    })
                    .collect()
            } else {
                s.split(split_str).map(|s| s.to_string()).collect()
            };

            let vec = Rf_allocVector(SEXPTYPE::STRSXP.0, parts.len() as c_int);
            for (j, part) in parts.iter().enumerate() {
                SET_STRING_ELT(
                    vec,
                    j as R_xlen_t,
                    Rf_mkChar(CString::new(part.as_str()).unwrap().as_ptr()),
                );
            }
            SET_VECTOR_ELT(ans, i as R_xlen_t, vec);
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// agrepl(pattern, x, max.distance, ...) — approximate grep (logical)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_agrepl(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Simplified: fall back to exact matching (no fuzzy)
        let pat = CAR(args);
        let text = CADR(args);

        if TYPEOF(pat) != SEXPTYPE::STRSXP.0 || TYPEOF(text) != SEXPTYPE::STRSXP.0 {
            return Rf_allocVector(SEXPTYPE::LGLSXP.0, 0);
        }

        let pat_str = CStr::from_ptr(CHAR(STRING_ELT(pat, 0)))
            .to_str()
            .unwrap_or("");
        let n = LENGTH(text);
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::LGLSXP.0, n));
        let p = LOGICAL(ans);

        for i in 0..n {
            let el = STRING_ELT(text, i as R_xlen_t);
            let na = crate::main::relop::NA_STRING();
            if el.is_null() || el == na {
                *p.add(i as usize) = NA_LOGICAL;
            } else {
                let s = CStr::from_ptr(CHAR(el)).to_str().unwrap_or("");
                *p.add(i as usize) = if s.contains(pat_str) { TRUE } else { FALSE };
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// xtfrm(x) — transform for sorting
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_xtfrm(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let t = TYPEOF(x);

        match t {
            tt if tt == SEXPTYPE::REALSXP.0 => {
                // Copy with NA handling
                let n = LENGTH(x);
                let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n));
                let src = REAL(x);
                let dst = REAL(ans);
                for i in 0..n as usize {
                    let v = *src.add(i);
                    if ISNAN(v) {
                        *dst.add(i) = NA_REAL;
                    } else {
                        *dst.add(i) = v;
                    }
                }
                Rf_unprotect(1);
                ans
            }
            tt if tt == SEXPTYPE::INTSXP.0 || tt == SEXPTYPE::LGLSXP.0 => {
                let n = LENGTH(x);
                let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n));
                let src = INTEGER(x);
                let dst = REAL(ans);
                for i in 0..n as usize {
                    let v = *src.add(i);
                    if v == NA_INTEGER {
                        *dst.add(i) = NA_REAL;
                    } else {
                        *dst.add(i) = v as f64;
                    }
                }
                Rf_unprotect(1);
                ans
            }
            tt if tt == SEXPTYPE::STRSXP.0 => {
                // String xtfrm: return the string itself (simplified)
                x
            }
            tt if tt == SEXPTYPE::CPLXSXP.0 => {
                // Complex: sort by real part, then imaginary
                let n = LENGTH(x);
                let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n));
                let src = COMPLEX(x);
                let dst = REAL(ans);
                for i in 0..n as usize {
                    let c = *src.add(i);
                    if ISNAN(c.r) {
                        *dst.add(i) = NA_REAL;
                    } else {
                        *dst.add(i) = c.r;
                    }
                }
                Rf_unprotect(1);
                ans
            }
            _ => {
                let n = LENGTH(x);
                Rf_allocVector(SEXPTYPE::REALSXP.0, n)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// as.function.default(x) — coerce to function
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_as_function_default(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) == SEXPTYPE::CLOSXP.0
            || TYPEOF(x) == SEXPTYPE::BUILTINSXP.0
            || TYPEOF(x) == SEXPTYPE::SPECIALSXP.0
        {
            return x;
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP.0 {
            let s = CStr::from_ptr(CHAR(STRING_ELT(x, 0)))
                .to_str()
                .unwrap_or("");
            let cs = CString::new(s).unwrap();
            let fun = Rf_install(cs.as_ptr());
            let val = crate::sexp::envir::findFun(fun, _env);
            if !val.is_null() && val != R_UnboundValue() {
                return val;
            }
        }
        std::panic::panic_any(RError {
            message: "cannot coerce to a function".to_string(),
        });
    }
}

// ---------------------------------------------------------------------------
// :: and ::: — namespace access
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_double_colon(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _ns = CAR(args);
        let name = CADR(args);
        if TYPEOF(name) == SEXPTYPE::SYMSXP.0 {
            let val = crate::sexp::envir::findVar(name, R_GlobalEnv());
            if val != R_UnboundValue() {
                return val;
            }
        }
        R_NilValue()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_triple_colon(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _ns = CAR(args);
        let name = CADR(args);
        if TYPEOF(name) == SEXPTYPE::SYMSXP.0 {
            let val = crate::sexp::envir::findVar(name, R_GlobalEnv());
            if val != R_UnboundValue() {
                return val;
            }
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Foreign interfaces: .C, .Fortran, .Call, .External, .External2,
//                     .Call.graphics, .External.graphics
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_foreign_C(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        eprintln!("Error: .C() not available in this port");
        R_NilValue()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_foreign_Fortran(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        eprintln!("Error: .Fortran() not available in this port");
        R_NilValue()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_foreign_Call(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        eprintln!("Error: .Call() not available in this port");
        R_NilValue()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_foreign_External(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        eprintln!("Error: .External() not available in this port");
        R_NilValue()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_foreign_External2(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        eprintln!("Error: .External2() not available in this port");
        R_NilValue()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_foreign_Call_graphics(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        eprintln!("Error: .Call.graphics() not available in this port");
        R_NilValue()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_foreign_External_graphics(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        eprintln!("Error: .External.graphics() not available in this port");
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// eapply(env, FUN, ...) — apply FUN to each element of an environment
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_eapply(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let env_arg = CAR(args);
        let _fun = CADR(args);

        if TYPEOF(env_arg) != SEXPTYPE::ENVSXP.0 {
            return R_NilValue();
        }

        // Collect all names in the environment
        let mut names_vec: Vec<SEXP> = Vec::new();
        let sym = crate::sexp::symbol::Rf_install(b"names\0".as_ptr() as *const c_char);
        let names_val = crate::sexp::envir::findVar(sym, env_arg);
        if !names_val.is_null()
            && names_val != R_NilValue()
            && names_val != R_UnboundValue()
            && TYPEOF(names_val) == SEXPTYPE::STRSXP.0
        {
            let n = LENGTH(names_val);
            for i in 0..n {
                names_vec.push(STRING_ELT(names_val, i as R_xlen_t));
            }
        }

        let n = names_vec.len() as c_int;
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, n));
        let ans_names = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n));

        for (i, name_sexp) in names_vec.iter().enumerate() {
            let name_str = CStr::from_ptr(CHAR(*name_sexp));
            let name = name_str.to_str().unwrap_or("");
            let cs = CString::new(name).unwrap();
            let sym = Rf_install(cs.as_ptr());
            let val = crate::sexp::envir::findVar(sym, env_arg);
            if val != R_UnboundValue() && TYPEOF(val) != SEXPTYPE::PROMSXP.0 {
                SET_VECTOR_ELT(ans, i as R_xlen_t, val);
            } else if TYPEOF(val) == SEXPTYPE::PROMSXP.0 {
                SET_VECTOR_ELT(ans, i as R_xlen_t, forcePromise(val));
            }
            SET_STRING_ELT(ans_names, i as R_xlen_t, *name_sexp);
        }

        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), ans_names);
        Rf_unprotect(2);
        ans
    }
}

// ---------------------------------------------------------------------------
// quit(save, status, runLast) — exit R
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_quit(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    std::process::exit(0);
}

// ---------------------------------------------------------------------------
// readline(prompt) — read a line from the terminal
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_readline(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _prompt = CAR(args);
        // Non-interactive: return empty string
        Rf_mkString(b"\0".as_ptr() as *const c_char)
    }
}

// ---------------------------------------------------------------------------
// system(command, intern, ...) — execute a system command
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_system(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let cmd = CAR(args);
        let intern = CADR(args);

        if TYPEOF(cmd) != SEXPTYPE::STRSXP.0 || LENGTH(cmd) < 1 {
            return Rf_ScalarInteger(1); // error status
        }

        let command = CStr::from_ptr(CHAR(STRING_ELT(cmd, 0)))
            .to_str()
            .unwrap_or("");

        let do_intern = if !intern.is_null() && intern != R_NilValue() {
            let v = *LOGICAL(intern);
            v == TRUE
        } else {
            false
        };

        if do_intern {
            // Capture output
            match std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .output()
            {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let lines: Vec<&str> = stdout.trim_end().split('\n').collect();
                    let n = lines.len() as c_int;
                    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n));
                    for (i, line) in lines.iter().enumerate() {
                        SET_STRING_ELT(
                            ans,
                            i as R_xlen_t,
                            Rf_mkChar(CString::new(*line).unwrap().as_ptr()),
                        );
                    }
                    Rf_unprotect(1);
                    // Set names attribute
                    let status = output.status.code().unwrap_or(-1);
                    let status_sym = Rf_install(b"status\0".as_ptr() as *const c_char);
                    crate::attrib_core::setAttrib(ans, status_sym, Rf_ScalarInteger(status));
                    ans
                }
                Err(_) => Rf_allocVector(SEXPTYPE::STRSXP.0, 0),
            }
        } else {
            // Just execute
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .status();
            Rf_ScalarInteger(status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1))
        }
    }
}

// ---------------------------------------------------------------------------
// parse(text, n, srcfile, keep.source) — parse R code
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_parse_fn(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Simplified: return empty expression
        Rf_allocVector(SEXPTYPE::EXPRSXP.0, 0)
    }
}

// ---------------------------------------------------------------------------
// eval(expr, envir) — evaluate an expression
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_eval_fn(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let eval_env = if CDR(args) != R_NilValue() && CADR(args) != R_NilValue() {
            CADR(args)
        } else {
            env
        };

        crate::eval::eval::Rf_eval(expr, eval_env)
    }
}

// ---------------------------------------------------------------------------
// sort(x, decreasing, na.last, method) — sort a vector
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_sort(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let decreasing = CADR(args);
        let na_last = CADDR(args);

        let mut decr = false;
        if !decreasing.is_null() && decreasing != R_NilValue() {
            let v = asLogical(decreasing);
            decr = v == TRUE;
        }

        let t = TYPEOF(x);
        let n = LENGTH(x) as usize;

        if n == 0 {
            return Rf_allocVector(t, 0);
        }

        match t {
            tt if tt == SEXPTYPE::REALSXP.0 => {
                let mut vals: Vec<f64> = Vec::with_capacity(n);
                let src = REAL(x);
                for i in 0..n {
                    vals.push(*src.add(i));
                }
                if decr {
                    vals.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Less));
                } else {
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Greater));
                }
                let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int));
                let dst = REAL(ans);
                for (i, v) in vals.iter().enumerate() {
                    *dst.add(i) = *v;
                }
                Rf_unprotect(1);
                ans
            }
            tt if tt == SEXPTYPE::INTSXP.0 || tt == SEXPTYPE::LGLSXP.0 => {
                let mut vals: Vec<c_int> = Vec::with_capacity(n);
                let src = INTEGER(x);
                for i in 0..n {
                    vals.push(*src.add(i));
                }
                if decr {
                    vals.sort_by(|a, b| b.cmp(a));
                } else {
                    vals.sort();
                }
                let ans = Rf_protect(Rf_allocVector(t, n as c_int));
                let dst = INTEGER(ans);
                for (i, v) in vals.iter().enumerate() {
                    *dst.add(i) = *v;
                }
                Rf_unprotect(1);
                ans
            }
            tt if tt == SEXPTYPE::STRSXP.0 => {
                let mut vals: Vec<String> = Vec::with_capacity(n);
                for i in 0..n {
                    let s = CStr::from_ptr(CHAR(STRING_ELT(x, i as R_xlen_t)));
                    vals.push(s.to_str().unwrap_or("").to_string());
                }
                if decr {
                    vals.sort_by(|a, b| b.cmp(a));
                } else {
                    vals.sort();
                }
                let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n as c_int));
                for (i, v) in vals.iter().enumerate() {
                    SET_STRING_ELT(
                        ans,
                        i as R_xlen_t,
                        Rf_mkChar(CString::new(v.as_str()).unwrap().as_ptr()),
                    );
                }
                Rf_unprotect(1);
                ans
            }
            _ => x,
        }
    }
}

// ---------------------------------------------------------------------------
// order(..., na.last, decreasing) — return a permutation vector
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_order(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _decreasing = CADR(args);

        let t = TYPEOF(x);
        let n = LENGTH(x) as usize;

        if n == 0 {
            return Rf_allocVector(SEXPTYPE::INTSXP.0, 0);
        }

        // Create index vector [0, 1, 2, ..., n-1]
        let mut indices: Vec<usize> = (0..n).collect();

        match t {
            tt if tt == SEXPTYPE::REALSXP.0 => {
                let src = REAL(x);
                indices.sort_by(|&a, &b| {
                    let va = *src.add(a);
                    let vb = *src.add(b);
                    va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            tt if tt == SEXPTYPE::INTSXP.0 || tt == SEXPTYPE::LGLSXP.0 => {
                let src = INTEGER(x);
                indices.sort_by(|&a, &b| (*src.add(a)).cmp(&*src.add(b)));
            }
            tt if tt == SEXPTYPE::STRSXP.0 => {
                indices.sort_by(|&a, &b| {
                    let sa = CStr::from_ptr(CHAR(STRING_ELT(x, a as R_xlen_t)));
                    let sb = CStr::from_ptr(CHAR(STRING_ELT(x, b as R_xlen_t)));
                    sa.cmp(sb)
                });
            }
            _ => {}
        }

        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, n as c_int));
        let p = INTEGER(ans);
        for (i, &idx) in indices.iter().enumerate() {
            *p.add(i) = (idx + 1) as c_int; // 1-based
        }
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// inspect(x) — inspect an object
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_inspect(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        // Print basic info about the object
        eprintln!("Inspect: type={}, length={}", TYPEOF(x), LENGTH(x));
        x
    }
}

// ---------------------------------------------------------------------------
// match.call(call, fun, expand.dots) — match a call to a function's formals
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_match_call(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let call = CAR(args);
        // Simplified: just return the call as-is
        call
    }
}

// ---------------------------------------------------------------------------
// polyroot(z) — find zeros of a polynomial
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_polyroot_fn(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Delegate to the full polyroot implementation
        crate::main::polyroot::do_polyroot(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// compareNumericVersion(a, b) — compare R version strings
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_compareNumericVersion(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b = CADR(args);

        if TYPEOF(a) != SEXPTYPE::STRSXP.0 || TYPEOF(b) != SEXPTYPE::STRSXP.0 {
            return Rf_ScalarInteger(NA_INTEGER);
        }

        let sa = CStr::from_ptr(CHAR(STRING_ELT(a, 0)))
            .to_str()
            .unwrap_or("");
        let sb = CStr::from_ptr(CHAR(STRING_ELT(b, 0)))
            .to_str()
            .unwrap_or("");

        // Parse version strings like "4.4.0"
        let parse_version =
            |s: &str| -> Vec<i32> { s.split('.').filter_map(|p| p.parse::<i32>().ok()).collect() };

        let va = parse_version(sa);
        let vb = parse_version(sb);

        let max_len = va.len().max(vb.len());
        let mut a_ext = va.clone();
        let mut b_ext = vb.clone();
        while a_ext.len() < max_len {
            a_ext.push(0);
        }
        while b_ext.len() < max_len {
            b_ext.push(0);
        }

        let cmp = a_ext.cmp(&b_ext);
        Rf_ScalarInteger(match cmp {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        })
    }
}

// ---------------------------------------------------------------------------
// .addGlobHands(handlers) — add global handlers
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_addGlobHands(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// C_tryCatchHelper — tryCatch C helper
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_C_tryCatchHelper(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// getNamespaceValue(ns, name) — get value from namespace
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_getNamespaceValue(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        let _ns = CAR(args);
        let name = CADR(args);

        if TYPEOF(name) == SEXPTYPE::SYMSXP.0 {
            let val = crate::sexp::envir::findVar(name, R_GlobalEnv());
            if val != R_UnboundValue() {
                return val;
            }
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// $ — dollar access (implemented in subset.rs as do_subset3)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_dollar(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Delegate to subset module's dollar implementation
        crate::main::subset::do_subset3(_call, _op, args, env)
    }
}

// ---------------------------------------------------------------------------
// @ — slot access (S4, simplified)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_at(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let name = CADR(args);

        if TYPEOF(obj) != SEXPTYPE::S4SXP.0 {
            // S4SXP = 25
            // Fall back to attribute access
            let name_str = if TYPEOF(name) == SEXPTYPE::SYMSXP.0 {
                CStr::from_ptr(CHAR(PRINTNAME(name))).to_str().unwrap_or("")
            } else if TYPEOF(name) == SEXPTYPE::STRSXP.0 {
                CStr::from_ptr(CHAR(STRING_ELT(name, 0)))
                    .to_str()
                    .unwrap_or("")
            } else {
                ""
            };

            if name_str.is_empty() {
                return R_NilValue();
            }

            let sym = Rf_install(CString::new(name_str).unwrap().as_ptr());
            let val = crate::attrib_core::getAttrib(obj, sym);
            if !val.is_null() && val != R_NilValue() {
                return val;
            }

            // Fall back to $ access
            return crate::main::subset::do_subset3(_call, _op, args, _env);
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_colon2 — :: (double colon / namespace access)
// ---------------------------------------------------------------------------

/// Implement :: operator (namespace access).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_colon2(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let pkg_name = CAR(args);
        let sym_name = CADR(args);

        let pkg_str = if TYPEOF(pkg_name) == SEXPTYPE::SYMSXP.0 {
            let pname = PRINTNAME(pkg_name);
            CStr::from_ptr(CHAR(pname))
                .to_str()
                .unwrap_or("")
                .to_string()
        } else if TYPEOF(pkg_name) == SEXPTYPE::STRSXP.0 && LENGTH(pkg_name) > 0 {
            CStr::from_ptr(CHAR(STRING_ELT(pkg_name, 0)))
                .to_str()
                .unwrap_or("")
                .to_string()
        } else {
            return R_NilValue();
        };

        // Look up in base namespace — simplified implementation
        let sym = if TYPEOF(sym_name) == SEXPTYPE::SYMSXP.0 {
            sym_name
        } else {
            return R_NilValue();
        };

        // Try to find in the global environment chain
        let val = crate::sexp::envir::R_findVar(sym, env);
        if val != crate::sexp::globals::R_UnboundValue() {
            return val;
        }

        // Not found — return unbound
        crate::sexp::globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_colon3 — ::: (triple colon / internal namespace access)
// ---------------------------------------------------------------------------

/// Implement ::: operator (internal namespace access).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_colon3(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Same as :: for now — full implementation needs namespace registry
        do_colon2(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// do_dotsElt — ...elt(n)
// ---------------------------------------------------------------------------

/// Implement ...elt(n) — access the nth element of ... (varargs).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_dotsElt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let which = CAR(args);
        let n = crate::main::coerce::asInteger(which);
        if n == NA_INTEGER || n < 1 {
            return R_MissingArg();
        }

        // Find ... in the current environment
        let dots_sym = crate::sexp::symbol::R_DotsSymbol();
        let dots_val = crate::sexp::envir::R_findVar(dots_sym, env);
        if dots_val == crate::sexp::globals::R_UnboundValue() || dots_val.is_null() {
            return R_NilValue();
        }

        // Walk the pairlist to the nth element
        let mut current = dots_val;
        let mut i: c_int = 1;
        while i < n && !current.is_null() && current != R_NilValue() {
            current = CDR(current);
            i += 1;
        }

        if current.is_null() || current == R_NilValue() {
            return R_NilValue();
        }

        // Force the promise
        if TYPEOF(CAR(current)) == SEXPTYPE::PROMSXP.0 {
            crate::sexp::envir::forcePromise(CAR(current))
        } else {
            CAR(current)
        }
    }
}

// ---------------------------------------------------------------------------
// do_dotsLength — ...length()
// ---------------------------------------------------------------------------

/// Implement ...length() — return the number of ... arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_dotsLength(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let dots_sym = crate::sexp::symbol::R_DotsSymbol();
        let dots_val = crate::sexp::envir::R_findVar(dots_sym, env);
        if dots_val == crate::sexp::globals::R_UnboundValue() || dots_val.is_null() {
            return Rf_ScalarInteger(0);
        }

        // Count pairlist length
        let mut n: c_int = 0;
        let mut current = dots_val;
        while !current.is_null() && current != R_NilValue() {
            n += 1;
            current = CDR(current);
        }

        Rf_ScalarInteger(n)
    }
}

// ---------------------------------------------------------------------------
// do_dotsNames — ...names()
// ---------------------------------------------------------------------------

/// Implement ...names() — return the names of ... arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_dotsNames(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let dots_sym = crate::sexp::symbol::R_DotsSymbol();
        let dots_val = crate::sexp::envir::R_findVar(dots_sym, env);
        if dots_val == crate::sexp::globals::R_UnboundValue() || dots_val.is_null() {
            return Rf_allocVector3(SEXPTYPE::STRSXP.0, 0);
        }

        // Count pairlist length
        let mut n: c_int = 0;
        let mut current = dots_val;
        while !current.is_null() && current != R_NilValue() {
            n += 1;
            current = CDR(current);
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t);
        if result.is_null() || n == 0 {
            return result;
        }

        current = dots_val;
        let mut i: R_xlen_t = 0;
        while !current.is_null() && current != R_NilValue() {
            let tag = TAG(current);
            if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP.0 {
                let pname = PRINTNAME(tag);
                let name_ch = CHAR(pname);
                if !name_ch.is_null() {
                    let name_cstr = CStr::from_ptr(name_ch);
                    let name_str = name_cstr.to_str().unwrap_or("");
                    let name_rstr = crate::sexp::constructors::Rf_mkChar(
                        CString::new(name_str).unwrap().as_ptr(),
                    );
                    SET_STRING_ELT(result, i, name_rstr);
                }
            }
            i += 1;
            current = CDR(current);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// do_docall — do.call(func, args)
// ---------------------------------------------------------------------------

/// Implement do.call(func, args, quote = FALSE).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_docall(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let fun = CAR(args);
        let call_args = CADR(args);
        let quote = if CDDR(args) != R_NilValue() {
            let q = crate::main::coerce::asLogical(CADDR(args));
            q != 0 && q != NA_INTEGER
        } else {
            false
        };

        // Evaluate the function
        let fun_val = crate::eval::eval::Rf_eval(fun, env);
        Rf_protect(fun_val);

        if TYPEOF(fun_val) == SEXPTYPE::LANGSXP.0 || TYPEOF(fun_val) == SEXPTYPE::SYMSXP.0 {
            // Build the call: fun(arg1, arg2, ...)
            let call_list: SEXP = R_NilValue();

            // Build args in reverse order
            let mut current = call_args;
            let mut tmp: SEXP = R_NilValue();
            while !current.is_null() && current != R_NilValue() {
                let arg_val = if quote {
                    CAR(current) // don't evaluate
                } else {
                    crate::eval::eval::Rf_eval(CAR(current), env)
                };
                tmp = Rf_cons(arg_val, tmp);
                current = CDR(current);
            }

            // Reverse the list
            let mut args_list: SEXP = R_NilValue();
            current = tmp;
            while !current.is_null() && current != R_NilValue() {
                args_list = Rf_cons(CAR(current), args_list);
                current = CDR(current);
            }

            // Build the call
            let new_call = Rf_cons(fun_val, args_list);
            (*new_call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            Rf_protect(new_call);

            let result = crate::eval::eval::Rf_eval(new_call, env);
            crate::sexp::protect::Rf_unprotect(2);
            return result;
        }

        crate::sexp::protect::Rf_unprotect(1);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_newenv — new.env(hash, size, parent)
// ---------------------------------------------------------------------------

/// Implement new.env(hash=TRUE, size=29L, parent=parent.frame()).
// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_newenv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _hash = if CAR(args) != R_NilValue() {
            crate::main::coerce::asLogical(CAR(args))
        } else {
            TRUE
        };

        let size = if CADR(args) != R_NilValue() {
            crate::main::coerce::asInteger(CADR(args))
        } else {
            29
        };

        let mut parent = if CADDR(args) != R_NilValue() {
            crate::eval::eval::Rf_eval(CADDR(args), env)
        } else {
            env
        };

        if parent.is_null() {
            parent = R_GlobalEnv();
        }

        crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// do_lengthgets — length(x) <- n
// ---------------------------------------------------------------------------

/// Implement length<- assignment.
// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_lengthgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let target = CAR(args);
        let new_len = crate::main::coerce::asInteger(CADR(args));

        let val = crate::eval::eval::Rf_eval(target, env);
        Rf_protect(val);

        if new_len == NA_INTEGER {
            crate::sexp::protect::Rf_unprotect(1);
            return R_NilValue();
        }

        let t = TYPEOF(val);
        let n = XLENGTH(val) as c_int;

        if new_len == n {
            crate::sexp::protect::Rf_unprotect(1);
            return val;
        }

        if new_len < 0 {
            crate::sexp::protect::Rf_unprotect(1);
            return R_NilValue();
        }

        // For simple vector types, we can resize
        if t == SEXPTYPE::LGLSXP.0
            || t == SEXPTYPE::INTSXP.0
            || t == SEXPTYPE::REALSXP.0
            || t == SEXPTYPE::CPLXSXP.0
            || t == SEXPTYPE::STRSXP.0
            || t == SEXPTYPE::RAWSXP.0
            || t == SEXPTYPE::VECSXP.0
            || t == SEXPTYPE::EXPRSXP.0
        {
            // Allocate new vector of the target length
            let result = Rf_allocVector3(t, new_len as R_xlen_t);
            if result.is_null() {
                crate::sexp::protect::Rf_unprotect(1);
                return R_NilValue();
            }
            Rf_protect(result);

            // Copy data from old to new
            let copy_len = if new_len < n { new_len } else { n };
            if copy_len > 0 {
                let elem_size =
                    crate::sexp::memory::sexp_elem_size(std::mem::transmute::<c_int, SEXPTYPE>(t));
                if elem_size > 0 {
                    let src = (*val).gengc_next_node;
                    let dst = (*result).gengc_next_node;
                    if !src.is_null() && !dst.is_null() {
                        ptr::copy_nonoverlapping(
                            src as *const u8,
                            dst as *mut u8,
                            (copy_len as usize) * elem_size,
                        );
                    }
                }
            }

            // Copy attributes
            crate::main::attrib::copyMostAttrib(val, result);
            crate::sexp::protect::Rf_unprotect(2);
            return result;
        }

        crate::sexp::protect::Rf_unprotect(1);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_makevector — vector(mode, length)
// ---------------------------------------------------------------------------

/// Implement vector(mode, length).
// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_makevector(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mode_arg = CAR(args);
        let len_arg = CADR(args);

        let mode_val = crate::eval::eval::Rf_eval(mode_arg, env);
        let len_val = crate::eval::eval::Rf_eval(len_arg, env);

        let len = crate::main::coerce::asInteger(len_val) as R_xlen_t;
        if len < 0 {
            return R_NilValue();
        }

        let mode_str = if TYPEOF(mode_val) == SEXPTYPE::STRSXP.0 && LENGTH(mode_val) > 0 {
            let s = CStr::from_ptr(CHAR(STRING_ELT(mode_val, 0)));
            s.to_str().unwrap_or("logical").to_string()
        } else if TYPEOF(mode_val) == SEXPTYPE::SYMSXP.0 {
            let pname = PRINTNAME(mode_val);
            CStr::from_ptr(CHAR(pname))
                .to_str()
                .unwrap_or("logical")
                .to_string()
        } else {
            "logical".to_string()
        };

        let sexptype = match mode_str.as_str() {
            "logical" => SEXPTYPE::LGLSXP.0,
            "integer" => SEXPTYPE::INTSXP.0,
            "double" | "numeric" => SEXPTYPE::REALSXP.0,
            "complex" => SEXPTYPE::CPLXSXP.0,
            "character" => SEXPTYPE::STRSXP.0,
            "list" => SEXPTYPE::VECSXP.0,
            "raw" => SEXPTYPE::RAWSXP.0,
            "expression" => SEXPTYPE::EXPRSXP.0,
            _ => SEXPTYPE::LGLSXP.0,
        };

        Rf_allocVector3(sexptype, len)
    }
}

// ---------------------------------------------------------------------------
// do_makelist — list(...)
// ---------------------------------------------------------------------------

/// Implement list(...) — create a list from its arguments.
// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_makelist(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Evaluate all arguments
        let evaled = crate::eval::dispatch::evalList(args, env, call, 0);
        Rf_protect(evaled);

        // Count the number of args
        let mut n: c_int = 0;
        let mut current = evaled;
        while !current.is_null() && current != R_NilValue() {
            n += 1;
            current = CDR(current);
        }

        // Create a vector (list)
        let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, n as R_xlen_t);
        if result.is_null() || n == 0 {
            crate::sexp::protect::Rf_unprotect(1);
            return result;
        }
        Rf_protect(result);

        // Fill the vector
        current = evaled;
        let mut i: R_xlen_t = 0;
        while !current.is_null() && current != R_NilValue() {
            SET_VECTOR_ELT(result, i, CAR(current));
            i += 1;
            current = CDR(current);
        }

        crate::sexp::protect::Rf_unprotect(2);
        result
    }
}

// ---------------------------------------------------------------------------
// do_matchcall — match.call(call, expand.dots = FALSE)
// ---------------------------------------------------------------------------

/// Implement match.call(call, expand.dots = FALSE).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_matchcall(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Simplified: just return the call expression
        let target = CAR(args);
        let target_val = crate::eval::eval::Rf_eval(target, env);
        target_val
    }
}

// ---------------------------------------------------------------------------
// do_makenames — make.names(names, allow_ = TRUE)
// ---------------------------------------------------------------------------

/// Implement make.names(names, allow_ = TRUE).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_makenames(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let names_arg = CAR(args);
        let names_val = crate::eval::eval::Rf_eval(names_arg, env);
        Rf_protect(names_val);

        let allow = if CADR(args) != R_NilValue() {
            crate::main::coerce::asLogical(CADR(args))
        } else {
            TRUE
        };

        let n = XLENGTH(names_val);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n);
        if result.is_null() || n == 0 {
            crate::sexp::protect::Rf_unprotect(1);
            return result;
        }
        Rf_protect(result);

        for i in 0..n as usize {
            let s = CStr::from_ptr(CHAR(STRING_ELT(names_val, i as R_xlen_t)));
            let mut name = s.to_str().unwrap_or("").to_string();

            // Make valid R name
            if name.is_empty() {
                name = "V1".to_string();
            }

            // Replace invalid chars with "."
            let mut chars: Vec<char> = name.chars().collect();
            if chars[0].is_ascii_digit() {
                chars.insert(0, 'V');
            }
            for c in chars.iter_mut() {
                if !c.is_alphanumeric() && *c != '.' && *c != '_' {
                    *c = '.';
                }
            }

            // Collapse consecutive dots
            let collapsed: String = chars
                .iter()
                .collect::<String>()
                .split('.')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(".");

            // Remove trailing dot
            let cleaned = collapsed.trim_end_matches('.');

            let rstr =
                crate::sexp::constructors::Rf_mkChar(CString::new(cleaned).unwrap().as_ptr());
            SET_STRING_ELT(result, i as R_xlen_t, rstr);
        }

        crate::sexp::protect::Rf_unprotect(2);
        result
    }
}

// ---------------------------------------------------------------------------
// do_makeunique — make.unique(names, sep = ".")
// ---------------------------------------------------------------------------

/// Implement make.unique(names, sep = ".").
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_makeunique(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let names_arg = CAR(args);
        let names_val = crate::eval::eval::Rf_eval(names_arg, env);
        Rf_protect(names_val);

        let sep = if CADR(args) != R_NilValue() && TYPEOF(CADR(args)) == SEXPTYPE::STRSXP.0 {
            let s = CStr::from_ptr(CHAR(STRING_ELT(CADR(args), 0)));
            s.to_str().unwrap_or(".").to_string()
        } else {
            ".".to_string()
        };

        let n = XLENGTH(names_val);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n);
        if result.is_null() || n == 0 {
            crate::sexp::protect::Rf_unprotect(1);
            return result;
        }
        Rf_protect(result);

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for i in 0..n as usize {
            let s = CStr::from_ptr(CHAR(STRING_ELT(names_val, i as R_xlen_t)));
            let name = s.to_str().unwrap_or("").to_string();

            if seen.contains(&name) {
                // Find a unique name
                let mut j: c_int = 1;
                loop {
                    let candidate = format!("{}{}{}", name, sep, j);
                    if !seen.contains(&candidate) {
                        let rstr = crate::sexp::constructors::Rf_mkChar(
                            CString::new(candidate.as_str()).unwrap().as_ptr(),
                        );
                        SET_STRING_ELT(result, i as R_xlen_t, rstr);
                        seen.insert(candidate);
                        break;
                    }
                    j += 1;
                }
            } else {
                let rstr = crate::sexp::constructors::Rf_mkChar(
                    CString::new(name.as_str()).unwrap().as_ptr(),
                );
                SET_STRING_ELT(result, i as R_xlen_t, rstr);
                seen.insert(name);
            }
        }

        crate::sexp::protect::Rf_unprotect(2);
        result
    }
}

// ---------------------------------------------------------------------------
// do_substrgets — substr(x, start, stop) <- value
// ---------------------------------------------------------------------------

/// Implement substr(x, start, stop) <- value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_substrgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let target = CAR(args);
        let start_arg = CADR(args);
        let stop_arg = CADDR(args);
        let value_arg = CAR(CDDDR(args));

        let x = crate::eval::eval::Rf_eval(target, env);
        Rf_protect(x);

        let start = crate::main::coerce::asInteger(crate::eval::eval::Rf_eval(start_arg, env));
        let stop = crate::main::coerce::asInteger(crate::eval::eval::Rf_eval(stop_arg, env));
        let value = crate::eval::eval::Rf_eval(value_arg, env);
        Rf_protect(value);

        let t = TYPEOF(x);
        if t != SEXPTYPE::STRSXP.0 {
            crate::sexp::protect::Rf_unprotect(2);
            return R_NilValue();
        }

        let n = LENGTH(x);
        if n == 0 {
            crate::sexp::protect::Rf_unprotect(2);
            return x;
        }

        // R uses 1-based indexing for substr
        let s_start = if start == NA_INTEGER { 1 } else { start };
        let s_stop = if stop == NA_INTEGER { n } else { stop };

        // Get replacement string
        let value_str = if TYPEOF(value) == SEXPTYPE::STRSXP.0 && LENGTH(value) > 0 {
            let s = CStr::from_ptr(CHAR(STRING_ELT(value, 0)));
            s.to_str().unwrap_or("").to_string()
        } else if TYPEOF(value) == SEXPTYPE::CHARSXP.0 {
            let s = CStr::from_ptr(CHAR(value));
            s.to_str().unwrap_or("").to_string()
        } else {
            crate::sexp::protect::Rf_unprotect(2);
            return R_NilValue();
        };

        // Create a copy with the replacement
        let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t);
        if result.is_null() {
            crate::sexp::protect::Rf_unprotect(2);
            return R_NilValue();
        }
        Rf_protect(result);

        for i in 0..n as usize {
            let orig = CStr::from_ptr(CHAR(STRING_ELT(x, i as R_xlen_t)));
            let orig_str = orig.to_str().unwrap_or("");

            // R's substr uses 1-based inclusive start/stop
            let byte_start = if s_start <= 1 {
                0
            } else {
                orig_str
                    .char_indices()
                    .nth((s_start - 1) as usize)
                    .map(|(pos, _)| pos)
                    .unwrap_or(orig_str.len())
            };
            let byte_end = if s_stop < 1 {
                0
            } else {
                orig_str
                    .char_indices()
                    .nth(s_stop as usize)
                    .map(|(pos, _)| pos)
                    .unwrap_or(orig_str.len())
            };

            let new_str = if i == 0 && s_start <= 1 && s_stop >= n {
                // Replace the whole string
                value_str.clone()
            } else {
                orig_str.to_string()
            };

            let rstr =
                crate::sexp::constructors::Rf_mkChar(CString::new(new_str).unwrap().as_ptr());
            SET_STRING_ELT(result, i as R_xlen_t, rstr);
        }

        crate::main::attrib::copyMostAttrib(x, result);
        crate::sexp::protect::Rf_unprotect(3);
        result
    }
}

// ---------------------------------------------------------------------------
// do_returnValue — returnValue()
// ---------------------------------------------------------------------------

/// Implement returnValue() (stub).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_returnValue(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_parentframe — parent.frame()
// ---------------------------------------------------------------------------

/// Implement parent.frame(n = 1).
// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_parentframe(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_sysbrowser — browserText(), browserCondition(), browserSetDebug()
// ---------------------------------------------------------------------------

/// Implement browserText(), browserCondition(), browserSetDebug() (stubs).
// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_sysbrowser(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_isloaded — is.loaded(name)
// ---------------------------------------------------------------------------

/// Implement is.loaded(name) (stub).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_isloaded(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        Rf_ScalarLogical(0) // FALSE
    }
}

// ---------------------------------------------------------------------------
// do_isunsorted — is.unsorted(x, na.rm = FALSE, strictly = FALSE)
// ---------------------------------------------------------------------------

/// Implement is.unsorted(x, na.rm = FALSE, strictly = FALSE).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_isunsorted(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let na_rm = if CADR(args) != R_NilValue() {
            crate::main::coerce::asLogical(CADR(args)) != 0
        } else {
            false
        };
        let strictly = if CDDR(args) != R_NilValue() {
            crate::main::coerce::asLogical(CADDR(args)) != 0
        } else {
            false
        };

        let x_val = crate::eval::eval::Rf_eval(x, env);
        Rf_protect(x_val);

        let t = TYPEOF(x_val);
        if t != SEXPTYPE::REALSXP.0 && t != SEXPTYPE::INTSXP.0 && t != SEXPTYPE::LGLSXP.0 {
            crate::sexp::protect::Rf_unprotect(1);
            return R_NilValue();
        }

        let n = XLENGTH(x_val);
        if n <= 1 {
            crate::sexp::protect::Rf_unprotect(1);
            return Rf_ScalarLogical(FALSE);
        }

        let na_bits = 0x7ff0000000001954u64;
        let mut prev: Option<f64> = None;
        let mut unsorted = false;

        for i in 0..n as usize {
            let val = if t == SEXPTYPE::REALSXP.0 {
                *REAL(x_val).add(i)
            } else if t == SEXPTYPE::INTSXP.0 {
                let v = *INTEGER(x_val).add(i);
                if v == NA_INTEGER {
                    f64::from_bits(na_bits)
                } else {
                    v as f64
                }
            } else {
                let v = *LOGICAL(x_val).add(i);
                if v == NA_INTEGER {
                    f64::from_bits(na_bits)
                } else {
                    v as f64
                }
            };

            if val.is_nan() {
                if !na_rm {
                    unsorted = true;
                    break;
                }
                continue;
            }

            if let Some(p) = prev {
                if strictly {
                    if val <= p {
                        unsorted = true;
                        break;
                    }
                } else {
                    if val < p {
                        unsorted = true;
                        break;
                    }
                }
            }
            prev = Some(val);
        }

        crate::sexp::protect::Rf_unprotect(1);
        Rf_ScalarLogical(if unsorted { TRUE } else { FALSE })
    }
}

// ---------------------------------------------------------------------------
// do_sorted_fpass, do_address, do_named, do_refcnt (stubs)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_sorted_fpass(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_address(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_named(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_refcnt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_pos2env — pos.to.env(pos)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_pos2env(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let pos = CAR(args);
        let pos_val = crate::eval::eval::Rf_eval(pos, env);
        let n = crate::main::coerce::asInteger(pos_val);

        // Walk the environment chain
        let mut current = env;
        let mut i: c_int = 1;
        while !current.is_null() && current != R_NilValue() && i < n {
            current = crate::sexp::accessors::ENCLOS(current);
            i += 1;
        }

        if current.is_null() {
            return R_GlobalEnv();
        }
        current
    }
}

// ---------------------------------------------------------------------------
// do_env2list — as.list(env, all.names = FALSE)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_env2list(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let target = CAR(args);
        let env_val = crate::eval::eval::Rf_eval(target, env);
        Rf_protect(env_val);

        if TYPEOF(env_val) != SEXPTYPE::ENVSXP.0 {
            crate::sexp::protect::Rf_unprotect(1);
            return R_NilValue();
        }

        // List all bindings in the environment
        let frame = crate::sexp::accessors::FRAME(env_val);
        let mut n: c_int = 0;
        let mut current = frame;
        while !current.is_null() && current != R_NilValue() {
            n += 1;
            current = CDR(current);
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, n as R_xlen_t);
        if result.is_null() || n == 0 {
            crate::sexp::protect::Rf_unprotect(1);
            return result;
        }
        Rf_protect(result);

        // Fill with values
        let names = Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t);
        Rf_protect(names);

        current = frame;
        let mut i: R_xlen_t = 0;
        while !current.is_null() && current != R_NilValue() {
            let val = CAR(current);
            SET_VECTOR_ELT(result, i, val);
            // Get name from TAG
            let tag = TAG(current);
            if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP.0 {
                let pname = PRINTNAME(tag);
                let rstr = crate::sexp::constructors::Rf_mkChar(pname as *const c_char);
                SET_STRING_ELT(names, i, rstr);
            }
            i += 1;
            current = CDR(current);
        }

        // Set names attribute
        crate::attrib_core::setAttrib(result, crate::attrib_core::R_NamesSymbol(), names);

        crate::sexp::protect::Rf_unprotect(3);
        result
    }
}

// ---------------------------------------------------------------------------
// do_envirName — environmentName(env)
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_envirName(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        Rf_ScalarString(crate::sexp::constructors::Rf_mkChar(
            b"<environment>\0".as_ptr() as *const c_char,
        ))
    }
}

// ---------------------------------------------------------------------------
// do_envirgets — environment<- assignment
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_envirgets(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Simplified: evaluate RHS, set CLOENV of function
        let target = CAR(args);
        let value = CADR(args);

        let target_val = crate::eval::eval::Rf_eval(target, env);
        Rf_protect(target_val);
        let value_val = crate::eval::eval::Rf_eval(value, env);

        if TYPEOF(target_val) == SEXPTYPE::CLOSXP.0 {
            crate::sexp::accessors::SET_CLOENV(target_val, value_val);
        }

        crate::sexp::protect::Rf_unprotect(1);
        value_val
    }
}

// ---------------------------------------------------------------------------
// do_debugOnOff (stub)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_debugOnOff(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}
