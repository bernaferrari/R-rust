#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::c_int;
use std::ptr;

use crate::mainutils::coerce::asInteger;
use crate::mainutils::duplicate::{duplicate, shallow_duplicate};
use crate::mainutils::relop::{PRIMVAL, checkArity};
use crate::sexp::accessors::{CAR, CDR, SET_INTEGER_ELT, SETCAR};
use crate::sexp::constructors::Rf_cons;
use crate::sexp::constructors::{Rf_allocVector, Rf_length};
use crate::sexp::context::{FrameValues, R_GlobalContext_in, RCNTXT, ctxt_flags, frame_values};
use crate::sexp::ffi::NA_INTEGER;
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::globals::R_GlobalEnv_in;
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::{RInstance, with_required_current_instance};
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Local error helper
// ---------------------------------------------------------------------------

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

fn isNull(x: SEXP) -> bool {
    crate::sexp::accessors::Rf_isNull(x) != 0
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

/// GNU `getLexicalCall(rho)`: call of the first `CTXT_FUNCTION` context
/// whose `cloenv` is `rho`. Top-level eval has no such frame, so `R_NilValue`.
pub fn get_lexical_call(rho: SEXP) -> SEXP {
    crate::sexp::instance::with_current_instance(|instance| {
        // SAFETY: the active instance owns the stack. Only copy an address while
        // reading its cells; this scan neither allocates R objects nor calls R.
        unsafe {
            (*instance)
                .context_stack
                .iter()
                .rev()
                .map(|cell| &*cell.get())
                .take_while(|frame| frame.callflag != ctxt_flags::CTXT_TOPLEVEL)
                .find(|frame| {
                    frame.callflag & ctxt_flags::CTXT_FUNCTION != 0
                        && frame.cloenv.as_raw() == rho
                })
                .map(|frame| frame.call.as_raw())
        }
    })
    .flatten()
    .filter(|call| !call.is_null())
    .unwrap_or_else(R_NilValue)
}

// ---------------------------------------------------------------------------
// R_sysframe — get environment of nth function context
// ---------------------------------------------------------------------------

pub unsafe fn R_sysframe(n: c_int, cptr: *mut RCNTXT) -> SEXP {
    with_required_current_instance(|instance| unsafe { R_sysframe_in(instance, n, cptr) })
}

pub unsafe fn R_sysframe_in(instance: *mut RInstance, n: c_int, cptr: *mut RCNTXT) -> SEXP {
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
                    return (*c).cloenv.as_raw();
                }
                n -= 1;
            }
            c = (*c).nextcontext;
        }
        error("not that many frames on the stack");
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
                    return shallow_duplicate((*c).call.as_raw());
                }
                n -= 1;
            }
            c = (*c).nextcontext;
        }
        error("not that many frames on the stack");
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
                    return duplicate((*c).callfun.as_raw());
                }
                n -= 1;
            }
            c = (*c).nextcontext;
        }
        error("not that many frames on the stack");
    }
}

// ---------------------------------------------------------------------------
// R_sysparent — get sysparent frame number (S-compatible semantics)
// ---------------------------------------------------------------------------

pub unsafe fn R_sysparent(n: c_int, cptr: *mut RCNTXT) -> c_int {
    with_required_current_instance(|instance| unsafe { R_sysparent_in(instance, n, cptr) })
}

