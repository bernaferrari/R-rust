use super::*;

// ---------------------------------------------------------------------------
// Exiting handlers (tryCatch) for warning conditions
// ---------------------------------------------------------------------------

/// Record the condition being signaled by `stop(<condition>)` (null clears
/// the slot). The object is a field of the active `RInstance`, so the normal
/// field owns its original allocation while an unwind is in flight.
pub(crate) fn set_signalled_condition(cond: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.signalled_condition =
            crate::sexp::instance::RuntimeValue::from_raw_in(inst, cond);
    });
}

pub(crate) fn signalled_condition() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.signalled_condition.as_raw()
    })
}

/// Snapshot the actual root before clearing the runtime slot or running a callback.
pub(crate) fn signalled_condition_owned() -> Option<crate::sexp::object::Sexp<'static>> {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.signalled_condition.owned()
    })
}

/// Message text of a condition object (its `message` field), mirroring
/// `conditionMessage()`.
pub(crate) unsafe fn condition_message_of(cond: SEXP) -> Option<String> {
    unsafe {
        let msg = crate::mainutils::essentials::tables::list_element_by_name(cond, "message")?;
        Some(elt_to_string(msg, 0))
    }
}

/// Class vector of a condition object. Handler selection matches the
/// `tryCatch` tag against these classes (upstream searches the exiting
/// handler entries in registration order and takes the first whose
/// class the condition inherits from).
pub(crate) unsafe fn condition_classes(cond: SEXP) -> Vec<String> {
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
pub(crate) fn try_catch_wants(classes: &[&str]) -> bool {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst)
            .error_state
            .try_catch_handler_classes
            .iter()
            .any(|frame| frame.iter().any(|c| classes.contains(&c.as_str())))
    })
}

pub(crate) fn condition_handler_stack() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.handler_stack.as_raw()
    })
}

pub(crate) fn set_condition_handler_stack(stack: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.handler_stack =
            crate::sexp::instance::RuntimeValue::from_raw_in(inst, stack);
    });
}

pub(crate) unsafe fn condition_entry_factory<'s>() -> crate::sexp::object::SessionNodeFactory<'s> {
    crate::sexp::object::SessionNodeFactory::new(
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
    )
}

pub(crate) fn condition_entry_value<'s>(
    factory: &crate::sexp::object::SessionNodeFactory<'s>,
    value: SEXP,
) -> crate::sexp::object::Sexp<'s> {
    factory
        .wrap(value)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

pub(crate) unsafe fn calling_handler_stack_from_args(
    args: SEXP,
    rho: SEXP,
    old_stack: SEXP,
) -> SEXP {
    unsafe {
        let factory = condition_entry_factory();
        let rho = condition_entry_value(&factory, rho);
        let mut cursor = condition_entry_value(&factory, args);
        let mut entries = Vec::new();
        // The optional runtime stack starts as null before any handlers are
        // installed; select this factory's original nil only at that boundary.
        let mut stack = if old_stack.is_null() {
            factory.nil()
        } else {
            condition_entry_value(&factory, old_stack)
        };
        while !cursor.is_nil() {
            // Retain the original next cell and selected expression before a
            // handler expression can detach either and collect its caller.
            let next = cursor
                .try_cdr()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if let Some(class_name) = tag_name(cursor.as_raw()) {
                let expression = cursor
                    .try_car()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let handler = condition_entry_value(
                    &factory,
                    crate::eval::eval::Rf_eval(expression.as_raw(), rho.as_raw()),
                );
                if is_function_value(handler.as_raw()) {
                    entries.push(calling_handler_entry(&factory, &class_name, handler, &rho));
                }
            }
            cursor = next;
        }
        for entry in entries.into_iter().rev() {
            stack = factory
                .pairlist_cell(&entry, &stack, &factory.nil())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        stack.as_raw()
    }
}

pub(crate) unsafe fn calling_handler_entry<'s>(
    factory: &crate::sexp::object::SessionNodeFactory<'s>,
    class_name: &str,
    handler: crate::sexp::object::Sexp<'s>,
    rho: &crate::sexp::object::Sexp<'s>,
) -> crate::sexp::object::Sexp<'s> {
    // Both signal paths share error_state.handler_stack. Use GNU's single
    // five-slot layout and CHARSXP class, including its calling-entry bit.
    let class = factory
        .allocate(|arena| Some(arena.alloc_charsxp(class_name.as_bytes())))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let entry = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 5)))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut entry = crate::sexp::object::SexpMut::try_from_checked(entry)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (index, value) in [class, rho.clone(), handler, factory.nil(), factory.nil()]
        .into_iter()
        .enumerate()
    {
        entry
            .try_set_vector_elt(index as R_xlen_t, value)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    unsafe {
        crate::sexp::accessors::SETLEVELS(entry.as_raw(), TRUE);
    }
    entry.freeze()
}
thread_local! {
    static WARNING_HANDLER_RAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A calling handler matched and ran for the warning just signaled.
pub(crate) fn warning_handler_invoked() -> bool {
    WARNING_HANDLER_RAN.with(|c| c.get())
}

pub(crate) unsafe fn signal_calling_handlers(condition: SEXP, _rho: SEXP) {
    unsafe {
        WARNING_HANDLER_RAN.with(|c| c.set(false));
        let _condition = protect(condition);
        let message =
            super::tables::list_element_by_name(condition, "message").unwrap_or(R_NilValue());
        let call = super::tables::list_element_by_name(condition, "call").unwrap_or(R_NilValue());
        let called = crate::mainutils::errors::signal_condition_object(condition, message, call);
        WARNING_HANDLER_RAN.with(|c| c.set(called));
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
        let scope = ScopedRestartStack::capture();
        let condition = restart_owned(condition);
        let rho = restart_owned(rho);
        let restart = restart_owned(restart_entry("muffleWarning", R_NilValue(), R_NilValue()));
        let new_stack = restart_owned(Rf_cons(restart.as_raw(), scope.previous().as_raw()));
        set_restart_stack(new_stack.as_raw());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            signal_calling_handlers(condition.as_raw(), rho.as_raw())
        }));
        scope.restore();
        scope.require_live();

        match result {
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
        }
    }
}
