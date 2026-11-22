#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Restarts and abort handling: stack/interrupt checks, jump-to-top-level
//! (longjmp replacement), onintr, and restart-stack manipulation.

use super::*;

// ---------------------------------------------------------------------------
// Stack overflow and interrupt checking
// ---------------------------------------------------------------------------

/// Signal a C stack overflow.
/// Matches C's `R_SignalCStackOverflow(intptr_t usage)`.
/// Uses R_makeCStackOverflowError condition object when available.
pub fn R_SignalCStackOverflow(usage: isize) {
    {
        // Try to use the expression stack overflow error condition
        let cond = R_makeCStackOverflowError(globals::R_NilValue(), usage);
        if !cond.is_null() {
            // calling handlers at this point might produce a C stack
            // overflow/SEGFAULT so treat them as failed and skip them
            R_signalErrorConditionEx(cond, globals::R_NilValue(), 1);
        } else {
            let msg = format!("C stack usage {} is too close to the limit", usage);
            R_SetErrmessage(&msg);
            std::panic::panic_any(RError { message: msg });
        }
    }
}

/// Check for stack overflow.
/// In C this checks against R_CStackLimit; in Rust we check eval depth.
pub fn R_CheckStack() {
    {
        let depth = globals::R_EvalDepth();
        let limit = globals::R_EvalDepthLimit();
        if depth >= limit {
            R_SignalCStackOverflow(depth as isize);
        }
    }
}

/// Check for stack overflow with extra space.
pub fn R_CheckStack2(_extra: usize) {
    {
        R_CheckStack();
    }
}

/// Check for user interrupts.
pub fn R_CheckUserInterrupt() {
    {
        R_CheckStack();
        if interrupts_suspended() {
            return;
        }
        if interrupts_pending() {
            onintr();
        }
    }
}

// ---------------------------------------------------------------------------
// Jump to top level (longjmp replacement)
// ---------------------------------------------------------------------------

/// Jump to the top-level context.
/// In C, this uses longjmp. In Rust, we panic with RError.
pub fn jump_to_top_ex(
    traceback: c_int,
    try_user_handler: c_int,
    process_warnings: c_int,
    reset_console: c_int,
    ignore_restart: c_int,
) {
    let old_in_error = in_error();
    let mut have_handler = false;

    if try_user_handler != 0 && old_in_error < 3 {
        if old_in_error == 0 {
            set_in_error(1);
        }
        let err_opt = GetOption1(crate::sexp::symbol::install_str("error"));
        have_handler = !err_opt.is_null() && err_opt != globals::R_NilValue();
        if have_handler {
            let is_lang = TYPEOF(err_opt) == SEXPTYPE::LANGSXP;
            let is_expr = TYPEOF(err_opt) == SEXPTYPE::EXPRSXP;
            if !is_lang && !is_expr {
                eprintln!("invalid option \"error\"");
            } else {
                set_in_error(3);
                if is_lang {
                    let _ = crate::eval::eval::Rf_eval(err_opt, globals::R_GlobalEnv());
                } else {
                    let n = LENGTH(err_opt);
                    for i in 0..n {
                        let _ = crate::eval::eval::Rf_eval(
                            crate::sexp::accessors::VECTOR_ELT(err_opt, i as R_xlen_t),
                            globals::R_GlobalEnv(),
                        );
                    }
                }
                set_in_error(old_in_error);
            }
        }
        set_in_error(old_in_error);
    }

    if process_warnings != 0 && collect_warnings() > 0 {
        PrintWarnings();
    }

    if ignore_restart == 0 {
        try_jump_to_restart();
    }

    if traceback != 0 && old_in_error < 2 {
        set_in_error(2);
        let tb = R_GetTracebackOnly(0);
        let sym = crate::sexp::symbol::install_str(".Traceback");
        SET_SYMVALUE(sym, tb);
        set_in_error(old_in_error);
    }

    set_in_error(0);
    std::panic::panic_any(RSignal::Error {
        message: "jump_to_top".to_string(),
    });
}

fn try_jump_to_restart() {
    let mut list = restart_stack();
    while !list.is_null() && list != globals::R_NilValue() {
        let restart = CAR(list);
        if TYPEOF(restart) == SEXPTYPE::VECSXP && LENGTH(restart) > 1 {
            let name = crate::sexp::accessors::VECTOR_ELT(restart, 0);
            if TYPEOF(name) == SEXPTYPE::STRSXP && LENGTH(name) == 1 {
                let cname = char_cstring(STRING_ELT(name, 0));
                if !cname.is_none() {
                    let bytes = cname.as_deref().unwrap_or_default().to_bytes();
                    if bytes == b"browser" || bytes == b"tryRestart" || bytes == b"abort" {
                        invokeRestart(restart, globals::R_NilValue());
                    }
                }
            }
        }
        list = CDR(list);
    }
}

fn invokeRestart(restart: SEXP, _args: SEXP) {
    std::panic::panic_any(RSignal::Error {
        message: "restart invoked".to_string(),
    });
}

