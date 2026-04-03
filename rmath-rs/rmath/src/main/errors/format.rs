#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Formatting utilities for error/warning messages.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::Ordering;

use crate::attrib_core::R_ClassSymbol;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals;

use super::{BUFSIZE, R_WARN_LENGTH};

// ---------------------------------------------------------------------------
// C library bindings
// ---------------------------------------------------------------------------

unsafe extern "C" {
    #[link_name = "vsnprintf"]
    fn vsnprintf_c(buf: *mut c_char, size: usize, format: *const c_char, ap: *mut c_void) -> c_int;
}

// ---------------------------------------------------------------------------
// Display width utility
// ---------------------------------------------------------------------------

/// Compute the display width of a string in columns.
pub fn wd(buf: &str) -> usize {
    buf.chars().count()
}

/// Display width from C string.
pub(crate) unsafe fn wd_c(s: *const c_char) -> usize {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let str = CStr::from_ptr(s).to_str().unwrap_or("");
        wd(str)
    }
}

// ---------------------------------------------------------------------------
// Buffer formatting helpers
// ---------------------------------------------------------------------------

/// Format string helper -- truncates at BUFSIZE, null-terminates.
pub(crate) fn format_to_buf(buf: &mut [u8; BUFSIZE + 1], fmt: &str) -> (usize, bool) {
    let mut truncated = false;
    let bytes = fmt.as_bytes();
    if bytes.len() >= BUFSIZE {
        // Find a safe truncation point (don't split multi-byte chars)
        let mut end = BUFSIZE - 1;
        while end > 0 && (bytes[end] & 0xC0) == 0x80 {
            end -= 1;
        }
        buf[..end].copy_from_slice(&bytes[..end]);
        buf[end] = 0;
        truncated = true;
    } else {
        buf[..bytes.len()].copy_from_slice(bytes);
        buf[bytes.len()] = 0;
    }
    (bytes.len(), truncated)
}

/// Append to buf, ensuring we don't overflow and don't split multi-byte chars.
pub(crate) fn bufcat(buf: &mut [u8], txt: &str) {
    let cur_len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let remaining = buf.len().saturating_sub(cur_len);
    if remaining == 0 {
        return;
    }
    let bytes = txt.as_bytes();
    let copy_len = bytes.len().min(remaining.saturating_sub(1));
    buf[cur_len..cur_len + copy_len].copy_from_slice(&bytes[..copy_len]);
    buf[cur_len + copy_len] = 0;
}

/// Append "[... truncated]" if needed.
pub(crate) fn print_trunc(buf: &mut [u8; BUFSIZE + 1], truncated: bool) {
    if truncated {
        let cur_len = buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE);
        let msg = " [... truncated]";
        if cur_len + msg.len() < BUFSIZE {
            bufcat(buf, msg);
        }
    }
}

/// Format a printf-style format string with variadic arguments into a Rust String.
/// Uses C's vsnprintf via FFI.
pub(crate) unsafe fn format_varargs(format: *const c_char, ap: *mut c_void) -> String {
    unsafe {
        if format.is_null() {
            return String::new();
        }
        if ap.is_null() {
            return CStr::from_ptr(format).to_str().unwrap_or("").to_string();
        }
        // First pass: determine required size
        let needed = vsnprintf_c(ptr::null_mut(), 0, format, ap);
        if needed < 0 {
            let fallback = CStr::from_ptr(format).to_str().unwrap_or("");
            return fallback.to_string();
        }
        let needed = needed as usize + 1;
        let mut buf = vec![0u8; needed];
        vsnprintf_c(buf.as_mut_ptr() as *mut c_char, needed, format, ap);
        if let Some(pos) = buf.iter().position(|&b| b == 0) {
            buf.truncate(pos);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }
}

/// Format a printf-style format string with variadic arguments into a buffer.
/// Returns (formatted_string, was_truncated).
pub(crate) unsafe fn format_varargs_to_buf(
    format: *const c_char,
    ap: *mut c_void,
) -> (String, bool) {
    unsafe {
        if format.is_null() {
            return (String::new(), false);
        }
        if ap.is_null() {
            let s = CStr::from_ptr(format).to_str().unwrap_or("").to_string();
            return (s, false);
        }
        let psize = std::cmp::min(BUFSIZE, R_WARN_LENGTH.load(Ordering::Relaxed) as usize) + 1;
        let mut buf = vec![0u8; psize];
        let pval = vsnprintf_c(buf.as_mut_ptr() as *mut c_char, psize, format, ap);
        let truncated = pval >= psize as i32;
        if psize > 0 {
            buf[psize - 1] = 0;
        }
        if let Some(pos) = buf.iter().position(|&b| b == 0) {
            buf.truncate(pos);
        }
        let s = String::from_utf8_lossy(&buf).into_owned();
        (s, truncated)
    }
}

// ---------------------------------------------------------------------------
// String utilities
// ---------------------------------------------------------------------------

/// Rstrncpy: like strncpy, but guaranteed to null-terminate.
pub(crate) fn r_strncpy(dest: &mut [u8], src: &[u8], n: usize) {
    let copy_len = src.len().min(n);
    if copy_len > 0 {
        dest[..copy_len].copy_from_slice(&src[..copy_len]);
    }
    if n > 0 && copy_len < dest.len() {
        dest[copy_len] = 0;
    }
}

/// ERRBUFCAT macro equivalent.
#[allow(unused_macros)]
macro_rules! ERRBUFCAT {
    ($buf:expr, $txt:expr) => {{
        let cur_len = $buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE);
        let remaining = BUFSIZE.saturating_sub(cur_len);
        if remaining > 0 {
            let bytes = $txt.as_bytes();
            let copy_len = bytes.len().min(remaining.saturating_sub(1));
            $buf[cur_len..cur_len + copy_len].copy_from_slice(&bytes[..copy_len]);
            $buf[cur_len + copy_len] = 0;
        }
    }};
}

