#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Special form implementations — ports R's special functions from eval.c.
//!
//! Special forms are functions where arguments are NOT pre-evaluated.
//! This includes: if, while, for, repeat, break, next, return, function, begin, (.

use crate::sexp::accessors::{
    CADDR, CADR, CAR, CDDR, CDR, CHAR, COMPLEX_ELT, INTEGER_ELT, LOGICAL_ELT, PRINTNAME, RAW_ELT,
    REAL_ELT, SET_STRING_ELT, SET_VECTOR_ELT, SETCDR, STRING_ELT, TAG, TYPEOF, VECTOR_ELT,
};
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
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
            "switch" => crate::mainutils::builtin::do_switch(call, op, args, rho),
            "call" => crate::mainutils::coerce::do_call(call, op, args, rho),
            "quote" => crate::mainutils::essentials::do_quote(call, op, args, rho),
            ".Internal" => crate::mainutils::names::do_internal(call, op, args, rho),
            ".Primitive" => crate::mainutils::names::do_primitive(call, op, args, rho),
            "expression" => do_expression(CDR(call)),
            "substitute" => crate::mainutils::coerce::do_substitute(call, op, args, rho),
            "invisible" => do_invisible(CDR(call), rho),
            "Exec" | "Tailcall" => crate::eval::jit::do_tailcall(call, op, args, rho),
            "on.exit" => do_on_exit_from_args(CDR(call), rho),
            "=" | "<-" | "<<-" => super::assignment::do_set(call, op, CDR(call), rho),
            "~" => crate::mainutils::names::do_tilde(call, op, args, rho),
            "$" => crate::mainutils::subset::do_subset3(call, op, args, rho),
            "@" => crate::mainutils::essentials::do_at(call, op, args, rho),
            "@<-" => crate::mainutils::essentials::do_at_set(call, op, args, rho),
            "$<-" => crate::mainutils::essentials::do_dollar_set(call, op, args, rho),
            _ => unimplemented_special_form(name),
        }
    }
}

