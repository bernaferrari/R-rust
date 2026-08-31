//! Session state — commandArgs, options, interactive, getRversion, ls.args,
//! deparse/dput/dget, bquote.

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
// R runtime
// ---------------------------------------------------------------------------

/// R's `commandArgs()` — returns the command line arguments as a character vector.
pub unsafe fn do_commandArgs(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let args: Vec<String> = std::env::args().collect();
        let n = args.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (i, arg) in args.iter().enumerate() {
            let cs = CString::new(arg.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        result
    }
}

/// R's `getOption(x)` — delegate to the canonical options implementation.
pub unsafe fn do_getOption(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::do_getOption(call, op, args, rho) }
}

/// R's `options(...)` — delegate to the canonical options implementation.
pub unsafe fn do_options(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::do_options(call, op, args, rho) }
}

/// R's `interactive()` — returns FALSE (not in interactive session).
pub unsafe fn do_interactive(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(FALSE) }
}

/// Alias for `interactive()`.
pub unsafe fn do_is_interactive(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(FALSE) }
}

/// R's `getRversion()` — returns an `R_system_version` package-version object.
pub unsafe fn do_getRversion(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        let version = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        if !version.is_null() {
            let _version_guard = protect(version);
            let data = INTEGER(version);
            *data.add(0) = 4;
            *data.add(1) = 4;
            *data.add(2) = 1;
            SET_VECTOR_ELT(result, 0, version);
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        if !class.is_null() {
            let _class_guard = protect(class);
            for (i, name) in ["R_system_version", "package_version", "numeric_version"]
                .iter()
                .enumerate()
            {
                let value = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(class, i as R_xlen_t, Rf_mkChar(value.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        result
    }
}

/// R's `R.version.string` — returns the full R version string.
pub unsafe fn do_R_version_string(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = c"R version 4.4.1 (Rust Port)";
        Rf_mkString(s.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime
// ---------------------------------------------------------------------------

/// R-like `ls_args()` — list argument names of current function (simplified: return empty character).
pub unsafe fn do_ls_args(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 0) }
}

/// R's `deparse1(expr, collapse, width.cutoff)` — deparse to a single string.
pub unsafe fn do_deparse1(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let collapse_arg = CAR(CDR(args));
        let sep = if collapse_arg.is_null() || collapse_arg == R_NilValue() {
            " ".to_string()
        } else {
            elt_to_string(collapse_arg, 0)
        };
        let lines = deparse_lines(expr);
        Rf_mkString(CString::new(lines.join(&sep)).unwrap_or_default().as_ptr())
    }
}

/// R's `dput(x, file)` — dump object using the deparser.
pub unsafe fn do_dput(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = arg_by_name_or_position(args, &["file"], 1);
        let lines = deparse_lines(x);
        let output = format!("{}\n", lines.join("\n"));

        let file = if file_arg.is_null() || file_arg == R_NilValue() || XLENGTH(file_arg) == 0 {
            String::new()
        } else {
            elt_to_string(file_arg, 0)
        };
        if file.is_empty() {
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&output);
            } else {
                print!("{}", output);
            }
        } else {
            std::fs::write(&file, output).unwrap_or_else(|err| {
                std::panic::panic_any(RError {
                    message: format!("cannot write dump file '{}': {err}", file),
                })
            });
        }
        x
    }
}

fn deparse_lines(expr: SEXP) -> Vec<String> {
    unsafe {
        let deparsed = crate::mainutils::deparse::deparse1(
            expr,
            false,
            crate::mainutils::deparse::DEFAULT_USER_DEPARSE,
        );
        let n = XLENGTH(deparsed);
        if deparsed.is_null() || deparsed == R_NilValue() || n == 0 {
            return vec!["NULL".to_string()];
        }
        (0..n).map(|i| elt_to_string(deparsed, i)).collect()
    }
}

/// R's `dget(file)` — read, parse, and evaluate a dumped expression.
pub unsafe fn do_dget(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        if file_arg.is_null() || file_arg == R_NilValue() || XLENGTH(file_arg) == 0 {
            std::panic::panic_any(RError {
                message: "invalid 'file' argument".to_string(),
            });
        }

        let path = elt_to_string(file_arg, 0);
        let code = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            std::panic::panic_any(RError {
                message: format!("cannot read dump file '{}': {err}", path),
            })
        });
        let expr = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse(&code, arena).map_err(|err| err.to_string())
        })
        .unwrap_or_else(|message| std::panic::panic_any(RError { message }));
        if expr.is_null() || expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(expr, rho)
        }
    }
}

/// R's `bquote(expr)` — quote with `.(...)` substitution.
pub unsafe fn do_bquote(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() {
            return R_NilValue();
        }
        bquote_walk(expr, rho)
    }
}

unsafe fn bquote_walk(expr: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let expr_type = TYPEOF(expr);
        if expr_type == SEXPTYPE::LANGSXP && is_bquote_unquote_call(expr) {
            let unquoted = CAR(CDR(expr));
            return crate::eval::eval::Rf_eval(unquoted, rho);
        }

        if expr_type != SEXPTYPE::LANGSXP && expr_type != SEXPTYPE::LISTSXP {
            return expr;
        }

        let mut source = expr;
        let mut head = R_NilValue();
        let mut tail = R_NilValue();
        while !source.is_null() && source != R_NilValue() {
            let value = bquote_walk(CAR(source), rho);
            let cell = Rf_cons(value, R_NilValue());
            SETTAG(cell, TAG(source));
            if head == R_NilValue() {
                head = cell;
            } else {
                SETCDR(tail, cell);
            }
            tail = cell;
            source = CDR(source);
        }
        if expr_type == SEXPTYPE::LANGSXP && !head.is_null() && head != R_NilValue() {
            (*head).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        head
    }
}

unsafe fn is_bquote_unquote_call(expr: SEXP) -> bool {
    unsafe {
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return false;
        }
        let head = CAR(expr);
        if TYPEOF(head) != SEXPTYPE::SYMSXP || symbol_name(head).as_deref() != Some(".") {
            return false;
        }
        let args = CDR(expr);
        !args.is_null()
            && args != R_NilValue()
            && (CDR(args).is_null() || CDR(args) == R_NilValue())
    }
}
