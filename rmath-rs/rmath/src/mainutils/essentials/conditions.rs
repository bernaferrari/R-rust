//! Essentials domain module `conditions` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::ffi::{FALSE, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete error handling — calling handlers and restarts
// ---------------------------------------------------------------------------

/// R's `withCallingHandlers(expr, ...)` — evaluate expr with calling handlers.
/// Handlers are evaluated before unwinding (unlike tryCatch).
/// R's `try(expr, silent)` — evaluate expr, converting an error into an
/// invisible "try-error" condition object (stock base::try, implemented at
/// C level here because the port has no R-level bootstrap definitions).
pub unsafe fn do_try(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let silent_arg = CADR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));

        match result {
            Ok(value) => value,
            Err(payload) => {
                // Extract the error message; re-panic anything that is not
                // one of the crate's error payloads.
                let message: String = if let Some(msg) =
                    payload.downcast_ref::<crate::sexp::context::RError>()
                {
                    msg.message.clone()
                } else if let Some(sig) = payload.downcast_ref::<crate::sexp::context::RSignal>() {
                    match sig {
                        crate::sexp::context::RSignal::Error { message } => message.clone(),
                        _ => std::panic::resume_unwind(payload),
                    }
                } else {
                    std::panic::resume_unwind(payload)
                };

                let silent = as_bool_arg(silent_arg, rho);

                // Stock try() composes "Error in <deparsed call>: msg\n" from
                // the deparsed try() call (the condition call is the internal
                // doTryCatch frame, remapped to the try() call for the prefix).
                let dcall_sexp = crate::mainutils::deparse::deparse1s(_call);
                let dcall: String = if !dcall_sexp.is_null() && dcall_sexp != R_NilValue() {
                    let elt = crate::sexp::accessors::STRING_ELT(dcall_sexp, 0);
                    if elt.is_null() {
                        String::new()
                    } else {
                        let chars = crate::sexp::accessors::CHAR(elt);
                        if chars.is_null() {
                            String::new()
                        } else {
                            std::ffi::CStr::from_ptr(chars)
                                .to_string_lossy()
                                .into_owned()
                        }
                    }
                } else {
                    String::new()
                };

                let first_line_len = message
                    .split('\n')
                    .next()
                    .map(str::chars)
                    .map_or(0, |c| c.count());
                let mut prefix = format!("Error in {dcall} : ");
                // stock: 14L + 2*nchar(dcall, "w") + nchar(first line, "w") > 75L
                let width = 14 + 2 * dcall.chars().count() + first_line_len;
                if width > 75 {
                    prefix.push_str("\n  ");
                }
                let out_text = format!("{prefix}{message}\n");

                if !silent {
                    eprint!("{out_text}");
                }

                // The stored condition keeps the internal doTryCatch frame as
                // its call, exactly like stock tryCatch (which try() wraps).
                let condition = simple_error_condition(&message);
                let _cond_guard = protect(condition);

                // structure(class = "try-error", condition = e, msg):
                // a character vector of the composed message.
                let msg_sexp = crate::sexp::constructors::Rf_mkString(
                    CString::new(out_text.as_str()).unwrap_or_default().as_ptr(),
                );
                let _msg_guard = protect(msg_sexp);
                let klass = crate::sexp::constructors::Rf_allocVector(
                    crate::sexp::ffi::SEXPTYPE::STRSXP,
                    1,
                );
                let _klass_guard = protect(klass);
                crate::sexp::accessors::SET_STRING_ELT(
                    klass,
                    0,
                    crate::sexp::constructors::Rf_mkChar(
                        c"try-error".as_ptr() as *const libc::c_char
                    ),
                );
                crate::sexp::attrib_core::Rf_setAttrib(
                    msg_sexp,
                    crate::eval::attrib_core::R_ClassSymbol(),
                    klass,
                );
                crate::sexp::attrib_core::Rf_setAttrib(
                    msg_sexp,
                    Rf_install(c"condition".as_ptr() as *const libc::c_char),
                    condition,
                );
                msg_sexp
            }
        }
    }
}

/// Evaluate the `silent` argument of try() in `rho` to a logical flag.
unsafe fn as_bool_arg(sexp: SEXP, rho: SEXP) -> bool {
    unsafe {
        if sexp.is_null() || sexp == R_NilValue() || sexp == crate::sexp::globals::R_MissingArg() {
            return false;
        }
        let v = crate::eval::eval::Rf_eval(sexp, rho);
        if v.is_null() || v == R_NilValue() {
            return false;
        }
        // LOGICAL first element, NA treated as false (stock asLogical).
        crate::sexp::accessors::LOGICAL_ELT(v, 0) == 1
    }
}

