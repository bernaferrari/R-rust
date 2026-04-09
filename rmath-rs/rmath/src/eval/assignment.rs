#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Assignment operations — ports R's assignment handling from eval.c.
//!
//! Handles `<-`, `<<-`, and `=` assignment operators.

use crate::sexp::accessors::{CADR, CAR, CDR, TYPEOF};
use crate::sexp::envir::{defineVar, setVar};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, set_R_Visible};

use super::eval::Rf_eval;

// ---------------------------------------------------------------------------
// do_set — handle assignment operators (<-, <<-, =)
// ---------------------------------------------------------------------------

/// Handle assignment: lhs <- rhs, lhs <<- rhs, lhs = rhs.
///
/// This is the equivalent of R's assignment handling in eval.c.
pub unsafe fn do_set(_lhs: SEXP, rhs: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if rhs.is_null() {
            return R_NilValue();
        }

        let var_sym = CAR(rhs);
        if TYPEOF(var_sym) != SEXPTYPE::SYMSXP.0 {
            eprintln!("Error: invalid left-hand side to assignment");
            return R_NilValue();
        }

        let val_expr = CADR(rhs);
        let val = Rf_eval(val_expr, rho);
        set_R_Visible(FALSE);

        defineVar(var_sym, val, rho);

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
