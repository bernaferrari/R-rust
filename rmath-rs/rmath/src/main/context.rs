#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/context.c -- context management.
//!
//! Contexts are a linked list of execution contexts used by the evaluator
//! for control flow constructs like "next", "break", "return", error
//! recovery, and on.exit handlers.
//!
//! In C, R uses setjmp/longjmp for non-local exits. In Rust, we use
//! `std::panic::panic_any` with a custom `RError` payload, caught by
//! `catch_unwind` at the appropriate context boundary.
//!
//! The RCNTXT struct and CONTEXT_STACK live in `sexp::context`. This module
//! provides the full set of context management functions ported from context.c.

use std::cell::Cell;
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use crate::sexp::accessors::{CADR, CAR, CDR, SETCAR, TYPEOF};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::context as sexp_context;
use crate::sexp::context::ctxt_flags;
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_NilValue};
use crate::sexp::protect::R_PreserveObject;

// ---------------------------------------------------------------------------
// Missing CTXT_ constants
// ---------------------------------------------------------------------------

/// CTXT_RESTART: a function call to restart was made inside the closure.
pub const CTXT_RESTART: c_int = 4;
/// CTXT_BREAK: target for "break".
pub const CTXT_BREAK: c_int = 8;

// ---------------------------------------------------------------------------
// Global state (thread-local, mirroring R's global variables from Defn.h)
// ---------------------------------------------------------------------------

thread_local! {
    /// R_Expressions -- remaining expression count (for set/restore).
    pub static R_Expressions: Cell<c_int> = const { Cell::new(0) };

    /// R_Expressions_keep -- saved expression count.
    pub static R_Expressions_keep: Cell<c_int> = const { Cell::new(0) };

    /// R_EvalDepth -- current evaluation depth.
    pub static R_EvalDepth: Cell<c_int> = const { Cell::new(0) };

    /// R_ReturnedValue -- value returned by a longjmp.
    pub static R_ReturnedValue: Cell<SEXP> = const { Cell::new(ptr::null_mut()) };

    /// R_ExitContext -- context for running on.exit handlers.
    pub static R_ExitContext: Cell<usize> = const { Cell::new(0) };

    /// R_ToplevelContext -- the top-level context.
    pub static R_ToplevelContext: Cell<usize> = const { Cell::new(0) };

    /// R_HandlerStack -- condition handler stack.
    pub static R_HandlerStack: Cell<SEXP> = const { Cell::new(ptr::null_mut()) };

    /// R_RestartStack -- restart stack.
    pub static R_RestartStack: Cell<SEXP> = const { Cell::new(ptr::null_mut()) };

    /// R_ShowErrorMessages -- whether to show error messages.
    pub static R_ShowErrorMessages: Cell<c_int> = const { Cell::new(1) };

    /// R_CurrentExpr -- current expression being evaluated.
    pub static R_CurrentExpr: Cell<SEXP> = const { Cell::new(ptr::null_mut()) };

    /// R_Srcref -- current source reference.
    pub static R_Srcref: Cell<SEXP> = const { Cell::new(ptr::null_mut()) };

    /// R_GCEnabled -- GC enabled flag.
    pub static R_GCEnabled: Cell<c_int> = const { Cell::new(1) };

    /// R_interrupts_suspended -- interrupts suspended flag.
    pub static R_interrupts_suspended: Cell<c_int> = const { Cell::new(0) };

    /// R_OldCStackLimit -- saved C stack limit during overflow handling.
    pub static R_OldCStackLimit: Cell<usize> = const { Cell::new(0) };
}

// ---------------------------------------------------------------------------
// Accessor helpers for thread-local globals
// ---------------------------------------------------------------------------

#[inline]
pub unsafe fn get_R_ReturnedValue() -> SEXP {
    R_ReturnedValue.with(|v| v.get())
}

#[inline]
pub unsafe fn set_R_ReturnedValue(v: SEXP) {
    R_ReturnedValue.with(|val| val.set(v));
}

#[inline]
pub unsafe fn get_R_HandlerStack() -> SEXP {
    R_HandlerStack.with(|v| v.get())
}

#[inline]
pub unsafe fn set_R_HandlerStack(v: SEXP) {
    R_HandlerStack.with(|s| s.set(v));
}

#[inline]
pub unsafe fn get_R_RestartStack() -> SEXP {
    R_RestartStack.with(|v| v.get())
}

#[inline]
pub unsafe fn set_R_RestartStack(v: SEXP) {
    R_RestartStack.with(|s| s.set(v));
}

#[inline]
pub unsafe fn get_R_ToplevelContext() -> *mut sexp_context::RCNTXT {
    let addr = R_ToplevelContext.with(|v| v.get());
    addr as *mut sexp_context::RCNTXT
}

#[inline]
pub unsafe fn set_R_ToplevelContext(v: *mut sexp_context::RCNTXT) {
    R_ToplevelContext.with(|c| c.set(v as usize));
}

#[inline]
pub unsafe fn get_R_ExitContext() -> *mut sexp_context::RCNTXT {
    let addr = R_ExitContext.with(|v| v.get());
    addr as *mut sexp_context::RCNTXT
}

#[inline]
pub unsafe fn set_R_ExitContext(v: *mut sexp_context::RCNTXT) {
    R_ExitContext.with(|c| c.set(v as usize));
}

// ---------------------------------------------------------------------------
// Helper functions / stubs
// ---------------------------------------------------------------------------

#[inline]
unsafe fn PROTECT(s: SEXP) -> SEXP {
    unsafe { crate::sexp::protect::Rf_protect(s) }
}

#[inline]
unsafe fn UNPROTECT(n: c_int) {
    unsafe {
        crate::sexp::protect::Rf_unprotect(n);
    }
}

#[inline]
unsafe fn vmaxget() -> *mut c_void {
    ptr::null_mut()
}

#[inline]
unsafe fn vmaxset(_vmax: *mut c_void) {}

#[inline]
unsafe fn R_CheckStack() {}

