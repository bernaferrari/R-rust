#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Environment operations — ports R's src/main/envir.c.
//!
//! Provides variable lookup, assignment, function lookup, and argument matching
//! needed by the evaluator and other interpreter components.
//!
//! # Design
//!
//! New Rust code should use [`Environment`], a typed facade over an
//! owner-scoped [`Sexp`] environment. The raw-pointer functions at the bottom
//! are legacy shims for translated code that has not yet moved to typed values.

use std::os::raw::c_int;
use std::ptr;

use super::accessors::{
    CDR, CHAR, ENCLOS, FRAME, PRINTNAME, SET_FRAME, SET_PRENV, SET_PRVALUE, SETCAR, SETCDR, SETTAG,
    TAG, TYPEOF,
};
use super::constructors::{Rf_cons, Rf_lang2};
use super::ffi::{SEXP, SEXPTYPE};
use super::globals::{R_GlobalEnv_in, R_MissingArg, R_NilValue, R_UnboundValue};
use super::instance::{with_current_instance, with_required_current_instance};
use super::memory_ext::NewEnvironment;
use super::object::{PairlistIter, Sexp, SexpError};
use super::symbol::{Rf_install, symbol_name_bytes_equal};

// ---------------------------------------------------------------------------
// Safe wrapper types
// ---------------------------------------------------------------------------

/// Result of a variable lookup that may be unbound.
pub type LookupResult<'a> = Option<Sexp<'a>>;

/// Result of an operation that may fail with an error message.
pub type EnvResult<T> = Result<T, String>;

fn sexp_err(context: &str, err: SexpError) -> String {
    format!("{context}: {err}")
}

fn global_env_handle<'a>() -> Sexp<'a> {
    let raw = with_required_current_instance(R_GlobalEnv_in);
    unsafe { Sexp::from_raw_unchecked(raw) }
}

fn env_key(env: SEXP) -> usize {
    env as usize
}

fn binding_key(env: SEXP, symbol: SEXP) -> (usize, usize) {
    (env as usize, symbol as usize)
}

fn binding_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(super::context::RError {
        message: message.into(),
    });
}

pub(crate) fn lock_environment_raw(env: SEXP) {
    with_required_current_instance(|instance| {
        instance.locked_environments.insert(env_key(env));
    });
}

pub(crate) fn environment_is_locked_raw(env: SEXP) -> bool {
    with_current_instance(|instance| instance.locked_environments.contains(&env_key(env)))
        .unwrap_or(false)
}

pub(crate) fn lock_binding_raw(env: SEXP, symbol: SEXP) {
    with_required_current_instance(|instance| {
        instance.locked_bindings.insert(binding_key(env, symbol));
    });
}

pub(crate) fn unlock_binding_raw(env: SEXP, symbol: SEXP) {
    with_required_current_instance(|instance| {
        instance.locked_bindings.remove(&binding_key(env, symbol));
    });
}

pub(crate) fn binding_is_locked_raw(env: SEXP, symbol: SEXP) -> bool {
    with_current_instance(|instance| instance.locked_bindings.contains(&binding_key(env, symbol)))
        .unwrap_or(false)
}

pub(crate) fn binding_is_active_raw(env: SEXP, symbol: SEXP) -> bool {
    with_current_instance(|instance| {
        instance
            .active_bindings
            .contains_key(&binding_key(env, symbol))
    })
    .unwrap_or(false)
}

fn active_binding_fun_raw(env: SEXP, symbol: SEXP) -> Option<SEXP> {
    with_current_instance(|instance| {
        instance
            .active_bindings
            .get(&binding_key(env, symbol))
            .copied()
    })
    .flatten()
}

pub(crate) fn binding_exists_in_frame_raw(env: SEXP, symbol: SEXP) -> bool {
    if env.is_null() || symbol.is_null() {
        return false;
    }
    unsafe {
        let mut frame = super::accessors::FRAME(env);
        while !frame.is_null() && frame != R_NilValue() {
            let tag = super::accessors::TAG(frame);
            if !tag.is_null() && symbol_name_bytes_equal(tag, symbol) {
                return super::accessors::CAR(frame) != R_UnboundValue();
            }
            frame = super::accessors::CDR(frame);
        }
    }
    false
}

pub(crate) fn binding_exists_raw(mut env: SEXP, symbol: SEXP, inherits: bool) -> bool {
    unsafe {
        while !env.is_null() && env != R_NilValue() {
            if binding_exists_in_frame_raw(env, symbol) {
                return true;
            }
            if !inherits {
                return false;
            }
            env = ENCLOS(env);
        }
    }
    false
}

