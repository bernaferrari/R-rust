use super::*;

/// R's `withRestarts(expr, ...)` — evaluate an expression with dynamic restarts.
pub unsafe fn do_withRestarts(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let scope = ScopedRestartStack::capture();
        let args = restart_owned(args);
        let expr = args
            .try_car()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let rho = restart_owned(rho);
        if expr.is_nil() {
            return scope.nil.as_raw();
        }
        let previous = scope.previous();
        let specifications = args
            .try_cdr()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut active_stack =
            restart_stack_from_args(specifications.as_raw(), rho.as_raw(), previous.as_raw());
        set_restart_stack(active_stack.as_raw());
        let mut result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            restart_owned(crate::eval::eval::Rf_eval(expr.as_raw(), rho.as_raw()))
        }));
        loop {
            scope.restore();
            match result {
                Ok(value) => {
                    scope.require_live();
                    return value.as_raw();
                }
                Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
                    Ok(signal) => match *signal {
                        crate::sexp::context::RSignal::Restart(ticket) => {
                            let transfer = ticket.resolve().unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                            transfer.require_live().unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                            let target = match transfer.data() {
                                crate::sexp::transfer::OwnedTransfer::Restart {
                                    target, ..
                                } => target.clone(),
                                _ => crate::sexp::context::r_error("invalid restart transfer kind"),
                            };
                            let mut entry = active_stack.clone();
                            let mut seen = std::collections::HashSet::new();
                            while entry.as_raw() != previous.as_raw() && !entry.is_nil() {
                                seen.try_reserve(1).unwrap_or_else(|_| {
                                    crate::sexp::context::r_error("cannot track restart stack")
                                });
                                if !seen.insert(entry.as_raw().addr()) {
                                    crate::sexp::context::r_error("cyclic restart stack");
                                }
                                if entry
                                    .try_car()
                                    .unwrap_or_else(|error| {
                                        crate::sexp::context::r_error(error.to_string())
                                    })
                                    .as_raw()
                                    == target.as_raw()
                                {
                                    break;
                                }
                                entry = entry.try_cdr().unwrap_or_else(|error| {
                                    crate::sexp::context::r_error(error.to_string())
                                });
                            }
                            if entry.as_raw() == previous.as_raw() || entry.is_nil() {
                                drop(transfer);
                                std::panic::panic_any(crate::sexp::context::RSignal::Restart(
                                    ticket,
                                ));
                            }
                            drop(transfer);
                            let transfer = ticket.take().unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                            transfer.require_live().unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                            active_stack = entry.try_cdr().unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                            set_restart_stack(active_stack.as_raw());
                            result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                let (target, args) = match transfer.data() {
                                    crate::sexp::transfer::OwnedTransfer::Restart {
                                        target,
                                        args,
                                    } => (target, args),
                                    _ => crate::sexp::context::r_error(
                                        "invalid restart transfer kind",
                                    ),
                                };
                                let handler = restart_owned(restart_handler(target.as_raw()));
                                let value = if is_function_value(handler.as_raw()) {
                                    call_function_with_args(
                                        handler.as_raw(),
                                        args.as_raw(),
                                        rho.as_raw(),
                                    )
                                } else {
                                    scope.nil.clone()
                                };
                                transfer.require_live().unwrap_or_else(|error| {
                                    crate::sexp::context::r_error(error.to_string())
                                });
                                value
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

unsafe fn restart_stack_from_args(
    args: SEXP,
    rho: SEXP,
    old_stack: SEXP,
) -> crate::sexp::object::Sexp<'static> {
    unsafe {
        let factory = condition_entry_factory();
        let rho = restart_owned(rho);
        let mut cursor = restart_owned(args);
        let mut entries = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while !cursor.is_nil() {
            seen.try_reserve(1).unwrap_or_else(|_| {
                crate::sexp::context::r_error("cannot track restart specifications")
            });
            if !seen.insert(cursor.as_raw().addr()) {
                crate::sexp::context::r_error("cyclic restart specifications");
            }
            let next = cursor
                .try_cdr()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if let Some(name) = tag_name(cursor.as_raw()) {
                let expr = cursor
                    .try_car()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let handler =
                    restart_owned(crate::eval::eval::Rf_eval(expr.as_raw(), rho.as_raw()));
                let exit = restart_owned(crate::sexp::memory_ext::NewEnvironment(
                    R_NilValue(),
                    rho.as_raw(),
                    R_NilValue(),
                ));
                entries.try_reserve(1).unwrap_or_else(|_| {
                    crate::sexp::context::r_error("cannot reserve restart specifications")
                });
                entries.push(restart_owned(restart_entry(
                    &name,
                    handler.as_raw(),
                    exit.as_raw(),
                )));
            }
            cursor = next;
        }
        let mut stack = restart_owned(old_stack);
        for entry in entries.into_iter().rev() {
            stack = factory
                .pairlist_cell(&entry, &stack, &factory.nil())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                .into_owned()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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
                if payload
                    .downcast_ref::<crate::sexp::context::RError>()
                    .is_some()
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
                let is_error = payload
                    .downcast_ref::<crate::sexp::context::RError>()
                    .is_some()
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
                if payload
                    .downcast_ref::<crate::sexp::context::RError>()
                    .is_some()
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
