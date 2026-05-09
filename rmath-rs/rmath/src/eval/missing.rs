#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Remaining miscellaneous functions ported from eval.c.
//!
//! This module contains functions from eval.c that didn't fit into the
//! other eval/ submodules:
//! - VectorToPairListNamed: convert named vector to pairlist
//! - DispatchAnyOrEval: dispatch checking all args for S4 methods
//! - classForGroupDispatch: get class for group dispatch
//! - tryDispatch: try S3 method dispatch
//! - tryAssignDispatch: try S3 assignment dispatch
//! - SrcrefPrompt: print source reference prompt
//! - PrintCall: print a call for debugging
//! - R_execMethod: execute S4 method
//! - EnsureLocal: ensure a binding is local (copy if shared)
//! - replaceCall: build a replacement function call
//! - unpromiseArgs: clear promise arguments
//! - signalMissingArgError: error for missing arguments in bytecoded code
//! - check_stack_balance: check protect stack balance
//! - evalKeepVis: evaluate preserving visibility
//! - do_forceAndCall: forceAndCall builtin
//! - do_eval: eval() builtin
//! - R_initEvalSymbols: initialize eval symbols
//! - Various helper functions

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_ClassSymbol, R_NamesSymbol, R_SrcFileSymbol, getAttrib};
use crate::sexp::accessors::{
    BODY, CADDR, CADR, CAR, CDDR, CDR, CHAR, CLOENV, FORMALS, LENGTH, NAMED, PRINTNAME, SET_FRAME,
    SET_NAMED, SET_STRING_ELT, SETCAR, SETTAG, STRING_ELT, TAG, TYPEOF,
};
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::{R_findVar, R_findVarInFrame, defineVar};
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue, R_UnboundValue};
use crate::sexp::instance::{RInstance, with_required_current_instance};
use crate::sexp::memory_ext::{NewEnvironment, allocLang, mkPROMISE, vmaxget, vmaxset};
use crate::sexp::protect::protect;
use crate::sexp::symbol::{R_DotsSymbol, Rf_install};

use super::builtin::PRIMNAME;
use super::closure::applyClosure;
use super::dispatch::{DispatchOrEval, promiseArgs};
use super::eval::Rf_eval;

// ---------------------------------------------------------------------------
// Types and stubs not yet in their home modules
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct R_varloc_t {
    pub cell: SEXP,
}

unsafe fn R_findVarLocInFrame(_rho: SEXP, _symbol: SEXP) -> R_varloc_t {
    R_varloc_t {
        cell: ptr::null_mut(),
    }
}

fn dots_context_error(message: &str) -> ! {
    std::panic::panic_any(RError {
        message: message.to_string(),
    });
}

unsafe fn current_dots(rho: SEXP) -> SEXP {
    unsafe {
        let dots = R_findVarInFrame(rho, R_DotsSymbol());
        if dots.is_null()
            || dots == R_UnboundValue()
            || dots == R_MissingArg()
            || TYPEOF(dots) != SEXPTYPE::DOTSXP
        {
            dots_context_error("incorrect context: the current call has no '...' to look in");
        }
        dots
    }
}

unsafe fn dots_len(dots: SEXP) -> c_int {
    unsafe {
        let mut n = 0;
        let mut cell = dots;
        while !cell.is_null() && cell != R_NilValue() {
            n += 1;
            cell = CDR(cell);
        }
        n
    }
}

unsafe fn dots_cell_at(dots: SEXP, index: c_int) -> SEXP {
    unsafe {
        if index <= 0 {
            dots_context_error("indexing '...' with an invalid index");
        }

        let mut i = 1;
        let mut cell = dots;
        while !cell.is_null() && cell != R_NilValue() {
            if i == index {
                return cell;
            }
            i += 1;
            cell = CDR(cell);
        }

        dots_context_error("indexing '...' with an invalid index");
    }
}

unsafe fn call_from_head_and_args(head: SEXP, args: SEXP) -> SEXP {
    unsafe {
        let mut nargs = 0;
        let mut arg = args;
        while !arg.is_null() && arg != R_NilValue() {
            nargs += 1;
            arg = CDR(arg);
        }

        let call = allocLang(nargs + 1);
        let mut out = call;
        SETCAR(out, head);
        out = CDR(out);

        arg = args;
        while !arg.is_null() && arg != R_NilValue() {
            SETCAR(out, CAR(arg));
            SETTAG(out, TAG(arg));
            out = CDR(out);
            arg = CDR(arg);
        }

        call
    }
}

pub unsafe fn do_dots_length(_call: SEXP, _op: SEXP, _args: SEXP, rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarInteger(dots_len(current_dots(rho))) }
}

pub unsafe fn do_dots_names(_call: SEXP, _op: SEXP, _args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let dots = current_dots(rho);
        let len = dots_len(dots);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
        let _names_guard = protect(names);
        let blank = Rf_mkChar(c"".as_ptr());

        let mut i: R_xlen_t = 0;
        let mut cell = dots;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if tag.is_null() || tag == R_NilValue() {
                blank
            } else {
                PRINTNAME(tag)
            };
            SET_STRING_ELT(names, i, name);
            i += 1;
            cell = CDR(cell);
        }

        names
    }
}

pub unsafe fn do_dots_elt(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let index = if args.is_null() || args == R_NilValue() {
            NA_INTEGER
        } else {
            crate::mainutils::coerce::asInteger(CAR(args))
        };
        let cell = dots_cell_at(current_dots(rho), index);
        let value = CAR(cell);
        if TYPEOF(value) == SEXPTYPE::PROMSXP {
            Rf_eval(value, rho)
        } else {
            value
        }
    }
}

