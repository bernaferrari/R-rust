#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Assignment operations — ports R's assignment handling from eval.c.
//!
//! Handles `<-`, `<<-`, and `=` assignment operators.

use crate::sexp::accessors::ENCLOS;
use crate::sexp::accessors::{
    CADR, CADDR, CAR, CDDR, CDR, CHAR, INTEGER_ELT, LOGICAL_ELT, NAMED, PRINTNAME, REAL_ELT,
    SET_INTEGER_ELT, SET_LOGICAL_ELT, SET_REAL_ELT, SETTAG, STRING_ELT, TAG, TYPEOF, XLENGTH,
};

use crate::sexp::envir::Environment;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::Sexp;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;


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

        if TYPEOF(CADR(expr)) == SEXPTYPE::LANGSXP {
            // GNU applydefine: evalseq the first argument, then
            // `*tmp*` replacement calls while the base is still a call.
            let tmp_sym = Rf_install(c"*tmp*".as_ptr());
            let saved_tmp = crate::sexp::envir::R_findVarInFrame(rho, tmp_sym);
            let _saved_tmp = protect(saved_tmp);
            struct RestoreTmp {
                rho: SEXP,
                tmp_sym: SEXP,
                saved: SEXP,
            }
            impl Drop for RestoreTmp {
                fn drop(&mut self) {
                    unsafe {
                        if !self.saved.is_null()
                            && self.saved != R_NilValue()
                            && self.saved != crate::sexp::globals::R_UnboundValue()
                        {
                            crate::sexp::envir::defineVar(self.tmp_sym, self.saved, self.rho);
                        } else {
                            crate::sexp::envir::remove_binding_raw(self.rho, self.tmp_sym);
                        }
                    }
                }
            }
            let _restore_tmp = RestoreTmp {
                rho,
                tmp_sym,
                saved: saved_tmp,
            };
            let mut lhs_expr = expr;
            let mut chain = crate::eval::missing::evalseq(CADR(lhs_expr), rho, forcelocal);
            let _chain_guard = protect(chain);
            let mut current_rhs = rhs;
            while TYPEOF(CADR(lhs_expr)) == SEXPTYPE::LANGSXP {
                let assign_fn = replacement_fun_head(CAR(lhs_expr));
                if assign_fn == R_NilValue() {
                    break;
                }
                crate::sexp::envir::defineVar(tmp_sym, CAR(chain), rho);
                let repl = replace_tmp_call(assign_fn, tmp_sym, CDDR(lhs_expr), current_rhs);
                let _repl = protect(repl);
                current_rhs = crate::eval::eval::Rf_eval(repl, rho);
                let _ = protect(current_rhs);
                let next = CDR(chain);
                if !next.is_null() && next != R_NilValue() && TYPEOF(next) == SEXPTYPE::LISTSXP {
                    chain = next;
                }
                lhs_expr = CADR(lhs_expr);
            }
            if TYPEOF(lhs_expr) == SEXPTYPE::LANGSXP {
                let assign_fn = replacement_fun_head(CAR(lhs_expr));
                if assign_fn != R_NilValue() {
                    crate::sexp::envir::defineVar(tmp_sym, CAR(chain), rho);
                    let repl = replace_tmp_call(assign_fn, tmp_sym, CDDR(lhs_expr), current_rhs);
                    let _repl = protect(repl);
                    current_rhs = crate::eval::eval::Rf_eval(repl, rho);
                    let _ = protect(current_rhs);
                }
            }

            let var_sym = CADR(lhs_expr);
            if !var_sym.is_null() && TYPEOF(var_sym) == SEXPTYPE::SYMSXP {
                bind_assignment(var_sym, current_rhs, primval, rho);
            }

            super::runtime::set_visible(FALSE);
            rhs

        } else {
            // Simple single-level assignment: x[i] <- val
            let lhs = expr;
            let func_sym = CAR(lhs);


            let assign_fn = replacement_fun_head(func_sym);

            if assign_fn == R_NilValue() {
                return rhs;
            }


            let target_env = if forcelocal == 0 && TYPEOF(CADR(lhs)) == SEXPTYPE::SYMSXP {
                let enc = crate::sexp::accessors::ENCLOS(rho);
                if enc.is_null() || enc == R_NilValue() { rho } else { enc }
            } else {
                rho
            };
            let target_expr = Rf_eval(CADR(lhs), target_env);
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
                let mut cell = slot_subs;
                while !cell.is_null() && cell != R_NilValue() {
                    let sub = CAR(cell);
                    if !sub.is_null() && TYPEOF(sub) == SEXPTYPE::SYMSXP {
                        let name = crate::sexp::accessors::PRINTNAME(sub);
                        if !name.is_null() && name != R_NilValue() {
                            let s = crate::sexp::constructors::Rf_mkString(
                                crate::sexp::accessors::CHAR(name),
                            );
                            crate::sexp::accessors::SETCAR(cell, s);
                        }
                    }
                    cell = CDR(cell);
                }
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
        let wrap_lang = |x: SEXP| match TYPEOF(x) {
            t if t == SEXPTYPE::LANGSXP
                || t == SEXPTYPE::SYMSXP
                || t == SEXPTYPE::EXPRSXP =>
            {
                crate::sexp::memory_ext::R_mkEVPROMISE(R_NilValue(), x)
            }
            _ => x,
        };
        let value = wrap_lang(value);
        let target = wrap_lang(target);
        let mut tail = crate::sexp::constructors::Rf_cons(value, R_NilValue());
        SETTAG(tail, crate::sexp::symbol::Rf_install(c"value".as_ptr()));
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
        if TYPEOF(assign_fn) != SEXPTYPE::SYMSXP {
            return Rf_eval(call, rho);
        }
        let name = crate::sexp::accessors::CHAR(crate::sexp::accessors::PRINTNAME(assign_fn));
        if name.is_null() {
            return Rf_eval(call, rho);
        }
        let Ok(name) = std::ffi::CStr::from_ptr(name).to_str() else {
            return Rf_eval(call, rho);
        };

        match name {

            "[<-" => crate::mainutils::subset::do_subassign(call, assign_fn, args, rho),
            "[[<-" => crate::mainutils::subset::do_subassign2(call, assign_fn, args, rho),

            "$<-" => crate::mainutils::essentials::do_dollar_set(call, assign_fn, args, rho),
            "@<-" => crate::mainutils::essentials::do_at_set(call, assign_fn, args, rho),
            "names<-" => crate::mainutils::essentials::do_names_set(call, assign_fn, args, rho),
            "dim<-" => crate::mainutils::essentials::do_dim_set(call, assign_fn, args, rho),
            "tsp<-" => crate::mainutils::essentials::do_tsp_set(call, assign_fn, args, rho),
            "length<-" => crate::mainutils::essentials::do_length_set(call, assign_fn, args, rho),
            "levels<-" => crate::mainutils::essentials::do_levels_set(call, assign_fn, args, rho),
            "storage.mode<-" | "mode<-" => {
                crate::mainutils::essentials::do_storage_mode_set(call, assign_fn, args, rho)
            }

            "dimnames<-" => {
                crate::mainutils::essentials::do_dimnames_set(call, assign_fn, args, rho)
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
        if !matches!(
            SEXPTYPE(TYPEOF(target)),
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
        ) || !matches!(
            SEXPTYPE(TYPEOF(value)),
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
        ) || XLENGTH(value) < 1
        {
            return None;
        }

        let index = scalar_positive_index(CAR(subs))?;
        if index >= XLENGTH(target) {
            return None;
        }

        // GNU do_subassign_dflt duplicates when MAYBE_SHARED (NAMED >= 2).
        // This AST scalar shortcut used to mutate in place and alias every
        // binding of the same vector (`y <- x; x[1] <- 9L`). Fall through
        // so the default path can shallow-duplicate first.
        if NAMED(target) >= 2 {
            return None;
        }

        match TYPEOF(target) {
            t if t == SEXPTYPE::REALSXP => {
                let replacement = match TYPEOF(value) {
                    vt if vt == SEXPTYPE::REALSXP => REAL_ELT(value, 0),
                    vt if vt == SEXPTYPE::INTSXP || vt == SEXPTYPE::LGLSXP => {
                        let v = if vt == SEXPTYPE::INTSXP {
                            INTEGER_ELT(value, 0)
                        } else {
                            LOGICAL_ELT(value, 0)
                        };
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
                    vt if vt == SEXPTYPE::INTSXP => INTEGER_ELT(value, 0),
                    vt if vt == SEXPTYPE::LGLSXP => LOGICAL_ELT(value, 0),
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
        if index.is_null()
            || !matches!(
                SEXPTYPE(TYPEOF(index)),
                SEXPTYPE::INTSXP | SEXPTYPE::REALSXP
            )
            || XLENGTH(index) != 1
        {
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

/// GNU `replaceCall(afun, *tmp*, rest, rhs)`: `afun(`*tmp*`, ...rest, rhs)`.
unsafe fn replace_tmp_call(assign_fn: SEXP, tmp_sym: SEXP, rest: SEXP, rhs: SEXP) -> SEXP {
    unsafe {
        let rhs_cell = crate::sexp::constructors::Rf_cons(rhs, R_NilValue());
        SETTAG(rhs_cell, crate::sexp::symbol::Rf_install(c"value".as_ptr()));
        let _rhs_cell = protect(rhs_cell);
        let mut tail = rhs_cell;

        let mut guards = Vec::new();
        let mut current = rest;
        let mut cells = Vec::new();
        while !current.is_null() && current != R_NilValue() {
            cells.push((CAR(current), TAG(current)));
            current = CDR(current);
        }
        for (arg, tag) in cells.into_iter().rev() {
            let cell = crate::sexp::constructors::Rf_cons(arg, tail);
            if !tag.is_null() && tag != R_NilValue() {
                SETTAG(cell, tag);
            }
            guards.push(protect(cell));
            tail = cell;
        }
        let tmp_cell = crate::sexp::constructors::Rf_cons(tmp_sym, tail);
        let _tmp_cell = protect(tmp_cell);
        let call = crate::sexp::constructors::Rf_cons(assign_fn, tmp_cell);
        if !call.is_null() {
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        call
    }
}

/// Convert `f` or `pkg::f` / `pkg:::f` to the assignment function head.

unsafe fn replacement_fun_head(fun: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(fun) == SEXPTYPE::SYMSXP {
            return get_assign_fcn_sym(fun);
        }
        if TYPEOF(fun) == SEXPTYPE::LANGSXP {
            let op = CAR(fun);
            let op_name = symbol_name(op);
            if matches!(op_name.as_deref(), Some("::") | Some(":::"))
                && TYPEOF(CADDR(fun)) == SEXPTYPE::SYMSXP
            {
                let assign = get_assign_fcn_sym(CADDR(fun));
                if assign != R_NilValue() {
                    let call = crate::sexp::constructors::Rf_lang3(op, CADR(fun), assign);
                    return call;
                }
            }
        }
        R_NilValue()
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
