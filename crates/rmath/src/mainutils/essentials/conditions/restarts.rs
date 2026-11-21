use super::*;
use crate::sexp::object::SexpMut;

/// R's `computeRestarts()` — compute available restarts for current condition.
pub fn do_computeRestarts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    restart_stack_as_list()
}

/// R's `findRestart(name)` — find a restart by name.
pub fn do_findRestart(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let name_arg = CAR(args);
        if name_arg.is_null() || name_arg == R_NilValue() {
            return R_NilValue();
        }
        let name = elt_to_string(name_arg, 0);
        find_restart_by_name(&name).unwrap_or_else(R_NilValue)
    }
}

/// R's `restarts()` — list available restarts.
pub fn do_restarts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    restart_stack_as_list()
}

/// R's `invokeRestart(restart, ...)` — call a restart and return to its dynamic extent.
pub fn do_invokeRestart(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
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
pub fn do_tryInvokeRestart(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let restart_arg = CAR(args);
        match resolve_restart_arg(restart_arg, true) {
            Some(restart) => invoke_restart(restart, CDR(args), rho),
            None => R_NilValue(),
        }
    }
}

/// R's `isRestart(x)` — check for a restart object.
pub fn do_isRestart(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarLogical(if is_restart_object(CAR(args)) {
        TRUE
    } else {
        FALSE
    })
}

/// R's `restartDescription(r)` — return the restart description, if any.
pub fn do_restartDescription(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let restart = CAR(args);
    if !is_restart_object(restart) {
        return R_NilValue();
    }
    let description = restart_field(restart, "description", 3);
    if description.is_null() || description == R_NilValue() {
        mk_string_cstr(c"")
    } else {
        description
    }
}

fn resolve_restart_arg(restart_arg: SEXP, require_active_object: bool) -> Option<SEXP> {
    {
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

fn restart_arg_name(restart_arg: SEXP) -> String {
    {
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

/// Native condition adapter: retain the original physical value and revocable
/// runtime capability before any allocation, callback, or non-local transfer.
pub(crate) fn restart_owned(value: SEXP) -> crate::sexp::object::Sexp<'static> {
    let factory = { condition_entry_factory() };
    condition_entry_value(&factory, value)
        .into_owned()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

pub(crate) struct ScopedRestartStack {
    pin: crate::sexp::owner::OwnerPin,
    previous: Option<crate::sexp::object::Sexp<'static>>,
    pub(crate) nil: crate::sexp::object::Sexp<'static>,
}

impl ScopedRestartStack {
    pub(crate) fn capture() -> Self {
        let owner = current_owner()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            .unwrap_or_else(|| crate::sexp::context::r_error("restart requires a runtime owner"));
        let previous = unsafe { (*pin.as_ptr()).error_state.restart_stack.owned() };
        let nil = owner
            .node_factory()
            .nil()
            .into_owned()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        Self { pin, previous, nil }
    }

    pub(crate) fn previous(&self) -> crate::sexp::object::Sexp<'static> {
        self.previous.clone().unwrap_or_else(|| self.nil.clone())
    }

    pub(crate) fn restore(&self) {
        // Cleanup retains the original physical runtime even after revocation.
        unsafe {
            (*self.pin.as_ptr()).error_state.restart_stack = self
                .previous
                .clone()
                .map(crate::sexp::instance::RuntimeValue::from_owned)
                .unwrap_or_default();
        }
    }

    pub(crate) fn require_live(&self) {
        self.pin
            .require_live()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
}

impl Drop for ScopedRestartStack {
    fn drop(&mut self) {
        self.restore();
    }
}

pub(crate) fn restart_ticket_matches(
    ticket: &crate::sexp::transfer::TransferTicket,
    target: SEXP,
) -> bool {
    let lease = ticket
        .resolve()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    lease
        .require_live()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    match lease.data() {
        crate::sexp::transfer::OwnedTransfer::Restart { target: value, .. } => {
            value.as_raw() == target
        }
        _ => false,
    }
}

pub(crate) fn consume_matching_restart(ticket: crate::sexp::transfer::TransferTicket) {
    let lease = ticket
        .take()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    lease
        .require_live()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
}

fn invoke_restart(restart: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let target_owned = restart_owned(restart);
    let args_owned = restart_owned(args);
    let restart = target_owned.as_raw();
    let args = args_owned.as_raw();
    if restart_name(restart).as_deref() == Some("abort")
        && restart_field(restart, "exit", 1) == R_NilValue()
    {
        std::panic::panic_any(crate::sexp::context::RSignal::Abort);
    }
    std::panic::panic_any(crate::sexp::context::RSignal::Restart(
        crate::sexp::context::restart_transfer(restart, args),
    ));
}

pub(crate) fn call_function_with_args(
    handler: SEXP,
    args: SEXP,
    rho: SEXP,
) -> crate::sexp::object::Sexp<'static> {
    unsafe {
        let handler = restart_owned(handler);
        let rho = restart_owned(rho);
        let mut cursor = restart_owned(args);
        let factory = condition_entry_factory();
        // Snapshot actual selected values/tags before callbacks can replace args.
        let mut entries = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while !cursor.is_nil() {
            seen.try_reserve(1).unwrap_or_else(|_| {
                crate::sexp::context::r_error("cannot track restart arguments")
            });
            if !seen.insert(cursor.as_raw().addr()) {
                crate::sexp::context::r_error("cyclic restart arguments");
            }
            let value = cursor
                .try_car()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let tag = cursor
                .try_tag()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            entries.try_reserve(1).unwrap_or_else(|_| {
                crate::sexp::context::r_error("cannot reserve restart arguments")
            });
            entries.push((value, tag));
            cursor = cursor
                .try_cdr()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        let mut quoted = factory.nil();
        for (value, tag) in entries.into_iter().rev() {
            let value = restart_owned(Rf_lang2(install_str("quote"), value.as_raw()));
            quoted = factory
                .pairlist_cell(&value, &quoted, &tag)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        let call = restart_owned(Rf_lang2(handler.as_raw(), R_NilValue()));
        SETCDR(call.as_raw(), quoted.as_raw());
        let result = if handler.typeof_() == SEXPTYPE::CLOSXP {
            crate::eval::closure::applyClosure(
                call.as_raw(),
                handler.as_raw(),
                quoted.as_raw(),
                rho.as_raw(),
                R_NilValue(),
                TRUE,
            )
        } else {
            crate::eval::eval::Rf_eval(call.as_raw(), rho.as_raw())
        };
        restart_owned(result)
    }
}

fn restart_stack() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.restart_stack.as_raw()
    })
}

pub(crate) fn set_restart_stack(stack: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.restart_stack =
            crate::sexp::instance::RuntimeValue::from_raw_in(inst, stack);
    });
}

