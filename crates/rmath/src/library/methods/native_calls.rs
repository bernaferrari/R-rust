//! Bind ported methods C routines so GNU `.Call(C_R_*, ...)` resolves.
//!
//! `useDynLib(methods, .registration = TRUE, .fixes = "C_")` is skipped on this
//! runtime. The methods R sources still evaluate those `C_*` symbols, so the
//! namespace must expose them and `.Call` must map the names onto the Rust
//! ports.

use std::ffi::CString;
use std::os::raw::c_char;

use crate::sexp::constructors::Rf_mkString;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::SEXP;
use crate::sexp::symbol::Rf_install;
use crate::unix::dynload::DL_FUNC;

const METHODS_CALL_NAMES: &[&str] = &[
    "C_R_M_setPrimitiveMethods",
    "C_R_clear_method_selection",
    "C_R_dummy_extern_place",
    "C_R_el_named",
    "C_R_externalptr_prototype_object",
    "C_R_getClassFromCache",
    "C_R_getGeneric",
    "C_R_get_slot",
    "C_R_hasSlot",
    "C_R_identC",
    "C_R_initMethodDispatch",
    "C_R_methodsPackageMetaName",
    "C_R_missingArg",
    "C_R_nextMethodCall",
    "C_R_quick_method_check",
    "C_R_selectMethod",
    "C_R_set_el_named",
    "C_R_set_method_dispatch",
    "C_R_set_slot",
    "C_R_standardGeneric",
    "C_Rf_allocS4Object",
    "C_do_substitute_direct",
    "C_R_get_primname",
    "C_new_object",
    "R_M_setPrimitiveMethods",
    "R_dummy_extern_place",
    "R_el_named",
    "R_externalptr_prototype_object",
    "R_getClassFromCache",
    "R_getGeneric",
    "R_get_slot",
    "R_hasSlot",
    "R_identC",
    "R_initMethodDispatch",
    "R_methodsPackageMetaName",
    "R_missingArg",
    "R_nextMethodCall",
    "R_quick_method_check",
    "R_selectMethod",
    "R_set_el_named",
    "R_set_method_dispatch",
    "R_set_slot",
    "R_standardGeneric",
    "Rf_allocS4Object",
    "do_substitute_direct",
    "R_get_primname",
    "new_object",
];


unsafe extern "C" fn c_r_get_generic(name: SEXP, must: SEXP, env: SEXP, pkg: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_getGeneric(name, must, env, pkg) }
}

unsafe extern "C" fn c_r_ident_c(e1: SEXP, e2: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_identC(e1, e2) }
}

unsafe extern "C" fn c_r_methods_package_meta_name(prefix: SEXP, name: SEXP, pkg: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_methodsPackageMetaName(prefix, name, pkg) }
}

unsafe extern "C" fn c_r_el_named(object: SEXP, what: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_el_named(object, what) }
}

unsafe extern "C" fn c_r_set_el_named(object: SEXP, what: SEXP, value: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_set_el_named(object, what, value) }
}

unsafe extern "C" fn c_r_missing_arg(symbol: SEXP, ev: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_missingArg(symbol, ev) }
}

unsafe extern "C" fn c_r_get_slot(obj: SEXP, name: SEXP) -> SEXP {
    unsafe { super::slot::R_get_slot(obj, name) }
}

unsafe extern "C" fn c_r_set_slot(obj: SEXP, name: SEXP, value: SEXP) -> SEXP {
    unsafe { super::slot::R_set_slot(obj, name, value) }
}

unsafe extern "C" fn c_r_has_slot(obj: SEXP, name: SEXP) -> SEXP {
    unsafe { super::slot::R_hasSlot(obj, name) }
}

unsafe extern "C" fn c_r_init_method_dispatch(envir: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_initMethodDispatch(envir) }
}

unsafe extern "C" fn c_r_standard_generic(fname: SEXP, ev: SEXP, fdef: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_standardGeneric(fname, ev, fdef) }
}

unsafe extern "C" fn c_r_select_method(fname: SEXP, ev: SEXP, mlist: SEXP, eval_args: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_selectMethod(fname, ev, mlist, eval_args) }
}

unsafe extern "C" fn c_r_get_class_from_cache(class: SEXP, table: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_getClassFromCache(class, table) }
}

unsafe extern "C" fn c_r_quick_method_check(args: SEXP, mlist: SEXP, fdef: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_quick_method_check(args, mlist, fdef) }
}

unsafe extern "C" fn c_r_next_method_call(matched_call: SEXP, ev: SEXP) -> SEXP {
    unsafe { super::methods_list_dispatch::R_nextMethodCall(matched_call, ev) }
}

unsafe extern "C" fn c_r_m_set_primitive_methods(
    fname: SEXP,
    op: SEXP,
    code_vec: SEXP,
    fundef: SEXP,
    mlist: SEXP,
) -> SEXP {
    unsafe {
        super::methods_list_dispatch::R_M_setPrimitiveMethods(fname, op, code_vec, fundef, mlist)
    }
}