unsafe fn do_expression(args: SEXP) -> SEXP {
    unsafe {
        let mut len = 0;
        let mut current = args;
        while !is_null(current) {
            len += 1;
            current = CDR(current);
        }

        let result = Rf_allocVector3(SEXPTYPE::EXPRSXP.as_c_int(), len);
        let _result_guard = protect(result);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), len);
        let _names_guard = protect(names);

        current = args;
        let mut i = 0;
        let mut has_names = false;
        while !is_null(current) {
            SET_VECTOR_ELT(result, i, CAR(current));
            let name = tag_name(current).unwrap_or_default();
            if !name.is_empty() {
                has_names = true;
            }
            SET_STRING_ELT(
                names,
                i,
                Rf_mkChar(std::ffi::CString::new(name).unwrap_or_default().as_ptr()),
            );
            i += 1;
            current = CDR(current);
        }

        if has_names {
            crate::eval::attrib_core::setAttrib(
                result,
                crate::eval::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

fn unimplemented_special_form(name: &str) -> ! {
    std::panic::panic_any(RError {
        message: format!("unimplemented special form '{name}'"),
    });
}

unsafe fn is_null(x: SEXP) -> bool {
    unsafe { x.is_null() || crate::sexp::accessors::Rf_isNull(x) != 0 }
}

unsafe fn tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(tag);
        if printname.is_null() || printname == R_NilValue() {
            return None;
        }
        let chars = CHAR(printname);
        if chars.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(chars)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

unsafe fn list_append(s: SEXP, t: SEXP) -> SEXP {
    unsafe {
        if is_null(s) {
            return t;
        }
        let mut r = s;
        while !is_null(CDR(r)) {
            r = CDR(r);
        }
        SETCDR(r, t);
        s
    }
}

unsafe fn find_function_context(rho: SEXP) -> *mut crate::sexp::context::RCNTXT {
    unsafe {
        let mut ctxt = super::runtime::global_context();
        while !ctxt.is_null()
            && !((*ctxt).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0
                && (*ctxt).cloenv == rho)
        {
            ctxt = (*ctxt).nextcontext;
        }
        ctxt
    }
}

/// Implement `on.exit(expr, add = FALSE, after = TRUE)`.
pub(crate) unsafe fn do_on_exit_from_args(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut expr = R_NilValue();
        let mut add_expr = R_NilValue();
        let mut after_expr = R_NilValue();
        let mut positional = 0usize;
        let mut current = args;
        while !is_null(current) {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some("expr") => expr = value,
                Some("add") => add_expr = value,
                Some("after") | Some("lifo") => after_expr = value,
                _ => {
                    match positional {
                        0 => expr = value,
                        1 => add_expr = value,
                        2 => after_expr = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let add = if is_null(add_expr) {
            FALSE
        } else {
            let value = crate::mainutils::coerce::asLogical(Rf_eval(add_expr, rho));
            if value == NA_INTEGER {
                std::panic::panic_any(RError {
                    message: "invalid 'add' argument".to_string(),
                });
            }
            value
        };
        let after = if is_null(after_expr) {
            TRUE
        } else {
            let value = crate::mainutils::coerce::asLogical(Rf_eval(after_expr, rho));
            if value == NA_INTEGER {
                std::panic::panic_any(RError {
                    message: "invalid 'after' argument".to_string(),
                });
            }
            value
        };

        let ctxt = find_function_context(rho);
        if !ctxt.is_null() {
            if is_null(expr) && add == FALSE {
                (*ctxt).conexit = R_NilValue();
            } else {
                let old = (*ctxt).conexit;
                if is_null(old) || add == FALSE {
                    (*ctxt).conexit = Rf_cons(expr, R_NilValue());
                } else if after != FALSE {
                    let copied = crate::mainutils::duplicate::shallow_duplicate(old);
                    (*ctxt).conexit = list_append(copied, Rf_cons(expr, R_NilValue()));
                } else {
                    (*ctxt).conexit = Rf_cons(expr, old);
                }
            }
        }
        super::runtime::set_visible(FALSE);
        R_NilValue()
    }
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
        super::runtime::set_visible(FALSE);
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
        let _cond_guard = protect(cond);
        let _body_guard = protect(body);

        crate::sexp::context::run_hoisted_loop(|| {
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

                Rf_eval(body, rho);
                crate::sexp::gengc::maybe_collect_at_eval_safe_point();
            }
        });

        crate::sexp::gengc::maybe_collect_at_eval_safe_point();
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
        let _var_guard = protect(var_sym);
        let _body_guard = protect(body);

        let seq_val = Rf_eval(seq_expr, rho);
        let _seq_guard = protect(seq_val);

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

        let n = crate::sexp::constructors::Rf_length(seq_val) as usize;
        let val_type = TYPEOF(seq_val);
        let i = std::cell::Cell::new(0usize);
        let list_cell_addr = std::cell::Cell::new(seq_val as usize);

        crate::sexp::context::run_hoisted_loop_with_continue(
            || {
                while i.get() < n {
                    crate::sexp::instance::check_cancellation();
                    let idx = i.get();

                    let val = match val_type {
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            VECTOR_ELT(seq_val, idx as i64)
                        }
                        t if t == SEXPTYPE::LISTSXP => {
                            let list_cell = list_cell_addr.get() as SEXP;
                            let v = CAR(list_cell);
                            list_cell_addr.set(CDR(list_cell) as usize);
                            v
                        }
                        t if t == SEXPTYPE::LGLSXP => {
                            Rf_ScalarLogical(LOGICAL_ELT(seq_val, idx as i32))
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            Rf_ScalarInteger(INTEGER_ELT(seq_val, idx as i32))
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            Rf_ScalarReal(REAL_ELT(seq_val, idx as i32))
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            Rf_ScalarComplex(COMPLEX_ELT(seq_val, idx as i32))
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            let scalar = Rf_allocVector(SEXPTYPE::STRSXP, 1);
                            SET_STRING_ELT(scalar, 0, STRING_ELT(seq_val, idx as i64));
                            scalar
                        }
                        t if t == SEXPTYPE::RAWSXP => Rf_ScalarRaw(RAW_ELT(seq_val, idx as i32)),
                        _ => {
                            std::panic::panic_any(crate::sexp::context::RSignal::Error {
                                message: "invalid for() loop sequence".to_string(),
                            });
                        }
                    };

                    if !crate::sexp::envir::define_var_updates(var_sym, val, rho) {
                        std::panic::panic_any(crate::sexp::context::RSignal::Error {
                            message: "failed to set `for` loop variable".to_string(),
                        });
                    }

                    Rf_eval(body, rho);
                    crate::sexp::gengc::maybe_collect_at_eval_safe_point();
                    i.set(idx + 1);
                }
            },
            || {
                i.set(i.get() + 1);
            },
        );

        crate::sexp::gengc::maybe_collect_at_eval_safe_point();
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
        let _body_guard = protect(body);

        // `repeat` re-enters after each successful body evaluation, so it cannot
        // share the single-setjmp structure used by `for`/`while`. A nested
        // `loop {}` inside `run_hoisted_loop` would never return on success and
        // would allocate until OOM.
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

            crate::sexp::gengc::maybe_collect_at_eval_safe_point();
        }

        crate::sexp::gengc::maybe_collect_at_eval_safe_point();
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
        crate::sexp::gengc::maybe_collect_at_eval_safe_point();
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
