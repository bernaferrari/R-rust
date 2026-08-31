//! R version introspection — version, R.version.string, args, formals, body,
//! environmentOf.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// R runtime essentials
// ---------------------------------------------------------------------------

unsafe fn make_r_version_list(simple_list_class: bool) -> SEXP {
    unsafe {
        let fields = [
            ("platform", "rust-port"),
            ("arch", std::env::consts::ARCH),
            ("os", std::env::consts::OS),
            ("system", "rust-port"),
            ("status", ""),
            ("major", "4"),
            ("minor", "4.1"),
            ("year", "2026"),
            ("month", "05"),
            ("day", "09"),
            ("svn rev", ""),
            ("language", "R"),
            ("version.string", "R version 4.4.1 (Rust Port)"),
            ("nickname", "Rust Port"),
        ];

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, fields.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (i, (_, value)) in fields.iter().enumerate() {
            let value = CString::new(*value).unwrap_or_default();
            SET_VECTOR_ELT(result, i as R_xlen_t, Rf_mkString(value.as_ptr()));
        }

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, fields.len() as R_xlen_t);
        if !names.is_null() {
            let _names_guard = protect(names);
            for (i, (name, _)) in fields.iter().enumerate() {
                let name = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(name.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }

        if simple_list_class {
            let class = Rf_mkString(c"simple.list".as_ptr());
            let _class_guard = protect(class);
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }

        result
    }
}

/// R's `version` — legacy constant alias for `R.version`.
pub unsafe fn do_version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { make_r_version_list(true) }
}

/// R's `R.version` — returns a named list with version info.
pub unsafe fn do_R_version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { make_r_version_list(true) }
}

/// R's `R.Version()` — returns the version info list without `simple.list` class.
pub unsafe fn do_R_Version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { make_r_version_list(false) }
}

/// R's `args(fn)` — returns the formal arguments of a function as a pairlist.
/// With the body set to NULL.
pub unsafe fn do_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            return crate::mainutils::dstruct::mkCLOSXP(
                FORMALS(fn_arg),
                R_NilValue(),
                crate::sexp::globals::R_GlobalEnv(),
            );
        }

        if t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
            return R_NilValue();
        }

        let primitive_name = crate::eval::primitive::PRIMNAME(fn_arg);
        let primitive_symbol =
            Rf_install(CString::new(primitive_name).unwrap_or_default().as_ptr());

        for registry in [".ArgsEnv", ".GenericArgsEnv"] {
            let registry_symbol = Rf_install(CString::new(registry).unwrap_or_default().as_ptr());
            let registry_env = crate::sexp::envir::R_findVarInFrame(
                crate::sexp::globals::R_BaseEnv(),
                registry_symbol,
            );
            if registry_env == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            let prototype = crate::sexp::envir::R_findVarInFrame(registry_env, primitive_symbol);
            if prototype != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(prototype) == SEXPTYPE::CLOSXP
            {
                return crate::mainutils::dstruct::mkCLOSXP(
                    FORMALS(prototype),
                    R_NilValue(),
                    crate::sexp::globals::R_GlobalEnv(),
                );
            }
        }

        R_NilValue()
    }
}

/// R's `formals(fn)` — get the formal arguments (parameter list) of a function.
pub unsafe fn do_formals(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            let formals = crate::sexp::accessors::FORMALS(fn_arg);
            if formals.is_null() {
                R_NilValue()
            } else {
                formals
            }
        } else {
            R_NilValue()
        }
    }
}

/// R's `body(fn)` — get the body of a function.
pub unsafe fn do_body(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            let body = crate::sexp::accessors::BODY(fn_arg);
            if body.is_null() {
                R_NilValue()
            } else if TYPEOF(body) == SEXPTYPE::BCODESXP {
                let source = crate::eval::bc_eval::BCODE_EXPR(body);
                if source.is_null() || source == R_NilValue() {
                    body
                } else {
                    source
                }
            } else {
                body
            }
        } else {
            R_NilValue()
        }
    }
}

/// R's `environment(fn)` — get the environment of a closure.
/// Same as do_environment, provided as an alternative name.
pub unsafe fn do_environment_of(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_environment(_call, _op, args, _rho) }
}
