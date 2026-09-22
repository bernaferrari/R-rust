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
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_lang2,
    Rf_mkChar, Rf_mkString,
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
        crate::mainutils::errors::set_error_call_less(false);
        let _try_nframe = TryCatchNframeGuard::push();


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

                // try() swallows the error here; any condition recorded by
                // a stop(<condition>) inside the expression is consumed
                // with it (a later unrelated error must not inherit it).
                // Consume the call-less flag first: verrorcall_dflt records
                // it on the same error that produced this payload.
                let call_less = crate::mainutils::errors::take_error_call_less()
                    || {
                        let buf = crate::mainutils::errors::R_GetErrorBuf();
                        crate::mainutils::errors::error_was_last_rendered(&message)
                            && buf.starts_with("Error: ")
                    };
                set_signalled_condition(std::ptr::null_mut());

                let silent = as_bool_arg(silent_arg, rho);

                // GNU try.default (New-Internal.R): if conditionCall(e)
                // is empty, prefix is "Error : "; otherwise deparse the
                // call (doTryCatch remapped to the tried expression).
                let (prefix, condition) = if call_less {
                    (
                        "Error : ".to_string(),
                        simple_error_condition_at(&message, Some(R_NilValue())),
                    )
                } else {
                    let display_call = if !expr.is_null()
                        && expr != R_NilValue()
                        && TYPEOF(expr) == SEXPTYPE::LANGSXP
                    {
                        expr
                    } else {
                        _call
                    };
                    let dcall_sexp = crate::mainutils::deparse::deparse1s(display_call);
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
                    // GNU New-Internal.R: 14L + nchar(dcall) + nchar(first line)
                    let width = 14 + dcall.chars().count() + first_line_len;
                    if width > 75 {
                        prefix.push_str("\n  ");
                    }
                    (prefix, simple_error_condition_at(&message, caught_error_call()))
                };
                let out_text = format!("{prefix}{message}\n");

                if !silent {
                    crate::sexp::output::capture_stderr(&out_text);
                }

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
                        c"try-error".as_ptr() as *const core::ffi::c_char
                    ),
                );
                crate::sexp::attrib_core::Rf_setAttrib(
                    msg_sexp,
                    crate::eval::attrib_core::R_ClassSymbol(),
                    klass,
                );
                crate::sexp::attrib_core::Rf_setAttrib(
                    msg_sexp,
                    Rf_install(c"condition".as_ptr() as *const core::ffi::c_char),
                    condition,
                );
                // Stock try() ends its error handler with
                // `invisible(structure(class = "try-error", ...))`, so the
                // try-error value must not auto-print even when the failed
                // expression left R_Visible set.
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
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
        // GNU formals `(expr, ...)`. `expr=` binds by name; other tags are handlers.
        let expr_sym = Rf_install(c"expr".as_ptr());
        let mut expr = crate::sexp::globals::R_MissingArg();
        let mut handler_args = R_NilValue();
        let mut p = args;
        let mut handler_cells = Vec::new();
        while !p.is_null() && p != R_NilValue() {
            let tag = TAG(p);
            if !tag.is_null() && tag != R_NilValue() && tag == expr_sym {
                expr = CAR(p);
            } else if tag.is_null() || tag == R_NilValue() {
                if expr == crate::sexp::globals::R_MissingArg() {
                    expr = CAR(p);
                }
            } else {
                handler_cells.push((tag, CAR(p)));
            }
            p = CDR(p);
        }
        for (tag, val) in handler_cells.into_iter().rev() {
            let cell = Rf_cons(val, handler_args);
            SETTAG(cell, tag);
            handler_args = cell;
        }
        let _ha = protect(handler_args);
        if expr.is_null() || expr == R_NilValue() || expr == crate::sexp::globals::R_MissingArg() {
            crate::mainutils::errors::errorcall_str(
                _call,
                "argument \"expr\" is missing, with no default",
            );

        }


        let old_stack = condition_handler_stack();
        let new_stack = calling_handler_stack_from_args(handler_args, rho, old_stack);
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

unsafe fn handlers_structurally_equal(a: SEXP, b: SEXP) -> bool {
    unsafe {
        if a == b {
            return true;
        }
        if TYPEOF(a) != TYPEOF(b) {
            return false;
        }
        if TYPEOF(a) != SEXPTYPE::CLOSXP {
            return crate::mainutils::identical::R_compute_identical(a, b, 32) != 0;
        }
        if crate::sexp::accessors::CLOENV(a) != crate::sexp::accessors::CLOENV(b) {
            return false;
        }
        if crate::mainutils::identical::R_compute_identical(
            crate::sexp::accessors::FORMALS(a),
            crate::sexp::accessors::FORMALS(b),
            0,
        ) == 0
        {
            return false;
        }
        if !lang_equal_ignore_srcref(
            crate::sexp::accessors::BODY(a),
            crate::sexp::accessors::BODY(b),
        ) {
            return false;
        }
        attribs_equal_ignore_source(
            crate::sexp::accessors::ATTRIB(a),
            crate::sexp::accessors::ATTRIB(b),
        )
    }
}

unsafe fn lang_equal_ignore_srcref(a: SEXP, b: SEXP) -> bool {
    unsafe {
        if a == b {
            return true;
        }
        if a.is_null() || b.is_null() || a == R_NilValue() || b == R_NilValue() {
            return a == b || (a == R_NilValue() && b == R_NilValue());
        }
        if TYPEOF(a) != TYPEOF(b) {
            return false;
        }
        if TYPEOF(a) == SEXPTYPE::LANGSXP || TYPEOF(a) == SEXPTYPE::LISTSXP {
            lang_equal_ignore_srcref(CAR(a), CAR(b)) && lang_equal_ignore_srcref(CDR(a), CDR(b))
        } else {
            crate::mainutils::identical::R_compute_identical(a, b, 0) != 0
        }
    }
}

unsafe fn attribs_equal_ignore_source(mut a: SEXP, mut b: SEXP) -> bool {
    unsafe {
        fn skip_src(mut p: SEXP) -> SEXP {
            unsafe {
                while !p.is_null() && p != R_NilValue() {
                    let tag = TAG(p);
                    let name = if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                        let c = crate::sexp::accessors::CHAR(PRINTNAME(tag));
                        if c.is_null() {
                            ""
                        } else {
                            std::ffi::CStr::from_ptr(c).to_str().unwrap_or("")
                        }
                    } else {
                        ""
                    };
                    if name == "srcref" || name == "srcfile" || name == "wholeSrcref" {
                        p = CDR(p);
                        continue;
                    }
                    break;
                }
                p
            }
        }
        a = skip_src(a);
        b = skip_src(b);
        crate::mainutils::identical::R_compute_identical(a, b, 0) != 0
    }
}


/// GNU `globalCallingHandlers(...)` — register/inspect/clear global calling handlers.
pub unsafe fn do_globalCallingHandlers(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {

    unsafe {
        if args.is_null() || args == R_NilValue() {
            crate::sexp::globals::set_R_Visible(TRUE);
            return global_handlers_list();
        }

        let first = CAR(args);
        let rest = CDR(args);
        let single = rest.is_null() || rest == R_NilValue();

        if single && (first.is_null() || first == R_NilValue()) {
            refuse_if_local_handlers();
            let old = global_handlers_list();
            let _old = protect(old);
            set_global_handlers_list(empty_named_list());
            install_global_handler_stack(rho);

            crate::sexp::globals::set_R_Visible(FALSE);
            return old;
        }

        let incoming = if single
            && TYPEOF(first) == SEXPTYPE::VECSXP
            && (TAG(args).is_null() || TAG(args) == R_NilValue())
        {
            first
        } else {
            named_list_from_dots(args)
        };
        let _incoming = protect(incoming);
        validate_named_handlers(incoming);
        refuse_if_local_handlers();

        let combined = prepend_handlers(incoming, global_handlers_list());
        let _combined = protect(combined);
        let combined = drop_duplicate_class_handlers(combined);
        let _dedup = protect(combined);
        set_global_handlers_list(combined);
        install_global_handler_stack(rho);

        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

fn global_handlers_list() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        let gh = (*inst).error_state.global_calling_handlers;
        if gh.is_null() || gh == R_NilValue() {
            empty_named_list()
        } else {
            gh
        }
    })
}