#[inline]
unsafe fn R_BCRelPC(_body: SEXP, _pc: *const c_void) -> isize {
    0
}

#[inline]
unsafe fn R_BCProtReset(_top: usize) {}

#[inline]
pub unsafe fn R_findBCInterpreterSrcref(_cptr: *mut sexp_context::RCNTXT) -> SEXP {
    unsafe { R_NilValue() }
}

thread_local! {
    /// Sentinel for "in BC interpreter". Use thread-local to avoid Sync issues.
    pub static R_InBCInterpreter: Cell<SEXP> = const { Cell::new(0x1 as SEXP) };
}

/// Get the R_InBCInterpreter sentinel value.
pub unsafe fn get_R_InBCInterpreter() -> SEXP {
    R_InBCInterpreter.with(|v| v.get())
}

#[inline]
unsafe fn R_SrcrefSymbol() -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(b"srcref".as_ptr() as *const c_char) }
}

#[inline]
unsafe fn R_BraceSymbol() -> SEXP {
    unsafe { crate::sexp::symbol::R_BraceSymbol() }
}

#[inline]
unsafe fn error(msg: *const c_char) -> ! {
    unsafe {
        let s = std::ffi::CStr::from_ptr(msg);
        std::panic::panic_any(sexp_context::RError {
            message: s.to_string_lossy().into_owned(),
        });
    }
}

#[inline]
unsafe fn errorcall(_call: SEXP, msg: *const c_char) -> ! {
    unsafe {
        error(msg);
    }
}

#[inline]
unsafe fn warning(_msg: *const c_char) {}

#[inline]
unsafe fn R_PendingPromises_set(_prstack: *mut c_void) {}

#[inline]
unsafe fn SET_PRSEEN(_promise: SEXP, _val: c_int) {}

#[inline]
unsafe fn R_FixupExitingHandlerResult(_saveretval: SEXP) {}

#[inline]
unsafe fn R_UnwindHandlerStack(_handlerstack: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[inline]
unsafe fn SET_RDEBUG(_rho: SEXP, _val: c_int) {}

#[inline]
unsafe fn RDEBUG(_rho: SEXP) -> c_int {
    0
}

#[inline]
unsafe fn checkArity(_op: SEXP, _args: SEXP) {}

#[inline]
unsafe fn PRIMVAL(_op: SEXP) -> c_int {
    0
}

#[inline]
unsafe fn asInteger(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() || s == R_NilValue() {
            return crate::sexp::ffi::NA_INTEGER;
        }
        let t = TYPEOF(s);
        if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            let data = crate::sexp::accessors::INTEGER(s);
            if !data.is_null() {
                return *data;
            }
        }
        crate::sexp::ffi::NA_INTEGER
    }
}

#[inline]
unsafe fn allocList(len: c_int) -> SEXP {
    unsafe {
        let mut result = R_NilValue();
        for _ in 0..len {
            result = crate::sexp::constructors::Rf_cons(R_NilValue(), result);
        }
        result
    }
}

#[inline]
unsafe fn allocVector(type_: c_int, len: c_int) -> SEXP {
    unsafe { Rf_allocVector(type_, len) }
}

#[inline]
unsafe fn length(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() || s == R_NilValue() {
            return 0;
        }
        let info = (*s).sxpinfo;
        let t = info.type_of();
        if t.is_vector_type() {
            return crate::sexp::accessors::LENGTH(s);
        }
        // For pairlists, walk the list
        let mut n: c_int = 0;
        let mut x = s;
        while !x.is_null() && x != R_NilValue() {
            n += 1;
            x = CDR(x);
        }
        n
    }
}

#[inline]
unsafe fn isNull(s: SEXP) -> bool {
    unsafe { s.is_null() || s == R_NilValue() }
}

#[inline]
unsafe fn ScalarInteger(val: c_int) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::INTSXP.0, 1);
        if !s.is_null() {
            let data = crate::sexp::accessors::INTEGER(s);
            if !data.is_null() {
                *data = val;
            }
            (*s).sxpinfo.set_scalar(true);
        }
        s
    }
}

#[inline]
unsafe fn shallow_duplicate(s: SEXP) -> SEXP {
    unsafe { crate::main::duplicate::shallow_duplicate(s) }
}

#[inline]
unsafe fn setAttrib(x: SEXP, what: SEXP, val: SEXP) {
    unsafe {
        crate::sexp::attrib_core::setAttrib(x, what, val);
    }
}

#[inline]
unsafe fn duplicate(s: SEXP) -> SEXP {
    unsafe { crate::main::duplicate::Rf_duplicate(s) }
}

#[inline]
unsafe fn CONS(car: SEXP, cdr: SEXP) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_cons(car, cdr) }
}

#[inline]
unsafe fn LCONS(car: SEXP, cdr: SEXP) -> SEXP {
    unsafe { CONS(car, cdr) }
}

#[inline]
unsafe fn eval(expr: SEXP, env: SEXP) -> SEXP {
    unsafe { crate::eval::eval::Rf_eval(expr, env) }
}

pub type Rboolean = c_int;
pub const R_TRUE: c_int = 1;
pub const R_FALSE: c_int = 0;

// ---------------------------------------------------------------------------
// SEXP_TO_STACKVAL
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct SEXP_STACKVAL {
    pub tag: c_int,
    pub u: SEXP_STACKVAL_UNION,
}

#[repr(C)]
pub union SEXP_STACKVAL_UNION {
    pub sxpval: SEXP,
    pub _padding: usize,
}

impl std::default::Default for SEXP_STACKVAL {
    fn default() -> Self {
        SEXP_STACKVAL {
            tag: 0,
            u: SEXP_STACKVAL_UNION { _padding: 0 },
        }
    }
}

#[inline]
pub fn SEXP_TO_STACKVAL(s: SEXP) -> SEXP_STACKVAL {
    SEXP_STACKVAL {
        tag: 0,
        u: SEXP_STACKVAL_UNION { sxpval: s },
    }
}

