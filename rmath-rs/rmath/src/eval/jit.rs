#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! JIT (Just-In-Time) compilation support -- ports JIT functions from eval.c.
//!
//! This module provides the JIT compilation infrastructure that R uses to
//! compile function bodies to bytecode on the fly when they are called
//! frequently enough.
//!
//! The key functions are:
//! - JIT_score: compute a score for whether a function should be compiled
//! - R_cmpfun: compile a function to bytecode
//! - R_compileExpr: compile an expression
//! - R_init_jit_enabled: initialize JIT from environment variables
//! - R_CheckJIT: check if a function should be JIT-compiled

use std::os::raw::{c_char, c_int};

use crate::eval::attrib_core::{R_SrcRefSymbol, getAttrib};
use crate::sexp::accessors::{
    BODY, CAR, CDR, CHAR, LENGTH, PRINTNAME, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::Rf_cons;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::instance::{RInstance, with_required_current_instance};
use crate::sexp::memory::with_arena_in;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install_in;

use super::eval::Rf_eval;

const BYTECODE_COMPILER_AVAILABLE: bool = false;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JitSettings {
    jit_enabled: c_int,
    compile_pkgs: c_int,
    disable_bytecode: c_int,
    min_jit_score: c_int,
    loop_jit_score: c_int,
}

impl JitSettings {
    fn from_env_values(
        enable_jit: Option<&str>,
        compile_pkgs: Option<&str>,
        disable_bytecode: Option<&str>,
    ) -> Self {
        let disable_bytecode = parse_enabled_flag(disable_bytecode, FALSE);
        let mut jit_enabled = parse_jit_level(enable_jit, 3);
        if disable_bytecode != FALSE || !BYTECODE_COMPILER_AVAILABLE {
            jit_enabled = 0;
        }
        let (min_jit_score, loop_jit_score) = jit_thresholds(jit_enabled);
        JitSettings {
            jit_enabled,
            compile_pkgs: parse_enabled_flag(compile_pkgs, FALSE),
            disable_bytecode,
            min_jit_score,
            loop_jit_score,
        }
    }
}

fn parse_jit_level(value: Option<&str>, default: c_int) -> c_int {
    value
        .and_then(|value| value.parse::<c_int>().ok())
        .unwrap_or(default)
        .clamp(0, 3)
}

fn parse_enabled_flag(value: Option<&str>, default: c_int) -> c_int {
    if parse_jit_level(value, default) > 0 {
        TRUE
    } else {
        FALSE
    }
}

fn jit_thresholds(jit_enabled: c_int) -> (c_int, c_int) {
    match jit_enabled {
        0 => (c_int::MAX, c_int::MAX),
        1 => (500, 500),
        2 => (100, 100),
        _ => (50, 50),
    }
}

fn current_env_settings() -> JitSettings {
    let enable_jit = std::env::var("R_ENABLE_JIT").ok();
    let compile_pkgs = std::env::var("_R_COMPILE_PKGS_").ok();
    let disable_bytecode = std::env::var("R_DISABLE_BYTECODE").ok();
    JitSettings::from_env_values(
        enable_jit.as_deref(),
        compile_pkgs.as_deref(),
        disable_bytecode.as_deref(),
    )
}

fn apply_jit_settings(settings: JitSettings) {
    with_required_current_instance(|inst| apply_jit_settings_in(inst, settings));
}

fn apply_jit_settings_in(inst: &mut RInstance, settings: JitSettings) {
    inst.eval_state.jit_enabled = settings.jit_enabled;
    inst.eval_state.compile_pkgs = settings.compile_pkgs;
    inst.eval_state.disable_bytecode = settings.disable_bytecode;
    inst.eval_state.min_jit_score = settings.min_jit_score;
    inst.eval_state.loop_jit_score = settings.loop_jit_score;
}

pub fn bytecode_compiler_available() -> bool {
    BYTECODE_COMPILER_AVAILABLE
}

// ---------------------------------------------------------------------------
// JIT scoring
// ---------------------------------------------------------------------------

/// Compute a hash for an expression.
///
/// Ported from R's `hashexpr1()` in eval.c. Uses the djb2 hash algorithm.
type R_exprhash_t = u64;

unsafe fn hash(data: &[u8], mut h: R_exprhash_t) -> R_exprhash_t {
    for &byte in data {
        h = h.wrapping_mul(33).wrapping_add(byte as u64);
    }
    h
}

unsafe fn hashexpr1(e: SEXP, h: R_exprhash_t) -> R_exprhash_t {
    unsafe {
        if e.is_null() || e == R_NilValue() {
            return h;
        }
        let len = LENGTH(e);
        let type_val = TYPEOF(e);
        let mut h = hash(&type_val.to_ne_bytes(), h);
        h = hash(&len.to_ne_bytes(), h);

        match type_val {
            t if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::LISTSXP => {
                let mut cur = e;
                while !cur.is_null() && cur != R_NilValue() {
                    h = hashexpr1(CAR(cur), h);
                    cur = CDR(cur);
                }
                h
            }
            t if t == SEXPTYPE::LGLSXP && len == 1 => {
                let data = crate::sexp::accessors::LOGICAL(e);
                if !data.is_null() {
                    h = hash(&(*data as i32).to_ne_bytes(), h);
                }
                h
            }
            t if t == SEXPTYPE::INTSXP && len == 1 => {
                let data = crate::sexp::accessors::INTEGER(e);
                if !data.is_null() {
                    h = hash(&(*data).to_ne_bytes(), h);
                }
                h
            }
            t if t == SEXPTYPE::REALSXP && len == 1 => {
                let data = crate::sexp::accessors::REAL(e);
                if !data.is_null() {
                    h = hash(&(*data).to_ne_bytes(), h);
                }
                h
            }
            t if t == SEXPTYPE::STRSXP && len == 1 => {
                let elt = STRING_ELT(e, 0);
                if !elt.is_null() {
                    let cs = CHAR(elt);
                    if !cs.is_null() {
                        let s = std::ffi::CStr::from_ptr(cs);
                        h = hash(s.to_bytes(), h);
                    }
                }
                h
            }
            _ => {
                // Hash by address for non-scalar types
                let addr = e as u64;
                h = hash(&addr.to_ne_bytes(), h);
                h
            }
        }
    }
}

unsafe fn hashexpr(e: SEXP) -> R_exprhash_t {
    unsafe {
        hashexpr1(e, 5381) // djb2 initial value
    }
}

unsafe fn hashfun(f: SEXP) -> R_exprhash_t {
    unsafe {
        let body = BODY(f);
        let mut h = hashexpr(body);
        if getAttrib(body, R_SrcRefSymbol()) == R_NilValue() {
            let srcref = getAttrib(f, R_SrcRefSymbol());
            h = hashsrcref(srcref, h);
        }
        h
    }
}

unsafe fn hashsrcref(e: SEXP, mut h: R_exprhash_t) -> R_exprhash_t {
    unsafe {
        if e.is_null() || TYPEOF(e) != SEXPTYPE::INTSXP || LENGTH(e) < 6 {
            return h;
        }
        let data = crate::sexp::accessors::INTEGER(e);
        if !data.is_null() {
            for i in 0..6 {
                h = hash(&(*data.add(i)).to_ne_bytes(), h);
            }
        }
        h
    }
}

/// Compute the JIT score for a closure.
///
/// Ported from R's `JIT_score()` in eval.c. The score is based on:
/// - Body complexity (number of function calls, loops, etc.)
/// - Whether the function is called in a loop
/// - Expression hash (for caching)
///
/// Functions with a score >= MIN_JIT_SCORE are compiled to bytecode.
pub unsafe fn JIT_score(e: SEXP) -> c_int {
    unsafe {
        if e.is_null() || TYPEOF(e) != SEXPTYPE::CLOSXP {
            return 0;
        }

        let body = BODY(e);
        if body.is_null() {
            return 0;
        }

        // If already bytecode, no need to compile
        if TYPEOF(body) == SEXPTYPE::BCODESXP {
            return 0;
        }

        let mut score: c_int = 0;

        // Count function calls and loops in the body
        score += count_calls(body, 0);

        // Add score for loops
        score += count_loops(body, 0);

        // Check if the function has been called enough times
        score
    }
}

/// Recursively count function calls in an expression.
fn count_calls(e: SEXP, depth: c_int) -> c_int {
    if depth > 20 {
        return 0; // prevent stack overflow
    }
    unsafe {
        if e.is_null() || e == R_NilValue() {
            return 0;
        }
        match TYPEOF(e) {
            t if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::LISTSXP => {
                let mut count = 0;
                // The CAR of a LANGSXP is the function being called
                if TYPEOF(e) == SEXPTYPE::LANGSXP {
                    count += 1;
                }
                // Recurse into sub-expressions
                let mut cur = e;
                while !cur.is_null() && cur != R_NilValue() {
                    count += count_calls(CAR(cur), depth + 1);
                    cur = CDR(cur);
                }
                count
            }
            _ => 0,
        }
    }
}

/// Recursively count loops in an expression.
fn count_loops(e: SEXP, depth: c_int) -> c_int {
    if depth > 20 {
        return 0;
    }
    unsafe {
        if e.is_null() || e == R_NilValue() {
            return 0;
        }
        let mut count = 0;
        match TYPEOF(e) {
            t if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::LISTSXP => {
                // Check if this is a loop call (while, for, repeat)
                let fun = CAR(e);
                if TYPEOF(fun) == SEXPTYPE::SYMSXP {
                    let pname = PRINTNAME(fun);
                    if !pname.is_null() {
                        let s = CHAR(pname);
                        if !s.is_null() {
                            let name = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                            if name == "while" || name == "for" || name == "repeat" {
                                count += 10; // Loops are heavily weighted
                            }
                        }
                    }
                }
                // Recurse
                let mut cur = e;
                while !cur.is_null() && cur != R_NilValue() {
                    count += count_loops(CAR(cur), depth + 1);
                    cur = CDR(cur);
                }
            }
            _ => {} // intentionally unhandled: SEXPTYPE has no loops to count
        }
        count
    }
}

// ---------------------------------------------------------------------------
// JIT compilation
// ---------------------------------------------------------------------------

/// Compile a function to bytecode.
///
/// Ported from R's `R_cmpfun()` in eval.c. Compiles the body of a
/// closure to bytecode if it isn't already compiled.
pub unsafe fn R_cmpfun(fun: SEXP) {
    unsafe {
        if fun.is_null() || TYPEOF(fun) != SEXPTYPE::CLOSXP {
            return;
        }

        let body = BODY(fun);
        if body.is_null() || TYPEOF(body) == SEXPTYPE::BCODESXP {
            return; // Already compiled or no body
        }

        if !BYTECODE_COMPILER_AVAILABLE || get_R_disable_bytecode() != FALSE {
            return;
        }

        // The bytecode compiler is deliberately gated until the compiler
        // package pipeline is ported. Automatic JIT must not mutate closures
        // or report success while compilation is unavailable.
    }
}

/// Compile an expression to bytecode.
///
/// Ported from R's `R_compileExpr()` in eval.c.
pub unsafe fn R_compileExpr(expr: SEXP, _rho: SEXP) -> SEXP {
    if !BYTECODE_COMPILER_AVAILABLE || get_R_disable_bytecode() != FALSE {
        return expr;
    }
    expr
}

// ---------------------------------------------------------------------------
// bytecodeExpr -- get the source expression from bytecode
// ---------------------------------------------------------------------------

/// Get the original expression from a bytecode object.
///
/// Ported from R's `bytecodeExpr()` in eval.c. If the expression is
/// bytecode, returns the first constant (the original expression).
/// Otherwise returns the expression as-is.
pub unsafe fn bytecodeExpr(e: SEXP) -> SEXP {
    unsafe {
        if !e.is_null() && TYPEOF(e) == SEXPTYPE::BCODESXP {
            let consts = super::bc_eval::BCODE_CONSTS(e);
            if !consts.is_null() && LENGTH(consts) > 0 {
                return crate::sexp::accessors::VECTOR_ELT(consts, 0);
            } else {
                return R_NilValue();
            }
        }
        e
    }
}

/// Get the bytecode expression (public API).
pub unsafe fn R_BytecodeExpr(e: SEXP) -> SEXP {
    unsafe { bytecodeExpr(e) }
}

/// Get the promise expression.
/// Note: no_mangle removed to avoid duplicate symbol with sexp/envir.rs.
pub(crate) unsafe fn r_PromiseExpr(p: SEXP) -> SEXP {
    unsafe {
        if p.is_null() || TYPEOF(p) != SEXPTYPE::PROMSXP {
            return R_NilValue();
        }
        bytecodeExpr(crate::sexp::accessors::PRCODE(p))
    }
}

/// Get the closure body expression.
pub unsafe fn R_ClosureExpr(p: SEXP) -> SEXP {
    unsafe {
        if p.is_null() || TYPEOF(p) != SEXPTYPE::CLOSXP {
            return R_NilValue();
        }
        bytecodeExpr(BODY(p))
    }
}

// ---------------------------------------------------------------------------
// R_ParseEvalString -- delegates to gram_main.rs
// ---------------------------------------------------------------------------

/// Parse and evaluate a string in an environment.
/// The primary definition lives in gram_main.rs; this is a thin wrapper
/// with #[no_mangle] removed to avoid duplicate symbol errors.
pub(crate) unsafe fn r_parse_eval_string(str: *const c_char, env: SEXP) -> SEXP {
    unsafe { crate::mainutils::gram_main::R_ParseEvalString(str, env) }
}

pub(crate) unsafe fn r_parse_string(str: *const c_char) -> SEXP {
    unsafe { r_parse_eval_string(str, std::ptr::null_mut()) }
}

/// Check compiler options.
///
/// Ported from R's `checkCompilerOptions()` in eval.c.
unsafe fn checkCompilerOptions(jitEnabled: c_int) {
    let (min_jit_score, loop_jit_score) = jit_thresholds(jitEnabled);
    with_required_current_instance(|inst| {
        set_R_min_jit_score_in(inst, min_jit_score);
        set_R_loop_jit_score_in(inst, loop_jit_score);
    });
}

/// Initialize JIT from environment variables.
///
/// Ported from R's `R_init_jit_enabled()` in eval.c. Reads:
/// - R_ENABLE_JIT: enable/disable JIT (default 3 = enabled)
/// - _R_COMPILE_PKGS_: compile package code
/// - R_DISABLE_BYTECODE: disable bytecode
/// - _R_COMPILE_PKGS_: compile packages
pub unsafe fn R_init_jit_enabled() {
    unsafe {
        let settings = current_env_settings();
        checkCompilerOptions(settings.jit_enabled);
        apply_jit_settings(settings);
    }
}

pub(crate) fn R_init_jit_enabled_in(inst: &mut RInstance) {
    let settings = current_env_settings();
    let (min_jit_score, loop_jit_score) = jit_thresholds(settings.jit_enabled);
    set_R_min_jit_score_in(inst, min_jit_score);
    set_R_loop_jit_score_in(inst, loop_jit_score);
    apply_jit_settings_in(inst, settings);
}

/// Check if a function should be JIT-compiled.
///
/// Ported from R's `R_CheckJIT()` in eval.c. Returns TRUE if the
/// function should be compiled based on JIT settings and scoring.
pub unsafe fn R_CheckJIT(op: SEXP) -> c_int {
    unsafe {
        if get_R_jit_enabled() == 0 || get_R_disable_bytecode() != 0 || !BYTECODE_COMPILER_AVAILABLE
        {
            return FALSE;
        }
        if op.is_null() || TYPEOF(op) != SEXPTYPE::CLOSXP {
            return FALSE;
        }
        let body = BODY(op);
        if TYPEOF(body) == SEXPTYPE::BCODESXP {
            return FALSE; // Already compiled
        }
        let score = JIT_score(op);
        if score >= with_required_current_instance(get_R_min_jit_score_in) {
            R_cmpfun(op);
            TRUE
        } else {
            FALSE
        }
    }
}

/// Get whether JIT is enabled.
pub fn get_R_jit_enabled() -> c_int {
    with_required_current_instance(get_R_jit_enabled_in)
}

pub(crate) fn get_R_jit_enabled_in(inst: &mut RInstance) -> c_int {
    inst.eval_state.jit_enabled
}

/// Set whether JIT is enabled.
pub fn set_R_jit_enabled(val: c_int) {
    with_required_current_instance(|inst| set_R_jit_enabled_in(inst, val));
}

pub(crate) fn set_R_jit_enabled_in(inst: &mut RInstance, val: c_int) {
    inst.eval_state.jit_enabled = val;
}

/// Get whether to compile packages.
pub fn get_R_compile_pkgs() -> c_int {
    with_required_current_instance(get_R_compile_pkgs_in)
}

pub(crate) fn get_R_compile_pkgs_in(inst: &mut RInstance) -> c_int {
    inst.eval_state.compile_pkgs
}

/// Get whether bytecode is disabled.
pub fn get_R_disable_bytecode() -> c_int {
    with_required_current_instance(get_R_disable_bytecode_in)
}

pub(crate) fn get_R_disable_bytecode_in(inst: &mut RInstance) -> c_int {
    inst.eval_state.disable_bytecode
}

/// Get the constant checking level.
pub fn get_R_check_constants() -> c_int {
    with_required_current_instance(get_R_check_constants_in)
}

pub(crate) fn get_R_check_constants_in(inst: &mut RInstance) -> c_int {
    inst.eval_state.check_constants
}

pub(crate) fn get_R_min_jit_score_in(inst: &mut RInstance) -> c_int {
    inst.eval_state.min_jit_score
}

pub(crate) fn set_R_min_jit_score_in(inst: &mut RInstance, val: c_int) {
    inst.eval_state.min_jit_score = val;
}

pub(crate) fn get_R_loop_jit_score_in(inst: &mut RInstance) -> c_int {
    inst.eval_state.loop_jit_score
}

pub(crate) fn set_R_loop_jit_score_in(inst: &mut RInstance, val: c_int) {
    inst.eval_state.loop_jit_score = val;
}

// ---------------------------------------------------------------------------
// R_exec_token -- for tail call optimization
// ---------------------------------------------------------------------------

// Initialize the exec token for tail call support.
pub unsafe fn init_exec_token() {
    with_required_current_instance(|inst| unsafe { init_exec_token_in(inst) });
    // In the full implementation, R_PreserveObject would be called here
}

pub(crate) unsafe fn init_exec_token_in(inst: &mut RInstance) {
    unsafe {
        let sym = Rf_install_in(inst, b".__EXEC__.\x00".as_ptr() as *const c_char);
        let token = with_arena_in(inst, |arena| {
            arena.cons(sym, R_NilValue(), std::ptr::null_mut())
        });
        set_R_exec_token_in(inst, token);
    }
}

pub(crate) fn get_R_exec_token_in(inst: &mut RInstance) -> SEXP {
    inst.eval_state.exec_token
}

pub(crate) fn set_R_exec_token_in(inst: &mut RInstance, token: SEXP) {
    inst.eval_state.exec_token = token;
}

/// Check if a value is an exec continuation (for tail call optimization).
pub unsafe fn is_exec_continuation(val: SEXP) -> c_int {
    with_required_current_instance(|inst| unsafe { is_exec_continuation_in(inst, val) })
}

pub(crate) unsafe fn is_exec_continuation_in(inst: &mut RInstance, val: SEXP) -> c_int {
    unsafe {
        if val.is_null() || TYPEOF(val) != SEXPTYPE::VECSXP {
            return FALSE;
        }
        let len = crate::sexp::accessors::XLENGTH(val);
        if len != 4 {
            return FALSE;
        }
        let token = get_R_exec_token_in(inst);
        if token.is_null() {
            return FALSE;
        }
        let elt = crate::sexp::accessors::VECTOR_ELT(val, 0);
        if elt == token { TRUE } else { FALSE }
    }
}

/// Handle an exec continuation (tail call optimization).
///
/// Ported from R's `handle_exec_continuation()` in eval.c.
pub unsafe fn handle_exec_continuation(mut val: SEXP) -> SEXP {
    unsafe {
        while is_exec_continuation(val) != FALSE {
            let call = crate::sexp::accessors::VECTOR_ELT(val, 1);
            let rho = crate::sexp::accessors::VECTOR_ELT(val, 2);
            let op = crate::sexp::accessors::VECTOR_ELT(val, 3);

            if TYPEOF(op) == SEXPTYPE::CLOSXP {
                let arglist = super::dispatch::promiseArgs(CDR(call), rho);
                let _arglist_guard = protect(arglist);
                let result =
                    super::closure::applyClosure(call, op, arglist, rho, R_NilValue(), TRUE);
                val = result;
            } else {
                // For non-closures, build a call and eval
                let expr = Rf_cons(op, CDR(call));
                if !expr.is_null() {
                    (*expr).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                let _expr_guard = protect(expr);
                val = super::eval::Rf_eval(expr, rho);
            }
        }
        val
    }
}

unsafe fn tailcall_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    })
}

