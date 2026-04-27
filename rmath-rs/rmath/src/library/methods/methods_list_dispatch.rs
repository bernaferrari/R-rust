/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/methods_list_dispatch.c
 *
 *  Stubs for S4 method dispatch functions.
 */

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
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

/// R_initMethodDispatch - initialize method dispatch.
/// Called from the methods package on load.
pub unsafe fn R_initMethodDispatch(_envir: SEXP) -> SEXP {
    nil_value()
}

/// R_standardGeneric - C version of the standardGeneric R function.
/// Dispatches to the appropriate method for a generic function call.
pub unsafe fn R_standardGeneric(_fname: SEXP, _ev: SEXP, _fdef: SEXP) -> SEXP {
    nil_value()
}

/// R_dispatchGeneric - table-based method dispatch.
pub unsafe fn R_dispatchGeneric(_fname: SEXP, _ev: SEXP, _fdef: SEXP) -> SEXP {
    nil_value()
}

/// R_quick_method_check - quick check if a method exists in the methods list.
pub unsafe fn R_quick_method_check(_object: SEXP, _fsym: SEXP, _fdef: SEXP) -> SEXP {
    nil_value()
}

/// R_quick_dispatch - quick table-based dispatch for primitives.
pub unsafe fn R_quick_dispatch(_args: SEXP, _genericEnv: SEXP, _fdef: SEXP) -> SEXP {
    nil_value()
}

/// R_getGeneric - get the generic function definition for a given name.
pub unsafe fn R_getGeneric(_name: SEXP, _mustFind: SEXP, _env: SEXP, _package: SEXP) -> SEXP {
    nil_value()
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
pub unsafe fn R_selectMethod(_fname: SEXP, _ev: SEXP, _mlist: SEXP, _evalArgs: SEXP) -> SEXP {
    nil_value()
}

/// R_M_setPrimitiveMethods - set methods for a primitive function.
pub unsafe fn R_M_setPrimitiveMethods(
    _fname: SEXP,
    _op: SEXP,
    _code_vec: SEXP,
    _fundef: SEXP,
    _mlist: SEXP,
) -> SEXP {
    nil_value()
}

/// R_nextMethodCall - implement .nextMethod() (callNextMethod).
pub unsafe fn R_nextMethodCall(_matched_call: SEXP, _ev: SEXP) -> SEXP {
    nil_value()
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
pub unsafe fn R_getClassFromCache(_class: SEXP, _table: SEXP) -> SEXP {
    nil_value()
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
