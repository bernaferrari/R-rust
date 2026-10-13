//! do.call wrappers and argument matching.

use super::*;

/// R's `do.call(what, args)` — call function with list of args.
pub(super) fn do_call_arguments(args: SEXP) -> [Option<SEXP>; 4] {
    unsafe {
        let formals = ["what", "args", "quote", "envir"];
        let mut matched = [None; 4];
        let mut positional = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = tag_name(current) {
                let index = formals
                    .iter()
                    .position(|formal| *formal == name)
                    .or_else(|| {
                        formals
                            .iter()
                            .position(|formal| !name.is_empty() && formal.starts_with(&name))
                    })
                    .unwrap_or_else(|| base_error(format!("unused argument ({name} = ...)")));
                if matched[index].is_some() {
                    base_error(format!(
                        "formal argument '{}' matched by multiple actual arguments",
                        formals[index]
                    ));
                }
                matched[index] = Some(CAR(current));
            } else {
                positional.push(CAR(current));
            }
            current = CDR(current);
        }
        for value in positional {
            let index = matched
                .iter()
                .position(Option::is_none)
                .unwrap_or_else(|| base_error("unused argument (...)"));
            matched[index] = Some(value);
        }
        for index in 0..2 {
            if matched[index].is_none() || matched[index] == Some(R_MissingArg()) {
                base_error(format!(
                    "argument '{}' is missing, with no default",
                    formals[index]
                ));
            }
        }
        matched
    }
}

pub unsafe fn do_do_call(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let matched = do_call_arguments(args);
        let what = crate::eval::eval::Rf_eval(matched[0].unwrap(), rho);
        let _what = protect(what);
        if !(TYPEOF(what) == SEXPTYPE::CLOSXP
            || TYPEOF(what) == SEXPTYPE::BUILTINSXP
            || TYPEOF(what) == SEXPTYPE::SPECIALSXP)
            && !(TYPEOF(what) == SEXPTYPE::STRSXP
                && XLENGTH(what) == 1
                && STRING_ELT(what, 0) != crate::sexp::globals::R_NaString())
        {
            base_error("'what' must be a function or character string");
        }
        let fun = callable_expr(what);
        let _fun = protect(fun);
        let arg_list = crate::eval::eval::Rf_eval(matched[1].unwrap(), rho);
        let _arg_list = protect(arg_list);
        if arg_list != R_NilValue() && TYPEOF(arg_list) != SEXPTYPE::VECSXP {
            base_error("second argument must be a list");
        }
        let env = matched[3].map_or(rho, |expr| crate::eval::eval::Rf_eval(expr, rho));
        let _env = protect(env);
        if TYPEOF(env) != SEXPTYPE::ENVSXP {
            base_error("'envir' must be an environment");
        }
        let quoted = if let Some(expr) = matched[2] {
            let value = crate::eval::eval::Rf_eval(expr, rho);
            let flag = crate::mainutils::coerce::asLogical(value);
            if flag == NA_LOGICAL {
                base_error("invalid 'quote' argument");
            }
            flag != FALSE
        } else {
            false
        };
        let names = crate::sexp::attrib_core::getAttrib(
            arg_list,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let mut call_args = R_NilValue();
        let mut guards = Vec::new();
        for i in (0..XLENGTH(arg_list)).rev() {
            let value = VECTOR_ELT(arg_list, i);
            let value = if quoted {
                let wrapped =
                    crate::sexp::constructors::Rf_lang2(Rf_install(c"quote".as_ptr()), value);
                guards.push(protect(wrapped));
                wrapped
            } else {
                value
            };
            let cell = Rf_cons(value, call_args);
            guards.push(protect(cell));
            if TYPEOF(names) == SEXPTYPE::STRSXP && i < XLENGTH(names) {
                let name_elt = STRING_ELT(names, i);
                if name_elt == crate::sexp::globals::R_NaString() {
                    SETTAG(cell, Rf_install(c"NA".as_ptr()));
                } else {
                    let chars = CHAR(name_elt);
                    if !chars.is_null() && *chars != 0 {
                        SETTAG(cell, Rf_install(chars));
                    }
                }
            }
            call_args = cell;
        }
        let call_sexp = Rf_cons(fun, call_args);
        let _call_sexp = protect(call_sexp);
        crate::sexp::accessors::SET_TYPEOF(call_sexp, SEXPTYPE::LANGSXP.as_c_int());
        let _ctx = crate::sexp::context::begin_context_guard(
            crate::sexp::context::ctxt_flags::CTXT_BUILTIN,
            _call,
            env,
            rho,
            None,
            what,
            call_args,
        );
        crate::eval::eval::Rf_eval(call_sexp, env)
    }
}

