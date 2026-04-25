#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Global singleton R objects.
//!
//! These are the immutable sentinel values that R's interpreter uses
//! everywhere. Mutable runtime environments are session-owned and reached
//! through the active `RInstance`, not through process-global fallback slots.

use std::ptr;
use std::sync::OnceLock;

use super::ffi::{SEXP, SEXPTYPE, SexprecCore, SexprecData, SxpInfo};

// ---------------------------------------------------------------------------
// Sentinel singletons (storing as usize to avoid Send requirement)
// ---------------------------------------------------------------------------

/// R_NilValue: the global nil/NULL object.
static NIL_VALUE: OnceLock<usize> = OnceLock::new();

/// R_UnboundValue: sentinel for unbound symbols in environments.
static UNBOUND_VALUE: OnceLock<usize> = OnceLock::new();

/// R_MissingArg: sentinel for missing function arguments.
static MISSING_ARG: OnceLock<usize> = OnceLock::new();

/// R_RestartToken: sentinel for restart tokens.
static RESTART_TOKEN: OnceLock<usize> = OnceLock::new();

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

fn init_nil() -> SEXP {
    *NIL_VALUE.get_or_init(|| {
        Box::into_raw(Box::new(SexprecCore {
            sxpinfo: SxpInfo::new(SEXPTYPE::NILSXP),
            attrib: ptr::null_mut(),
            gengc_next_node: ptr::null_mut(),
            gengc_prev_node: ptr::null_mut(),
            data: SexprecData::default(),
        })) as usize
    }) as SEXP
}

fn init_unbound() -> SEXP {
    *UNBOUND_VALUE.get_or_init(|| {
        let mut info = SxpInfo::new(SEXPTYPE::SYMSXP);
        info.set_mark(true);
        Box::into_raw(Box::new(SexprecCore {
            sxpinfo: info,
            attrib: ptr::null_mut(),
            gengc_next_node: ptr::null_mut(),
            gengc_prev_node: ptr::null_mut(),
            data: SexprecData::default(),
        })) as usize
    }) as SEXP
}

fn init_missing() -> SEXP {
    *MISSING_ARG.get_or_init(|| {
        let mut info = SxpInfo::new(SEXPTYPE::SYMSXP);
        info.set_mark(true);
        Box::into_raw(Box::new(SexprecCore {
            sxpinfo: info,
            attrib: ptr::null_mut(),
            gengc_next_node: ptr::null_mut(),
            gengc_prev_node: ptr::null_mut(),
            data: SexprecData::default(),
        })) as usize
    }) as SEXP
}

fn init_restart() -> SEXP {
    *RESTART_TOKEN.get_or_init(|| {
        let mut info = SxpInfo::new(SEXPTYPE::SPECIALSXP);
        info.set_mark(true);
        Box::into_raw(Box::new(SexprecCore {
            sxpinfo: info,
            attrib: ptr::null_mut(),
            gengc_next_node: ptr::null_mut(),
            gengc_prev_node: ptr::null_mut(),
            data: SexprecData::default(),
        })) as usize
    }) as SEXP
}

// ---------------------------------------------------------------------------
// Accessor functions for global singletons
// ---------------------------------------------------------------------------

/// Get a pointer to R_NilValue.
pub unsafe fn R_NilValue() -> SEXP {
    init_nil()
}

/// Get a pointer to R_UnboundValue.
pub unsafe fn R_UnboundValue() -> SEXP {
    init_unbound()
}

/// Get a pointer to R_MissingArg.
pub unsafe fn R_MissingArg() -> SEXP {
    init_missing()
}

/// Get a pointer to R_RestartToken.
pub unsafe fn R_RestartToken() -> SEXP {
    init_restart()
}

// ---------------------------------------------------------------------------
// Global environment accessor functions
// ---------------------------------------------------------------------------

pub unsafe fn R_GlobalEnv() -> SEXP {
    super::instance::with_required_current_instance(|inst| inst.global_env)
}

pub unsafe fn R_BaseEnv() -> SEXP {
    super::instance::with_required_current_instance(|inst| inst.base_env)
}

pub unsafe fn R_EmptyEnv() -> SEXP {
    super::instance::with_required_current_instance(|inst| inst.empty_env)
}

/// Set the global environment.
pub unsafe fn set_R_GlobalEnv(env: SEXP) {
    super::instance::with_required_current_instance(|inst| inst.global_env = env);
}

/// Set the base environment.
pub unsafe fn set_R_BaseEnv(env: SEXP) {
    super::instance::with_required_current_instance(|inst| inst.base_env = env);
}

/// Set the empty environment.
pub unsafe fn set_R_EmptyEnv(env: SEXP) {
    super::instance::with_required_current_instance(|inst| inst.empty_env = env);
}

// ---------------------------------------------------------------------------
// NA helpers
// ---------------------------------------------------------------------------

/// Check if a logical value is NA.
#[inline]
pub fn LOGICAL_IS_NA(x: i32) -> bool {
    x == super::ffi::NA_INTEGER
}

/// Check if an integer value is NA.
#[inline]
pub fn INTEGER_IS_NA(x: i32) -> bool {
    x == super::ffi::NA_INTEGER
}

// ---------------------------------------------------------------------------
// Evaluator globals
// ---------------------------------------------------------------------------

/// Get the current R_Visible flag.
pub fn R_Visible() -> i32 {
    super::instance::with_required_current_instance(|inst| inst.eval_state.visible)
}

/// Set the R_Visible flag.
pub fn set_R_Visible(v: i32) {
    super::instance::with_required_current_instance(|inst| inst.eval_state.visible = v);
}