fn set_global_handlers_list(list: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.global_calling_handlers = list;
    });
}

unsafe fn empty_named_list() -> SEXP {
    unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 0) }
}

unsafe fn named_list_from_dots(mut args: SEXP) -> SEXP {
    unsafe {
        let mut n = 0;
        let mut p = args;
        while !p.is_null() && p != R_NilValue() {
            n += 1;
            p = CDR(p);
        }
        let list = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        let _list = protect(list);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _names = protect(names);
        let mut i = 0i64;
        while !args.is_null() && args != R_NilValue() {
            SET_VECTOR_ELT(list, i, CAR(args));
            let tag = TAG(args);
            if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
                SET_STRING_ELT(names, i, Rf_mkChar(c"".as_ptr()));
            } else {
                SET_STRING_ELT(names, i, PRINTNAME(tag));
            }
            i += 1;
            args = CDR(args);
        }
        crate::sexp::attrib_core::setAttrib(list, crate::sexp::attrib_core::R_NamesSymbol(), names);
        list
    }
}

unsafe fn validate_named_handlers(list: SEXP) {
    unsafe {
        let n = XLENGTH(list);
        let names = crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        if n > 0 && (names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP) {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "condition handlers must be specified with a condition class".to_string(),
            });
        }
        for i in 0..n {
            if names.is_null()
                || names == R_NilValue()
                || STRING_ELT(names, i).is_null()
                || crate::sexp::accessors::CHAR(STRING_ELT(names, i)).is_null()
                || std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(STRING_ELT(names, i)))
                    .to_bytes()
                    .is_empty()
            {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "condition handlers must be specified with a condition class"
                        .to_string(),
                });
            }
            if !crate::mainutils::essentials::is_function_value(VECTOR_ELT(list, i)) {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "condition handlers must be functions".to_string(),
                });
            }
        }
    }
}

unsafe fn prepend_handlers(new: SEXP, old: SEXP) -> SEXP {
    unsafe {
        let nn = XLENGTH(new);
        let no = if old.is_null() || old == R_NilValue() {
            0
        } else {
            XLENGTH(old)
        };
        let out = Rf_allocVector3(SEXPTYPE::VECSXP, nn + no);
        let _out = protect(out);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, nn + no);
        let _names = protect(names);
        let new_names = crate::sexp::attrib_core::getAttrib(new, crate::sexp::attrib_core::R_NamesSymbol());
        let old_names = if old.is_null() || old == R_NilValue() {
            R_NilValue()
        } else {
            crate::sexp::attrib_core::getAttrib(old, crate::sexp::attrib_core::R_NamesSymbol())
        };
        for i in 0..nn {
            SET_VECTOR_ELT(out, i, VECTOR_ELT(new, i));
            if !new_names.is_null() && new_names != R_NilValue() {
                SET_STRING_ELT(names, i, STRING_ELT(new_names, i));
            }
        }
        for i in 0..no {
            SET_VECTOR_ELT(out, nn + i, VECTOR_ELT(old, i));
            if !old_names.is_null() && old_names != R_NilValue() {
                SET_STRING_ELT(names, nn + i, STRING_ELT(old_names, i));
            }
        }
        crate::sexp::attrib_core::setAttrib(out, crate::sexp::attrib_core::R_NamesSymbol(), names);
        out
    }
}

unsafe fn drop_duplicate_class_handlers(list: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(list);
        if n <= 1 {
            return list;
        }
        let names =
            crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        let mut keep = vec![true; n as usize];
        for i in 0..n {
            if !keep[i as usize] {
                continue;
            }
            let mut dropped = false;
            for j in (i + 1)..n {
                if !keep[j as usize] {
                    continue;
                }
                if !chars_equal(STRING_ELT(names, i), STRING_ELT(names, j)) {
                    continue;
                }
                if handlers_structurally_equal(VECTOR_ELT(list, i), VECTOR_ELT(list, j)) {
                    keep[j as usize] = false;
                    dropped = true;
                }
            }
            if dropped {
                let class_name = elt_to_string(names, i);
                let msg = crate::sexp::constructors::Rf_mkString(
                    std::ffi::CString::new(format!(
                        "pushing duplicate `{class_name}` handler on top of the stack"
                    ))
                    .unwrap_or_default()
                    .as_ptr(),
                );
                crate::mainutils::essentials::do_message(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    crate::sexp::constructors::Rf_cons(msg, R_NilValue()),
                    R_NilValue(),
                );
            }
        }
        let kept = keep.iter().filter(|k| **k).count() as i64;
        if kept == n {
            return list;
        }
        let out = Rf_allocVector3(SEXPTYPE::VECSXP, kept);
        let _out = protect(out);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, kept);
        let _on = protect(out_names);
        let mut k = 0i64;
        for i in 0..n {
            if keep[i as usize] {
                SET_VECTOR_ELT(out, k, VECTOR_ELT(list, i));
                SET_STRING_ELT(out_names, k, STRING_ELT(names, i));
                k += 1;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            out,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
        out
    }
}

unsafe fn chars_equal(a: SEXP, b: SEXP) -> bool {
    unsafe {
        if a.is_null() || b.is_null() {
            return a == b;
        }
        let ca = crate::sexp::accessors::CHAR(a);
        let cb = crate::sexp::accessors::CHAR(b);
        if ca.is_null() || cb.is_null() {
            return ca == cb;
        }
        std::ffi::CStr::from_ptr(ca) == std::ffi::CStr::from_ptr(cb)
    }
}

unsafe fn refuse_if_local_handlers() {
    unsafe {
        let gh = global_handlers_list();
        let n = if gh.is_null() || gh == R_NilValue() {
            0
        } else {
            XLENGTH(gh)
        };
        if pairlist_len(condition_handler_stack()) > n {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "should not be called with handlers on the stack".to_string(),
            });
        }
    }
}

unsafe fn install_global_handler_stack(rho: SEXP) {
    unsafe {
        let gh = global_handlers_list();
        let n = if gh.is_null() || gh == R_NilValue() {
            0
        } else {
            XLENGTH(gh)
        };
        let names = if n == 0 {
            R_NilValue()
        } else {
            crate::sexp::attrib_core::getAttrib(gh, crate::sexp::attrib_core::R_NamesSymbol())
        };

        let mut stack = R_NilValue();
        if n > 0 {
            for i in (0..n).rev() {
                let class_name = elt_to_string(names, i);
                let handler = VECTOR_ELT(gh, i);
                let entry = calling_handler_entry(&class_name, handler, rho);
                let _e = protect(entry);
                stack = Rf_cons(entry, stack);
                let _s = protect(stack);
            }
        }
        set_condition_handler_stack(stack);
    }
}

unsafe fn pairlist_len(mut p: SEXP) -> i64 {
    unsafe {
        let mut n = 0i64;
        while !p.is_null() && p != R_NilValue() {
            n += 1;
            p = CDR(p);
        }
        n
    }
}





