#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/source.c — parse() internal function.
//!
//! Provides do_parse() for .Internal(parse(...)).

use std::ffi::CStr;
use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{
    CAR, CDR, CHAR, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{Rf_allocVector3, Rf_mkCharLen};
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
        reset_parse_state();
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
            remember_parse_context(&source);
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
    let message = message.into();
    unsafe {
        store_parse_error(
            &message,
            1,
            R_GetParseErrorCol().max(1),
            R_GetParseErrorFile(),
        );
    }
    std::panic::panic_any(RError { message });
}

pub(crate) fn reset_parse_state() {
    crate::sexp::instance::with_current_instance(|inst| {
        inst.eval_state.parse_error_msg.fill(0);
        inst.eval_state.parse_error = 0;
        inst.eval_state.parse_error_col = 0;
        inst.eval_state.parse_error_file = ptr::null_mut();
        inst.eval_state.parse_context_line = 0;
        inst.eval_state.parse_context.clear();
    });
}

pub(crate) fn remember_parse_context(source: &str) {
    crate::sexp::instance::with_current_instance(|inst| {
        inst.eval_state.parse_context.clear();
        inst.eval_state
            .parse_context
            .extend(source.lines().map(str::to_owned));
        if inst.eval_state.parse_context.is_empty() {
            inst.eval_state.parse_context.push(String::new());
        }
        inst.eval_state.parse_context_line = inst.eval_state.parse_context.len() as c_int;
    });
}

pub(crate) unsafe fn store_parse_error(message: &str, status: c_int, col: c_int, file: SEXP) {
    crate::sexp::instance::with_current_instance(|inst| {
        let msg = &mut inst.eval_state.parse_error_msg;
        msg.fill(0);
        let bytes = message.as_bytes();
        let copy_len = bytes
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(bytes.len())
            .min(msg.len().saturating_sub(1));
        msg[..copy_len].copy_from_slice(&bytes[..copy_len]);
        inst.eval_state.parse_error = status;
        inst.eval_state.parse_error_col = col.max(0);
        inst.eval_state.parse_error_file = if file.is_null() {
            ptr::null_mut()
        } else {
            file
        };
    });
}

fn current_parse_error_message() -> String {
    crate::sexp::instance::with_current_instance(|inst| {
        let bytes = &inst.eval_state.parse_error_msg;
        let len = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..len]).into_owned()
    })
    .filter(|message| !message.is_empty())
    .unwrap_or_else(|| "parse error".to_string())
}

/// Raise a parse error using the current per-session parser context.
pub unsafe fn parseError(call: SEXP, linenum: c_int) {
    unsafe {
        let message = current_parse_error_message();
        store_parse_error(&message, 1, R_GetParseErrorCol(), call);
        crate::sexp::instance::with_current_instance(|inst| {
            if linenum > 0 {
                inst.eval_state.parse_context_line = linenum;
            }
        });
        std::panic::panic_any(RError {
            message: format_parse_error_message(&message, linenum),
        });
    }
}

/// Return the most recent parser context as a character vector.
pub unsafe fn getParseContext() -> SEXP {
    unsafe {
        let context = crate::sexp::instance::with_current_instance(|inst| {
            inst.eval_state.parse_context.clone()
        })
        .unwrap_or_default();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, context.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        for (index, line) in context.iter().enumerate() {
            let bytes = line.as_bytes();
            let charsxp = Rf_mkCharLen(bytes.as_ptr() as *const _, bytes.len() as c_int);
            SET_STRING_ELT(result, index as R_xlen_t, charsxp);
        }
        result
    }
}

fn format_parse_error_message(message: &str, linenum: c_int) -> String {
    let context = crate::sexp::instance::with_current_instance(|inst| {
        (
            inst.eval_state.parse_context.clone(),
            inst.eval_state.parse_context_line,
            inst.eval_state.parse_error_col,
        )
    })
    .unwrap_or_default();
    let (lines, context_line, col) = context;
    if linenum > 0 && lines.is_empty() {
        return format!("{linenum}:{col}: {message}");
    }
    match lines.as_slice() {
        [] => message.to_string(),
        [line] if linenum > 0 => format!("{linenum}:{col}: {message}\n{context_line}: {line}"),
        [line] => format!("{message} in \"{line}\""),
        lines if linenum > 0 => {
            let prev = &lines[lines.len() - 2];
            let last = &lines[lines.len() - 1];
            format!(
                "{linenum}:{col}: {message}\n{}: {prev}\n{context_line}: {last}",
                context_line - 1
            )
        }
        lines => format!(
            "{message} in:\n\"{}\n{}\"",
            lines[lines.len() - 2],
            lines[lines.len() - 1]
        ),
    }
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

    #[test]
    fn test_do_parse_records_error_message_and_context() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let text = crate::sexp::constructors::Rf_mkString(c"1 +".as_ptr());
            let n = crate::sexp::constructors::Rf_ScalarInteger(-1);
            let args = crate::sexp::constructors::Rf_cons(
                R_NilValue(),
                crate::sexp::constructors::Rf_cons(
                    n,
                    crate::sexp::constructors::Rf_cons(text, R_NilValue()),
                ),
            );
            let err = std::panic::catch_unwind(|| {
                do_parse(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            });
            assert!(err.is_err());
            assert_eq!(R_GetParseError(), 1);
            assert_eq!(R_GetParseErrorCol(), 1);
            assert!(
                CStr::from_ptr(crate::mainutils::main::R_GetParseErrorMsg())
                    .to_string_lossy()
                    .contains("unexpected end of input")
            );

            let context = getParseContext();
            assert_eq!(TYPEOF(context), SEXPTYPE::STRSXP);
            assert_eq!(XLENGTH(context), 1);
            assert_eq!(
                CStr::from_ptr(CHAR(STRING_ELT(context, 0))).to_str(),
                Ok("1 +")
            );
        }
    }

    #[test]
    fn test_parse_error_panics_with_context() {
        let _session = crate::sexp::session::RSession::new();
        remember_parse_context("x <- 1\ny +");
        unsafe {
            store_parse_error("unexpected token", 2, 3, ptr::null_mut());
        }
        let err = std::panic::catch_unwind(|| unsafe {
            parseError(ptr::null_mut(), 2);
        });
        assert!(err.is_err());
        assert_eq!(unsafe { R_GetParseError() }, 1);
        assert_eq!(unsafe { R_GetParseContextLine() }, 2);
    }
}
