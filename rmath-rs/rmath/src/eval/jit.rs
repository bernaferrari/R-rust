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

use std::cell::{Cell, RefCell};
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_SrcRefSymbol, getAttrib};
use crate::sexp::accessors::{BODY, CAR, CDR, CHAR, LENGTH, PRINTNAME, STRING_ELT, TYPEOF};
use crate::sexp::constructors::Rf_cons;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::Rf_protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// JIT state (static globals)
// ---------------------------------------------------------------------------

// Minimum score for JIT compilation.
thread_local! { static MIN_JIT_SCORE: Cell<c_int> = Cell::new(50); }

// Loop JIT score threshold.
thread_local! { static LOOP_JIT_SCORE: Cell<c_int> = Cell::new(50); }

// Whether JIT is enabled (0 = disabled, 3 = default enabled).
thread_local! { static R_jit_enabled: Cell<c_int> = Cell::new(0); }

// Whether to compile package code.
thread_local! { static R_compile_pkgs: Cell<c_int> = Cell::new(0); }

// Whether bytecode is disabled.
thread_local! { static R_disable_bytecode: Cell<c_int> = Cell::new(0); }

// Constant checking level (0 = no checking, default).
thread_local! { static R_check_constants: Cell<c_int> = Cell::new(0); }

/// JIT statistics.
#[derive(Default)]
struct JitInfo {
    count: u64,
    envcount: u64,
    bdcount: u64,
}

thread_local! { static jit_info: RefCell<JitInfo> = RefCell::new(JitInfo::default()); }

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

        // Try to compile via the compiler namespace
        // In the full implementation, this calls cmpfun() from the compiler package
        // For now, we leave functions uncompiled
    }
}

/// Compile an expression to bytecode.
///
/// Ported from R's `R_compileExpr()` in eval.c.
pub unsafe fn R_compileExpr(_expr: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Stub: JIT compilation not yet implemented
        R_NilValue()
    }
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
unsafe fn checkCompilerOptions(_jitEnabled: c_int) {
    // Stub: not yet implemented
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
        // Default: JIT enabled at level 3
        let mut val: c_int = 3;

        // Check R_ENABLE_JIT environment variable
        if let Ok(enable) = std::env::var("R_ENABLE_JIT")
            && let Ok(v) = enable.parse::<c_int>()
        {
            val = v;
        }

        if val != 0 {
            // loadCompilerNamespace(); // stub: not yet implemented
            checkCompilerOptions(val);
        }
        R_jit_enabled.with(|v| v.set(val));

        // Check _R_COMPILE_PKGS_
        if let Ok(compile) = std::env::var("_R_COMPILE_PKGS_")
            && let Ok(v) = compile.parse::<c_int>()
        {
            R_compile_pkgs.with(|cell| cell.set(if v > 0 { TRUE } else { FALSE }));
        }

        // Check R_DISABLE_BYTECODE
        if let Ok(disable) = std::env::var("R_DISABLE_BYTECODE")
            && let Ok(v) = disable.parse::<c_int>()
        {
            R_disable_bytecode.with(|cell| cell.set(if v > 0 { TRUE } else { FALSE }));
        }
    }
}

/// Check if a function should be JIT-compiled.
///
/// Ported from R's `R_CheckJIT()` in eval.c. Returns TRUE if the
/// function should be compiled based on JIT settings and scoring.
pub unsafe fn R_CheckJIT(op: SEXP) -> c_int {
    unsafe {
        if R_jit_enabled.with(|v| v.get()) == 0 || R_disable_bytecode.with(|v| v.get()) != 0 {
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
        if score >= MIN_JIT_SCORE.with(|v| v.get()) {
            R_cmpfun(op);
            TRUE
        } else {
            FALSE
        }
    }
}

/// Get whether JIT is enabled.
pub unsafe fn get_R_jit_enabled() -> c_int {
    R_jit_enabled.with(|v| v.get())
}

/// Set whether JIT is enabled.
pub unsafe fn set_R_jit_enabled(val: c_int) {
    R_jit_enabled.with(|v| v.set(val));
}

/// Get whether to compile packages.
pub unsafe fn get_R_compile_pkgs() -> c_int {
    R_compile_pkgs.with(|v| v.get())
}

/// Get whether bytecode is disabled.
pub unsafe fn get_R_disable_bytecode() -> c_int {
    R_disable_bytecode.with(|v| v.get())
}

/// Get the constant checking level.
pub unsafe fn get_R_check_constants() -> c_int {
    R_check_constants.with(|v| v.get())
}

// ---------------------------------------------------------------------------
// R_exec_token -- for tail call optimization
// ---------------------------------------------------------------------------

// Token used for tail call (Exec) optimization.
thread_local! { static R_exec_token: Cell<SEXP> = Cell::new(ptr::null_mut()); }

// Initialize the exec token for tail call support.
pub unsafe fn init_exec_token() {
    unsafe {
        let sym = Rf_install(b".__EXEC__.\x00".as_ptr() as *const c_char);
        let token = Rf_cons(sym, R_NilValue());
        R_exec_token.with(|v| v.set(token));
    }
    // In the full implementation, R_PreserveObject would be called here
}

/// Check if a value is an exec continuation (for tail call optimization).
pub unsafe fn is_exec_continuation(val: SEXP) -> c_int {
    unsafe {
        if val.is_null() || TYPEOF(val) != SEXPTYPE::VECSXP {
            return FALSE;
        }
        let len = crate::sexp::accessors::XLENGTH(val);
        if len != 4 {
            return FALSE;
        }
        if R_exec_token.with(|v| v.get()).is_null() {
            return FALSE;
        }
        let elt = crate::sexp::accessors::VECTOR_ELT(val, 0);
        let token = R_exec_token.with(|v| v.get());
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
                Rf_protect(arglist);
                let result =
                    super::closure::applyClosure(call, op, arglist, rho, R_NilValue(), TRUE);
                val = result;
            } else {
                // For non-closures, build a call and eval
                let expr = Rf_cons(op, CDR(call));
                if !expr.is_null() {
                    (*expr).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                val = super::eval::Rf_eval(expr, rho);
            }
        }
        val
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