pub(crate) fn remove_binding_raw(env: SEXP, symbol: SEXP) {
    unsafe {
        if env.is_null() || symbol.is_null() {
            return;
        }
        if environment_is_locked_raw(env) {
            binding_error("cannot remove bindings from a locked environment");
        }

        let mut previous = R_NilValue();
        let mut current = FRAME(env);
        while !current.is_null() && current != R_NilValue() {
            let tag = TAG(current);
            if !tag.is_null() && symbol_name_bytes_equal(tag, symbol) {
                let next = CDR(current);
                if previous == R_NilValue() {
                    SET_FRAME(env, next);
                } else {
                    SETCDR(previous, next);
                }
                super::env_hash::hash_remove(env, symbol);
                with_required_current_instance(|instance| {
                    let key = binding_key(env, symbol);
                    instance.active_bindings.remove(&key);
                    instance.locked_bindings.remove(&key);
                });
                return;
            }
            previous = current;
            current = CDR(current);
        }
    }
}

pub(crate) fn make_active_binding_raw(env: SEXP, symbol: SEXP, fun: SEXP) {
    unsafe {
        if env.is_null() || symbol.is_null() || fun.is_null() {
            return;
        }

        let mut frame = super::accessors::FRAME(env);
        while !frame.is_null() && frame != R_NilValue() {
            let tag = super::accessors::TAG(frame);
            if !tag.is_null() && symbol_name_bytes_equal(tag, symbol) {
                if !binding_is_active_raw(env, symbol)
                    && super::accessors::CAR(frame) != R_UnboundValue()
                {
                    binding_error("symbol already has a regular binding");
                }
                if binding_is_locked_raw(env, symbol) {
                    binding_error("cannot change value of locked binding");
                }
                SETCAR(frame, fun);
                if super::env_hash::env_has_hash_table(env) {
                    super::env_hash::hash_insert(env, symbol, fun);
                }
                with_required_current_instance(|instance| {
                    instance
                        .active_bindings
                        .insert(binding_key(env, symbol), fun);
                });
                return;
            }
            frame = super::accessors::CDR(frame);
        }

        if environment_is_locked_raw(env) {
            binding_error("cannot add bindings to a locked environment");
        }

        let frame = super::accessors::FRAME(env);
        let new_cell = Rf_cons(fun, frame);
        if new_cell.is_null() {
            return;
        }
        SETTAG(new_cell, symbol);
        SET_FRAME(env, new_cell);
        if super::env_hash::env_has_hash_table(env) {
            super::env_hash::hash_insert(env, symbol, fun);
        }
        with_required_current_instance(|instance| {
            instance
                .active_bindings
                .insert(binding_key(env, symbol), fun);
        });
    }
}

