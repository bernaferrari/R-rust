use super::*;

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
        let _ctx = crate::sexp::context::begin_context_guard(
            crate::sexp::context::ctxt_flags::CTXT_BUILTIN,
            _call,
            _rho,
            _rho,
            None,
            R_NilValue(),
            R_NilValue(),
        );
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
                crate::mainutils::errors::save_error_traceback();
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
            // This public shim stands in for GNU's stop closure. The call
            // belongs to its caller, including when bytecode records the
            // inlined stop expression for other implicit errors.
            crate::mainutils::errors::condition_caller_call()
        } else {
            R_NilValue()
        };
        crate::mainutils::errors::save_error_traceback();
        crate::mainutils::errors::implicit_errorcall_str(call, &s);
    }
}

fn condition_object(value: SEXP) -> SEXP {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return std::ptr::null_mut();
        }
        if condition_classes(value)
            .iter()
            .any(|class| class == "condition")
        {
            value
        } else {
            std::ptr::null_mut()
        }
    }
}

/// R's `warning(...)` — issue warning.
pub unsafe fn do_warning(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ctx = crate::sexp::context::begin_context_guard(
            crate::sexp::context::ctxt_flags::CTXT_BUILTIN,
            _call,
            rho,
            rho,
            None,
            R_NilValue(),
            R_NilValue(),
        );
        let first = CAR(args);
        let passed = condition_object(first);
        let warning_call = if !passed.is_null() {
            crate::mainutils::essentials::tables::list_element_by_name(passed, "call")
                .unwrap_or(R_NilValue())
        } else if named_call_dot(args) {
            crate::mainutils::errors::condition_caller_call()
        } else {
            R_NilValue()
        };
        let _warning_call = protect(warning_call);
        let warning_text = if !passed.is_null() {
            condition_message_of(passed).unwrap_or_default()
        } else {
            condition_message_text(args, &["call.", "immediate.", "noBreaks.", "domain"])
        };
        let condition = if !passed.is_null() {
            passed
        } else {
            simple_warning_condition(&warning_text, warning_call)
        };
        let mut cond_classes = vec![
            "simpleWarning".to_string(),
            "warning".to_string(),
            "condition".to_string(),
        ];
        let first = CAR(args);
        if !first.is_null() && first != R_NilValue() {
            let class = crate::eval::attrib_core::getAttrib(
                first,
                crate::eval::attrib_core::R_ClassSymbol(),
            );
            if !class.is_null() && class != R_NilValue() && TYPEOF(class) == SEXPTYPE::STRSXP {
                cond_classes.clear();
                for i in 0..XLENGTH(class) {
                    let elt = STRING_ELT(class, i);
                    if !elt.is_null() {
                        cond_classes.push(
                            std::ffi::CStr::from_ptr(CHAR(elt))
                                .to_string_lossy()
                                .into_owned(),
                        );
                    }
                }
            }
        }
        if crate::mainutils::errors::warning_class_suppressed(&cond_classes) {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return Rf_mkString(CString::new(warning_text).unwrap_or_default().as_ptr());
        }
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
        if try_catch_wants(&["simpleWarning", "warning", "condition"])
            || (!passed.is_null()
                && try_catch_wants(
                    &condition_classes(passed)
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                ))
        {
            set_signalled_condition(condition);
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
        let wcall = warning_call;
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
        if last.is_null() || last == R_NilValue() || last == crate::sexp::globals::R_UnboundValue()
        {
            last = crate::sexp::envir::R_findVar(sym, crate::sexp::globals::R_BaseEnv());
        }
        if last.is_null() || last == R_NilValue() || last == crate::sexp::globals::R_UnboundValue()
        {
            last = crate::sexp::envir::R_findVar(sym, crate::sexp::globals::R_BaseEnv());
        }
        if last.is_null() || last == R_NilValue() || last == crate::sexp::globals::R_UnboundValue()
        {
            last = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let _last = protect(last);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _class = protect(class);
        SET_STRING_ELT(
            class,
            0,
            crate::sexp::constructors::Rf_mkChar(c"warnings".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(last, crate::sexp::attrib_core::R_ClassSymbol(), class);
        last
    }
}

/// R's `message(...)` — print message.
pub unsafe fn do_message(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let first = CAR(args);
        let passed = condition_object(first);
        let text = if !passed.is_null() {
            condition_message_of(passed).unwrap_or_default()
        } else {
            condition_message_text(args, &["domain", "appendLF"])
        };
        let message = if text.ends_with('\n') {
            text
        } else {
            format!("{text}\n")
        };
        let condition = if !passed.is_null() {
            passed
        } else {
            simple_condition_at(
                &message,
                &["simpleMessage", "message", "condition"],
                crate::mainutils::errors::R_getCurrentCall(),
            )
        };
        let classes = condition_classes(condition);
        let scope = ScopedRestartStack::capture();
        let condition_owned = restart_owned(condition);
        let rho_owned = restart_owned(rho);
        let restart = restart_owned(restart_entry("muffleMessage", R_NilValue(), R_NilValue()));
        let new_stack = restart_owned(Rf_cons(restart.as_raw(), scope.previous().as_raw()));
        set_restart_stack(new_stack.as_raw());
        let signaled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            signal_calling_handlers(condition_owned.as_raw(), rho_owned.as_raw());
        }));
        scope.restore();
        scope.require_live();
        let muffled = match signaled {
            Ok(()) => false,
            Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
                Ok(signal) => match *signal {
                    crate::sexp::context::RSignal::Restart(ticket)
                        if restart_ticket_matches(&ticket, restart.as_raw()) =>
                    {
                        consume_matching_restart(ticket);
                        scope.require_live();
                        true
                    }
                    other => std::panic::panic_any(other),
                },
                Err(payload) => std::panic::resume_unwind(payload),
            },
        };
        if !muffled
            && !crate::mainutils::errors::message_class_suppressed(&classes)
            && try_catch_wants(&classes.iter().map(String::as_str).collect::<Vec<_>>())
        {
            set_signalled_condition(condition);
            std::panic::panic_any(crate::sexp::context::RSignal::Message { message });
        }
        if !muffled && !crate::mainutils::errors::message_class_suppressed(&classes) {
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

pub(crate) unsafe fn simple_error_condition(message: &str) -> SEXP {
    unsafe { simple_error_condition_at(message, None) }
}

pub(crate) unsafe fn simple_error_condition_at(message: &str, call: Option<SEXP>) -> SEXP {
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
        let which = if message.starts_with("subscript out of bounds") {
            crate::mainutils::subset::take_oob_subscript()
        } else {
            0
        };
        let c_msg = CString::new(message).unwrap_or_default();
        let cond = crate::mainutils::errors::R_makeErrorCondition(
            call,
            c"simpleError".as_ptr() as *const core::ffi::c_char,
            std::ptr::null(),
            if which > 0 { 1 } else { 0 },
            c_msg.as_ptr(),
        );
        if which > 0 {
            let scalar = Rf_ScalarInteger(which);
            let _g = protect(scalar);
            crate::mainutils::errors::R_setConditionField(cond, 2, c"subscript".as_ptr(), scalar);
        }
        cond
    }
}

