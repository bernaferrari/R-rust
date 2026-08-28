#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Assignment operations — ports R's assignment handling from eval.c.
//!
//! Handles `<-`, `<<-`, and `=` assignment operators.

use crate::sexp::accessors::ENCLOS;
use crate::sexp::accessors::{
    CADR, CAR, CDDR, CDR, CHAR, INTEGER_ELT, LOGICAL_ELT, PRINTNAME, REAL_ELT, SET_INTEGER_ELT,
    SET_LOGICAL_ELT, SET_REAL_ELT, SETTAG, STRING_ELT, TAG, TYPEOF, XLENGTH,
};
use crate::sexp::envir::Environment;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::Sexp;
use crate::sexp::protect::protect;

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
                let _rhs_guard = protect(rhs);
                assign_to_symbol(sym, rhs, primval, rho);
                rhs
            }
            t if t == SEXPTYPE::SYMSXP => {
                let rhs = Rf_eval(CADR(args), rho);
                let _rhs_guard = protect(rhs);
                assign_to_symbol(lhs, rhs, primval, rho);
                rhs
            }
            t if t == SEXPTYPE::LANGSXP => {
                super::runtime::set_visible(FALSE);
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
        bind_assignment(sym, value, primval, rho);
        super::runtime::set_visible(FALSE);
    }
}