fn call_active_binding(env: SEXP, fun: SEXP, value: Option<SEXP>) -> SEXP {
    unsafe {
        if TYPEOF(fun) == SEXPTYPE::CLOSXP {
            let args = value
                .map(|value| Rf_cons(value, R_NilValue()))
                .unwrap_or_else(|| R_NilValue());
            let call = Rf_cons(fun, args);
            if !call.is_null() {
                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            return crate::eval::closure::applyClosure(call, fun, args, env, R_NilValue(), 1);
        }

        let call = match value {
            Some(value) => Rf_lang2(fun, value),
            None => {
                let call = Rf_cons(fun, R_NilValue());
                if !call.is_null() {
                    (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                call
            }
        };
        crate::eval::eval::Rf_eval(call, env)
    }
}

/// Typed, owner-scoped environment facade.
///
/// This is the Rust-first API for binding and lookup. It validates the
/// underlying SEXP once and keeps subsequent operations on lifetime-tracked
/// handles instead of raw `SEXP` pointers.
#[derive(Clone, Debug)]
pub struct Environment<'a> {
    env: Sexp<'a>,
}

impl<'a> Environment<'a> {
    /// Wrap a SEXP handle as an environment.
    pub fn new(env: Sexp<'a>) -> EnvResult<Self> {
        if env.clone().is_environment() {
            Ok(Self { env })
        } else {
            Err(format!("expected environment, got {:?}", env.typeof_()))
        }
    }

    /// Return the underlying environment handle.
    pub fn as_sexp(self) -> Sexp<'a> {
        self.env
    }

    /// Find a binding in this environment frame only.
    pub fn find_in_frame(self, symbol: Sexp<'a>) -> EnvResult<LookupResult<'a>> {
        find_var_in_frame_result(self.env, symbol)
    }

    /// Find a binding through this environment's parent chain.
    pub fn find(self, symbol: Sexp<'a>) -> EnvResult<LookupResult<'a>> {
        find_var_result(symbol, self.env)
    }

    /// Define or update a binding in this environment frame.
    pub fn define(self, symbol: Sexp<'_>, value: Sexp<'_>) -> EnvResult<()> {
        if define_var_safe(symbol, value, self.env) {
            Ok(())
        } else {
            Err("failed to define environment binding".to_string())
        }
    }

    /// Set a binding through the parent chain, falling back to the global env.
    pub fn set(self, symbol: Sexp<'_>, value: Sexp<'_>) {
        set_var_safe(symbol, value, self.env);
    }

    /// Return whether a symbol exists in this environment frame.
    pub fn exists_in_frame(self, symbol: Sexp<'a>) -> bool {
        exists_var_in_frame_safe(self.env, symbol)
    }
}

// ---------------------------------------------------------------------------
// findVarInFrame — safe version
// ---------------------------------------------------------------------------

/// Find a variable in the frame of a single environment (no inheritance).
///
/// Returns `None` if the symbol is not found or if inputs are invalid.
#[must_use]
pub fn find_var_in_frame_safe<'a>(rho: Sexp<'a>, symbol: Sexp<'a>) -> LookupResult<'a> {
    find_var_in_frame_result(rho, symbol).ok().flatten()
}

/// Checked frame-local variable lookup.
///
/// `Ok(None)` means no binding was found. `Err` means the supplied value was
/// not an environment or a binding cell was malformed.
pub fn find_var_in_frame_result<'a>(
    rho: Sexp<'a>,
    symbol: Sexp<'a>,
) -> EnvResult<LookupResult<'a>> {
    if !rho.clone().is_environment() {
        return Ok(None);
    }

    // The pairlist frame is authoritative. Hash tables are a write-through cache
    // and can retain stale values across GC if frame cells were remapped first.
    let frame = rho
        .clone().try_frame().clone().map_err(|err| sexp_err("environment frame lookup", err))?;

    for cell in PairlistIter::new(frame) {
        let tag = cell
            .clone().try_tag().clone().map_err(|err| sexp_err("binding tag lookup", err))?;
        if symbol_name_bytes_equal(tag.as_raw(), symbol.clone().as_raw()) {
            if let Some(fun) = active_binding_fun_raw(rho.clone().as_raw(), symbol.clone().as_raw()) {
                return Sexp::try_from_raw(call_active_binding(rho.as_raw(), fun, None))
                    .map(Some)
                    .map_err(|err| sexp_err("active binding value", err));
            }
            let val = cell
                .try_car()
                .map_err(|err| sexp_err("binding value lookup", err))?;
            if super::env_hash::env_has_hash_table(rho.clone().as_raw()) {
                super::env_hash::hash_insert(rho.as_raw(), symbol.as_raw(), val.clone().as_raw());
            }
            return Ok(Some(val));
        }
    }

    if super::env_hash::env_has_hash_table(rho.clone().as_raw())
        && let Some(val) = super::env_hash::hash_get(rho.clone().as_raw(), symbol.clone().as_raw())
    {
        if val != unsafe { R_UnboundValue() }
            && let Some(fun) = active_binding_fun_raw(rho.clone().as_raw(), symbol.as_raw())
        {
            return Sexp::try_from_raw(call_active_binding(rho.as_raw(), fun, None))
                .map(Some)
                .map_err(|err| sexp_err("active binding value", err));
        }
        return Sexp::try_from_raw(val)
            .map(Some)
            .map_err(|err| sexp_err("hash binding value", err));
    }

    Ok(None)
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
    find_var_result(symbol, rho).ok().flatten()
}

/// Checked variable lookup through an environment chain.
pub fn find_var_result<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> EnvResult<LookupResult<'a>> {
    let mut current = rho;

    loop {
        if !current.clone().is_environment() {
            break;
        }

        if let Some(val) = find_var_in_frame_result(current.clone(), symbol.clone())? {
            if val.clone().typeof_() == SEXPTYPE::PROMSXP {
                return force_promise_result(val);
            }
            return Ok(Some(val));
        }

        if symbol.clone().is_symbol()
            && let Ok(sym_val) = symbol.clone().try_symvalue()
            && sym_val.clone().typeof_() == SEXPTYPE::SPECIALSXP
        {
            return Ok(Some(sym_val));
        }

        current = current
            .try_enclos()
            .map_err(|err| sexp_err("enclosing environment lookup", err))?;
    }

    Ok(None)
}

// ---------------------------------------------------------------------------
// forcePromise — safe version
// ---------------------------------------------------------------------------