// ---------------------------------------------------------------------------
// VectorToPairListNamed -- convert named vector to pairlist
// ---------------------------------------------------------------------------

/// Convert a named vector to a pairlist, keeping only non-empty names.
///
/// Ported from R's `VectorToPairListNamed()` in eval.c. Used by do_eval()
/// to convert a VECSXP environment to a pairlist for NewEnvironment.
pub unsafe fn VectorToPairListNamed(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let _vmax = vmaxget();
        let _x_guard = protect(x);

        let xnames = getAttrib(x, R_NamesSymbol());
        let _xnames_guard = protect(xnames);
        let named = if !xnames.is_null() && xnames != R_NilValue() {
            TRUE
        } else {
            FALSE
        };

        // Count non-empty names
        let mut len: c_int = 0;
        if named != FALSE {
            let n = LENGTH(x);
            for i in 0..n {
                let elt = STRING_ELT(xnames, i as R_xlen_t);
                if !elt.is_null() {
                    let cs = CHAR(elt);
                    if !cs.is_null() && *cs != 0 {
                        len += 1;
                    }
                }
            }
        }

        let xnew = if len > 0 {
            let new = Rf_allocList(len);
            let _new_guard = protect(new);
            let mut xptr = new;
            let n = LENGTH(x);
            for i in 0..n {
                let elt = STRING_ELT(xnames, i as R_xlen_t);
                if !elt.is_null() {
                    let cs = CHAR(elt);
                    if !cs.is_null() && *cs != 0 {
                        SETCAR(xptr, crate::sexp::accessors::VECTOR_ELT(x, i as R_xlen_t));
                        let tag_name = crate::mainutils::subset::installTrChar(elt);
                        SETTAG(xptr, tag_name);
                        xptr = CDR(xptr);
                    }
                }
            }
            new
        } else {
            Rf_allocList(0)
        };

        vmaxset(_vmax);
        xnew
    }
}

// ---------------------------------------------------------------------------
// DispatchAnyOrEval -- dispatch checking all args for S4
// ---------------------------------------------------------------------------

/// A version of DispatchOrEval that checks all arguments for S4 methods.
///
/// Ported from R's `DispatchAnyOrEval()` in eval.c. Used by c() and
/// previously by [. Differs in that all arguments are evaluated immediately.
pub unsafe fn DispatchAnyOrEval(
    call: SEXP,
    op: SEXP,
    generic: *const c_char,
    args: SEXP,
    rho: SEXP,
    ans: *mut SEXP,
    dropmissing: c_int,
    argsevald: c_int,
) -> c_int {
    unsafe {
        // Check if there are S4 methods
        let has_methods = crate::mainutils::objects::R_has_methods(op);

        if has_methods != FALSE {
            let argValue: SEXP;
            let mut _arg_value_guard = None;

            if argsevald == 0 {
                // Evaluate all arguments
                argValue = super::dispatch::evalList(args, rho, ptr::null_mut(), 0);
                _arg_value_guard = Some(protect(argValue));
            } else {
                argValue = args;
            }

            // Check each argument for S4 objects
            let mut el = argValue;
            while !el.is_null() && el != R_NilValue() {
                if crate::mainutils::coerce::IS_S4_OBJECT(CAR(el)) != FALSE {
                    let value = crate::mainutils::objects::R_possible_dispatch(
                        call, op, argValue, rho, TRUE,
                    );
                    if !value.is_null() && value != R_NilValue() {
                        if !ans.is_null() {
                            *ans = value;
                        }
                        return TRUE as c_int;
                    } else {
                        break;
                    }
                }
                el = CDR(el);
            }

            // Fall through to regular dispatch
            let dispatch = DispatchOrEval(call, op, generic, argValue, rho, ans, dropmissing, TRUE);
            return dispatch;
        }

        DispatchOrEval(call, op, generic, args, rho, ans, dropmissing, argsevald)
    }
}

// ---------------------------------------------------------------------------
// classForGroupDispatch -- get class for group dispatch
// ---------------------------------------------------------------------------

/// Get the class of an object for group generic dispatch.
///
/// Ported from R's `classForGroupDispatch()` in eval.c. Returns the
/// class attribute if it exists, otherwise the implicit class.
unsafe fn classForGroupDispatch(obj: SEXP) -> SEXP {
    unsafe {
        if obj.is_null() || obj == R_NilValue() {
            return R_NilValue();
        }

        let klass = getAttrib(obj, R_ClassSymbol());
        if !klass.is_null()
            && klass != R_NilValue()
            && TYPEOF(klass) == SEXPTYPE::STRSXP
            && LENGTH(klass) > 0
        {
            return klass;
        }

        // Fall back to implicit class
        let t = TYPEOF(obj);
        let type_str = match t {
            x if x == SEXPTYPE::LGLSXP => "logical",
            x if x == SEXPTYPE::INTSXP => "integer",
            x if x == SEXPTYPE::REALSXP => "numeric",
            x if x == SEXPTYPE::CPLXSXP => "complex",
            x if x == SEXPTYPE::STRSXP => "character",
            x if x == SEXPTYPE::RAWSXP => "raw",
            x if x == SEXPTYPE::VECSXP => "list",
            x if x == SEXPTYPE::LISTSXP => "list",
            x if x == SEXPTYPE::NILSXP => "NULL",
            x if x == SEXPTYPE::CLOSXP => "function",
            x if x == SEXPTYPE::SPECIALSXP => "function",
            x if x == SEXPTYPE::BUILTINSXP => "function",
            _ => "unknown",
        };
        Rf_ScalarString(crate::sexp::symbol::Rf_install(
            type_str.as_ptr() as *const c_char
        ))
    }
}