#[inline]
pub unsafe fn STACKVAL_TO_SEXP(sv: *const SEXP_STACKVAL) -> SEXP {
    unsafe {
        if sv.is_null() {
            return ptr::null_mut();
        }
        if (*sv).tag == 0 {
            (*sv).u.sxpval
        } else {
            ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// R_run_onexits
// ---------------------------------------------------------------------------

/// Run all on.exit handlers from R_GlobalContext down to (not including)
/// the given context pointer.
pub unsafe fn R_run_onexits_impl(cptr: *mut sexp_context::RCNTXT) {
    unsafe {
        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            let mut idx = stack.len();
            while idx > 0 {
                idx -= 1;
                let ctx = &stack[idx];
                let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                if ctx_ptr == cptr {
                    break;
                }
                // Run on.exit handlers
                if !ctx.cloenv.is_null()
                    && ctx.cloenv != R_NilValue()
                    && !ctx.conexit.is_null()
                    && ctx.conexit != R_NilValue()
                {
                    let s = ctx.conexit;
                    let savecontext = get_R_ExitContext();
                    set_R_ExitContext(ctx_ptr);
                    (*ctx_ptr).conexit = R_NilValue();
                    set_R_HandlerStack(ctx.handlerstack);
                    set_R_RestartStack(ctx.restartstack);
                    PROTECT(s);
                    R_Expressions.with(|e| e.set(R_Expressions_keep.with(|k| k.get()) + 500));
                    R_CheckStack();
                    let mut cur = s;
                    while !cur.is_null() && cur != R_NilValue() {
                        let expr = CAR(cur);
                        let next = CDR(cur);
                        (*ctx_ptr).conexit = next;
                        if !expr.is_null() && expr != R_NilValue() {
                            eval(expr, ctx.cloenv);
                        }
                        cur = next;
                    }
                    UNPROTECT(1);
                    set_R_ExitContext(savecontext);
                }
                if get_R_ExitContext() == ctx_ptr {
                    set_R_ExitContext(ptr::null_mut());
                }
            }
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_run_onexits(cptr: *mut sexp_context::RCNTXT) {
    unsafe {
        R_run_onexits_impl(cptr);
    }
}

// ---------------------------------------------------------------------------
// R_restore_globals
// ---------------------------------------------------------------------------

unsafe fn R_restore_globals(cptr: *mut sexp_context::RCNTXT) {
    unsafe {
        R_EvalDepth.with(|d| d.set((*cptr).evaldepth));
        R_Expressions.with(|e| e.set(R_Expressions_keep.with(|k| k.get())));
    }
}

// ---------------------------------------------------------------------------
// first_jump_target
// ---------------------------------------------------------------------------

unsafe fn first_jump_target(
    cptr: *mut sexp_context::RCNTXT,
    _mask: c_int,
) -> *mut sexp_context::RCNTXT {
    unsafe {
        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                if ctx_ptr == cptr {
                    return cptr;
                }
                let has_onexit = !ctx.cloenv.is_null()
                    && ctx.cloenv != R_NilValue()
                    && !ctx.conexit.is_null()
                    && ctx.conexit != R_NilValue();
                if has_onexit {
                    return ctx_ptr;
                }
            }
            cptr
        })
    }
}

// ---------------------------------------------------------------------------
// R_jumpctxt
// ---------------------------------------------------------------------------

pub unsafe fn R_jumpctxt_impl(targetcptr: *mut sexp_context::RCNTXT, mask: c_int, val: SEXP) {
    unsafe {
        let savevis = crate::sexp::globals::R_Visible();

        let cptr = first_jump_target(targetcptr, mask);

        R_run_onexits_impl(cptr);
        crate::sexp::globals::set_R_Visible(savevis);

        set_R_ReturnedValue(val);

        R_OldCStackLimit.with(|old| {
            if old.get() != 0 {
                old.set(0);
            }
        });

        let effective_mask = if mask == 0 { 1 } else { mask };
        std::panic::panic_any(sexp_context::RError {
            message: format!("R_jumpctxt: mask={}", effective_mask),
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_jumpctxt(targetcptr: *mut sexp_context::RCNTXT, mask: c_int, val: SEXP) {
    unsafe {
        R_jumpctxt_impl(targetcptr, mask, val);
    }
}

// ---------------------------------------------------------------------------
// begincontext
// ---------------------------------------------------------------------------

pub unsafe fn begincontext(
    cptr: *mut sexp_context::RCNTXT,
    flags: c_int,
    syscall: SEXP,
    env: SEXP,
    sysp: SEXP,
    promargs: SEXP,
    callfun: SEXP,
) {
    unsafe {
        (*cptr).callflag = flags;
        (*cptr).call = syscall;
        (*cptr).cloenv = env;
        (*cptr).sysparent = sysp as SEXP;
        (*cptr).closure = callfun;
        (*cptr).promiseargs = promargs;
        (*cptr).conexit = R_NilValue();
        (*cptr).evaldepth = R_EvalDepth.with(|d| d.get());
        (*cptr).handlerstack = get_R_HandlerStack();
        (*cptr).restartstack = get_R_RestartStack();
        (*cptr).returnValue = ptr::null_mut();

        let prev = sexp_context::R_GlobalContext();
        (*cptr).nextcontext = prev;

        sexp_context::CONTEXT_STACK.with(|stack| {
            let mut boxed = Box::new(ptr::read(cptr));
            boxed.nextcontext = prev;
            stack.borrow_mut().push(boxed);
        });
    }
}

// ---------------------------------------------------------------------------
// endcontext
// ---------------------------------------------------------------------------

pub unsafe fn endcontext(cptr: *mut sexp_context::RCNTXT) {
    unsafe {
        let handlerstack = (*cptr).handlerstack;
        let _ = R_UnwindHandlerStack(handlerstack);
        set_R_RestartStack((*cptr).restartstack);

        if !(*cptr).cloenv.is_null()
            && (*cptr).cloenv != R_NilValue()
            && !(*cptr).conexit.is_null()
            && (*cptr).conexit != R_NilValue()
        {
            let s = (*cptr).conexit;
            let savevis = crate::sexp::globals::R_Visible();
            let savecontext = get_R_ExitContext();
            let saveretval = get_R_ReturnedValue();

            set_R_ExitContext(cptr);
            (*cptr).conexit = R_NilValue();
            PROTECT(saveretval);
            PROTECT(s);
            R_FixupExitingHandlerResult(saveretval);

            let cptr_retval = (*cptr).returnValue;

            let mut cur = s;
            while !cur.is_null() && cur != R_NilValue() {
                let next = CDR(cur);
                (*cptr).conexit = next;
                eval(CAR(cur), (*cptr).cloenv);
                cur = next;
            }

            let _ = cptr_retval; // suppress unused warning

            set_R_ReturnedValue(saveretval);
            UNPROTECT(2);
            set_R_ExitContext(savecontext);
            crate::sexp::globals::set_R_Visible(savevis);
        }

        if get_R_ExitContext() == cptr {
            set_R_ExitContext(ptr::null_mut());
        }

        sexp_context::CONTEXT_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(top) = stack.last() {
                let top_ptr: *mut sexp_context::RCNTXT = &**top as *const _ as *mut _;
                if top_ptr == cptr {
                    stack.pop();
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// findcontext
// ---------------------------------------------------------------------------

pub unsafe fn findcontext(mask: c_int, env: SEXP, val: SEXP) -> ! {
    unsafe {
        let target = sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            if (mask & ctxt_flags::CTXT_LOOP) != 0 {
                for ctx in stack.iter().rev() {
                    let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                    if (ctx.callflag & ctxt_flags::CTXT_LOOP) != 0 && ctx.cloenv == env {
                        return Some(ctx_ptr);
                    }
                    if ctx.callflag == ctxt_flags::CTXT_TOPLEVEL {
                        break;
                    }
                }
                None
            } else {
                for ctx in stack.iter().rev() {
                    let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                    if (ctx.callflag & mask) != 0 && ctx.cloenv == env {
                        return Some(ctx_ptr);
                    }
                    if ctx.callflag == ctxt_flags::CTXT_TOPLEVEL {
                        break;
                    }
                }
                None
            }
        });

        if let Some(ctx_ptr) = target {
            R_jumpctxt_impl(ctx_ptr, mask, val);
        }

        if (mask & ctxt_flags::CTXT_LOOP) != 0 {
            let msg =
                std::ffi::CString::new("no loop for break/next, jumping to top level").unwrap();
            error(msg.as_ptr());
        } else {
            let msg =
                std::ffi::CString::new("no function to return from, jumping to top level").unwrap();
            error(msg.as_ptr());
        }
    }
}

// ---------------------------------------------------------------------------
// R_JumpToContext
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_JumpToContext(
    target: *mut sexp_context::RCNTXT,
    mask: c_int,
    val: SEXP,
) -> ! {
    unsafe {
        let found = sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                if ctx_ptr == target {
                    return true;
                }
                if ctx_ptr == get_R_ExitContext() {
                    set_R_ExitContext(ptr::null_mut());
                }
                if ctx.callflag == ctxt_flags::CTXT_TOPLEVEL {
                    break;
                }
            }
            false
        });

        if found {
            R_jumpctxt_impl(target, mask, val);
        }

        let msg = std::ffi::CString::new("target context is not on the stack").unwrap();
        error(msg.as_ptr());
    }
}

// ---------------------------------------------------------------------------
// R_sysframe (helper)
// ---------------------------------------------------------------------------

pub unsafe fn R_sysframe(n: c_int, _cptr: *mut sexp_context::RCNTXT) -> SEXP {
    unsafe {
        if n == 0 {
            return R_GlobalEnv();
        }

        if n == crate::sexp::ffi::NA_INTEGER {
            let msg = std::ffi::CString::new("NA argument is invalid").unwrap();
            error(msg.as_ptr());
        }

        let mut n = if n > 0 { ctx_framedepth() - n } else { -n };

        if n < 0 {
            let msg = std::ffi::CString::new("not that many frames on the stack").unwrap();
            error(msg.as_ptr());
        }

        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                let ctx_ref = &**ctx;
                if (ctx_ref.callflag & ctxt_flags::CTXT_FUNCTION) != 0 {
                    if n == 0 {
                        return ctx_ref.cloenv;
                    } else {
                        n -= 1;
                    }
                }
                if ctx_ref.nextcontext.is_null() {
                    if n == 0 {
                        return R_GlobalEnv();
                    } else {
                        let msg =
                            std::ffi::CString::new("not that many frames on the stack").unwrap();
                        error(msg.as_ptr());
                    }
                }
            }
            R_GlobalEnv()
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_sysframe_c(n: c_int, cptr: *mut sexp_context::RCNTXT) -> SEXP {
    unsafe { R_sysframe(n, cptr) }
}

// ---------------------------------------------------------------------------
// R_sysparent (helper)
// ---------------------------------------------------------------------------

pub unsafe fn R_sysparent(n: c_int, _cptr: *mut sexp_context::RCNTXT) -> c_int {
    unsafe {
        if n <= 0 {
            let toplevel = get_R_ToplevelContext();
            let call = if toplevel.is_null() {
                ptr::null_mut()
            } else {
                (*toplevel).call
            };
            let msg = std::ffi::CString::new("only positive values of 'n' are allowed").unwrap();
            errorcall(call, msg.as_ptr());
        }

        let mut n = n;
        let mut j: c_int = 0;

        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            let mut ctx_iter = stack.iter().rev().peekable();

            while n > 1 {
                match ctx_iter.next() {
                    Some(ctx) if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 => {
                        n -= 1;
                    }
                    Some(_) => {}
                    None => break,
                }
            }

            let sysparent = match ctx_iter.peek() {
                Some(ctx) => ctx.sysparent as SEXP,
                None => return 0,
            };

            if sysparent == R_GlobalEnv() {
                return 0;
            }

            for ctx in stack.iter().rev() {
                if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 {
                    j += 1;
                    if ctx.cloenv == sysparent {
                        n = j;
                    }
                }
            }
            j - n + 1
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_sysparent_c(n: c_int, cptr: *mut sexp_context::RCNTXT) -> c_int {
    unsafe { R_sysparent(n, cptr) }
}

// ---------------------------------------------------------------------------
// framedepth (helper)
// ---------------------------------------------------------------------------

pub unsafe fn ctx_framedepth() -> c_int {
    let mut nframe: c_int = 0;
    sexp_context::CONTEXT_STACK.with(|stack| {
        let stack = stack.borrow();
        for ctx in stack.iter().rev() {
            if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 {
                nframe += 1;
            }
            if ctx.nextcontext.is_null() {
                break;
            }
        }
    });
    nframe
}

pub unsafe fn framedepth(_cptr: *mut sexp_context::RCNTXT) -> c_int {
    unsafe { ctx_framedepth() }
}

pub unsafe fn framedepth_c(cptr: *mut sexp_context::RCNTXT) -> c_int {
    unsafe { framedepth(cptr) }
}

// ---------------------------------------------------------------------------
// getCallWithSrcref
// ---------------------------------------------------------------------------

unsafe fn getCallWithSrcref(cptr: *mut sexp_context::RCNTXT) -> SEXP {
    unsafe {
        let result = shallow_duplicate((*cptr).call);
        PROTECT(result);
        let srcref_ptr = R_Srcref.with(|s| s.get());
        if !srcref_ptr.is_null() && !isNull(srcref_ptr) {
            let sref = if srcref_ptr == get_R_InBCInterpreter() {
                R_findBCInterpreterSrcref(cptr)
            } else {
                srcref_ptr
            };
            if !sref.is_null() && !isNull(sref) {
                setAttrib(result, R_SrcrefSymbol(), duplicate(sref));
            }
        }
        UNPROTECT(1);
        result
    }
}

// ---------------------------------------------------------------------------
// R_syscall (helper)
// ---------------------------------------------------------------------------

pub unsafe fn R_syscall(n: c_int, _cptr: *mut sexp_context::RCNTXT) -> SEXP {
    unsafe {
        let mut n = if n > 0 { ctx_framedepth() - n } else { -n };

        if n < 0 {
            let msg = std::ffi::CString::new("not that many frames on the stack").unwrap();
            error(msg.as_ptr());
        }

        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 {
                    if n == 0 {
                        return getCallWithSrcref(&**ctx as *const _ as *mut sexp_context::RCNTXT);
                    } else {
                        n -= 1;
                    }
                }
                if ctx.nextcontext.is_null() {
                    if n == 0 {
                        return getCallWithSrcref(&**ctx as *const _ as *mut sexp_context::RCNTXT);
                    }
                    let msg = std::ffi::CString::new("not that many frames on the stack").unwrap();
                    error(msg.as_ptr());
                }
            }
            R_NilValue()
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_syscall_c(n: c_int, cptr: *mut sexp_context::RCNTXT) -> SEXP {
    unsafe { R_syscall(n, cptr) }
}

// ---------------------------------------------------------------------------
// R_sysfunction (helper)
// ---------------------------------------------------------------------------

pub unsafe fn R_sysfunction(n: c_int, _cptr: *mut sexp_context::RCNTXT) -> SEXP {
    unsafe {
        let mut n = if n > 0 { ctx_framedepth() - n } else { -n };

        if n < 0 {
            let msg = std::ffi::CString::new("not that many frames on the stack").unwrap();
            error(msg.as_ptr());
        }

        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 {
                    if n == 0 {
                        return duplicate(ctx.closure);
                    } else {
                        n -= 1;
                    }
                }
                if ctx.nextcontext.is_null() {
                    if n == 0 {
                        return duplicate(ctx.closure);
                    }
                    let msg = std::ffi::CString::new("not that many frames on the stack").unwrap();
                    error(msg.as_ptr());
                }
            }
            R_NilValue()
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_sysfunction_c(n: c_int, cptr: *mut sexp_context::RCNTXT) -> SEXP {
    unsafe { R_sysfunction(n, cptr) }
}

// ---------------------------------------------------------------------------
// countContexts (helper)
// ---------------------------------------------------------------------------

pub unsafe fn countContexts(ctxttype: c_int, browser: c_int) -> c_int {
    unsafe {
        let mut n: c_int = 0;
        let toplevel = get_R_ToplevelContext();

        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                if ctx_ptr == toplevel {
                    break;
                }
                if ctx.callflag == ctxttype {
                    n += 1;
                } else if browser != 0 {
                    if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 && RDEBUG(ctx.cloenv) != 0 {
                        n += 1;
                    }
                }
            }
        });

        n
    }
}

pub unsafe fn countContexts_c(ctxttype: c_int, browser: c_int) -> c_int {
    unsafe { countContexts(ctxttype, browser) }
}

// ---------------------------------------------------------------------------
// do_sysbrowser
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe extern "C" fn do_sysbrowser(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut rval = R_NilValue();
        checkArity(op, args);
        let n = asInteger(CAR(args));
        if n < 1 {
            let msg = std::ffi::CString::new("number of contexts must be positive").unwrap();
            error(msg.as_ptr());
        }

        let toplevel = get_R_ToplevelContext();

        let mut found_browser: *mut sexp_context::RCNTXT = ptr::null_mut();
        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                if ctx_ptr == toplevel {
                    break;
                }
                if ctx.callflag == ctxt_flags::CTXT_BROWSER {
                    found_browser = ctx_ptr;
                    break;
                }
            }
        });

        if found_browser.is_null() || (*found_browser).callflag != ctxt_flags::CTXT_BROWSER {
            let msg = std::ffi::CString::new("no browser context to query").unwrap();
            error(msg.as_ptr());
        }

        let pval = PRIMVAL(op);

        match pval {
            1 | 2 => {
                if n > 1 {
                    let mut remaining = n;
                    let mut target_browser = ptr::null_mut();
                    sexp_context::CONTEXT_STACK.with(|stack| {
                        let stack = stack.borrow();
                        for ctx in stack.iter().rev() {
                            let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                            if ctx_ptr == toplevel {
                                break;
                            }
                            if ctx.callflag == ctxt_flags::CTXT_BROWSER {
                                remaining -= 1;
                                if remaining <= 0 {
                                    target_browser = ctx_ptr;
                                    break;
                                }
                            }
                        }
                    });
                    if target_browser.is_null()
                        || (*target_browser).callflag != ctxt_flags::CTXT_BROWSER
                    {
                        let msg =
                            std::ffi::CString::new("not that many calls to browser are active")
                                .unwrap();
                        error(msg.as_ptr());
                    }
                    found_browser = target_browser;
                }
                let promargs = (*found_browser).promiseargs;
                if pval == 1 {
                    rval = if !promargs.is_null() && !isNull(promargs) {
                        CAR(promargs)
                    } else {
                        R_NilValue()
                    };
                } else {
                    rval = if !promargs.is_null() && !isNull(promargs) {
                        CADR(promargs)
                    } else {
                        R_NilValue()
                    };
                }
            }
            3 => {
                let mut remaining = n;
                let mut target_fn: *mut sexp_context::RCNTXT = ptr::null_mut();
                sexp_context::CONTEXT_STACK.with(|stack| {
                    let stack = stack.borrow();
                    for ctx in stack.iter().rev() {
                        let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                        if ctx_ptr == toplevel {
                            break;
                        }
                        if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 {
                            remaining -= 1;
                            if remaining <= 0 {
                                target_fn = ctx_ptr;
                                break;
                            }
                        }
                    }
                });
                if target_fn.is_null() || ((*target_fn).callflag & ctxt_flags::CTXT_FUNCTION) == 0 {
                    let msg = std::ffi::CString::new("not that many functions on the call stack")
                        .unwrap();
                    error(msg.as_ptr());
                }
                SET_RDEBUG((*target_fn).cloenv, 1);
            }
            _ => {}
        }

        rval
    }
}

// ---------------------------------------------------------------------------
// getLexicalContext (helper)
// ---------------------------------------------------------------------------

pub unsafe fn getLexicalContext(rho: SEXP) -> *mut sexp_context::RCNTXT {
    unsafe {
        if rho.is_null() {
            return ptr::null_mut();
        }
        let toplevel = get_R_ToplevelContext();
        let mut result: *mut sexp_context::RCNTXT = ptr::null_mut();

        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
                if ctx_ptr == toplevel {
                    break;
                }
                if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 && ctx.cloenv == rho {
                    result = ctx_ptr;
                    break;
                }
            }
        });

        result
    }
}

