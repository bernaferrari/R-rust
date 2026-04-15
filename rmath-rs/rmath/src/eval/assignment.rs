#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Assignment operations — ports R's assignment handling from eval.c.
//!
//! Handles `<-`, `<<-`, and `=` assignment operators.

use crate::sexp::accessors::ENCLOS;
use crate::sexp::accessors::{CADR, CAR, CDDR, CDR, STRING_ELT, TYPEOF};
use crate::sexp::envir::{defineVar, setVar};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, set_R_Visible};
use crate::sexp::protect::Rf_protect;

use super::eval::Rf_eval;

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RSignal::Error {
        message: msg.to_string(),
    })
}

// ---------------------------------------------------------------------------
// do_set — handle assignment operators (<-, <<-, =)
// ---------------------------------------------------------------------------

/// Handle assignment: lhs <- rhs, lhs <<- rhs, lhs = rhs.
/// Matches C's `do_set()` in eval.c line 3565.
///
/// `op` carries PRIMVAL: 1 or 3 for `<-`/`=`, 2 for `<<-`.
pub unsafe fn do_set(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args == R_NilValue() || CDR(args) == R_NilValue() || CDDR(args) != R_NilValue() {
            error("wrong argument count for assignment");
        }

        let lhs = CAR(args);
        let primval = crate::mainutils::relop::PRIMVAL(op);

        match TYPEOF(lhs) {
            t if t == SEXPTYPE::STRSXP => {
                let sym = crate::mainutils::subset::installTrChar(STRING_ELT(lhs, 0));
                let rhs = Rf_eval(CADR(args), rho);
                Rf_protect(rhs);
                assign_to_symbol(sym, rhs, primval, rho);
                Rf_protect(rhs);
                rhs
            }
            t if t == SEXPTYPE::SYMSXP => {
                let rhs = Rf_eval(CADR(args), rho);
                Rf_protect(rhs);
                assign_to_symbol(lhs, rhs, primval, rho);
                rhs
            }
            t if t == SEXPTYPE::LANGSXP => {
                set_R_Visible(FALSE);
                return applydefine(call, op, args, rho);
            }
            _ => {
                error("invalid (do_set) left-hand side to assignment");
            }
        }
    }
}

