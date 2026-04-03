#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Assignment operations — ports R's assignment handling from eval.c.
//!
//! Handles `<-`, `<<-`, and `=` assignment operators.

use std::ptr;

use crate::sexp::accessors::{
    CADDDR, CADDR, CADR, CAR, CDDDR, CDR, Rf_isNull, SETCAR, TAG, TYPEOF,
};
use crate::sexp::constructors::*;
use crate::sexp::envir::{R_findVarInFrame, defineVar, setVar};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, set_R_Visible};
use crate::sexp::memory_ext::mkPROMISE;
use crate::sexp::symbol::Rf_install;

use super::eval::Rf_eval;

// ---------------------------------------------------------------------------
// do_set — handle assignment operators (<-, <<-, =)
// ---------------------------------------------------------------------------

/// Handle assignment: lhs <- rhs, lhs <<- rhs, lhs = rhs.
///
/// This is the equivalent of R's assignment handling in eval.c.
pub unsafe fn do_set(lhs: SEXP, rhs: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if lhs.is_null() || rhs.is_null() {
            return R_NilValue();
        }

        let fun_sym = CAR(lhs);
        let args = CDR(lhs);

        // Get the variable name being assigned to
        let var_sym = if TYPEOF(fun_sym) == SEXPTYPE::SYMSXP.0 {
            fun_sym
        } else {
            eprintln!("Error: invalid (do_set) left-hand side to assignment");
            std::panic::panic_any(crate::sexp::context::RError {
                message: "invalid left-hand side to assignment".to_string(),
            });
        };

        // Evaluate the right-hand side
        let val = Rf_eval(rhs, rho);
        set_R_Visible(FALSE);

        // For simple assignment, define in the current environment
        // Check if it's <- or <<-
        let assign_name = crate::sexp::accessors::PRINTNAME(fun_sym);
        let name_str = if !assign_name.is_null() {
            let s = crate::sexp::accessors::CHAR(assign_name);
            if !s.is_null() {
                std::ffi::CStr::from_ptr(s).to_str().unwrap_or("")
            } else {
                ""
            }
        } else {
            ""
        };

        if name_str == "<<-" {
            // Global assignment — search parent environments
            setVar(var_sym, val, rho);
        } else {
            // Local assignment (<- or =)
            defineVar(var_sym, val, rho);
        }

        val
    }
}

// ---------------------------------------------------------------------------
// evalseq — evaluate a sequence with assignment
// ---------------------------------------------------------------------------

/// Evaluate a sequence of expressions (for use in multi-expression bodies).
///
/// This is the equivalent of R's `evalseq()` in eval.c.
pub unsafe fn evalseq(expr: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let mut result = R_NilValue();
        let mut current = expr;
        while !current.is_null() && current != R_NilValue() {
            result = Rf_eval(CAR(current), rho);
            current = CDR(current);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// applydefine — handle complex assignment (a[b] <- value)
// ---------------------------------------------------------------------------

/// Handle complex/subscript assignment (e.g., x[1] <- 5).
///
/// This is the equivalent of R's `applydefine()` in eval.c.
pub unsafe fn applydefine(call: SEXP, lhs: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // For now, just evaluate the RHS and return it
        // Full implementation requires subscript.c
        let val = if args.is_null() || args == R_NilValue() {
            R_NilValue()
        } else {
            Rf_eval(CAR(args), rho)
        };
        val
    }
}
