#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/gram.c — parser entry points.
//!
//! The original C parser is generated from `gram.y`. This Rust port routes the
//! C-shaped parser entry points through the hand-written Rust parser used by
//! session evaluation so legacy callers observe the same syntax support.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

use crate::sexp::accessors::{CHAR, SET_VECTOR_ELT, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Parse status constants
// ---------------------------------------------------------------------------

pub const PARSE_OK: c_int = 1;
pub const PARSE_INCOMPLETE: c_int = 2;
pub const PARSE_ERROR: c_int = 4;
pub const PARSE_EOF: c_int = 8;

// ---------------------------------------------------------------------------
// R_ParseVector — parse text into R expressions
// ---------------------------------------------------------------------------

/// Parse a character vector of R code into a list of expressions.
pub unsafe fn R_ParseVector(text: SEXP, n: c_int, status: *mut c_int, _srcfile: SEXP) -> SEXP {
    unsafe {
        match parse_vector(text, n) {
            Ok(exprs) => {
                set_parse_status(status, PARSE_OK);
                exprs_to_exprsxp(exprs)
            }
            Err(_) => {
                set_parse_status(status, PARSE_ERROR);
                Rf_allocVector(SEXPTYPE::EXPRSXP, 0)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R_ParseEvalString — parse and evaluate a string
// ---------------------------------------------------------------------------

/// Parse and evaluate a string of R code.
pub unsafe fn R_ParseEvalString(s: *const c_char, envir: SEXP) -> SEXP {
    unsafe {
        let source = c_string_source(s).unwrap_or_else(|| parse_failure("invalid parse string"));
        parse_eval_source(&source, envir)
    }
}

// ---------------------------------------------------------------------------
// R_ParseEval — parse and evaluate a string with completion handler
// ---------------------------------------------------------------------------

/// Parse and evaluate a string of R code with completion.
pub unsafe fn R_ParseEval(s: *const c_char, envir: SEXP) -> SEXP {
    unsafe { R_ParseEvalString(s, envir) }
}

// ---------------------------------------------------------------------------
// R_ParseEvalBuffer — parse and evaluate a buffer
// ---------------------------------------------------------------------------

/// Parse and evaluate a buffer of R code.
pub unsafe fn R_ParseEvalBuffer(buf: *const c_char, len: c_int, envir: SEXP) -> SEXP {
    unsafe {
        let source =
            buffer_source(buf, len).unwrap_or_else(|| parse_failure("invalid parse buffer"));
        parse_eval_source(&source, envir)
    }
}

// ---------------------------------------------------------------------------
// R_CurrentParseLine — current parse line number
// ---------------------------------------------------------------------------

/// Get the current parse line number.
pub unsafe fn R_CurrentParseLine() -> c_int {
    unsafe { crate::mainutils::source::R_GetParseContextLine() }
}

// ---------------------------------------------------------------------------
// R_ParseFilename — get the current parse filename
// ---------------------------------------------------------------------------

/// Get the current parse filename.
pub unsafe fn R_ParseFilename() -> *const c_char {
    unsafe {
        let file = crate::mainutils::source::R_GetParseErrorFile();
        if !file.is_null()
            && file != R_NilValue()
            && TYPEOF(file) == SEXPTYPE::STRSXP
            && XLENGTH(file) > 0
        {
            let charsxp = STRING_ELT(file, 0);
            if !charsxp.is_null() && charsxp != R_NaString() {
                let value = CHAR(charsxp);
                if !value.is_null() {
                    return value;
                }
            }
        }
    }
    static EMPTY: [c_char; 1] = [0];
    EMPTY.as_ptr()
}

// ---------------------------------------------------------------------------
// R_ParseContext — parse context management
// ---------------------------------------------------------------------------

/// Enter a new parse context.
pub unsafe fn R_ParseContext(buf: *const c_char, len: c_int) -> c_int {
    unsafe {
        let Some(source) = buffer_source(buf, len) else {
            crate::mainutils::source::store_parse_error(
                "invalid parse context buffer",
                PARSE_ERROR,
                0,
                R_NilValue(),
            );
            return PARSE_ERROR;
        };
        crate::mainutils::source::remember_parse_context(&source);
    }
    0
}

/// End the current parse context.
pub unsafe fn R_ParseContextEnd() {}

// ---------------------------------------------------------------------------
// R_ParseVectorBuffer — parse from buffer
// ---------------------------------------------------------------------------

/// Parse from a character buffer.
pub unsafe fn R_ParseVectorBuffer(
    text: *const c_char,
    len: R_xlen_t,
    n: c_int,
    status: *mut c_int,
    _srcfile: SEXP,
) -> SEXP {
    unsafe {
        let Some(source) = buffer_source(text, len as c_int) else {
            set_parse_status(status, PARSE_ERROR);
            return Rf_allocVector(SEXPTYPE::EXPRSXP, 0);
        };
        match parse_source_list(std::iter::once(Ok(source)), n) {
            Ok(exprs) => {
                set_parse_status(status, PARSE_OK);
                exprs_to_exprsxp(exprs)
            }
            Err(_) => {
                set_parse_status(status, PARSE_ERROR);
                Rf_allocVector(SEXPTYPE::EXPRSXP, 0)
            }
        }
    }
}

unsafe fn set_parse_status(status: *mut c_int, value: c_int) {
    unsafe {
        if !status.is_null() {
            *status = value;
        }
    }
}

unsafe fn parse_vector(text: SEXP, n: c_int) -> Result<Vec<SEXP>, String> {
    unsafe {
        if text.is_null() || text == R_NilValue() {
            return Ok(Vec::new());
        }
        if TYPEOF(text) != SEXPTYPE::STRSXP {
            return Err("parse input must be a character vector".to_string());
        }

        let len = XLENGTH(text);
        let sources = (0..len).map(|i| {
            let elt = STRING_ELT(text, i);
            if elt == R_NaString() || elt.is_null() {
                Err("parse input contains NA".to_string())
            } else {
                Ok(CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned())
            }
        });

        parse_source_list(sources, n)
    }
}

fn parse_source_list<I>(sources: I, n: c_int) -> Result<Vec<SEXP>, String>
where
    I: Iterator<Item = Result<String, String>>,
{
    let limit = if n > 0 { Some(n as usize) } else { None };
    let mut combined = String::new();
    for source in sources {
        let line = source?;
        if line.trim().is_empty() {
            continue;
        }
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&line);
    }
    if combined.trim().is_empty() {
        return Ok(Vec::new());
    }

    crate::mainutils::source::remember_parse_context(&combined);
    let mut exprs = with_required_current_instance(|instance| {
        crate::eval::parser::parse_expressions(&combined, &mut instance.arena)
            .map_err(|err| err.to_string())
    })?;
    if let Some(limit) = limit {
        exprs.truncate(limit);
    }
    Ok(exprs)
}

fn parse_one_source(source: &str) -> Result<SEXP, String> {
    crate::mainutils::source::remember_parse_context(source);
    with_required_current_instance(|instance| {
        crate::eval::parser::parse(source, &mut instance.arena).map_err(|err| err.to_string())
    })
}

unsafe fn exprs_to_exprsxp(exprs: Vec<SEXP>) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::EXPRSXP, exprs.len() as R_xlen_t);
        let _result_guard = protect(result);
        for (i, expr) in exprs.into_iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, expr);
        }
        result
    }
}

