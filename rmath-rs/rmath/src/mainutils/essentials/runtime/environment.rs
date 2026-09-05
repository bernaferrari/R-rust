//! Environment builtins — emptyenv/baseenv/globalenv/new.env/environment,
//! parent.env, environmentName, envName, isEmpty.

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
// Environment functions
// ---------------------------------------------------------------------------

/// R's `emptyenv()` — returns the empty environment (root of environment chain).
pub unsafe fn do_emptyenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_EmptyEnv() }
}

/// R's `baseenv()` — returns the base environment.
pub unsafe fn do_baseenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_BaseEnv() }
}

/// R's `globalenv()` — returns the global environment.
pub unsafe fn do_globalenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_GlobalEnv() }
}

/// R's `new.env(hash, parent, size)` — create a new environment.
///
/// Upstream formals are `function (hash = TRUE, parent = parent.frame(),
/// size = 29L)`: the missing `parent` default evaluates to the caller's
/// frame (`parent.frame()`), which for a builtin is the evaluation
/// environment `rho`. Sourcing package files with `envir = <namespace>`
/// therefore nests `new.env()` children under the namespace, matching
/// GNU R (aaa.R's `capsule` must enclose under namespace:R6).
pub unsafe fn do_new_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let parent_arg = arg_by_name_or_position(args, &["parent"], 1);
        let parent = if parent_arg.is_null() || parent_arg == R_NilValue() {
            // parent = parent.frame() — the caller's evaluation frame
            if _rho.is_null() || _rho == R_NilValue() {
                crate::sexp::globals::R_GlobalEnv()
            } else {
                _rho
            }
        } else if TYPEOF(parent_arg) == SEXPTYPE::ENVSXP {
            parent_arg
        } else {
            crate::sexp::globals::R_GlobalEnv()
        };

        // Create a new environment with empty frame and parent
        let env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(), // empty frame
            parent,       // enclosing env
            R_NilValue(), // no hash table (simplified)
        );
        env
    }
}

/// R's `environment(fun)` — get the environment associated with a closure.
/// With no argument, returns the current evaluation environment (callers
/// rely on `environment()` inside `local({...})` blocks to capture the
/// fresh child env).
pub unsafe fn do_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return if _rho.is_null() { R_NilValue() } else { _rho };
        }
        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            let env = crate::sexp::accessors::CLOENV(fn_arg);
            if env.is_null() { R_NilValue() } else { env }
        } else if t == SEXPTYPE::ENVSXP {
            fn_arg
        } else {
            R_NilValue()
        }
    }
}

// Environment binding and locking builtins live in the `environment_bindings` submodule.

// ---------------------------------------------------------------------------
// Environment completion
// ---------------------------------------------------------------------------

/// R's `parent.env(env)` — returns the parent environment.
pub unsafe fn do_parent_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        if env.is_null() || env == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(env);
        if t != SEXPTYPE::ENVSXP {
            return R_NilValue();
        }
        // enclos is the enclosing/parent environment
        let parent = (*env).data.envsxp.enclos;
        if parent.is_null() {
            return crate::sexp::globals::R_EmptyEnv();
        }
        parent
    }
}

/// R's `set_parent.env(env, parent)` — set the parent environment.
pub unsafe fn do_set_parent_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        let parent = CAR(CDR(args));
        if env.is_null() || env == R_NilValue() || TYPEOF(env) != SEXPTYPE::ENVSXP {
            return R_NilValue();
        }
        if parent.is_null() || parent == R_NilValue() || TYPEOF(parent) != SEXPTYPE::ENVSXP {
            return env;
        }
        SET_ENCLOS(env, parent);
        env
    }
}

/// R's `env_name(env)` — returns the name of an environment.
pub unsafe fn do_env_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        if env.is_null() || env == R_NilValue() {
            return Rf_mkString(c"NULL".as_ptr());
        }
        let t = TYPEOF(env);
        if t != SEXPTYPE::ENVSXP {
            return Rf_mkString(c"".as_ptr());
        }
        // Check if it's a special environment
        if env == crate::sexp::globals::R_GlobalEnv() {
            return Rf_mkString(c"R_GlobalEnv".as_ptr());
        }
        if env == crate::sexp::globals::R_EmptyEnv() {
            return Rf_mkString(c"R_EmptyEnv".as_ptr());
        }
        if env == crate::sexp::globals::R_BaseEnv() {
            return Rf_mkString(c"base".as_ptr());
        }
        let name = crate::sexp::attrib_core::getAttrib(env, Rf_install(c"name".as_ptr()));
        if TYPEOF(name) == SEXPTYPE::STRSXP && XLENGTH(name) > 0 {
            let value = STRING_ELT(name, 0);
            if !value.is_null() && value != R_NilValue() {
                return Rf_mkString(CHAR(value));
            }
        }
        Rf_mkString(c"".as_ptr())
    }
}

/// R's `environmentName(env)` — returns the name of an environment.
pub unsafe fn do_environment_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_env_name(_call, _op, args, _rho) }
}

/// R-like `is_empty(env)` — check if environment is empty (simplified).
pub unsafe fn do_is_empty(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        if env.is_null() || env == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        let t = TYPEOF(env);
        if t == SEXPTYPE::ENVSXP {
            // Check frame - if it's NULL/NILSXP, env is empty
            let frame = (*env).data.envsxp.frame;
            if frame.is_null() || frame == R_NilValue() {
                return Rf_ScalarLogical(TRUE);
            }
            return Rf_ScalarLogical(FALSE);
        }
        // For vectors, check length
        let n = XLENGTH(env);
        Rf_ScalarLogical(if n == 0 { TRUE } else { FALSE })
    }
}
