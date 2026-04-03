#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Core eval() function — the heart of the R interpreter.
//!
//! This module ports R's `eval()` function from src/main/eval.c.
//! It handles expression evaluation by dispatching based on SEXPTYPE:
//! - Self-evaluating types (NILSXP, LGLSXP, INTSXP, etc.) → return as-is
//! - SYMSXP → variable lookup via R_findVar
//! - PROMSXP → force the promise
//! - LANGSXP → function call (dispatch to SPECIAL/BUILTIN/CLOSXP)
//! - BCODESXP → bytecode evaluation

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::mainutils::errors::R_MissingArgError;
use crate::sexp::accessors::{CADDDR, CADDR, CAR, CDDDR, CDR, PRIMOFFSET, SET_NAMED, TYPEOF};
use crate::sexp::envir::{R_findVar, findFun, forcePromise};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{
    R_EvalDepth, R_GlobalEnv, R_MissingArg, R_NilValue, R_UnboundValue, set_R_Visible,
};
use crate::sexp::memory_ext::vmaxget;
use crate::sexp::protect::Rf_protect;
use crate::sexp::symbol::R_DotsSymbol;

// ---------------------------------------------------------------------------
// Primitive function dispatch
// ---------------------------------------------------------------------------

/// Function pointer type for primitive functions (SPECIAL and BUILTIN).
pub type PRIMFUN = unsafe extern "C" fn(
    SEXP, // call
    SEXP, // op (the function)
    SEXP, // args
    SEXP, // rho (environment)
) -> SEXP;

/// Get the primitive function pointer for a SPECIAL or BUILTIN.
pub unsafe fn get_primfun(op: SEXP) -> Option<PRIMFUN> {
    unsafe {
        if op.is_null() {
            return None;
        }
        let t = TYPEOF(op);
        if t != SEXPTYPE::SPECIALSXP.0 && t != SEXPTYPE::BUILTINSXP.0 {
            return None;
        }
        let offset = PRIMOFFSET(op);
        if offset < 0 {
            return None;
        }
        // Look up in the function table
        get_fun_tab_entry(offset)
    }
}

/// Get a function table entry by offset.
///
/// This is a stub — the full implementation would use R_FunTab.
pub unsafe fn get_fun_tab_entry(offset: c_int) -> Option<PRIMFUN> {
    let _ = offset;
    None
}

/// Check the PRIMPRINT flag (visibility hint for primitives).
pub unsafe fn PRIMPRINT(op: SEXP) -> c_int {
    unsafe {
        if op.is_null() {
            return 0;
        }
        let t = TYPEOF(op);
        if t != SEXPTYPE::SPECIALSXP.0 && t != SEXPTYPE::BUILTINSXP.0 {
            return 0;
        }
        // PRIMPRINT is stored in gp bits
        // bit 0: visible (1 = invisible, 0 = visible)
        // For now, default to visible
        0
    }
}

/// Get the PRIMNAME for a primitive.
pub unsafe fn PRIMNAME(op: SEXP) -> &'static str {
    "unknown"
}

// ---------------------------------------------------------------------------
// The core eval() function
// ---------------------------------------------------------------------------

/// Evaluate an R expression in an environment.
///
/// This is the equivalent of R's `eval()` from src/main/eval.c.
/// It is the main dispatch function of the interpreter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_eval(e: SEXP, rho: SEXP) -> SEXP {
    unsafe { eval_inner(e, rho) }
}

/// Internal eval implementation.
pub unsafe fn eval_inner(e: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if e.is_null() {
            return R_NilValue();
        }

        // Set visible flag
        set_R_Visible(TRUE);

        // Self-evaluating types — return immediately
        let t = TYPEOF(e);
        match t {
        0 | // NILSXP
        2 | // LISTSXP
        10 | // LGLSXP
        13 | // INTSXP
        14 | // REALSXP
        16 | // STRSXP
        15 | // CPLXSXP
        24 | // RAWSXP
        25 | // OBJSXP
        7  | // SPECIALSXP
        8  | // BUILTINSXP
        4  | // ENVSXP
        3  | // CLOSXP
        19 | // VECSXP
        20 | // EXPRSXP
        22 | // EXTPTRSXP
        23 => // WEAKREFSXP
            return e,
        _ => {}
    }

        // Check evaluation depth
        let depth = R_EvalDepth() + 1;
        if depth > 500 {
            eprintln!(
                "Error: evaluation nested too deeply: infinite recursion / options(expressions=)?"
            );
            std::panic::panic_any(crate::sexp::context::RError {
                message: "evaluation nested too deeply".to_string(),
            });
        }
        crate::sexp::globals::set_R_EvalDepth(depth);

        let result = match t {
            // Symbol lookup
            1 => {
                // SYMSXP
                if e == R_DotsSymbol() {
                    eprintln!("Error: '...' used in an incorrect context");
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: "'...' used in an incorrect context".to_string(),
                    });
                }

                let tmp = R_findVar(e, rho);
                if tmp == R_UnboundValue() {
                    // Object not found
                    let pname = crate::sexp::accessors::PRINTNAME(e);
                    let name = if !pname.is_null() {
                        let s = crate::sexp::accessors::CHAR(pname);
                        if !s.is_null() {
                            std::ffi::CStr::from_ptr(s)
                                .to_str()
                                .unwrap_or("???")
                                .to_string()
                        } else {
                            "???".to_string()
                        }
                    } else {
                        "???".to_string()
                    };
                    eprintln!("Error: object '{}' not found", name);
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: format!("object '{}' not found", name),
                    });
                } else if tmp == R_MissingArg() {
                    R_MissingArgError(e, ptr::null_mut(), std::ptr::null::<c_char>());
                    R_NilValue() // unreachable
                } else if TYPEOF(tmp) == SEXPTYPE::PROMSXP.0 {
                    forcePromise(tmp)
                } else {
                    tmp
                }
            }

            // Promise — force it
            5 => {
                // PROMSXP
                forcePromise(e)
            }

            // Language (function call)
            6 => {
                // LANGSXP
                eval_lang(e, rho)
            }

            // Bytecode
            21 => {
                // BCODESXP
                // bcEval(e, rho) — stub for now
                eprintln!("Warning: bytecode evaluation not yet implemented");
                R_NilValue()
            }

            // DOTSXP in wrong context
            17 => {
                // DOTSXP
                eprintln!("Error: '...' used in an incorrect context");
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "'...' used in an incorrect context".to_string(),
                });
            }

            _ => {
                eprintln!("Error: unimplemented type in eval: {}", t);
                R_NilValue()
            }
        };

        // Restore depth
        crate::sexp::globals::set_R_EvalDepth(depth - 1);

        result
    }
}

