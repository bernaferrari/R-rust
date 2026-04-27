#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Special form implementations — ports R's special functions from eval.c.
//!
//! Special forms are functions where arguments are NOT pre-evaluated.
//! This includes: if, while, for, repeat, break, next, return, function, begin, (.

use crate::sexp::accessors::{CADDR, CADR, CAR, CDDR, CDR, TYPEOF};
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, set_R_Visible};
use crate::sexp::symbol::R_BraceSymbol;

use super::eval::Rf_eval;

// ---------------------------------------------------------------------------
// Special form dispatch
// ---------------------------------------------------------------------------

/// Dispatch a special form call.
///
/// This is called by eval_lang when the function is a SPECIALSXP.
pub unsafe fn do_special_dispatch(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let offset = crate::sexp::accessors::PRIMOFFSET(op);

        // Match by symbol name
        let fun_sym = CAR(call);
        if TYPEOF(fun_sym) == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(fun_sym);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    let name = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                    return dispatch_special_by_name(name, call, op, args, rho);
                }
            }
        }

        unimplemented_special_form("<unknown>")
    }
}

/// Dispatch special forms by name.
unsafe fn dispatch_special_by_name(
    name: &str,
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        match name {
            "{" => do_begin(CDR(call), rho),
            "(" => do_paren(CDR(call), rho),
            "if" => do_if(CDR(call), rho),
            "while" => do_while(CDR(call), rho),
            "for" => do_for(CDR(call), rho),
            "repeat" => do_repeat(CDR(call), rho),
            "break" => do_break(),
            "next" => do_next(),
            "function" => do_function(CDR(call), rho),
            "return" => do_return(CDR(call), rho),
            "invisible" => do_invisible(CDR(call), rho),
            "=" | "<-" | "<<-" => super::assignment::do_set(call, op, CDR(call), rho),
            "$" => crate::mainutils::subset::do_subset3(call, op, args, rho),
            _ => unimplemented_special_form(name),
        }
    }
}

fn unimplemented_special_form(name: &str) -> ! {
    std::panic::panic_any(RError {
        message: format!("unimplemented special form '{name}'"),
    });
}

/// Implement `invisible(x)` when it is reached through the special-form path.
unsafe fn do_invisible(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = if args.is_null() || args == R_NilValue() {
            R_NilValue()
        } else {
            CAR(args)
        };
        let value = Rf_eval(expr, rho);
        set_R_Visible(FALSE);
        value
    }
}

// ---------------------------------------------------------------------------
// do_if — the if/else special form
// ---------------------------------------------------------------------------

/// Implement the `if` special form.
unsafe fn do_if(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // args = (condition, true_branch, [false_branch])
        let cond = CAR(args);
        let true_branch = CADR(args);
        let false_branch = if CDDR(args) != R_NilValue() {
            CADDR(args)
        } else {
            R_NilValue()
        };

        let cond_val = Rf_eval(cond, rho);

        // Check condition (logical vector, take first element)
        let result = if TYPEOF(cond_val) == SEXPTYPE::LGLSXP {
            let data = crate::sexp::accessors::LOGICAL(cond_val);
            if data.is_null() {
                eprintln!("Error: missing value where TRUE/FALSE needed");
                std::panic::panic_any(RError {
                    message: "missing value where TRUE/FALSE needed".to_string(),
                });
            }
            let v = *data;
            if v == 1 {
                // TRUE
                Rf_eval(true_branch, rho)
            } else if v == 0 {
                // FALSE
                if false_branch == R_NilValue() {
                    R_NilValue()
                } else {
                    Rf_eval(false_branch, rho)
                }
            } else {
                // NA_LOGICAL
                eprintln!("Error: missing value where TRUE/FALSE needed");
                std::panic::panic_any(RError {
                    message: "missing value where TRUE/FALSE needed".to_string(),
                });
            }
        } else {
            eprintln!("Error: argument is not interpretable as logical");
            std::panic::panic_any(RError {
                message: "argument is not interpretable as logical".to_string(),
            });
        };

        result
    }
}

// ---------------------------------------------------------------------------
// do_while — the while loop special form
// ---------------------------------------------------------------------------

