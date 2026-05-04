/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/methods_list_dispatch.c
 *
 *  Rust-shaped S4 methods dispatch support.  The full methods package method
 *  selection algorithm is still intentionally small, but unsupported paths now
 *  fail explicitly instead of returning NULL as if dispatch had succeeded.
 */

use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::protect::*;

fn nil_value() -> SEXP {
    unsafe { R_NilValue() }
}

fn scalar_logical(value: c_int) -> SEXP {
    unsafe { Rf_ScalarLogical(value) }
}

fn r_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

unsafe fn named_element(object: SEXP, name: &str) -> SEXP {
    unsafe {
        if object.is_null() || object == R_NilValue() {
            return R_NilValue();
        }
        match TYPEOF(object) {
            kind if kind == SEXPTYPE::VECSXP.as_c_int() || kind == SEXPTYPE::EXPRSXP.as_c_int() => {
                let names =
                    crate::attrib_core::getAttrib(object, crate::attrib_core::R_NamesSymbol());
                if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
                    return R_NilValue();
                }
                for i in 0..LENGTH(names) {
                    let elt = STRING_ELT(names, i as R_xlen_t);
                    let ptr = if elt.is_null() {
                        ptr::null()
                    } else {
                        CHAR(elt)
                    };
                    if !ptr.is_null() && CStr::from_ptr(ptr).to_bytes() == name.as_bytes() {
                        return VECTOR_ELT(object, i as R_xlen_t);
                    }
                }
                R_NilValue()
            }
            kind if kind == SEXPTYPE::LISTSXP.as_c_int() => {
                let mut cell = object;
                while !cell.is_null() && cell != R_NilValue() {
                    let tag = TAG(cell);
                    if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                        let printname = PRINTNAME(tag);
                        if !printname.is_null()
                            && CStr::from_ptr(CHAR(printname)).to_bytes() == name.as_bytes()
                        {
                            return CAR(cell);
                        }
                    }
                    cell = CDR(cell);
                }
                R_NilValue()
            }
            _ => R_NilValue(),
        }
    }
}

unsafe fn all_methods_slot(mlist: SEXP) -> SEXP {
    unsafe { named_slot(mlist, "allMethods") }
}

unsafe fn named_slot(object: SEXP, name: &str) -> SEXP {
    unsafe {
        let Ok(name) = CString::new(name) else {
            return R_NilValue();
        };
        let charsxp = Rf_mkChar(name.as_ptr());
        crate::library::methods::slot::R_get_slot(object, charsxp)
    }
}