unsafe extern "C" fn c_do_substitute_direct(f: SEXP, env: SEXP) -> SEXP {
    unsafe { super::do_substitute_direct::do_substitute_direct(f, env) }
}

unsafe extern "C" fn c_r_get_primname(object: SEXP) -> SEXP {
    unsafe { super::class_support::R_get_primname(object) }
}

unsafe extern "C" fn c_new_object(class_def: SEXP) -> SEXP {
    unsafe { super::class_support::new_object(class_def) }
}

unsafe extern "C" fn c_rf_alloc_s4_object() -> SEXP {
    unsafe { super::class_support::Rf_allocS4Object() }
}

unsafe extern "C" fn c_r_externalptr_prototype_object() -> SEXP {
    unsafe { super::tests::R_externalptr_prototype_object() }
}

unsafe extern "C" fn c_r_dummy_extern_place() -> SEXP {
    unsafe { super::tests::R_dummy_extern_place() }
}

unsafe extern "C" fn c_r_set_method_dispatch(on_off: SEXP) -> SEXP {
    super::methods_list_dispatch::R_set_method_dispatch(on_off)
}


fn as_dl<T>(f: T) -> DL_FUNC {
    Some(unsafe { std::mem::transmute_copy(&f) })
}

/// Resolve a methods `.Call` name (`C_R_getGeneric` or `R_getGeneric`).
pub fn lookup(name: &str) -> DL_FUNC {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "R_getGeneric" => as_dl(c_r_get_generic as unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP),
        "R_identC" => as_dl(c_r_ident_c as unsafe extern "C" fn(SEXP, SEXP) -> SEXP),
        "R_methodsPackageMetaName" => {
            as_dl(c_r_methods_package_meta_name as unsafe extern "C" fn(SEXP, SEXP, SEXP) -> SEXP)
        }
        "R_el_named" => as_dl(c_r_el_named as unsafe extern "C" fn(SEXP, SEXP) -> SEXP),
        "R_set_el_named" => as_dl(c_r_set_el_named as unsafe extern "C" fn(SEXP, SEXP, SEXP) -> SEXP),
        "R_missingArg" => as_dl(c_r_missing_arg as unsafe extern "C" fn(SEXP, SEXP) -> SEXP),
        "R_get_slot" => as_dl(c_r_get_slot as unsafe extern "C" fn(SEXP, SEXP) -> SEXP),
        "R_set_slot" => as_dl(c_r_set_slot as unsafe extern "C" fn(SEXP, SEXP, SEXP) -> SEXP),
        "R_hasSlot" => as_dl(c_r_has_slot as unsafe extern "C" fn(SEXP, SEXP) -> SEXP),
        "R_initMethodDispatch" => as_dl(c_r_init_method_dispatch as unsafe extern "C" fn(SEXP) -> SEXP),
        "R_standardGeneric" => {
            as_dl(c_r_standard_generic as unsafe extern "C" fn(SEXP, SEXP, SEXP) -> SEXP)
        }
        "R_selectMethod" => {
            as_dl(c_r_select_method as unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP)
        }
        "R_getClassFromCache" => {
            as_dl(c_r_get_class_from_cache as unsafe extern "C" fn(SEXP, SEXP) -> SEXP)
        }
        "R_quick_method_check" => {
            as_dl(c_r_quick_method_check as unsafe extern "C" fn(SEXP, SEXP, SEXP) -> SEXP)
        }
        "R_nextMethodCall" => as_dl(c_r_next_method_call as unsafe extern "C" fn(SEXP, SEXP) -> SEXP),
        "R_M_setPrimitiveMethods" => as_dl(
            c_r_m_set_primitive_methods
                as unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP,
        ),
        "do_substitute_direct" => {
            as_dl(c_do_substitute_direct as unsafe extern "C" fn(SEXP, SEXP) -> SEXP)
        }
        "R_get_primname" => as_dl(c_r_get_primname as unsafe extern "C" fn(SEXP) -> SEXP),
        "new_object" => as_dl(c_new_object as unsafe extern "C" fn(SEXP) -> SEXP),
        "Rf_allocS4Object" => as_dl(c_rf_alloc_s4_object as unsafe extern "C" fn() -> SEXP),
        "R_externalptr_prototype_object" => {
            as_dl(c_r_externalptr_prototype_object as unsafe extern "C" fn() -> SEXP)
        }
        "R_dummy_extern_place" => as_dl(c_r_dummy_extern_place as unsafe extern "C" fn() -> SEXP),
        "R_set_method_dispatch" => {
            as_dl(c_r_set_method_dispatch as unsafe extern "C" fn(SEXP) -> SEXP)
        }
        _ => None,

    }
}

/// Install `C_*` string bindings so methods R sources can evaluate them.
pub unsafe fn install_methods_call_symbols(env: SEXP) {
    unsafe {
        for name in METHODS_CALL_NAMES {
            let cname = CString::new(*name).unwrap_or_default();
            defineVar(
                Rf_install(cname.as_ptr()),
                Rf_mkString(cname.as_ptr() as *const c_char),
                env,
            );
        }
    }
}
