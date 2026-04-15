#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Argument evaluation and dispatch — ports parts of eval.c.
//!
//! Handles:
//! - evalList: evaluate argument lists
//! - promiseArgs: create promises for closure arguments
//! - forcePromise: force evaluation of promises
//! - DispatchOrEval: S3/S4 method dispatch
//! - DispatchGroup: group generic dispatch (Math, Summary, Ops, Complex)
//! - findmethod: find S3 method in class hierarchy

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_ClassSymbol, getAttrib, isObject};
use crate::sexp::accessors::{
    CADR, CAR, CDR, CHAR, LENGTH, PRINTNAME, SET_STRING_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT,
    TAG, TYPEOF,
};
use crate::sexp::constructors::*;
use crate::sexp::envir::{R_findVar, R_findVarInFrame, R_isMissing, forcePromise};
use crate::sexp::ffi::{FALSE, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_BaseEnv, R_MissingArg, R_NilValue};
use crate::sexp::memory_ext::{CONS_NR, NewEnvironment, mkPROMISE, vmaxget, vmaxset};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::R_DotsSymbol;

use super::builtin::PRIMNAME;
use super::closure::applyClosure;
use super::eval::Rf_eval;

// ---------------------------------------------------------------------------
// evalList — evaluate each element of a pairlist
// ---------------------------------------------------------------------------

/// Evaluate each element of a pairlist, returning a new pairlist of results.
///
/// This is the equivalent of R's `evalList()` in eval.c.
pub unsafe fn evalList(el: SEXP, rho: SEXP, call: SEXP, nargs: c_int) -> SEXP {
    unsafe {
        if el.is_null() || el == R_NilValue() {
            return R_NilValue();
        }

        let mut result: SEXP = R_NilValue();
        let mut result_tail: SEXP = R_NilValue();

        let mut current = el;
        let mut count: c_int = 0;
        while !current.is_null() && current != R_NilValue() {
            if nargs >= 0 && count >= nargs {
                break;
            }

            let val = Rf_eval(CAR(current), rho);
            let cell = Rf_cons(val, R_NilValue());
            if !cell.is_null() {
                SETTAG(cell, TAG(current));
                if result.is_null() || result == R_NilValue() {
                    result = cell;
                    result_tail = cell;
                } else {
                    SETCDR(result_tail, cell);
                    result_tail = cell;
                }
            }

            current = CDR(current);
            count += 1;
        }

        if result.is_null() {
            result = R_NilValue();
        }
        result
    }
}

// ---------------------------------------------------------------------------
// promiseArgs — create promises for closure arguments
// ---------------------------------------------------------------------------