/// GNU `simpleCondition(message, call = NULL)`.
pub unsafe fn do_simpleCondition(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let message = if args.is_null() || args == R_NilValue() {
            String::new()
        } else {
            elt_to_string(CAR(args), 0)
        };
        simple_condition(&message, &["simpleCondition", "condition"])
    }
}

/// GNU `signalCondition(cond)` — invoke matching calling handlers.
pub unsafe fn do_signalCondition_r(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut cond = if args.is_null() || args == R_NilValue() {
            R_NilValue()
        } else {
            CAR(args)
        };
        if cond.is_null()
            || cond == R_NilValue()
            || crate::mainutils::objects::inherits2(cond, c"condition".as_ptr()) == 0
        {
            let msg = elt_to_string(cond, 0);
            cond = simple_condition(&msg, &["simpleCondition", "condition"]);
        }
        let _cond = protect(cond);
        signal_calling_handlers(cond, rho);
        R_NilValue()
    }
}



// ---------------------------------------------------------------------------
// Exiting handlers (tryCatch) for warning conditions
// ---------------------------------------------------------------------------

/// Record the condition being signaled by `stop(<condition>)` (null clears
/// the slot). The object is a field of the active `RInstance`, so the normal
/// session GC marks and rewrites it while an unwind is in flight.
pub(crate) fn set_signalled_condition(cond: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.signalled_condition = cond;
    });
}

fn signalled_condition() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.signalled_condition
    })
}

/// Message text of a condition object (its `message` field), mirroring
/// `conditionMessage()`.
unsafe fn condition_message_of(cond: SEXP) -> Option<String> {
    unsafe {
        let msg = crate::mainutils::essentials::tables::list_element_by_name(cond, "message")?;
        Some(elt_to_string(msg, 0))
    }
}

/// Class vector of a condition object. Handler selection matches the
/// `tryCatch` tag against these classes (upstream searches the exiting
/// handler entries in registration order and takes the first whose
/// class the condition inherits from).
unsafe fn condition_classes(cond: SEXP) -> Vec<String> {
    unsafe {
        let class_attr = crate::sexp::attrib_core::getAttrib(cond, Rf_install(c"class".as_ptr()));
        if class_attr.is_null()
            || class_attr == R_NilValue()
            || TYPEOF(class_attr) != SEXPTYPE::STRSXP
        {
            return vec!["error".to_string()];
        }
        (0..XLENGTH(class_attr))
            .map(|i| elt_to_string(class_attr, i))
            .collect()
    }
}
/// Panic payload for warning unwinds: `RSignal::Warning { message }`
/// (sexp::context) carries the warning out of `warning()` into an
/// enclosing `tryCatch(..., warning = )` frame; the R panic hook already
/// silences RSignal payloads, and every RSignal match site passes
/// unknown variants through.
/// Does any enclosing tryCatch frame register one of `classes`?
pub(crate) fn try_catch_wants_warning() -> bool {
    try_catch_wants(&["simpleWarning", "warning", "condition"])
}
fn try_catch_wants(classes: &[&str]) -> bool {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst)
            .error_state
            .try_catch_handler_classes
            .iter()
            .any(|frame| frame.iter().any(|c| classes.contains(&c.as_str())))
    })
}

fn condition_handler_stack() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.handler_stack
    })
}

fn set_condition_handler_stack(stack: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.handler_stack = stack;
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
thread_local! {
    static WARNING_HANDLER_RAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A calling handler matched and ran for the warning just signaled.
pub(crate) fn warning_handler_invoked() -> bool {
    WARNING_HANDLER_RAN.with(|c| c.get())
}

unsafe fn signal_calling_handlers(condition: SEXP, rho: SEXP) {
    unsafe {
        WARNING_HANDLER_RAN.with(|c| c.set(false));
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
                    WARNING_HANDLER_RAN.with(|c| c.set(true));
                }
                current = CDR(current);
            }
        }
    }
}

/// Signal an already-constructed warning condition to active calling handlers.
///
/// Returns `true` when a handler invoked the dynamically-scoped
/// `muffleWarning` restart. Internal runtime sites use this when a warning has
/// a more specific class than `simpleWarning`; keeping the concrete condition
/// intact is required for class-selective handlers and `inherits()` checks.
pub(crate) unsafe fn signal_calling_warning_condition(condition: SEXP, rho: SEXP) -> bool {
    unsafe {
        let old_stack = restart_stack();
        let restart = restart_entry("muffleWarning", R_NilValue(), R_NilValue());
        let _restart_guard = protect(restart);
        let new_stack = Rf_cons(restart, old_stack);
        let _stack_guard = protect(new_stack);
        set_restart_stack(new_stack);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            signal_calling_handlers(condition, rho)
        }));
        set_restart_stack(old_stack);

        match result {
            Ok(()) => false,
            Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
                Ok(signal) => match *signal {
                    crate::sexp::context::RSignal::Restart(jump) if jump.target == restart => true,
                    other => std::panic::panic_any(other),
                },
                Err(payload) => std::panic::resume_unwind(payload),
            },
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
            if is_restart_object(restart_arg)
                && restart_name(restart_arg).as_deref() == Some("abort")
                && restart_field(restart_arg, "exit", 1) == R_NilValue()
            {
                return Some(restart_arg);
            }
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

unsafe fn invoke_restart(restart: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if restart_name(restart).as_deref() == Some("abort")
            && restart_field(restart, "exit", 1) == R_NilValue()
        {
            std::panic::panic_any(crate::sexp::context::RSignal::Abort);
        }
    }
    std::panic::panic_any(crate::sexp::context::RSignal::Restart(
        crate::sexp::context::RestartJump::new(restart, args),
    ));
}

unsafe fn call_function_with_args(handler: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _handler_guard = protect(handler);
        let _args_guard = protect(args);
        // GNU withRestarts uses do.call with enquoted argument values.
        // Symbols and language objects must remain data, not execute again.
        let mut entries = Vec::new();
        let mut entry = args;
        while !entry.is_null() && entry != R_NilValue() {
            entries.push(entry);
            entry = CDR(entry);
        }
        let mut quoted = R_NilValue();
        let mut guards = Vec::new();
        for entry in entries.into_iter().rev() {
            let value = Rf_lang2(
                crate::sexp::symbol::Rf_install(c"quote".as_ptr()),
                CAR(entry),
            );
            guards.push(protect(value));
            quoted = Rf_cons(value, quoted);
            guards.push(protect(quoted));
            SETTAG(quoted, TAG(entry));
        }
        let call = Rf_lang2(handler, R_NilValue());
        SETCDR(call, quoted);
        let _call_guard = protect(call);
        if TYPEOF(handler) == SEXPTYPE::CLOSXP {
            crate::eval::closure::applyClosure(call, handler, quoted, rho, R_NilValue(), TRUE)
        } else {
            crate::eval::eval::Rf_eval(call, rho)
        }
    }
}

fn restart_stack() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.restart_stack
    })
}

