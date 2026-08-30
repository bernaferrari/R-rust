#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Traceback support and source-reference helpers.

use super::*;

// ---------------------------------------------------------------------------
// Traceback support
// ---------------------------------------------------------------------------

/// R_GetTracebackOnly — return traceback without deparsing calls.
/// Ported from errors.c R_GetTracebackOnly().
pub unsafe fn R_GetTracebackOnly(skip: c_int) -> SEXP {
    unsafe {
        let mut nback: c_int = 0;
        let mut ns = skip;

        // First pass: count frames
        let ctx = crate::sexp::context::R_GlobalContext();
        let mut c = ctx;
        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0
            {
                if ns > 0 {
                    ns -= 1;
                } else {
                    nback += 1;
                }
            }
            c = ctx_ref.nextcontext;
        }

        let s = Rf_allocList(nback);
        let mut t = s;
        let mut skip2 = skip;

        // Second pass: fill in the calls
        c = ctx;
        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0
            {
                if skip2 > 0 {
                    skip2 -= 1;
                } else {
                    // SETCAR(t, duplicate(ctx_ref.call));
                    //  set to the call (no deep copy)
                    if !t.is_null() {
                        SETCAR(t, ctx_ref.call);
                    }
                    t = CDR(t);
                }
            }
            c = ctx_ref.nextcontext;
        }

        s
    }
}

/// R_ConciseTraceback — return a concise call chain as a string.
/// Ported from errors.c R_ConciseTraceback().
pub unsafe fn R_ConciseTraceback(call: SEXP, skip: c_int) -> String {
    unsafe {
        let ctx = crate::sexp::context::R_GlobalContext();
        let mut c = ctx;
        let mut buf = String::new();
        let mut ncalls: c_int = 0;
        let mut too_many = false;
        let mut top = "";
        let mut skip_count = skip;

        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0
            {
                if skip_count > 0 {
                    skip_count -= 1;
                } else {
                    // Get function name from call
                    let fun = if !ctx_ref.call.is_null() {
                        CAR(ctx_ref.call)
                    } else {
                        ptr::null_mut()
                    };
                    let this = if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::SYMSXP {
                        let name = CHAR_local(PRINTNAME(fun));
                        CStr::from_ptr(name).to_str().unwrap_or("<Anonymous>")
                    } else {
                        "<Anonymous>"
                    };

                    // Skip internal functions
                    if this == "stop"
                        || this == "warning"
                        || this == "suppressWarnings"
                        || this == ".signalSimpleWarning"
                    {
                        buf.clear();
                        ncalls = 0;
                        too_many = false;
                    } else {
                        ncalls += 1;
                        if too_many {
                            top = this;
                        } else if buf.len() > R_NSHOWCALLS {
                            buf = format!("... {}", buf);
                            too_many = true;
                            top = this;
                        } else if !buf.is_empty() {
                            buf = format!("{} -> {}", this, buf);
                        } else {
                            buf = this.to_string();
                        }
                    }
                }
            }
            c = ctx_ref.nextcontext;
        }

        if too_many && top.len() < 50 {
            buf = format!("{} {}", top, buf);
        }

        buf
    }
}

/// do_traceback — traceback().
pub unsafe fn do_traceback(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let skip = if isInteger(CAR(args)) != 0 && LENGTH(CAR(args)) >= 1 {
            *INTEGER(CAR(args))
        } else {
            crate::sexp::ffi::NA_INTEGER
        };
        if skip == crate::sexp::ffi::NA_INTEGER || skip < 0 {
            errorcall(call, b"invalid 'skip' value\x00".as_ptr() as *const c_char);
        }
        R_GetTracebackOnly(skip)
    }
}

// ---------------------------------------------------------------------------
// R_GetCurrentSrcref (simplified)
// ---------------------------------------------------------------------------

/// R_GetCurrentSrcref — get the current source reference.
pub unsafe fn R_GetCurrentSrcref(skip: c_int) -> SEXP {
    unsafe {
        // Simplified: no source references in Rust port yet
        globals::R_NilValue()
    }
}

/// R_GetSrcFilename — get source filename from a srcref.
pub unsafe fn R_GetSrcFilename(_srcref: SEXP) -> SEXP {
    unsafe { Rf_mkString(b"\x00".as_ptr() as *const c_char) }
}