// ---------------------------------------------------------------------------
// tryDispatch -- try S3 method dispatch
// ---------------------------------------------------------------------------

/// Try to dispatch to an S3 method.
///
/// Ported from R's `tryDispatch()` in eval.c. Creates promises for
/// the arguments, then calls usemethod to dispatch to the appropriate
/// method. Returns TRUE if dispatch succeeded, FALSE otherwise.
unsafe fn tryDispatch(
    generic: *mut c_char,
    call: SEXP,
    x: SEXP,
    rho: SEXP,
    pv: *mut SEXP,
) -> c_int {
    unsafe {
        let generic_sym = Rf_install(generic);

        let pargs = promiseArgs(CDR(call), rho);
        let _pargs_guard = protect(pargs);

        // Set the first promise value to x
        if !pargs.is_null() && pargs != R_NilValue() {
            let first_promise = CAR(pargs);
            if TYPEOF(first_promise) == SEXPTYPE::PROMSXP {
                crate::sexp::accessors::SET_PRVALUE(first_promise, x);
            }
        }

        // Check for S4 methods
        if crate::mainutils::coerce::IS_S4_OBJECT(x) != FALSE
            && crate::mainutils::objects::R_has_methods(generic_sym) != FALSE
        {
            let value =
                crate::mainutils::objects::R_possible_dispatch(call, generic_sym, pargs, rho, TRUE);
            if !value.is_null() && value != R_NilValue() {
                if !pv.is_null() {
                    *pv = value;
                }
                return TRUE as c_int;
            }
        }

        // Try S3 dispatch
        let rho1 = NewEnvironment(R_NilValue(), R_NilValue(), rho);
        let _rho1_guard = protect(rho1);

        let mut dispatched: c_int = FALSE;
        let mut result: SEXP = R_NilValue();
        let dispatch_result = crate::mainutils::objects::usemethod(
            generic,
            x,
            call,
            pargs,
            rho1,
            rho,
            super::runtime::base_env(),
            &mut result,
        );
        if dispatch_result != FALSE {
            dispatched = TRUE;
            if !pv.is_null() {
                *pv = result;
            }
        }

        if dispatched != FALSE { TRUE } else { FALSE }
    }
}

// ---------------------------------------------------------------------------
// tryAssignDispatch -- try S3 method dispatch for assignment
// ---------------------------------------------------------------------------

/// Try S3 method dispatch for assignment operations.
///
/// Ported from R's `tryAssignDispatch()` in eval.c. Creates a copy of
/// the call with the RHS wrapped in a promise, then tries dispatch.
unsafe fn tryAssignDispatch(
    generic: *mut c_char,
    call: SEXP,
    lhs: SEXP,
    rhs: SEXP,
    rho: SEXP,
    pv: *mut SEXP,
) -> c_int {
    unsafe {
        // Duplicate the call
        let ncall = crate::mainutils::duplicate::Rf_duplicate(call);
        let _ncall_guard = protect(ncall);

        // Find the last element and wrap RHS in a promise
        let mut last = ncall;
        while !CDR(last).is_null() && CDR(last) != R_NilValue() {
            last = CDR(last);
        }
        let prom = mkPROMISE(CAR(last), rho);
        SETCAR(last, prom);

        let result = tryDispatch(generic, ncall, lhs, rho, pv);
        result
    }
}

// ---------------------------------------------------------------------------
// SrcrefPrompt -- print source reference prompt
// ---------------------------------------------------------------------------