fn set_restart_stack(stack: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.restart_stack = stack;
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

        let abort = abort_restart_entry();
        let _abort_guard = protect(abort);
        restarts.push(abort);

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
        if name == "abort" {
            Some(abort_restart_entry())
        } else {
            None
        }
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

unsafe fn restart_entry(name: &str, handler: SEXP, exit: SEXP) -> SEXP {
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
        SET_VECTOR_ELT(restart, 1, exit);
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

unsafe fn abort_restart_entry() -> SEXP {
    unsafe {
        let restart = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if restart.is_null() {
            return R_NilValue();
        }
        let _restart_guard = protect(restart);
        SET_VECTOR_ELT(
            restart,
            0,
            Rf_mkString(CString::new("abort").unwrap_or_default().as_ptr()),
        );
        SET_VECTOR_ELT(restart, 1, R_NilValue());
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
/// GNU `stop(..., call. = TRUE)`: `call.` is a named argument after `...`.
/// Default TRUE (include the current call). `call. = FALSE` is `errorcall(R_NilValue)`.
unsafe fn named_call_dot(args: SEXP) -> bool {
    unsafe {
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            if tag_name(cell).as_deref() == Some("call.") {
                return crate::mainutils::coerce::asLogical(CAR(cell)) != 0;
            }
            cell = CDR(cell);
        }
        true
    }
}


pub unsafe fn do_stop(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // stop(<condition>): upstream signals the condition's own object
        // (class c(class, "error", "condition") from errorCondition()) and
        // packages like zeallot build classed conditions and pass them
        // here. Record the object in the signalled-condition slot so the
        // tryCatch catch site can hand the ORIGINAL to a class-matching
        // handler, signal calling handlers with it (.signalCondition),
        // then default to an error carrying its message/call
        // (.dfltStop). Every signaling start clears the slot first: a
        // fresh stop() (condition or not) supersedes any earlier one.
        set_signalled_condition(std::ptr::null_mut());
        let first = CAR(args);
        if crate::mainutils::essentials::sexp_has_class(first, "condition") {
            if let Some(msg) =
                crate::mainutils::essentials::tables::list_element_by_name(first, "message")
            {
                let text = elt_to_string(msg, 0);
                set_signalled_condition(first);
                signal_calling_handlers(first, _rho);
                // `.dfltStop(message, call)` uses conditionCall semantics:
                // the call field kept only when it is a language object.
                let cond_call =
                    crate::mainutils::essentials::tables::list_element_by_name(first, "call")
                        .filter(|c| {
                            let t = TYPEOF(*c);
                            t == SEXPTYPE::LANGSXP.as_c_int()
                                || t == SEXPTYPE::SYMSXP.as_c_int()
                                || t == SEXPTYPE::EXPRSXP.as_c_int()
                        })
                        .unwrap_or(std::ptr::null_mut());
                crate::mainutils::errors::dflt_stop_str(cond_call, &text);
            }
        }

        let s = condition_message_text(args, &["call.", "domain"]);
        let call = if named_call_dot(args) {
            crate::mainutils::errors::R_getCurrentCall()
        } else {
            R_NilValue()
        };
        crate::mainutils::errors::errorcall_str(call, &s);


    }
}

/// R's `warning(...)` — issue warning.
pub unsafe fn do_warning(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let warning_text =
            condition_message_text(args, &["call.", "immediate.", "noBreaks.", "domain"]);
        let condition = simple_condition(&warning_text, &["simpleWarning", "warning", "condition"]);
        // Muffle-aware calling-handler signal: a handler invoking the
        // dynamically scoped muffleWarning restart (upstream
        // invokeRestart inside withCallingHandlers(..., warning =))
        // suppresses BOTH the default print/collection and the exiting-
        // handler unwind below.
        let muffled = {
            let _cond_guard = protect(condition);
            signal_calling_warning_condition(condition, rho)
        };
        if muffled {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return Rf_mkString(CString::new(warning_text).unwrap_or_default().as_ptr());
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

        // Stock defers printing: the warning enters the collection buffer
        // (errors.c vwarningcall_dflt, warn == 0) and the REPL loop renders
        // it via PrintWarnings() at the statement boundary — after that
        // statement's auto-printed value, before the next statement's
        // output. Routing the builtin through the same warningcall()
        // collection path as C-internal warning sites keeps one deferral
        // model: the script loop's boundary flush (and result assembly's
        // tail flush for the final statement) positions the rendered block
        // correctly between print() side effects and auto-printed values,
        // while message() output stays in signal order with it (case 372).
        let wcall = crate::mainutils::errors::R_getCurrentCall();
        let c_msg = CString::new(warning_text.as_str()).unwrap_or_default();
        crate::mainutils::errors::mark_calling_handlers_signaled();
        crate::mainutils::errors::warningcall(wcall, c_msg.as_ptr());
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        // Stock returns the message string (the R closure wraps it in
        // invisible()); print(warning("w")) therefore renders `[1] "w"`.
        Rf_mkString(c_msg.as_ptr())
    }
}

/// GNU `warnings()` — return `last.warning` as a `"warnings"` object.
pub unsafe fn do_warnings(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = Rf_install(c"last.warning".as_ptr());
        let mut last = crate::sexp::accessors::SYMVALUE(sym);
        if last.is_null()
            || last == R_NilValue()
            || last == crate::sexp::globals::R_UnboundValue()
        {
            last = crate::sexp::envir::R_findVar(sym, crate::sexp::globals::R_BaseEnv());
        }
        if last.is_null()
            || last == R_NilValue()
            || last == crate::sexp::globals::R_UnboundValue()
        {
            last = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let _last = protect(last);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _class = protect(class);
        SET_STRING_ELT(class, 0, crate::sexp::constructors::Rf_mkChar(c"warnings".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            last,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        last
    }
}


/// R's `message(...)` — print message.
pub unsafe fn do_message(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let text = condition_message_text(args, &["domain", "appendLF"]);
        let message = format!("{}\n", text);
        let condition = simple_condition(&message, &["simpleMessage", "message", "condition"]);
        signal_calling_handlers(condition, rho);
        // suppressMessages() gates signal-time emission via a depth counter
        // (mirroring suppressWarnings' warning gate): the muffled message
        // never reaches the output stream.
        if crate::mainutils::errors::suppress_messages_depth() == 0 {
            // Signal-time emission into the session's single interleaved
            // output stream (sink-diversion-free, like upstream stderr
            // traffic): keeps message() output in signal order with deferred
            // warnings and auto-printed values (case 372).
            crate::sexp::output::capture_interleaved(&message);
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
        let class_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
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

unsafe fn current_framedepth() -> i32 {
    unsafe { crate::eval::context::framedepth(crate::sexp::context::R_GlobalContext()) }
}

struct TryCatchNframeGuard;

impl TryCatchNframeGuard {
    fn push() -> Self {
        crate::mainutils::errors::push_try_catch_nframe(unsafe { current_framedepth() });
        Self
    }
}

impl Drop for TryCatchNframeGuard {
    fn drop(&mut self) {
        crate::mainutils::errors::pop_try_catch_nframe();
    }
}

/// GNU `errorcall(call)` stores the applied language object; `stop()` uses
/// `getCurrentCall()`. tryCatch fabricates `doTryCatch(...)` only when the
/// raise is in the tryCatch frame itself (`stop("boom")`).
unsafe fn caught_error_call() -> Option<SEXP> {
    unsafe {
        let Some((call, explicit, nframe)) = crate::mainutils::errors::take_recorded_error_call()
        else {
            return None;
        };
        if call.is_null() || call == R_NilValue() {
            return Some(R_NilValue());
        }
        if explicit {
            return Some(call);
        }
        if let Some(entry) = crate::mainutils::errors::try_catch_entry_nframe()
            && nframe > entry
        {
            return Some(call);
        }
        None
    }
}

unsafe fn simple_error_condition(message: &str) -> SEXP {
    unsafe { simple_error_condition_at(message, None) }
}

unsafe fn simple_error_condition_at(message: &str, call: Option<SEXP>) -> SEXP {
    unsafe {
        // GNU stop() inside tryCatch attributes to doTryCatch because that
        // is getCurrentCall(). Unused-arg / Math1 pass the applied call to
        // errorcall; call. = FALSE is a NULL call.
        let mut _call_guard = None;
        let call = match call {
            Some(call) => call,
            None => {
                let s = |name: &str| Rf_install(CString::new(name).unwrap_or_default().as_ptr());
                let inner = crate::sexp::constructors::Rf_lang2(s("return"), s("expr"));
                let call = crate::sexp::constructors::Rf_lang5(
                    s("doTryCatch"),
                    inner,
                    s("name"),
                    s("parentenv"),
                    s("handler"),
                );
                _call_guard = Some(protect(call));
                call
            }
        };
        let c_msg = CString::new(message).unwrap_or_default();
        crate::mainutils::errors::R_makeErrorCondition(
            call,
            c"simpleError".as_ptr() as *const core::ffi::c_char,
            std::ptr::null(),
            0,
            c_msg.as_ptr(),
        )
    }
}

pub(crate) unsafe fn simple_warning_condition(message: &str) -> SEXP {
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
            c"simpleWarning".as_ptr() as *const core::ffi::c_char,
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
            SET_STRING_ELT(names, 0, Rf_mkChar(c"message".as_ptr()));
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }

        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"message".as_ptr()), msg);

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, classes.len() as i64);

        if !class.is_null() {
            let _cp = protect(class);
            for (i, name) in classes.iter().enumerate() {
                SET_STRING_ELT(
                    class,
                    i as R_xlen_t,
                    Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class);
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
        // `handlers <- list(...)`. `finally=` is not a condition handler:
        // GNU evaluates it after the body (success or error).
        let mut handlers: Vec<(String, SEXP)> = Vec::new();
        let mut finally_expr = R_NilValue();
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if let Some(tag) = tag_name(current) {
                if tag == "finally" {
                    finally_expr = CAR(current);
                } else {
                    let handler = crate::eval::eval::Rf_eval(CAR(current), rho);
                    if !handler.is_null() && handler != R_NilValue() {
                        handlers.push((tag, handler));
                    }
                }
            }
            current = CDR(current);
        }
        let _handler_guards: Vec<_> = handlers.iter().map(|(_, h)| protect(*h)).collect();
        crate::sexp::instance::with_required_current_instance(|inst| unsafe {
            (*inst)
                .error_state
                .try_catch_handler_classes
                .push(handlers.iter().map(|(tag, _)| tag.clone()).collect());
            (*inst)
                .error_state
                .try_catch_nframes
                .push(current_framedepth());
        });
        struct PopHandlers(*mut crate::sexp::instance::RInstance);
        impl Drop for PopHandlers {
            fn drop(&mut self) {
                unsafe {
                    (*self.0).error_state.try_catch_handler_classes.pop();
                    (*self.0).error_state.try_catch_nframes.pop();
                }
            }
        }
        let _pop_guard = PopHandlers(crate::sexp::instance::with_required_current_instance(
            |inst| inst,
        ));
        let run_finally = || {
            if !finally_expr.is_null() && finally_expr != R_NilValue() {
                let _ = crate::eval::eval::Rf_eval(finally_expr, rho);
            }
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        let caught_call = if result.is_err() {
            caught_error_call()
        } else {
            None
        };
        drop(_pop_guard);

        match result {
            Ok(val) => {
                run_finally();
                val
            }
            Err(payload) => {
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
                                let call =
                                    crate::sexp::constructors::Rf_lang2(*handler, condition);
                                let handled = crate::eval::eval::Rf_eval(call, rho);
                                run_finally();
                                return handled;
                            }
                            run_finally();
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
                        other => {
                            run_finally();
                            std::panic::panic_any(other);
                        }
                    },
                    Err(payload) => match payload.downcast::<crate::sexp::context::RError>() {
                        Ok(err) => err.message.clone(),
                        Err(payload) => {
                            run_finally();
                            std::panic::resume_unwind(payload);
                        }
                    },
                };

                let slot_cond = signalled_condition();
                let mut original: SEXP = std::ptr::null_mut();
                if !slot_cond.is_null() {
                    if condition_message_of(slot_cond).as_deref() == Some(message.as_str()) {
                        original = slot_cond;
                    } else {
                        set_signalled_condition(std::ptr::null_mut());
                    }
                }

                let condition = if !original.is_null() {
                    original
                } else {
                    simple_error_condition_at(&message, caught_call)
                };
                let classes = condition_classes(condition);
                let Some(handler) = handlers
                    .iter()
                    .find(|(tag, _)| classes.iter().any(|class| class == tag))
                    .map(|(_, handler)| *handler)
                else {
                    run_finally();
                    if let Some(call) = caught_call {
                        crate::mainutils::errors::record_error_call(call, true);
                    }
                    std::panic::panic_any(crate::sexp::context::RError { message });
                };
                if !original.is_null() {
                    set_signalled_condition(std::ptr::null_mut());
                }
                let _cond_guard = protect(condition);
                let call = crate::sexp::constructors::Rf_lang2(handler, condition);
                let handled = crate::eval::eval::Rf_eval(call, rho);
                run_finally();
                handled
            }
        }

    }
}

