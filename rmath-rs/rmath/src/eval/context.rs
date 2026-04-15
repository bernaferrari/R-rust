#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::c_int;
use std::ptr;

use crate::mainutils::coerce::asInteger;
use crate::mainutils::duplicate::{duplicate, shallow_duplicate};
use crate::mainutils::relop::{PRIMVAL, checkArity};
use crate::sexp::accessors::{CAR, CDR, INTEGER, SETCAR};
use crate::sexp::constructors::Rf_cons;
use crate::sexp::constructors::{Rf_allocVector, Rf_length};
use crate::sexp::context::{R_GlobalContext, RCNTXT, ctxt_flags};
use crate::sexp::ffi::NA_INTEGER;
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::globals::{R_GlobalEnv, R_NilValue};

// ---------------------------------------------------------------------------
// Local error helper
// ---------------------------------------------------------------------------

unsafe fn error(msg: &str) {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

unsafe fn isNull(x: SEXP) -> bool {
    unsafe { crate::sexp::accessors::Rf_isNull(x) != 0 }
}

// ---------------------------------------------------------------------------
// framedepth — count function contexts on the stack
// ---------------------------------------------------------------------------

pub unsafe fn framedepth(cptr: *mut RCNTXT) -> c_int {
    unsafe {
        let mut nframe: c_int = 0;
        let mut c = cptr;
        while !(*c).nextcontext.is_null() {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                nframe += 1;
            }
            c = (*c).nextcontext;
        }
        nframe
    }
}

// ---------------------------------------------------------------------------
// R_sysframe — get environment of nth function context
// ---------------------------------------------------------------------------