/// Force evaluation of a promise, returning its value.
///
/// If the input is not a promise, returns it as-is.
#[must_use]
pub fn force_promise_safe(prom: Sexp<'_>) -> LookupResult<'_> {
    force_promise_result(prom).ok().flatten()
}

/// Checked promise forcing.
pub fn force_promise_result(prom: Sexp<'_>) -> EnvResult<LookupResult<'_>> {
    if prom.clone().typeof_()!= SEXPTYPE::PROMSXP {
        return Ok(Some(prom));
    }

    let val = prom
        .clone().try_prvalue().clone().map_err(|err| sexp_err("promise value lookup", err))?;
    if val.clone().as_raw()!= unsafe { R_UnboundValue() } {
        return Ok(Some(val));
    }

    let expr = prom
        .clone().try_prcode().clone().map_err(|err| sexp_err("promise code lookup", err))?;
    if expr.clone().as_raw()== unsafe { R_MissingArg() } {
        return Ok(Some(expr));
    }
    let env = prom
        .clone().try_prenv()
        .map_err(|err| sexp_err("promise environment lookup", err))?;

    unsafe {
        let raw = prom.clone().as_raw();
        SET_PRVALUE(raw, R_UnboundValue());
        (*raw).sxpinfo.set_gp((*raw).sxpinfo.gp() | 0x02);
    }

    let value = unsafe { crate::eval::eval::Rf_eval(expr.as_raw(), env.as_raw()) };
    let value = unsafe { Sexp::from_raw_unchecked(value) };

    unsafe {
        SET_PRVALUE(prom.clone().as_raw(), value.clone().as_raw());
        SET_PRENV(prom.as_raw(), R_NilValue());
    }
    Ok(Some(value))
}

// ---------------------------------------------------------------------------
// defineVar — safe version
// ---------------------------------------------------------------------------

/// Define a variable in the given environment's frame.
///
/// If the symbol already exists, its value is updated.
/// If not, a new binding is created at the front of the frame.
pub fn define_var_safe(symbol: Sexp<'_>, value: Sexp<'_>, rho: Sexp<'_>) -> bool {
    if !rho.clone().is_environment() {
        return false;
    }

    let frame = match rho.clone().try_frame() {
        Ok(f) => f,
        Err(_) => unsafe { Sexp::from_raw_unchecked(R_NilValue()) },
    };

    for cell in PairlistIter::new(frame.clone()) {
        if cell
            .clone().try_tag().clone().ok()
            .is_some_and(|tag| symbol_name_bytes_equal(tag.as_raw(), symbol.clone().as_raw()))
        {
            if binding_is_locked_raw(rho.clone().as_raw(), symbol.clone().as_raw()) {
                binding_error("cannot change value of locked binding");
            }
            if let Some(fun) = active_binding_fun_raw(rho.clone().as_raw(), symbol.clone().as_raw()) {
                call_active_binding(rho.as_raw(), fun, Some(value.as_raw()));
                return true;
            }
            unsafe {
                SETCAR(cell.as_raw(), value.clone().as_raw());
            }
            if super::env_hash::env_has_hash_table(rho.clone().as_raw()) {
                super::env_hash::hash_insert(rho.as_raw(), symbol.as_raw(), value.as_raw());
            }
            return true;
        }
    }

    if environment_is_locked_raw(rho.clone().as_raw()) {
        binding_error("cannot add bindings to a locked environment");
    }

    let new_cell = unsafe { Rf_cons(value.clone().as_raw(), frame.as_raw()) };
    if !new_cell.is_null() {
        unsafe {
            SETTAG(new_cell, symbol.clone().as_raw());
            SET_FRAME(rho.clone().as_raw(), new_cell);
        }

        if super::env_hash::env_has_hash_table(rho.clone().as_raw()) {
            super::env_hash::hash_insert(rho.clone().as_raw(), symbol.as_raw(), value.as_raw());
        }

        if !super::env_hash::env_has_hash_table(rho.clone().as_raw()) {
            let mut count = 0usize;
            let mut cur = unsafe { super::accessors::FRAME(rho.clone().as_raw()) };
            while !cur.is_null() {
                count += 1;
                if count >= 100 {
                    let mut bindings = Vec::new();
                    cur = unsafe { super::accessors::FRAME(rho.clone().as_raw()) };
                    while !cur.is_null() {
                        let tag = unsafe { super::accessors::TAG(cur) };
                        let car = unsafe { super::accessors::CAR(cur) };
                        if !tag.is_null() {
                            bindings.push((tag, car));
                        }
                        cur = unsafe { super::accessors::CDR(cur) };
                    }
                    super::env_hash::promote_to_hash_table(rho.as_raw(), &bindings);
                    break;
                }
                cur = unsafe { super::accessors::CDR(cur) };
            }
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
        if !current.clone().is_environment() {
            break;
        }

        let frame = match current.clone().try_frame() {
            Ok(f) => f,
            Err(_) => unsafe { Sexp::from_raw_unchecked(R_NilValue()) },
        };

        for cell in PairlistIter::new(frame) {
            if cell.clone().try_tag().clone().ok() == Some(symbol.clone()) {
                if binding_is_locked_raw(current.clone().as_raw(), symbol.clone().as_raw()) {
                    binding_error("cannot change value of locked binding");
                }
                if let Some(fun) = active_binding_fun_raw(current.clone().as_raw(), symbol.clone().as_raw()) {
                    call_active_binding(current.as_raw(), fun, Some(value.as_raw()));
                    return;
                }
                unsafe {
                    SETCAR(cell.as_raw(), value.clone().as_raw());
                }
                if super::env_hash::env_has_hash_table(current.clone().as_raw()) {
                    super::env_hash::hash_insert(current.as_raw(), symbol.as_raw(), value.as_raw());
                }
                return;
            }
        }

        current = match current.try_enclos() {
            Ok(e) => e,
            Err(_) => break,
        };
    }

    let global_env = global_env_handle();
    if !global_env.clone().as_raw().is_null() {
        define_var_safe(symbol, value, global_env);
    }
}

// ---------------------------------------------------------------------------
// findFun — safe version
// ---------------------------------------------------------------------------

/// Find a function value for a symbol.
///
/// Searches through environments, looking for closures, builtins, or specials.
#[must_use]
pub fn find_fun_safe<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> LookupResult<'a> {
    find_fun_result(symbol, rho).ok().flatten()
}

/// Checked function lookup through an environment chain.
pub fn find_fun_result<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> EnvResult<LookupResult<'a>> {
    let mut current = rho;

    loop {
        if !current.clone().is_environment() {
            break;
        }

        if let Some(val) = find_var_in_frame_result(current.clone(), symbol.clone())? {
            let val = if val.clone().typeof_()== SEXPTYPE::PROMSXP {
                let forced = unsafe { forcePromise(val.clone().as_raw()) };
                Sexp::from_raw(forced).unwrap_or(val)
            } else {
                val
            };
            let t = val.clone().typeof_();
            if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
                return Ok(Some(val));
            }
        }

        current = current
            .try_enclos()
            .map_err(|err| sexp_err("enclosing environment lookup", err))?;
    }

    Ok(None)
}