/// GNU `exists`/`ls`: `where`/`pos` may be an environment, 1-based search
/// index, or search-path name (`"package:methods"`). `-1` means the caller env.
unsafe fn coerce_search_envir(arg: SEXP, default: SEXP) -> SEXP {
    unsafe {
        if arg.is_null() || arg == R_NilValue() {
            return default;
        }
        if TYPEOF(arg) == SEXPTYPE::ENVSXP {
            return arg;
        }
        if TYPEOF(arg) == SEXPTYPE::INTSXP || TYPEOF(arg) == SEXPTYPE::REALSXP {
            if XLENGTH(arg) < 1 {
                return default;
            }
            let pos = if TYPEOF(arg) == SEXPTYPE::INTSXP {
                INTEGER_ELT(arg, 0)
            } else {
                REAL_ELT(arg, 0) as c_int
            };
            if pos == -1 {
                return default;
            }
            return super::mathstats::search_env_from_position(pos);
        }
        if TYPEOF(arg) == SEXPTYPE::STRSXP && XLENGTH(arg) > 0 {
            return super::mathstats::search_env_from_name(&elt_to_string(arg, 0));
        }
        default
    }
}


/// R's `exists(x, envir)` — check name exists.
pub unsafe fn do_exists(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
        let env = coerce_search_envir(
            arg_by_name_or_position(args, &["envir", "where", "frame"], 1),
            rho,
        );

        let inherits = named_logical_arg(args, "inherits").unwrap_or(true);
        // GNU exists(x, where, envir, frame, mode, inherits). A string at
        // position 1 is `where` (search-path name), not `mode`.
        let mode_arg = arg_by_name_or_position(args, &["mode"], 4);
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() || XLENGTH(mode_arg) == 0 {
            "any".to_string()
        } else if TYPEOF(mode_arg) == SEXPTYPE::STRSXP {
            elt_to_string(mode_arg, 0)
        } else {
            "any".to_string()
        };

        let found = if crate::eval::builtin::is_hidden_builtin_name(&name) {
            false
        } else if mode == "function" {
            let value = if inherits {
                crate::sexp::envir::R_findVar(sym, env)
            } else {
                crate::sexp::envir::R_findVarInFrame(env, sym)
            };
            is_function_value(value)
        } else {
            crate::sexp::envir::binding_exists_raw(env, sym, inherits)
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
        let mode_arg = arg_by_name_or_position(args, &["mode"], 3);
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() || XLENGTH(mode_arg) == 0 {
            "any".to_string()
        } else if TYPEOF(mode_arg) == SEXPTYPE::STRSXP {
            elt_to_string(mode_arg, 0)
        } else {
            "any".to_string()
        };
        let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
        if mode == "function" {
            return crate::sexp::envir::findFun(sym, env);
        }
        let inherits = named_logical_arg(args, "inherits").unwrap_or(true);
        if inherits {
            crate::sexp::envir::R_findVar(sym, env)
        } else {
            crate::sexp::envir::R_findVarInFrame(env, sym)
        }
    }
}