pub unsafe fn R_sysframe(n: c_int, cptr: *mut RCNTXT) -> SEXP {
    unsafe {
        if n == 0 {
            return R_GlobalEnv();
        }
        if n == NA_INTEGER {
            error("NA argument is invalid");
        }

        let mut n = n;
        if n > 0 {
            n = framedepth(cptr) - n;
        } else {
            n = -n;
        }
        if n < 0 {
            error("not that many frames on the stack");
        }

        let mut c = cptr;
        while !(*c).nextcontext.is_null() {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                if n == 0 {
                    return (*c).cloenv;
                }
                n -= 1;
            }
            c = (*c).nextcontext;
        }
        if n == 0 && (*c).nextcontext.is_null() {
            return R_GlobalEnv();
        }
        error("not that many frames on the stack");
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_syscall — get call of nth function context
// ---------------------------------------------------------------------------

pub unsafe fn R_syscall(n: c_int, cptr: *mut RCNTXT) -> SEXP {
    unsafe {
        let mut n = n;
        if n > 0 {
            n = framedepth(cptr) - n;
        } else {
            n = -n;
        }
        if n < 0 {
            error("not that many frames on the stack");
        }
        let mut c = cptr;
        while !(*c).nextcontext.is_null() {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                if n == 0 {
                    return shallow_duplicate((*c).call);
                }
                n -= 1;
            }
            c = (*c).nextcontext;
        }
        if n == 0 && (*c).nextcontext.is_null() {
            return shallow_duplicate((*c).call);
        }
        error("not that many frames on the stack");
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_sysfunction — get function of nth function context
// ---------------------------------------------------------------------------

pub unsafe fn R_sysfunction(n: c_int, cptr: *mut RCNTXT) -> SEXP {
    unsafe {
        let mut n = n;
        if n > 0 {
            n = framedepth(cptr) - n;
        } else {
            n = -n;
        }
        if n < 0 {
            error("not that many frames on the stack");
        }
        let mut c = cptr;
        while !(*c).nextcontext.is_null() {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                if n == 0 {
                    return duplicate((*c).callfun);
                }
                n -= 1;
            }
            c = (*c).nextcontext;
        }
        if n == 0 && (*c).nextcontext.is_null() {
            return duplicate((*c).callfun);
        }
        error("not that many frames on the stack");
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_sysparent — get sysparent frame number (S-compatible semantics)
// ---------------------------------------------------------------------------

pub unsafe fn R_sysparent(n: c_int, cptr: *mut RCNTXT) -> c_int {
    unsafe {
        if n <= 0 {
            error("only positive values of 'n' are allowed");
        }
        let mut c = cptr;
        let mut n = n;
        while !(*c).nextcontext.is_null() && n > 1 {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                n -= 1;
            }
            c = (*c).nextcontext;
        }
        while !(*c).nextcontext.is_null() && (*c).callflag & ctxt_flags::CTXT_FUNCTION == 0 {
            c = (*c).nextcontext;
        }
        let s = (*c).sysparent;
        if s == R_GlobalEnv() {
            return 0;
        }
        let mut j: c_int = 0;
        let mut target_n: c_int = 0;
        let mut c2 = cptr;
        while !c2.is_null() {
            if (*c2).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                j += 1;
                if (*c2).cloenv == s {
                    target_n = j;
                }
            }
            c2 = (*c2).nextcontext;
        }
        let result = j - target_n + 1;
        if result < 0 { 0 } else { result }
    }
}

// ---------------------------------------------------------------------------
// countContexts — count contexts of a given type
// ---------------------------------------------------------------------------

pub unsafe fn countContexts(ctxttype: c_int, browser: c_int) -> c_int {
    unsafe {
        let mut n: c_int = 0;
        let mut c = R_GlobalContext();
        while !c.is_null() {
            if (*c).callflag == ctxttype
                || (browser != 0 && (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0)
            {
                n += 1;
            }
            c = (*c).nextcontext;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// R_findExecContext — find context with matching cloenv
// ---------------------------------------------------------------------------

pub unsafe fn R_findExecContext(cptr: *mut RCNTXT, envir: SEXP) -> *mut RCNTXT {
    unsafe {
        let mut c = cptr;
        while !(*c).nextcontext.is_null() {
            if ((*c).callflag & ctxt_flags::CTXT_FUNCTION) != 0 && (*c).cloenv == envir {
                return c;
            }
            c = (*c).nextcontext;
        }
        ptr::null_mut()
    }
}

// ---------------------------------------------------------------------------
// R_findParentContext — find parent function context
// ---------------------------------------------------------------------------

pub unsafe fn R_findParentContext(cptr: *mut RCNTXT, mut n: c_int) -> *mut RCNTXT {
    unsafe {
        let mut c = cptr;
        loop {
            c = R_findExecContext(c, (*c).sysparent);
            if c.is_null() {
                return ptr::null_mut();
            }
            if n == 1 {
                return c;
            }
            n -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// getLexicalContext — find first CTXT_FUNCTION context matching env
// ---------------------------------------------------------------------------

pub unsafe fn getLexicalContext(rho: SEXP) -> *mut RCNTXT {
    unsafe {
        let mut c = R_GlobalContext();
        while !(*c).nextcontext.is_null() {
            if ((*c).callflag & ctxt_flags::CTXT_FUNCTION) != 0 && (*c).cloenv == rho {
                return c;
            }
            c = (*c).nextcontext;
        }
        R_GlobalContext()
    }
}

// ---------------------------------------------------------------------------
// do_sys — central dispatcher for sys.parent/call/frame/nframe/calls/frames/on.exit/parents/function
// ---------------------------------------------------------------------------

pub unsafe fn do_sys(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let t = (*R_GlobalContext()).sysparent;
        let cptr = getLexicalContext(t);

        let mut n: c_int = -1;
        if Rf_length(args) == 1 {
            n = asInteger(CAR(args));
        }

        let primval = PRIMVAL(op);
        match primval {
            1 => {
                // sys.parent
                if n == NA_INTEGER {
                    error("invalid 'n' argument");
                }
                let nframe = framedepth(cptr);
                let mut i = nframe;
                let mut count = n;
                while count > 0 {
                    i = R_sysparent(nframe - i + 1, cptr);
                    count -= 1;
                }
                crate::sexp::constructors::Rf_ScalarInteger(i)
            }
            2 => {
                // sys.call
                if n == NA_INTEGER {
                    error("invalid 'which' argument");
                }
                R_syscall(n, cptr)
            }
            3 => {
                // sys.frame
                if n == NA_INTEGER {
                    error("invalid 'which' argument");
                }
                R_sysframe(n, cptr)
            }
            4 => {
                // sys.nframe
                crate::sexp::constructors::Rf_ScalarInteger(framedepth(cptr))
            }
            5 => {
                // sys.calls
                let nframe = framedepth(cptr);
                let rval = crate::sexp::constructors::Rf_allocList(nframe);
                let mut t = rval;
                for i in 1..=nframe {
                    SETCAR(t, R_syscall(i, cptr));
                    t = CDR(t);
                }
                rval
            }
            6 => {
                // sys.frames
                let nframe = framedepth(cptr);
                let rval = crate::sexp::constructors::Rf_allocList(nframe);
                let mut t = rval;
                for i in 1..=nframe {
                    SETCAR(t, R_sysframe(i, cptr));
                    t = CDR(t);
                }
                rval
            }
            7 => {
                // sys.on.exit
                let conexit = (*cptr).conexit;
                if isNull(conexit) {
                    R_NilValue()
                } else if isNull(CDR(conexit)) {
                    CAR(conexit)
                } else {
                    Rf_cons(crate::sexp::symbol::R_BraceSymbol(), conexit)
                }
            }
            8 => {
                // sys.parents
                let nframe = framedepth(cptr);
                let rval = Rf_allocVector(SEXPTYPE::INTSXP, nframe);
                for i in 0..nframe {
                    *INTEGER(rval).add(i as usize) = R_sysparent(nframe - i, cptr);
                }
                rval
            }
            9 => {
                // sys.function
                if n == NA_INTEGER {
                    error("invalid 'which' value");
                }
                R_sysfunction(n, cptr)
            }
            _ => {
                error("internal error in 'do_sys'");
                R_NilValue()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// do_parentframe — parent.frame()
// ---------------------------------------------------------------------------

pub unsafe fn do_parentframe(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let n = asInteger(CAR(args));
        if n == NA_INTEGER || n < 1 {
            error("invalid 'n' value");
        }
        let cptr = R_findParentContext(R_GlobalContext(), n);
        if !cptr.is_null() {
            (*cptr).sysparent
        } else {
            R_GlobalEnv()
        }
    }
}

// ---------------------------------------------------------------------------
// do_sysbrowser — browser context queries
// ---------------------------------------------------------------------------

pub unsafe fn do_sysbrowser(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let n = asInteger(CAR(args));
        if n < 1 {
            error("number of contexts must be positive");
        }

        let mut cptr = R_GlobalContext();
        while !cptr.is_null() {
            if (*cptr).callflag == ctxt_flags::CTXT_BROWSER {
                break;
            }
            cptr = (*cptr).nextcontext;
        }

        if cptr.is_null() || (*cptr).callflag != ctxt_flags::CTXT_BROWSER {
            error("no browser context to query");
        }

        // Simplified: return nil for browser queries in embedded mode
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_run_onexits — run on.exit handlers (stub — full impl needs eval)
// ---------------------------------------------------------------------------

pub unsafe fn R_run_onexits() {}

// ---------------------------------------------------------------------------
// eval_CleanUp — cleanup on error or normal exit
// ---------------------------------------------------------------------------

pub unsafe fn eval_CleanUp(_sa: c_int, _status: c_int, _RunLast: c_int) {
    unsafe {
        R_run_onexits();
    }
}

// ---------------------------------------------------------------------------
// R_jumpctxt — jump to a specific context (panic-based)
// ---------------------------------------------------------------------------

pub unsafe fn R_jumpctxt(_ctxt: *mut RCNTXT, _retval: c_int) {
    std::panic::panic_any(crate::sexp::context::RError {
        message: "jump_to_context".to_string(),
    });
}

// ---------------------------------------------------------------------------
// R_jump_to_top — jump to the top-level context
// ---------------------------------------------------------------------------

pub unsafe fn R_jump_to_top() {
    std::panic::panic_any(crate::sexp::context::RError {
        message: "jump_to_top".to_string(),
    });
}

// ---------------------------------------------------------------------------
// R_InsertRestartHandlers — manage restart handlers
// ---------------------------------------------------------------------------

pub unsafe fn R_InsertRestartHandlers(_call: SEXP, _rho: SEXP) {}

// ---------------------------------------------------------------------------
// R_GetCurrentEnv — get cloenv of current function context
// ---------------------------------------------------------------------------

pub unsafe fn R_GetCurrentEnv() -> SEXP {
    unsafe {
        let mut c = R_GlobalContext();
        while !c.is_null() {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                return (*c).cloenv;
            }
            c = (*c).nextcontext;
        }
        R_GlobalEnv()
    }
}