pub unsafe fn do_withCallingHandlers(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let old_stack = condition_handler_stack();
        let new_stack = calling_handler_stack_from_args(CDR(args), rho, old_stack);
        set_condition_handler_stack(new_stack);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        set_condition_handler_stack(old_stack);

        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

// ---------------------------------------------------------------------------
// Exiting handlers (tryCatch) for warning conditions
// ---------------------------------------------------------------------------

thread_local! {
    /// Classes of the exiting handlers established by enclosing
    /// `tryCatch(...)` frames, innermost last.  Upstream vwarningcall()
    /// signals the simpleWarning through R_HandlerStack and an exiting
    /// handler takes over; the port's equivalent is to unwind (panic)
    /// out of `warning()` only when a frame actually registered a
    /// matching class.
    static TRY_CATCH_HANDLER_CLASSES: std::cell::RefCell<Vec<Vec<String>>> =
        std::cell::RefCell::new(Vec::new());
}
/// Panic payload for warning unwinds: `RSignal::Warning { message }`
/// (sexp::context) carries the warning out of `warning()` into an
/// enclosing `tryCatch(..., warning = )` frame; the R panic hook already
/// silences RSignal payloads, and every RSignal match site passes
/// unknown variants through.
/// Does any enclosing tryCatch frame register one of `classes`?
fn try_catch_wants(classes: &[&str]) -> bool {
    TRY_CATCH_HANDLER_CLASSES.with(|stack| {
        stack
            .borrow()
            .iter()
            .any(|frame| frame.iter().any(|c| classes.contains(&c.as_str())))
    })
}

fn condition_handler_stack() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| inst.error_state.handler_stack)
}

fn set_condition_handler_stack(stack: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.error_state.handler_stack = stack;
    });
}

unsafe fn calling_handler_stack_from_args(mut args: SEXP, rho: SEXP, old_stack: SEXP) -> SEXP {
    unsafe {
        let mut entries = Vec::new();
        while !args.is_null() && args != R_NilValue() {
            let Some(class_name) = tag_name(args) else {
                args = CDR(args);
                continue;
            };
            let handler = crate::eval::eval::Rf_eval(CAR(args), rho);
            if is_function_value(handler) {
                entries.push(calling_handler_entry(&class_name, handler, rho));
            }
            args = CDR(args);
        }

        let mut stack = old_stack;
        for entry in entries.into_iter().rev() {
            stack = Rf_cons(entry, stack);
        }
        stack
    }
}

unsafe fn calling_handler_entry(class_name: &str, handler: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let entry = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        if entry.is_null() {
            return R_NilValue();
        }
        let _entry_guard = protect(entry);
        SET_VECTOR_ELT(
            entry,
            0,
            Rf_mkString(CString::new(class_name).unwrap_or_default().as_ptr()),
        );
        SET_VECTOR_ELT(entry, 1, handler);
        SET_VECTOR_ELT(entry, 2, rho);
        entry
    }
}

unsafe fn signal_calling_handlers(condition: SEXP, rho: SEXP) {
    unsafe {
        let classes = crate::sexp::attrib_core::getAttrib(condition, Rf_install(c"class".as_ptr()));
        if classes.is_null() || classes == R_NilValue() || TYPEOF(classes) != SEXPTYPE::STRSXP {
            return;
        }

        let stack = condition_handler_stack();
        for class_idx in 0..XLENGTH(classes) {
            let class_name = elt_to_string(classes, class_idx);
            let mut current = stack;
            while !current.is_null() && current != R_NilValue() {
                let entry = CAR(current);
                if calling_handler_entry_class(entry).as_deref() == Some(class_name.as_str()) {
                    let handler = VECTOR_ELT(entry, 1);
                    call_condition_handler(handler, condition, rho);
                }
                current = CDR(current);
            }
        }
    }
}

unsafe fn calling_handler_entry_class(entry: SEXP) -> Option<String> {
    unsafe {
        if entry.is_null() || entry == R_NilValue() || TYPEOF(entry) != SEXPTYPE::VECSXP {
            return None;
        }
        let class = VECTOR_ELT(entry, 0);
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return None;
        }
        Some(elt_to_string(class, 0))
    }
}

unsafe fn call_condition_handler(handler: SEXP, condition: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(handler) == SEXPTYPE::CLOSXP {
            let args = Rf_cons(condition, R_NilValue());
            let call = Rf_cons(handler, args);
            if !call.is_null() {
                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            crate::eval::closure::applyClosure(call, handler, args, rho, R_NilValue(), TRUE)
        } else {
            let call = crate::sexp::constructors::Rf_lang2(handler, condition);
            crate::eval::eval::Rf_eval(call, rho)
        }
    }
}

/// R's `computeRestarts()` — compute available restarts for current condition.
pub unsafe fn do_computeRestarts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { restart_stack_as_list() }
}

/// R's `findRestart(name)` — find a restart by name.
pub unsafe fn do_findRestart(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = CAR(args);
        if name_arg.is_null() || name_arg == R_NilValue() {
            return R_NilValue();
        }
        let name = elt_to_string(name_arg, 0);
        find_restart_by_name(&name).unwrap_or_else(|| R_NilValue())
    }
}

/// R's `restarts()` — list available restarts.
pub unsafe fn do_restarts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { restart_stack_as_list() }
}

/// R's `invokeRestart(restart, ...)` — call a restart and return to its dynamic extent.
pub unsafe fn do_invokeRestart(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let restart_arg = CAR(args);
        let restart = resolve_restart_arg(restart_arg, true).unwrap_or_else(|| {
            base_error(format!(
                "no 'restart' '{}' found",
                restart_arg_name(restart_arg)
            ));
        });
        invoke_restart(restart, CDR(args), rho)
    }
}