unsafe fn eval_exec_call(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            tailcall_error("argument \"expr\" is missing, with no default");
        }

        let mut expr = Rf_eval(CAR(args), rho);
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP && XLENGTH(expr) == 1 {
            expr = VECTOR_ELT(expr, 0);
        }
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            tailcall_error("\"expr\" must be a call expression");
        }

        let env_arg = CDR(args);
        let env = if env_arg.is_null() || env_arg == R_NilValue() || CAR(env_arg) == R_MissingArg()
        {
            rho
        } else {
            Rf_eval(CAR(env_arg), rho)
        };
        if TYPEOF(env) != SEXPTYPE::ENVSXP {
            tailcall_error("\"envir\" must be an environment");
        }

        Rf_eval(expr, env)
    }
}

unsafe fn eval_tailcall_call(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            tailcall_error("argument \"FUN\" is missing, with no default");
        }

        let expr = Rf_cons(CAR(args), CDR(args));
        if expr.is_null() {
            return R_NilValue();
        }
        (*expr).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        Rf_eval(expr, rho)
    }
}

/// `Exec()` and `Tailcall()` special forms.
///
/// GNU R can turn these calls into an exec continuation when they are in a
/// proven tail position. The Rust evaluator does not yet have the same
/// non-local jump contract, so this preserves user-visible semantics by
/// evaluating the target call directly.
pub unsafe fn do_tailcall(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let is_exec = !op.is_null()
            && crate::eval::primitive::PrimitiveDescriptor::from_raw(op)
                .map(|primitive| primitive.operation_code == 0)
                .unwrap_or_else(|| {
                    let head = CAR(call);
                    !head.is_null()
                        && TYPEOF(head) == SEXPTYPE::SYMSXP
                        && crate::sexp::symbol::symbol_name_bytes_equal(head, {
                            static EXEC: &[u8] = b"Exec\0";
                            crate::sexp::symbol::Rf_install(EXEC.as_ptr() as *const c_char)
                        })
                });

        if is_exec {
            eval_exec_call(args, rho)
        } else {
            eval_tailcall_call(args, rho)
        }
    }
}

