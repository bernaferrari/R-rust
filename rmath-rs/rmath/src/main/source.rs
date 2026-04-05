#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/source.c -- parse() internal function.
//!
//! Provides:
//! - `do_parse()` for `.Internal(parse(...))` -- stub (depends on R connections,
//!   encoding, and the Bison parser grammar)
//! - `parseError()` -- real implementation that formats and reports parse errors
//!   with context from the circular parse buffer
//! - `getParseContext()` -- real implementation that extracts context lines from
//!   the circular `R_ParseContext` buffer
//! - `R_GetParseError()` / `R_SetParseError()` -- real parse error line tracking
//! - `R_GetParseErrorCol()` / `R_SetParseErrorCol()` -- real parse error column
//! - `R_GetParseErrorFile()` / `R_SetParseErrorFile()` -- real parse error file
//! - `R_GetParseContextLine()` / `R_SetParseContextLine()` -- real context line
//! - `R_ParseBuffer()` / `R_ParseConn()` -- stubs (depend on Bison parser)
//!
//! Functions depending on the Bison parser (gram.y) remain stubs:
//! `R_ParseVector` (in gram_main.rs), `R_ParseBuffer`, `R_ParseConn`, `do_parse`.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Parse error message buffer -- shared with gram_main.rs / main.rs
// ---------------------------------------------------------------------------

/// Maximum size of the parse error message buffer.
pub const PARSE_ERROR_SIZE: usize = 256;

/// Maximum size of the circular parse context buffer.
pub const PARSE_CONTEXT_SIZE: c_int = 256;

// ---------------------------------------------------------------------------
// Parse error state -- line where parse error occurred
// ---------------------------------------------------------------------------

static mut R_ParseError_val: c_int = 0;

/// Get parse error line number.
///
/// Equivalent to reading the C global `R_ParseError`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseError() -> c_int {
    unsafe { R_ParseError_val }
}

/// Set parse error line number.
///
/// Equivalent to writing the C global `R_ParseError`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetParseError(v: c_int) {
    unsafe {
        R_ParseError_val = v;
    }
}

// ---------------------------------------------------------------------------
// Parse error column
// ---------------------------------------------------------------------------

static mut R_ParseErrorCol_val: c_int = 0;

/// Get parse error column number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseErrorCol() -> c_int {
    unsafe { R_ParseErrorCol_val }
}

/// Set parse error column number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetParseErrorCol(v: c_int) {
    unsafe {
        R_ParseErrorCol_val = v;
    }
}

// ---------------------------------------------------------------------------
// Parse error file
// ---------------------------------------------------------------------------

static mut R_ParseErrorFile_val: SEXP = ptr::null_mut();

/// Get parse error file (STRSXP or SrcFile ENVSXP, or R_NilValue).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseErrorFile() -> SEXP {
    unsafe { R_ParseErrorFile_val }
}

/// Set parse error file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetParseErrorFile(v: SEXP) {
    unsafe {
        R_ParseErrorFile_val = v;
    }
}

// ---------------------------------------------------------------------------
// Parse context circular buffer
// ---------------------------------------------------------------------------

/// Circular buffer that holds recent parse context characters.
static mut R_ParseContext_buf: [u8; PARSE_CONTEXT_SIZE as usize] =
    [0u8; PARSE_CONTEXT_SIZE as usize];

/// Index of the last character written to the parse context buffer.
static mut R_ParseContextLast_val: c_int = 0;

/// Line number of the context buffer content.
static mut R_ParseContextLine_val: c_int = 0;

/// Get parse context line number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseContextLine() -> c_int {
    unsafe { R_ParseContextLine_val }
}

/// Set parse context line number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetParseContextLine(v: c_int) {
    unsafe {
        R_ParseContextLine_val = v;
    }
}

/// Get the index of the last character in the parse context buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseContextLast() -> c_int {
    unsafe { R_ParseContextLast_val }
}

/// Set the index of the last character in the parse context buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetParseContextLast(v: c_int) {
    unsafe {
        R_ParseContextLast_val = v;
    }
}

/// Get pointer to the parse context circular buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseContextBuf() -> *mut u8 {
    std::ptr::addr_of_mut!(R_ParseContext_buf) as *mut u8
}

// ---------------------------------------------------------------------------
// getParseContext -- extract context lines from the circular buffer
// ---------------------------------------------------------------------------
// Port of R's `getParseContext()` from source.c.
// This function reads backwards from `R_ParseContextLast` in the circular
// buffer, collecting characters until a NUL is found, then splits the result
// into lines and returns them as a STRSXP.