/// R's `tryInvokeRestart(restart, ...)` — invoke a restart if one is active.
pub unsafe fn do_tryInvokeRestart(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let restart_arg = CAR(args);
        match resolve_restart_arg(restart_arg, true) {
            Some(restart) => invoke_restart(restart, CDR(args), rho),
            None => R_NilValue(),
        }
    }
}

/// R's `isRestart(x)` — check for a restart object.
pub unsafe fn do_isRestart(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        Rf_ScalarLogical(if is_restart_object(CAR(args)) {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `restartDescription(r)` — return the restart description, if any.
pub unsafe fn do_restartDescription(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let restart = CAR(args);
        if !is_restart_object(restart) {
            return R_NilValue();
        }
        let description = restart_field(restart, "description", 3);
        if description.is_null() || description == R_NilValue() {
            Rf_mkString(c"".as_ptr())
        } else {
            description
        }
    }
}

unsafe fn resolve_restart_arg(restart_arg: SEXP, require_active_object: bool) -> Option<SEXP> {
    unsafe {
        if restart_arg.is_null() || restart_arg == R_NilValue() {
            return None;
        }
        if TYPEOF(restart_arg) == SEXPTYPE::VECSXP {
            if require_active_object {
                return find_restart_by_object(restart_arg)
                    .or_else(|| base_error("restart not on stack"));
            }
            return if is_restart_object(restart_arg) {
                Some(restart_arg)
            } else {
                None
            };
        }
        if TYPEOF(restart_arg) == SEXPTYPE::STRSXP {
            return find_restart_by_name(&elt_to_string(restart_arg, 0));
        }
        None
    }
}

unsafe fn restart_arg_name(restart_arg: SEXP) -> String {
    unsafe {
        if !restart_arg.is_null()
            && restart_arg != R_NilValue()
            && TYPEOF(restart_arg) == SEXPTYPE::STRSXP
        {
            elt_to_string(restart_arg, 0)
        } else {
            String::new()
        }
    }
}

unsafe fn invoke_restart(restart: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let handler = restart_handler(restart);
        let value = if is_function_value(handler) {
            call_function_with_args(handler, args, rho)
        } else {
            R_NilValue()
        };
        std::panic::panic_any(crate::sexp::context::RSignal::Restart(value));
    }
}

unsafe fn call_function_with_args(handler: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let call = Rf_cons(handler, args);
        if !call.is_null() {
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        if TYPEOF(handler) == SEXPTYPE::CLOSXP {
            crate::eval::closure::applyClosure(call, handler, args, rho, R_NilValue(), TRUE)
        } else {
            crate::eval::eval::Rf_eval(call, rho)
        }
    }
}

fn restart_stack() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| inst.error_state.restart_stack)
}

fn set_restart_stack(stack: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.error_state.restart_stack = stack;
    });
}

unsafe fn restart_stack_as_list() -> SEXP {
    unsafe {
        let mut restarts = Vec::new();
        let mut current = restart_stack();
        while !current.is_null() && current != R_NilValue() {
            restarts.push(CAR(current));
            current = CDR(current);
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, restarts.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, restart) in restarts.iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, *restart);
        }
        result
    }
}

unsafe fn find_restart_by_name(name: &str) -> Option<SEXP> {
    unsafe {
        let mut current = restart_stack();
        while !current.is_null() && current != R_NilValue() {
            let restart = CAR(current);
            if restart_name(restart).as_deref() == Some(name) {
                return Some(restart);
            }
            current = CDR(current);
        }
        None
    }
}

unsafe fn find_restart_by_object(needle: SEXP) -> Option<SEXP> {
    unsafe {
        let mut current = restart_stack();
        while !current.is_null() && current != R_NilValue() {
            let restart = CAR(current);
            if restart == needle {
                return Some(restart);
            }
            current = CDR(current);
        }
        None
    }
}

unsafe fn restart_name(restart: SEXP) -> Option<String> {
    unsafe {
        if restart.is_null() || restart == R_NilValue() || TYPEOF(restart) != SEXPTYPE::VECSXP {
            return None;
        }
        let name = restart_field(restart, "name", 0);
        if name.is_null() || name == R_NilValue() || TYPEOF(name) != SEXPTYPE::STRSXP {
            return None;
        }
        Some(elt_to_string(name, 0))
    }
}

unsafe fn restart_handler(restart: SEXP) -> SEXP {
    unsafe { restart_field(restart, "handler", 2) }
}

unsafe fn restart_field(restart: SEXP, field_name: &str, fallback_index: R_xlen_t) -> SEXP {
    unsafe {
        if restart.is_null() || restart == R_NilValue() || TYPEOF(restart) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names = crate::sexp::attrib_core::getAttrib(restart, Rf_install(c"names".as_ptr()));
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            let limit = XLENGTH(names).min(XLENGTH(restart));
            for index in 0..limit {
                if elt_to_string(names, index) == field_name {
                    return VECTOR_ELT(restart, index);
                }
            }
        }
        if fallback_index < XLENGTH(restart) {
            VECTOR_ELT(restart, fallback_index)
        } else {
            R_NilValue()
        }
    }
}

