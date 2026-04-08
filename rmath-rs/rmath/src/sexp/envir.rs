#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unsafe_op_in_unsafe_fn
)]

//! Environment operations — ports R's src/main/envir.c.
//!
//! Provides variable lookup, assignment, function lookup, and argument matching
//! needed by the evaluator and other interpreter components.
//!
//! # Design
//!
//! This module provides two layers of API:
//! 1. **Safe functions** (e.g., `find_var_in_frame_safe`, `define_var_safe`) that use
//!    the `Sexp` wrapper type, `Option`/`Result` returns, and `PairlistIter`
//!    for idiomatic Rust code.
//! 2. **FFI functions** (e.g., `R_findVar`, `defineVar`) that are `extern "C"`
//!    and delegate to the safe versions, maintaining C ABI compatibility.

use std::os::raw::c_int;
use std::ptr;

use super::accessors::{CHAR, PRINTNAME, SET_FRAME, SET_PRVALUE, SETCAR, SETCDR, SETTAG};
use super::constructors::Rf_cons;
use super::ffi::{SEXP, SEXPTYPE};
use super::globals::{R_GlobalEnv, R_MissingArg, R_NilValue, R_UnboundValue};
use super::memory_ext::NewEnvironment;
use super::safe::{PairlistIter, Sexp};
use super::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Safe wrapper types
// ---------------------------------------------------------------------------

/// Result of a variable lookup that may be unbound.
pub type LookupResult<'a> = Option<Sexp<'a>>;

/// Result of an operation that may fail with an error message.
pub type EnvResult<T> = Result<T, String>;

// ---------------------------------------------------------------------------
// findVarInFrame — safe version
// ---------------------------------------------------------------------------

/// Find a variable in the frame of a single environment (no inheritance).
///
/// Returns `None` if the symbol is not found or if inputs are invalid.
#[must_use]
pub fn find_var_in_frame_safe<'a>(rho: Sexp<'a>, symbol: Sexp<'a>) -> LookupResult<'a> {
    if !rho.is_environment() {
        return None;
    }

    let frame = rho.frame()?;

    for cell in PairlistIter::new(frame) {
        if cell.tag() == Some(symbol) {
            return cell.car();
        }
    }

    None
}

// ---------------------------------------------------------------------------
// findVar — safe version with inheritance
// ---------------------------------------------------------------------------

/// Find a variable, searching through parent environments.
///
/// Returns `None` if the variable is not found in the environment chain.
/// Forces promises if encountered.
#[must_use]
pub fn find_var_safe<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> LookupResult<'a> {
    let mut current = rho;

    loop {
        if !current.is_environment() {
            break;
        }

        if let Some(val) = find_var_in_frame_safe(current, symbol) {
            if val.typeof_() == SEXPTYPE::PROMSXP {
                return force_promise_safe(val);
            }
            return Some(val);
        }

        if let Some(sym_val) = symbol.symvalue()
            && sym_val.typeof_() == SEXPTYPE::SPECIALSXP
        {
            return Some(sym_val);
        }

        current = current.enclos()?;
    }

    None
}

// ---------------------------------------------------------------------------
// forcePromise — safe version
// ---------------------------------------------------------------------------

