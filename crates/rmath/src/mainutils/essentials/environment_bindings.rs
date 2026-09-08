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

/// R's `list2env(x, envir = NULL, parent = parent.frame(),
/// hash = (length(x) > 100), size = max(29L, length(x)))` — transfer the
/// named elements of a list (or pairlist) into `envir` as bindings
/// (upstream `base::list2env` wraps `.Internal(list2env(x, envir))`;
/// defineVar per element, last one wins). Returns the environment.
pub unsafe fn do_list2env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let envir_arg = if !CDR(args).is_null() && CDR(args) != R_NilValue() {
            CAR(CDR(args))
        } else {
            R_NilValue()
        };

        let x_type = TYPEOF(x);
        let is_list = x_type == SEXPTYPE::VECSXP.as_c_int();
        let is_pairlist =
            x_type == SEXPTYPE::LISTSXP.as_c_int() || x_type == SEXPTYPE::NILSXP.as_c_int();
        if !is_list && !is_pairlist {
            base_error("invalid 'x' argument");
        }

        let envir = if envir_arg.is_null() || envir_arg == R_NilValue() {
            // envir = NULL: fresh env under parent = parent.frame() (the
            // caller's evaluation frame for a builtin).
            let parent = if !_rho.is_null() && _rho != R_NilValue() {
                _rho
            } else {
                crate::sexp::globals::R_GlobalEnv()
            };
            let _guard = protect(parent);
            crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue())
        } else if TYPEOF(envir_arg) == SEXPTYPE::ENVSXP.as_c_int() {
            envir_arg
        } else {
            base_error("invalid 'envir' argument");
        };
        let _envir_guard = protect(envir);

        if is_pairlist {
            let mut cell = x;
            while !cell.is_null() && cell != R_NilValue() {
                let tag = TAG(cell);
                if tag.is_null() || tag == R_NilValue() {
                    base_error("'x' must be a named list or pairlist");
                }
                crate::sexp::envir::defineVar(tag, CAR(cell), envir);
                cell = CDR(cell);
            }
        } else {
            let n = XLENGTH(x);
            let names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            let _names_guard = protect(names);
            if n > 0 && (names.is_null() || names == R_NilValue() || XLENGTH(names) != n) {
                base_error("'x' must be a named list or pairlist");
            }
            for i in 0..n {
                let name = elt_to_string(names, i);
                if name.is_empty() || name == "NA" {
                    base_error("'x' must be a named list or pairlist");
                }
                let Ok(name_cstr) = CString::new(name) else {
                    base_error("'x' must be a named list or pairlist");
                };
                let sym = Rf_install(name_cstr.as_ptr());
                crate::sexp::envir::defineVar(sym, VECTOR_ELT(x, i), envir);
            }
        }

        envir
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