/// R's `get0(x, envir, mode = "any", inherits = TRUE, ifnotfound = NULL)` —
/// `get()` that returns `ifnotfound` instead of erroring when the binding
/// is missing (upstream get0 delegates to mget with a one-element
/// ifnotfound list and unwraps the result).
pub unsafe fn do_get0(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let env = environment_arg_or_default(args, &["envir", "pos"], 1, rho);
        let inherits = named_logical_arg(args, "inherits").unwrap_or(true);
        let ifnotfound = arg_by_name_or_position(args, &["ifnotfound"], 4);
        let fallback = if ifnotfound.is_null() {
            R_NilValue()
        } else {
            ifnotfound
        };
        let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
        let value = if inherits {
            crate::sexp::envir::R_findVar(sym, env)
        } else {
            crate::sexp::envir::R_findVarInFrame(env, sym)
        };
        if value.is_null() || value == R_UnboundValue() {
            fallback
        } else {
            value
        }
    }
}

/// R's `mget(x, envir, mode = "any", ifnotfound, inherits = FALSE)` —
/// look up each name of a character vector, returning a named list.
/// `envir` may be one environment or a list recycled along `x`; a missing
/// `ifnotfound` errors for absent bindings, as upstream does.
pub unsafe fn do_mget(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = arg_by_name_or_position(args, &["x"], 0);
        if x_arg.is_null() || x_arg == R_NilValue() || TYPEOF(x_arg) != SEXPTYPE::STRSXP {
            base_error("invalid first argument");
        }
        let n = XLENGTH(x_arg);
        let envir_arg = arg_by_name_or_position(args, &["envir"], 1);
        let inherits = named_logical_arg(args, "inherits").unwrap_or(false);
        let ifnotfound_arg = arg_by_name_or_position(args, &["ifnotfound"], 3);

        // Single environment or recyclable list of environments.
        let mut env_list: Vec<SEXP> = Vec::new();
        if !envir_arg.is_null() && envir_arg != R_NilValue() {
            if TYPEOF(envir_arg) == SEXPTYPE::ENVSXP {
                env_list.push(envir_arg);
            } else if TYPEOF(envir_arg) == SEXPTYPE::VECSXP {
                for i in 0..XLENGTH(envir_arg) {
                    let env = VECTOR_ELT(envir_arg, i);
                    if TYPEOF(env) == SEXPTYPE::ENVSXP {
                        env_list.push(env);
                    }
                }
            }
        } else {
            env_list.push(crate::sexp::globals::R_GlobalEnv());
        }
        if env_list.is_empty() {
            base_error("invalid 'envir' argument");
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        let _result_guard = protect(result);
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _names_guard = protect(names_vec);
        for i in 0..n {
            let name = elt_to_string(x_arg, i);
            crate::sexp::accessors::SET_STRING_ELT(
                names_vec,
                i,
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
            let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
            let env = env_list[(i as usize) % env_list.len()];
            let value = if inherits {
                crate::sexp::envir::R_findVar(sym, env)
            } else {
                crate::sexp::envir::R_findVarInFrame(env, sym)
            };
            if !value.is_null() && value != R_UnboundValue() {
                crate::sexp::accessors::SET_VECTOR_ELT(result, i, value);
            } else if !ifnotfound_arg.is_null() && ifnotfound_arg != R_NilValue() {
                let fallback = if TYPEOF(ifnotfound_arg) == SEXPTYPE::VECSXP {
                    VECTOR_ELT(ifnotfound_arg, (i % XLENGTH(ifnotfound_arg)) as i64)
                } else {
                    ifnotfound_arg
                };
                crate::sexp::accessors::SET_VECTOR_ELT(result, i, fallback);
            } else {
                base_error(format!("value for '{name}' not found"));
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names_vec,
        );
        result
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
                    env = coerce_search_envir(arg, env);
                }
                Some("all.names") => all_names = logical_arg(arg, all_names),
                Some("sorted") => sorted = logical_arg(arg, sorted),
                _ if TYPEOF(arg) == SEXPTYPE::ENVSXP
                    || TYPEOF(arg) == SEXPTYPE::INTSXP
                    || TYPEOF(arg) == SEXPTYPE::REALSXP
                    || TYPEOF(arg) == SEXPTYPE::STRSXP =>
                {
                    env = coerce_search_envir(arg, env);
                }
                _ => {}
            }
            cell = CDR(cell);
        }


        let mut names = if TYPEOF(env) == SEXPTYPE::ENVSXP {
            super::shared::frame_binding_names(env, all_names)
        } else {
            Vec::new()
        };

        if sorted {
            names.sort_by(|a, b| super::sets::collate_str(a, b));
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
        // Upstream rm is a closure: `...` dots arrive UNEVALUATED (this is
        // an unevaluated builtin) and must each be a symbol or character
        // string naming a binding; the named formals are evaluated here.
        let mut names: Vec<String> = Vec::new();
        let mut list_arg: SEXP = R_NilValue();
        let mut pos_arg: SEXP = crate::sexp::globals::R_MissingArg();
        let mut envir_arg: SEXP = crate::sexp::globals::R_MissingArg();
        let mut current = args;
        while current != R_NilValue() && !current.is_null() {
            let tag = TAG(current);
            let expr = CAR(current);
            let tagged = !tag.is_null() && tag != R_NilValue();
            if !tagged {
                match TYPEOF(expr) {
                    t if t == SEXPTYPE::SYMSXP => {
                        // PRINTNAME -> string
                        let pn = PRINTNAME(expr);
                        if !pn.is_null() && pn != R_NilValue() {
                            names.push(
                                std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pn))
                                    .to_string_lossy()
                                    .into_owned(),
                            );
                        }
                    }
                    t if t == SEXPTYPE::STRSXP && XLENGTH(expr) > 0 => {
                        let pn = crate::sexp::accessors::STRING_ELT(expr, 0);
                        if !pn.is_null() && pn != crate::sexp::globals::R_NaString() {
                            names.push(
                                std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pn))
                                    .to_string_lossy()
                                    .into_owned(),
                            );
                        }
                    }
                    t if t == SEXPTYPE::LANGSXP => {
                        // Constant-folded string like '"x"' may arrive as a
                        // call node; evaluate it and accept a string.
                        let val = crate::eval::eval::Rf_eval(expr, rho);
                        if !val.is_null() && TYPEOF(val) == SEXPTYPE::STRSXP && XLENGTH(val) > 0 {
                            let pn = crate::sexp::accessors::STRING_ELT(val, 0);
                            if !pn.is_null() && pn != crate::sexp::globals::R_NaString() {
                                names.push(
                                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pn))
                                        .to_string_lossy()
                                        .into_owned(),
                                );
                            }
                        } else {
                            std::panic::panic_any(crate::sexp::context::RError {
                                message: "... must contain names or character strings".into(),
                            });
                        }
                    }
                    _ => {
                        std::panic::panic_any(crate::sexp::context::RError {
                            message: "... must contain names or character strings".into(),
                        });
                    }
                }
            } else {
                let name = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned();
                match name.as_str() {
                    "list" => list_arg = crate::eval::eval::Rf_eval(expr, rho),
                    "pos" => pos_arg = crate::eval::eval::Rf_eval(expr, rho),
                    "envir" => envir_arg = crate::eval::eval::Rf_eval(expr, rho),
                    _ => {}
                }
            }
            current = CDR(current);
        }
        // Merge an explicit list= argument into the dot names.
        if !list_arg.is_null() && list_arg != R_NilValue() {
            if TYPEOF(list_arg) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(list_arg) {
                    names.push(elt_to_string(list_arg, i));
                }
            }
        }
        if names.is_empty() {
            crate::sexp::globals::set_R_Visible(FALSE);
            return R_NilValue();
        }
        // Resolve the target environment: envir= wins, then pos=.
        let env = if !envir_arg.is_null() && envir_arg != crate::sexp::globals::R_MissingArg() {
            envir_arg
        } else if !pos_arg.is_null() && pos_arg != crate::sexp::globals::R_MissingArg() {
            // pos=-1 (default) means the global environment.
            let pos_val = crate::eval::eval::Rf_eval(pos_arg, rho);
            if !pos_val.is_null()
                && TYPEOF(pos_val) == SEXPTYPE::INTSXP
                && XLENGTH(pos_val) > 0
                && INTEGER_ELT(pos_val, 0) == -1
            {
                crate::sexp::globals::R_GlobalEnv()
            } else {
                crate::sexp::globals::R_GlobalEnv()
            }
        } else {
            rho
        };
        for name in names {
            let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
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
            return Rf_mkString(c"".as_ptr());
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
        let msg_sym = Rf_install(c"message".as_ptr());
        let msg = crate::sexp::attrib_core::getAttrib(cond, msg_sym);
        if !msg.is_null() && msg != R_NilValue() && TYPEOF(msg) == SEXPTYPE::STRSXP {
            return msg;
        }
        Rf_mkString(c"".as_ptr())
    }
}

