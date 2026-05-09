#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::c_int;
use std::ptr;

use crate::mainutils::coerce::asInteger;
use crate::mainutils::duplicate::{duplicate, shallow_duplicate};
use crate::mainutils::relop::{PRIMVAL, checkArity};
use crate::sexp::accessors::{CAR, CDR, INTEGER, SETCAR};
use crate::sexp::constructors::Rf_cons;
use crate::sexp::constructors::{Rf_allocVector, Rf_length};
use crate::sexp::context::{R_GlobalContext_in, RCNTXT, ctxt_flags};
use crate::sexp::ffi::NA_INTEGER;
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::globals::R_GlobalEnv_in;
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::{RInstance, with_required_current_instance};

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
        if c.is_null() {
            return 0;
        }
        while !c.is_null() {
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
    with_required_current_instance(|instance| unsafe { R_sysframe_in(instance, n, cptr) })
}

pub unsafe fn R_sysframe_in(instance: &mut RInstance, n: c_int, cptr: *mut RCNTXT) -> SEXP {
    unsafe {
        if n == 0 {
            return R_GlobalEnv_in(instance);
        }
        if n == NA_INTEGER {
            error("NA argument is invalid");
        }

        let cptr = context_or_top_in(instance, cptr);
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
        while !c.is_null() {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                if n == 0 {
                    return (*c).cloenv;
                }
                n -= 1;
            }
            c = (*c).nextcontext;
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
        while !c.is_null() {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                if n == 0 {
                    return shallow_duplicate((*c).call);
                }
                n -= 1;
            }
            c = (*c).nextcontext;
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
        while !c.is_null() {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                if n == 0 {
                    return duplicate((*c).callfun);
                }
                n -= 1;
            }
            c = (*c).nextcontext;
        }
        error("not that many frames on the stack");
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_sysparent — get sysparent frame number (S-compatible semantics)
// ---------------------------------------------------------------------------

pub unsafe fn R_sysparent(n: c_int, cptr: *mut RCNTXT) -> c_int {
    with_required_current_instance(|instance| unsafe { R_sysparent_in(instance, n, cptr) })
}

pub unsafe fn R_sysparent_in(instance: &mut RInstance, n: c_int, cptr: *mut RCNTXT) -> c_int {
    unsafe {
        if n <= 0 {
            error("only positive values of 'n' are allowed");
        }
        let mut c = context_or_top_in(instance, cptr);
        if c.is_null() {
            return 0;
        }
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
        if s == R_GlobalEnv_in(instance) {
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
    with_required_current_instance(|instance| unsafe {
        countContexts_in(instance, ctxttype, browser)
    })
}

pub unsafe fn countContexts_in(instance: &mut RInstance, ctxttype: c_int, browser: c_int) -> c_int {
    unsafe {
        let mut n: c_int = 0;
        let mut c = R_GlobalContext_in(instance);
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
        if c.is_null() {
            return ptr::null_mut();
        }
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
        if c.is_null() {
            return ptr::null_mut();
        }
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
    with_required_current_instance(|instance| unsafe { getLexicalContext_in(instance, rho) })
}

pub unsafe fn getLexicalContext_in(instance: &mut RInstance, rho: SEXP) -> *mut RCNTXT {
    unsafe {
        let mut c = R_GlobalContext_in(instance);
        if c.is_null() {
            return ptr::null_mut();
        }
        while !(*c).nextcontext.is_null() {
            if ((*c).callflag & ctxt_flags::CTXT_FUNCTION) != 0 && (*c).cloenv == rho {
                return c;
            }
            c = (*c).nextcontext;
        }
        R_GlobalContext_in(instance)
    }
}

// ---------------------------------------------------------------------------
// do_sys — central dispatcher for sys.parent/call/frame/nframe/calls/frames/on.exit/parents/function
// ---------------------------------------------------------------------------

pub unsafe fn do_sys(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    with_required_current_instance(|instance| unsafe { do_sys_in(instance, call, op, args, rho) })
}

pub unsafe fn do_sys_in(
    instance: &mut RInstance,
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        checkArity(op, args);

        let top = R_GlobalContext_in(instance);
        if top.is_null() {
            return R_NilValue();
        }
        let t = (*top).sysparent;
        let cptr = getLexicalContext_in(instance, t);
        if cptr.is_null() {
            return R_NilValue();
        }

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
                    i = R_sysparent_in(instance, nframe - i + 1, cptr);
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
                R_sysframe_in(instance, n, cptr)
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
                    SETCAR(t, R_sysframe_in(instance, i, cptr));
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
                    *INTEGER(rval).add(i as usize) = R_sysparent_in(instance, nframe - i, cptr);
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
    with_required_current_instance(|instance| unsafe {
        do_parentframe_in(instance, call, op, args, rho)
    })
}

pub unsafe fn do_parentframe_in(
    instance: &mut RInstance,
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        checkArity(op, args);
        let n = asInteger(CAR(args));
        if n == NA_INTEGER || n < 1 {
            error("invalid 'n' value");
        }
        let top = R_GlobalContext_in(instance);
        if top.is_null() {
            return R_GlobalEnv_in(instance);
        }
        let cptr = R_findParentContext(top, n);
        if !cptr.is_null() {
            (*cptr).sysparent
        } else {
            R_GlobalEnv_in(instance)
        }
    }
}

// ---------------------------------------------------------------------------
// do_sysbrowser — browser context queries
// ---------------------------------------------------------------------------

pub unsafe fn do_sysbrowser(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    with_required_current_instance(|instance| unsafe {
        do_sysbrowser_in(instance, call, op, args, rho)
    })
}

pub unsafe fn do_browser(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

pub unsafe fn do_sysbrowser_in(
    instance: &mut RInstance,
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        checkArity(op, args);
        let n = asInteger(CAR(args));
        if n < 1 {
            error("number of contexts must be positive");
        }

        let mut cptr = R_GlobalContext_in(instance);
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
// R_run_onexits — run on.exit handlers
// ---------------------------------------------------------------------------

pub(crate) unsafe fn R_run_onexits_for_context(cptr: *mut RCNTXT) {
    unsafe {
        if cptr.is_null() {
            return;
        }
        let conexit = (*cptr).conexit;
        if isNull(conexit) {
            return;
        }
        (*cptr).conexit = R_NilValue();

        let rho = (*cptr).cloenv;
        let mut current = conexit;
        while !isNull(current) {
            let expr = CAR(current);
            if !isNull(expr) {
                let _ = super::eval::Rf_eval(expr, rho);
            }
            current = CDR(current);
        }
    }
}

pub fn R_run_onexits() {
    with_required_current_instance(|instance| unsafe { R_run_onexits_in(instance) });
}

pub unsafe fn R_run_onexits_in(instance: &mut RInstance) {
    unsafe {
        R_run_onexits_for_context(R_GlobalContext_in(instance));
    }
}

// ---------------------------------------------------------------------------
// eval_CleanUp — cleanup on error or normal exit
// ---------------------------------------------------------------------------

pub fn eval_CleanUp(_sa: c_int, _status: c_int, _RunLast: c_int) {
    R_run_onexits();
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

pub fn R_jump_to_top() {
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
    with_required_current_instance(|instance| unsafe { R_GetCurrentEnv_in(instance) })
}

pub unsafe fn R_GetCurrentEnv_in(instance: &mut RInstance) -> SEXP {
    unsafe {
        let mut c = R_GlobalContext_in(instance);
        while !c.is_null() {
            if (*c).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                return (*c).cloenv;
            }
            c = (*c).nextcontext;
        }
        R_GlobalEnv_in(instance)
    }
}

unsafe fn context_or_top_in(instance: &mut RInstance, cptr: *mut RCNTXT) -> *mut RCNTXT {
    if cptr.is_null() {
        unsafe { R_GlobalContext_in(instance) }
    } else {
        cptr
    }
}
