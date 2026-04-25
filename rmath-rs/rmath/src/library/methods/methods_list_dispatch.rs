#![allow(unsafe_op_in_unsafe_fn)] // legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/methods_list_dispatch.c
 *
 *  Stubs for S4 method dispatch functions.
 */

use std::cell::Cell;
use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

/// R_initMethodDispatch - initialize method dispatch.
/// Called from the methods package on load.
pub unsafe fn R_initMethodDispatch(_envir: SEXP) -> SEXP {
    R_NilValue()
}

/// R_standardGeneric - C version of the standardGeneric R function.
/// Dispatches to the appropriate method for a generic function call.
pub unsafe fn R_standardGeneric(_fname: SEXP, _ev: SEXP, _fdef: SEXP) -> SEXP {
    R_NilValue()
}

/// R_dispatchGeneric - table-based method dispatch.
pub unsafe fn R_dispatchGeneric(_fname: SEXP, _ev: SEXP, _fdef: SEXP) -> SEXP {
    R_NilValue()
}

/// R_quick_method_check - quick check if a method exists in the methods list.
pub unsafe fn R_quick_method_check(_object: SEXP, _fsym: SEXP, _fdef: SEXP) -> SEXP {
    R_NilValue()
}

/// R_quick_dispatch - quick table-based dispatch for primitives.
pub unsafe fn R_quick_dispatch(_args: SEXP, _genericEnv: SEXP, _fdef: SEXP) -> SEXP {
    R_NilValue()
}

/// R_getGeneric - get the generic function definition for a given name.
pub unsafe fn R_getGeneric(_name: SEXP, _mustFind: SEXP, _env: SEXP, _package: SEXP) -> SEXP {
    R_NilValue()
}

/// R_missingArg - check if an argument is missing in a method call.
/// Ported from R's R_missingArg() in methods_list_dispatch.c.
pub unsafe fn R_missingArg(symbol: SEXP, ev: SEXP) -> SEXP {
    let res = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
    Rf_protect(res);
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
    Rf_unprotect(1);
    res
}

/// R_selectMethod - select a method for the given call.
pub unsafe fn R_selectMethod(_fname: SEXP, _ev: SEXP, _mlist: SEXP, _evalArgs: SEXP) -> SEXP {
    R_NilValue()
}

/// R_M_setPrimitiveMethods - set methods for a primitive function.
pub unsafe fn R_M_setPrimitiveMethods(
    _fname: SEXP,
    _op: SEXP,
    _code_vec: SEXP,
    _fundef: SEXP,
    _mlist: SEXP,
) -> SEXP {
    R_NilValue()
}

/// R_nextMethodCall - implement .nextMethod() (callNextMethod).
pub unsafe fn R_nextMethodCall(_matched_call: SEXP, _ev: SEXP) -> SEXP {
    R_NilValue()
}

/// R_clear_method_selection - clear the method selection cache.
/// Ported from R's R_clear_method_selection() in methods_list_dispatch.c.
thread_local! { static N_OV: Cell<c_int> = Cell::new(0); }

pub extern "C" fn R_clear_method_selection() -> SEXP {
    N_OV.with(|v| v.set(0));
    unsafe { R_NilValue() }
}

thread_local! { static TABLE_DISPATCH_ON: Cell<c_int> = Cell::new(0); }

pub extern "C" fn R_set_method_dispatch(onOff: SEXP) -> SEXP {
    let prev = TABLE_DISPATCH_ON.with(|v| v.get());
    let value = if unsafe { TYPEOF(onOff) } == SEXPTYPE::LGLSXP && unsafe { LENGTH(onOff) } >= 1 {
        unsafe { LOGICAL_ELT(onOff, 0) }
    } else {
        0 // NA_LOGICAL treated as "return previous"
    };
    if value != std::os::raw::c_int::MIN {
        // NA_LOGICAL == INT_MIN
        TABLE_DISPATCH_ON.with(|v| v.set(if value != 0 { 1 } else { 0 }));
    }
    unsafe { Rf_ScalarLogical(prev) }
}

/// R_methodsPackageMetaName - construct a meta-data object name.
/// Format: .__prefix__name or .__prefix__name:pkg
/// Ported from R's R_methodsPackageMetaName() in methods_list_dispatch.c.
pub unsafe fn R_methodsPackageMetaName(prefix: SEXP, name: SEXP, pkg: SEXP) -> SEXP {
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

/// R_identC - test if two single-string objects are identical at the C level.
/// Ported from R's R_identC() in methods_list_dispatch.c.
pub unsafe fn R_identC(e1: SEXP, e2: SEXP) -> SEXP {
    if TYPEOF(e1) == SEXPTYPE::STRSXP
        && TYPEOF(e2) == SEXPTYPE::STRSXP
        && LENGTH(e1) == 1
        && LENGTH(e2) == 1
    {
        let s1 = STRING_ELT(e1, 0);
        let s2 = STRING_ELT(e2, 0);
        if s1 == s2 {
            return Rf_ScalarLogical(1); // TRUE
        }
    }
    Rf_ScalarLogical(0) // FALSE
}

/// R_getClassFromCache - look up a class definition in the class cache table.
pub unsafe fn R_getClassFromCache(_class: SEXP, _table: SEXP) -> SEXP {
    R_NilValue()
}

/// asChar - local helper to coerce to a single CHARSXP.
unsafe fn asChar(x: SEXP) -> SEXP {
    if TYPEOF(x) == SEXPTYPE::STRSXP && LENGTH(x) >= 1 {
        STRING_ELT(x, 0)
    } else if TYPEOF(x) == SEXPTYPE::SYMSXP {
        PRINTNAME(x)
    } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
        x
    } else {
        R_NilValue()
    }
}

/// R_el_named - get a named element from a list (no partial matching).
/// Ported from R's R_el_named() in methods_list_dispatch.c.
pub unsafe fn R_el_named(object: SEXP, what: SEXP) -> SEXP {
    let w = asChar(what);
    if w.is_null() {
        return R_NilValue();
    }
    let str = CHAR(w);
    if str.is_null() {
        return R_NilValue();
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
    R_NilValue()
}

/// R_set_el_named - set a named element in a list.
/// Ported from R's R_set_el_named() in methods_list_dispatch.c.
pub unsafe fn R_set_el_named(object: SEXP, what: SEXP, value: SEXP) -> SEXP {
    let w = asChar(what);
    if w.is_null() {
        return R_NilValue();
    }
    let str = CHAR(w);
    if str.is_null() {
        return R_NilValue();
    }
    let sym = crate::sexp::symbol::Rf_install(str);
    // Delegate to R_subassign3_dflt (the $<- default method)
    crate::main::subassign::R_subassign3_dflt(R_NilValue(), object, sym, value)
}