unsafe fn restart_entry(name: &str, handler: SEXP) -> SEXP {
    unsafe {
        let restart = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
        if restart.is_null() {
            return R_NilValue();
        }
        let _restart_guard = protect(restart);
        SET_VECTOR_ELT(
            restart,
            0,
            Rf_mkString(CString::new(name).unwrap_or_default().as_ptr()),
        );
        SET_VECTOR_ELT(restart, 1, R_NilValue());
        SET_VECTOR_ELT(restart, 2, handler);
        SET_VECTOR_ELT(restart, 3, Rf_mkString(c"".as_ptr()));
        SET_VECTOR_ELT(restart, 4, R_NilValue());
        SET_VECTOR_ELT(restart, 5, R_NilValue());

        let names = string_vector(&[
            "name".to_string(),
            "exit".to_string(),
            "handler".to_string(),
            "description".to_string(),
            "test".to_string(),
            "interactive".to_string(),
        ]);
        crate::sexp::attrib_core::setAttrib(restart, Rf_install(c"names".as_ptr()), names);
        crate::sexp::attrib_core::setAttrib(
            restart,
            Rf_install(c"class".as_ptr()),
            Rf_mkString(c"restart".as_ptr()),
        );
        restart
    }
}

unsafe fn is_restart_object(value: SEXP) -> bool {
    unsafe {
        !value.is_null()
            && value != R_NilValue()
            && TYPEOF(value) == SEXPTYPE::VECSXP
            && inherits_class(value, "restart")
    }
}

// ---------------------------------------------------------------------------
// Error handling: stop, warning, message, tryCatch, inherits, exists, get, assign
// ---------------------------------------------------------------------------

/// R's `stop(...)` — raise error.
pub unsafe fn do_stop(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = elt_to_string(CAR(args), 0);
        // Upstream `stop()` signals with the call of the frame that invoked
        // stop (findCall skips stop's own closure frame). stop is a builtin
        // here, so the innermost context call — R_getCurrentCall() — is that
        // caller's call; at top level it is R_NilValue and the render stays
        // "Error: <message>" exactly like stock R.
        crate::mainutils::errors::errorcall_str(crate::mainutils::errors::R_getCurrentCall(), &s);
    }
}

/// R's `warning(...)` — issue warning.
pub unsafe fn do_warning(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let warning_text =
            condition_message_text(args, &["call.", "immediate.", "noBreaks.", "domain"]);
        let condition = simple_condition(&warning_text, &["simpleWarning", "warning", "condition"]);
        {
            let _cond_guard = protect(condition);
            signal_calling_handlers(condition, rho);
        }

        // Exiting handlers: when an enclosing tryCatch(...) registered a
        // handler for one of this condition's classes, unwind into it
        // (upstream vwarningcall signals through R_HandlerStack and the
        // exiting handler takes over; the warning is then neither
        // printed nor collected).
        if try_catch_wants(&["simpleWarning", "warning", "condition"]) {
            std::panic::panic_any(crate::sexp::context::RSignal::Warning {
                message: warning_text,
            });
        }

        // Stock defers printing and renders via PrintWarnings()
        // (errors.c:615-631): header "Warning message:", then either
        // "In <dcall> : <msg>" (warning attributed to the enclosing call —
        // stock findCall(); R_getCurrentCall() is the builtin-call
        // equivalent here, R_NilValue at top level) with the LONGWARN
        // one-space wrap, or "<msg> " without a call. The port emits at
        // signal time into the stderr capture so builtin warning() output
        // stays in signal order with message() output (case 372); C-internal
        // warningcall() sites keep the deferred collection path.
        let wcall = crate::mainutils::errors::R_getCurrentCall();
        let call_ok = !wcall.is_null() && wcall != R_NilValue();
        let mut text = String::from("Warning message:\n");
        if call_ok {
            let dcall = crate::mainutils::errors::warning_dcall(wcall);
            text.push_str("In ");
            text.push_str(&dcall);
            text.push_str(" :");
            let msgline1 = warning_text.split('\n').next().map_or(0, str::len);
            if 6 + dcall.len() + msgline1 > 75 {
                text.push('\n');
                text.push(' ');
            }
            text.push(' ');
        }
        text.push_str(&warning_text);
        text.push(if call_ok { '\n' } else { ' ' });
        text.push('\n');
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stderr(&text);
        } else {
            eprint!("{text}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        // Stock returns the message string (the R closure wraps it in
        // invisible()); print(warning("w")) therefore renders `[1] "w"`.
        let c_msg = CString::new(warning_text.as_str()).unwrap_or_default();
        Rf_mkString(c_msg.as_ptr())
    }
}