fn restart_stack_as_list() -> SEXP {
    let owner =
        current_owner().unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let pin = owner
        .pin()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .unwrap_or_else(|| crate::sexp::context::r_error("restart requires a runtime owner"));
    let stack = restart_stack();
    let mut cursor = if stack.is_null() {
        owner.node_factory().nil().into_owned().unwrap()
    } else {
        restart_owned(stack)
    };
    let mut restarts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while !cursor.is_nil() {
        seen.try_reserve(1)
            .unwrap_or_else(|_| crate::sexp::context::r_error("cannot track restart stack"));
        if !seen.insert(cursor.as_raw().addr()) {
            crate::sexp::context::r_error("cyclic restart stack");
        }
        restarts
            .try_reserve(1)
            .unwrap_or_else(|_| crate::sexp::context::r_error("cannot reserve restarts"));
        restarts.push(
            cursor
                .try_car()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
        );
        cursor = cursor
            .try_cdr()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    restarts.push(restart_owned(abort_restart_entry()));
    let factory = condition_entry_factory();
    let result = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, restarts.len() as R_xlen_t)))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut result_mut = SexpMut::try_from_checked(result.clone())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (index, value) in restarts.iter().enumerate() {
        result_mut
            .try_set_vector_elt(index as R_xlen_t, value.clone())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    pin.require_live()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    result.as_raw()
}

fn find_restart_by_name(name: &str) -> Option<SEXP> {
    {
        let factory = condition_entry_factory();
        let stack = restart_stack();
        let mut cursor = if stack.is_null() {
            factory.nil()
        } else {
            condition_entry_value(&factory, stack)
        };
        let mut seen = std::collections::HashSet::new();
        while !cursor.is_nil() {
            seen.try_reserve(1)
                .unwrap_or_else(|_| crate::sexp::context::r_error("cannot track restart stack"));
            if !seen.insert(cursor.as_raw().addr()) {
                crate::sexp::context::r_error("cyclic restart stack");
            }
            let entry = cursor
                .try_car()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let next = cursor
                .try_cdr()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if restart_name(entry.as_raw()).as_deref() == Some(name) {
                return Some(entry.as_raw());
            }
            cursor = next;
        }
        if name == "abort" {
            Some(abort_restart_entry())
        } else {
            None
        }
    }
}

