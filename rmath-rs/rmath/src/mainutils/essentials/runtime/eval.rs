//! `eval`, `substitute`, `quote`, `parse` plus source-parsing helpers.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete R runtime: eval, substitute, quote, parse
// ---------------------------------------------------------------------------

/// R's `local(expr, envir = new.env())` — evaluate `expr` in a fresh child
/// environment and return its value (eval.c `do_local`). The default
/// environment parents to the caller (`_rho`); an explicit ENVSXP `envir`
/// is used as-is (wrapped in a child so assignments stay local).
pub unsafe fn do_local(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let envir_arg = CAR(CDR(args));
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let parent = if envir_arg.is_null() || envir_arg == R_NilValue() {
            _rho
        } else {
            envir_arg
        };
        let env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            parent,
            R_NilValue(),
        );
        if env.is_null() {
            return R_NilValue();
        }
        let _guard = protect(env);
        crate::eval::eval::Rf_eval(expr, env)
    }
}

pub unsafe fn do_eval(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let envir_arg = CAR(CDR(args));
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let envir = if envir_arg.is_null() || envir_arg == R_NilValue() {
            _rho
        } else {
            envir_arg
        };
        // eval.c do_eval(): language/symbol/bytecode values evaluate in
        // `envir`; expression vectors evaluate element-wise returning the
        // last value; any other value is returned unchanged (Rf_eval no
        // longer treats expression vectors as evaluable).
        let bcode = SEXPTYPE::BCODESXP;
        if TYPEOF(expr) == SEXPTYPE::LANGSXP
            || TYPEOF(expr) == SEXPTYPE::SYMSXP
            || TYPEOF(expr) == bcode
        {
            return crate::eval::eval::Rf_eval(expr, envir);
        }
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP {
            let n = XLENGTH(expr);
            let mut result = R_NilValue();
            for i in 0..n {
                let element = VECTOR_ELT(expr, i);
                if element.is_null() || element == R_NilValue() {
                    continue;
                }
                result = crate::eval::eval::Rf_eval(element, envir);
            }
            return result;
        }
        expr
    }
}

/// R's `substitute(expr, env)` — substitute symbols in expression.
pub unsafe fn do_substitute(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::coerce::do_substitute(_call, _op, args, _rho) }
}

/// R's `quote(expr)` — return expression unevaluated.
pub unsafe fn do_quote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, NAMED, SET_NAMED};
        let mut nargs = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            nargs += 1;
            current = CDR(current);
        }
        if nargs != 1 {
            base_error(format!(
                "{nargs} arguments passed to 'quote' which requires 1"
            ));
        }
        let tag = TAG(args);
        if !tag.is_null() && tag != R_NilValue() {
            let name = if TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let printname = PRINTNAME(tag);
                if printname.is_null() {
                    String::new()
                } else {
                    let chars = CHAR(printname);
                    if chars.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(chars).to_string_lossy().into_owned()
                    }
                }
            } else {
                String::new()
            };
            if name != "expr" {
                base_error(format!(
                    "supplied argument name '{name}' does not match 'expr'"
                ));
            }
        }
        let val = CAR(args);
        if val.is_null() || val == R_NilValue() {
            return R_NilValue();
        }
        // ENSURE_NAMEDMAX — prevent modification of source code references
        if NAMED(val) < 2 {
            SET_NAMED(val, 2);
        }
        val
    }
}

/// R's `parse(text)` — parse R code strings into an expression vector.
pub unsafe fn do_parse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let text_arg = arg_by_name_or_position(args, &["text"], 0);
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        if text_arg.is_null() || text_arg == R_NilValue() {
            if !file_arg.is_null() && file_arg != R_NilValue() {
                let file_path = elt_to_string(file_arg, 0);
                let content = std::fs::read_to_string(&file_path).unwrap_or_else(|err| {
                    base_error(format!("cannot open file '{}': {}", file_path, err))
                });
                return parse_source_expression_vector(&content);
            }
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let n = XLENGTH(text_arg);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let mut source = Vec::with_capacity(n as usize);
        for i in 0..n {
            if TYPEOF(text_arg) == SEXPTYPE::STRSXP && is_string_na(text_arg, i) {
                std::panic::panic_any(RError {
                    message: "invalid 'text' argument".to_string(),
                });
            }
            let text = elt_to_string(text_arg, i);
            source.push(text);
        }
        parse_source_strings(&source)
    }
}

unsafe fn parse_source_strings(source: &[String]) -> SEXP {
    let combined = source.join("\n");
    unsafe { parse_source_expression_vector(&combined) }
}

pub(crate) unsafe fn parse_source_expression_vector(source: &str) -> SEXP {
    unsafe {
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions_strict(source, arena).map_err(|err| err.to_string())
        })
        .unwrap_or_else(|message| std::panic::panic_any(RError { message }));

        let result = Rf_allocVector3(SEXPTYPE::EXPRSXP, parsed.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in parsed.into_iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, value);
        }
        result
    }
}
