//! Rf_eval entry points and do_* builtins (eval, Rf_eval, withVisible, Recall) — extracted verbatim from eval/eval.rs

use super::*;

// Application of closures, specials, and builtins lives in `eval::apply`.
// ---------------------------------------------------------------------------
// Owner-scoped evaluation API
// ---------------------------------------------------------------------------

/// Evaluate an R expression in an environment.
///
/// This wraps the raw FFI `Rf_eval` and provides a `Result` return type.
#[must_use = "eval returns a Result that should be checked"]
/// # Safety
/// Activate the live owner of all inputs and retain their reachable graphs
/// through allocation and R reentry. No Rust payload loan may cross execution.
pub unsafe fn eval<'a>(e: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    unsafe {
        /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
        eval_safe(e, rho)
    }
}

/// Extract the name of a symbol for error messages.
pub(crate) unsafe fn get_symbol_name(sym: SEXP) -> String {
    let pname = unsafe { crate::sexp::accessors::PRINTNAME(sym) };
    if pname.is_null() {
        return "???".to_string();
    }
    let s = unsafe { crate::sexp::accessors::CHAR(pname) };
    if s.is_null() {
        return "???".to_string();
    }
    unsafe { std::ffi::CStr::from_ptr(s) }
        .to_str()
        .unwrap_or("???")
        .to_string()
}

// ---------------------------------------------------------------------------
// Raw eval function — thin shim delegating to eval_safe
// ---------------------------------------------------------------------------

/// Evaluate an R expression in an environment.
///
/// This is the equivalent of R's `eval()` from src/main/eval.c.
/// It is the main dispatch function of the interpreter.
///
/// # Safety
///
/// `e` and `rho` must be valid SEXP pointers (or null).
#[must_use]
pub(crate) unsafe fn Rf_eval(e: SEXP, rho: SEXP) -> SEXP {
    if e.is_null() || rho.is_null() {
        return unsafe { R_NilValue() };
    }
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|err| crate::sexp::context::r_error(err.to_string()));
    let factory = crate::sexp::object::SessionNodeFactory::new(owner);
    let expr = factory
        .wrap(e)
        .unwrap_or_else(|err| crate::sexp::context::r_error(err.to_string()));
    let env = factory
        .wrap(rho)
        .unwrap_or_else(|err| crate::sexp::context::r_error(err.to_string()));
    match unsafe {
        /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
        eval_expr(expr, env)
    } {
        Ok(result) => unsafe { super::super::jit::handle_exec_continuation(result.as_raw()) },
        Err(msg) => {
            std::panic::panic_any(crate::sexp::context::RSignal::Error { message: msg });
        }
    }
}

pub(crate) fn is_simple_warning_hook_call(expr: Sexp<'_>) -> bool {
    let Ok(fun) = expr.try_car() else {
        return false;
    };
    if !fun.clone().is_symbol() {
        return false;
    }
    matches!(
        symbol_name_from_ptr(fun.as_raw()).as_deref(),
        Some(".signalSimpleWarning")
    )
}

/// Raw entry for operations being migrated to owned evaluation.
pub(crate) unsafe fn eval_inner(e: SEXP, rho: SEXP) -> SEXP {
    unsafe { Rf_eval(e, rho) }
}

// ---------------------------------------------------------------------------
// eval with visibility preservation (for C code calling eval)
// ---------------------------------------------------------------------------

/// Evaluate an expression, preserving the R_Visible flag.
///
/// This is the equivalent of R's `evalKeepVis()` from errors.c.
pub(crate) unsafe fn eval_keep_vis(e: SEXP, rho: SEXP) -> SEXP {
    let _visibility = super::super::runtime::VisibilityGuard::new();
    let val = unsafe { Rf_eval(e, rho) };
    val
}

// ---------------------------------------------------------------------------
// do_withVisible -- evaluate and return list(value, visible)
// ---------------------------------------------------------------------------