/// GNU `.Internal(do.call(what, args, envir))` contract.
///
/// GNU's R-level `do.call` closure handles `quote` itself and hands the
/// internal exactly three arguments. This engine registers `do_do_call` as
/// the R-level builtin with the four-argument `(what, args, quote, envir)`
/// shape, so `.Internal` calls arriving from upstream base source closures
/// are reshaped here before delegation.
pub unsafe fn do_do_call_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut matched: [Option<SEXP>; 3] = [None; 3];
        let mut positional = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            match tag_name(current).as_deref() {
                Some("what") => matched[0] = Some(CAR(current)),
                Some("args") => matched[1] = Some(CAR(current)),
                Some("envir") => matched[2] = Some(CAR(current)),
                Some(_) => positional.push(CAR(current)),
                None => positional.push(CAR(current)),
            }
            current = CDR(current);
        }
        for value in positional {
            let index = matched
                .iter()
                .position(Option::is_none)
                .unwrap_or_else(|| base_error("unused argument (...)"));
            matched[index] = Some(value);
        }

        let what = matched[0].unwrap_or(R_MissingArg());
        let arg_list = matched[1].unwrap_or(R_MissingArg());
        let envir = matched[2].unwrap_or(R_MissingArg());
        let _what_guard = protect(what);
        let _args_guard = protect(arg_list);
        let _envir_guard = protect(envir);
        let false_flag = crate::sexp::constructors::Rf_ScalarLogical(0);
        let _false_guard = protect(false_flag);

        let env_cell = Rf_cons(envir, R_NilValue());
        let _env_cell_guard = protect(env_cell);
        SETTAG(env_cell, Rf_install(c"envir".as_ptr()));
        let quote_cell = Rf_cons(false_flag, env_cell);
        let _quote_cell_guard = protect(quote_cell);
        SETTAG(quote_cell, Rf_install(c"quote".as_ptr()));
        let args_cell = Rf_cons(arg_list, quote_cell);
        let _args_cell_guard = protect(args_cell);
        SETTAG(args_cell, Rf_install(c"args".as_ptr()));
        let spread = Rf_cons(what, args_cell);
        let _spread_guard = protect(spread);
        SETTAG(spread, Rf_install(c"what".as_ptr()));
        do_do_call(call, op, spread, rho)
    }
}

pub(super) fn callable_arg_by_name_or_position(
    args: SEXP,
    names: &[&str],
    position: usize,
    rho: SEXP,
) -> SEXP {
    unsafe { match_fun_arg(arg_by_name_or_position(args, names, position), rho) }
}

pub(super) unsafe fn match_fun_arg(expr: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return expr;
        }
        let t = TYPEOF(expr);
        if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            return expr;
        }
        let val = if t == SEXPTYPE::SYMSXP
            || t == SEXPTYPE::LANGSXP
            || t == SEXPTYPE::PROMSXP
            || t == SEXPTYPE::BCODESXP
        {
            crate::eval::eval::Rf_eval(expr, rho)
        } else {
            expr
        };
        let vt = TYPEOF(val);
        if vt == SEXPTYPE::CLOSXP || vt == SEXPTYPE::BUILTINSXP || vt == SEXPTYPE::SPECIALSXP {
            return val;
        }
        if vt == SEXPTYPE::STRSXP && XLENGTH(val) > 0 {
            let charsxp = STRING_ELT(val, 0);
            if charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString() {
                return R_NilValue();
            }
            let name = CHAR(charsxp);
            if name.is_null() {
                return R_NilValue();
            }
            return crate::eval::eval::Rf_eval(Rf_install(name), rho);
        }
        if vt == SEXPTYPE::SYMSXP {
            return crate::eval::eval::Rf_eval(val, rho);
        }
        val
    }
}

pub(super) fn eval_arg_by_name_or_position(
    args: SEXP,
    names: &[&str],
    position: usize,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let expr = arg_by_name_or_position(args, names, position);
        if expr.is_null() || expr == R_NilValue() {
            R_NilValue()
        } else if is_already_a_value(expr) {
            // Evaluated builtins (vapply -> do_lapply) arrive with args
            // already evaluated: X is a pairlist/vector, not a symbol or
            // call. Re-evaluating a pairlist evaluates EACH element, which
            // panics on a missing-arg formal (evalList "argument N is
            // empty"). Values pass through untouched.
            expr
        } else {
            crate::eval::eval::Rf_eval(expr, rho)
        }
    }
}