unsafe fn first_data_class_name(object: SEXP) -> Option<String> {
    unsafe {
        let class = crate::eval::attrib_core::R_data_class(object);
        if class.is_null()
            || class == R_NilValue()
            || TYPEOF(class) != SEXPTYPE::STRSXP
            || LENGTH(class) == 0
        {
            return None;
        }
        let elt = STRING_ELT(class, 0);
        if elt.is_null() {
            return None;
        }
        let ptr = CHAR(elt);
        (!ptr.is_null()).then(|| CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

unsafe fn scalar_signature_length(value: SEXP) -> Option<c_int> {
    unsafe {
        if value.is_null() || value == R_NilValue() || LENGTH(value) == 0 {
            return None;
        }
        match TYPEOF(value) {
            kind if kind == SEXPTYPE::INTSXP.as_c_int() => Some(*INTEGER(value)),
            kind if kind == SEXPTYPE::REALSXP.as_c_int() => Some(*REAL(value) as c_int),
            _ => None,
        }
    }
}

unsafe fn sexp_to_string(value: SEXP) -> Option<String> {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return None;
        }
        let charsxp = if TYPEOF(value) == SEXPTYPE::STRSXP && LENGTH(value) > 0 {
            STRING_ELT(value, 0)
        } else if TYPEOF(value) == SEXPTYPE::SYMSXP {
            PRINTNAME(value)
        } else if TYPEOF(value) == SEXPTYPE::CHARSXP {
            value
        } else {
            return None;
        };
        if charsxp.is_null() {
            return None;
        }
        let ptr = CHAR(charsxp);
        if ptr.is_null() {
            return None;
        }
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

/// R_initMethodDispatch - initialize method dispatch.
/// Called from the methods package on load.
pub unsafe fn R_initMethodDispatch(envir: SEXP) -> SEXP {
    let table_dispatch_on = with_methods_dispatch_state(|state| state.table_dispatch_on);
    unsafe {
        let standard_generic = if table_dispatch_on != 0 {
            Some(R_dispatchGeneric as unsafe fn(SEXP, SEXP, SEXP) -> SEXP)
        } else {
            Some(R_standardGeneric as unsafe fn(SEXP, SEXP, SEXP) -> SEXP)
        };
        let quick_check = if table_dispatch_on != 0 {
            Some(R_quick_dispatch as unsafe fn(SEXP, SEXP, SEXP) -> SEXP)
        } else {
            Some(R_quick_method_check as unsafe fn(SEXP, SEXP, SEXP) -> SEXP)
        };
        crate::mainutils::objects::R_set_standardGeneric_ptr(standard_generic, envir);
        crate::mainutils::objects::R_set_quick_method_check(quick_check);
        if envir.is_null() { R_NilValue() } else { envir }
    }
}

/// R_standardGeneric - C version of the standardGeneric R function.
/// Dispatches to the appropriate method for a generic function call.
pub unsafe fn R_standardGeneric(fname: SEXP, ev: SEXP, fdef: SEXP) -> SEXP {
    unsafe {
        let name = sexp_to_string(fname).unwrap_or_else(|| "<unknown>".to_string());
        let mlist = match TYPEOF(fdef) {
            kind if kind == SEXPTYPE::CLOSXP.as_c_int() => {
                let dot_methods = crate::sexp::symbol::Rf_install(c".Methods".as_ptr());
                let value = crate::sexp::envir::R_findVarInFrame(CLOENV(fdef), dot_methods);
                if value == R_UnboundValue() {
                    R_NilValue()
                } else {
                    value
                }
            }
            kind if kind == SEXPTYPE::SPECIALSXP.as_c_int()
                || kind == SEXPTYPE::BUILTINSXP.as_c_int() =>
            {
                crate::mainutils::objects::R_primitive_methods(fdef)
            }
            _ => r_error(format!(
                "invalid generic function object for method selection for function '{}'",
                name
            )),
        };

        let method = if mlist.is_null() || mlist == R_NilValue() {
            R_NilValue()
        } else if Rf_isFunction(mlist) != 0 {
            mlist
        } else {
            R_selectMethod(fname, ev, mlist, Rf_ScalarLogical(TRUE))
        };

        if method.is_null() || method == R_NilValue() {
            r_error(format!(
                "no direct or inherited method for function '{}' for this call",
                name
            ));
        }
        match TYPEOF(method) {
            kind if kind == SEXPTYPE::CLOSXP.as_c_int() => {
                crate::eval::missing::R_execMethod(method, ev)
            }
            kind if kind == SEXPTYPE::SPECIALSXP.as_c_int()
                || kind == SEXPTYPE::BUILTINSXP.as_c_int() =>
            {
                crate::mainutils::objects::R_deferred_default_method()
            }
            _ => r_error("invalid object (non-function) used as method"),
        }
    }
}

/// R_dispatchGeneric - table-based method dispatch.
pub unsafe fn R_dispatchGeneric(fname: SEXP, _ev: SEXP, _fdef: SEXP) -> SEXP {
    let name = unsafe { sexp_to_string(fname) }.unwrap_or_else(|| "<unknown>".to_string());
    r_error(format!(
        "table-based S4 method dispatch for generic '{}' is not implemented yet",
        name
    ));
}

/// R_quick_method_check - quick check if a method exists in the methods list.
pub unsafe fn R_quick_method_check(args: SEXP, mlist: SEXP, _fdef: SEXP) -> SEXP {
    unsafe {
        if mlist.is_null() || mlist == R_NilValue() {
            return R_NilValue();
        }
        let mut methods = all_methods_slot(mlist);
        if methods.is_null() || methods == R_NilValue() {
            return R_NilValue();
        }
        let mut arg = args;
        while !arg.is_null() && arg != R_NilValue() && !methods.is_null() && methods != R_NilValue()
        {
            let object = CAR(arg);
            let Some(class) = first_data_class_name(object) else {
                return R_NilValue();
            };
            let value = named_element(methods, &class);
            if value.is_null() || value == R_NilValue() || Rf_isFunction(value) != 0 {
                return value;
            }
            methods = all_methods_slot(value);
            arg = CDR(arg);
        }
        R_NilValue()
    }
}

/// R_quick_dispatch - quick table-based dispatch for primitives.
pub unsafe fn R_quick_dispatch(args: SEXP, generic_env: SEXP, _fdef: SEXP) -> SEXP {
    unsafe {
        if generic_env.is_null()
            || generic_env == R_NilValue()
            || TYPEOF(generic_env) != SEXPTYPE::ENVSXP
        {
            return R_NilValue();
        }
        let all_mtable = crate::sexp::symbol::Rf_install(c".AllMTable".as_ptr());
        let sig_length = crate::sexp::symbol::Rf_install(c".SigLength".as_ptr());
        let mtable = crate::sexp::envir::R_findVarInFrame(generic_env, all_mtable);
        if mtable == R_UnboundValue() || TYPEOF(mtable) != SEXPTYPE::ENVSXP {
            return R_NilValue();
        }
        let nsig_value = crate::sexp::envir::R_findVarInFrame(generic_env, sig_length);
        let Some(nsig) = scalar_signature_length(nsig_value) else {
            return R_NilValue();
        };
        if nsig < 0 {
            return R_NilValue();
        }

        let mut classes = Vec::with_capacity(nsig as usize);
        let mut current = args;
        while !current.is_null() && current != R_NilValue() && classes.len() < nsig as usize {
            let object = CAR(current);
            let class = if object == R_MissingArg() {
                "missing".to_string()
            } else {
                let Some(class) = first_data_class_name(object) else {
                    return R_NilValue();
                };
                class
            };
            classes.push(class);
            current = CDR(current);
        }
        while classes.len() < nsig as usize {
            classes.push("missing".to_string());
        }
        let label = classes.join("#");
        let Ok(label) = CString::new(label) else {
            return R_NilValue();
        };
        let symbol = crate::sexp::symbol::Rf_install(label.as_ptr());
        let value = crate::sexp::envir::R_findVarInFrame(mtable, symbol);
        if value == R_UnboundValue() {
            R_NilValue()
        } else {
            value
        }
    }
}

/// R_getGeneric - get the generic function definition for a given name.
pub unsafe fn R_getGeneric(name: SEXP, mustFind: SEXP, env: SEXP, _package: SEXP) -> SEXP {
    unsafe {
        let Some(name_string) = sexp_to_string(name) else {
            r_error("The argument \"f\" to getGeneric must be a single string or symbol");
        };
        if env.is_null() || env == R_NilValue() || TYPEOF(env) != SEXPTYPE::ENVSXP {
            if crate::mainutils::coerce::asLogical(mustFind) != 0 {
                r_error(format!(
                    "no generic function definition found for '{}'",
                    name_string
                ));
            }
            return R_NilValue();
        }
        let cname = CString::new(name_string.as_str()).unwrap_or_default();
        let symbol = crate::sexp::symbol::Rf_install(cname.as_ptr());
        let value = crate::sexp::envir::R_findVarInFrame(env, symbol);
        if value == R_UnboundValue() {
            if crate::mainutils::coerce::asLogical(mustFind) != 0 {
                r_error(format!(
                    "no generic function definition found for '{}' in the supplied environment",
                    name_string
                ));
            }
            R_NilValue()
        } else {
            value
        }
    }
}

/// R_missingArg - check if an argument is missing in a method call.
/// Ported from R's R_missingArg() in methods_list_dispatch.c.
pub unsafe fn R_missingArg(symbol: SEXP, ev: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
            r_error("invalid symbol in checking for missing argument in method dispatch");
        }
        if ev.is_null() || ev == R_NilValue() {
            r_error("use of NULL environment is defunct");
        }
        if TYPEOF(ev) != SEXPTYPE::ENVSXP {
            r_error("invalid environment in checking for missing argument in method dispatch");
        }
        let res = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
        let _res_guard = protect(res);
        let ip = LOGICAL(res);

        let val = crate::sexp::envir::R_findVarInFrame(ev, symbol);
        if val == crate::sexp::globals::R_MissingArg() {
            *ip.add(0) = 1; // TRUE
        } else {
            *ip.add(0) = 0; // FALSE
        }
        res
    }
}

/// R_selectMethod - select a method for the given call.
pub unsafe fn R_selectMethod(fname: SEXP, _ev: SEXP, mlist: SEXP, _evalArgs: SEXP) -> SEXP {
    unsafe {
        if !mlist.is_null() && mlist != R_NilValue() && Rf_isFunction(mlist) != 0 {
            return mlist;
        }
    }
    unsafe {
        let eval_args = crate::mainutils::coerce::asLogical(_evalArgs) != 0;
        select_method_from_list(fname, _ev, mlist, eval_args, true)
    }
}

unsafe fn select_method_from_list(
    fname: SEXP,
    ev: SEXP,
    mlist: SEXP,
    eval_args: bool,
    first_try: bool,
) -> SEXP {
    unsafe {
        if mlist.is_null() || mlist == R_NilValue() {
            return R_NilValue();
        }
        if Rf_isFunction(mlist) != 0 {
            return mlist;
        }
        let arg_slot = named_slot(mlist, "argument");
        let Some(arg_name) = sexp_to_string(arg_slot) else {
            let name = sexp_to_string(fname).unwrap_or_else(|| "<unknown>".to_string());
            r_error(format!(
                "object used as methods list for function '{}' has no 'argument' slot",
                name
            ));
        };
        if ev.is_null() || ev == R_NilValue() || TYPEOF(ev) != SEXPTYPE::ENVSXP {
            let name = sexp_to_string(fname).unwrap_or_else(|| "<unknown>".to_string());
            r_error(format!(
                "the 'environment' argument for dispatch for function '{}' must be an R environment",
                name
            ));
        }
        let arg_symbol = crate::sexp::symbol::Rf_install(
            CString::new(arg_name.as_str()).unwrap_or_default().as_ptr(),
        );
        let arg_value = crate::sexp::envir::R_findVarInFrame(ev, arg_symbol);
        let class = if arg_value == R_UnboundValue() || arg_value == R_MissingArg() {
            "missing".to_string()
        } else if eval_args {
            let Some(class) = first_data_class_name(arg_value) else {
                return R_NilValue();
            };
            class
        } else {
            let Some(class) = sexp_to_string(arg_value) else {
                return R_NilValue();
            };
            class
        };
        let methods = all_methods_slot(mlist);
        if methods.is_null() || methods == R_NilValue() {
            return R_NilValue();
        }
        let method = named_element(methods, &class);
        if method.is_null() || method == R_NilValue() {
            if first_try {
                return R_NilValue();
            }
            let name = sexp_to_string(fname).unwrap_or_else(|| "<unknown>".to_string());
            r_error(format!(
                "no matching method for function '{}' with class \"{}\"",
                name, class
            ));
        }
        if Rf_isFunction(method) != 0 {
            method
        } else {
            select_method_from_list(fname, ev, method, eval_args, first_try)
        }
    }
}

/// R_M_setPrimitiveMethods - set methods for a primitive function.
pub unsafe fn R_M_setPrimitiveMethods(
    fname: SEXP,
    op: SEXP,
    code_vec: SEXP,
    fundef: SEXP,
    mlist: SEXP,
) -> SEXP {
    unsafe { crate::mainutils::objects::R_set_prim_method(fname, op, code_vec, fundef, mlist) }
}

/// R_nextMethodCall - implement .nextMethod() (callNextMethod).
pub unsafe fn R_nextMethodCall(_matched_call: SEXP, _ev: SEXP) -> SEXP {
    r_error("callNextMethod/.nextMethod dispatch is not implemented yet");
}

pub(crate) struct MethodsDispatchState {
    n_overrides: c_int,
    table_dispatch_on: c_int,
}

impl Default for MethodsDispatchState {
    fn default() -> Self {
        Self {
            n_overrides: 0,
            table_dispatch_on: 0,
        }
    }
}

fn with_methods_dispatch_state<R>(f: impl FnOnce(&mut MethodsDispatchState) -> R) -> R {
    with_required_current_instance(|instance| f(&mut instance.methods_dispatch_state))
}

pub extern "C" fn R_clear_method_selection() -> SEXP {
    with_methods_dispatch_state(|state| state.n_overrides = 0);
    nil_value()
}

pub extern "C" fn R_set_method_dispatch(onOff: SEXP) -> SEXP {
    unsafe {
        let prev = with_methods_dispatch_state(|state| state.table_dispatch_on);
        let value = if TYPEOF(onOff) == SEXPTYPE::LGLSXP && LENGTH(onOff) >= 1 {
            LOGICAL_ELT(onOff, 0)
        } else {
            0 // NA_LOGICAL treated as "return previous"
        };
        if value != std::os::raw::c_int::MIN {
            // NA_LOGICAL == INT_MIN
            with_methods_dispatch_state(|state| {
                state.table_dispatch_on = if value != 0 { 1 } else { 0 }
            });
        }
        scalar_logical(prev)
    }
}

/// R_methodsPackageMetaName - construct a meta-data object name.
/// Format: .__prefix__name or .__prefix__name:pkg
/// Ported from R's R_methodsPackageMetaName() in methods_list_dispatch.c.
pub unsafe fn R_methodsPackageMetaName(prefix: SEXP, name: SEXP, pkg: SEXP) -> SEXP {
    unsafe {
        // Extract strings
        let prefix_str =
            if !prefix.is_null() && TYPEOF(prefix) == SEXPTYPE::STRSXP && LENGTH(prefix) >= 1 {
                let s = STRING_ELT(prefix, 0);
                if !s.is_null() { CHAR(s) } else { ptr::null() }
            } else {
                ptr::null()
            };
        let name_str = if !name.is_null() && TYPEOF(name) == SEXPTYPE::STRSXP && LENGTH(name) >= 1 {
            let s = STRING_ELT(name, 0);
            if !s.is_null() { CHAR(s) } else { ptr::null() }
        } else {
            ptr::null()
        };
        let pkg_str = if !pkg.is_null() && TYPEOF(pkg) == SEXPTYPE::STRSXP && LENGTH(pkg) >= 1 {
            let s = STRING_ELT(pkg, 0);
            if !s.is_null() { CHAR(s) } else { ptr::null() }
        } else {
            ptr::null()
        };

        let prefix_c = if !prefix_str.is_null() {
            std::ffi::CStr::from_ptr(prefix_str).to_str().unwrap_or("")
        } else {
            ""
        };
        let name_c = if !name_str.is_null() {
            std::ffi::CStr::from_ptr(name_str).to_str().unwrap_or("")
        } else {
            ""
        };
        let pkg_c = if !pkg_str.is_null() {
            std::ffi::CStr::from_ptr(pkg_str).to_str().unwrap_or("")
        } else {
            ""
        };

        let res_str = if !pkg_c.is_empty() {
            format!(".__{}__{}:{}", prefix_c, name_c, pkg_c)
        } else {
            format!(".__{}__{}", prefix_c, name_c)
        };

        let c_str = std::ffi::CString::new(res_str)
            .unwrap_or_else(|_| std::ffi::CString::new("").unwrap_or_default());
        Rf_mkString(c_str.as_ptr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::{RInstance, replace_current_instance};

    fn assert_r_error(action: impl FnOnce()) -> RError {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("expected RError panic");
        payload
            .downcast_ref::<RError>()
            .expect("expected RError payload")
            .clone()
    }

    unsafe fn named_list(entries: &[(&str, SEXP)]) -> SEXP {
        unsafe {
            let out = Rf_allocVector(SEXPTYPE::VECSXP, entries.len() as c_int);
            let _out_guard = protect(out);
            let names = Rf_allocVector(SEXPTYPE::STRSXP, entries.len() as c_int);
            let _names_guard = protect(names);
            for (i, (name, value)) in entries.iter().enumerate() {
                SET_VECTOR_ELT(out, i as R_xlen_t, *value);
                let cname = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(cname.as_ptr()));
            }
            crate::eval::attrib_core::setAttrib(
                out,
                crate::eval::attrib_core::R_NamesSymbol(),
                names,
            );
            out
        }
    }

    unsafe fn methods_list(all_methods: SEXP) -> SEXP {
        unsafe { named_list(&[("allMethods", all_methods)]) }
    }

    unsafe fn methods_list_for_argument(argument: &str, all_methods: SEXP) -> SEXP {
        unsafe {
            let arg = crate::sexp::symbol::Rf_install(
                CString::new(argument).unwrap_or_default().as_ptr(),
            );
            named_list(&[("argument", arg), ("allMethods", all_methods)])
        }
    }

    unsafe fn env_with(bindings: &[(&str, SEXP)]) -> SEXP {
        unsafe {
            let env =
                crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_NilValue(), R_NilValue());
            let _env_guard = protect(env);
            for (name, value) in bindings {
                let cname = CString::new(*name).unwrap_or_default();
                let sym = crate::sexp::symbol::Rf_install(cname.as_ptr());
                crate::sexp::envir::defineVar(sym, *value, env);
            }
            env
        }
    }

    #[test]
    fn methods_dispatch_state_is_session_local() {
        let mut first = RInstance::new();
        let mut second = RInstance::new();

        unsafe {
            let previous = replace_current_instance(Some(&mut first as *mut RInstance));
            first.methods_dispatch_state.n_overrides = 7;
            let on = Rf_ScalarLogical(1);
            assert_eq!(*LOGICAL(R_set_method_dispatch(on)), 0);
            assert_eq!(
                with_methods_dispatch_state(|state| state.table_dispatch_on),
                1
            );
            R_clear_method_selection();
            replace_current_instance(previous);

            let previous = replace_current_instance(Some(&mut second as *mut RInstance));
            assert_eq!(
                with_methods_dispatch_state(|state| state.table_dispatch_on),
                0
            );
            let off = Rf_ScalarLogical(0);
            assert_eq!(*LOGICAL(R_set_method_dispatch(off)), 0);
            replace_current_instance(previous);
        }

        assert_eq!(first.methods_dispatch_state.n_overrides, 0);
        assert_eq!(first.methods_dispatch_state.table_dispatch_on, 1);
        assert_eq!(second.methods_dispatch_state.n_overrides, 0);
        assert_eq!(second.methods_dispatch_state.table_dispatch_on, 0);
    }

    #[test]
    fn init_method_dispatch_registers_session_local_dispatchers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::mainutils::objects::R_set_standardGeneric_ptr(None, ptr::null_mut());
            assert_eq!(crate::mainutils::objects::isMethodsDispatchOn(), FALSE);
            let result = R_initMethodDispatch(R_NilValue());
            assert_eq!(result, R_NilValue());
            assert_eq!(crate::mainutils::objects::isMethodsDispatchOn(), TRUE);
            crate::mainutils::objects::R_set_standardGeneric_ptr(None, ptr::null_mut());
        }
    }

    #[test]
    fn standard_generic_errors_instead_of_returning_null() {
        let _session = crate::sexp::session::RSession::new();
        let err = assert_r_error(|| unsafe {
            let name = Rf_mkString(b"show\0".as_ptr() as *const std::os::raw::c_char);
            R_standardGeneric(name, R_NilValue(), R_NilValue());
        });
        assert!(err.message.contains("invalid generic function object"));
    }

    #[test]
    fn standard_generic_executes_direct_closure_method() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let method = crate::mainutils::dstruct::mkCLOSXP(
                R_NilValue(),
                Rf_ScalarInteger(42),
                R_NilValue(),
            );
            let methods = named_list(&[("integer", method)]);
            let mlist = methods_list_for_argument("x", methods);
            let f_env = env_with(&[(".Methods", mlist)]);
            let fdef = crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), R_NilValue(), f_env);
            let ev = env_with(&[("x", Rf_ScalarInteger(1))]);
            let name = Rf_mkString(c"show".as_ptr());

            let result = R_standardGeneric(name, ev, fdef);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result), 42);
        }
    }

    #[test]
    fn quick_method_check_returns_direct_class_method() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let method =
                crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), R_NilValue(), R_NilValue());
            let methods = named_list(&[("integer", method)]);
            let mlist = methods_list(methods);
            let object = Rf_ScalarInteger(1);
            let args = Rf_cons(object, R_NilValue());

            let result = R_quick_method_check(args, mlist, R_NilValue());
            assert_eq!(result, method);
        }
    }

    #[test]
    fn quick_method_check_returns_nil_for_missing_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let method =
                crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), R_NilValue(), R_NilValue());
            let methods = named_list(&[("character", method)]);
            let mlist = methods_list(methods);
            let object = Rf_ScalarInteger(1);
            let args = Rf_cons(object, R_NilValue());

            let result = R_quick_method_check(args, mlist, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn quick_dispatch_returns_table_method_with_missing_fill() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let method =
                crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), R_NilValue(), R_NilValue());
            let mtable = env_with(&[("integer#missing", method)]);
            let generic_env =
                env_with(&[(".AllMTable", mtable), (".SigLength", Rf_ScalarInteger(2))]);
            let args = Rf_cons(Rf_ScalarInteger(1), R_NilValue());

            let result = R_quick_dispatch(args, generic_env, R_NilValue());
            assert_eq!(result, method);
        }
    }

    #[test]
    fn quick_dispatch_returns_nil_for_missing_table_entry() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mtable = env_with(&[]);
            let generic_env =
                env_with(&[(".AllMTable", mtable), (".SigLength", Rf_ScalarInteger(1))]);
            let args = Rf_cons(Rf_ScalarInteger(1), R_NilValue());

            let result = R_quick_dispatch(args, generic_env, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn select_method_returns_direct_match_from_environment_argument() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let method =
                crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), R_NilValue(), R_NilValue());
            let methods = named_list(&[("integer", method)]);
            let mlist = methods_list_for_argument("x", methods);
            let env = env_with(&[("x", Rf_ScalarInteger(1))]);
            let fname = Rf_mkString(c"show".as_ptr());

            let result = R_selectMethod(fname, env, mlist, Rf_ScalarLogical(TRUE));
            assert_eq!(result, method);
        }
    }

    #[test]
    fn select_method_can_use_precomputed_class_string() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let method =
                crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), R_NilValue(), R_NilValue());
            let methods = named_list(&[("integer", method)]);
            let mlist = methods_list_for_argument("x", methods);
            let class = Rf_mkString(c"integer".as_ptr());
            let env = env_with(&[("x", class)]);
            let fname = Rf_mkString(c"show".as_ptr());

            let result = R_selectMethod(fname, env, mlist, Rf_ScalarLogical(FALSE));
            assert_eq!(result, method);
        }
    }

    #[test]
    fn missing_arg_reports_bound_missing_argument() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = env_with(&[("x", crate::sexp::globals::R_MissingArg())]);
            let sym = crate::sexp::symbol::Rf_install(c"x".as_ptr());

            let result = R_missingArg(sym, env);
            assert_eq!(*LOGICAL(result), TRUE);
        }
    }

    #[test]
    fn missing_arg_rejects_non_symbol() {
        let _session = crate::sexp::session::RSession::new();
        let err = assert_r_error(|| unsafe {
            R_missingArg(Rf_mkString(c"x".as_ptr()), R_NilValue());
        });
        assert!(err.message.contains("invalid symbol"));
    }

    #[test]
    fn class_cache_reads_session_local_s4_registry() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::mainutils::objects::register_s4_class(
                "CacheClass".to_string(),
                Vec::new(),
                false,
            );
            let class = Rf_mkString(b"CacheClass\0".as_ptr() as *const std::os::raw::c_char);
            let result = R_getClassFromCache(class, R_NilValue());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
        }
    }
}