/// Evaluate expression and return `list(value = <result>, visible = <flag>)`.
///
/// Ported from R's `do_withVisible()` in eval.c.
/// This is a special `.Internal`.
pub(crate) unsafe fn do_withVisible(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::attrib_core::{R_NamesSymbol, setAttrib};
    use crate::sexp::accessors::{CAR, SET_STRING_ELT, SET_VECTOR_ELT};
    use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector, Rf_mkChar};
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::protect::protect;
    use std::os::raw::c_char;

    unsafe {
        let x = Rf_eval(CAR(args), rho);
        let _x_guard = protect(x);

        let ret = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _ret_guard = protect(ret);

        let nm = Rf_allocVector(SEXPTYPE::STRSXP, 2);
        let _names_guard = protect(nm);

        SET_STRING_ELT(nm, 0, Rf_mkChar(b"value\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"visible\0".as_ptr() as *const c_char));

        SET_VECTOR_ELT(ret, 0, x);
        SET_VECTOR_ELT(ret, 1, Rf_ScalarLogical(super::super::runtime::visible()));

        setAttrib(ret, R_NamesSymbol(), nm);

        ret
    }
}

// ---------------------------------------------------------------------------
// do_recall -- re-invoke the calling generic function
// ---------------------------------------------------------------------------

/// Implements R's `Recall()` — re-invokes the calling generic.
///
/// Ported from R's `do_recall()` in eval.c.
/// This is a special `.Internal`.
pub(crate) unsafe fn do_recall(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::closure::applyClosure;
    use crate::mainutils::errors::Rf_error;
    use crate::sexp::accessors::{CAR, TYPEOF};
    use crate::sexp::context::ctxt_flags::CTXT_RETURN;
    use crate::sexp::envir::findFun;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_NilValue;
    use std::os::raw::c_char;

    unsafe {
        let instance = crate::sexp::instance::with_required_current_instance(|instance| instance);
        let pin = crate::sexp::context::pin_context_owner_in(instance);
        let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
        let capture = |value: SEXP| {
            if value.is_null() {
                None
            } else {
                Some(
                    owner
                        .sexp(value)
                        .and_then(Sexp::into_owned)
                        .unwrap_or_else(|error| {
                            crate::sexp::context::r_error(format!("invalid Recall value: {error}"))
                        }),
                )
            }
        };
        let projection = |value: &Option<Sexp<'static>>| {
            value.as_ref().map_or(std::ptr::null_mut(), Sexp::as_raw)
        };
        let top = super::super::runtime::global_context();
        if top.is_null() {
            Rf_error(b"'Recall' called from outside a closure\0".as_ptr() as *const c_char);
        }
        let top_cell =
            crate::sexp::context::retain_context_in(instance, top).unwrap_or_else(|| {
                crate::sexp::context::r_error("Recall context no longer belongs to its owner")
            });
        let parent = (*top_cell.get()).sysparent.owned();
        let parent_pointer = projection(&parent);

        // Both searches finish before evaluating a function expression or
        // active binding. Their original context cells stay physically owned.
        let mut argument_context = None;
        let mut function_context = None;
        let mut current = top;
        while !current.is_null() {
            let cell =
                crate::sexp::context::retain_context_in(instance, current).unwrap_or_else(|| {
                    crate::sexp::context::r_error("Recall context no longer belongs to its owner")
                });
            let (flag, environment, next) = {
                let context = &*cell.get();
                (
                    context.callflag,
                    context.cloenv.as_raw(),
                    context.nextcontext,
                )
            };
            if flag & CTXT_RETURN != 0 {
                if argument_context.is_none() && environment == rho {
                    argument_context = Some(cell.clone());
                }
                if function_context.is_none() && environment == parent_pointer {
                    function_context = Some(cell);
                }
            }
            if argument_context.is_some() && function_context.is_some() {
                break;
            }
            current = next;
        }
        let recall_args = argument_context
            .as_ref()
            .map(|context| (*context.get()).promiseargs.owned())
            .unwrap_or_else(|| capture(args));
        let Some(function_context) = function_context else {
            crate::mainutils::errors::errorcall_str(
                R_NilValue(),
                "'Recall' called from outside a closure",
            );
        };
        let (source_call, function, environment) = {
            let context = &*function_context.get();
            (
                context.call.owned(),
                context.callfun.owned(),
                context.sysparent.owned(),
            )
        };
        let source = projection(&source_call);
        let environment_pointer = projection(&environment);
        let function_pointer = projection(&function);
        let function = if !function_pointer.is_null() && function_pointer != R_NilValue() {
            function
        } else {
            let head = CAR(source);
            let head_owned = capture(head);
            let head = projection(&head_owned);
            let function = if TYPEOF(head) == SEXPTYPE::SYMSXP {
                findFun(head, environment_pointer)
            } else {
                Rf_eval(head, environment_pointer)
            };
            crate::sexp::context::require_context_owner_live(pin.as_ref());
            capture(function)
        };
        let function_pointer = projection(&function);
        if TYPEOF(function_pointer) != SEXPTYPE::CLOSXP {
            Rf_error(b"'Recall' called from outside a closure\0".as_ptr() as *const c_char);
        }
        let result = applyClosure(
            source,
            function_pointer,
            projection(&recall_args),
            environment_pointer,
            R_NilValue(),
            1,
        );
        crate::sexp::context::require_context_owner_live(pin.as_ref());
        result
    }
}
