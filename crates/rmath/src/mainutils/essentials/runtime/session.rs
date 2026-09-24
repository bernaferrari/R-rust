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

/// `getNamespaceVersion(ns)` for a base package is the R version.
pub unsafe fn do_getNamespaceVersion(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_getRversion(call, op, args, rho) }
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

/// R's `dput(x, file, control)` — dump using the requested deparse options.
pub unsafe fn do_dput(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = arg_by_name_or_position(args, &["file"], 1);
        let control_arg = arg_by_name_or_position(args, &["control"], 2);
        let opts = if control_arg.is_null()
            || control_arg == R_NilValue()
            || control_arg == R_MissingArg()
        {
            crate::mainutils::deparse::DEFAULT_USER_DEPARSE
        } else if TYPEOF(control_arg) == SEXPTYPE::INTSXP
            || TYPEOF(control_arg) == SEXPTYPE::REALSXP
        {
            crate::mainutils::coerce::asInteger(control_arg)
        } else {
            crate::mainutils::deparse::deparse_opts_from_control(control_arg)
        };
        let deparsed = crate::mainutils::deparse::deparse1(x, false, opts);
        let n = if deparsed.is_null() || deparsed == R_NilValue() {
            0
        } else {
            XLENGTH(deparsed)
        };
        let lines: Vec<String> = if n == 0 {
            vec!["NULL".to_string()]
        } else {
            (0..n).map(|i| elt_to_string(deparsed, i)).collect()
        };
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
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);

        x
    }
}

/// GNU `dump(list, file, ...)` — write `name <-` deparsed values.
pub unsafe fn do_dump(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let names = arg_by_name_or_position(args, &["list"], 0);
        if names.is_null()
            || names == R_NilValue()
            || TYPEOF(names) != SEXPTYPE::STRSXP
            || XLENGTH(names) == 0
        {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let file_arg = arg_by_name_or_position(args, &["file"], 1);
        let file = if file_arg.is_null()
            || file_arg == R_NilValue()
            || TYPEOF(file_arg) != SEXPTYPE::STRSXP
            || XLENGTH(file_arg) == 0
        {
            String::new()
        } else {
            elt_to_string(file_arg, 0)
        };
        let envir = arg_by_name_or_position(args, &["envir"], 4);
        let env = if TYPEOF(envir) == SEXPTYPE::ENVSXP {
            envir
        } else {
            rho
        };
        let mut output = String::new();
        let n = XLENGTH(names);
        let outnames = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(outnames);
        for i in 0..n {
            SET_STRING_ELT(outnames, i, STRING_ELT(names, i));
            let nm = elt_to_string(names, i);
            let cname = CString::new(nm.as_str()).unwrap_or_default();
            let sym = crate::sexp::symbol::Rf_install(cname.as_ptr());
            let mut val = crate::sexp::envir::R_findVar(sym, env);
            if val.is_null() || val == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            if TYPEOF(val) == SEXPTYPE::PROMSXP {
                val = crate::sexp::envir::forcePromise(val);
            }
            let lines = deparse_lines(val);
            if crate::mainutils::deparse::isValidName(cname.as_ptr()) {
                output.push_str(&format!("{} <-\n", nm));
            } else {
                output.push_str(&format!("`{}` <-\n", nm));
            }
            output.push_str(&lines.join("\n"));
            output.push('\n');
        }
        if TYPEOF(file_arg) == SEXPTYPE::INTSXP && XLENGTH(file_arg) >= 1 {
            crate::mainutils::connections::connection_write_bytes(
                *INTEGER(file_arg),
                output.as_bytes(),
            );
        } else if file.is_empty() {
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&output);
            } else {
                print!("{}", output);
            }
        } else {
            std::fs::write(&file, output).unwrap_or_else(|err| {
                std::panic::panic_any(RError {
                    message: format!("cannot write dump file '{file}': {err}"),
                })
            });
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        outnames
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
        let splice = {
            let mut cell = CDR(args);
            let mut on = false;
            while !cell.is_null() && cell != R_NilValue() {
                let tag = TAG(cell);
                let named = !tag.is_null() && tag != R_NilValue()
                    && symbol_name(tag).as_deref() == Some("splice");
                if named || (tag.is_null() || tag == R_NilValue()) {
                    let v = crate::eval::eval::Rf_eval(CAR(cell), rho);
                    if TYPEOF(v) == SEXPTYPE::LGLSXP && XLENGTH(v) > 0 && *LOGICAL(v) != 0 {
                        on = true;
                    }
                    if named {
                        break;
                    }
                }
                cell = CDR(cell);
            }
            on
        };
        bquote_walk(expr, rho, splice)
    }
}

unsafe fn bquote_walk(expr: SEXP, rho: SEXP, splice: bool) -> SEXP {
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
            if splice && is_bquote_splice_call(CAR(source)) {
                let unquoted = CAR(CDR(CAR(source)));
                let value = crate::eval::eval::Rf_eval(unquoted, rho);
                let mut elt = value;
                if TYPEOF(value) == SEXPTYPE::EXPRSXP || TYPEOF(value) == SEXPTYPE::VECSXP {
                    let n = XLENGTH(value);
                    for i in 0..n {
                        let cell = Rf_cons(VECTOR_ELT(value, i), R_NilValue());
                        if head == R_NilValue() {
                            head = cell;
                        } else {
                            SETCDR(tail, cell);
                        }
                        tail = cell;
                    }
                } else {
                    while !elt.is_null() && elt != R_NilValue() && (TYPEOF(elt) == SEXPTYPE::LISTSXP || TYPEOF(elt) == SEXPTYPE::LANGSXP) {
                        let cell = Rf_cons(CAR(elt), R_NilValue());
                        if head == R_NilValue() {
                            head = cell;
                        } else {
                            SETCDR(tail, cell);
                        }
                        tail = cell;
                        elt = CDR(elt);
                    }
                }
                source = CDR(source);
                continue;
            }
            let value = bquote_walk(CAR(source), rho, splice);
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
unsafe fn is_bquote_splice_call(expr: SEXP) -> bool {
    unsafe {
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return false;
        }
        let head = CAR(expr);
        TYPEOF(head) == SEXPTYPE::SYMSXP && symbol_name(head).as_deref() == Some("..")
    }
}
