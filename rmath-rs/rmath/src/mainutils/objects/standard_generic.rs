#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables, unused_imports)]

use super::*;

// ---------------------------------------------------------------------------
// standardGeneric infrastructure
// ---------------------------------------------------------------------------

/// Get the current standardGeneric function pointer.
pub(crate) unsafe fn R_get_standardGeneric_ptr() -> R_stdGen_ptr_t {
    with_objects_state(|state| state.standard_generic_ptr)
}

/// Set the standardGeneric function pointer.
pub unsafe fn R_set_standardGeneric_ptr(val: R_stdGen_ptr_t, _envir: SEXP) -> R_stdGen_ptr_t {
    with_objects_state(|state| {
        let old = state.standard_generic_ptr;
        state.standard_generic_ptr = val;
        old
    })
}

/// Check whether S4 methods dispatch is currently enabled.
pub unsafe fn isMethodsDispatchOn() -> c_int {
    match with_objects_state(|state| state.standard_generic_ptr) {
        None => FALSE,
        Some(_) => TRUE,
    }
}

/// do_S4on -- primitive for .isMethodsDispatchOn()
pub unsafe fn do_S4on(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || length(args) == 0 {
            return Rf_ScalarLogical(isMethodsDispatchOn());
        }
        Rf_ScalarLogical(isMethodsDispatchOn())
    }
}

/// dispatchNonGeneric -- dispatch the non-generic definition of a function.
///
/// Used to trap calls to standardGeneric during the loading of the methods package.
/// Searches the enclosing environments for a non-generic version of the function,
/// then evaluates a call to it with the same arguments.
///
/// Ported from objects.c:1275-1319.
pub(crate) unsafe fn dispatchNonGeneric(name: SEXP, env: SEXP, _fdef: SEXP) -> SEXP {
    unsafe {
        let symbol = crate::mainutils::subset::installTrChar(asChar(name));

        // Search enclosing environments for a non-generic version
        let mut fun = R_UnboundValue();
        let mut rho = ENCLOS(env);
        while rho != R_EmptyEnv() && !rho.is_null() {
            let val = crate::sexp::envir::R_findVarInFrame(rho, symbol);
            if val == R_UnboundValue() {
                rho = ENCLOS(rho);
                continue;
            }
            let t = TYPEOF(val);
            if t == SEXPTYPE::CLOSXP {
                let gen_attr = crate::sexp::envir::R_findVarInFrame(CLOENV(val), sym("Generic"));
                if gen_attr != R_UnboundValue() {
                    rho = ENCLOS(rho);
                    continue;
                }
                fun = val;
                break;
            }
            rho = ENCLOS(rho);
            continue;
        }

        // Fall back to the symbol's global value
        if fun == R_UnboundValue() {
            fun = SYMVALUE(symbol);
        }
        if fun == R_UnboundValue() {
            let name_str = if !name.is_null() && TYPEOF(name) == SEXPTYPE::STRSXP {
                let c = CHAR(STRING_ELT(name, 0));
                if !c.is_null() {
                    std::ffi::CStr::from_ptr(c).to_str().unwrap_or("<unknown>")
                } else {
                    "<unknown>"
                }
            } else {
                "<unknown>"
            };
            error(&format!(
                "unable to find a non-generic version of function \"{}\"",
                name_str
            ));
        }

        // Find the calling context matching env
        let mut cptr = R_GlobalContext();
        while !cptr.is_null() {
            let cf = (*cptr).callflag;
            if (cf & crate::sexp::context::ctxt_flags::CTXT_FUNCTION) != 0 && (*cptr).cloenv == env
            {
                break;
            }
            cptr = (*cptr).nextcontext;
        }

        if cptr.is_null() {
            return R_NilValue();
        }

        // Duplicate the call and replace the function with the non-generic
        let e = crate::mainutils::duplicate::shallow_duplicate(crate::eval::context::R_syscall(
            0, cptr,
        ));
        let _e_guard = protect(e);
        SETCAR(e, fun);

        let value = crate::eval::eval::Rf_eval(e, (*cptr).sysparent);
        value
    }
}

// ---------------------------------------------------------------------------
// get_this_generic -- walk context stack to find the generic function
// ---------------------------------------------------------------------------

/// Walk the context stack looking for a function with a "generic" attribute
/// matching the supplied name. If a second argument is provided, return it
/// directly as the function definition.
///
/// Ported from objects.c:1540-1562.
unsafe fn get_this_generic(args: SEXP) -> SEXP {
    unsafe {
        let cdr_args = CDR(args);
        if !cdr_args.is_null() && cdr_args != R_NilValue() {
            return CAR(cdr_args);
        }

        let fname = STRING_ELT(CAR(args), 0);

        let mut cptr = R_GlobalContext();
        while !cptr.is_null() {
            let cf = (*cptr).callflag;
            if (cf & crate::sexp::context::ctxt_flags::CTXT_FUNCTION) != 0
                && isObject((*cptr).callfun) != FALSE
            {
                let generic = getAttrib((*cptr).callfun, sym("generic"));
                if isValidString(generic) != FALSE && Seql(fname, STRING_ELT(generic, 0)) != FALSE {
                    return (*cptr).callfun;
                }
            }
            cptr = (*cptr).nextcontext;
        }

        R_NilValue()
    }
}

/// do_standardGeneric -- standardGeneric() .Internal
///
/// Ported from objects.c:1324-1370.
pub unsafe fn do_standardGeneric(call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            error("'standardGeneric' requires a generic function name");
        }
        let arg = CAR(args);
        if isValidString(arg) == FALSE {
            error("argument to 'standardGeneric' must be a non-empty character string");
        }

        let fdef = get_this_generic(args);
        if fdef.is_null() || fdef == R_NilValue() {
            let generic = CHAR(STRING_ELT(arg, 0));
            let generic = if generic.is_null() {
                "<unknown>"
            } else {
                std::ffi::CStr::from_ptr(generic)
                    .to_str()
                    .unwrap_or("<unknown>")
            };
            error(&format!(
                "call to standardGeneric(\"{}\") apparently not from the body of that generic function",
                generic
            ));
        }

        // Route through the methods package dispatch when it has been
        // initialized; otherwise initialize it on first use. The bare
        // dispatchNonGeneric fallback cannot dispatch S4 methods and would
        // recurse forever when the name still resolves to the generic, so
        // only take that path for genuinely non-generic functions.
        let dispatch = crate::library::methods::methods_list_dispatch::standard_generic_dispatch();
        if std::ptr::fn_addr_eq(
            dispatch,
            dispatchNonGeneric as unsafe fn(SEXP, SEXP, SEXP) -> SEXP,
        ) {
            let generic_marker = sym("generic");
            let gen_attr = getAttrib(fdef, generic_marker);
            let env_marker = if TYPEOF(fdef) == SEXPTYPE::CLOSXP {
                crate::sexp::envir::R_findVarInFrame(CLOENV(fdef), sym("Generic"))
            } else {
                R_UnboundValue()
            };
            let is_generic_fdef =
                (gen_attr != R_NilValue() && !gen_attr.is_null()) || env_marker != R_UnboundValue();
            if is_generic_fdef {
                let raw = CHAR(STRING_ELT(arg, 0));
                let name_str = if raw.is_null() {
                    "<unknown>"
                } else {
                    CStr::from_ptr(raw).to_str().unwrap_or("<unknown>")
                };
                error(&format!(
                    "no direct or inherited method for function '{}' for this call",
                    name_str
                ));
            }
        }
        dispatch(arg, env, fdef)
    }
}