/// R_identC - test if two single-string objects are identical at the C level.
/// Ported from R's R_identC() in methods_list_dispatch.c.
pub unsafe fn R_identC(e1: SEXP, e2: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(e1) == SEXPTYPE::STRSXP
            && TYPEOF(e2) == SEXPTYPE::STRSXP
            && LENGTH(e1) == 1
            && LENGTH(e2) == 1
        {
            let s1 = STRING_ELT(e1, 0);
            let s2 = STRING_ELT(e2, 0);
            if s1 == s2 {
                return scalar_logical(1); // TRUE
            }
        }
        scalar_logical(0) // FALSE
    }
}

/// R_getClassFromCache - look up a class definition in the class cache table.
pub unsafe fn R_getClassFromCache(class: SEXP, _table: SEXP) -> SEXP {
    unsafe {
        let Some(class_name) = sexp_to_string(class) else {
            return R_NilValue();
        };
        if crate::mainutils::objects::s4_class(&class_name).is_none() {
            return R_NilValue();
        }
        let cname = CString::new(class_name).unwrap_or_default();
        Rf_mkString(cname.as_ptr())
    }
}

/// asChar - local helper to coerce to a single CHARSXP.
unsafe fn asChar(x: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::STRSXP && LENGTH(x) >= 1 {
            STRING_ELT(x, 0)
        } else if TYPEOF(x) == SEXPTYPE::SYMSXP {
            PRINTNAME(x)
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
            x
        } else {
            nil_value()
        }
    }
}

