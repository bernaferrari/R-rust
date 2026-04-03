#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/source.c — parse() internal function.
//!
//! Provides do_parse() for .Internal(parse(...)).
//! Currently stubbed since it depends on the parser (gram.y).

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

/// Parse R expressions (stub).
///
/// This is the equivalent of R's `do_parse()` from source.c.
///
/// .Internal( parse(file, n, text, prompt, srcfile, encoding) )
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_parse(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Stub: return empty expression vector
        R_NilValue()
    }
}

/// Parse error handler (stub).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn parseError(_call: SEXP, _linenum: c_int) {
    // Stub: does nothing (error handling is via panic)
}

/// Get the parse context (stub).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getParseContext() -> SEXP {
    unsafe { R_NilValue() }
}

/// Parse context buffer size.
pub const PARSE_CONTEXT_SIZE: c_int = 256;

/// Parse error state (stub).
static mut R_ParseError_val: c_int = 0;

/// Get parse error line number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseError() -> c_int {
    unsafe { R_ParseError_val }
}

/// Set parse error line number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetParseError(v: c_int) {
    unsafe {
        R_ParseError_val = v;
    }
}

/// Parse error column (stub).
static mut R_ParseErrorCol_val: c_int = 0;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseErrorCol() -> c_int {
    unsafe { R_ParseErrorCol_val }
}

/// Parse error file (stub).
static mut R_ParseErrorFile_val: SEXP = ptr::null_mut();

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseErrorFile() -> SEXP {
    unsafe { R_ParseErrorFile_val }
}

/// Parse context line (stub).
static mut R_ParseContextLine_val: c_int = 0;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseContextLine() -> c_int {
    unsafe { R_ParseContextLine_val }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_parse_stub() {
        unsafe {
            let result = do_parse(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_parse_error_state() {
        unsafe {
            R_SetParseError(42);
            assert_eq!(R_GetParseError(), 42);
        }
    }
}
