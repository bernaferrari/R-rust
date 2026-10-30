use super::*;

unsafe fn current_framedepth() -> i32 {
    unsafe { crate::eval::context::framedepth(crate::sexp::context::R_GlobalContext()) }
}

/// The native tryCatch evaluator replaces GNU's doTryCatch closure.
/// Keep its call on the context stack while forcing the protected expression,
/// so an inferred error call has a real caller even at top level.
pub(crate) unsafe fn try_catch_evaluation_context(rho: SEXP) -> crate::sexp::context::ContextGuard {
    unsafe {
        let inner = crate::sexp::constructors::Rf_lang2(
            Rf_install(c"return".as_ptr()),
            Rf_install(c"expr".as_ptr()),
        );
        let _inner_guard = protect(inner);
        let call = crate::sexp::constructors::Rf_lang5(
            Rf_install(c"doTryCatch".as_ptr()),
            inner,
            Rf_install(c"name".as_ptr()),
            Rf_install(c"parentenv".as_ptr()),
            Rf_install(c"handler".as_ptr()),
        );
        let _call_guard = protect(call);
        crate::sexp::context::begin_context_guard(
            crate::sexp::context::ctxt_flags::CTXT_RETURN,
            call,
            rho,
            rho,
            None,
            R_NilValue(),
            R_NilValue(),
        )
    }
}