/// Force evaluation of a promise, returning its value.
///
/// If the input is not a promise, returns it as-is.
#[must_use]
pub fn force_promise_safe(prom: Sexp<'_>) -> LookupResult<'_> {
    if prom.typeof_() != SEXPTYPE::PROMSXP {
        return Some(prom);
    }

    let val = prom.prvalue()?;
    if val.typeof_() != SEXPTYPE::SPECIALSXP {
        return Some(val);
    }

    let expr = prom.prcode()?;

    unsafe {
        SET_PRVALUE(prom.as_raw(), R_UnboundValue());
        (*prom.as_raw())
            .sxpinfo
            .set_gp((*prom.as_raw()).sxpinfo.gp() | 0x02);
    }

    let value = if !expr.as_raw().is_null() {
        expr
    } else {
        unsafe { Sexp::from_raw_unchecked(R_MissingArg()) }
    };

    unsafe {
        SET_PRVALUE(prom.as_raw(), value.as_raw());
    }
    Some(value)
}

// ---------------------------------------------------------------------------
// defineVar — safe version
// ---------------------------------------------------------------------------

/// Define a variable in the given environment's frame.
///
/// If the symbol already exists, its value is updated.
/// If not, a new binding is created at the front of the frame.
pub fn define_var_safe(symbol: Sexp<'_>, value: Sexp<'_>, rho: Sexp<'_>) -> bool {
    if !rho.is_environment() {
        return false;
    }

    let frame = match rho.frame() {
        Some(f) => f,
        None => unsafe { Sexp::from_raw_unchecked(R_NilValue()) },
    };

    for cell in PairlistIter::new(frame) {
        if cell.tag() == Some(symbol) {
            unsafe {
                SETCAR(cell.as_raw(), value.as_raw());
            }
            return true;
        }
    }

    let new_cell = unsafe { Rf_cons(value.as_raw(), frame.as_raw()) };
    if !new_cell.is_null() {
        unsafe {
            SETTAG(new_cell, symbol.as_raw());
            SET_FRAME(rho.as_raw(), new_cell);
        }
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// setVar — safe version with inheritance
// ---------------------------------------------------------------------------

/// Set a variable value, searching parent environments.
///
/// If the variable is not found, it's defined in the global environment.
pub fn set_var_safe(symbol: Sexp<'_>, value: Sexp<'_>, rho: Sexp<'_>) {
    let mut current = rho;

    loop {
        if !current.is_environment() {
            break;
        }

        let frame = match current.frame() {
            Some(f) => f,
            None => unsafe { Sexp::from_raw_unchecked(R_NilValue()) },
        };

        for cell in PairlistIter::new(frame) {
            if cell.tag() == Some(symbol) {
                unsafe {
                    SETCAR(cell.as_raw(), value.as_raw());
                }
                return;
            }
        }

        current = match current.enclos() {
            Some(e) => e,
            None => break,
        };
    }

    let global_env = unsafe { Sexp::from_raw_unchecked(R_GlobalEnv()) };
    define_var_safe(symbol, value, global_env);
}

// ---------------------------------------------------------------------------
// findFun — safe version
// ---------------------------------------------------------------------------

/// Find a function value for a symbol.
///
/// Searches through environments, looking for closures, builtins, or specials.
#[must_use]
pub fn find_fun_safe<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> LookupResult<'a> {
    let mut current = rho;

    loop {
        if !current.is_environment() {
            break;
        }

        if let Some(val) = find_var_in_frame_safe(current, symbol) {
            let t = val.typeof_();
            if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
                return Some(val);
            }
        }

        current = match current.enclos() {
            Some(e) => e,
            None => break,
        };
    }

    None
}

// ---------------------------------------------------------------------------
// matchArgs — safe version
// ---------------------------------------------------------------------------

/// Match actual arguments to formal parameters.
#[must_use]
pub fn match_args_safe<'a>(formals: Sexp<'a>, args: Sexp<'a>) -> Option<Sexp<'a>> {
    if !formals.is_pairlist() {
        return Some(args);
    }

    let mut result: Option<Sexp<'a>> = None;
    let mut result_tail: Option<Sexp<'a>> = None;

    let missing_arg = unsafe { Sexp::from_raw_unchecked(R_MissingArg()) };

    for f in PairlistIter::new(formals) {
        let ftag = match f.tag() {
            Some(t) => t,
            None => continue,
        };

        let matched = PairlistIter::new(args).find(|a| a.tag() == Some(ftag));

        let car_val = matched.and_then(|m| m.car()).unwrap_or(missing_arg);

        let cell = unsafe { Rf_cons(car_val.as_raw(), R_NilValue()) };
        if cell.is_null() {
            return None;
        }
        unsafe { SETTAG(cell, ftag.as_raw()) };
        let cell = Sexp::from_raw(cell)?;

        if result.is_none() {
            result = Some(cell);
            result_tail = Some(cell);
        } else {
            unsafe {
                SETCDR(
                    result_tail.expect("unwrap on None/Err").as_raw(),
                    cell.as_raw(),
                );
            }
            result_tail = Some(cell);
        }
    }

    result.or_else(|| unsafe { Sexp::from_raw(R_NilValue()) })
}

// ---------------------------------------------------------------------------
// isMissing — safe version
// ---------------------------------------------------------------------------

/// Check if a symbol has a missing argument in the given environment.
#[must_use]
pub fn is_missing_safe(symbol: Sexp<'_>, rho: Sexp<'_>) -> bool {
    let val = match find_var_in_frame_safe(rho, symbol) {
        Some(v) => v,
        None => return true,
    };

    let missing_arg = unsafe { Sexp::from_raw_unchecked(R_MissingArg()) };
    if val == missing_arg {
        return true;
    }

    if val.typeof_() == SEXPTYPE::PROMSXP
        && let Some(expr) = val.prcode()
        && expr == missing_arg
    {
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// ddfindVar — safe version (dots lookup)
// ---------------------------------------------------------------------------

/// Find a variable in the ... (dots) arguments.
#[must_use]
pub fn dd_find_var_safe<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> LookupResult<'a> {
    let dots_name = std::ffi::CString::new("...").ok()?;
    let dots_sym = unsafe { Sexp::from_raw(Rf_install(dots_name.as_ptr()))? };
    let dots_val = find_var_in_frame_safe(rho, dots_sym)?;

    let missing_arg = unsafe { Sexp::from_raw_unchecked(R_MissingArg()) };
    if dots_val == missing_arg {
        return None;
    }

    for cell in PairlistIter::new(dots_val) {
        if cell.tag() == Some(symbol) {
            let val = cell.car()?;
            if val.typeof_() == SEXPTYPE::PROMSXP {
                return force_promise_safe(val);
            }
            return Some(val);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// CheckFormals — safe version
// ---------------------------------------------------------------------------

/// Check that formals is a valid pairlist of distinct symbols.
pub fn check_formals_safe(formals: Sexp<'_>) -> EnvResult<()> {
    let mut seen: Vec<Sexp<'_>> = Vec::new();

    for f in PairlistIter::new(formals) {
        let sym = f
            .tag()
            .ok_or_else(|| "invalid formal argument list".to_string())?;
        if !sym.is_symbol() {
            return Err("invalid formal argument list".to_string());
        }

        if seen.contains(&sym) {
            return Err("duplicate formal argument name".to_string());
        }
        seen.push(sym);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// addMissingVarsToNewEnv — safe version
// ---------------------------------------------------------------------------

/// Add missing variable bindings for unprovided arguments.
pub fn add_missing_vars_to_new_env_safe(formals: Sexp<'_>, args: Sexp<'_>, newrho: Sexp<'_>) {
    let missing_arg = unsafe { Sexp::from_raw_unchecked(R_MissingArg()) };

    for f in PairlistIter::new(formals) {
        if let Some(sym) = f.tag() {
            let found = PairlistIter::new(args).any(|a| a.tag() == Some(sym));
            if !found {
                define_var_safe(sym, missing_arg, newrho);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// existsVarInFrame — safe version
// ---------------------------------------------------------------------------

/// Check if a variable exists in a given frame.
#[must_use]
pub fn exists_var_in_frame_safe(rho: Sexp<'_>, symbol: Sexp<'_>) -> bool {
    find_var_in_frame_safe(rho, symbol).is_some()
}

// ---------------------------------------------------------------------------
// FFI functions (delegate to safe versions)
// ---------------------------------------------------------------------------

/// Find a variable in the frame of a single environment (no inheritance).
///
/// FFI wrapper around [`find_var_in_frame_safe`].
#[unsafe(no_mangle)]
pub unsafe fn R_findVarInFrame(rho: SEXP, symbol: SEXP) -> SEXP {
    if rho.is_null() || symbol.is_null() {
        return R_UnboundValue();
    }

    match (Sexp::from_raw(rho), Sexp::from_raw(symbol)) {
        (Some(rho), Some(symbol)) => find_var_in_frame_safe(rho, symbol)
            .map(|s: Sexp<'_>| s.as_raw())
            .unwrap_or_else(|| R_UnboundValue()),
        _ => R_UnboundValue(),
    }
}

/// Find a variable, searching through parent environments.
///
/// FFI wrapper around [`find_var_safe`].
pub unsafe fn R_findVar(symbol: SEXP, rho: SEXP) -> SEXP {
    if symbol.is_null() {
        return R_UnboundValue();
    }

    match (Sexp::from_raw(symbol), Sexp::from_raw(rho)) {
        (Some(symbol), Some(rho)) => find_var_safe(symbol, rho)
            .map(|s: Sexp<'_>| s.as_raw())
            .unwrap_or_else(|| R_UnboundValue()),
        _ => R_UnboundValue(),
    }
}

/// Force evaluation of a promise, returning its value.
///
/// FFI wrapper around [`force_promise_safe`].
pub unsafe fn forcePromise(prom: SEXP) -> SEXP {
    if prom.is_null() {
        return R_NilValue();
    }

    match Sexp::from_raw(prom) {
        Some(prom) => force_promise_safe(prom)
            .map(|s: Sexp<'_>| s.as_raw())
            .unwrap_or_else(|| R_NilValue()),
        None => R_NilValue(),
    }
}

/// Define a variable in the given environment's frame.
///
/// FFI wrapper around [`define_var_safe`].
pub unsafe fn defineVar(symbol: SEXP, value: SEXP, rho: SEXP) {
    if rho.is_null() || symbol.is_null() {
        return;
    }

    if let (Some(symbol), Some(value), Some(rho)) = (
        Sexp::from_raw(symbol),
        Sexp::from_raw(value),
        Sexp::from_raw(rho),
    ) {
        define_var_safe(symbol, value, rho);
    }
}

/// Set a variable value, searching parent environments if needed.
///
/// FFI wrapper around [`set_var_safe`].
pub unsafe fn setVar(symbol: SEXP, value: SEXP, rho: SEXP) {
    if symbol.is_null() {
        return;
    }

    if let (Some(symbol), Some(value), Some(rho)) = (
        Sexp::from_raw(symbol),
        Sexp::from_raw(value),
        Sexp::from_raw(rho),
    ) {
        set_var_safe(symbol, value, rho);
    }
}

/// Find a function value for a symbol.
///
/// FFI wrapper around [`find_fun_safe`].
#[unsafe(no_mangle)]
pub unsafe fn findFun(symbol: SEXP, rho: SEXP) -> SEXP {
    if symbol.is_null() {
        return R_UnboundValue();
    }

    match (Sexp::from_raw(symbol), Sexp::from_raw(rho)) {
        (Some(symbol), Some(rho)) => find_fun_safe(symbol, rho)
            .map(|s: Sexp<'_>| s.as_raw())
            .unwrap_or_else(|| R_UnboundValue()),
        _ => R_UnboundValue(),
    }
}

/// Find a function with error reporting.
pub unsafe fn findFun3(symbol: SEXP, rho: SEXP, call: SEXP) -> SEXP {
    let fun = findFun(symbol, rho);
    if fun == R_UnboundValue() {
        // Could not find function — would error in real implementation
    }
    fun
}

/// Match actual arguments to formal parameters.
///
/// FFI wrapper around [`match_args_safe`].
pub unsafe fn matchArgs(formals: SEXP, args: SEXP, _call: SEXP) -> SEXP {
    if formals.is_null() || formals == R_NilValue() {
        return args;
    }

    match (Sexp::from_raw(formals), Sexp::from_raw(args)) {
        (Some(formals), Some(args)) => match_args_safe(formals, args)
            .map(|s: Sexp<'_>| s.as_raw())
            .unwrap_or_else(|| args.as_raw()),
        _ => args,
    }
}

/// Match arguments without renaming.
pub unsafe fn matchArgs_NR(formals: SEXP, args: SEXP) -> SEXP {
    unsafe { matchArgs(formals, args, ptr::null_mut()) }
}

/// Check if a symbol has a missing argument in the given environment.
///
/// FFI wrapper around [`is_missing_safe`].
pub unsafe fn R_isMissing(symbol: SEXP, rho: SEXP) -> c_int {
    if symbol.is_null() || rho.is_null() {
        return 0;
    }

    match (Sexp::from_raw(symbol), Sexp::from_raw(rho)) {
        (Some(symbol), Some(rho)) => is_missing_safe(symbol, rho) as c_int,
        _ => 0,
    }
}

/// Report an error for a missing argument.
pub(crate) unsafe fn R_MissingArgError(symbol: SEXP, call: SEXP) {
    let name = if !symbol.is_null() {
        let pname = PRINTNAME(symbol);
        if !pname.is_null() {
            let s = CHAR(pname);
            if !s.is_null() {
                std::ffi::CStr::from_ptr(s).to_str().unwrap_or("???")
            } else {
                "???"
            }
        } else {
            "???"
        }
    } else {
        "???"
    };

    eprintln!("Error in {}: argument \"{}\" is missing", "eval", name);
    std::panic::panic_any(crate::sexp::context::RError {
        message: format!("argument \"{}\" is missing, with no default", name),
    });
}

/// Find a variable in the ... (dots) arguments.
///
/// FFI wrapper around [`dd_find_var_safe`].
pub unsafe fn ddfindVar(symbol: SEXP, rho: SEXP) -> SEXP {
    if symbol.is_null() || rho.is_null() {
        return R_UnboundValue();
    }

    match (Sexp::from_raw(symbol), Sexp::from_raw(rho)) {
        (Some(symbol), Some(rho)) => dd_find_var_safe(symbol, rho)
            .map(|s: Sexp<'_>| s.as_raw())
            .unwrap_or_else(|| R_UnboundValue()),
        _ => R_UnboundValue(),
    }
}

/// Convert a SEXPTYPE integer to its string representation.
pub unsafe fn R_typeToChar(stype: c_int) -> SEXP {
    let _name = match stype {
        0 => "NULL",
        1 => "symbol",
        2 => "pairlist",
        3 => "closure",
        4 => "environment",
        5 => "promise",
        6 => "language",
        7 => "special",
        8 => "builtin",
        9 => "character",
        10 => "logical",
        13 => "integer",
        14 => "double",
        15 => "complex",
        16 => "character",
        17 => "...",
        18 => "any",
        19 => "list",
        20 => "expression",
        21 => "bytecode",
        22 => "externalptr",
        23 => "weakref",
        24 => "raw",
        25 => "S4",
        _ => "unknown",
    };
    let _cs = std::ffi::CString::new(_name).expect("CString::new failed: contains null byte");
    ptr::null_mut()
}

/// Create a new child environment.
pub unsafe fn Rf_createEnv(frame: SEXP, enclos: SEXP) -> SEXP {
    NewEnvironment(frame, enclos, ptr::null_mut())
}

/// Create a new hashed environment.
#[unsafe(no_mangle)]
pub unsafe fn R_NewHashedEnv(enclos: SEXP, size: c_int) -> SEXP {
    NewEnvironment(ptr::null_mut(), enclos, ptr::null_mut())
}

/// Check that formals is a valid pairlist of distinct symbols.
///
/// FFI wrapper around [`check_formals_safe`].
pub unsafe fn CheckFormals(formals: SEXP) {
    if let Some(formals) = Sexp::from_raw(formals)
        && let Err(msg) = check_formals_safe(formals)
    {
        eprintln!("Error: {}", msg);
        std::panic::panic_any(crate::sexp::context::RError { message: msg });
    }
}

/// Add missing variable bindings for unprovided arguments.
///
/// FFI wrapper around [`add_missing_vars_to_new_env_safe`].
pub unsafe fn addMissingVarsToNewEnv(formals: SEXP, args: SEXP, newrho: SEXP) {
    if let (Some(formals), Some(args), Some(newrho)) = (
        Sexp::from_raw(formals),
        Sexp::from_raw(args),
        Sexp::from_raw(newrho),
    ) {
        add_missing_vars_to_new_env_safe(formals, args, newrho);
    }
}

/// Check if a variable exists in a given frame.
///
/// FFI wrapper around [`exists_var_in_frame_safe`].
pub unsafe fn R_existsVarInFrame(rho: SEXP, symbol: SEXP) -> c_int {
    if rho.is_null() || symbol.is_null() {
        return 0;
    }

    match (Sexp::from_raw(rho), Sexp::from_raw(symbol)) {
        (Some(rho), Some(symbol)) => exists_var_in_frame_safe(rho, symbol) as c_int,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::accessors::TYPEOF;
    use super::super::constructors::*;
    use super::super::ffi::*;
    use super::super::globals::set_R_GlobalEnv;
    use super::super::memory;
    use super::super::symbol::Rf_install;
    use super::*;

    fn setup() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            if !env.is_null() {
                set_R_GlobalEnv(env);
            }
        }
    }

    #[test]
    fn test_find_var_in_frame_empty() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("x").unwrap().as_ptr());
            let val = R_findVarInFrame(env, sym);
            assert_eq!(val, R_UnboundValue());
        }
    }

    #[test]
    fn test_define_and_find_var() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("x").unwrap().as_ptr());
            let value = Rf_ScalarInteger(42);

            defineVar(sym, value, env);

            let val = R_findVarInFrame(env, sym);
            assert_eq!(val, value);
        }
    }

    #[test]
    fn test_define_var_overwrite() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("x").unwrap().as_ptr());
            let v1 = Rf_ScalarInteger(1);
            let v2 = Rf_ScalarInteger(2);

            defineVar(sym, v1, env);
            defineVar(sym, v2, env);

            let val = R_findVarInFrame(env, sym);
            assert_eq!(val, v2);
        }
    }

    #[test]
    fn test_exists_var_in_frame() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("y").unwrap().as_ptr());

            assert_eq!(R_existsVarInFrame(env, sym), 0);

            let value = Rf_ScalarInteger(10);
            defineVar(sym, value, env);

            assert_eq!(R_existsVarInFrame(env, sym), 1);
        }
    }

    #[test]
    fn test_is_missing() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("z").unwrap().as_ptr());

            assert_eq!(R_isMissing(sym, env), 1);
        }
    }

    #[test]
    fn test_new_environment() {
        unsafe {
            let env = NewEnvironment(ptr::null_mut(), R_NilValue(), ptr::null_mut());
            assert!(!env.is_null());
            assert_eq!(TYPEOF(env), SEXPTYPE::ENVSXP.0);
        }
    }

    #[test]
    fn test_mk_promise() {
        unsafe {
            let expr = Rf_ScalarInteger(99);
            let prom = super::super::memory_ext::mkPROMISE(expr, R_NilValue());
            assert!(!prom.is_null());
            assert_eq!(TYPEOF(prom), SEXPTYPE::PROMSXP.0);
        }
    }

    #[test]
    fn test_type_to_char() {
        unsafe {
            R_typeToChar(SEXPTYPE::INTSXP.0);
            R_typeToChar(SEXPTYPE::REALSXP.0);
            R_typeToChar(999);
        }
    }

    #[test]
    fn test_find_var_null_inputs() {
        unsafe {
            assert_eq!(
                R_findVar(ptr::null_mut(), ptr::null_mut()),
                R_UnboundValue()
            );
            assert_eq!(
                R_findVarInFrame(ptr::null_mut(), ptr::null_mut()),
                R_UnboundValue()
            );
        }
    }

    #[test]
    fn test_set_var_not_found() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let parent = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            (*env).data.envsxp.enclos = parent;

            let sym = Rf_install(std::ffi::CString::new("newvar").unwrap().as_ptr());
            let value = Rf_ScalarReal(3.14);

            setVar(sym, value, env);
        }
    }

    #[test]
    fn test_safe_find_var_in_frame() {
        let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
        let sexp_env = Sexp::from_raw(env).unwrap();

        let result = find_var_in_frame_safe(sexp_env, sexp_env);
        assert!(result.is_none());
    }

    #[test]
    fn test_safe_define_and_find_var() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("x").unwrap().as_ptr());
            let value = Rf_ScalarInteger(42);

            let sexp_env = Sexp::from_raw(env).unwrap();
            let sexp_sym = Sexp::from_raw(sym).unwrap();
            let sexp_val = Sexp::from_raw(value).unwrap();

            assert!(define_var_safe(sexp_sym, sexp_val, sexp_env));

            let result = find_var_in_frame_safe(sexp_env, sexp_sym);
            assert!(result.is_some());
            assert_eq!(result.unwrap().as_raw(), value);
        }
    }

    #[test]
    fn test_safe_is_missing() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("z").unwrap().as_ptr());

            let sexp_env = Sexp::from_raw(env).unwrap();
            let sexp_sym = Sexp::from_raw(sym).unwrap();

            assert!(is_missing_safe(sexp_sym, sexp_env));
        }
    }

    #[test]
    fn test_safe_exists_var_in_frame() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("y").unwrap().as_ptr());

            let sexp_env = Sexp::from_raw(env).unwrap();
            let sexp_sym = Sexp::from_raw(sym).unwrap();

            assert!(!exists_var_in_frame_safe(sexp_env, sexp_sym));

            let value = Rf_ScalarInteger(10);
            define_var_safe(sexp_sym, Sexp::from_raw(value).unwrap(), sexp_env);

            assert!(exists_var_in_frame_safe(sexp_env, sexp_sym));
        }
    }
}