fn bind_assignment(sym: SEXP, value: SEXP, primval: i32, rho: SEXP) {
    let target_env = if primval == 2 {
        unsafe { ENCLOS(rho) }
    } else {
        rho
    };
    let (Some(sym), Some(value), Some(target_env)) = (
        Sexp::from_raw(sym),
        Sexp::from_raw(value),
        Sexp::from_raw(target_env),
    ) else {
        return;
    };
    let Ok(env) = Environment::new(target_env) else {
        return;
    };

    if primval == 2 {
        env.set(sym, value);
    } else if let Err(err) = env.define(sym, value) {
        error(&format!("failed to assign binding: {err}"));
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
        let _rhs_guard = protect(rhs);

        let primval = crate::mainutils::relop::PRIMVAL(op);
        let forcelocal = if primval == 1 || primval == 3 { 1 } else { 0 };

        // Complex assignment: the LHS is a chain of calls, e.g.
        // `ll$a[1] <- 9` is `[<-`(`$<-`(ll, a), 1, 9).
        if TYPEOF(CADR(expr)) == SEXPTYPE::LANGSXP {
            // Collect the replacement levels from the outermost call down
            // to the innermost (assign_fn, subscript args) pairs, and the
            // chain of intermediate values from evalseq: the chain is
            // (v_innermost_call . ... (v_first_call . base_symbol)).
            let mut levels: Vec<(SEXP, SEXP)> = Vec::new();
            let mut tmp = expr;
            while TYPEOF(tmp) == SEXPTYPE::LANGSXP {
                let assign_fn = get_assign_fcn_sym(CAR(tmp));
                if assign_fn == R_NilValue() || TYPEOF(assign_fn) != SEXPTYPE::SYMSXP {
                    levels.clear();
                    break;
                }
                levels.push((assign_fn, CDDR(tmp)));
                tmp = CADR(tmp);
            }

            if !levels.is_empty() {
                let chain = crate::eval::missing::evalseq(CADR(expr), rho, forcelocal);
                let _chain_guard = protect(chain);

                // Evaluate each level's subscript arguments once, keeping
                // R_MissingArg slots (e.g. `m[1,]` in `m[1,][2] <- 9`).
                // Slot replacements (`$<-`, `@<-`) receive their name
                // symbols raw, like the simple-assignment path.
                let mut evaluated_levels: Vec<(SEXP, SEXP)> = Vec::new();
                for (assign_fn, rest_args) in &levels {
                    let slot_level = matches!(
                        symbol_name(*assign_fn).as_deref(),
                        Some("$<-") | Some("@<-")
                    );
                    let evaled = if slot_level {
                        *rest_args
                    } else {
                        super::dispatch::evalListKeepMissing(*rest_args, rho)
                    };
                    let _evaled_guard = protect(evaled);
                    evaluated_levels.push((*assign_fn, evaled));
                }

                // Apply the replacement calls outside-in: level k receives
                // the value of its own base expression (the next chain cell)
                // and the running value, exactly like upstream applydefine.
                let mut current_rhs = rhs;
                let _current_rhs_guard = protect(current_rhs);
                let mut cell = chain;
                for (assign_fn, rest_args) in &evaluated_levels {
                    let target_val = CAR(cell);
                    let _target_guard = protect(target_val);

                    // `e$x[1] <<- v`: assigning a slot of an environment
                    // defines the binding directly, like the simple path.
                    current_rhs = if symbol_name(*assign_fn).as_deref() == Some("$<-")
                        && TYPEOF(target_val) == SEXPTYPE::ENVSXP
                        && let Some(field_sym) = dollar_field_symbol(CAR(*rest_args))
                    {
                        crate::sexp::envir::defineVar(field_sym, current_rhs, target_val);
                        target_val
                    } else {
                        let arg_list = build_replacement_args(target_val, *rest_args, current_rhs);
                        let _arg_list_guard = protect(arg_list);
                        let repl_call = crate::sexp::constructors::Rf_cons(*assign_fn, arg_list);
                        if !repl_call.is_null() {
                            (*repl_call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        let _repl_call_guard = protect(repl_call);
                        apply_replacement_call(*assign_fn, repl_call, arg_list, rho)
                    };
                    protect(current_rhs);
                    // Step one chain cell inward for the next level's base.
                    let next = CDR(cell);
                    if !next.is_null() && next != R_NilValue() && TYPEOF(next) == SEXPTYPE::LISTSXP
                    {
                        cell = next;
                    }
                }

                // The deepest chain tail is the variable being assigned.
                let mut var_cell = cell;
                loop {
                    let next = CDR(var_cell);
                    if !next.is_null() && next != R_NilValue() && TYPEOF(next) == SEXPTYPE::LISTSXP
                    {
                        var_cell = next;
                    } else {
                        break;
                    }
                }
                let var_sym = CDR(var_cell);
                if !var_sym.is_null() && TYPEOF(var_sym) == SEXPTYPE::SYMSXP {
                    bind_assignment(var_sym, current_rhs, primval, rho);
                }
            }

            super::runtime::set_visible(FALSE);
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
                return rhs;
            }

            let target_expr = Rf_eval(CADR(lhs), rho);
            let _target_guard = protect(target_expr);

            if symbol_name(func_sym).as_deref() == Some("$")
                && TYPEOF(target_expr) == SEXPTYPE::ENVSXP
                && let Some(field_sym) = dollar_field_symbol(CAR(CDDR(lhs)))
            {
                crate::sexp::envir::defineVar(field_sym, rhs, target_expr);
                super::runtime::set_visible(FALSE);
                return rhs;
            }

            let call_args = CDDR(lhs);
            let slot_subs;
            let raw_subscript = matches!(symbol_name(func_sym).as_deref(), Some("@") | Some("$"));
            let evaluated_subs = if raw_subscript {
                call_args
            } else {
                // `[<-` / `[[<-` subscript slots may be empty (`m[1,] <- v`),
                // which must pass through as R_MissingArg the way upstream's
                // subset/assign handlers expect (evalListKeepMissing).
                slot_subs = super::dispatch::evalListKeepMissing(call_args, rho);
                let _subs_guard = protect(slot_subs);
                slot_subs
            };

            let result = if let Some(result) =
                try_simple_vector_subassign(target_expr, evaluated_subs, rhs)
            {
                result
            } else {
                let arg_list = build_replacement_args(target_expr, evaluated_subs, rhs);
                let _arg_list_guard = protect(arg_list);
                let repl_call = crate::sexp::constructors::Rf_cons(assign_fn, arg_list);
                if !repl_call.is_null() {
                    (*repl_call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                let _repl_call_guard = protect(repl_call);

                apply_replacement_call(assign_fn, repl_call, arg_list, rho)
            };
            let _result_guard = protect(result);

            let var_sym = CADR(lhs);
            if TYPEOF(var_sym) == SEXPTYPE::SYMSXP {
                bind_assignment(var_sym, result, primval, rho);
            }

            super::runtime::set_visible(FALSE);
            rhs
        }
    }
}

unsafe fn symbol_name(symbol: SEXP) -> Option<String> {
    unsafe {
        if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(symbol);
        if printname.is_null() {
            return None;
        }
        let chars = CHAR(printname);
        if chars.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .ok()
            .map(str::to_owned)
    }
}

unsafe fn dollar_field_symbol(field: SEXP) -> Option<SEXP> {
    unsafe {
        if field.is_null() || field == R_NilValue() {
            return None;
        }
        if TYPEOF(field) == SEXPTYPE::SYMSXP {
            return Some(field);
        }
        if TYPEOF(field) == SEXPTYPE::STRSXP && XLENGTH(field) > 0 {
            return Some(crate::mainutils::subset::installTrChar(STRING_ELT(
                field, 0,
            )));
        }
        None
    }
}

unsafe fn build_replacement_args(target: SEXP, subs: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let mut tail = crate::sexp::constructors::Rf_cons(value, R_NilValue());
        let mut guards = vec![protect(tail)];
        let mut sub_args = Vec::new();
        let mut current = subs;
        while current != R_NilValue() && !current.is_null() {
            sub_args.push((CAR(current), TAG(current)));
            current = CDR(current);
        }
        for (arg, tag) in sub_args.into_iter().rev() {
            let cell = crate::sexp::constructors::Rf_cons(arg, tail);
            if !tag.is_null() {
                SETTAG(cell, tag);
            }
            tail = cell;
            guards.push(protect(tail));
        }
        crate::sexp::constructors::Rf_cons(target, tail)
    }
}

unsafe fn apply_replacement_call(assign_fn: SEXP, call: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name = crate::sexp::accessors::CHAR(crate::sexp::accessors::PRINTNAME(assign_fn));
        if name.is_null() {
            return Rf_eval(call, rho);
        }

        let Ok(name) = std::ffi::CStr::from_ptr(name).to_str() else {
            return Rf_eval(call, rho);
        };

        match name {
            "[<-" => crate::mainutils::subassign::do_subassign_dflt(call, assign_fn, args, rho),
            "[[<-" => crate::mainutils::subassign::do_subassign2_dflt(call, assign_fn, args, rho),
            "$<-" => crate::mainutils::essentials::do_dollar_set(call, assign_fn, args, rho),
            "@<-" => crate::mainutils::essentials::do_at_set(call, assign_fn, args, rho),
            "names<-" => crate::mainutils::essentials::do_names_set(call, assign_fn, args, rho),
            "dim<-" => crate::mainutils::essentials::do_dim_set(call, assign_fn, args, rho),
            "tsp<-" => crate::mainutils::essentials::do_tsp_set(call, assign_fn, args, rho),
            "length<-" => crate::mainutils::essentials::do_length_set(call, assign_fn, args, rho),
            "levels<-" => crate::mainutils::essentials::do_levels_set(call, assign_fn, args, rho),
            "storage.mode<-" => {
                crate::mainutils::essentials::do_storage_mode_set(call, assign_fn, args, rho)
            }
            "dimnames<-" => {
                crate::mainutils::essentials::do_dimnames_set(call, assign_fn, args, rho)
            }
            "rownames<-" => {
                crate::mainutils::essentials::do_rownames_set(call, assign_fn, args, rho)
            }
            "colnames<-" => {
                crate::mainutils::essentials::do_colnames_set(call, assign_fn, args, rho)
            }
            _ => Rf_eval(call, rho),
        }
    }
}

unsafe fn try_simple_vector_subassign(target: SEXP, subs: SEXP, value: SEXP) -> Option<SEXP> {
    unsafe {
        if subs == R_NilValue() || subs.is_null() || CDR(subs) != R_NilValue() {
            return None;
        }
        if XLENGTH(value) < 1 {
            return None;
        }

        let index = scalar_positive_index(CAR(subs))?;
        if index >= XLENGTH(target) {
            return None;
        }

        match TYPEOF(target) {
            t if t == SEXPTYPE::REALSXP => {
                let replacement = match TYPEOF(value) {
                    vt if vt == SEXPTYPE::REALSXP => REAL_ELT(value, 0),
                    vt if vt == SEXPTYPE::INTSXP || vt == SEXPTYPE::LGLSXP => {
                        let v = INTEGER_ELT(value, 0);
                        if v == crate::sexp::ffi::NA_INTEGER {
                            crate::sexp::ffi::NA_REAL
                        } else {
                            v as f64
                        }
                    }
                    _ => return None,
                };
                SET_REAL_ELT(target, index as i32, replacement);
                Some(target)
            }
            t if t == SEXPTYPE::INTSXP => {
                let replacement = match TYPEOF(value) {
                    vt if vt == SEXPTYPE::INTSXP || vt == SEXPTYPE::LGLSXP => INTEGER_ELT(value, 0),
                    _ => return None,
                };
                SET_INTEGER_ELT(target, index as i32, replacement);
                Some(target)
            }
            t if t == SEXPTYPE::LGLSXP => {
                let replacement = match TYPEOF(value) {
                    vt if vt == SEXPTYPE::LGLSXP => LOGICAL_ELT(value, 0),
                    _ => return None,
                };
                SET_LOGICAL_ELT(target, index as i32, replacement);
                Some(target)
            }
            _ => None,
        }
    }
}

unsafe fn scalar_positive_index(index: SEXP) -> Option<crate::sexp::ffi::R_xlen_t> {
    unsafe {
        if index.is_null() || index == R_NilValue() || XLENGTH(index) != 1 {
            return None;
        }
        let raw = match TYPEOF(index) {
            t if t == SEXPTYPE::INTSXP => INTEGER_ELT(index, 0) as crate::sexp::ffi::R_xlen_t,
            t if t == SEXPTYPE::REALSXP => {
                let value = REAL_ELT(index, 0);
                if !value.is_finite() || value.fract() != 0.0 {
                    return None;
                }
                value as crate::sexp::ffi::R_xlen_t
            }
            _ => return None,
        };
        if raw < 1 { None } else { Some(raw - 1) }
    }
}

/// Convert a function symbol to its assignment variant: `[` -> `[<-`, `$` -> `$<-`.
fn get_assign_fcn_sym(sym: SEXP) -> SEXP {
    unsafe {
        let name = crate::sexp::accessors::CHAR(crate::sexp::accessors::PRINTNAME(sym));
        if name.is_null() {
            return R_NilValue();
        }
        let Ok(s) = std::ffi::CStr::from_ptr(name).to_str() else {
            return R_NilValue();
        };
        let assign_name = format!("{}<-", s);
        let c_name = std::ffi::CString::new(assign_name)
            .expect("assignment symbol derived from a CStr cannot contain NUL");
        crate::sexp::symbol::Rf_install(c_name.as_ptr())
    }
}