/// GNU `errorcall(call)` stores the applied language object; `stop()` uses
/// `getCurrentCall()`. tryCatch fabricates `doTryCatch(...)` only when the
/// raise is in the tryCatch frame itself (`stop("boom")`).
pub(crate) unsafe fn caught_error_call() -> Option<crate::sexp::object::Sexp<'static>> {
    unsafe {
        let (call, explicit, nframe) = crate::mainutils::errors::take_recorded_error_call()?;
        if call.as_raw() == R_NilValue() {
            return Some(call);
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

/// R's `tryCatch(expr, ...)` — exiting-handler support.  Handlers for any
/// condition class may be supplied; warning conditions raised in the body
/// unwind here through `RSignal::Warning`, error panics through the existing
/// RSignal/RError payloads (upstream: R_TryCatch / vwarningcall).
pub unsafe fn do_tryCatch(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let _operation_pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            .unwrap_or_else(|| {
                crate::sexp::context::r_error("tryCatch requires a managed runtime")
            });
        let factory = owner.node_factory();
        let rho_owner = factory
            .wrap(rho)
            .and_then(|value| value.into_owned())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let args_owner = factory
            .wrap(args)
            .and_then(|value| value.into_owned())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut supplied = Vec::new();
        let mut current = args_owner;
        while !current.is_nil() {
            let tag = tag_name(current.as_raw());
            let value = current
                .try_car()
                .and_then(|value| value.into_owned())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            supplied.push((tag, value));
            current = current
                .try_cdr()
                .and_then(|value| value.into_owned())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        let mut expr = factory.nil().as_raw();
        let mut handlers: Vec<(String, crate::sexp::object::Sexp<'static>)> = Vec::new();
        let mut finally_expr = factory.nil().as_raw();
        let mut saw_expr = false;
        for (tag, value) in &supplied {
            let is_expr = match tag.as_deref() {
                None if !saw_expr => true,
                Some("expr") => true,
                _ => false,
            };
            if is_expr {
                expr = value.as_raw();
                saw_expr = true;
            } else if let Some(tag) = tag {
                if tag == "finally" {
                    finally_expr = value.as_raw();
                } else {
                    let handler = crate::eval::eval::Rf_eval(value.as_raw(), rho_owner.as_raw());
                    owner
                        .require_active()
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    if !handler.is_null() && handler != R_NilValue() {
                        let handler = factory
                            .wrap(handler)
                            .and_then(|value| value.into_owned())
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                        handlers.push((tag.clone(), handler));
                    }
                }
            }
        }
        if expr.is_null() {
            return R_NilValue();
        }
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
        struct PopHandlers(crate::sexp::owner::OwnerPin);
        impl Drop for PopHandlers {
            fn drop(&mut self) {
                unsafe {
                    (*self.0.as_ptr())
                        .error_state
                        .try_catch_handler_classes
                        .pop();
                    (*self.0.as_ptr()).error_state.try_catch_nframes.pop();
                }
            }
        }
        let _pop_guard = PopHandlers(owner.pin().unwrap().unwrap());
        let run_finally = || {
            if !finally_expr.is_null() && finally_expr != R_NilValue() {
                owner
                    .require_active()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let _ = crate::eval::eval::Rf_eval(finally_expr, rho_owner.as_raw());
                owner
                    .require_active()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
        };
        // The result is not bound until tryCatch returns. `finally` may
        // allocate and collect, so the value stays rooted across that eval.
        let return_after_finally = |value: SEXP| {
            let visible = crate::sexp::globals::R_Visible();
            let value = factory
                .wrap(value)
                .and_then(|value| value.into_owned())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            run_finally();
            owner
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            crate::sexp::globals::set_R_Visible(visible);
            value.as_raw()
        };

        let own_condition = |condition| {
            factory
                .wrap(condition)
                .and_then(|value| value.into_owned())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        };
        let invoke_handler = |handler, condition: &crate::sexp::object::Sexp<'static>| {
            let call = own_condition(crate::sexp::constructors::Rf_lang2(
                handler,
                condition.as_raw(),
            ));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::eval::eval::Rf_eval(call.as_raw(), rho_owner.as_raw())
            }));
            match result {
                Ok(value) => return_after_finally(value),
                Err(payload) => {
                    // The handler runs after its exiting-handler frame has
                    // been popped, but finally still runs if the handler exits.
                    let raised = signalled_condition_owned();
                    run_finally();
                    owner
                        .require_active()
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    set_signalled_condition(
                        raised
                            .as_ref()
                            .map_or(std::ptr::null_mut(), |value| value.as_raw()),
                    );
                    std::panic::resume_unwind(payload)
                }
            }
        };

        let classes = factory
            .strings(
                &handlers
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::mainutils::errors::try_catch_owned(
                || {
                    let _trace = crate::mainutils::debug::NativeTryCatchTrace::enter();
                    let _context = try_catch_evaluation_context(rho_owner.as_raw());
                    own_condition(crate::eval::eval::Rf_eval(expr, rho_owner.as_raw()))
                },
                &classes,
                |condition| {
                    let message = condition_message_of(condition.as_raw()).unwrap_or_default();
                    set_signalled_condition(condition.as_raw());
                    std::panic::panic_any(crate::sexp::context::RError { message })
                },
                || {},
            )
            .as_raw()
        }));
        let caught_call = if result.is_err() {
            caught_error_call()
        } else {
            None
        };
        drop(_pop_guard);

        match result {
            Ok(val) => return_after_finally(val),
            Err(payload) => {
                let payload = match payload.downcast::<crate::sexp::context::RSignal>() {
                    Ok(signal) => match *signal {
                        crate::sexp::context::RSignal::Message { message } => {
                            let condition = signalled_condition_owned().unwrap_or_else(|| {
                                own_condition(simple_condition(
                                    &message,
                                    &["simpleMessage", "message", "condition"],
                                ))
                            });
                            set_signalled_condition(std::ptr::null_mut());
                            let classes = condition_classes(condition.as_raw());
                            let matching = handlers
                                .iter()
                                .find(|(tag, _)| classes.iter().any(|class| class == tag));
                            if let Some((_, handler)) = matching {
                                return invoke_handler(handler.as_raw(), &condition);
                            }
                            run_finally();
                            set_signalled_condition(condition.as_raw());
                            std::panic::resume_unwind(Box::new(
                                crate::sexp::context::RSignal::Message { message },
                            ));
                        }
                        crate::sexp::context::RSignal::Warning { message } => {
                            let stashed_owner = signalled_condition_owned();
                            let stashed = stashed_owner
                                .as_ref()
                                .map_or(std::ptr::null_mut(), |value| value.as_raw());
                            let condition = if !stashed.is_null() {
                                set_signalled_condition(std::ptr::null_mut());
                                stashed
                            } else {
                                simple_warning_condition(
                                    &message,
                                    crate::sexp::globals::R_NilValue(),
                                )
                            };
                            let condition = own_condition(condition);
                            let classes = condition_classes(condition.as_raw());
                            let matching = handlers
                                .iter()
                                .find(|(tag, _)| classes.iter().any(|class| class == tag));
                            if let Some((_, handler)) = matching {
                                return invoke_handler(handler.as_raw(), &condition);
                            }
                            run_finally();
                            set_signalled_condition(condition.as_raw());
                            std::panic::resume_unwind(Box::new(
                                crate::sexp::context::RSignal::Warning { message },
                            ));
                        }
                        other => Box::new(other) as Box<dyn std::any::Any + Send>,
                    },
                    Err(payload) => payload,
                };

                let message = if let Some(signal) =
                    payload.downcast_ref::<crate::sexp::context::RSignal>()
                {
                    if let crate::sexp::context::RSignal::Error { message } = signal {
                        message.as_str()
                    } else {
                        run_finally();
                        std::panic::resume_unwind(payload);
                    }
                } else if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
                    error.message.as_str()
                } else {
                    run_finally();
                    std::panic::resume_unwind(payload);
                };

                let slot_cond_owner = signalled_condition_owned();
                let slot_cond = slot_cond_owner
                    .as_ref()
                    .map_or(std::ptr::null_mut(), |value| value.as_raw());
                let mut original: SEXP = std::ptr::null_mut();
                if !slot_cond.is_null() {
                    if condition_message_of(slot_cond).as_deref() == Some(message) {
                        original = slot_cond;
                    } else {
                        set_signalled_condition(std::ptr::null_mut());
                    }
                }

                let condition = if !original.is_null() {
                    original
                } else {
                    simple_error_condition_at(
                        message,
                        caught_call.as_ref().map(|value| value.as_raw()),
                    )
                };
                let condition = own_condition(condition);
                let classes = condition_classes(condition.as_raw());
                let Some(handler) = handlers
                    .iter()
                    .find(|(tag, _)| classes.iter().any(|class| class == tag))
                    .map(|(_, handler)| handler.clone())
                else {
                    run_finally();
                    if let Some(call) = &caught_call {
                        crate::mainutils::errors::record_error_call(call.as_raw(), true);
                    }
                    set_signalled_condition(condition.as_raw());
                    std::panic::resume_unwind(payload);
                };
                if !original.is_null() {
                    set_signalled_condition(std::ptr::null_mut());
                }
                invoke_handler(handler.as_raw(), &condition)
            }
        }
    }
}