// ---------------------------------------------------------------------------
// eval_lang — evaluate a language/function call
// ---------------------------------------------------------------------------

/// Evaluate a LANGSXP (function call expression).
unsafe fn eval_lang(e: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = CAR(e);
        let args = CDR(e);

        // Find the function
        let op = if TYPEOF(fun) == SEXPTYPE::SYMSXP.0 {
            findFun(fun, rho)
        } else {
            Rf_eval(fun, rho)
        };

        if op == R_UnboundValue() {
            let name = if TYPEOF(fun) == SEXPTYPE::SYMSXP.0 {
                let pname = crate::sexp::accessors::PRINTNAME(fun);
                if !pname.is_null() {
                    let s = crate::sexp::accessors::CHAR(pname);
                    if !s.is_null() {
                        std::ffi::CStr::from_ptr(s)
                            .to_str()
                            .unwrap_or("???")
                            .to_string()
                    } else {
                        "???".to_string()
                    }
                } else {
                    "???".to_string()
                }
            } else {
                "???".to_string()
            };
            eprintln!("Error: could not find function \"{}\"", name);
            std::panic::panic_any(crate::sexp::context::RError {
                message: format!("could not find function \"{}\"", name),
            });
        }

        Rf_protect(op);

        let result = match TYPEOF(op) {
            // Special — arguments not evaluated
            7 => {
                // SPECIALSXP
                let _vmax = vmaxget();
                Rf_protect(e);
                let flag = PRIMPRINT(op);
                set_R_Visible(if flag != 1 { TRUE } else { FALSE });

                let tmp = if let Some(primfun) = get_primfun(op) {
                    primfun(e, op, args, rho)
                } else {
                    // Fallback: call do_special_dispatch
                    super::special::do_special_dispatch(e, op, args, rho)
                };

                if flag < 2 {
                    set_R_Visible(if flag != 1 { TRUE } else { FALSE });
                }
                tmp
            }

            // Builtin — arguments evaluated first
            8 => {
                // BUILTINSXP
                let _vmax = vmaxget();
                Rf_protect(e);

                // Evaluate arguments
                let evaled_args = super::dispatch::evalList(args, rho, e, 0);
                Rf_protect(evaled_args);

                let flag = PRIMPRINT(op);
                set_R_Visible(if flag != 1 { TRUE } else { FALSE });

                let tmp = if let Some(primfun) = get_primfun(op) {
                    primfun(e, op, evaled_args, rho)
                } else {
                    eprintln!("Warning: builtin function not implemented");
                    R_NilValue()
                };

                if flag < 2 {
                    set_R_Visible(if flag != 1 { TRUE } else { FALSE });
                }
                tmp
            }

            // Closure — full function call
            3 => {
                // CLOSXP
                let pargs = super::dispatch::promiseArgs(args, rho);
                Rf_protect(pargs);
                super::closure::applyClosure(e, op, pargs, rho, R_NilValue(), TRUE)
            }

            _ => {
                eprintln!("Error: attempt to apply non-function");
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "attempt to apply non-function".to_string(),
                });
            }
        };

        result
    }
}

// ---------------------------------------------------------------------------
// eval with visibility preservation (for C code calling eval)
// ---------------------------------------------------------------------------

/// Evaluate an expression, preserving the R_Visible flag.
///
/// This is the equivalent of R's `evalKeepVis()` from errors.c.
pub unsafe fn evalKeepVis(e: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let oldvis = crate::sexp::globals::R_Visible();
        let val = Rf_eval(e, rho);
        set_R_Visible(oldvis);
        val
    }
}
