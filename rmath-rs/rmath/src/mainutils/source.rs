#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/source.c — parse() internal function.
//!
//! Provides do_parse() for .Internal(parse(...)).

use std::ffi::CStr;
use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{CAR, CDR, CHAR, SET_VECTOR_ELT, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::context::RError;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::protect::protect;

/// Parse R expressions.
///
/// This is the equivalent of R's `do_parse()` from source.c.
///
/// .Internal( parse(file, n, text, prompt, srcfile, encoding) )
pub unsafe fn do_parse(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let text = parse_text_arg(args);
        if text.is_null() || text == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }
        if TYPEOF(text) != SEXPTYPE::STRSXP {
            parse_failure("invalid 'text' argument");
        }

        let n = XLENGTH(text);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let mut parsed = Vec::new();
        for i in 0..n {
            let elt = STRING_ELT(text, i);
            if elt == R_NaString() {
                parse_failure("invalid 'text' argument");
            }
            let source = CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned();
            if source.trim().is_empty() {
                continue;
            }
            let expr = crate::sexp::memory::with_arena(|arena| {
                crate::eval::parser::parse(&source, arena).map_err(|err| err.to_string())
            });
            match expr {
                Ok(expr) if !expr.is_null() && expr != R_NilValue() => parsed.push(expr),
                Ok(_) => {}
                Err(message) => parse_failure(message),
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::EXPRSXP, parsed.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, expr) in parsed.into_iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, expr);
        }
        result
    }
}

unsafe fn parse_text_arg(args: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }
        let after_file = CDR(args);
        if after_file.is_null() || after_file == R_NilValue() {
            return CAR(args);
        }
        let after_n = CDR(after_file);
        if after_n.is_null() || after_n == R_NilValue() {
            return CAR(args);
        }
        CAR(after_n)
    }
}

fn parse_failure(message: impl Into<String>) -> ! {
    unsafe {
        R_SetParseError(1);
    }
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

/// Parse error handler (stub).
pub unsafe fn parseError(_call: SEXP, _linenum: c_int) {
    // does nothing (error handling is via panic)
}

/// Get the parse context (stub).
pub unsafe fn getParseContext() -> SEXP {
    unsafe { R_NilValue() }
}

/// Parse context buffer size.
pub const PARSE_CONTEXT_SIZE: c_int = 256;

pub unsafe fn R_GetParseError() -> c_int {
    crate::sexp::instance::with_current_instance(|inst| inst.eval_state.parse_error).unwrap_or(0)
}

pub unsafe fn R_SetParseError(val: c_int) {
    crate::sexp::instance::with_current_instance(|inst| {
        inst.eval_state.parse_error = val;
    });
}

pub unsafe fn R_GetParseErrorCol() -> c_int {
    crate::sexp::instance::with_current_instance(|inst| inst.eval_state.parse_error_col)
        .unwrap_or(0)
}

pub unsafe fn R_GetParseErrorFile() -> SEXP {
    crate::sexp::instance::with_current_instance(|inst| inst.eval_state.parse_error_file)
        .unwrap_or(ptr::null_mut())
}

pub unsafe fn R_GetParseContextLine() -> c_int {
    crate::sexp::instance::with_current_instance(|inst| inst.eval_state.parse_context_line)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_parse_stub() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_parse(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(TYPEOF(result), SEXPTYPE::EXPRSXP);
            assert_eq!(XLENGTH(result), 0);
        }
    }

    #[test]
    fn test_do_parse_text_arg() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let text = crate::sexp::constructors::Rf_mkString(c"1 + 2".as_ptr());
            let n = crate::sexp::constructors::Rf_ScalarInteger(-1);
            let args = crate::sexp::constructors::Rf_cons(
                R_NilValue(),
                crate::sexp::constructors::Rf_cons(
                    n,
                    crate::sexp::constructors::Rf_cons(text, R_NilValue()),
                ),
            );
            let result = do_parse(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::EXPRSXP);
            assert_eq!(XLENGTH(result), 1);
        }
    }

    #[test]
    fn test_parse_error_state() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            R_SetParseError(42);
        }
        assert_eq!(unsafe { R_GetParseError() }, 42);
    }
}