// ---------------------------------------------------------------------------
// do_declare -- declare() special form (no-op)
// ---------------------------------------------------------------------------

/// The `declare()` special form -- currently a no-op.
///
/// Ported from R's `do_declare()` in eval.c.
// no_mangle removed (duplicate)
pub unsafe fn do_declare(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[cfg(test)]
mod tests {
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::memory::with_arena;
    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn test_jit_settings_parse_and_gate_unavailable_compiler() {
        assert!(!bytecode_compiler_available());

        let defaults = JitSettings::from_env_values(None, None, None);
        assert_eq!(defaults.jit_enabled, 0);
        assert_eq!(defaults.compile_pkgs, FALSE);
        assert_eq!(defaults.disable_bytecode, FALSE);
        assert_eq!(defaults.min_jit_score, c_int::MAX);

        let requested = JitSettings::from_env_values(Some("3"), Some("1"), Some("0"));
        assert_eq!(requested.jit_enabled, 0);
        assert_eq!(requested.compile_pkgs, TRUE);
        assert_eq!(requested.disable_bytecode, FALSE);

        let disabled = JitSettings::from_env_values(Some("3"), None, Some("1"));
        assert_eq!(disabled.jit_enabled, 0);
        assert_eq!(disabled.disable_bytecode, TRUE);

        let invalid = JitSettings::from_env_values(Some("not-a-number"), Some("-1"), None);
        assert_eq!(invalid.jit_enabled, 0);
        assert_eq!(invalid.compile_pkgs, FALSE);
    }

    #[test]
    fn test_jit_settings_are_session_local() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.with_arena(|_| {
            apply_jit_settings(JitSettings::from_env_values(
                Some("3"),
                Some("1"),
                Some("0"),
            ));
            assert_eq!(get_R_jit_enabled(), 0);
            assert_eq!(get_R_compile_pkgs(), TRUE);
            assert_eq!(get_R_disable_bytecode(), FALSE);
        })
        .unwrap();

        right
            .with_arena(|_| {
                apply_jit_settings(JitSettings::from_env_values(
                    Some("0"),
                    Some("0"),
                    Some("1"),
                ));
                assert_eq!(get_R_jit_enabled(), 0);
                assert_eq!(get_R_compile_pkgs(), FALSE);
                assert_eq!(get_R_disable_bytecode(), TRUE);
            })
            .unwrap();

        left.with_arena(|_| {
            assert_eq!(get_R_jit_enabled(), 0);
            assert_eq!(get_R_compile_pkgs(), TRUE);
            assert_eq!(get_R_disable_bytecode(), FALSE);
        })
        .unwrap();
    }

    #[test]
    fn test_jit_settings_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        apply_jit_settings_in(
            &mut left,
            JitSettings::from_env_values(Some("3"), Some("1"), Some("0")),
        );
        apply_jit_settings_in(
            &mut right,
            JitSettings::from_env_values(Some("0"), Some("0"), Some("1")),
        );

        assert_eq!(get_R_jit_enabled_in(&mut left), 0);
        assert_eq!(get_R_compile_pkgs_in(&mut left), TRUE);
        assert_eq!(get_R_disable_bytecode_in(&mut left), FALSE);
        assert_eq!(get_R_min_jit_score_in(&mut left), c_int::MAX);
        assert_eq!(get_R_loop_jit_score_in(&mut left), c_int::MAX);

        assert_eq!(get_R_jit_enabled_in(&mut right), 0);
        assert_eq!(get_R_compile_pkgs_in(&mut right), FALSE);
        assert_eq!(get_R_disable_bytecode_in(&mut right), TRUE);
        assert_eq!(get_R_min_jit_score_in(&mut right), c_int::MAX);
        assert_eq!(get_R_loop_jit_score_in(&mut right), c_int::MAX);
    }

    #[test]
    fn test_compile_expr_returns_original_expression_without_compiler() {
        let _session = RSession::new();
        let expr = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
        unsafe {
            assert_eq!(R_compileExpr(expr, R_NilValue()), expr);
        }
    }

    #[test]
    fn test_check_jit_does_not_claim_compilation_without_compiler() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_code_with_output_capture("function(x) { x + 1 }");
        let fun = result.expect("closure should evaluate").as_raw();

        unsafe {
            set_R_jit_enabled(3);
            with_required_current_instance(|inst| set_R_min_jit_score_in(inst, 0));

            assert_eq!(R_CheckJIT(fun), FALSE);
            assert_ne!(TYPEOF(BODY(fun)), SEXPTYPE::BCODESXP);
        }
    }

    #[test]
    fn test_session_jit_state_is_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.with_arena(|_| unsafe {
            set_R_jit_enabled(3);
            init_exec_token();
            let left_token = with_required_current_instance(get_R_exec_token_in);
            assert_eq!(get_R_jit_enabled(), 3);
            assert!(!left_token.is_null());
        })
        .unwrap();

        right
            .with_arena(|_| unsafe {
                assert_eq!(get_R_jit_enabled(), 0);
                assert!(with_required_current_instance(get_R_exec_token_in).is_null());
                set_R_jit_enabled(1);
                init_exec_token();
                let right_token = with_required_current_instance(get_R_exec_token_in);
                assert_eq!(get_R_jit_enabled(), 1);
                assert!(!right_token.is_null());
            })
            .unwrap();

        left.with_arena(|_| {
            assert_eq!(get_R_jit_enabled(), 3);
            assert!(!with_required_current_instance(get_R_exec_token_in).is_null());
        })
        .unwrap();
    }

    #[test]
    fn test_exec_token_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        unsafe {
            init_exec_token_in(&mut left);
            let left_token = get_R_exec_token_in(&mut left);
            assert!(!left_token.is_null());
            assert!(get_R_exec_token_in(&mut right).is_null());

            let continuation = with_arena_in(&mut left, |arena| {
                let vec = arena.alloc_vector(SEXPTYPE::VECSXP, 4);
                crate::sexp::accessors::SET_VECTOR_ELT(vec, 0, left_token);
                vec
            });

            assert_eq!(is_exec_continuation_in(&mut left, continuation), TRUE);
            assert_eq!(is_exec_continuation_in(&mut right, continuation), FALSE);
        }
    }

    #[test]
    fn exec_and_tailcall_special_forms_evaluate_target_calls() {
        let mut session = crate::android::RSession::new();

        let exec = session.eval("x <- 6\nExec(quote(identity(x)))");
        assert_eq!(exec.output, "[1] 6");

        let tailcall = session.eval("f <- function(x) Tailcall(identity, x + 1)\nf(5)");
        assert_eq!(tailcall.output, "[1] 6");
    }

    #[test]
    fn exec_and_tailcall_are_registered_as_primitives() {
        let mut session = crate::android::RSession::new();

        let exec = session.eval("is.primitive(Exec)");
        assert_eq!(exec.output, "[1] TRUE");

        let tailcall = session.eval("is.primitive(Tailcall)");
        assert_eq!(tailcall.output, "[1] TRUE");
    }
}