/// R's `conditionCall(cond)` — get call from condition object.
pub unsafe fn do_conditionCall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cond = CAR(args);
        if cond.is_null() || cond == R_NilValue() {
            return R_NilValue();
        }
        // Upstream conditionCall.default reads the `call` field and keeps
        // it only when it is a language object:
        // `if (is.null(c <- cond$call) || !is.language(c)) NULL else c`.
        let call = crate::mainutils::essentials::tables::list_element_by_name(cond, "call")
            .unwrap_or(R_NilValue());
        let t = TYPEOF(call);
        let is_language = t == SEXPTYPE::LANGSXP.as_c_int()
            || t == SEXPTYPE::SYMSXP.as_c_int()
            || t == SEXPTYPE::EXPRSXP.as_c_int();
        if !call.is_null() && call != R_NilValue() && is_language {
            return call;
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
            let cstr = c"message";
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
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
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class);
        }
        result
    }
}

/// R's `errorCondition(message, ..., class = NULL, call = NULL)`.
///
/// Upstream base defines this as an R-level closure:
/// `structure(list(message = as.character(message), call = call, ...),
/// class = c(class, "error", "condition"))`. The port implements it as a
/// builtin; arguments arrive already evaluated with their tags, so the
/// message/class/call formals are picked out by tag (the first untagged
/// value is the message). zeallot's error paths build their conditions
/// through this.
pub unsafe fn do_errorCondition(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { error_or_warning_condition(args, "error") }
}

/// R's `warningCondition(message, ..., class = NULL, call = NULL)` —
/// see [`do_errorCondition`].
pub unsafe fn do_warningCondition(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { error_or_warning_condition(args, "warning") }
}

unsafe fn error_or_warning_condition(args: SEXP, kind: &str) -> SEXP {
    unsafe {
        let mut message: SEXP = R_NilValue();
        let mut class_arg: SEXP = R_NilValue();
        let mut call_arg: SEXP = R_NilValue();
        // Upstream keeps the `...` fields after message/call:
        // `structure(list(message = ..., call = call, ...), class = ...)`
        // — errorCondition("m", code = 42L) exposes `condition$code`.
        let mut extras: Vec<(String, SEXP)> = Vec::new();
        let mut cur = args;
        while !cur.is_null() && cur != R_NilValue() {
            let tag = crate::sexp::accessors::TAG(cur);
            let value = crate::sexp::accessors::CAR(cur);
            let name = if tag.is_null() || tag == R_NilValue() {
                String::new()
            } else {
                symbol_name(tag).unwrap_or_default()
            };
            if name == "class" {
                class_arg = value;
            } else if name == "call" {
                call_arg = value;
            } else if (name == "message" || name.is_empty()) && message == R_NilValue() {
                message = value;
            } else {
                extras.push((name, value));
            }
            cur = crate::sexp::accessors::CDR(cur);
        }
        let _extras_guards: Vec<_> = extras.iter().map(|(_, v)| protect(*v)).collect();

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, (2 + extras.len()) as i64);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let message_str = if message.is_null() || message == R_NilValue() {
            crate::mainutils::coerce::coerceVector(R_NilValue(), SEXPTYPE::STRSXP.as_c_int())
        } else {
            crate::mainutils::coerce::coerceVector(message, SEXPTYPE::STRSXP.as_c_int())
        };
        let _message_guard = protect(message_str);
        SET_VECTOR_ELT(result, 0, message_str);
        SET_VECTOR_ELT(result, 1, call_arg);
        for (i, (_, value)) in extras.iter().enumerate() {
            SET_VECTOR_ELT(result, (2 + i) as i64, *value);
        }

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, (2 + extras.len()) as i64);
        if !names.is_null() {
            let mut fields: Vec<&str> = vec!["message", "call"];
            fields.extend(extras.iter().map(|(name, _)| name.as_str()));
            for (i, n) in fields.iter().enumerate() {
                let cstr = CString::new(*n).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    let data = (*names).gengc_next_node as *mut SEXP;
                    *data.add(i) = charsxp;
                }
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }

        let extra_classes = if class_arg.is_null()
            || class_arg == R_NilValue()
            || crate::sexp::accessors::TYPEOF(class_arg) != SEXPTYPE::STRSXP
        {
            0
        } else {
            crate::sexp::accessors::LENGTH(class_arg)
        };
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, extra_classes as i64 + 2);
        if !class.is_null() {
            let mut slot = 0usize;
            for i in 0..extra_classes as isize {
                let src = crate::sexp::accessors::STRING_ELT(class_arg, i as i64);
                if !src.is_null() {
                    let data = (*class).gengc_next_node as *mut SEXP;
                    *data.add(slot) = src;
                    slot += 1;
                }
            }
            for name in [kind, "condition"] {
                let cstr = CString::new(name).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    let data = (*class).gengc_next_node as *mut SEXP;
                    *data.add(slot) = charsxp;
                    slot += 1;
                }
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class);
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
            let cstr = c"message";
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
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
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class);
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
        let _old_stack_guard = protect(old_stack);
        let new_stack = restart_stack_from_args(CDR(args), rho, old_stack);
        let _stack_guard = protect(new_stack);
        set_restart_stack(new_stack);

        let mut active_stack = new_stack;
        let mut result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        loop {
            set_restart_stack(old_stack);
            match result {
                Ok(value) => return value,
                Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
                    Ok(signal) => match *signal {
                        crate::sexp::context::RSignal::Restart(jump) => {
                            let mut entry = active_stack;
                            while entry != old_stack && !entry.is_null() && entry != R_NilValue() {
                                if CAR(entry) == jump.target {
                                    break;
                                }
                                entry = CDR(entry);
                            }
                            if entry == old_stack || entry.is_null() || entry == R_NilValue() {
                                std::panic::panic_any(crate::sexp::context::RSignal::Restart(jump));
                            }
                            // Multiple specs have nested dynamic extents in GNU:
                            // earlier specs disappear; later specs remain available
                            // and can themselves be invoked by this handler.
                            active_stack = CDR(entry);
                            set_restart_stack(active_stack);
                            result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                let handler = restart_handler(jump.target);
                                if is_function_value(handler) {
                                    call_function_with_args(handler, jump.args, rho)
                                } else {
                                    R_NilValue()
                                }
                            }));
                        }
                        other => std::panic::panic_any(other),
                    },
                    Err(payload) => std::panic::resume_unwind(payload),
                },
            }
        }
    }
}