unsafe fn assign_to_symbol(sym: SEXP, value: SEXP, primval: i32, rho: SEXP) {
    unsafe {
        if primval == 2 {
            setVar(sym, value, ENCLOS(rho));
        } else {
            defineVar(sym, value, rho);
        }
        set_R_Visible(FALSE);
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

/// Handle complex/subscript assignment (e.g., x[1] <- 5, x$name <- val,
/// x[i][j] <- val for nested cases).
///
/// Uses `evalseq` from missing.rs to recursively evaluate the LHS chain
/// for nested assignments, then walks back up applying replacement functions.
///
/// Ported from applydefine() in eval.c:3367.
pub unsafe fn applydefine(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            error("invalid complex assignment");
        }

        let rhs = Rf_eval(CADR(args), rho);
        Rf_protect(rhs);

        let primval = crate::mainutils::relop::PRIMVAL(op);
        let forcelocal = if primval == 1 || primval == 3 { 1 } else { 0 };

        // Check if this is a nested assignment: CADR(expr) is LANGSXP
        if TYPEOF(CADR(expr)) == SEXPTYPE::LANGSXP {
            // Nested assignment: use evalseq to evaluate LHS chain
            let lhs = crate::eval::missing::evalseq(CADR(expr), rho, forcelocal);
            Rf_protect(lhs);

            // Walk the chain applying replacement functions
            let mut current_lhs = lhs;
            let mut current_expr = CADR(expr);
            let mut current_rhs = rhs;

            // Process inner levels (nested [i], [j], etc.)
            while TYPEOF(current_expr) == SEXPTYPE::LANGSXP
                && TYPEOF(CADR(current_expr)) == SEXPTYPE::LANGSXP
            {
                let func_sym = CAR(current_expr);
                let assign_fn = get_assign_fcn_sym(func_sym);
                if assign_fn == R_NilValue() || TYPEOF(assign_fn) != SEXPTYPE::SYMSXP {
                    break;
                }

                let target_val = CAR(current_lhs);
                let rest_args = CDDR(current_expr);

                let arg_list = crate::sexp::constructors::Rf_cons(
                    target_val,
                    crate::sexp::constructors::Rf_cons(current_rhs, rest_args),
                );
                Rf_protect(arg_list);
                let repl_call = crate::sexp::constructors::Rf_cons(assign_fn, arg_list);
                Rf_protect(repl_call);

                current_rhs = Rf_eval(repl_call, rho);
                Rf_protect(current_rhs);

                crate::sexp::protect::Rf_unprotect(2);
                current_lhs = CDR(current_lhs);
                current_expr = CADR(current_expr);
            }

            // Final (outermost) level
            let func_sym = CAR(expr);
            let assign_fn = get_assign_fcn_sym(func_sym);
            if assign_fn != R_NilValue() && TYPEOF(assign_fn) == SEXPTYPE::SYMSXP {
                let target_val = CAR(current_lhs);
                let rest_args = CDDR(expr);

                let arg_list = crate::sexp::constructors::Rf_cons(
                    target_val,
                    crate::sexp::constructors::Rf_cons(current_rhs, rest_args),
                );
                Rf_protect(arg_list);
                let repl_call = crate::sexp::constructors::Rf_cons(assign_fn, arg_list);
                Rf_protect(repl_call);

                let result = Rf_eval(repl_call, rho);
                Rf_protect(result);

                // Assign back to the outermost variable
                let var_sym = CADR(expr);
                if TYPEOF(var_sym) == SEXPTYPE::SYMSXP {
                    // Use the symbol from the deepest evalseq level
                    let deep_sym =
                        if !CDR(current_lhs).is_null() && CDR(current_lhs) != R_NilValue() {
                            CDR(current_lhs)
                        } else {
                            var_sym
                        };
                    if primval == 2 {
                        setVar(deep_sym, result, ENCLOS(rho));
                    } else {
                        defineVar(deep_sym, result, rho);
                    }
                }

                crate::sexp::protect::Rf_unprotect(3);
            }

            crate::sexp::protect::Rf_unprotect(2);
            set_R_Visible(FALSE);
            rhs
        } else {
            // Simple single-level assignment: x[i] <- val
            let lhs = expr;
            let func_sym = CAR(lhs);

            let assign_fn = if TYPEOF(func_sym) == SEXPTYPE::SYMSXP {
                get_assign_fcn_sym(func_sym)
            } else {
                R_NilValue()
            };

            if assign_fn == R_NilValue() || TYPEOF(assign_fn) != SEXPTYPE::SYMSXP {
                crate::sexp::protect::Rf_unprotect(1);
                return rhs;
            }

            let target_expr = Rf_eval(CADR(lhs), rho);
            Rf_protect(target_expr);

            let call_args = CDDR(lhs);
            let arg_list = crate::sexp::constructors::Rf_cons(
                target_expr,
                crate::sexp::constructors::Rf_cons(rhs, call_args),
            );
            Rf_protect(arg_list);
            let repl_call = crate::sexp::constructors::Rf_cons(assign_fn, arg_list);
            Rf_protect(repl_call);

            let result = Rf_eval(repl_call, rho);
            Rf_protect(result);

            let var_sym = CADR(lhs);
            if TYPEOF(var_sym) == SEXPTYPE::SYMSXP {
                if primval == 2 {
                    setVar(var_sym, result, ENCLOS(rho));
                } else {
                    defineVar(var_sym, result, rho);
                }
            }

            set_R_Visible(FALSE);
            crate::sexp::protect::Rf_unprotect(5);
            rhs
        }
    }
}

/// Convert a function symbol to its assignment variant: `[` -> `[<-`, `$` -> `$<-`.
fn get_assign_fcn_sym(sym: SEXP) -> SEXP {
    unsafe {
        let name = crate::sexp::accessors::CHAR(crate::sexp::accessors::PRINTNAME(sym));
        if name.is_null() {
            return R_NilValue();
        }
        let s = std::ffi::CStr::from_ptr(name).to_str().unwrap_or("");
        let assign_name = format!("{}<-", s);
        let c_name = std::ffi::CString::new(assign_name).unwrap_or_default();
        crate::sexp::symbol::Rf_install(c_name.as_ptr())
    }
}