pub unsafe fn R_sysparent_in(instance: *mut RInstance, n: c_int, cptr: *mut RCNTXT) -> c_int {
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
        let s = (*c).sysparent.as_raw();
        if s == R_GlobalEnv_in(instance) {
            return 0;
        }
        let mut j: c_int = 0;
        // Upstream reuses the (walk-adjusted) `n` as the fallback target:
        // `if (cptr->cloenv == s) n = j;` — when the sysparent environment
        // matches no frame's cloenv, the result is j - n + 1 with the
        // original n, NOT j + 1 (a zero target would over-count by one
        // and could exceed the frame depth).
        let mut target_n: c_int = n;
        let mut c2 = cptr;
        while !c2.is_null() {
            if (*c2).callflag & ctxt_flags::CTXT_FUNCTION != 0 {
                j += 1;
                if (*c2).cloenv.as_raw() == s {
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
// R_findExecContext — find context with matching cloenv
// ---------------------------------------------------------------------------

pub unsafe fn R_findExecContext(cptr: *mut RCNTXT, envir: SEXP) -> *mut RCNTXT {
    unsafe {
        let mut c = cptr;
        if c.is_null() {
            return ptr::null_mut();
        }
        while !(*c).nextcontext.is_null() {
            if ((*c).callflag & ctxt_flags::CTXT_FUNCTION) != 0 && (*c).cloenv.as_raw() == envir {
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
            c = R_findExecContext(c, (*c).sysparent.as_raw());
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

pub fn getLexicalContext(rho: SEXP) -> *mut RCNTXT {
    with_required_current_instance(|instance| unsafe { getLexicalContext_in(instance, rho) })
}

pub unsafe fn getLexicalContext_in(instance: *mut RInstance, rho: SEXP) -> *mut RCNTXT {
    unsafe {
        let mut c = R_GlobalContext_in(instance);
        if c.is_null() {
            return ptr::null_mut();
        }
        while !c.is_null() {
            if ((*c).callflag & ctxt_flags::CTXT_FUNCTION) != 0 && (*c).cloenv.as_raw() == rho {
                return c;
            }
            c = (*c).nextcontext;
        }
        R_GlobalContext_in(instance)
    }
}

/// Build the outermost-first pairlist sys.calls()/sys.frames() returns.
///
/// Frame values are retained before the list allocation can run GC
/// callbacks that might pop or clear the stack.
pub(crate) fn frame_value_list(start: *mut RCNTXT, values: FrameValues) -> SEXP {
    let duplicate_calls = matches!(values, FrameValues::Calls);
    let values = frame_values(start, values);
    let result = crate::sexp::constructors::Rf_allocList(values.len() as c_int);
    if values.is_empty() {
        return result;
    }
    let _result = protect(result);
    let mut cell = result;
    for value in &values {
        let value = value.as_raw();
        SETCAR(
            cell,
            if duplicate_calls {
                shallow_duplicate(value)
            } else {
                value
            },
        );
        cell = CDR(cell);
    }
    result
}

// ---------------------------------------------------------------------------
// do_sys — central dispatcher for sys.parent/call/frame/nframe/calls/frames/on.exit/parents/function
// ---------------------------------------------------------------------------

pub fn do_sys(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    with_required_current_instance(|instance| unsafe { do_sys_in(instance, call, op, args, rho) })
}

pub unsafe fn do_sys_in(
    instance: *mut RInstance,
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
        let t = (*top).sysparent.as_raw();
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
            5 => frame_value_list(cptr, FrameValues::Calls),
            6 => frame_value_list(cptr, FrameValues::Environments),
            7 => {
                // sys.on.exit
                let conexit = (*cptr).conexit.as_raw();
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
                let parents = (0..nframe)
                    .map(|i| R_sysparent_in(instance, nframe - i, cptr))
                    .collect::<Vec<_>>();
                let rval = Rf_allocVector(SEXPTYPE::INTSXP, nframe);
                for (i, parent) in parents.into_iter().enumerate() {
                    SET_INTEGER_ELT(rval, i as c_int, parent);
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
            }
        }
    }
}

pub fn do_browser(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// R_run_onexits — run on.exit / cend handlers
// ---------------------------------------------------------------------------

/// Run the interpreted `on.exit` chain (and optional `cend` thunk) on one context.
///
/// Upstream `context.c` PROTECTs the chain head and keeps `conexit` pointing at
/// the not-yet-run remainder while each handler evaluates. Cleared before the
/// loop to prevent recursion if a handler itself jumps.
pub(crate) unsafe fn R_run_onexits_for_context(cptr: *mut RCNTXT) {
    unsafe {
        if cptr.is_null() {
            return;
        }
        let instance = crate::sexp::instance::current_instance_ptr()
            .unwrap_or_else(|| error("no active context owner"));
        let owner_pin = crate::sexp::context::pin_context_owner_in(instance);
        let _context_lease = crate::sexp::context::retain_context_in(instance, cptr)
            .unwrap_or_else(|| error("context no longer belongs to the active owner"));
        if let Some(cend) = (*cptr).cend {
            (*cptr).cend = None;
            let data = (*cptr).cenddata;
            cend(data);
            crate::sexp::context::require_context_owner_live(owner_pin.as_ref());
        }
        let Some(chain) = (*cptr).conexit.owned() else {
            return;
        };
        let conexit = chain.as_raw();
        if isNull(conexit) {
            return;
        }
        (*cptr).conexit.replace_from_raw(R_NilValue());
        (*cptr).onexit_active = 1;

        let environment = (*cptr).cloenv.owned();
        let rho = environment
            .as_ref()
            .map_or(ptr::null_mut(), |value| value.as_raw());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut current = conexit;
            while !isNull(current) {
                let expr = CAR(current);
                (*cptr).conexit.replace_from_raw(CDR(current));
                if !isNull(expr) {
                    let _ = super::eval::Rf_eval(expr, rho);
                    crate::sexp::context::require_context_owner_live(owner_pin.as_ref());
                }
                current = (*cptr).conexit.as_raw();
            }
        }));
        (*cptr).onexit_active = 0;
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }
}

/// Run `cend` + `conexit` for every context from the stack top down to, but not
/// including, `target` (GNU `R_run_onexits`). Null `target` drains the whole stack.
///
/// Handlers run *before* any Rust unwind so interpreter callbacks never execute
/// from a Drop/catch_unwind frame that still owns live context mutability.
pub unsafe fn R_run_onexits_until(target: *mut RCNTXT) {
    with_required_current_instance(|instance| unsafe {
        R_run_onexits_until_in(instance, target);
    });
}

pub unsafe fn R_run_onexits_until_in(instance: *mut RInstance, target: *mut RCNTXT) {
    unsafe {
        let owner_pin = crate::sexp::context::pin_context_owner_in(instance);
        let mut c = R_GlobalContext_in(instance);
        while !c.is_null() && c != target {
            let context_lease = crate::sexp::context::retain_context_in(instance, c)
                .unwrap_or_else(|| error("context no longer belongs to its owner"));
            R_run_onexits_for_context(c);
            crate::sexp::context::require_context_owner_live(owner_pin.as_ref());
            c = (*context_lease.get()).nextcontext;
        }
        if !target.is_null() && c.is_null() {
            error("bad target context--should NEVER happen if R was called correctly");
        }
    }
}

/// GNU `R_run_onexits(NULL)` — drain the entire context stack.
pub fn R_run_onexits() {
    with_required_current_instance(|instance| unsafe { R_run_onexits_in(instance) });
}

pub unsafe fn R_run_onexits_in(instance: *mut RInstance) {
    unsafe {
        R_run_onexits_until_in(instance, ptr::null_mut());
    }
}

// ---------------------------------------------------------------------------
// eval_CleanUp — cleanup on error or normal exit
// ---------------------------------------------------------------------------

pub fn eval_CleanUp(_sa: c_int, _status: c_int, _RunLast: c_int) {
    R_run_onexits();
}

// ---------------------------------------------------------------------------
// R_jumpctxt — jump to a specific context
// ---------------------------------------------------------------------------

/// GNU `CTXT_NEXT` / `CTXT_BREAK` jump-return codes (Defn.h). Distinct from
/// this port's `ctxt_flags` context-type encoding.
pub const JUMP_NEXT: c_int = 1;
pub const JUMP_BREAK: c_int = 2;

/// Jump to `target`, running intervening on.exit/cend handlers first.
///
/// Mirrors GNU `R_jumpctxt(target, mask, val)`. Raises a typed `RSignal`
/// (`Break`/`Next`/`Return`/`Jump`) instead of `RError("jump_to_context")`.
pub unsafe fn R_jumpctxt(target: *mut RCNTXT, mask: c_int, val: SEXP) -> ! {
    unsafe {
        // Capture the actual value and original target cell before cleanup can
        // replace context fields, detach the stack, or collect the heap.
        let transfer = if mask == JUMP_BREAK || mask == JUMP_NEXT {
            None
        } else if (mask & ctxt_flags::CTXT_FUNCTION) != 0
            || (mask & ctxt_flags::CTXT_RETURN) != 0
            || (mask & ctxt_flags::CTXT_BROWSER) != 0
        {
            Some(crate::sexp::context::RSignal::Return(
                crate::sexp::context::return_transfer(target, val),
            ))
        } else {
            Some(crate::sexp::context::RSignal::Jump(
                crate::sexp::context::jump_transfer(target, mask, val),
            ))
        };
        if !target.is_null() {
            (*target).returnValue.replace_from_raw(val);
            (*target).jumped = 1;
        }
        let savevis = super::runtime::visible();
        R_run_onexits_until(target);
        super::runtime::set_visible(savevis);

        if mask == JUMP_BREAK {
            std::panic::panic_any(crate::sexp::context::RSignal::Break);
        }
        if mask == JUMP_NEXT {
            std::panic::panic_any(crate::sexp::context::RSignal::Next);
        }
        let transfer = transfer.expect("value-bearing context transfer");
        let ticket = match &transfer {
            crate::sexp::context::RSignal::Return(ticket)
            | crate::sexp::context::RSignal::Jump(ticket) => ticket,
            _ => unreachable!("value-bearing context transfer"),
        };
        ticket
            .resolve()
            .and_then(|lease| lease.require_live())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        std::panic::panic_any(transfer);
    }
}

/// Locate a matching context and [`R_jumpctxt`] into it (GNU `findcontext`).
pub fn findcontext_jump(mask: c_int, env: SEXP, val: SEXP) -> ! {
    with_required_current_instance(|instance| unsafe {
        findcontext_jump_in(instance, mask, env, val);
    });
    unreachable!("findcontext_jump_in always diverges")
}

pub unsafe fn findcontext_jump_in(
    instance: *mut RInstance,
    mask: c_int,
    env: SEXP,
    val: SEXP,
) -> ! {
    unsafe {
        let loop_jump = mask == JUMP_BREAK || mask == JUMP_NEXT;
        let mut c = R_GlobalContext_in(instance);
        while !c.is_null() {
            let flag = (*c).callflag;
            if flag == ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            let env_ok = env.is_null() || (*c).cloenv.as_raw() == env;
            if loop_jump {
                if (flag & ctxt_flags::CTXT_LOOP) != 0 && env_ok {
                    R_jumpctxt(c, mask, val);
                }
            } else if env_ok && (flag & mask) != 0 {
                R_jumpctxt(c, mask, val);
            }
            c = (*c).nextcontext;
        }
        if loop_jump {
            // No AST CTXT_LOOP. Compiled loops record their exit on the
            // bytecode loop stack and catch this signal around OP_CALL.
            // A real top-level next/break still becomes the "no loop" error
            // once that catcher is absent.
            if mask == JUMP_BREAK {
                std::panic::panic_any(crate::sexp::context::RSignal::Break);
            }
            std::panic::panic_any(crate::sexp::context::RSignal::Next);
        } else {
            error("no function to return from, jumping to top level");
        }
    }
}

// ---------------------------------------------------------------------------
// R_jump_to_top — jump to the top-level context
// ---------------------------------------------------------------------------

pub fn R_jump_to_top() {
    let transfer = unsafe {
        crate::sexp::context::jump_transfer(
            ptr::null_mut(),
            ctxt_flags::CTXT_TOPLEVEL,
            R_NilValue(),
        )
    };
    unsafe {
        R_run_onexits_until(ptr::null_mut());
    }
    transfer
        .resolve()
        .and_then(|lease| lease.require_live())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    std::panic::panic_any(crate::sexp::context::RSignal::Jump(transfer));
}

// ---------------------------------------------------------------------------
// R_InsertRestartHandlers — manage restart handlers
// ---------------------------------------------------------------------------

/// Known gap: no-op stub. Full interactive `abort`/`browser`/`tryRestart`
/// defaults remain incomplete versus GNU `R_InsertRestartHandlers`.
pub fn R_InsertRestartHandlers(_call: SEXP, _rho: SEXP) {}

unsafe fn context_or_top_in(instance: *mut RInstance, cptr: *mut RCNTXT) -> *mut RCNTXT {
    if cptr.is_null() {
        unsafe { R_GlobalContext_in(instance) }
    } else {
        cptr
    }
}

#[cfg(test)]
mod owned_transfer_tests {
    use super::*;
    use crate::sexp::{context::RSignal, session::RSession, transfer::OwnedTransfer};

    #[test]
    fn frame_value_list_survives_stack_removal_during_allocation() {
        use std::{cell::Cell, rc::Rc};
        use crate::sexp::{constructors::Rf_lang2, context::Rf_begincontext_in, symbol::install_str};

        for values in [FrameValues::Calls, FrameValues::Environments] {
            let calls = matches!(values, FrameValues::Calls);
            let session = RSession::new_for_gc_tests();
            session.with_active_in(|instance| unsafe {
                let nil = R_NilValue();
                let global = crate::sexp::globals::R_GlobalEnv();
                let mut top = ptr::null_mut();
                let mut expected = Vec::new();
                for name in ["outer", "inner"] {
                    let symbol = install_str(name);
                    let call = Rf_lang2(symbol, symbol);
                    let root = protect(call);
                    let env = crate::sexp::memory_ext::NewEnvironment(nil, global, nil);
                    top = Rf_begincontext_in(instance, ctxt_flags::CTXT_FUNCTION, call, env, global, None, nil, nil);
                    expected.push(if calls { symbol } else { env });
                    drop(root);
                }
                let notified = Rc::new(Cell::new(false));
                let observed = notified.clone();
                crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                    if !observed.replace(true) {
                        (*instance).context_stack.clear();
                        crate::sexp::gengc::full_gc_in(instance);
                    }
                }));
                crate::mainutils::memory_main::set_gc_torture_for_test_in(instance, 1, Some(1));
                let result = frame_value_list(top, values);
                assert!(notified.get());
                assert_eq!(crate::sexp::context::current_frame_depth(), 0);
                let observed = [CAR(result), CAR(CDR(result))];
                if calls {
                    assert_eq!(observed.map(CAR).as_slice(), expected);
                } else {
                    assert_eq!(observed.as_slice(), expected);
                }
                assert_eq!(CDR(CDR(result)), nil);
            });
        }
    }

    #[test]
    fn owned_jump_transfer_retains_value_and_target_through_collecting_cleanup() {
        struct Cleanup {
            instance: *mut RInstance,
            target: *mut RCNTXT,
            called: bool,
        }
        unsafe extern "C" fn cleanup(data: *mut std::ffi::c_void) {
            let data = unsafe { &mut *data.cast::<Cleanup>() };
            unsafe {
                (*data.target).returnValue.replace_from_raw(R_NilValue());
                (*data.instance).context_stack.clear();
                crate::sexp::gengc::full_gc_in(data.instance);
            }
            data.called = true;
        }
        for mask in [ctxt_flags::CTXT_RETURN, ctxt_flags::CTXT_GENERIC] {
            let session = RSession::new_for_gc_tests();
            session.with_active_in(|instance| unsafe {
                let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
                let value = owner
                    .node_factory()
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                    .unwrap();
                let pointer = value.as_raw();
                SET_INTEGER_ELT(pointer, 0, 87);
                let node = crate::sexp::memory::checked_projection(pointer).unwrap().1;
                let target = crate::sexp::context::Rf_begincontext_in(
                    instance,
                    mask,
                    R_NilValue(),
                    R_NilValue(),
                    R_NilValue(),
                    None,
                    R_NilValue(),
                    R_NilValue(),
                );
                let original = crate::sexp::context::retain_context_in(instance, target).unwrap();
                let intermediate = crate::sexp::context::Rf_begincontext_in(
                    instance,
                    ctxt_flags::CTXT_FUNCTION,
                    R_NilValue(),
                    R_NilValue(),
                    R_NilValue(),
                    None,
                    R_NilValue(),
                    R_NilValue(),
                );
                let mut data = Cleanup {
                    instance,
                    target,
                    called: false,
                };
                (*intermediate).cend = Some(cleanup);
                (*intermediate).cenddata = (&mut data as *mut Cleanup).cast();
                drop(value);
                let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    R_jumpctxt(target, mask, pointer)
                }))
                .expect_err("targeted transfer must unwind");
                assert!(data.called);
                assert!(node.is_live());
                let signal = payload
                    .downcast::<RSignal>()
                    .expect("owned transfer signal");
                let ticket = match *signal {
                    RSignal::Return(ticket) | RSignal::Jump(ticket) => ticket,
                    other => panic!("unexpected signal: {other:?}"),
                };
                let lease = ticket.take().unwrap();
                match lease.data() {
                    OwnedTransfer::Return {
                        target: Some(target),
                        value,
                    }
                    | OwnedTransfer::Jump {
                        target: Some(target),
                        value,
                        ..
                    } => {
                        assert!(std::rc::Rc::ptr_eq(target, &original));
                        assert_eq!(value.integer_elt(0).unwrap(), 87);
                    }
                    _ => panic!("unexpected transfer"),
                }
                lease.require_live().unwrap();
                drop(lease);
                drop(original);
                owner.full_gc().unwrap();
                assert!(!node.is_live(), "consumed transfer must release its value");
            });
        }
    }
}