pub unsafe fn getLexicalContext_c(rho: SEXP) -> *mut sexp_context::RCNTXT {
    unsafe { getLexicalContext(rho) }
}

// ---------------------------------------------------------------------------
// getLexicalCall (helper)
// ---------------------------------------------------------------------------

pub unsafe fn getLexicalCall(rho: SEXP) -> SEXP {
    unsafe {
        let cptr = getLexicalContext(rho);
        if cptr.is_null() {
            R_NilValue()
        } else {
            (*cptr).call
        }
    }
}

pub unsafe fn getLexicalCall_c(rho: SEXP) -> SEXP {
    unsafe { getLexicalCall(rho) }
}

// ---------------------------------------------------------------------------
// do_sys
// ---------------------------------------------------------------------------

pub unsafe fn do_sys(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n: c_int = -1;
        checkArity(op, args);

        let t = sexp_context::R_GlobalContext();
        let sysparent = if t.is_null() {
            ptr::null_mut()
        } else {
            (*t).sysparent as SEXP
        };
        let cptr = getLexicalContext(sysparent);

        if length(args) == 1 {
            n = asInteger(CAR(args));
        }

        let pval = PRIMVAL(op);

        match pval {
            1 => {
                // parent
                if n == crate::sexp::ffi::NA_INTEGER {
                    let msg = std::ffi::CString::new("invalid 'n' argument").unwrap();
                    error(msg.as_ptr());
                }
                let nframe = ctx_framedepth();
                let mut i = nframe;
                let mut nn = n;
                while nn > 0 {
                    i = R_sysparent(nframe - i + 1, ptr::null_mut());
                    nn -= 1;
                }
                ScalarInteger(i)
            }
            2 => {
                // call
                if n == crate::sexp::ffi::NA_INTEGER {
                    let msg = std::ffi::CString::new("invalid 'which' argument").unwrap();
                    error(msg.as_ptr());
                }
                R_syscall(n, cptr)
            }
            3 => {
                // frame
                if n == crate::sexp::ffi::NA_INTEGER {
                    let msg = std::ffi::CString::new("invalid 'which' argument").unwrap();
                    error(msg.as_ptr());
                }
                R_sysframe(n, cptr)
            }
            4 => {
                // sys.nframe
                ScalarInteger(ctx_framedepth())
            }
            5 => {
                // sys.calls
                let nframe = ctx_framedepth();
                let rval = allocList(nframe);
                PROTECT(rval);
                let mut t = rval;
                for i in 1..=nframe {
                    let sc = R_syscall(i, cptr);
                    SETCAR(t, sc);
                    t = CDR(t);
                }
                UNPROTECT(1);
                rval
            }
            6 => {
                // sys.frames
                let nframe = ctx_framedepth();
                let rval = allocList(nframe);
                PROTECT(rval);
                let mut t = rval;
                for i in 1..=nframe {
                    let sf = R_sysframe(i, cptr);
                    SETCAR(t, sf);
                    t = CDR(t);
                }
                UNPROTECT(1);
                rval
            }
            7 => {
                // sys.on.exit
                let conexit = if cptr.is_null() {
                    R_NilValue()
                } else {
                    (*cptr).conexit
                };
                if isNull(conexit) {
                    R_NilValue()
                } else if isNull(CDR(conexit)) {
                    CAR(conexit)
                } else {
                    LCONS(R_BraceSymbol(), conexit)
                }
            }
            8 => {
                // sys.parents
                let nframe = ctx_framedepth();
                let rval = allocVector(SEXPTYPE::INTSXP.0, nframe);
                for i in 0..nframe {
                    let data = crate::sexp::accessors::INTEGER(rval);
                    if !data.is_null() {
                        let offset = i as isize;
                        *data.offset(offset) = R_sysparent(nframe - i, ptr::null_mut());
                    }
                }
                rval
            }
            9 => {
                // sys.function
                if n == crate::sexp::ffi::NA_INTEGER {
                    let msg = std::ffi::CString::new("invalid 'which' value").unwrap();
                    error(msg.as_ptr());
                }
                R_sysfunction(n, cptr)
            }
            _ => {
                let msg = std::ffi::CString::new("internal error in 'do_sys'").unwrap();
                error(msg.as_ptr());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// do_parentframe
// ---------------------------------------------------------------------------

pub unsafe fn do_parentframe(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let n = asInteger(CAR(args));
        if n == crate::sexp::ffi::NA_INTEGER || n < 1 {
            let msg = std::ffi::CString::new("invalid 'n' value").unwrap();
            error(msg.as_ptr());
        }

        let cptr = sexp_context::R_GlobalContext();
        let parent = R_findParentContext_impl(cptr, n);
        if parent.is_null() {
            R_GlobalEnv()
        } else {
            (*parent).sysparent as SEXP
        }
    }
}

// ---------------------------------------------------------------------------
// R_findExecContext (helper)
// ---------------------------------------------------------------------------

pub unsafe fn R_findExecContext_impl(
    start: *mut sexp_context::RCNTXT,
    envir: SEXP,
) -> *mut sexp_context::RCNTXT {
    sexp_context::CONTEXT_STACK.with(|stack| {
        let stack = stack.borrow();
        let mut found = false;
        for ctx in stack.iter().rev() {
            let ctx_ptr: *mut sexp_context::RCNTXT = &**ctx as *const _ as *mut _;
            if ctx_ptr == start && !found {
                found = true;
                continue;
            }
            if found {
                if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 && ctx.cloenv == envir {
                    return ctx_ptr;
                }
            }
            if ctx.nextcontext.is_null() {
                break;
            }
        }
        ptr::null_mut()
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_findExecContext(
    cptr: *mut sexp_context::RCNTXT,
    envir: SEXP,
) -> *mut sexp_context::RCNTXT {
    unsafe { R_findExecContext_impl(cptr, envir) }
}

// ---------------------------------------------------------------------------
// R_findParentContext (helper)
// ---------------------------------------------------------------------------

pub unsafe fn R_findParentContext_impl(
    cptr: *mut sexp_context::RCNTXT,
    mut n: c_int,
) -> *mut sexp_context::RCNTXT {
    unsafe {
        let sysparent = if cptr.is_null() {
            ptr::null_mut()
        } else {
            (*cptr).sysparent as SEXP
        };

        let mut current = cptr;
        loop {
            let found = R_findExecContext_impl(current, sysparent);
            if found.is_null() {
                return ptr::null_mut();
            }
            if n == 1 {
                return found;
            }
            n -= 1;
            current = found;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_findParentContext(
    cptr: *mut sexp_context::RCNTXT,
    n: c_int,
) -> *mut sexp_context::RCNTXT {
    unsafe { R_findParentContext_impl(cptr, n) }
}

// ---------------------------------------------------------------------------
// R_ToplevelExec (helper)
// ---------------------------------------------------------------------------

pub unsafe fn R_ToplevelExec_impl(
    fun: Option<unsafe extern "C" fn(*mut c_void)>,
    data: *mut c_void,
) -> Rboolean {
    unsafe {
        let topExp = R_CurrentExpr.with(|e| e.get());
        let oldHStack = get_R_HandlerStack();
        let oldRStack = get_R_RestartStack();
        let oldRVal = get_R_ReturnedValue();
        let oldvis = crate::sexp::globals::R_Visible();
        let saveToplevelContext = get_R_ToplevelContext();

        set_R_HandlerStack(R_NilValue());
        set_R_RestartStack(R_NilValue());

        let cptr = sexp_context::Rf_begincontext(
            ctxt_flags::CTXT_TOPLEVEL,
            R_NilValue(),
            R_GlobalEnv(),
            R_NilValue(),
            None,
            R_NilValue(),
            R_NilValue(),
        );

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            set_R_ToplevelContext(cptr);
            if let Some(f) = fun {
                f(data);
            }
        }));

        sexp_context::Rf_endcontext(cptr);

        set_R_ToplevelContext(saveToplevelContext);
        R_CurrentExpr.with(|e| e.set(topExp));
        set_R_HandlerStack(oldHStack);
        set_R_RestartStack(oldRStack);
        set_R_ReturnedValue(oldRVal);
        crate::sexp::globals::set_R_Visible(oldvis);

        match result {
            Ok(()) => R_TRUE,
            Err(_) => R_FALSE,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ToplevelExec(
    fun: Option<unsafe extern "C" fn(*mut c_void)>,
    data: *mut c_void,
) -> Rboolean {
    unsafe { R_ToplevelExec_impl(fun, data) }
}

// ---------------------------------------------------------------------------
// R_GetCurrentEnv
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetCurrentEnv() -> SEXP {
    unsafe {
        sexp_context::CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                if (ctx.callflag & ctxt_flags::CTXT_FUNCTION) != 0 {
                    return ctx.cloenv;
                }
                if ctx.nextcontext.is_null() {
                    break;
                }
            }
            R_GlobalEnv()
        })
    }
}

// ---------------------------------------------------------------------------
// R_tryEval
// ---------------------------------------------------------------------------

#[repr(C)]
struct ProtectedEvalData {
    expression: SEXP,
    val: SEXP,
    env: SEXP,
}

unsafe extern "C" fn protectedEval_trampoline(data: *mut c_void) {
    unsafe {
        let data = &mut *(data as *mut ProtectedEvalData);
        let eval_env = if data.env.is_null() || data.env == R_NilValue() {
            R_GlobalEnv()
        } else {
            data.env
        };
        data.val = eval(data.expression, eval_env);
        R_PreserveObject(data.val);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tryEval(e: SEXP, env: SEXP, ErrorOccurred: *mut c_int) -> SEXP {
    unsafe {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let eval_env = if env.is_null() || env == R_NilValue() {
                R_GlobalEnv()
            } else {
                env
            };
            eval(e, eval_env)
        }));

        match result {
            Ok(val) => {
                if !ErrorOccurred.is_null() {
                    *ErrorOccurred = 0;
                }
                val
            }
            Err(_) => {
                if !ErrorOccurred.is_null() {
                    *ErrorOccurred = 1;
                }
                ptr::null_mut()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R_tryEvalSilent
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tryEvalSilent(e: SEXP, env: SEXP, ErrorOccurred: *mut c_int) -> SEXP {
    unsafe {
        let oldshow = R_ShowErrorMessages.with(|s| s.get());
        R_ShowErrorMessages.with(|s| s.set(0));
        let val = R_tryEval(e, env, ErrorOccurred);
        R_ShowErrorMessages.with(|s| s.set(oldshow));
        val
    }
}

// ---------------------------------------------------------------------------
// R_ExecWithCleanup
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ExecWithCleanup(
    fun: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    data: *mut c_void,
    cleanfun: Option<unsafe extern "C" fn(*mut c_void)>,
    cleandata: *mut c_void,
) -> SEXP {
    unsafe {
        let cptr = sexp_context::Rf_begincontext(
            ctxt_flags::CTXT_CCODE,
            R_NilValue(),
            R_BaseEnv(),
            R_NilValue(),
            None,
            R_NilValue(),
            R_NilValue(),
        );

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(f) = fun {
                f(data)
            } else {
                R_NilValue()
            }
        }));

        if let Some(f) = cleanfun {
            f(cleandata);
        }

        sexp_context::Rf_endcontext(cptr);

        match result {
            Ok(val) => val,
            Err(_) => R_NilValue(),
        }
    }
}

// ---------------------------------------------------------------------------
// Unwind-protect mechanism
// ---------------------------------------------------------------------------

#[repr(C)]
struct unwind_cont_t {
    jumpmask: c_int,
    jumptarget: usize,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_MakeUnwindCont() -> SEXP {
    unsafe {
        let n_doubles = (std::mem::size_of::<unwind_cont_t>() + std::mem::size_of::<c_double>()
            - 1)
            / std::mem::size_of::<c_double>();
        let raw = Rf_allocVector(SEXPTYPE::REALSXP.0, n_doubles as c_int);
        if raw.is_null() {
            return R_NilValue();
        }
        CONS(R_NilValue(), raw)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ContinueUnwind(_cont: SEXP) -> ! {
    std::panic::panic_any(sexp_context::RError {
        message: "R_ContinueUnwind: unwind continuation".to_string(),
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_UnwindProtect(
    fun: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    data: *mut c_void,
    cleanfun: Option<unsafe extern "C" fn(*mut c_void, Rboolean)>,
    cleandata: *mut c_void,
    cont: SEXP,
) -> SEXP {
    unsafe {
        let cont = if cont.is_null() {
            PROTECT(R_MakeUnwindCont());
            let result = R_UnwindProtect(fun, data, cleanfun, cleandata, cont);
            UNPROTECT(1);
            return result;
        } else {
            cont
        };

        let cptr = sexp_context::Rf_begincontext(
            ctxt_flags::CTXT_TOPLEVEL,
            R_NilValue(),
            R_GlobalEnv(),
            R_NilValue(),
            None,
            R_NilValue(),
            R_NilValue(),
        );

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(f) = fun {
                f(data)
            } else {
                R_NilValue()
            }
        }));

        sexp_context::Rf_endcontext(cptr);

        let jump = result.is_err();
        if let Some(f) = cleanfun {
            f(cleandata, if jump { R_TRUE } else { R_FALSE });
        }

        if jump {
            R_ContinueUnwind(cont);
        }

        match result {
            Ok(val) => val,
            Err(_) => R_NilValue(),
        }
    }
}

// ---------------------------------------------------------------------------
// R_jump_to_top
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_jump_to_top() {
    unsafe {
        let toplevel = get_R_ToplevelContext();
        if !toplevel.is_null() {
            R_jumpctxt_impl(toplevel, 0, ptr::null_mut());
        } else {
            std::panic::panic_any(sexp_context::RError {
                message: "jump_to_top: no toplevel context".to_string(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// R_InsertRestartHandlers
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InsertRestartHandlers(_call: SEXP, _rho: SEXP) {
    // Stub
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::context::RCNTXT;

    #[test]
    fn test_ctxt_constants() {
        assert_eq!(ctxt_flags::CTXT_TOPLEVEL, 0);
        assert_eq!(ctxt_flags::CTXT_FUNCTION, 1);
        assert_eq!(ctxt_flags::CTXT_CCODE, 2);
        assert_eq!(ctxt_flags::CTXT_LOOP, 16);
    }

    #[test]
    fn test_stackval_roundtrip() {
        let val = SEXP_TO_STACKVAL(ptr::null_mut());
        assert_eq!(val.tag, 0);
        unsafe {
            assert_eq!(STACKVAL_TO_SEXP(&val), ptr::null_mut());
        }
    }

    #[test]
    fn test_framedepth_empty() {
        unsafe {
            sexp_context::CONTEXT_STACK.with(|s| s.borrow_mut().clear());
            let depth = ctx_framedepth();
            assert_eq!(depth, 0);
        }
    }

    #[test]
    fn test_get_current_env_empty() {
        unsafe {
            sexp_context::CONTEXT_STACK.with(|s| s.borrow_mut().clear());
            let env = R_GetCurrentEnv();
            assert!(!env.is_null());
        }
    }

    #[test]
    fn test_count_contexts_empty() {
        unsafe {
            sexp_context::CONTEXT_STACK.with(|s| s.borrow_mut().clear());
            let count = countContexts(ctxt_flags::CTXT_FUNCTION, 0);
            assert_eq!(count, 0);
        }
    }
}