/// Create promises for each argument, matching to formals.
///
/// This is the equivalent of R's `promiseArgs()` in eval.c.
pub unsafe fn promiseArgs(call: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if call.is_null() || call == R_NilValue() {
            return R_NilValue();
        }

        let mut result: SEXP = R_NilValue();
        let mut result_tail: SEXP = R_NilValue();

        let mut current = call;
        while !current.is_null() && current != R_NilValue() {
            let arg_expr = CAR(current);
            let tag = TAG(current);

            // Create a promise for each argument
            let prom = mkPROMISE(arg_expr, rho);
            let cell = Rf_cons(prom, R_NilValue());
            if !cell.is_null() {
                SETTAG(cell, tag);
                if result.is_null() || result == R_NilValue() {
                    result = cell;
                    result_tail = cell;
                } else {
                    SETCDR(result_tail, cell);
                    result_tail = cell;
                }
            }

            current = CDR(current);
        }

        if result.is_null() {
            result = R_NilValue();
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Helper: evalArgs — evaluate arguments with dropmissing support
// ---------------------------------------------------------------------------

/// Evaluate arguments, optionally dropping missing values.
/// Used by DispatchOrEval when args need to be evaluated before passing
/// to the generic code.
unsafe fn evalArgs(
    args: SEXP,
    rho: SEXP,
    dropmissing: c_int,
    _call: SEXP,
    argsevald: c_int,
) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        let mut result: SEXP = R_NilValue();
        let mut tail: SEXP = R_NilValue();
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            let mut val = R_NilValue();

            if argsevald == 0 {
                val = Rf_eval(arg, rho);
            } else {
                val = arg;
            }

            // Skip missing arguments if dropmissing is set
            if dropmissing != 0 && val == R_MissingArg() {
                current = CDR(current);
                continue;
            }

            let cell = Rf_cons(val, R_NilValue());
            if !result.is_null() && result != R_NilValue() {
                SETCDR(tail, cell);
            } else {
                result = cell;
            }
            tail = cell;
            current = CDR(current);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Helper: isFunction — check if SEXP is a function
// ---------------------------------------------------------------------------

unsafe fn isFunction(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return FALSE;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::CLOSXP
            || t == SEXPTYPE::BUILTINSXP
            || t == SEXPTYPE::SPECIALSXP
        {
            TRUE
        } else {
            FALSE
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: isSymbol — check if SEXP is a symbol
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe fn isSymbol(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return FALSE;
        }
        if TYPEOF(x) == SEXPTYPE::SYMSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: translateChar — get C string from CHARSXP
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe fn translateChar(x: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(x) }
}

// ---------------------------------------------------------------------------
// Helper: streql — compare two C strings
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
unsafe fn streql(a: *const c_char, b: *const c_char) -> c_int {
    unsafe {
        if a.is_null() || b.is_null() {
            return FALSE;
        }
        if libc::strcmp(a, b) == 0 { TRUE } else { FALSE }
    }
}

// ---------------------------------------------------------------------------
// Helper: Rf_strrchr — find last occurrence of char in string
// ---------------------------------------------------------------------------

unsafe fn Rf_strrchr(s: *const c_char, c: c_char) -> *const c_char {
    unsafe {
        if s.is_null() {
            return ptr::null();
        }
        let mut len = 0;
        let mut p = s;
        while *p != 0 {
            p = p.add(1);
            len += 1;
        }
        if len == 0 {
            return ptr::null();
        }
        p = s.add(len as usize);
        while p != s {
            p = p.sub(1);
            if *p == c {
                return p;
            }
        }
        ptr::null()
    }
}

// ---------------------------------------------------------------------------
// Helper: R_mkString — create a length-1 character vector
// ---------------------------------------------------------------------------

unsafe fn R_mkString(s: *const c_char) -> SEXP {
    unsafe {
        if s.is_null() {
            return R_NilValue();
        }
        Rf_mkString(s)
    }
}

// ---------------------------------------------------------------------------
// Helper: stringSuffix — get suffix of character vector starting at pos
// ---------------------------------------------------------------------------

unsafe fn stringSuffix(klass: SEXP, pos: c_int) -> SEXP {
    unsafe {
        if klass.is_null() || pos < 0 {
            return R_NilValue();
        }
        let n = LENGTH(klass);
        if pos >= n {
            return R_NilValue();
        }
        let len = n - pos;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, len);
        Rf_protect(ans);
        for i in 0..len {
            let src = STRING_ELT(klass, (pos + i) as R_xlen_t);
            SET_STRING_ELT(ans, i as R_xlen_t, src);
        }
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// Helper: stringPositionTr — find string in character vector
// ---------------------------------------------------------------------------

unsafe fn stringPositionTr(klass: SEXP, what: *const c_char) -> c_int {
    unsafe {
        if klass.is_null() || what.is_null() {
            return -1;
        }
        let n = LENGTH(klass);
        for i in 0..n {
            let elt = STRING_ELT(klass, i as R_xlen_t);
            if !elt.is_null() {
                let cs = CHAR(elt);
                if !cs.is_null() && libc::strcmp(cs, what) == 0 {
                    return i;
                }
            }
        }
        -1
    }
}

// ---------------------------------------------------------------------------
// Helper: R_data_class — get the class of an object (S3)
// ---------------------------------------------------------------------------

/// Get the data class of an object, equivalent to R's R_data_class2.
unsafe fn R_data_class(obj: SEXP) -> SEXP {
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
        // Fall back to implicit class based on TYPEOF
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
        R_mkString(type_str.as_ptr() as *const c_char)
    }
}

// ---------------------------------------------------------------------------
// Helper: R_BlankScalarString — return a blank scalar string
// ---------------------------------------------------------------------------

unsafe fn R_BlankScalarString_val() -> SEXP {
    unsafe { Rf_mkString(b"\x00".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// R_forceAndCall — force a specific number of promises and call a function
// ---------------------------------------------------------------------------

/// Force the first n promises in an argument list and call a function.
///
/// This is the equivalent of R's `R_forceAndCall()` in eval.c.
pub unsafe fn R_forceAndCall(e: SEXP, op: SEXP, args: SEXP, rho: SEXP, n: c_int) -> SEXP {
    unsafe {
        // Force the first n promises
        let forced_args = args;
        let mut count: c_int = 0;
        let tail: SEXP = ptr::null_mut();

        let mut current = args;
        while !current.is_null() && current != R_NilValue() && count < n {
            let val = CAR(current);
            if TYPEOF(val) == SEXPTYPE::PROMSXP {
                let forced_val = forcePromise(val);
                SETCAR(current, forced_val);
            }
            count += 1;
            current = CDR(current);
        }

        // Call the function
        if TYPEOF(op) == SEXPTYPE::BUILTINSXP {
            // Builtin: pass already-evaluated args
            if let Some(primfun) = super::eval::get_primfun(op) {
                primfun(e, op, args, rho)
            } else {
                R_NilValue()
            }
        } else if TYPEOF(op) == SEXPTYPE::CLOSXP {
            super::closure::applyClosure(e, op, args, rho, R_NilValue(), TRUE)
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// DispatchOrEval — S3/S4 dispatch
// ---------------------------------------------------------------------------

/// Dispatch or evaluate an expression, handling S3/S4 method dispatch.
///
/// This is the equivalent of R's `DispatchOrEval()` in eval.c.
/// Returns 1 if a method was dispatched (result in *ans), 0 if not
/// (evaluated args in *ans).
pub unsafe fn DispatchOrEval(
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
        let mut x: SEXP = R_NilValue();
        let mut dots: c_int = FALSE;
        let mut nprotect: c_int = 0;

        if generic.is_null() || ans.is_null() {
            return 0;
        }

        // Step 1: Find the object to dispatch on
        if argsevald != 0 {
            // Args are already evaluated
            x = CAR(args);
            if !x.is_null() {
                Rf_protect(x);
                nprotect += 1;
            }
        } else {
            // Find the object, dropping leading ... with missing/empty values
            let mut args_iter = args;
            while !args_iter.is_null() && args_iter != R_NilValue() {
                if CAR(args_iter) == R_DotsSymbol() {
                    let h = R_findVar(R_DotsSymbol(), rho);
                    if TYPEOF(h) == SEXPTYPE::DOTSXP {
                        dots = TRUE;
                        x = Rf_eval(CAR(h), rho);
                        break;
                    } else if h != R_NilValue() && h != R_MissingArg() {
                        // '...' used in incorrect context — skip
                        args_iter = CDR(args_iter);
                        continue;
                    }
                } else {
                    dots = FALSE;
                    x = Rf_eval(CAR(args_iter), rho);
                    break;
                }
                args_iter = CDR(args_iter);
            }
            if !x.is_null() {
                Rf_protect(x);
                nprotect += 1;
            }
        }

        // Step 2: Try to dispatch if x is an object
        if !x.is_null() && x != R_NilValue() && isObject(x) != FALSE {
            // Check if the generic name ends with ".default" — if so, no dispatch
            let mut pt: *const c_char = ptr::null();
            if isSymbol(CAR(call)) != FALSE {
                let pname = PRINTNAME(CAR(call));
                if !pname.is_null() {
                    let cs = CHAR(pname);
                    if !cs.is_null() {
                        pt = Rf_strrchr(cs, '.' as c_char);
                    }
                }
            }

            // Only dispatch if not already the default method
            if pt.is_null() || streql(pt, b".default\x00".as_ptr() as *const c_char) == FALSE {
                // Create promises for the arguments
                let pargs = Rf_protect(promiseArgs(args, rho));
                nprotect += 1;

                // Create a new environment for dispatch context
                let rho1 = Rf_protect(NewEnvironment(R_NilValue(), R_NilValue(), rho));
                nprotect += 1;

                // Set the evaluated value as the first promise's value
                // (IF_PROMSXP_SET_PRVALUE)
                if !pargs.is_null()
                    && pargs != R_NilValue()
                    && TYPEOF(CAR(pargs)) == SEXPTYPE::PROMSXP
                {
                    // Force the first promise to be x
                    SETCAR(pargs, x);
                }

                // Try to dispatch via usemethod
                let dispatched = crate::mainutils::objects::usemethod(
                    generic,
                    x,
                    call,
                    pargs,
                    rho1,
                    rho,
                    R_BaseEnv(),
                    ans,
                );

                if dispatched != FALSE {
                    Rf_unprotect(nprotect);
                    return 1;
                }
            }
        }

        // Step 3: No dispatch — evaluate arguments and return them
        if argsevald == 0 {
            if dots != FALSE {
                *ans = evalArgs(args, rho, dropmissing, call, 0);
            } else {
                // Put evaluated x back with rest of evaluated args
                let rest = evalArgs(CDR(args), rho, dropmissing, call, 1);
                let arglist = CONS_NR(x, rest);
                SETTAG(arglist, TAG(args));
                *ans = arglist;
            }
        } else {
            *ans = args;
        }

        Rf_unprotect(nprotect);
        0
    }
}

// ---------------------------------------------------------------------------
// findmethod — find an S3 method (for group dispatch)
// ---------------------------------------------------------------------------

/// Find an S3 method by interleaving group and generic method lookups.
///
/// For each class in the hierarchy, first looks for "generic.class",
/// then "group.class". Returns via output parameters.
///
/// `gr` must be protected by the caller after this function returns.
unsafe fn findmethod(
    class: SEXP,
    group: *const c_char,
    generic: *const c_char,
    sxp: *mut SEXP,
    gr: *mut SEXP,
    meth: *mut SEXP,
    which: *mut c_int,
    _objSlot: SEXP,
    rho: SEXP,
) {
    unsafe {
        if class.is_null() || class == R_NilValue() {
            *sxp = R_NilValue();
            *gr = R_NilValue();
            *meth = R_NilValue();
            *which = 0;
            return;
        }

        let len = LENGTH(class);
        let _vmax = vmaxget();
        let mut whichclass: c_int = 0;

        // Interleave: for each class, try generic then group
        for wc in 0..len {
            whichclass = wc;
            let ss = translateChar(STRING_ELT(class, wc as R_xlen_t));
            if ss.is_null() {
                continue;
            }

            // Try generic.class
            let m = crate::mainutils::names::installS3Signature(generic, ss);
            *meth = m;
            let val = crate::mainutils::objects::R_LookupMethod(m, rho, rho, R_BaseEnv());
            *sxp = val;
            if isFunction(val) != FALSE {
                *gr = R_BlankScalarString_val();
                break;
            }

            // Try group.class
            let mg = crate::mainutils::names::installS3Signature(group, ss);
            *meth = mg;
            let valg = crate::mainutils::objects::R_LookupMethod(mg, rho, rho, R_BaseEnv());
            *sxp = valg;
            if isFunction(valg) != FALSE {
                *gr = R_mkString(group);
                break;
            }
        }

        vmaxset(_vmax);
        *which = whichclass;
    }
}

// ---------------------------------------------------------------------------
// DispatchGroup — group dispatch for Math/Summary/Ops/Complex
// ---------------------------------------------------------------------------

/// Dispatch to a group generic method.
///
/// This is the equivalent of R's `DispatchGroup()` in eval.c.
/// Returns 1 if dispatched (result in *ans), 0 if not dispatched.
pub unsafe fn DispatchGroup(
    group: *const c_char,
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
    ans: *mut SEXP,
) -> c_int {
    unsafe {
        if args.is_null() || args == R_NilValue() || ans.is_null() {
            return 0;
        }

        // Pre-test: skip if first arg isn't an object and there's no second arg
        // that's an object either
        if !isObject(CAR(args)) != FALSE
            && (CDR(args).is_null() || CDR(args) == R_NilValue() || !isObject(CADR(args)) != FALSE)
        {
            return 0;
        }

        // Check if we're already processing the default method
        if isSymbol(CAR(call)) != FALSE {
            let pname = PRINTNAME(CAR(call));
            if !pname.is_null() {
                let cs = CHAR(pname);
                if !cs.is_null() {
                    let dot = libc::strchr(cs, '.' as c_int);
                    if !dot.is_null() {
                        let after_dot = dot.add(1);
                        if streql(after_dot, b"default\x00".as_ptr() as *const c_char) != FALSE {
                            return 0;
                        }
                    }
                }
            }
        }

        // For Ops group, check both args; for others, only the first
        let is_ops = streql(group, b"Ops\x00".as_ptr() as *const c_char) != FALSE
            || streql(group, b"matrixOps\x00".as_ptr() as *const c_char) != FALSE;
        let nargs: c_int = if is_ops { LENGTH(args) } else { 1 };

        if nargs == 1 && !isObject(CAR(args)) != FALSE {
            return 0;
        }

        // Get generic name from op
        let generic = PRIMNAME(op);

        // Get class of first arg
        let mut lclass = Rf_protect(R_data_class(CAR(args)));
        let rclass = if nargs == 2 {
            Rf_protect(R_data_class(CADR(args)))
        } else {
            R_NilValue()
        };
        let mut nprotect: c_int = 2;

        let mut lmeth: SEXP = R_NilValue();
        let mut lsxp: SEXP = R_NilValue();
        let mut lgr: SEXP = R_NilValue();
        let mut rmeth: SEXP = R_NilValue();
        let mut rsxp: SEXP = R_NilValue();
        let mut rgr: SEXP = R_NilValue();
        let mut lwhich: c_int = 0;
        let mut rwhich: c_int = 0;

        findmethod(
            lclass,
            group,
            generic.as_ptr() as *const c_char,
            &mut lsxp,
            &mut lgr,
            &mut lmeth,
            &mut lwhich,
            args,
            rho,
        );
        Rf_protect(lgr);
        nprotect += 1;

        if nargs == 2 {
            findmethod(
                rclass,
                group,
                generic.as_ptr() as *const c_char,
                &mut rsxp,
                &mut rgr,
                &mut rmeth,
                &mut rwhich,
                CDR(args),
                rho,
            );
            Rf_protect(rgr);
            nprotect += 1;
        }

        // If no method found for either side, use default
        if isFunction(lsxp) == FALSE && isFunction(rsxp) == FALSE {
            Rf_unprotect(nprotect);
            return 0;
        }

        // For Ops with two different methods, prefer the left one
        if lsxp != rsxp {
            if isFunction(lsxp) != FALSE && isFunction(rsxp) != FALSE {
                // Both have methods — for now prefer left (simplified;
                // full R would call R_chooseOpsMethod)
            }
            // If left side has no method, use right
            if isFunction(lsxp) == FALSE {
                lsxp = rsxp;
                lmeth = rmeth;
                lgr = rgr;
                lclass = rclass;
                lwhich = rwhich;
            }
        }

        // Build the method vector for each argument
        let dispatch_class_name = translateChar(STRING_ELT(lclass, lwhich as R_xlen_t));
        let _vmax = vmaxget();

        let m = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, nargs));
        nprotect += 1;

        let mut s = args;
        for i in 0..nargs {
            let t = R_data_class(CAR(s));
            if !t.is_null()
                && TYPEOF(t) == SEXPTYPE::STRSXP
                && stringPositionTr(t, dispatch_class_name) >= 0
            {
                SET_STRING_ELT(m, i as R_xlen_t, PRINTNAME(lmeth));
            } else {
                SET_STRING_ELT(m, i as R_xlen_t, R_BlankScalarString_val());
            }
            s = CDR(s);
        }
        vmaxset(_vmax);

        // Create the S3 dispatch variables
        let generic_str = R_mkString(generic.as_ptr() as *const c_char);
        Rf_protect(generic_str);
        nprotect += 1;

        let dot_class = Rf_protect(stringSuffix(lclass, lwhich));
        nprotect += 1;

        let newvars = Rf_protect(crate::mainutils::objects::createS3Vars(
            generic_str,
            lgr,
            dot_class,
            m,
            rho,
            R_BaseEnv(),
        ));
        nprotect += 1;

        // Build the new call: (method . rest-of-call)
        let newcall = Rf_protect(Rf_cons(lmeth, CDR(call)));
        nprotect += 1;

        // Create promises for the arguments
        let pargs = Rf_protect(promiseArgs(CDR(call), rho));
        nprotect += 1;

        // Set promise values to the evaluated args
        let mut pi = pargs;
        let mut ai = args;
        while !pi.is_null() && pi != R_NilValue() && !ai.is_null() && ai != R_NilValue() {
            if TYPEOF(CAR(pi)) == SEXPTYPE::PROMSXP {
                SETCAR(pi, CAR(ai));
            }
            if is_ops {
                SETTAG(pi, R_NilValue());
            }
            pi = CDR(pi);
            ai = CDR(ai);
        }

        // Dispatch via applyClosure
        *ans = applyClosure(newcall, lsxp, pargs, rho, newvars, TRUE);

        Rf_unprotect(nprotect);
        1
    }
}

// ---------------------------------------------------------------------------
// evalListKeepMissing — evaluate pairlist preserving R_MissingArg
// ---------------------------------------------------------------------------

/// Evaluate each element of a pairlist, but preserve `R_MissingArg` arguments
/// rather than erroring on them.
///
/// Ported from R's `evalListKeepMissing()` in eval.c.
/// Iterative (not recursive) to avoid protection stack growth.
pub unsafe fn evalListKeepMissing(el: SEXP, rho: SEXP) -> SEXP {
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::symbol::R_DotsSymbol;

    unsafe {
        let mut head: SEXP = R_NilValue();
        let mut tail: SEXP = ptr::null_mut();

        let mut remaining = el;
        while !remaining.is_null() && remaining != R_NilValue() {
            let mut val: SEXP;

            if CAR(remaining) == R_DotsSymbol() {
                // Handle ... expansion
                let h = R_findVarInFrame(rho, CAR(remaining));
                Rf_protect(h);
                if TYPEOF(h) == SEXPTYPE::DOTSXP || h == R_NilValue() {
                    let mut dh = h;
                    while !dh.is_null() && dh != R_NilValue() {
                        if CAR(dh) == R_MissingArg() {
                            val = R_MissingArg();
                        } else {
                            val = Rf_eval(CAR(dh), rho);
                        }
                        let ev = CONS_NR(val, R_NilValue());
                        if head == R_NilValue() {
                            Rf_unprotect(1); // h
                            head = ev;
                            Rf_protect(head);
                            Rf_protect(dh); // re-protect h
                        } else {
                            SETCDR(tail, ev);
                        }
                        // Copy tag from dots element
                        if TAG(dh) != R_NilValue() {
                            SETTAG(ev, TAG(dh));
                        }
                        tail = ev;
                        dh = CDR(dh);
                    }
                } else if h != R_MissingArg() {
                    Rf_unprotect(1);
                    crate::mainutils::errors::Rf_error(
                        b"'...' used in an incorrect context\0".as_ptr() as *const c_char,
                    );
                }
                Rf_unprotect(1); // h
            } else {
                // Regular argument
                if CAR(remaining) == R_MissingArg()
                    || (TYPEOF(CAR(remaining)) == SEXPTYPE::SYMSXP
                        && R_isMissing(CAR(remaining), rho) != 0)
                {
                    val = R_MissingArg();
                } else {
                    val = Rf_eval(CAR(remaining), rho);
                }
                let ev = CONS_NR(val, R_NilValue());
                if head == R_NilValue() {
                    head = ev;
                    Rf_protect(head);
                } else {
                    SETCDR(tail, ev);
                }
                // Copy tag from original element
                if TAG(remaining) != R_NilValue() {
                    SETTAG(ev, TAG(remaining));
                }
                tail = ev;
            }
            remaining = CDR(remaining);
        }

        if head != R_NilValue() {
            Rf_unprotect(1);
        }

        head
    }
}