/// R's `message(...)` — print message.
pub unsafe fn do_message(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let text = condition_message_text(args, &["domain", "appendLF"]);
        let message = format!("{}\n", text);
        let condition = simple_condition(&message, &["simpleMessage", "message", "condition"]);
        signal_calling_handlers(condition, rho);

        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stderr(&message);
        } else {
            eprint!("{message}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `inherits(x, what)` — check class.
pub unsafe fn do_inherits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let what = CAR(CDR(args));
        if x.is_null() || what.is_null() {
            return Rf_ScalarLogical(FALSE);
        }
        let class_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if class_attr.is_null() || TYPEOF(class_attr) != SEXPTYPE::STRSXP {
            return Rf_ScalarLogical(FALSE);
        }
        let target = elt_to_string(what, 0);
        let n = XLENGTH(class_attr);
        for i in 0..n {
            if elt_to_string(class_attr, i) == target {
                return Rf_ScalarLogical(TRUE);
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

unsafe fn simple_error_condition(message: &str) -> SEXP {
    unsafe {
        // stock: conditions caught by tryCatch's error handler carry the
        // internal doTryCatch(return(expr), name, parentenv, handler) frame
        // as their call.
        let s = |name: &str| Rf_install(CString::new(name).unwrap_or_default().as_ptr());
        let inner = crate::sexp::constructors::Rf_lang2(s("return"), s("expr"));
        let call = crate::sexp::constructors::Rf_lang5(
            s("doTryCatch"),
            inner,
            s("name"),
            s("parentenv"),
            s("handler"),
        );
        let _call_guard = protect(call);
        let c_msg = CString::new(message).unwrap_or_default();
        crate::mainutils::errors::R_makeErrorCondition(
            call,
            c"simpleError".as_ptr() as *const libc::c_char,
            std::ptr::null(),
            0,
            c_msg.as_ptr(),
        )
    }
}

unsafe fn simple_warning_condition(message: &str) -> SEXP {
    unsafe {
        // stock: warnings caught by tryCatch's warning handler carry the
        // internal doTryCatch(return(expr), name, parentenv, handler) frame
        // as their call (print.condition renders it: `<simpleWarning in
        // doTryCatch(...): msg>`).
        let s = |name: &str| Rf_install(CString::new(name).unwrap_or_default().as_ptr());
        let inner = crate::sexp::constructors::Rf_lang2(s("return"), s("expr"));
        let call = crate::sexp::constructors::Rf_lang5(
            s("doTryCatch"),
            inner,
            s("name"),
            s("parentenv"),
            s("handler"),
        );
        let _call_guard = protect(call);
        let c_msg = CString::new(message).unwrap_or_default();
        crate::mainutils::errors::R_makeWarningCondition(
            call,
            c"simpleWarning".as_ptr() as *const libc::c_char,
            std::ptr::null(),
            0,
            c_msg.as_ptr(),
        )
    }
}
unsafe fn simple_condition(message: &str, classes: &[&str]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let msg = Rf_mkString(CString::new(message).unwrap_or_default().as_ptr());
        SET_VECTOR_ELT(result, 0, msg);

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !names.is_null() {
            let _np = protect(names);
            SET_STRING_ELT(
                names,
                0,
                Rf_mkChar(CString::new("message").unwrap_or_default().as_ptr()),
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("message").unwrap_or_default().as_ptr()),
            msg,
        );

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        if !class.is_null() {
            let _cp = protect(class);
            for (i, name) in classes.iter().enumerate() {
                SET_STRING_ELT(
                    class,
                    i as R_xlen_t,
                    Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class,
            );
        }

        result
    }
}

/// R's `tryCatch(expr, ...)` — exiting-handler support.  Handlers for any
/// condition class may be supplied; warning conditions raised in the body
/// unwind here through `RSignal::Warning`, error panics through the existing
/// RSignal/RError payloads (upstream: R_TryCatch / vwarningcall).
pub unsafe fn do_tryCatch(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() {
            return R_NilValue();
        }

        // Evaluate every handler up front, like upstream tryCatch's
        // `handlers <- list(...)`, and register the classes so warnings
        // raised in the body know whether to unwind here.
        let mut handlers: Vec<(String, SEXP)> = Vec::new();
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if let Some(tag) = tag_name(current) {
                let handler = crate::eval::eval::Rf_eval(CAR(current), rho);
                if !handler.is_null() && handler != R_NilValue() {
                    handlers.push((tag, handler));
                }
            }
            current = CDR(current);
        }
        let _handler_guards: Vec<_> = handlers.iter().map(|(_, h)| protect(*h)).collect();
        TRY_CATCH_HANDLER_CLASSES.with(|stack| {
            stack
                .borrow_mut()
                .push(handlers.iter().map(|(tag, _)| tag.clone()).collect());
        });
        struct PopHandlers;
        impl Drop for PopHandlers {
            fn drop(&mut self) {
                TRY_CATCH_HANDLER_CLASSES.with(|stack| {
                    stack.borrow_mut().pop();
                });
            }
        }
        let _pop_guard = PopHandlers;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        // Pop before invoking any handler so a warning raised inside a
        // handler does not match this frame again.
        drop(_pop_guard);

        match result {
            Ok(val) => val,
            Err(payload) => {
                // Warning conditions: route to a matching exiting handler
                // (simpleWarning/warning/condition); otherwise pass the
                // panic on to outer frames.
                let payload = match payload.downcast::<crate::sexp::context::RSignal>() {
                    Ok(signal) => match *signal {
                        crate::sexp::context::RSignal::Warning { message } => {
                            let classes = ["simpleWarning", "warning", "condition"];
                            let matching = handlers
                                .iter()
                                .find(|(tag, _)| classes.contains(&tag.as_str()));
                            if let Some((_, handler)) = matching {
                                let condition = simple_warning_condition(&message);
                                let _cond_guard = protect(condition);
                                let call = crate::sexp::constructors::Rf_lang2(*handler, condition);
                                return crate::eval::eval::Rf_eval(call, rho);
                            }
                            std::panic::resume_unwind(Box::new(
                                crate::sexp::context::RSignal::Warning { message },
                            ));
                        }
                        other => Box::new(other) as Box<dyn std::any::Any + Send>,
                    },
                    Err(payload) => payload,
                };

                let message = match payload.downcast::<crate::sexp::context::RSignal>() {
                    Ok(signal) => match *signal {
                        crate::sexp::context::RSignal::Error { message } => message,
                        other => std::panic::panic_any(other),
                    },
                    Err(payload) => match payload.downcast::<crate::sexp::context::RError>() {
                        Ok(err) => err.message.clone(),
                        Err(payload) => std::panic::resume_unwind(payload),
                    },
                };

                let Some(handler) = handlers
                    .iter()
                    .find(|(tag, _)| tag == "error")
                    .map(|(_, handler)| *handler)
                else {
                    std::panic::panic_any(crate::sexp::context::RError { message });
                };
                let condition = simple_error_condition(&message);
                let _cond_guard = protect(condition);
                let call = crate::sexp::constructors::Rf_lang2(handler, condition);
                crate::eval::eval::Rf_eval(call, rho)
            }
        }
    }
}

/// R's `exists(x, envir)` — check name exists.
pub unsafe fn do_exists(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
        let env = environment_arg_or_default(args, &["envir", "where", "frame"], 1, rho);
        let inherits = named_logical_arg(args, "inherits").unwrap_or(true);
        let mode_arg = {
            let named = arg_by_name_or_position(args, &["mode"], 2);
            if !named.is_null() && named != R_NilValue() {
                named
            } else {
                let second = arg_by_name_or_position(args, &[], 1);
                if !second.is_null() && second != R_NilValue() && TYPEOF(second) == SEXPTYPE::STRSXP
                {
                    second
                } else {
                    R_NilValue()
                }
            }
        };
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() || XLENGTH(mode_arg) == 0 {
            "any".to_string()
        } else {
            elt_to_string(mode_arg, 0)
        };
        let found = if crate::eval::builtin::is_hidden_builtin_name(&name) {
            false
        } else if mode == "function" {
            let value = if inherits {
                crate::sexp::envir::R_findVar(sym, env)
            } else {
                crate::sexp::envir::R_findVarInFrame(env, sym)
            };
            crate::eval::builtin::has_builtin_handler(&name) || is_function_value(value)
        } else {
            crate::sexp::envir::binding_exists_raw(env, sym, inherits)
                || crate::eval::builtin::has_builtin_handler(&name)
        };
        Rf_ScalarLogical(if found { TRUE } else { FALSE })
    }
}

/// R's `find(what, mode = "any")` — locate a name on the search path.
pub unsafe fn do_find(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let what_arg = arg_by_name_or_position(args, &["what"], 0);
        if what_arg.is_null() || what_arg == R_NilValue() || XLENGTH(what_arg) == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let name = elt_to_string(what_arg, 0);
        if name.is_empty() || crate::eval::builtin::is_hidden_builtin_name(&name) {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let mode_arg = arg_by_name_or_position(args, &["mode"], 1);
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() || XLENGTH(mode_arg) == 0 {
            "any".to_string()
        } else {
            elt_to_string(mode_arg, 0)
        };
        let want_function = mode == "function";
        let numeric = logical_arg_by_name_or_position(args, "numeric", 2).unwrap_or(false);

        let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
        let mut matches = Vec::new();
        for (label, env) in search_path_entries() {
            if find_matches_mode(env, sym, &name, want_function) {
                matches.push(label);
            }
        }

        if numeric {
            return find_numeric_result(&matches);
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
        for (i, value) in matches.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

unsafe fn find_numeric_result(matches: &[String]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
        for (i, value) in matches.iter().enumerate() {
            *INTEGER(result).add(i) = (i + 1) as c_int;
            SET_STRING_ELT(
                names,
                i as R_xlen_t,
                Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}

unsafe fn find_matches_mode(env: SEXP, symbol: SEXP, name: &str, want_function: bool) -> bool {
    unsafe {
        if env.is_null() || env == R_NilValue() {
            return false;
        }
        let value = crate::sexp::envir::R_findVarInFrame(env, symbol);
        let is_base_builtin = env == crate::sexp::globals::R_BaseEnv()
            && crate::eval::builtin::has_builtin_handler(name);
        if value == R_UnboundValue() {
            return !want_function && is_base_builtin;
        }
        if want_function {
            is_function_value(value) || is_base_builtin
        } else {
            true
        }
    }
}

/// R's `get(x, envir)` — get value.
pub unsafe fn do_get(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let env = environment_arg_or_default(args, &["envir", "pos"], 1, rho);
        let inherits = named_logical_arg(args, "inherits").unwrap_or(true);
        let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
        if inherits {
            crate::sexp::envir::R_findVar(sym, env)
        } else {
            crate::sexp::envir::R_findVarInFrame(env, sym)
        }
    }
}

/// R's `assign(x, value, envir)` — assign value.
pub unsafe fn do_assign(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let val = arg_by_name_or_position(args, &["value"], 1);
        if val.is_null() {
            return R_NilValue();
        }
        let env = environment_arg_or_default(args, &["envir", "pos"], 2, rho);
        crate::sexp::envir::defineVar(
            Rf_install(CString::new(name).unwrap_or_default().as_ptr()),
            val,
            env,
        );
        crate::sexp::globals::set_R_Visible(FALSE);
        val
    }
}

pub(crate) unsafe fn symbol_name(sym: SEXP) -> Option<String> {
    unsafe {
        if sym.is_null() || sym == R_NilValue() || TYPEOF(sym) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(sym);
        if printname.is_null() || printname == R_NilValue() {
            return None;
        }
        let ptr = CHAR(printname);
        if ptr.is_null() {
            return None;
        }
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

pub(crate) unsafe fn logical_arg(arg: SEXP, default: bool) -> bool {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) < 1 {
            return default;
        }
        if TYPEOF(arg) == SEXPTYPE::LGLSXP {
            return *LOGICAL(arg) != FALSE;
        }
        if TYPEOF(arg) == SEXPTYPE::INTSXP {
            return *INTEGER(arg) != 0;
        }
        default
    }
}

/// R's `ls(envir)` — list objects.
pub unsafe fn do_ls(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut env = rho;
        let mut all_names = false;
        let mut sorted = true;

        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let arg = CAR(cell);
            let name = symbol_name(TAG(cell));
            match name.as_deref() {
                Some("name") | Some("pos") | Some("envir") => {
                    if TYPEOF(arg) == SEXPTYPE::ENVSXP {
                        env = arg;
                    }
                }
                Some("all.names") => all_names = logical_arg(arg, all_names),
                Some("sorted") => sorted = logical_arg(arg, sorted),
                _ if TYPEOF(arg) == SEXPTYPE::ENVSXP => env = arg,
                _ => {}
            }
            cell = CDR(cell);
        }

        let mut names = Vec::new();
        if TYPEOF(env) == SEXPTYPE::ENVSXP {
            let mut frame = FRAME(env);
            while !frame.is_null() && frame != R_NilValue() {
                let value = CAR(frame);
                if value != crate::sexp::globals::R_UnboundValue()
                    && let Some(name) = symbol_name(TAG(frame))
                    && (all_names || !name.starts_with('.'))
                {
                    names.push(name);
                }
                frame = CDR(frame);
            }
        }

        if sorted {
            names.sort();
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        for (i, name) in names.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }
        result
    }
}

/// R's `rm(list, envir)` — remove objects.
pub unsafe fn do_rm(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let list = arg_by_name_or_position(args, &["list"], 0);
        if list.is_null() || TYPEOF(list) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let env = environment_arg_or_default(args, &["envir"], 1, rho);
        for i in 0..XLENGTH(list) {
            let sym = Rf_install(
                CString::new(elt_to_string(list, i))
                    .unwrap_or_default()
                    .as_ptr(),
            );
            crate::sexp::envir::remove_binding_raw(env, sym);
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Complete error system: condition handling
// ---------------------------------------------------------------------------

/// R's `conditionMessage(cond)` — get message from condition object.
pub unsafe fn do_conditionMessage(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cond = CAR(args);
        if cond.is_null() || cond == R_NilValue() {
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }
        // Stock conditionMessage.condition is `c$message`: the element of
        // the condition list whose name is "message" (not an attribute).
        if let Some(msg) =
            crate::mainutils::essentials::tables::list_element_by_name(cond, "message")
        {
            if !msg.is_null() && msg != R_NilValue() && TYPEOF(msg) == SEXPTYPE::STRSXP {
                return msg;
            }
        }
        // Fall back to the explicit "message" attribute for non-list
        // condition representations.
        let msg_sym = Rf_install(CString::new("message").unwrap_or_default().as_ptr());
        let msg = crate::sexp::attrib_core::getAttrib(cond, msg_sym);
        if !msg.is_null() && msg != R_NilValue() && TYPEOF(msg) == SEXPTYPE::STRSXP {
            return msg;
        }
        Rf_mkString(CString::new("").unwrap_or_default().as_ptr())
    }
}

/// R's `conditionCall(cond)` — get call from condition object.
pub unsafe fn do_conditionCall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cond = CAR(args);
        if cond.is_null() || cond == R_NilValue() {
            return R_NilValue();
        }
        let call_sym = Rf_install(CString::new("call").unwrap_or_default().as_ptr());
        let call_val = crate::sexp::attrib_core::getAttrib(cond, call_sym);
        if !call_val.is_null() && call_val != R_NilValue() {
            return call_val;
        }
        R_NilValue()
    }
}

/// R's `simpleError(message, call)` — create a simple error condition.
pub unsafe fn do_simpleError(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let message_arg = CAR(args);
        let call_arg = CAR(CDR(args));
        let message = if message_arg.is_null() || message_arg == R_NilValue() {
            String::new()
        } else {
            elt_to_string(message_arg, 0)
        };
        // Create a simple list with class "simpleError" and "error" and "condition"
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let msg_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !msg_vec.is_null() {
            let cstr = CString::new(message).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*msg_vec).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
        }
        SET_VECTOR_ELT(result, 0, msg_vec);
        // Set names
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !names.is_null() {
            let cstr = CString::new("message").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }
        // Set class
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        if !class.is_null() {
            let classes = ["simpleError", "error", "condition"];
            for (i, &c) in classes.iter().enumerate() {
                let cs = CString::new(c).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
                if !charsxp.is_null() {
                    let data = (*class).gengc_next_node as *mut SEXP;
                    *data.add(i) = charsxp;
                }
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class,
            );
        }
        result
    }
}

