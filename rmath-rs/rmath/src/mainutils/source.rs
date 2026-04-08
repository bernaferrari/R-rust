#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/source.c — parse() internal function.
//!
//! Provides do_parse() for .Internal(parse(...)).
//! Currently stubbed since it depends on the parser (gram.y).

use std::cell::Cell;
use std::os::raw::c_int;
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

/// Parse R expressions (stub).
///
/// This is the equivalent of R's `do_parse()` from source.c.
///
/// .Internal( parse(file, n, text, prompt, srcfile, encoding) )
pub unsafe fn do_parse(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Stub: return empty expression vector
        R_NilValue()
    }
}

/// Parse error handler (stub).
pub unsafe fn parseError(_call: SEXP, _linenum: c_int) {
    // Stub: does nothing (error handling is via panic)
}

/// Get the parse context (stub).
pub unsafe fn getParseContext() -> SEXP {
    unsafe { R_NilValue() }
}

/// Parse context buffer size.
pub const PARSE_CONTEXT_SIZE: c_int = 256;

thread_local! { static R_ParseError_val: Cell<c_int> = Cell::new(0); }

pub unsafe fn R_GetParseError() -> c_int {
    R_ParseError_val.with(|v| v.get())
}

pub unsafe fn R_SetParseError(val: c_int) {
    R_ParseError_val.with(|v| v.set(val));
}

thread_local! { static R_ParseErrorCol_val: Cell<c_int> = Cell::new(0); }

pub unsafe fn R_GetParseErrorCol() -> c_int {
    R_ParseErrorCol_val.with(|v| v.get())
}

thread_local! { static R_ParseErrorFile_val: Cell<SEXP> = Cell::new(ptr::null_mut()); }

pub unsafe fn R_GetParseErrorFile() -> SEXP {
    R_ParseErrorFile_val.with(|v| v.get())
}

thread_local! { static R_ParseContextLine_val: Cell<c_int> = Cell::new(0); }

pub unsafe fn R_GetParseContextLine() -> c_int {
    R_ParseContextLine_val.with(|v| v.get())
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
        }
        assert_eq!(unsafe { R_GetParseError() }, 42);
    }
}