unsafe fn restart_stack_from_args(mut args: SEXP, rho: SEXP, old_stack: SEXP) -> SEXP {
    unsafe {
        let mut entries = Vec::new();
        let mut guards = Vec::new();
        while !args.is_null() && args != R_NilValue() {
            let Some(name) = tag_name(args) else {
                args = CDR(args);
                continue;
            };
            let handler = crate::eval::eval::Rf_eval(CAR(args), rho);
            let _handler_guard = protect(handler);
            let exit = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
            let _exit_guard = protect(exit);
            let entry = restart_entry(&name, handler, exit);
            guards.push(protect(entry));
            entries.push(entry);
            args = CDR(args);
        }

        let mut stack = old_stack;
        for entry in entries.into_iter().rev() {
            stack = Rf_cons(entry, stack);
            guards.push(protect(stack));
        }
        stack
    }
}

unsafe fn deparse_assert_expr(expr: SEXP) -> String {
    unsafe {
        let dcall = crate::mainutils::deparse::deparse1s(expr);
        if !dcall.is_null() && dcall != R_NilValue() {
            let elt = STRING_ELT(dcall, 0);
            if !elt.is_null() {
                return CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned();
            }
        }
        "expr".to_string()
    }
}

/// GNU `tools::assertError(expr)` — require `expr` to signal an error.
pub unsafe fn do_assertError(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let dtext = deparse_assert_expr(expr);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        match caught {
            Err(payload) => {
                if payload.downcast_ref::<crate::sexp::context::RError>().is_some()
                    || matches!(
                        payload.downcast_ref::<crate::sexp::context::RSignal>(),
                        Some(crate::sexp::context::RSignal::Error { .. })
                    )
                {
                    crate::sexp::globals::set_R_Visible(FALSE);
                    return R_NilValue();
                }
                std::panic::resume_unwind(payload);
            }
            Ok(_) => {
                crate::mainutils::errors::errorcall_str(
                    _call,
                    &format!("Failed to get error in evaluating {dtext}"),
                );
            }
        }
    }
}

/// GNU `tools::assertWarning(expr)` — require a warning, reject a bare error.
pub unsafe fn do_assertWarning(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let dtext = deparse_assert_expr(expr);
        let before = crate::mainutils::errors::collect_warnings();
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        let warned = crate::mainutils::errors::collect_warnings() > before;
        match caught {
            Err(payload) => {
                let is_error = payload.downcast_ref::<crate::sexp::context::RError>().is_some()
                    || matches!(
                        payload.downcast_ref::<crate::sexp::context::RSignal>(),
                        Some(crate::sexp::context::RSignal::Error { .. })
                    );
                if is_error && warned {
                    crate::mainutils::errors::errorcall_str(
                        _call,
                        &format!("Got warning in evaluating {dtext}, but also an error"),
                    );
                }
                if is_error {
                    crate::mainutils::errors::errorcall_str(
                        _call,
                        &format!("Failed to get warning in evaluating {dtext}"),
                    );
                }
                std::panic::resume_unwind(payload);
            }
            Ok(_) if warned => {
                crate::mainutils::errors::restore_collect_warnings(before);
                crate::sexp::globals::set_R_Visible(FALSE);
                R_NilValue()
            }

            Ok(_) => crate::mainutils::errors::errorcall_str(
                _call,
                &format!("Failed to get warning in evaluating {dtext}"),
            ),
        }
    }
}

/// GNU `tools::assertCondition(expr)` — require any condition.
pub unsafe fn do_assertCondition(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let dtext = deparse_assert_expr(expr);
        let before = crate::mainutils::errors::collect_warnings();
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        let warned = crate::mainutils::errors::collect_warnings() > before;
        match caught {
            Err(payload) => {
                if payload.downcast_ref::<crate::sexp::context::RError>().is_some()
                    || matches!(
                        payload.downcast_ref::<crate::sexp::context::RSignal>(),
                        Some(crate::sexp::context::RSignal::Error { .. })
                    )
                    || warned
                {
                    crate::mainutils::errors::restore_collect_warnings(before);
                    crate::sexp::globals::set_R_Visible(FALSE);
                    return R_NilValue();
                }
                std::panic::resume_unwind(payload);
            }
            Ok(_) if warned => {
                crate::mainutils::errors::restore_collect_warnings(before);
                crate::sexp::globals::set_R_Visible(FALSE);
                R_NilValue()
            }

            Ok(_) => crate::mainutils::errors::errorcall_str(
                _call,
                &format!("Failed to get any condition in evaluating {dtext}"),
            ),
        }
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
        // The uncaught warning is deferred into the collection buffer
        // (rendered by the REPL tail / result assembly, not the captured
        // streams of this raw session API) — it must be pending, with the
        // rendered block available, and the buffer must be drained after.
        assert!(unsafe { crate::mainutils::errors::collect_warnings() } > 0);
        let block = unsafe { crate::mainutils::errors::take_warnings_block() };
        assert!(block.is_some_and(|b| b.contains("Warning message")));
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

    #[test]
    fn signalled_condition_is_session_local_and_gc_rooted() {
        let first = crate::sexp::session::RSession::new();
        unsafe {
            let condition = simple_error_condition("survives gc");
            set_signalled_condition(condition);
        }

        first.gc();
        let retained = signalled_condition();
        assert!(!retained.is_null());
        let message = unsafe { condition_message_of(retained) };
        assert_eq!(message.as_deref(), Some("survives gc"));

        // RSession::new installs a fresh instance on this same thread. A
        // thread-local slot would incorrectly expose `retained` here.
        let second = crate::sexp::session::RSession::new();
        assert!(signalled_condition().is_null());
        drop(second);
        drop(first);
    }

    #[test]
    fn mathlib_warning_guard_roots_nested_calls_and_restores_owner() {
        let first = crate::sexp::session::RSession::new();
        let outer_call = unsafe { crate::sexp::constructors::Rf_ScalarInteger(11) };
        let outer = crate::mainutils::errors::mathlib_warning_call_guard(outer_call);
        let nested_call = unsafe { crate::sexp::constructors::Rf_ScalarInteger(22) };
        let nested = crate::mainutils::errors::mathlib_warning_call_guard(nested_call);

        first.gc();
        let current = crate::mainutils::errors::mathlib_warning_call();
        assert_eq!(
            unsafe { crate::sexp::accessors::INTEGER_ELT(current, 0) },
            22
        );
        drop(nested);
        let restored = crate::mainutils::errors::mathlib_warning_call();
        assert_eq!(
            unsafe { crate::sexp::accessors::INTEGER_ELT(restored, 0) },
            11
        );

        // Dropping while another session is ambient must still restore the
        // owner session, rather than writing the current session's slot.
        let second = crate::sexp::session::RSession::new();
        drop(outer);
        first.with_active(|| {
            assert!(crate::mainutils::errors::mathlib_warning_call().is_null());
        });
        drop(second);
        drop(first);
    }
}