/// R's `simpleWarning(message, call)` — create a simple warning condition.
pub unsafe fn do_simpleWarning(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let message_arg = CAR(args);
        let message = if message_arg.is_null() || message_arg == R_NilValue() {
            String::new()
        } else {
            elt_to_string(message_arg, 0)
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let msg_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !msg_vec.is_null() {
            let cstr = CString::new(message).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*msg_vec).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
        }
        SET_VECTOR_ELT(result, 0, msg_vec);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !names.is_null() {
            let cstr = CString::new("message").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        if !class.is_null() {
            let classes = ["simpleWarning", "warning", "condition"];
            for (i, &c) in classes.iter().enumerate() {
                let cs = CString::new(c).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
                if !charsxp.is_null() {
                    let data = (*class).gengc_next_node as *mut SEXP;
                    *data.add(i) = charsxp;
                }
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class,
            );
        }
        result
    }
}

/// R's `withRestarts(expr, ...)` — evaluate an expression with dynamic restarts.
pub unsafe fn do_withRestarts(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let old_stack = restart_stack();
        let new_stack = restart_stack_from_args(CDR(args), rho, old_stack);
        set_restart_stack(new_stack);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        set_restart_stack(old_stack);

        match result {
            Ok(value) => value,
            Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
                Ok(signal) => match *signal {
                    crate::sexp::context::RSignal::Restart(value) => value,
                    other => std::panic::panic_any(other),
                },
                Err(payload) => std::panic::resume_unwind(payload),
            },
        }
    }
}