/// Print a source reference prompt.
///
/// Ported from R's `SrcrefPrompt()` in eval.c. If a valid srcref is
/// available, prints the filename and line number.
pub unsafe fn SrcrefPrompt(prefix: *const c_char, srcref: SEXP) {
    unsafe {
        if srcref.is_null() || srcref == R_NilValue() {
            if !prefix.is_null() {
                if let Ok(s) = std::ffi::CStr::from_ptr(prefix).to_str() {
                    eprint!("{}: ", s);
                }
            }
            return;
        }

        let mut sref = srcref;
        if TYPEOF(srcref) == SEXPTYPE::VECSXP {
            sref = crate::sexp::accessors::VECTOR_ELT(srcref, 0);
        }

        let srcfile = getAttrib(sref, R_SrcFileSymbol());
        if !srcfile.is_null() && TYPEOF(srcfile) == SEXPTYPE::ENVSXP {
            let filename_sym = Rf_install(b"filename\x00".as_ptr() as *const c_char);
            let filename = R_findVarInFrame(srcfile, filename_sym);
            if TYPEOF(filename) == SEXPTYPE::STRSXP && LENGTH(filename) > 0 {
                let fname_elt = STRING_ELT(filename, 0);
                let fname_cs = if !fname_elt.is_null() {
                    CHAR(fname_elt)
                } else {
                    ptr::null()
                };
                let line_num = if TYPEOF(sref) == SEXPTYPE::INTSXP && LENGTH(sref) > 0 {
                    let d = crate::sexp::accessors::INTEGER(sref);
                    if !d.is_null() { *d } else { 0 }
                } else {
                    0
                };
                if !fname_cs.is_null() && *fname_cs != 0 {
                    if let Ok(fname) = std::ffi::CStr::from_ptr(fname_cs).to_str() {
                        let pfx = if !prefix.is_null() {
                            std::ffi::CStr::from_ptr(prefix).to_str().ok()
                        } else {
                            None
                        };
                        eprintln!("{} at {}#{}: ", pfx.unwrap_or_default(), fname, line_num);
                        return;
                    }
                }
            }
        }

        // Default
        if !prefix.is_null() {
            if let Ok(s) = std::ffi::CStr::from_ptr(prefix).to_str() {
                eprint!("{}: ", s);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PrintCall -- print a call for debugging
// ---------------------------------------------------------------------------

/// Print a call for debugging purposes.
///
/// Ported from R's `PrintCall()` in eval.c. Used when debugging closures.
pub unsafe fn PrintCall(call: SEXP, _rho: SEXP) {
    unsafe {
        if call.is_null() {
            return;
        }
        // Simplified: just print the call using deparse
        // In the full implementation, this uses PrintValueRec
        crate::mainutils::print::PrintValue(call);
    }
}

// ---------------------------------------------------------------------------
// R_execMethod -- execute an S4 method
// ---------------------------------------------------------------------------

/// Execute an S4 method in the appropriate environment.
///
/// Ported from R's `R_execMethod()` in eval.c. Creates a new environment
/// for the method, copies bindings from the generic call, and executes
/// the method body.
pub unsafe fn R_execMethod(op: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if op.is_null() || TYPEOF(op) != SEXPTYPE::CLOSXP {
            return R_NilValue();
        }

        // Create new environment enclosed by the method's lexical environment
        let newrho = NewEnvironment(R_NilValue(), R_NilValue(), CLOENV(op));
        let _newrho_guard = protect(newrho);

        // Copy formal bindings from the generic call
        let mut next = FORMALS(op);
        while !next.is_null() && next != R_NilValue() {
            let symbol = TAG(next);
            if !symbol.is_null() {
                let val = R_findVarInFrame(rho, symbol);
                if val != R_UnboundValue() {
                    let cell = Rf_cons(val, crate::sexp::accessors::FRAME(newrho));
                    SETTAG(cell, symbol);
                    SET_FRAME(newrho, cell);
                }
            }
            next = CDR(next);
        }

        // Copy S4 dispatch variables
        let dot_defined = Rf_install(b".defined\x00".as_ptr() as *const c_char);
        let dot_Method = Rf_install(b".Method\x00".as_ptr() as *const c_char);
        let dot_target = Rf_install(b".target\x00".as_ptr() as *const c_char);
        let dot_Generic = Rf_install(b".Generic\x00".as_ptr() as *const c_char);
        let dot_Methods = Rf_install(b".Methods\x00".as_ptr() as *const c_char);

        let dd = R_findVarInFrame(rho, dot_defined);
        defineVar(dot_defined, dd, newrho);
        let dm = R_findVarInFrame(rho, dot_Method);
        defineVar(dot_Method, dm, newrho);
        let dt = R_findVarInFrame(rho, dot_target);
        defineVar(dot_target, dt, newrho);
        let dg = R_findVar(dot_Generic, rho);
        defineVar(dot_Generic, dg, newrho);
        let dms = R_findVar(dot_Methods, rho);
        defineVar(dot_Methods, dms, newrho);

        // Execute the method body
        let body = BODY(op);
        let val = Rf_eval(body, newrho);

        val
    }
}

// ---------------------------------------------------------------------------
// NAMED helpers -- MAYBE_SHARED, INCREMENT_NAMED
// ---------------------------------------------------------------------------

/// Check if an object might be shared (NAMED >= 2).
unsafe fn MAYBE_SHARED(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        NAMED(x) >= 2
    }
}

/// Increment the NAMED field, capped at 2.
unsafe fn INCREMENT_NAMED(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let n = NAMED(x);
            if n < 2 {
                SET_NAMED(x, n + 1);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// EnsureLocal -- ensure a binding is local (copy if shared)
// ---------------------------------------------------------------------------

/// Ensure a variable binding is local to the current environment.
///
/// Ported from R's `EnsureLocal()` in eval.c. If the variable exists
/// in the environment and might be shared, duplicate it. If it doesn't
/// exist locally, look it up in enclosing environments and copy it locally.
/// Ensure a variable binding is local to the current environment.
///
/// Ported from R's `EnsureLocal()` in eval.c (lines 2570-2603).
/// If the variable exists in the environment and might be shared, duplicate it it If it
/// doesn't exist locally, look it up in enclosing environments and copy it locally.
///
/// Returns the pair of (value, R_varloc_t) where R_varloc_t has the
/// location of the binding for potential future mutations.
pub unsafe fn EnsureLocal(symbol: SEXP, rho: SEXP, ploc: *mut R_varloc_t) -> SEXP {
    unsafe {
        if symbol.is_null() || rho.is_null() || ploc.is_null() {
            return R_NilValue();
        }

        let mut vl = R_findVarInFrame(rho, symbol);
        if vl != R_UnboundValue() {
            // Found locally — evaluate (for promises) and copy if shared (C lines 2575-2576)
            vl = Rf_eval(symbol, rho);
            if MAYBE_SHARED(vl) {
                // Duplicate using R_shallow_duplicate_attr which may defer
                // duplicating data until it is needed. If the data are duplicated,
                // then the wrapper can be discarded at the end of the
                // assignment process in try_assign_unwrap(). (C lines 2577-2586)
                let _vl_guard = protect(vl);
                vl = crate::mainutils::duplicate::R_shallow_duplicate_attr(vl);
                defineVar(symbol, vl, rho);
                INCREMENT_NAMED(vl);
            }
            // Look up the location for future mutation (C lines 2587-2589)
            let _vl_guard = protect(vl);
            *ploc = R_findVarLocInFrame(rho, symbol);
            vl
        } else {
            // Not found locally -- look up in enclosing environments (C lines 2593-2601)
            let enc = crate::sexp::accessors::ENCLOS(rho);
            vl = Rf_eval(symbol, enc);
            if vl == R_UnboundValue() {
                let pname = PRINTNAME(symbol);
                let name = if !pname.is_null() {
                    let s = CHAR(pname);
                    if !s.is_null() {
                        std::ffi::CStr::from_ptr(s)
                            .to_str()
                            .map(str::to_string)
                            .unwrap_or_else(|_| "???".to_string())
                    } else {
                        "???".to_string()
                    }
                } else {
                    "???".to_string()
                };
                eprintln!("Error: object '{}' not found", name);
                std::panic::panic_any(RError {
                    message: format!("object '{}' not found", name),
                });
            }
            // Create local copy (C lines 2597-2601)
            let _vl_guard = protect(vl);
            vl = crate::mainutils::duplicate::shallow_duplicate(vl);
            defineVar(symbol, vl, rho);
            *ploc = R_findVarLocInFrame(rho, symbol);
            INCREMENT_NAMED(vl);
            vl
        }
    }
}

// ---------------------------------------------------------------------------
// replaceCall -- build a replacement function call
// ---------------------------------------------------------------------------

/// Build a replacement function call from components.
///
/// Ported from R's `replaceCall()` in eval.c. Constructs a call like:
///   fun(target, indices..., value = rhs)
pub unsafe fn replaceCall(fun: SEXP, val: SEXP, args: SEXP, rhs: SEXP) -> SEXP {
    unsafe {
        let nargs = if !args.is_null() && args != R_NilValue() {
            LENGTH(args)
        } else {
            0
        };
        let total = nargs + 3;

        let _fun_guard = protect(fun);
        let _args_guard = protect(args);
        let _rhs_guard = protect(rhs);
        let _val_guard = protect(val);

        let tmp = allocLang(total);
        let _tmp_guard = protect(tmp);
        let mut ptmp = tmp;

        SETCAR(ptmp, fun);
        ptmp = CDR(ptmp);
        SETCAR(ptmp, val);
        ptmp = CDR(ptmp);

        let mut a = args;
        while !a.is_null() && a != R_NilValue() {
            SETCAR(ptmp, CAR(a));
            SETTAG(ptmp, TAG(a));
            ptmp = CDR(ptmp);
            a = CDR(a);
        }

        SETCAR(ptmp, rhs);
        let value_sym = Rf_install(b"value\x00".as_ptr() as *const c_char);
        SETTAG(ptmp, value_sym);

        tmp
    }
}

// ---------------------------------------------------------------------------
// unpromiseArgs -- clear promise arguments after closure execution
// ---------------------------------------------------------------------------

/// Clear promise arguments to allow GC to reclaim their environments.
///
/// Ported from R's `unpromiseArgs()` in eval.c. This is called after
/// a closure execution to clean up promise arguments that are no longer
/// needed. It clears the promise code and environment fields.
pub unsafe fn unpromiseArgs(pargs: SEXP) {
    unsafe {
        let mut current = pargs;
        while !current.is_null() && current != R_NilValue() {
            let v = CAR(current);
            if TYPEOF(v) == SEXPTYPE::PROMSXP {
                // Clear the promise to allow GC
                crate::sexp::accessors::SET_PRVALUE(v, R_UnboundValue());
                crate::sexp::accessors::SET_PRENV(v, R_NilValue());
                crate::sexp::accessors::SET_PRCODE(v, R_NilValue());
            }
            SETCAR(current, R_NilValue());
            current = CDR(current);
        }
    }
}

// ---------------------------------------------------------------------------
// signalMissingArgError -- error for missing arguments in bytecoded code
// ---------------------------------------------------------------------------

/// Signal an error for a missing argument in bytecoded code.
///
/// Ported from R's `signalMissingArgError()` in eval.c. Called when
/// the bytecode interpreter encounters a missing argument in a context
/// where one is required.
pub unsafe fn signalMissingArgError(call: SEXP, _rho: SEXP, arg_sym: SEXP) {
    unsafe {
        let msg = if arg_sym.is_null() {
            "argument is missing, with no default".to_string()
        } else {
            let pname = PRINTNAME(arg_sym);
            let name = if !pname.is_null() {
                let s = CHAR(pname);
                if !s.is_null() {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .map(str::to_string)
                        .unwrap_or_else(|_| "???".to_string())
                } else {
                    "???".to_string()
                }
            } else {
                "???".to_string()
            };
            format!("argument \"{}\" is missing, with no default", name)
        };
        crate::mainutils::errors::errorcall_cpy(
            call,
            std::ffi::CString::new(msg)
                .expect("generated missing-argument message has no interior NUL")
                .as_ptr(),
        );
    }
}

// ---------------------------------------------------------------------------
// check_stack_balance -- check protect stack balance
// ---------------------------------------------------------------------------

/// Check that the protect stack is balanced after a primitive call.
///
/// Ported from R's `check_stack_balance()` in eval.c.
pub unsafe fn check_stack_balance(op: SEXP, save: c_int) {
    unsafe {
        let current = crate::mainutils::main::R_PPStackTop();
        if save == current {
            return;
        }
        let name = PRIMNAME(op);
        eprintln!(
            "Warning: stack imbalance in '{}', {} then {}",
            name, save, current
        );
    }
}

// ---------------------------------------------------------------------------
// do_forceAndCall -- the forceAndCall builtin
// ---------------------------------------------------------------------------

/// Implement the `.Internal(forceAndCall(n, expr))` builtin.
///
/// Ported from R's `do_forceAndCall()` in eval.c. Forces the first n
/// promises in the argument list and then calls the function.
// no_mangle removed (duplicate)
pub unsafe fn do_forceAndCall(call: SEXP, _op: SEXP, _args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let n_expr = CADR(call);
        let n = crate::mainutils::coerce::asInteger(Rf_eval(n_expr, rho));
        let e = CDDR(call);

        // Build a proper call from the expression
        let fun_expr = CAR(e);
        let rest = CDR(e);

        // Find the function
        let fun = if TYPEOF(fun_expr) == SEXPTYPE::SYMSXP {
            crate::sexp::envir::findFun(fun_expr, rho)
        } else {
            Rf_eval(fun_expr, rho)
        };

        if fun == R_UnboundValue() {
            return R_NilValue();
        }

        let _fun_guard = protect(fun);

        let result = if TYPEOF(fun) == SEXPTYPE::BUILTINSXP {
            let delegated_call = call_from_head_and_args(fun_expr, rest);
            let _delegated_call_guard = protect(delegated_call);
            Rf_eval(delegated_call, rho)
        } else if TYPEOF(fun) == SEXPTYPE::CLOSXP {
            let pargs = promiseArgs(rest, rho);
            let _pargs_guard = protect(pargs);
            // Force the first n promises
            let mut a = pargs;
            let mut count: c_int = 0;
            while !a.is_null() && a != R_NilValue() && count < n {
                let p = CAR(a);
                if TYPEOF(p) == SEXPTYPE::PROMSXP {
                    let _ = Rf_eval(p, rho);
                } else if p == R_MissingArg() {
                    eprintln!("Error: argument {} is empty", count + 1);
                    std::panic::panic_any(RError {
                        message: format!("argument {} is empty", count + 1),
                    });
                }
                count += 1;
                a = CDR(a);
            }
            applyClosure(call, fun, pargs, rho, R_NilValue(), TRUE)
        } else if TYPEOF(fun) == SEXPTYPE::SPECIALSXP {
            let flag = super::eval::PRIMPRINT(fun);
            super::runtime::set_visible_for_print_flag(flag);
            if let Some(primfun) = super::eval::get_primfun(fun) {
                let tmp = primfun(call, fun, rest, rho);
                if flag < 2 {
                    super::runtime::set_visible_for_print_flag(flag);
                }
                tmp
            } else {
                R_NilValue()
            }
        } else {
            R_NilValue()
        };

        result
    }
}

// ---------------------------------------------------------------------------
// do_eval -- the eval() builtin
// ---------------------------------------------------------------------------

/// Implement the `eval(expr, envir, enclos)` builtin.
///
/// Ported from R's `do_eval()` in eval.c. Evaluates an expression in
/// the specified environment.
pub unsafe fn do_eval(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let mut env = CADR(args);
        let encl = CADDR(args);
        let mut _env_guard = None;

        // Handle enclos
        let mut encl_val = encl;
        if encl_val.is_null() || encl_val == R_NilValue() {
            encl_val = super::runtime::base_env();
        }

        // Handle different environment types
        match TYPEOF(env) {
            t if t == SEXPTYPE::NILSXP => {
                env = encl_val;
            }
            t if t == SEXPTYPE::ENVSXP => {
                // OK
            }
            t if t == SEXPTYPE::LISTSXP => {
                // Create environment from pairlist
                let dup = crate::mainutils::duplicate::Rf_duplicate(env);
                let _dup_guard = protect(dup);
                env = NewEnvironment(R_NilValue(), dup, encl_val);
                _env_guard = Some(protect(env));
            }
            t if t == SEXPTYPE::VECSXP => {
                let x = VectorToPairListNamed(env);
                let _x_guard = protect(x);
                // Ensure NAMEDMAX on values
                let mut xptr = x;
                while !xptr.is_null() && xptr != R_NilValue() {
                    SET_NAMED(CAR(xptr), 2);
                    xptr = CDR(xptr);
                }
                env = NewEnvironment(R_NilValue(), x, encl_val);
                _env_guard = Some(protect(env));
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP => {
                // Numeric environment = sys.frame(n)
                let frame = crate::mainutils::coerce::asInteger(env);
                if frame == NA_INTEGER {
                    eprintln!("Error: invalid 'envir' argument");
                    return R_NilValue();
                }
                env = super::context::R_sysframe(frame, ptr::null_mut());
            }
            _ => {
                eprintln!("Error: invalid 'envir' argument");
                return R_NilValue();
            }
        }

        // Evaluate the expression
        let _visibility = super::runtime::VisibilityGuard::new();
        let val = Rf_eval(expr, env);
        val
    }
}

// ---------------------------------------------------------------------------
// R_initEvalSymbols -- initialize all eval-related symbols
// ---------------------------------------------------------------------------

/// Initialize all symbols used by the evaluator.
///
/// Ported from R's `R_initEvalSymbols()` in eval.c. Installs symbols
/// for assignment operators and other special forms.
pub unsafe fn R_initEvalSymbols() {
    unsafe {
        // Assignment operator symbols
        Rf_install(b":=\x00".as_ptr() as *const c_char);
        Rf_install(b"<-\x00".as_ptr() as *const c_char);
        Rf_install(b"<<-\x00".as_ptr() as *const c_char);
        Rf_install(b"=\x00".as_ptr() as *const c_char);

        // Subset and subassign symbols
        Rf_install(b"[\x00".as_ptr() as *const c_char);
        Rf_install(b"[<-\x00".as_ptr() as *const c_char);
        Rf_install(b"[[\x00".as_ptr() as *const c_char);
        Rf_install(b"[[<-\x00".as_ptr() as *const c_char);
        Rf_install(b"$<-\x00".as_ptr() as *const c_char);
        Rf_install(b"value\x00".as_ptr() as *const c_char);

        // Control flow symbols
        Rf_install(b"if\x00".as_ptr() as *const c_char);
        Rf_install(b"while\x00".as_ptr() as *const c_char);
        Rf_install(b"for\x00".as_ptr() as *const c_char);
        Rf_install(b"repeat\x00".as_ptr() as *const c_char);
        Rf_install(b"break\x00".as_ptr() as *const c_char);
        Rf_install(b"next\x00".as_ptr() as *const c_char);
        Rf_install(b"return\x00".as_ptr() as *const c_char);
        Rf_install(b"function\x00".as_ptr() as *const c_char);
        Rf_install(b"quote\x00".as_ptr() as *const c_char);
        Rf_install(b"missing\x00".as_ptr() as *const c_char);
        Rf_install(b"on.exit\x00".as_ptr() as *const c_char);

        // Initialize JIT
        super::jit::R_init_jit_enabled();
        super::jit::init_exec_token();

        // Initialize eval symbols from symbols.rs
        super::symbols::R_initEvalSymbols();
    }
}

// ---------------------------------------------------------------------------
// evalseq -- evaluate assignment LHS sequence
// ---------------------------------------------------------------------------

/// Evaluate the LHS of a complex assignment expression.
///
/// Ported from R's `evalseq()` in eval.c. This recursively evaluates
/// the LHS of a complex assignment (e.g., x[i][j] <- val) to find
/// the target variable and intermediate values.
pub unsafe fn evalseq(expr: SEXP, rho: SEXP, forcelocal: c_int) -> SEXP {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            eprintln!("Error: invalid (NULL) left side of assignment");
            std::panic::panic_any(RError {
                message: "invalid (NULL) left side of assignment".to_string(),
            });
        }

        if TYPEOF(expr) == SEXPTYPE::SYMSXP {
            // Simple symbol -- the target variable
            let val = if forcelocal != FALSE {
                let mut ploc = R_varloc_t {
                    cell: ptr::null_mut(),
                };
                EnsureLocal(expr, rho, &mut ploc)
            } else {
                let enc = crate::sexp::accessors::ENCLOS(rho);
                Rf_eval(expr, enc)
            };
            // Return (val . sym) pair
            let cell = Rf_cons(val, expr);
            if !cell.is_null() {
                SETTAG(cell, expr);
            }
            cell
        } else if TYPEOF(expr) == SEXPTYPE::LANGSXP {
            // Complex LHS -- recurse
            let inner = evalseq(CADR(expr), rho, forcelocal);
            let _inner_guard = protect(inner);
            let target_val = CAR(inner);
            let target_sym = CDR(inner);

            // Rebuild the expression with the target value
            let rest = CDDR(expr);
            let new_inner = Rf_cons(target_sym, rest);
            let _new_inner_guard = protect(new_inner);
            let new_expr = Rf_cons(CAR(expr), new_inner);
            let _new_expr_guard = protect(new_expr);
            if !new_expr.is_null() {
                (*new_expr).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }

            let nval = Rf_eval(new_expr, rho);
            let _nval_guard = protect(nval);

            // Simplified: always duplicate for safety
            let dup = crate::mainutils::duplicate::shallow_duplicate(nval);
            let _dup_guard = protect(dup);
            let cell = Rf_cons(dup, inner);
            cell
        } else {
            eprintln!("Error: target of assignment expands to non-language object");
            std::panic::panic_any(RError {
                message: "target of assignment expands to non-language object".to_string(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// R_BCIntActive -- bytecode interpreter reentrancy flag
// ---------------------------------------------------------------------------

/// Get whether the bytecode interpreter is active.
#[inline]
pub fn get_R_BCIntActive() -> c_int {
    with_required_current_instance(get_R_BCIntActive_in)
}

pub(crate) fn get_R_BCIntActive_in(inst: &mut RInstance) -> c_int {
    inst.eval_state.bc_int_active
}

/// Set whether the bytecode interpreter is active.
#[inline]
pub fn set_R_BCIntActive(val: c_int) {
    with_required_current_instance(|inst| set_R_BCIntActive_in(inst, val));
}

pub(crate) fn set_R_BCIntActive_in(inst: &mut RInstance, val: c_int) {
    inst.eval_state.bc_int_active = val;
}

// ---------------------------------------------------------------------------
// markSpecialArgs -- mark arguments that need special handling
// ---------------------------------------------------------------------------

/// Mark arguments in a call that need special handling by the bytecode
/// interpreter (missing, ..., etc.).
///
/// Ported from R's `markSpecialArgs()` in eval.c.
pub unsafe fn markSpecialArgs(args: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return args;
        }
        // Simplified: return args as-is
        // In the full implementation, this marks nodes with special flags
        // for the bytecode interpreter
        args
    }
}

// ---------------------------------------------------------------------------
// inflateAssignmentCall -- inflate an assignment call for the BC interpreter
// ---------------------------------------------------------------------------

/// Inflate a complex assignment call for the bytecode interpreter.
///
/// Ported from R's `inflateAssignmentCall()` in eval.c. The bytecode
/// compiler produces compact assignment calls that need to be expanded
/// to their full form for the interpreter.
pub unsafe fn inflateAssignmentCall(expr: SEXP) -> SEXP {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: return as-is
        // In the full implementation, this expands compact assignment forms
        // like x[i] <- val into [<-(x, i, value)
        expr
    }
}

// ---------------------------------------------------------------------------
// bc_check_sigint -- check for user interrupts in bytecode loop
// ---------------------------------------------------------------------------

/// Check for user interrupts during bytecode execution.
///
/// Ported from R's `bc_check_sigint()` in eval.c.
pub unsafe fn bc_check_sigint() {
    // In the full implementation, this calls R_CheckUserInterrupt()
    // and R_RunPendingFinalizers()
}

// ---------------------------------------------------------------------------
// findLocTable -- find a location table for the BC interpreter
// ---------------------------------------------------------------------------

/// Find a location table entry in bytecode constants.
///
/// Ported from R's `findLocTable()` in eval.c.
pub unsafe fn findLocTable(constants: SEXP, tclass: *const c_char) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// R_findBCInterpreterLocation -- find BC interpreter source location
// ---------------------------------------------------------------------------

/// Find the source location of the currently executing bytecode.
///
/// Ported from R's `R_findBCInterpreterLocation()` in eval.c.
pub unsafe fn R_findBCInterpreterLocation(_cptr: SEXP, _iname: *const c_char) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// R_findBCInterpreterExpression -- find the current BC expression
// ---------------------------------------------------------------------------

/// Find the expression currently being evaluated by the bytecode interpreter.
///
/// Ported from R's `R_findBCInterpreterExpression()` in eval.c.
pub unsafe fn R_findBCInterpreterExpression() -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_missing -- the missing() builtin
// ---------------------------------------------------------------------------

/// Implement the `missing()` builtin.
///
/// Ported from R's `do_missing()` in eval.c. Checks whether a formal
/// argument is missing.
// no_mangle removed (duplicate)
pub unsafe fn do_missing(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let sym = CAR(args);
        if TYPEOF(sym) != SEXPTYPE::SYMSXP {
            // Evaluate and check
            let val = Rf_eval(sym, rho);
            if val == R_MissingArg() {
                Rf_ScalarLogical(TRUE)
            } else {
                Rf_ScalarLogical(FALSE)
            }
        } else {
            let missing = crate::sexp::envir::R_isMissing(sym, rho);
            Rf_ScalarLogical(missing)
        }
    }
}

// ---------------------------------------------------------------------------
// on.exit helpers
// ---------------------------------------------------------------------------

/// Run on.exit handlers registered in contexts.
///
/// This is called when a function exits (normally or via error).
pub unsafe fn run_onexits() {
    unsafe {
        super::context::R_run_onexits();
    }
}

// ---------------------------------------------------------------------------
// R_Srcref -- current source reference (thread-local)
// ---------------------------------------------------------------------------

/// Set the current source reference.
pub unsafe fn set_R_Srcref(_srcref: SEXP) {
    // In the full implementation, this sets a thread-local global
}

// ---------------------------------------------------------------------------
// R_initialize_bcode -- initialize bytecode system
// ---------------------------------------------------------------------------

/// Full initialization of the bytecode system.
///
/// Ported from R's `R_initialize_bcode()` in eval.c (the full version
/// that sets up the opcode table for threaded code).
pub unsafe fn R_initialize_bcode_full() {
    // In the full implementation, this initializes the opcode table
    // for the threaded code interpreter. Our simplified version
    // doesn't need this.
}

// ---------------------------------------------------------------------------
// R_ensureNamed -- ensure NAMED bit is set
// ---------------------------------------------------------------------------

/// Ensure a value has NAMED >= 1.
///
/// Ported from the ENSURE_NAMED macro in eval.c.
pub unsafe fn R_ensureNamed(x: SEXP, n: c_int) {
    unsafe {
        if !x.is_null() && x != R_NilValue() {
            let current = crate::sexp::accessors::NAMED(x);
            if current < n {
                SET_NAMED(x, n);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R_ensureNamedMax -- ensure NAMED == NAMEDMAX (2)
// ---------------------------------------------------------------------------

/// Ensure a value has NAMED == NAMEDMAX.
pub unsafe fn R_ensureNamedMax(x: SEXP) {
    unsafe {
        R_ensureNamed(x, 2);
    }
}

#[cfg(test)]
mod tests {
    use crate::sexp::instance::RInstance;
    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn test_session_bc_int_active_is_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.with_arena(|_| {
            set_R_BCIntActive(1);
            assert_eq!(get_R_BCIntActive(), 1);
        })
        .unwrap();

        right
            .with_arena(|_| {
                assert_eq!(get_R_BCIntActive(), 0);
                set_R_BCIntActive(2);
                assert_eq!(get_R_BCIntActive(), 2);
            })
            .unwrap();

        left.with_arena(|_| {
            assert_eq!(get_R_BCIntActive(), 1);
            set_R_BCIntActive(0);
        })
        .unwrap();
    }

    #[test]
    fn test_bc_int_active_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        set_R_BCIntActive_in(&mut left, 1);
        set_R_BCIntActive_in(&mut right, 2);

        assert_eq!(get_R_BCIntActive_in(&mut left), 1);
        assert_eq!(get_R_BCIntActive_in(&mut right), 2);
    }
}