unsafe fn parse_eval_source(source: &str, envir: SEXP) -> SEXP {
    unsafe {
        let expr = parse_one_source(source).unwrap_or_else(|message| parse_failure(message));
        let rho = if envir.is_null() || envir == R_NilValue() {
            with_required_current_instance(|instance| instance.global_env)
        } else {
            envir
        };
        crate::eval::eval::Rf_eval(expr, rho)
    }
}

unsafe fn c_string_source(s: *const c_char) -> Option<String> {
    unsafe {
        if s.is_null() {
            None
        } else {
            Some(CStr::from_ptr(s).to_string_lossy().into_owned())
        }
    }
}

unsafe fn buffer_source(buf: *const c_char, len: c_int) -> Option<String> {
    unsafe {
        if buf.is_null() || len < 0 {
            return None;
        }
        let bytes = std::slice::from_raw_parts(buf.cast::<u8>(), len as usize);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn parse_failure(message: impl Into<String>) -> ! {
    let message = message.into();
    unsafe {
        crate::mainutils::source::store_parse_error(&message, 1, 1, R_NilValue());
    }
    std::panic::panic_any(RError { message });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};
    use std::ptr;

    use crate::sexp::accessors::{REAL, TYPEOF, VECTOR_ELT, XLENGTH};

    use super::*;

    #[test]
    fn test_parse_status_constants() {
        let _session = crate::sexp::session::RSession::new();
        assert!(PARSE_OK > 0);
        assert!(PARSE_INCOMPLETE > 0);
        assert!(PARSE_ERROR > 0);
        assert!(PARSE_EOF > 0);
    }

    #[test]
    fn test_parse_vector_uses_rust_parser() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let text = Rf_mkString(CString::new("1 + 2").unwrap().as_ptr());
            let _text_guard = protect(text);
            let mut status: c_int = 0;
            let result = R_ParseVector(text, 1, &mut status, ptr::null_mut());
            assert_eq!(status, PARSE_OK);
            assert_eq!(TYPEOF(result), SEXPTYPE::EXPRSXP);
            assert_eq!(XLENGTH(result), 1);
            assert_eq!(TYPEOF(VECTOR_ELT(result, 0)), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_parse_eval_string_evaluates_source() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let source = CString::new("1 + 2").unwrap();
            let result = R_ParseEvalString(source.as_ptr(), ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(*REAL(result), 3.0);
        }
    }

    #[test]
    fn test_current_parse_line() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_CurrentParseLine(), 0);
            assert_eq!(R_ParseContext(c"x <- 1\ny".as_ptr(), 8), 0);
            assert_eq!(R_CurrentParseLine(), 2);
        }
    }

    #[test]
    fn test_parse_filename() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = R_ParseFilename();
            assert!(!s.is_null());
        }
    }

    #[test]
    fn test_parse_filename_uses_parse_error_file() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let file = Rf_mkString(c"script.R".as_ptr());
            crate::mainutils::source::store_parse_error("parse error", 1, 1, file);
            let s = R_ParseFilename();
            assert_eq!(CStr::from_ptr(s).to_str(), Ok("script.R"));
        }
    }
}