/// Get the current evaluation depth.
pub fn R_EvalDepth() -> i32 {
    super::instance::with_required_current_instance(|inst| inst.eval_state.eval_depth)
}

/// Set the evaluation depth.
pub fn set_R_EvalDepth(d: i32) {
    super::instance::with_required_current_instance(|inst| inst.eval_state.eval_depth = d);
}

/// Get the evaluation depth limit.
pub fn R_EvalDepthLimit() -> i32 {
    super::instance::with_required_current_instance(|inst| inst.eval_state.eval_depth_limit)
}

// ---------------------------------------------------------------------------
// Common symbols (re-exports from symbol.rs for convenience)
// ---------------------------------------------------------------------------

/// Get R_DotsSymbol (the "..." symbol).
pub unsafe fn R_DotsSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_DotsSymbol() }
}

/// Get R_IfSymbol (the "if" symbol).
pub unsafe fn R_IfSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_IfSymbol() }
}

/// Get R_WhileSymbol (the "while" symbol).
pub unsafe fn R_WhileSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_WhileSymbol() }
}

/// Get R_ForSymbol (the "for" symbol).
pub unsafe fn R_ForSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_ForSymbol() }
}

/// Get R_RepeatSymbol (the "repeat" symbol).
pub unsafe fn R_RepeatSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_RepeatSymbol() }
}

/// Get R_BraceSymbol (the "{" symbol).
pub unsafe fn R_BraceSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_BraceSymbol() }
}

// ---------------------------------------------------------------------------
// R_True and R_False logical singletons
// ---------------------------------------------------------------------------

/// R_True: the logical TRUE singleton (scalar LGLSXP with value 1).
static R_TRUE: OnceLock<usize> = OnceLock::new();

/// R_False: the logical FALSE singleton (scalar LGLSXP with value 0).
static R_FALSE: OnceLock<usize> = OnceLock::new();

/// Get a pointer to R_True (logical scalar TRUE).
pub unsafe fn R_True() -> SEXP {
    *R_TRUE.get_or_init(|| {
        let mut node = SexprecCore::new_vector(SEXPTYPE::LGLSXP, 1);
        node.sxpinfo.set_scalar(true);
        Box::into_raw(Box::new(node)) as usize
    }) as SEXP
}

/// Get a pointer to R_False (logical scalar FALSE).
pub unsafe fn R_False() -> SEXP {
    *R_FALSE.get_or_init(|| {
        let mut node = SexprecCore::new_vector(SEXPTYPE::LGLSXP, 1);
        node.sxpinfo.set_scalar(true);
        Box::into_raw(Box::new(node)) as usize
    }) as SEXP
}

// ---------------------------------------------------------------------------
// NA_STRING sentinel
// ---------------------------------------------------------------------------

/// R_NaString: the NA_STRING sentinel (a special CHARSXP with NA bit in gp).
static NA_STRING_PTR: OnceLock<usize> = OnceLock::new();

/// Get a pointer to R's NA_STRING — a CHARSXP sentinel representing NA.
///
/// In R, `NA_character_` is a STRSXP of length 1 whose sole element is this
/// sentinel. The sentinel itself has type CHARSXP and the NA bit set in its
/// gp field.
pub unsafe fn R_NaString() -> SEXP {
    *NA_STRING_PTR.get_or_init(|| {
        let mut info = SxpInfo::new(SEXPTYPE::CHARSXP);
        // Set gp=1 to mark as NA (R's convention for NA_STRING)
        info.set_gp(1);
        Box::into_raw(Box::new(SexprecCore {
            sxpinfo: info,
            attrib: ptr::null_mut(),
            gengc_next_node: ptr::null_mut(),
            gengc_prev_node: ptr::null_mut(),
            data: SexprecData::default(),
        })) as usize
    }) as SEXP
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::ffi::*;
    use super::*;

    #[test]
    fn test_r_nilvalue_type() {
        unsafe {
            let nil = R_NilValue();
            assert!(!nil.is_null());
            assert_eq!((*nil).sxpinfo.type_of(), SEXPTYPE::NILSXP);
        }
    }

    #[test]
    fn test_r_nilvalue_stable() {
        unsafe {
            let nil1 = R_NilValue();
            let nil2 = R_NilValue();
            assert_eq!(nil1, nil2);
        }
    }

    #[test]
    fn test_r_unboundvalue_type() {
        unsafe {
            let ub = R_UnboundValue();
            assert!(!ub.is_null());
            assert_eq!((*ub).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
            assert!((*ub).sxpinfo.mark());
        }
    }

    #[test]
    fn test_r_missingarg_type() {
        unsafe {
            let ma = R_MissingArg();
            assert!(!ma.is_null());
            assert_eq!((*ma).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
        }
    }

    #[test]
    fn test_logical_is_na() {
        assert!(LOGICAL_IS_NA(NA_INTEGER));
        assert!(!LOGICAL_IS_NA(0));
        assert!(!LOGICAL_IS_NA(1));
    }

    #[test]
    fn test_integer_is_na() {
        assert!(INTEGER_IS_NA(NA_INTEGER));
        assert!(!INTEGER_IS_NA(0));
        assert!(!INTEGER_IS_NA(42));
    }

    #[test]
    fn test_set_global_env() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let saved = R_GlobalEnv();
            let fake = 0x1 as SEXP;
            set_R_GlobalEnv(fake);
            assert_eq!(R_GlobalEnv(), fake);
            set_R_GlobalEnv(saved);
        }
    }
}