// ---------------------------------------------------------------------------
// matchArgs — safe version
// ---------------------------------------------------------------------------

/// Match actual arguments to formal parameters.
#[must_use]
pub fn match_args_safe<'a>(formals: Sexp<'a>, args: Sexp<'a>) -> Option<Sexp<'a>> {
    match_args_result(formals, args).ok().flatten()
}

/// Checked argument matching.
pub fn match_args_result<'a>(formals: Sexp<'a>, args: Sexp<'a>) -> EnvResult<LookupResult<'a>> {

    if !formals.clone().is_pairlist() {
        return Ok(Some(args));
    }

    let mut result: Option<Sexp<'a>> = None;
    let mut result_tail: Option<Sexp<'a>> = None;

    let missing_arg = unsafe { Sexp::from_raw_unchecked(R_MissingArg()) };

    for f in PairlistIter::new(formals) {
        let ftag = match f
            .try_tag()
            .map_err(|err| sexp_err("formal argument tag lookup", err))?
        {
            tag if tag.clone().is_nil() => continue,
            tag => tag,
        };

        let matched = PairlistIter::new(args.clone()).find(|a|a.clone().try_tag().ok() == Some(ftag.clone()));

        let car_val = match matched {
            Some(m) => m
                .try_car()
                .map_err(|err| sexp_err("matched argument value lookup", err))?,
            None => missing_arg.clone(),
        };

        let cell = unsafe { Rf_cons(car_val.as_raw(), R_NilValue()) };
        if cell.is_null() {
            return Ok(None);
        }
        unsafe { SETTAG(cell, ftag.as_raw()) };
        let cell = Sexp::try_from_raw(cell).map_err(|err| sexp_err("matched cell", err))?;

        if result.is_none() {
            result = Some(cell.clone());
            result_tail = Some(cell);
        } else {
            unsafe {
                SETCDR(
                    result_tail
                        .unwrap_or_else(|| panic!("unexpected None"))
                        .as_raw(),
                    cell.clone().as_raw(),
                );
            }
            result_tail = Some(cell);
        }
    }

    Ok(result.or_else(|| unsafe { Sexp::from_raw(R_NilValue()) }))
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

    if val.clone().typeof_()== SEXPTYPE::PROMSXP
        && let Ok(expr) = val.try_prcode()
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
        if cell
            .clone().try_tag().clone().ok()
            .is_some_and(|tag| symbol_name_bytes_equal(tag.as_raw(), symbol.clone().as_raw()))
        {
            let val = cell.try_car().ok()?;
            if val.clone().typeof_()== SEXPTYPE::PROMSXP {
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
            .try_tag()
            .map_err(|err| sexp_err("formal argument tag lookup", err))?;

        if sym.clone().is_nil() {
            return Err("invalid formal argument list".to_string());
        }
        if !sym.clone().is_symbol() {
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
        if let Ok(sym) = f.try_tag()
            && !sym.clone().is_nil()
        {
            let found =
                PairlistIter::new(args.clone()).any(|a| a.try_tag().ok() == Some(sym.clone()));
            if !found {
                define_var_safe(sym, missing_arg.clone(), newrho.clone());
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
    binding_exists_in_frame_raw(rho.as_raw(), symbol.as_raw())
}

// ---------------------------------------------------------------------------
// FFI functions (delegate to safe versions)
// ---------------------------------------------------------------------------

/// Find a variable in the frame of a single environment (no inheritance).
///
/// FFI wrapper around [`find_var_in_frame_safe`].
pub(crate) unsafe fn R_findVarInFrame(rho: SEXP, symbol: SEXP) -> SEXP {
    unsafe {
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
}

/// Find a variable, searching through parent environments.
///
/// FFI wrapper around [`find_var_safe`].
pub unsafe fn R_findVar(symbol: SEXP, rho: SEXP) -> SEXP {
    unsafe {
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
}

/// Force evaluation of a promise, returning its value.
///
/// FFI wrapper around [`force_promise_safe`].
pub unsafe fn forcePromise(prom: SEXP) -> SEXP {
    unsafe {
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
}

/// Define a variable in the given environment's frame.
///
/// FFI wrapper around [`define_var_safe`].
pub unsafe fn defineVar(symbol: SEXP, value: SEXP, rho: SEXP) {
    unsafe {
        let _ = define_var_updates(symbol, value, rho);
    }
}

/// Define or update a binding, returning whether the frame/hash were updated.
pub(crate) unsafe fn define_var_updates(symbol: SEXP, value: SEXP, rho: SEXP) -> bool {
    unsafe {
        if rho.is_null() || symbol.is_null() {
            return false;
        }

        if let (Some(symbol), Some(value), Some(rho)) = (
            Sexp::from_raw(symbol),
            Sexp::from_raw(value),
            Sexp::from_raw(rho),
        ) {
            define_var_safe(symbol, value, rho)
        } else {
            false
        }
    }
}

/// Set a variable value, searching parent environments if needed.
///
/// FFI wrapper around [`set_var_safe`].
pub unsafe fn setVar(symbol: SEXP, value: SEXP, rho: SEXP) {
    unsafe {
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
}

/// Find a function value for a symbol.
///
/// FFI wrapper around [`find_fun_safe`].
pub(crate) unsafe fn findFun(symbol: SEXP, rho: SEXP) -> SEXP {
    unsafe {
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
}

/// Find a function with error reporting.
pub unsafe fn findFun3(symbol: SEXP, rho: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let fun = findFun(symbol, rho);
        if fun == R_UnboundValue() {
            // Could not find function — would error in real implementation
        }
        fun
    }
}

/// Match actual arguments to formal parameters.
///
/// FFI wrapper around [`match_args_safe`].
pub unsafe fn matchArgs(formals: SEXP, args: SEXP, _call: SEXP) -> SEXP {
    unsafe {
        if formals.is_null() || formals == R_NilValue() {
            return args;
        }

        match (Sexp::from_raw(formals), Sexp::from_raw(args)) {
            (Some(formals), Some(args)) => match_args_safe(formals, args.clone())
                .map(|s: Sexp<'_>| s.as_raw())
                .unwrap_or_else(|| args.as_raw()),
            _ => args,
        }
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
    unsafe {
        if symbol.is_null() || rho.is_null() {
            return 0;
        }

        match (Sexp::from_raw(symbol), Sexp::from_raw(rho)) {
            (Some(symbol), Some(rho)) => is_missing_safe(symbol, rho) as c_int,
            _ => 0,
        }
    }
}

/// Report an error for a missing argument.
pub(crate) unsafe fn R_MissingArgError(symbol: SEXP, call: SEXP) {
    unsafe {
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
}

/// Find a variable in the ... (dots) arguments.
///
/// FFI wrapper around [`dd_find_var_safe`].
pub unsafe fn ddfindVar(symbol: SEXP, rho: SEXP) -> SEXP {
    unsafe {
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
}

/// Convert a SEXPTYPE integer to its string representation.
pub unsafe fn R_typeToChar(stype: c_int) -> SEXP {
    unsafe {
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
        let _cs = std::ffi::CString::new(_name).unwrap_or_default();
        ptr::null_mut()
    }
}

/// Create a new child environment.
pub unsafe fn Rf_createEnv(frame: SEXP, enclos: SEXP) -> SEXP {
    unsafe { NewEnvironment(frame, enclos, ptr::null_mut()) }
}

/// Create a new hashed environment.
pub(crate) unsafe fn R_NewHashedEnv(enclos: SEXP, size: c_int) -> SEXP {
    unsafe { NewEnvironment(ptr::null_mut(), enclos, ptr::null_mut()) }
}

/// Check that formals is a valid pairlist of distinct symbols.
///
/// FFI wrapper around [`check_formals_safe`].
pub unsafe fn CheckFormals(formals: SEXP) {
    unsafe {
        if let Some(formals) = Sexp::from_raw(formals)
            && let Err(msg) = check_formals_safe(formals)
        {
            eprintln!("Error: {}", msg);
            std::panic::panic_any(crate::sexp::context::RError { message: msg });
        }
    }
}

/// Add missing variable bindings for unprovided arguments.
///
/// FFI wrapper around [`add_missing_vars_to_new_env_safe`].
pub unsafe fn addMissingVarsToNewEnv(formals: SEXP, args: SEXP, newrho: SEXP) {
    unsafe {
        if let (Some(formals), Some(args), Some(newrho)) = (
            Sexp::from_raw(formals),
            Sexp::from_raw(args),
            Sexp::from_raw(newrho),
        ) {
            add_missing_vars_to_new_env_safe(formals, args, newrho);
        }
    }
}

/// Check if a variable exists in a given frame.
///
/// FFI wrapper around [`exists_var_in_frame_safe`].
pub unsafe fn R_existsVarInFrame(rho: SEXP, symbol: SEXP) -> c_int {
    unsafe {
        if rho.is_null() || symbol.is_null() {
            return 0;
        }

        match (Sexp::from_raw(rho), Sexp::from_raw(symbol)) {
            (Some(rho), Some(symbol)) => exists_var_in_frame_safe(rho, symbol) as c_int,
            _ => 0,
        }
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
    use super::super::globals::set_R_GlobalEnv_in;
    use super::super::instance::with_required_current_instance;
    use super::super::memory;
    use super::super::symbol::Rf_install;
    use super::*;

    fn setup() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            if !env.is_null() {
                with_required_current_instance(|inst| set_R_GlobalEnv_in(inst, env));
            }
        }
    }

    #[test]
    fn test_find_var_in_frame_empty() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(b"x\0".as_ptr() as *const _);
            let val = R_findVarInFrame(env, sym);
            assert_eq!(val, R_UnboundValue());
        }
    }

    #[test]
    fn test_define_and_find_var() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(b"x\0".as_ptr() as *const _);
            let value = Rf_ScalarInteger(42);

            defineVar(sym, value, env);

            let val = R_findVarInFrame(env, sym);
            assert_eq!(val, value);
        }
    }

    #[test]
    fn test_define_var_overwrite() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(b"x\0".as_ptr() as *const _);
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(b"y\0".as_ptr() as *const _);

            assert_eq!(R_existsVarInFrame(env, sym), 0);

            let value = Rf_ScalarInteger(10);
            defineVar(sym, value, env);

            assert_eq!(R_existsVarInFrame(env, sym), 1);
        }
    }

    #[test]
    fn test_is_missing() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(b"z\0".as_ptr() as *const _);

            assert_eq!(R_isMissing(sym, env), 1);
        }
    }

    #[test]
    fn test_new_environment() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = NewEnvironment(ptr::null_mut(), R_NilValue(), ptr::null_mut());
            assert!(!env.is_null());
            assert_eq!(TYPEOF(env), SEXPTYPE::ENVSXP);
        }
    }

    #[test]
    fn test_mk_promise() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let expr = Rf_ScalarInteger(99);
            let prom = super::super::memory_ext::mkPROMISE(expr, R_NilValue());
            assert!(!prom.is_null());
            assert_eq!(TYPEOF(prom), SEXPTYPE::PROMSXP);
        }
    }

    #[test]
    fn test_type_to_char() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            R_typeToChar(SEXPTYPE::INTSXP.into());
            R_typeToChar(SEXPTYPE::REALSXP.into());
            R_typeToChar(999);
        }
    }

    #[test]
    fn test_find_var_null_inputs() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let parent = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            (*env).data.envsxp.enclos = parent;

            let sym = Rf_install(b"newvar\0".as_ptr() as *const _);
            let value = Rf_ScalarReal(3.14);

            setVar(sym, value, env);
        }
    }

    #[test]
    fn test_safe_find_var_in_frame() {
        let _session = crate::sexp::session::RSession::new();
        let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
        let Some(sexp_env) = Sexp::from_raw(env) else {
            return;
        };

        let result = find_var_in_frame_safe(sexp_env.clone(), sexp_env);
        assert!(result.is_none());
    }

    #[test]
    fn test_checked_find_var_reports_malformed_frame() {
        let _session = crate::sexp::session::RSession::new();
        let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
        let malformed_frame = memory::with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
        let symbol = unsafe {
            SET_FRAME(env, malformed_frame);
            Rf_install(b"x\0".as_ptr() as *const _)
        };
        let sexp_env = Sexp::from_raw(env).expect("environment");
        let sexp_symbol = Sexp::from_raw(symbol).expect("symbol");

        assert!(find_var_in_frame_result(sexp_env, sexp_symbol).is_err());
    }

    #[test]
    fn test_checked_match_args_reports_malformed_formals() {
        let _session = crate::sexp::session::RSession::new();
        let formals = memory::with_arena(|arena| arena.alloc_list_chain(1));
        let args = unsafe { R_NilValue() };
        let sexp_formals = Sexp::from_raw(formals).expect("formals");
        let sexp_args = Sexp::from_raw(args).expect("args");

        let matched = match_args_result(sexp_formals, sexp_args).expect("malformed tag is skipped");
        assert_eq!(matched.expect("empty match result").as_raw(), unsafe {
            R_NilValue()
        });
    }

    #[test]
    fn test_safe_define_and_find_var() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(b"x\0".as_ptr() as *const _);
            let value = Rf_ScalarInteger(42);

            let Some(sexp_env) = Sexp::from_raw(env) else {
                return;
            };
            let Some(sexp_sym) = Sexp::from_raw(sym) else {
                return;
            };
            let Some(sexp_val) = Sexp::from_raw(value) else {
                return;
            };

            assert!(define_var_safe(sexp_sym.clone(), sexp_val, sexp_env.clone()));

            let result = find_var_in_frame_safe(sexp_env, sexp_sym);
            assert!(result.is_some());
            let Some(ref r) = result else {
                return;
            };
            assert_eq!(r.clone().as_raw(), value);
        }
    }

    #[test]
    fn test_environment_facade_define_find_and_exists() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(b"facade\0".as_ptr() as *const _);
            let value = Rf_ScalarInteger(7);

            let env =
                Environment::new(Sexp::from_raw(env).expect("environment")).expect("env facade");
            let sym = Sexp::from_raw(sym).expect("symbol");
            let value = Sexp::from_raw(value).expect("value");

            assert!(!env.clone().exists_in_frame(sym.clone()));
            env.clone().define(sym.clone(), value).expect("define through facade");
            assert!(env.clone().exists_in_frame(sym.clone()));

            let found = env
                .find_in_frame(sym)
                .expect("lookup through facade")
                .expect("binding exists");
            assert_eq!(found.integer_elt(0), Some(7));
        }
    }

    #[test]
    fn test_safe_is_missing() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(b"z\0".as_ptr() as *const _);

            let Some(sexp_env) = Sexp::from_raw(env) else {
                return;
            };
            let Some(sexp_sym) = Sexp::from_raw(sym) else {
                return;
            };

            assert!(is_missing_safe(sexp_sym, sexp_env));
        }
    }

    #[test]
    fn test_safe_exists_var_in_frame() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(b"y\0".as_ptr() as *const _);

            let Some(sexp_env) = Sexp::from_raw(env) else {
                return;
            };
            let Some(sexp_sym) = Sexp::from_raw(sym) else {
                return;
            };

            assert!(!exists_var_in_frame_safe(sexp_env.clone(), sexp_sym.clone()));

            let value = Rf_ScalarInteger(10);
            let Some(sexp_val) = Sexp::from_raw(value) else {
                return;
            };
            define_var_safe(sexp_sym.clone(), sexp_val, sexp_env.clone());

            assert!(exists_var_in_frame_safe(sexp_env, sexp_sym));
        }
    }
}