/// Implement the `while` special form.
unsafe fn do_while(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let cond = CAR(args);
        let body = CADR(args);

        loop {
            crate::sexp::instance::check_cancellation();
            let cond_val = Rf_eval(cond, rho);

            let should_continue = if TYPEOF(cond_val) == SEXPTYPE::LGLSXP {
                let data = crate::sexp::accessors::LOGICAL(cond_val);
                *data == 1
            } else {
                let len = crate::sexp::constructors::Rf_length(cond_val);
                if len > 0 {
                    true
                } else {
                    std::panic::panic_any(crate::sexp::context::RSignal::Error {
                        message: "argument is not interpretable as logical".to_string(),
                    });
                }
            };

            if !should_continue {
                break;
            }

            let body_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Rf_eval(body, rho)));

            match body_result {
                Ok(_) => {}
                Err(payload) => match crate::sexp::context::handle_loop_signal(payload) {
                    crate::sexp::context::LoopAction::Break => break,
                    crate::sexp::context::LoopAction::Continue => continue,
                },
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_for — the for loop special form
// ---------------------------------------------------------------------------

/// Implement the `for` special form.
unsafe fn do_for(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let var_sym = CAR(args);
        let seq_expr = CADR(args);
        let body = CADDR(args);

        let seq_val = Rf_eval(seq_expr, rho);

        if TYPEOF(seq_val) != SEXPTYPE::VECSXP
            && TYPEOF(seq_val) != SEXPTYPE::LISTSXP
            && TYPEOF(seq_val) != SEXPTYPE::LANGSXP
            && TYPEOF(seq_val) != SEXPTYPE::EXPRSXP
            && TYPEOF(seq_val) != SEXPTYPE::LGLSXP
            && TYPEOF(seq_val) != SEXPTYPE::INTSXP
            && TYPEOF(seq_val) != SEXPTYPE::REALSXP
            && TYPEOF(seq_val) != SEXPTYPE::CPLXSXP
            && TYPEOF(seq_val) != SEXPTYPE::STRSXP
            && TYPEOF(seq_val) != SEXPTYPE::RAWSXP
        {
            std::panic::panic_any(crate::sexp::context::RSignal::Error {
                message: "invalid 'for' loop variable sequence".to_string(),
            });
        }

        let n = crate::sexp::constructors::Rf_length(seq_val);

        for i in 0..n {
            crate::sexp::instance::check_cancellation();
            let val = if TYPEOF(seq_val) == SEXPTYPE::VECSXP || TYPEOF(seq_val) == SEXPTYPE::EXPRSXP
            {
                crate::sexp::accessors::VECTOR_ELT(seq_val, i as i64)
            } else {
                let mut current = seq_val;
                for _ in 0..i {
                    current = CDR(current);
                }
                CAR(current)
            };

            defineVar(var_sym, val, rho);

            let body_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Rf_eval(body, rho)));

            match body_result {
                Ok(_) => {}
                Err(payload) => match crate::sexp::context::handle_loop_signal(payload) {
                    crate::sexp::context::LoopAction::Break => break,
                    crate::sexp::context::LoopAction::Continue => continue,
                },
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_repeat — the repeat loop special form
// ---------------------------------------------------------------------------

/// Implement the `repeat` special form.
unsafe fn do_repeat(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let body = CAR(args);

        loop {
            crate::sexp::instance::check_cancellation();
            let body_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Rf_eval(body, rho)));

            match body_result {
                Ok(_) => {}
                Err(payload) => match crate::sexp::context::handle_loop_signal(payload) {
                    crate::sexp::context::LoopAction::Break => break,
                    crate::sexp::context::LoopAction::Continue => continue,
                },
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_break — the break statement
// ---------------------------------------------------------------------------

/// Implement the `break` statement.
///
/// In C, this uses longjmp. In Rust, we panic with a Break signal.
pub unsafe fn do_break() -> SEXP {
    std::panic::panic_any(crate::sexp::context::RSignal::Break);
}

// ---------------------------------------------------------------------------
// do_next — the next statement
// ---------------------------------------------------------------------------

/// Implement the `next` statement.
///
/// In C, this uses longjmp. In Rust, we panic with a Next signal.
pub unsafe fn do_next() -> SEXP {
    std::panic::panic_any(crate::sexp::context::RSignal::Next);
}

// ---------------------------------------------------------------------------
// do_function — the function constructor
// ---------------------------------------------------------------------------

/// Implement the `function` special form.
///
/// Creates a closure (CLOSXP) from formals and body.
unsafe fn do_function(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let formals = CAR(args);
        let body = CDR(args);

        // Create a CLOSXP
        let clos = crate::sexp::memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::CLOSXP));
        if !clos.is_null() {
            (*clos).data.closxp.formals = formals;
            (*clos).data.closxp.body = if CDR(body) == R_NilValue() {
                CAR(body)
            } else {
                // Multiple expressions — wrap in { }
                let begin = Rf_cons(R_BraceSymbol(), body);
                if !begin.is_null() {
                    (*begin).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                begin
            };
            (*clos).data.closxp.env = rho;
        }

        clos
    }
}

// ---------------------------------------------------------------------------
// do_begin — the { } compound expression
// ---------------------------------------------------------------------------

/// Implement the `{` special form (begin/compound expression).
///
/// Evaluates each expression in sequence, returning the last.
pub unsafe fn do_begin(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        let mut result = R_NilValue();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            result = Rf_eval(CAR(current), rho);
            current = CDR(current);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_paren — the ( ) grouping expression
// ---------------------------------------------------------------------------

/// Implement the `(` special form (parenthesized expression).
unsafe fn do_paren(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }
        Rf_eval(CAR(args), rho)
    }
}

// ---------------------------------------------------------------------------
// do_return — the return statement
// ---------------------------------------------------------------------------

/// Implement the `return` special form.
unsafe fn do_return(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let val = if args.is_null() || args == R_NilValue() {
            R_NilValue()
        } else {
            Rf_eval(CAR(args), rho)
        };
        std::panic::panic_any(crate::sexp::context::RSignal::Return(val));
    }
}