// ---------------------------------------------------------------------------
// Message formatting helpers
// ---------------------------------------------------------------------------

/// Count the number of % escapes in a format string.
pub(crate) fn count_format_args(s: &str) -> usize {
    let mut count = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('%') => {
                    chars.next();
                }
                Some(&c)
                    if !matches!(
                        c,
                        's' | 'd' | 'f' | 'g' | 'e' | 'i' | 'o' | 'u' | 'x' | 'X' | 'c' | 'p' | 'l'
                    ) => {}
                Some(_) => {
                    count += 1;
                }
                None => {}
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Internal helpers (pub(crate) for use across submodules)
// ---------------------------------------------------------------------------
// All functions below are SAFE wrappers -- callers do not need `unsafe` blocks.
// The unsafe operations are contained within each function body.

/// Delegates to the real GetOption1 implementation in options.rs.
pub(crate) fn GetOption1(sym: SEXP) -> SEXP {
    unsafe { crate::main::options::GetOption1(sym) }
}

/// Check if an SEXP is a function (CLOSXP or BUILTINSXP).
pub(crate) fn isFunction(s: SEXP) -> c_int {
    unsafe {
        let t = TYPEOF(s);
        (t == SEXPTYPE::CLOSXP.0 || t == SEXPTYPE::BUILTINSXP.0 || t == SEXPTYPE::SPECIALSXP.0)
            as c_int
    }
}

/// Check if an SEXP is a language object.
pub(crate) fn isLanguage(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::LANGSXP.0) as c_int }
}

/// Check if an SEXP is an expression.
pub(crate) fn isExpression(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::EXPRSXP.0) as c_int }
}

/// Check if an SEXP is a string vector.
pub(crate) fn isString(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::STRSXP.0) as c_int }
}

/// Check if an SEXP is a logical vector.
pub(crate) fn isLogical(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::LGLSXP.0) as c_int }
}

/// Check if an SEXP is an integer vector.
pub(crate) fn isInteger(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::INTSXP.0) as c_int }
}

/// Check if an SEXP is a real vector.
pub(crate) fn isReal(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::REALSXP.0) as c_int }
}

/// Convert SEXP to logical (simplified).
pub(crate) fn asLogical(s: SEXP) -> c_int {
    unsafe {
        if isLogical(s) != 0 && LENGTH(s) >= 1 {
            *LOGICAL(s).offset(0)
        } else if isInteger(s) != 0 && LENGTH(s) >= 1 {
            *INTEGER(s).offset(0)
        } else if isReal(s) != 0 && LENGTH(s) >= 1 {
            if *REAL(s).offset(0) == 0.0_f64 { 0 } else { 1 }
        } else {
            crate::sexp::ffi::NA_INTEGER
        }
    }
}

/// Check if an SEXP is NULL.
pub(crate) fn isNull(s: SEXP) -> c_int {
    unsafe { (s.is_null() || TYPEOF(s) == SEXPTYPE::NILSXP.0) as c_int }
}