fn find_restart_by_object(needle: SEXP) -> Option<SEXP> {
    {
        let factory = condition_entry_factory();
        let stack = restart_stack();
        let mut cursor = if stack.is_null() {
            factory.nil()
        } else {
            condition_entry_value(&factory, stack)
        };
        let mut seen = std::collections::HashSet::new();
        while !cursor.is_nil() {
            seen.try_reserve(1)
                .unwrap_or_else(|_| crate::sexp::context::r_error("cannot track restart stack"));
            if !seen.insert(cursor.as_raw().addr()) {
                crate::sexp::context::r_error("cyclic restart stack");
            }
            let entry = cursor
                .try_car()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if entry.as_raw() == needle {
                return Some(entry.as_raw());
            }
            cursor = cursor
                .try_cdr()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        None
    }
}

fn restart_name(restart: SEXP) -> Option<String> {
    {
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

pub(crate) fn restart_handler(restart: SEXP) -> SEXP {
    restart_field(restart, "handler", 2)
}

fn restart_field(restart: SEXP, field_name: &str, fallback_index: R_xlen_t) -> SEXP {
    if restart.is_null() || restart == R_NilValue() || TYPEOF(restart) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let restart_owned = restart_owned(restart);
    let restart = restart_owned.as_raw();
    let names = crate::sexp::attrib_core::getAttrib(restart, install_str("names"));
    let names_owned = if names.is_null() {
        None
    } else {
        Some(condition_entry_value(&condition_entry_factory(), names))
    };
    if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
        let limit = XLENGTH(names).min(XLENGTH(restart));
        for index in 0..limit {
            if elt_to_string(names, index) == field_name {
                return restart_owned
                    .try_vector_elt(index)
                    .map(|elt| elt.as_raw())
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
        }
    }
    if fallback_index < XLENGTH(restart) {
        restart_owned
            .try_vector_elt(fallback_index)
            .map(|elt| elt.as_raw())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    } else {
        R_NilValue()
    }
}

pub(crate) fn restart_entry(name: &str, handler: SEXP, exit: SEXP) -> SEXP {
    let handler_owned = restart_owned(handler);
    let exit_owned = restart_owned(exit);
    let handler = handler_owned.as_raw();
    let exit = exit_owned.as_raw();
    let restart = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
    if restart.is_null() {
        return R_NilValue();
    }
    let restart_guard = restart_owned(restart);
    let mut restart_mut = SexpMut::try_from_checked(restart_guard.clone())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let put = |i: R_xlen_t, value: SEXP, restart_mut: &mut SexpMut<'_>| {
        restart_mut
            .try_set_vector_elt(i, restart_owned(value))
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    };
    put(
        0,
        mk_string_cstr(&CString::new(name).unwrap_or_default()),
        &mut restart_mut,
    );
    put(1, exit, &mut restart_mut);
    put(2, handler, &mut restart_mut);
    put(3, mk_string_cstr(c""), &mut restart_mut);
    put(4, R_NilValue(), &mut restart_mut);
    put(5, R_NilValue(), &mut restart_mut);

    let names = string_vector(&[
        "name".to_string(),
        "exit".to_string(),
        "handler".to_string(),
        "description".to_string(),
        "test".to_string(),
        "interactive".to_string(),
    ]);
    let names_owned = restart_owned(names);
    crate::sexp::attrib_core::setAttrib(
        restart,
        install_str("names"),
        names_owned.as_raw(),
    );
    crate::sexp::attrib_core::setAttrib(
        restart,
        install_str("class"),
        mk_string_cstr(c"restart"),
    );
    restart
}

fn abort_restart_entry() -> SEXP {
    let restart = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    if restart.is_null() {
        return R_NilValue();
    }
    let restart_guard = restart_owned(restart);
    let mut restart_mut = SexpMut::try_from_checked(restart_guard.clone())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    restart_mut
        .try_set_vector_elt(
            0,
            restart_owned(mk_string_cstr(&CString::new("abort").unwrap_or_default())),
        )
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    restart_mut
        .try_set_vector_elt(1, restart_owned(R_NilValue()))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    crate::sexp::attrib_core::setAttrib(
        restart,
        install_str("class"),
        mk_string_cstr(c"restart"),
    );
    restart
}

fn is_restart_object(value: SEXP) -> bool {
    {
        !value.is_null()
            && value != R_NilValue()
            && TYPEOF(value) == SEXPTYPE::VECSXP
            && inherits_class(value, "restart")
    }
}
