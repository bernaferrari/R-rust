// Environment binding and locking builtins. Kept behind the historical
// `essentials::do_*` re-export so the builtin registration table stays stable.
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
use super::*;

/// R's `lockBinding(sym, env)` — lock a binding in an environment.
pub unsafe fn do_lockBinding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let env = environment_arg(CAR(CDR(args)));
        if crate::sexp::envir::R_findVarInFrame(env, sym) == R_UnboundValue() {
            base_error("no binding for symbol");
        }
        crate::sexp::envir::lock_binding_raw(env, sym);
        R_NilValue()
    }
}

/// R's `unlockBinding(sym, env)` — unlock a binding in an environment.
pub unsafe fn do_unlockBinding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let env = environment_arg(CAR(CDR(args)));
        crate::sexp::envir::unlock_binding_raw(env, sym);
        R_NilValue()
    }
}

/// R's `bindingIsLocked(sym, env)` — check if a binding is locked.
pub unsafe fn do_bindingIsLocked(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let env = environment_arg(CAR(CDR(args)));
        Rf_ScalarLogical(crate::sexp::envir::binding_is_locked_raw(env, sym) as c_int)
    }
}

/// R's `bindingIsActive(sym, env)` — check if a binding is active.
pub unsafe fn do_bindingIsActive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let env = environment_arg(CAR(CDR(args)));
        if !crate::sexp::envir::binding_exists_in_frame_raw(env, sym) {
            base_error("no binding for symbol");
        }
        Rf_ScalarLogical(crate::sexp::envir::binding_is_active_raw(env, sym) as c_int)
    }
}

/// R's `makeActiveBinding(sym, fun, env)` — create an active binding.
pub unsafe fn do_makeActiveBinding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let fun = CAR(CDR(args));
        let env = environment_arg(CAR(CDR(CDR(args))));
        if !is_function_value(fun) {
            base_error("not a function");
        }
        crate::sexp::envir::make_active_binding_raw(env, sym, fun);
        R_NilValue()
    }
}

/// R's `lockEnvironment(env, bindings)` — lock an environment.
pub unsafe fn do_lockEnvironment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = environment_arg(CAR(args));
        crate::sexp::envir::lock_environment_raw(env);

        let lock_bindings = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            false
        } else {
            real_or_default(CAR(CDR(args)), 0.0) != 0.0
        };
        if lock_bindings {
            let mut frame = FRAME(env);
            while !frame.is_null() && frame != R_NilValue() {
                let tag = TAG(frame);
                if !tag.is_null() && tag != R_NilValue() {
                    crate::sexp::envir::lock_binding_raw(env, tag);
                }
                frame = CDR(frame);
            }
        }

        R_NilValue()
    }
}

/// R's `environmentIsLocked(env)` — check if an environment is locked.
pub unsafe fn do_environmentIsLocked(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = environment_arg(CAR(args));
        Rf_ScalarLogical(crate::sexp::envir::environment_is_locked_raw(env) as c_int)
    }
}

unsafe fn environment_arg(value: SEXP) -> SEXP {
    unsafe {
        if value.is_null() || value == R_NilValue() || TYPEOF(value) != SEXPTYPE::ENVSXP {
            base_error("not an environment");
        }
        value
    }
}

unsafe fn binding_symbol_arg(value: SEXP) -> SEXP {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            base_error("invalid symbol");
        }
        match TYPEOF(value) {
            t if t == SEXPTYPE::SYMSXP.as_c_int() => value,
            t if t == SEXPTYPE::STRSXP.as_c_int() && XLENGTH(value) > 0 => {
                let name = elt_to_string(value, 0);
                let c_name = CString::new(name).unwrap_or_default();
                Rf_install(c_name.as_ptr())
            }
            _ => base_error("invalid symbol"),
        }
    }
}