unsafe fn restart_stack_from_args(mut args: SEXP, rho: SEXP, old_stack: SEXP) -> SEXP {
    unsafe {
        let mut entries = Vec::new();
        while !args.is_null() && args != R_NilValue() {
            let Some(name) = tag_name(args) else {
                args = CDR(args);
                continue;
            };
            let handler = crate::eval::eval::Rf_eval(CAR(args), rho);
            entries.push(restart_entry(&name, handler));
            args = CDR(args);
        }

        let mut stack = old_stack;
        for entry in entries.into_iter().rev() {
            stack = Rf_cons(entry, stack);
        }
        stack
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tryCatch(..., warning = ) exiting handlers must catch warnings
    /// raised in the body; unmatched classes must not catch.
    #[test]
    fn test_try_catch_warning_handler() {
        let mut session = crate::sexp::session::RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            "tryCatch(warning('x'), warning = function(e) paste('caught:', conditionMessage(e)))",
        );
        let value = result
            .map(|s| unsafe {
                std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::STRING_ELT(s.as_raw(), 0),
                ))
                .to_string_lossy()
                .into_owned()
            })
            .unwrap_or_default();
        assert_eq!(value, "caught: x");
        // The caught warning is neither printed nor collected.
        assert!(!output.stderr.contains("Warning message"));

        // An error-only frame does not catch warnings: the warning falls
        // through to the default print path and tryCatch returns the
        // warning() return value — the message string, invisibly (stock
        // do_warning returns CAR(args)).
        let (result, output, _) = session.eval_script_with_output_capture(
            "identical(tryCatch(warning('y'), error = function(e) 'E'), 'y')",
        );
        let identical = result
            .map(|s| unsafe { crate::sexp::accessors::LOGICAL(s.as_raw()).read() == TRUE })
            .unwrap_or(false);
        assert!(identical);
        assert!(output.stderr.contains("Warning message"));
    }

    /// suppressWarnings keeps muting warnings (capture-based), including
    /// with an exiting warning handler in scope of the wider script.
    #[test]
    fn test_suppress_warnings_still_mutes() {
        let mut session = crate::sexp::session::RSession::new();
        let (result, output, _) =
            session.eval_script_with_output_capture("suppressWarnings(warning('quiet'))");
        assert!(result.is_ok());
        assert!(!output.stderr.contains("quiet"));
    }
}
