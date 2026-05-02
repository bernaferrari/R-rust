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
pub unsafe fn R_standardGeneric(fname: SEXP, _ev: SEXP, _fdef: SEXP) -> SEXP {
    let name = unsafe { sexp_to_string(fname) }.unwrap_or_else(|| "<unknown>".to_string());
    r_error(format!(
        "S4 method selection for generic '{}' is not implemented yet",
        name
    ));
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
pub unsafe fn R_quick_method_check(_object: SEXP, _fsym: SEXP, fdef: SEXP) -> SEXP {
    unsafe {
        if fdef.is_null() || fdef == R_NilValue() {
            return R_NilValue();
        }
    }
    r_error("quick S4 method lookup is not implemented yet");
}

/// R_quick_dispatch - quick table-based dispatch for primitives.
pub unsafe fn R_quick_dispatch(_args: SEXP, _genericEnv: SEXP, fdef: SEXP) -> SEXP {
    unsafe {
        if fdef.is_null() || fdef == R_NilValue() {
            return R_NilValue();
        }
    }
    r_error("quick table-based S4 dispatch is not implemented yet");
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
        let res = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
        let _res_guard = protect(res);
        let ip = LOGICAL(res);

        if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
            // Invalid: not a symbol
            *ip.add(0) = 0; // FALSE
        } else {
            // Check if the symbol is bound to R_MissingArg in the environment
            let val = crate::sexp::envir::R_findVarInFrame(ev, symbol);
            if val == crate::sexp::globals::R_MissingArg() {
                *ip.add(0) = 1; // TRUE
            } else {
                *ip.add(0) = 0; // FALSE
            }
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
    let name = unsafe { sexp_to_string(fname) }.unwrap_or_else(|| "<unknown>".to_string());
    r_error(format!(
        "S4 method selection for '{}' requires a methods list and is not implemented yet",
        name
    ));
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
        assert!(err.message.contains("S4 method selection"));
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