/// Get the parse context as a character vector of lines.
///
/// This is the equivalent of R's `getParseContext()` from source.c.
/// It reads the circular `R_ParseContext` buffer backwards from
/// `R_ParseContextLast`, collects the text, and splits it into lines.
pub unsafe fn getParseContext() -> SEXP {
    unsafe {
        let last = PARSE_CONTEXT_SIZE as usize;
        let mut context = [0u8; (PARSE_CONTEXT_SIZE as usize) + 1];
        context[last] = 0; // NUL-terminate at the end

        // Read backwards from R_ParseContextLast in the circular buffer
        let mut i = R_ParseContextLast_val as usize;
        let mut pos = last;
        while pos > 0 {
            i = i % (PARSE_CONTEXT_SIZE as usize);
            pos -= 1;
            context[pos] = *std::ptr::addr_of!(R_ParseContext_buf).cast::<u8>().add(i);
            if context[pos] == 0 {
                pos += 1; // skip the NUL, start from next position
                break;
            }
            i += 1;
        }

        // Count the number of lines (separated by '\n') in the context text
        let mut nn: c_int = 16; // initial allocation
        let mut ans = Rf_allocVector3(SEXPTYPE::STRSXP.0, nn as R_xlen_t);
        let mut nread: c_int = 0;

        let mut c = context[pos];
        while c != 0 {
            nread += 1;
            // Grow the vector if needed
            if nread >= nn {
                let ans2 = Rf_allocVector3(SEXPTYPE::STRSXP.0, (2 * nn) as R_xlen_t);
                for j in 0..nn {
                    SET_STRING_ELT(ans2, j as R_xlen_t, STRING_ELT(ans, j as R_xlen_t));
                }
                nn *= 2;
                Rf_unprotect(1);
                ans = ans2;
            }
            // Find the end of this line (look for '\n')
            let mut j = pos;
            loop {
                c = context[j];
                j += 1;
                if c == 0 || c == b'\n' {
                    break;
                }
            }
            // NUL-terminate at the newline position to get the line string
            context[j - 1] = 0;
            // Create the CHARSXP from this line
            let line_ptr = context[pos..].as_ptr();
            SET_STRING_ELT(
                ans,
                (nread - 1) as R_xlen_t,
                Rf_mkChar(line_ptr as *const c_char),
            );
            pos = j;
        }

        // Get rid of empty line after last newline
        if nread > 0 {
            let last_line = STRING_ELT(ans, (nread - 1) as R_xlen_t);
            if Rf_length(last_line) == 0 {
                nread -= 1;
                std::ptr::write(
                    std::ptr::addr_of_mut!(R_ParseContextLine_val),
                    R_ParseContextLine_val - 1,
                );
            }
        }

        // Create the final correctly-sized result
        let ans2 = Rf_allocVector3(SEXPTYPE::STRSXP.0, nread as R_xlen_t);
        for j in 0..nread {
            SET_STRING_ELT(ans2, j as R_xlen_t, STRING_ELT(ans, j as R_xlen_t));
        }
        Rf_unprotect(1);
        ans2
    }
}

// ---------------------------------------------------------------------------
// getParseFilename -- extract filename from R_ParseErrorFile
// ---------------------------------------------------------------------------
// Port of R's `getParseFilename()` from source.c.
// Handles both ENVSXP (srcfile object with "filename" element) and STRSXP.