pub(crate) unsafe fn simple_warning_condition(message: &str, call: SEXP) -> SEXP {
    unsafe {
        let call = if call.is_null() {
            crate::sexp::globals::R_NilValue()
        } else {
            call
        };
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
pub(crate) unsafe fn simple_condition(message: &str, classes: &[&str]) -> SEXP {
    unsafe { simple_condition_at(message, classes, R_NilValue()) }
}

pub(crate) unsafe fn simple_condition_at(message: &str, classes: &[&str], call: SEXP) -> SEXP {
    unsafe {
        // GNU simpleWarning/simpleMessage/simpleCondition all carry both a
        // `message` and a `call` field (GNU conditions.R / message.R).
        let call = if call.is_null() { R_NilValue() } else { call };
        let _call_guard = protect(call);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let msg = Rf_mkString(CString::new(message).unwrap_or_default().as_ptr());
        let _msg_guard = protect(msg);
        SET_VECTOR_ELT(result, 0, msg);
        SET_VECTOR_ELT(result, 1, call);

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !names.is_null() {
            let _np = protect(names);
            SET_STRING_ELT(names, 0, Rf_mkChar(c"message".as_ptr()));
            SET_STRING_ELT(names, 1, Rf_mkChar(c"call".as_ptr()));
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }

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