/// Handle interrupt signal.
pub fn onintr() {
    {
        jump_to_top_ex(1, 1, 0, 0, 0);
    }
}

/// Handle interrupt signal without resume option.
pub fn onintrNoResume() {
    {
        jump_to_top_ex(0, 1, 0, 0, 0);
    }
}

// Restart handling
// ---------------------------------------------------------------------------

/// do_getRestart — get a restart from the restart stack.
pub fn do_getRestart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let mut i = if isInteger(CAR(args)) != 0 && LENGTH(CAR(args)) >= 1 {
            *INTEGER(CAR(args))
        } else {
            crate::sexp::ffi::NA_INTEGER
        };

        let mut list = restart_stack();
        while !list.is_null() && i > 1 {
            list = CDR(list);
            i -= 1;
        }
        if !list.is_null() {
            CAR(list)
        } else if i == 1 {
            // Return abort restart
            let name = crate::sexp::constructors::mk_string_str("abort");
            let entry = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(entry, 0, name);
            SET_VECTOR_ELT(entry, 1, globals::R_NilValue());
            setAttrib_wrap(
                entry,
                R_ClassSymbol(),
                crate::sexp::constructors::mk_string_str("restart"),
            );
            entry
        } else {
            globals::R_NilValue()
        }
    }
}

/// do_addRestart — add a restart to the restart stack.
pub fn do_addRestart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    checkArity(op, args);
    let r = CAR(args);
    if TYPEOF(r) != SEXPTYPE::VECSXP || LENGTH(r) < 2 {
        errorcall_cstr(call, c"bad restart");
    }
    set_restart_stack(Rf_cons(r, restart_stack()));
    globals::R_NilValue()
}

/// do_invokeRestart — invoke a restart.
pub fn do_invokeRestart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    checkArity(op, args);
    let r = CAR(args);
    if TYPEOF(r) != SEXPTYPE::VECSXP || LENGTH(r) < 2 {
        errorcall_cstr(call, c"bad restart");
    }
    // invokeRestart would jump to the restart; for now just panic
    let exit = VECTOR_ELT(r, 1);
    if isNull(exit) != 0 {
        set_restart_stack(globals::R_NilValue());
        jump_to_top_ex(0, 0, 1, 1, 1);
    }
    ptr::null_mut() // unreachable
}

/// do_addTryHandlers — add tryCatch handlers.
pub fn do_addTryHandlers(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let global_ctx = crate::sexp::context::R_GlobalContext();
        if global_ctx.is_null()
            || (*global_ctx).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION == 0
        {
            errorcall_cstr(call, c"not in a try context");
        }
        (*global_ctx).callflag |= crate::sexp::context::ctxt_flags::CTXT_RETURN;
        // SAFETY: `global_ctx` is the live, non-null global context checked above.
        R_InsertRestartHandlers(global_ctx, c"tryRestart");
        globals::R_NilValue()
    }
}

/// # Safety
///
/// `cptr` must point to a live context on the current context stack.
unsafe fn R_InsertRestartHandlers(cptr: *mut crate::sexp::context::RCNTXT, cname: &CStr) {
    unsafe {
        let h = GetOption1(crate::sexp::symbol::install_str("browser.error.handler"));
        let h = if !h.is_null() && TYPEOF(h) == SEXPTYPE::CLOSXP {
            h
        } else {
            globals::R_RestartToken()
        };
        let rho = (*cptr).cloenv.as_raw();
        let klass = mk_char("error");
        let _klass_guard = protect(klass);
        let entry = mkHandlerEntry(klass, rho, h, rho, globals::R_NilValue(), 1);
        let old_stack = handler_stack();
        let new_top = Rf_cons(entry, old_stack);
        set_handler_stack(new_top);

        addInternalRestart(cptr, cname);
    }
}

fn addInternalRestart(cptr: *mut crate::sexp::context::RCNTXT, cname: &CStr) {
    let cname_str = cname.to_bytes();
    let name = crate::sexp::constructors::mk_string_bytes(
        std::ffi::CString::new(cname_str)
            .unwrap_or_default()
            .to_bytes(),
    );
    let _name_guard = protect(name);
    let entry = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let _entry_guard = protect(entry);
    crate::sexp::accessors::SET_VECTOR_ELT(entry, 0, name);
    let ext_ptr = crate::mainutils::memory_main::R_MakeExternalPtr(
        cptr as *mut c_void,
        globals::R_NilValue(),
        globals::R_NilValue(),
    );
    crate::sexp::accessors::SET_VECTOR_ELT(entry, 1, ext_ptr);
    crate::eval::attrib_core::setAttrib(entry, R_ClassSymbol(), mk_string("restart"));
    let old_stack = restart_stack();
    let new_top = Rf_cons(entry, old_stack);
    set_restart_stack(new_top);
}

// ---------------------------------------------------------------------------

/// jump_to_toplevel — jump to top level without traceback, user error handler,
/// or try/browser frames.
///
/// Matches C's `void jump_to_toplevel(void)`.
pub fn jump_to_toplevel() {
    {
        jump_to_top_ex(0, 0, 1, 1, 1);
    }
}