/// R_el_named - get a named element from a list (no partial matching).
/// Ported from R's R_el_named() in methods_list_dispatch.c.
pub unsafe fn R_el_named(object: SEXP, what: SEXP) -> SEXP {
    unsafe {
        let w = asChar(what);
        if w.is_null() {
            return nil_value();
        }
        let str = CHAR(w);
        if str.is_null() {
            return nil_value();
        }
        let target = std::ffi::CStr::from_ptr(str).to_bytes();

        // Get the names attribute of object
        let names = crate::attrib_core::getAttrib(object, crate::attrib_core::R_NamesSymbol());
        let n = LENGTH(names);
        if n > 0 {
            for i in 0..n {
                let s = STRING_ELT(names, i as std::os::raw::c_longlong);
                if !s.is_null() {
                    let c = CHAR(s);
                    if !c.is_null() {
                        let name_bytes = std::ffi::CStr::from_ptr(c).to_bytes();
                        if name_bytes == target {
                            return VECTOR_ELT(object, i as std::os::raw::c_longlong);
                        }
                    }
                }
            }
        }
        nil_value()
    }
}

/// R_set_el_named - set a named element in a list.
/// Ported from R's R_set_el_named() in methods_list_dispatch.c.
pub unsafe fn R_set_el_named(object: SEXP, what: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let w = asChar(what);
        if w.is_null() {
            return nil_value();
        }
        let str = CHAR(w);
        if str.is_null() {
            return nil_value();
        }
        let sym = crate::sexp::symbol::Rf_install(str);
        // Delegate to R_subassign3_dflt (the $<- default method)
        crate::main::subassign::R_subassign3_dflt(nil_value(), object, sym, value)
    }
}