/// Check if a string SEXP is valid (non-NA).
pub(crate) fn isValidString(s: SEXP) -> c_int {
    unsafe {
        if isString(s) == 0 || LENGTH(s) < 1 {
            return 0;
        }
        let elt = STRING_ELT(s, 0);
        if elt.is_null() {
            return 0;
        }
        1 // Simplified -- full version checks for NA_STRING
    }
}

/// Get C string from CHARSXP (simplified).
pub(crate) fn CHAR_local(s: SEXP) -> *const c_char {
    unsafe {
        if s.is_null() || TYPEOF(s) != SEXPTYPE::CHARSXP.0 {
            return b"\0" as *const u8 as *const c_char;
        }
        crate::sexp::accessors::CHAR(s)
    }
}

/// Get C string from STRING_ELT.
pub(crate) fn translateChar(s: SEXP) -> *const c_char {
    if s.is_null() {
        return b"\0" as *const u8 as *const c_char;
    }
    CHAR_local(s)
}

/// Check argument arity (simplified).
/// Note: This uses a fully-qualified path to avoid circular imports between
/// format.rs and error.rs.
pub(crate) fn checkArity(op: SEXP, args: SEXP) {
    unsafe {
        crate::main::errors::error::Rf_checkArityCall(op, args, getCurrentCall());
    }
}

/// Create a scalar integer.
pub(crate) fn ScalarInteger(x: c_int) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::INTSXP.0, 1);
        if !s.is_null() {
            *INTEGER(s).offset(0) = x;
            (*s).sxpinfo.set_scalar(true);
        }
        s
    }
}

/// Create a scalar logical.
pub(crate) fn ScalarLogical(x: c_int) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::LGLSXP.0, 1);
        if !s.is_null() {
            *LOGICAL(s).offset(0) = x;
            (*s).sxpinfo.set_scalar(true);
        }
        s
    }
}

/// Get/set class attribute (simplified).
pub(crate) fn classgets(x: SEXP, klass: SEXP) -> SEXP {
    unsafe {
        crate::attrib_core::setAttrib(x, R_ClassSymbol(), klass);
        x
    }
}

/// Wrapper for getAttrib using the real implementation.
#[inline]
pub(crate) fn getAttrib_wrap(x: SEXP, which: SEXP) -> SEXP {
    unsafe { crate::attrib_core::getAttrib(x, which) }
}

/// Wrapper for setAttrib using the real implementation.
#[inline]
pub(crate) fn setAttrib_wrap(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        crate::attrib_core::setAttrib(x, which, value);
    }
}

/// Get the number of arguments (length of pairlist).
pub(crate) fn length(x: SEXP) -> c_int {
    unsafe {
        let mut count: c_int = 0;
        let mut p = x;
        while !p.is_null() && TYPEOF(p) == SEXPTYPE::LISTSXP.0 {
            count += 1;
            p = CDR(p);
        }
        count
    }
}

// ---------------------------------------------------------------------------
// getCurrentCall (simplified)
// ---------------------------------------------------------------------------

/// Get the current call from the context stack.
pub(crate) fn getCurrentCall() -> SEXP {
    unsafe {
        let ctx = crate::sexp::context::R_GlobalContext();
        if ctx.is_null() {
            return globals::R_NilValue();
        }
        let c = &*ctx;
        // Skip CTXT_BUILTIN contexts
        if (c.callflag & crate::sexp::context::ctxt_flags::CTXT_BUILTIN) != 0 {
            if !c.nextcontext.is_null() {
                let next = &*c.nextcontext;
                return if next.call.is_null() {
                    globals::R_NilValue()
                } else {
                    next.call
                };
            }
        }
        if c.call.is_null() {
            globals::R_NilValue()
        } else {
            c.call
        }
    }
}

/// findCall: find the function context's call for error reporting.
pub(crate) fn findCall() -> SEXP {
    unsafe {
        let ctx = crate::sexp::context::R_GlobalContext();
        if ctx.is_null() {
            return globals::R_NilValue();
        }
        let mut c = (*ctx).nextcontext;
        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION) != 0 {
                return if ctx_ref.call.is_null() {
                    globals::R_NilValue()
                } else {
                    ctx_ref.call
                };
            }
            c = ctx_ref.nextcontext;
        }
        globals::R_NilValue()
    }
}