unsafe fn getParseFilename(buffer: &mut [u8]) {
    unsafe {
        buffer[0] = 0;
        let file = R_ParseErrorFile_val;
        if file.is_null() {
            return;
        }

        // Check if it's an environment (SrcFile) or a string
        let sexptype = crate::sexp::accessors::TYPEOF(file);
        if sexptype == SEXPTYPE::ENVSXP.0 {
            // SrcFile environment -- look up the "filename" variable
            let sym = Rf_install(b"filename\0".as_ptr() as *const c_char);
            let filename = Rf_findVar(sym, file);
            if !filename.is_null() {
                let ftype = crate::sexp::accessors::TYPEOF(filename);
                if ftype == SEXPTYPE::STRSXP.0 && Rf_length(filename) > 0 {
                    let s = STRING_ELT(filename, 0);
                    if !s.is_null() {
                        let chars = Rf_translateChar(s);
                        if !chars.is_null() {
                            let cstr = CStr::from_ptr(chars);
                            let bytes = cstr.to_bytes();
                            let copy_len = bytes.len().min(buffer.len() - 1);
                            buffer[..copy_len].copy_from_slice(&bytes[..copy_len]);
                            buffer[copy_len] = 0;
                        }
                    }
                }
            }
        } else if sexptype == SEXPTYPE::STRSXP.0 && Rf_length(file) > 0 {
            let s = STRING_ELT(file, 0);
            if !s.is_null() {
                let chars = Rf_translateChar(s);
                if !chars.is_null() {
                    let cstr = CStr::from_ptr(chars);
                    let bytes = cstr.to_bytes();
                    let copy_len = bytes.len().min(buffer.len() - 1);
                    buffer[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    buffer[copy_len] = 0;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// tabExpand -- expand tabs to spaces in a STRSXP
// ---------------------------------------------------------------------------
// Port of R's `tabExpand()` from source.c.
// Tabs are expanded to the next 8-character tab stop.

unsafe fn tabExpand(strings: SEXP) -> SEXP {
    unsafe {
        Rf_protect(strings);
        let len = Rf_length(strings);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, len as R_xlen_t);

        let mut buf = [0u8; 200];
        for i in 0..len {
            let s = STRING_ELT(strings, i as R_xlen_t);
            if s.is_null() {
                SET_STRING_ELT(
                    result,
                    i as R_xlen_t,
                    Rf_mkChar(b"\0".as_ptr() as *const c_char),
                );
                continue;
            }
            let input_ptr = Rf_translateChar(s);
            if input_ptr.is_null() {
                SET_STRING_ELT(
                    result,
                    i as R_xlen_t,
                    Rf_mkChar(b"\0".as_ptr() as *const c_char),
                );
                continue;
            }
            let input = CStr::from_ptr(input_ptr).to_bytes();

            let mut bpos: usize = 0;
            for &ch in input.iter() {
                if bpos >= 192 {
                    break;
                }
                if ch == b'\t' {
                    loop {
                        buf[bpos] = b' ';
                        bpos += 1;
                        if (bpos & 7) == 0 || bpos >= 192 {
                            break;
                        }
                    }
                } else {
                    buf[bpos] = ch;
                    bpos += 1;
                }
            }
            buf[bpos] = 0;
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(buf.as_ptr() as *const c_char),
            );
        }
        Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// parseError -- report a parse error with context
// ---------------------------------------------------------------------------
// Port of R's `parseError()` from source.c.
// This function does NOT return -- it calls Rf_error which panics.

/// Report a parse error with context information.
///
/// This is the equivalent of R's `parseError()` from source.c.
/// It formats the error message with filename, line number, column,
/// and context lines from the parse buffer, then calls `Rf_error`.
///
/// This function does NOT return (it panics via `Rf_error`).
pub unsafe fn parseError(_call: SEXP, linenum: c_int) {
    unsafe {
        let context = Rf_protect(tabExpand(getParseContext()));
        let len = Rf_length(context);

        // Read static values through raw pointers (mutable statics)
        let parse_col = std::ptr::read(std::ptr::addr_of!(R_ParseErrorCol_val));
        let parse_line = std::ptr::read(std::ptr::addr_of!(R_ParseContextLine_val));

        let mut filename_buf = [0u8; 128];
        getParseFilename(&mut filename_buf);

        // Get the error message string
        let errmsg_ptr = R_GetParseErrorMsg();
        let errmsg = if errmsg_ptr.is_null() {
            "parse error"
        } else {
            let cstr = CStr::from_ptr(errmsg_ptr);
            cstr.to_str().unwrap_or("parse error")
        };

        if linenum != 0 {
            // Build the filename prefix
            let filename_str = if filename_buf[0] != 0 {
                let cstr = CStr::from_ptr(filename_buf.as_ptr() as *const c_char);
                let s = cstr.to_str().unwrap_or("");
                format!("{}:", s)
            } else {
                String::new()
            };

            match len {
                0 => {
                    let msg = format!("{}{}:{}: {}", filename_str, linenum, parse_col, errmsg);
                    Rf_error_cstr(&msg);
                }
                1 => {
                    let line_str = STRING_ELT_to_string(context, 0);
                    let width = format!("{}: ", parse_line).len();
                    let spaces = " ".repeat(width + parse_col as usize);
                    let msg = format!(
                        "{}{}:{}: {}\n{}: {}\n{}^",
                        filename_str, linenum, parse_col, errmsg, parse_line, line_str, spaces
                    );
                    Rf_error_cstr(&msg);
                }
                _ => {
                    let prev_line_str = STRING_ELT_to_string(context, (len - 2) as R_xlen_t);
                    let line_str = STRING_ELT_to_string(context, (len - 1) as R_xlen_t);
                    let width = format!("{}:", parse_line).len();
                    let spaces = " ".repeat(width + parse_col as usize);
                    let msg = format!(
                        "{}{}:{}: {}\n{}: {}\n{}: {}\n{}^",
                        filename_str,
                        linenum,
                        parse_col,
                        errmsg,
                        parse_line - 1,
                        prev_line_str,
                        parse_line,
                        line_str,
                        spaces
                    );
                    Rf_error_cstr(&msg);
                }
            }
        } else {
            // No line number information
            match len {
                0 => {
                    Rf_error_cstr(errmsg);
                }
                1 => {
                    let line_str = STRING_ELT_to_string(context, 0);
                    let msg = format!("{} in \"{}\"", errmsg, line_str);
                    Rf_error_cstr(&msg);
                }
                _ => {
                    let prev_line_str = STRING_ELT_to_string(context, (len - 2) as R_xlen_t);
                    let line_str = STRING_ELT_to_string(context, (len - 1) as R_xlen_t);
                    let msg = format!("{} in:\n\"{}\n{}\"", errmsg, prev_line_str, line_str);
                    Rf_error_cstr(&msg);
                }
            }
        }
        Rf_unprotect(1);
    }
}

// ---------------------------------------------------------------------------
// Helper: convert a STRING_ELT to a Rust String
// ---------------------------------------------------------------------------

unsafe fn STRING_ELT_to_string(vec: SEXP, i: R_xlen_t) -> String {
    unsafe {
        let s = STRING_ELT(vec, i);
        if s.is_null() {
            return String::new();
        }
        let chars = Rf_translateChar(s);
        if chars.is_null() {
            return String::new();
        }
        let cstr = CStr::from_ptr(chars);
        cstr.to_str().unwrap_or("").to_string()
    }
}

// ---------------------------------------------------------------------------
// Helper: call Rf_error with a Rust string
// ---------------------------------------------------------------------------

unsafe fn Rf_error_cstr(msg: &str) {
    // Rf_error takes a C format string; for a simple string with no %,
    // we can pass it directly. Use a NUL-terminated temporary.
    let cmsg = std::ffi::CString::new(msg)
        .unwrap_or_else(|_| std::ffi::CString::new("<error message contained NUL>").unwrap());
    crate::main::errors::Rf_error(cmsg.as_ptr());
}

// ---------------------------------------------------------------------------
// do_parse -- the user-level parse() entry point
// ---------------------------------------------------------------------------
// Stub: depends on R connections (getConnection, R_ParseConn), the Bison
// parser (R_ParseVector, R_ParseBuffer), and encoding handling.
//
// .Internal( parse(file, n, text, prompt, srcfile, encoding) )

/// Parse R expressions (stub).
///
/// This is the equivalent of R's `do_parse()` from source.c.
///
/// Remains a stub because it depends on:
/// - R connection infrastructure (getConnection, R_ParseConn)
/// - The Bison parser grammar (R_ParseVector, R_ParseBuffer)
/// - Encoding handling (known_to_be_latin1, known_to_be_utf8)
pub unsafe fn do_parse(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Stub: return empty expression vector
        Rf_allocVector(SEXPTYPE::EXPRSXP.0, 0)
    }
}

// ---------------------------------------------------------------------------
// R_ParseBuffer -- parse from an IoBuffer (stub)
// ---------------------------------------------------------------------------
// Stub: depends on the Bison parser (defined in gram.y).
// Called by do_parse() when reading from the console.

/// Parse from an IoBuffer (stub).
///
/// This is the equivalent of R's `R_ParseBuffer()` declared in Parse.h.
/// Remains a stub because it depends on the Bison parser.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ParseBuffer(
    _buffer: *mut std::ffi::c_void,
    _n: c_int,
    _status: *mut c_int,
    _prompt: SEXP,
    _srcfile: SEXP,
) -> SEXP {
    unsafe {
        // Stub: depends on the Bison parser
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_ParseConn -- parse from a connection (stub)
// ---------------------------------------------------------------------------
// Stub: depends on R connections and the Bison parser.
// Called by do_parse() when reading from a file connection.

/// Parse from a connection (stub).
///
/// This is the equivalent of R's `R_ParseConn()` declared in Parse.h.
/// Remains a stub because it depends on R connections and the Bison parser.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ParseConn(
    _con: *mut std::ffi::c_void,
    _n: c_int,
    _status: *mut c_int,
    _srcfile: SEXP,
) -> SEXP {
    unsafe {
        // Stub: depends on R connections and the Bison parser
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Declarations for functions from other modules (sexp constructors, etc.)
// ---------------------------------------------------------------------------

unsafe extern "C" {
    /// Allocate an R vector of given type and length.
    fn Rf_allocVector(sexptype: c_int, length: c_int) -> SEXP;

    /// Allocate an R vector of given type and xlen_t length.
    fn Rf_allocVector3(sexptype: c_int, length: R_xlen_t) -> SEXP;

    /// Create a CHARSXP from a C string.
    fn Rf_mkChar(s: *const c_char) -> SEXP;

    /// Get element i from a character vector.
    fn STRING_ELT(x: SEXP, i: R_xlen_t) -> SEXP;

    /// Set element i of a character vector.
    fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP);

    /// Get the length of an SEXP.
    fn Rf_length(x: SEXP) -> c_int;

    /// Translate a CHARSXP to a native C string.
    fn Rf_translateChar(x: SEXP) -> *const c_char;

    /// Protect an SEXP from garbage collection.
    fn Rf_protect(x: SEXP) -> SEXP;

    /// Unprotect n SEXPs.
    fn Rf_unprotect(n: c_int);

    /// Install a symbol name.
    fn Rf_install(name: *const c_char) -> SEXP;

    /// Find a variable in an environment.
    fn Rf_findVar(symbol: SEXP, rho: SEXP) -> SEXP;
}

unsafe extern "C" {
    /// Get the parse error message string pointer.
    ///
    /// Defined in main/main.rs.
    fn R_GetParseErrorMsg() -> *const c_char;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_parse_error_state() {
        unsafe {
            R_SetParseError(42);
            assert_eq!(R_GetParseError(), 42);
            R_SetParseError(0);
            assert_eq!(R_GetParseError(), 0);
        }
    }

    #[test]
    fn test_parse_error_col() {
        unsafe {
            R_SetParseErrorCol(10);
            assert_eq!(R_GetParseErrorCol(), 10);
            R_SetParseErrorCol(0);
            assert_eq!(R_GetParseErrorCol(), 0);
        }
    }

    #[test]
    fn test_parse_error_file() {
        unsafe {
            assert!(R_GetParseErrorFile().is_null());
            // Can't test setting a real SEXP without the arena, but test null
            R_SetParseErrorFile(ptr::null_mut());
            assert!(R_GetParseErrorFile().is_null());
        }
    }

    #[test]
    fn test_parse_context_line() {
        unsafe {
            R_SetParseContextLine(5);
            assert_eq!(R_GetParseContextLine(), 5);
            R_SetParseContextLine(0);
            assert_eq!(R_GetParseContextLine(), 0);
        }
    }

    #[test]
    fn test_parse_context_last() {
        unsafe {
            R_SetParseContextLast(100);
            assert_eq!(R_GetParseContextLast(), 100);
            R_SetParseContextLast(0);
            assert_eq!(R_GetParseContextLast(), 0);
        }
    }

    #[test]
    fn test_parse_context_buffer() {
        unsafe {
            let buf = R_GetParseContextBuf();
            assert!(!buf.is_null());
            // Write some test data into the circular buffer
            let test_str = b"hello\n";
            for (i, &ch) in test_str.iter().enumerate() {
                *buf.add(i) = ch;
            }
            // Set R_ParseContextLast to point to the end
            R_SetParseContextLast(test_str.len() as c_int - 1);
            assert_eq!(R_GetParseContextLast(), test_str.len() as c_int - 1);
            // Verify the data was written
            assert_eq!(*buf.add(0), b'h');
            assert_eq!(*buf.add(4), b'o');
            assert_eq!(*buf.add(5), b'\n');
        }
    }

    #[test]
    fn test_parse_context_size() {
        assert_eq!(PARSE_CONTEXT_SIZE, 256);
    }

    #[test]
    fn test_parse_error_size() {
        assert_eq!(PARSE_ERROR_SIZE, 256);
    }

    #[test]
    fn test_parse_buffer_stub() {
        unsafe {
            let mut status: c_int = 0;
            let result = R_ParseBuffer(
                ptr::null_mut(),
                1,
                &mut status,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_parse_conn_stub() {
        unsafe {
            let mut status: c_int = 0;
            let result = R_ParseConn(ptr::null_mut(), 1, &mut status, ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }
}
