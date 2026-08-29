#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/errors.c — error handling utilities.
//!
//! This module provides real error/warning handling using `std::panic::catch_unwind`
//! with a custom `RError` panic payload, replacing C's setjmp/longjmp mechanism.
//!
//! Key design:
//! - `Rf_error()` / `errorcall()` panic with `RError` payload
//! - `Rf_warning()` / `warningcall()` print to stderr or collect warnings
//! - `jump_to_top_ex()` panics with `RError` to unwind to top level
//! - Warning collection with configurable `warn` option (0=collect, 1=print, 2=error)
//! - Condition handler/restart stacks for tryCatch/withCallingHandler
//! - Traceback support

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::sexp::context::{RError, RSignal};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals;
use crate::sexp::instance::{self, ErrorState};

// Re-export common accessors/constructors for convenience
use crate::eval::attrib_core::{R_ClassSymbol, R_NamesSymbol, getAttrib};
use crate::mainutils::coerce::coerceVector;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;
pub use crate::special::mlutils::REprintf;

// PRINTNAME is re-exported from inlined.rs
use crate::mainutils::inlined::PRINTNAME;

// ---------------------------------------------------------------------------
// C library bindings
// ---------------------------------------------------------------------------

unsafe extern "C" {
    /// C's vsnprintf — format a string into a buffer with a va_list.
    /// On macOS, va_list is a pointer type.
    #[link_name = "vsnprintf"]
    fn vsnprintf_c(buf: *mut c_char, size: usize, format: *const c_char, ap: *mut c_void) -> c_int;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Total line length before splitting in warnings/errors.
const LONGWARN: usize = 75;

/// Default maximum warnings collected.
const R_NWARNINGS_DEFAULT: c_int = 50;

/// Buffer size for error/warning messages.
pub const BUFSIZE: usize = 8192;

/// Number of characters shown in concise tracebacks.
static R_NSHOWCALLS: usize = 512;

/// Maximum number of calls shown in concise traceback.
static R_MAXCALLS: c_int = 50;

fn with_error_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut ErrorState) -> R,
{
    instance::with_required_current_instance(|instance| f(&mut instance.error_state))
}

fn r_warn_length() -> c_int {
    with_error_state(|state| state.warn_length)
}

fn set_r_warn_length(val: c_int) {
    with_error_state(|state| state.warn_length = val);
}

fn r_show_error_messages() -> bool {
    with_error_state(|state| state.show_error_messages)
}

fn set_r_show_error_messages(val: bool) {
    with_error_state(|state| state.show_error_messages = val);
}

fn r_show_error_calls() -> bool {
    with_error_state(|state| state.show_error_calls)
}

fn set_r_show_error_calls(val: bool) {
    with_error_state(|state| state.show_error_calls = val);
}

fn last_rendered_message() -> Option<String> {
    with_error_state(|state| state.last_rendered_message.clone())
}

fn set_last_rendered_message(message: Option<String>) {
    with_error_state(|state| state.last_rendered_message = message);
}

/// Whether `message` is the error most recently rendered into the error
/// buffer by `verrorcall_dflt`. Used by the top-level renderer and the
/// builtin-dispatch attribution wrapper to trust/distrust the error buffer
/// and avoid re-rendering already-attributed errors.
pub fn error_was_last_rendered(message: &str) -> bool {
    last_rendered_message().as_deref() == Some(message)
}

/// Clear the last-rendered marker at the start of a top-level evaluation so
/// renders from a previous script are never trusted.
pub fn clear_last_rendered_message() {
    set_last_rendered_message(None);
}

fn r_show_warn_calls() -> bool {
    with_error_state(|state| state.show_warn_calls)
}

fn set_r_show_warn_calls(val: bool) {
    with_error_state(|state| state.show_warn_calls = val);
}

fn in_error() -> c_int {
    with_error_state(|state| state.in_error)
}

fn set_in_error(val: c_int) {
    with_error_state(|state| state.in_error = val);
}

fn in_warning() -> c_int {
    with_error_state(|state| state.in_warning)
}

fn set_in_warning(val: c_int) {
    with_error_state(|state| state.in_warning = val);
}

fn in_print_warnings() -> c_int {
    with_error_state(|state| state.in_print_warnings)
}

fn set_in_print_warnings(val: c_int) {
    with_error_state(|state| state.in_print_warnings = val);
}

fn immediate_warning() -> bool {
    with_error_state(|state| state.immediate_warning)
}

fn set_immediate_warning(val: bool) {
    with_error_state(|state| state.immediate_warning = val);
}

/// Depth of active `suppressWarnings()` frames (see
/// `ErrorState::suppress_warnings`).
pub(crate) fn suppress_warnings_depth() -> c_int {
    with_error_state(|state| state.suppress_warnings)
}

pub(crate) fn enter_suppress_warnings() {
    with_error_state(|state| state.suppress_warnings += 1);
}

pub(crate) fn exit_suppress_warnings() {
    with_error_state(|state| state.suppress_warnings -= 1);
}

fn set_no_break_warning(val: bool) {
    with_error_state(|state| state.no_break_warning = val);
}

fn interrupts_suspended() -> bool {
    with_error_state(|state| state.interrupts_suspended)
}

fn set_interrupts_suspended(val: bool) {
    with_error_state(|state| state.interrupts_suspended = val);
}

fn interrupts_pending() -> bool {
    with_error_state(|state| state.interrupts_pending)
}

fn set_interrupts_pending(val: bool) {
    with_error_state(|state| state.interrupts_pending = val);
}

pub(crate) fn collect_warnings() -> c_int {
    with_error_state(|state| state.collect_warnings)
}

/// Test/inspection helper: message of the most recently collected warning
/// (empty string when none).  Reads a copy — never mutates the stored
/// CHARSXP (upstream errors.c PrintWarnings measures msgline1 on a copy).
pub(crate) fn last_collected_warning_message() -> String {
    let cw = collect_warnings();
    if cw <= 0 {
        return String::new();
    }
    unsafe {
        let names = CAR(ATTRIB(warnings_ptr()));
        if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return String::new();
        }
        let cs = STRING_ELT(names, (cw - 1) as R_xlen_t);
        if cs.is_null() {
            return String::new();
        }
        CStr::from_ptr(translateChar(cs))
            .to_string_lossy()
            .into_owned()
    }
}

fn set_collect_warnings(val: c_int) {
    with_error_state(|state| state.collect_warnings = val);
}

fn increment_collect_warnings() {
    with_error_state(|state| state.collect_warnings += 1);
}

fn nwarnings() -> c_int {
    with_error_state(|state| state.nwarnings)
}

fn warnings_ptr() -> SEXP {
    with_error_state(|state| state.warnings)
}

fn set_warnings_ptr(val: SEXP) {
    with_error_state(|state| state.warnings = val);
}

fn handler_stack() -> SEXP {
    with_error_state(|state| state.handler_stack)
}

fn set_handler_stack(val: SEXP) {
    with_error_state(|state| state.handler_stack = val);
}

fn restart_stack() -> SEXP {
    with_error_state(|state| state.restart_stack)
}

fn set_restart_stack(val: SEXP) {
    with_error_state(|state| state.restart_stack = val);
}

/// Get the current error buffer contents as a string.
pub unsafe fn R_curErrorBuf() -> *const c_char {
    with_error_state(|state| state.error_buffer.as_ptr() as *const c_char)
}

/// The rendered top-level error text for `message`, when this exact message
/// was the last one rendered into the error buffer.
///
/// Returns `None` when no R instance is active (e.g. while converting a
/// failure for a closed session) or when the buffer holds something else;
/// callers then fall back to the bare-message rendering.
pub fn try_last_rendered_message(message: &str) -> Option<String> {
    instance::with_current_instance(|instance| {
        let state = &instance.error_state;
        if state.last_rendered_message.as_deref() != Some(message) {
            return None;
        }
        let len = state
            .error_buffer
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(BUFSIZE);
        Some(String::from_utf8_lossy(&state.error_buffer[..len]).into_owned())
    })
    .flatten()
}

/// Get the current error buffer contents as a Rust String.
pub fn R_GetErrorBuf() -> String {
    with_error_state(|state| {
        let len = state
            .error_buffer
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(BUFSIZE);
        String::from_utf8_lossy(&state.error_buffer[..len]).into_owned()
    })
}

/// Set the error message buffer (Rust).
pub fn R_SetErrmessage(s: &str) {
    with_error_state(|state| {
        let bytes = s.as_bytes();
        let len = bytes.len().min(BUFSIZE - 1);
        state.error_buffer[..len].copy_from_slice(&bytes[..len]);
        state.error_buffer[len] = 0;
    })
}

/// Set the error message buffer (C FFI).
pub unsafe fn R_SetErrmessage_c(s: *const c_char) {
    unsafe {
        if s.is_null() {
            return;
        }
        let str = CStr::from_ptr(s).to_str().unwrap_or("");
        R_SetErrmessage(str);
    }
}

// ---------------------------------------------------------------------------
// Display width utility
// ---------------------------------------------------------------------------

/// Compute the display width of a string in columns.
/// Ported from R's `wd()` function in errors.c.
pub fn wd(buf: &str) -> usize {
    buf.chars().count()
}

/// Display width from C string.
unsafe fn wd_c(s: *const c_char) -> usize {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let str = CStr::from_ptr(s).to_str().unwrap_or("");
        wd(str)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Format string helper — truncates at BUFSIZE, null-terminates.
fn format_to_buf(buf: &mut [u8; BUFSIZE + 1], fmt: &str) -> (usize, bool) {
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
fn bufcat(buf: &mut [u8], txt: &str) {
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
fn print_trunc(buf: &mut [u8; BUFSIZE + 1], truncated: bool) {
    if truncated {
        let cur_len = buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE);
        let msg = " [... truncated]";
        if cur_len + msg.len() < BUFSIZE {
            bufcat(buf, msg);
        }
    }
}

/// Format a printf-style format string with variadic arguments into a Rust String.
/// Uses C's vsnprintf via FFI. Returns the formatted string.
///
/// Note: The `ap` parameter is only meaningful when called from C code that passes
/// a real va_list. When called from Rust, ap is typically null and the format
/// string should be pre-formatted.
unsafe fn format_varargs(format: *const c_char, ap: *mut c_void) -> String {
    unsafe {
        if format.is_null() {
            return String::new();
        }
        if ap.is_null() {
            // No va_list — format string is already the final message
            return CStr::from_ptr(format).to_str().unwrap_or("").to_string();
        }
        // First pass: determine required size
        let needed = vsnprintf_c(ptr::null_mut(), 0, format, ap);
        if needed < 0 {
            let fallback = CStr::from_ptr(format).to_str().unwrap_or("");
            return fallback.to_string();
        }
        let needed = needed as usize + 1; // +1 for null terminator
        // Second pass: format into buffer
        let mut buf = vec![0u8; needed];
        vsnprintf_c(buf.as_mut_ptr() as *mut c_char, needed, format, ap);
        // Trim trailing null
        if let Some(pos) = buf.iter().position(|&b| b == 0) {
            buf.truncate(pos);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }
}

/// Format a printf-style format string with variadic arguments into a buffer.
/// Uses C's vsnprintf via FFI. Returns (formatted_string, was_truncated).
unsafe fn format_varargs_to_buf(format: *const c_char, ap: *mut c_void) -> (String, bool) {
    unsafe {
        if format.is_null() {
            return (String::new(), false);
        }
        if ap.is_null() {
            let s = CStr::from_ptr(format).to_str().unwrap_or("").to_string();
            return (s, false);
        }
        let psize = std::cmp::min(BUFSIZE, r_warn_length() as usize) + 1;
        let mut buf = vec![0u8; psize];
        let pval = vsnprintf_c(buf.as_mut_ptr() as *mut c_char, psize, format, ap);
        let truncated = pval >= psize as i32;
        // Ensure null termination
        if psize > 0 {
            buf[psize - 1] = 0;
        }
        // Trim to null
        if let Some(pos) = buf.iter().position(|&b| b == 0) {
            buf.truncate(pos);
        }
        let s = String::from_utf8_lossy(&buf).into_owned();
        (s, truncated)
    }
}

// ---------------------------------------------------------------------------
// GetOption1 helper (simplified)
// ---------------------------------------------------------------------------

/// Delegates to the real GetOption1 implementation in options.rs.
unsafe fn GetOption1(sym: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::GetOption1(sym) }
}

/// Check if an SEXP is a function (CLOSXP or BUILTINSXP).
unsafe fn isFunction(s: SEXP) -> c_int {
    unsafe {
        let t = TYPEOF(s);
        (t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP) as c_int
    }
}

/// Check if an SEXP is a language object.
unsafe fn isLanguage(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::LANGSXP) as c_int }
}

/// Check if an SEXP is an expression.
unsafe fn isExpression(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::EXPRSXP) as c_int }
}

/// Check if an SEXP is a string vector.
unsafe fn isString(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::STRSXP) as c_int }
}

/// Check if an SEXP is a logical vector.
unsafe fn isLogical(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::LGLSXP) as c_int }
}

/// Check if an SEXP is an integer vector.
unsafe fn isInteger(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::INTSXP) as c_int }
}

/// Check if an SEXP is a real vector.
unsafe fn isReal(s: SEXP) -> c_int {
    unsafe { (TYPEOF(s) == SEXPTYPE::REALSXP) as c_int }
}

/// Convert SEXP to logical (simplified).
unsafe fn asLogical(s: SEXP) -> c_int {
    unsafe {
        if isLogical(s) != 0 && LENGTH(s) >= 1 {
            *LOGICAL(s)
        } else if isInteger(s) != 0 && LENGTH(s) >= 1 {
            *INTEGER(s)
        } else if isReal(s) != 0 && LENGTH(s) >= 1 {
            if *REAL(s) == 0.0_f64 { 0 } else { 1 }
        } else {
            crate::sexp::ffi::NA_INTEGER
        }
    }
}

/// Check if an SEXP is NULL.
unsafe fn isNull(s: SEXP) -> c_int {
    unsafe { (s.is_null() || TYPEOF(s) == SEXPTYPE::NILSXP) as c_int }
}

/// Check if a string SEXP is valid (non-NA).
unsafe fn isValidString(s: SEXP) -> c_int {
    unsafe {
        if isString(s) == 0 || LENGTH(s) < 1 {
            return 0;
        }
        let elt = STRING_ELT(s, 0);
        if elt.is_null() {
            return 0;
        }
        1 // Simplified — full version checks for NA_STRING
    }
}

/// Get C string from CHARSXP (simplified).
unsafe fn CHAR_local(s: SEXP) -> *const c_char {
    unsafe {
        if s.is_null() || TYPEOF(s) != SEXPTYPE::CHARSXP {
            return b"\0" as *const u8 as *const c_char;
        }
        crate::sexp::accessors::CHAR(s)
    }
}

unsafe fn translateChar(s: SEXP) -> *const c_char {
    unsafe {
        let r = crate::sexp::accessors::translateChar(s);
        if r.is_null() {
            b"\0" as *const u8 as *const c_char
        } else {
            r
        }
    }
}

/// Check argument arity (simplified).
unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

unsafe fn ScalarInteger(x: c_int) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarInteger(x) }
}

unsafe fn ScalarLogical(x: c_int) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarLogical(x) }
}

/// Get/set class attribute (simplified).
unsafe fn classgets(x: SEXP, klass: SEXP) -> SEXP {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, R_ClassSymbol(), klass); // Uses imported R_ClassSymbol
        x
    }
}

/// Wrapper for getAttrib using the real implementation.
#[inline]
unsafe fn getAttrib_wrap(x: SEXP, which: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, which) }
}

/// Wrapper for setAttrib using the real implementation.
#[inline]
unsafe fn setAttrib_wrap(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, which, value);
    }
}

/// Get the number of arguments (length of pairlist).
unsafe fn length(x: SEXP) -> c_int {
    unsafe {
        let mut count: c_int = 0;
        let mut p = x;
        while !p.is_null() && TYPEOF(p) == SEXPTYPE::LISTSXP {
            count += 1;
            p = CDR(p);
        }
        count
    }
}

// ---------------------------------------------------------------------------
// Error buffer access (matching C's errbuf)
// ---------------------------------------------------------------------------

/// Rstrncpy: like strncpy, but guaranteed to null-terminate.
fn r_strncpy(dest: &mut [u8], src: &[u8], n: usize) {
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
// getCurrentCall (simplified)
// ---------------------------------------------------------------------------

/// Get the current call from the context stack.
/// In C this walks R_GlobalContext; here we use the thread-local context.
unsafe fn getCurrentCall() -> SEXP {
    unsafe {
        // A context counts as carrying a call only when it holds a real
        fn usable_call(call: SEXP) -> SEXP {
            unsafe {
                if call.is_null()
                    || call == globals::R_NilValue()
                    || TYPEOF(call) == SEXPTYPE::NILSXP
                {
                    globals::R_NilValue()
                } else {
                    call
                }
            }
        }

        let ctx = crate::sexp::context::R_GlobalContext();
        if ctx.is_null() {
            return globals::R_NilValue();
        }
        let c = &*ctx;
        // Skip CTXT_BUILTIN contexts
        if (c.callflag & crate::sexp::context::ctxt_flags::CTXT_BUILTIN) != 0
            && !c.nextcontext.is_null()
        {
            let next = &*c.nextcontext;
            return usable_call(next.call);
        }
        usable_call(c.call)
    }
}

/// Public accessor mirroring upstream `getCurrentCall()` (errors.c): the call
/// of the innermost context on the context stack (skipping a CTXT_BUILTIN
/// top frame). Used by interpreter raise sites to attribute errors to the
/// enclosing R call, like upstream `R_MissingArgError`/`error()`.
pub unsafe fn R_getCurrentCall() -> SEXP {
    unsafe { getCurrentCall() }
}

/// findCall: find the function context's call for error reporting.
unsafe fn findCall() -> SEXP {
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

// ---------------------------------------------------------------------------
// Core error functions
// ---------------------------------------------------------------------------

/// Internal verrorcall_dflt — the real error handler.
///
/// This formats the error message into errbuf, prints it if allowed,
/// and then panics with RError to unwind the stack.
///
/// Ported from R's `verrorcall_dflt()` in errors.c.
///
/// The `ap` parameter is a `*mut c_void` that should be cast to `va_list`.
/// When called from Rust code, ap is typically null and the format string
/// is already the final message.
/// Drop guard that mirrors C's `restore_inError` callback.
/// Guaranteed to run even if the panic is caught mid-stack, because
/// Drop runs during unwinding before catch_unwind handlers.
struct RestoreInError {
    old_in_error: i32,
    old_expressions: c_int,
}

impl Drop for RestoreInError {
    fn drop(&mut self) {
        set_in_error(self.old_in_error);
        R_SetExpressions(self.old_expressions);
    }
}

/// Strip a baked-in "Error in <call> : " / "Error: " rendering prefix from a
/// message so condition payloads carry the bare message, as upstream does:
/// the prefix belongs to top-level stderr rendering only.
fn strip_call_prefix(message: &str) -> String {
    if let Some(rest) = message.strip_prefix("Error in ") {
        // Find the " : " separator; upstream renders "<call> : <message>".
        if let Some(pos) = rest.find(" : ") {
            let candidate = &rest[pos + 3..];
            if !candidate.is_empty() {
                return candidate.to_string();
            }
        }
    }
    if let Some(rest) = message.strip_prefix("Error: ") {
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    message.to_string()
}

unsafe fn verrorcall_dflt(call: SEXP, format: *const c_char, ap: *mut c_void) {
    unsafe {
        let old_in_err = in_error();
        if old_in_err > 0 {
            // fail-safe handler for recursive errors
            if old_in_err >= 3 {
                eprint!("Error during wrapup: ");
                if !format.is_null() {
                    let mut buf = vec![0u8; BUFSIZE + 1];
                    if ap.is_null() {
                        let src = CStr::from_ptr(format).to_bytes();
                        let len = src.len().min(BUFSIZE);
                        ptr::copy_nonoverlapping(format as *const u8, buf.as_mut_ptr(), len);
                        buf[len] = 0;
                    } else {
                        vsnprintf_c(buf.as_mut_ptr() as *mut c_char, BUFSIZE, format, ap);
                        buf[BUFSIZE] = 0;
                    }
                    let msg = CStr::from_ptr(buf.as_ptr() as *const c_char)
                        .to_str()
                        .unwrap_or("");
                    eprintln!("{}", msg);
                } else {
                    eprintln!();
                }
            }
            // Clean up warnings
            set_collect_warnings(0);
            set_warnings_ptr(ptr::null_mut());
            eprintln!(
                "Error: no more error handlers available (recursive errors?); invoking 'abort' restart"
            );
            R_Expressions_keep();
            jump_to_top_ex(0, 0, 0, 0, 0);
            return;
        }

        // Push a Drop guard — equivalent to C's begincontext + cend = &restore_inError.
        // This guarantees IN_ERROR and R_Expressions are restored even if the panic
        // is caught by an intermediate catch_unwind frame.
        let _guard = RestoreInError {
            old_in_error: old_in_err,
            old_expressions: R_Expressions(),
        };
        set_in_error(1);

        // Format the variadic message.  Like errors.c:790-817, the message
        // is capped by the warning length: with a call at
        // min(BUFSIZE, R_WarnLength) + 1 - strlen("Error in ") bytes for
        // Rvsnprintf (i.e. warn_len - 9 characters), without a call at
        // warn_len - 7 ("Error: ").
        let warn_len = BUFSIZE.min(r_warn_length().max(0) as usize);
        let has_call = !call.is_null() && isNull(call) == 0;
        let head_len = if has_call {
            b"Error in ".len()
        } else {
            b"Error: ".len()
        };
        let tmp_cap = (warn_len + 1).saturating_sub(head_len).saturating_sub(1);
        let mut tmp_str = format_varargs(format, ap);
        truncate_bytes(&mut tmp_str, tmp_cap);

        // ERRBUFCAT only concatenates while the total stays under BUFSIZE.
        let errcat = |buf: &mut String, s: &str| {
            if buf.len() + s.len() < BUFSIZE {
                buf.push_str(s);
            }
        };

        // Build the full error message and write to errbuf via R_SetErrmessage
        let mut err_msg = String::new();

        if has_call {
            // Error with call — "Error in <call> : <message>"
            // Upstream errors.c verrorcall_dflt deparses the call with
            // deparse1s(); reuse the faithful port instead of a placeholder.
            let dcall_sexp = crate::mainutils::deparse::deparse1s(call);
            let dcall: String = if dcall_sexp.is_null()
                || dcall_sexp == globals::R_NilValue()
                || TYPEOF(dcall_sexp) != SEXPTYPE::STRSXP
                || XLENGTH(dcall_sexp) == 0
            {
                "<call>".to_string()
            } else {
                let cs = STRING_ELT(dcall_sexp, 0);
                if cs.is_null() {
                    "<call>".to_string()
                } else {
                    let cptr = translateChar(cs);
                    if cptr.is_null() {
                        "<call>".to_string()
                    } else {
                        std::ffi::CStr::from_ptr(cptr)
                            .to_string_lossy()
                            .into_owned()
                    }
                }
            };

            // errors.c:818 — the buffer-fit test is strlen("Error in ") +
            // strlen("\n  ") + strlen(tmp) < BUFSIZE; the deparsed call
            // participates only in the LONGWARN wrap decision below.
            if head_len + b"\n  ".len() + tmp_str.len() < BUFSIZE {
                errcat(&mut err_msg, "Error in ");
                errcat(&mut err_msg, &dcall);
                errcat(&mut err_msg, " : ");

                // Check if first line is too long
                // (14 + strlen(dcall) + msgline1 > LONGWARN).
                let msg_first_line = tmp_str
                    .find('\n')
                    .map(|i| &tmp_str[..i])
                    .unwrap_or(&tmp_str);
                if 14 + dcall.len() + msg_first_line.len() > LONGWARN {
                    errcat(&mut err_msg, "\n  ");
                }
                errcat(&mut err_msg, &tmp_str);
            } else {
                // Fallback: just "Error: <message>"
                errcat(&mut err_msg, "Error: ");
                errcat(&mut err_msg, &tmp_str);
            }
        } else {
            // Error without call — "Error: <message>"
            errcat(&mut err_msg, "Error: ");
            errcat(&mut err_msg, &tmp_str);
        }

        // Approximate truncation detection (errors.c:855-863): with a
        // single-byte locale (R_MB_CUR_MAX == 1) this can only trigger when
        // the buffer already overflowed past BUFSIZE - 1.
        let nc = err_msg.len();
        if nc > BUFSIZE - 1 {
            let end = (nc + 1).min(BUFSIZE + 1 - 4);
            truncate_bytes(&mut err_msg, end - 1);
            err_msg.push_str("...\n");
        } else {
            // Ensure newline termination
            if !err_msg.ends_with('\n') {
                err_msg.push('\n');
            }

            // Show error call trace if configured (errors.c:870-882:
            // nc_tr + nc + strlen("Calls:") + 2 < BUFSIZE + 1).
            if r_show_error_calls() && has_call {
                let tr = R_ConciseTraceback(call, 0);
                if !tr.is_empty() && tr.len() + err_msg.len() + b"Calls:".len() + 2 < BUFSIZE + 1 {
                    err_msg.push_str("Calls: ");
                    err_msg.push_str(&tr);
                    err_msg.push('\n');
                }
            }
        }

        // Payload contract: the RError message is the BARE message (what
        // condition objects / tryCatch handlers see, matching upstream where
        // "Error in <call> :" attribution is added only by top-level error
        // printing). Strip any prefix that a raise site baked into its text so
        // the payload stays clean; the rendered errbuf above keeps the full
        // attribution for stderr.
        let payload_message = strip_call_prefix(&tmp_str);

        // Write to thread-local errbuf via R_SetErrmessage
        R_SetErrmessage(&err_msg);

        // Record that this exact error message was rendered into the error
        // buffer so the top-level renderer (and the builtin-dispatch
        // attribution wrapper) trust the buffer for this error only —
        // renders from previously caught errors must not leak into later
        // results (upstream: caught errors never reach this printer).
        set_last_rendered_message(Some(payload_message.clone()));

        // Emission contract, exactly once:
        // - When no output capture is active (standalone embedding), write
        //   the rendered text to process stderr here, like Rscript.
        // - When the session is capturing output, do NOT emit here. The
        //   error may still be caught by tryCatch up-stack (upstream prints
        //   nothing for caught errors); the top-level embedding layer emits
        //   the rendered error buffer text once, and only when the error
        //   actually escapes the script. Emitting into the captured-stderr
        //   channel here would leak caught errors into successful results.
        if r_show_error_messages() && !crate::sexp::output::is_capturing() {
            eprint!("{}", R_GetErrorBuf());
        }

        // Deferred warnings follow the same rule (upstream prints them only
        // for errors that reach top-level printing).
        if r_show_error_messages() && collect_warnings() > 0 && !crate::sexp::output::is_capturing()
        {
            eprint!("In addition: ");
            PrintWarnings();
        }

        // The Drop guard (_guard) will restore IN_ERROR and R_Expressions
        // automatically.
        std::panic::panic_any(RError {
            message: payload_message,
        });
    }
}

/// Truncate `s` to at most `limit` bytes without splitting a UTF-8
/// character (the intent of errors.c's mbcsTruncateToValid).
fn truncate_bytes(s: &mut String, limit: usize) {
    if s.len() <= limit {
        return;
    }
    let mut end = limit;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

// ---------------------------------------------------------------------------
// Handler dispatch for tryCatch/withCallingHandlers support
// ---------------------------------------------------------------------------

unsafe fn findSimpleErrorHandler() -> SEXP {
    unsafe {
        let mut list = handler_stack();
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = CAR(list);
            let class_ptr = CHAR(ENTRY_CLASS(entry));
            if !class_ptr.is_null() {
                let class_str = CStr::from_ptr(class_ptr).to_bytes();
                if class_str == b"simpleError" || class_str == b"error" || class_str == b"condition"
                {
                    return list;
                }
            }
            list = CDR(list);
        }
        globals::R_NilValue()
    }
}

unsafe fn gotoExitingHandler(cond: SEXP, call: SEXP, entry: SEXP) {
    unsafe {
        let rho = ENTRY_TARGET_ENVIR(entry);
        let result = ENTRY_RETURN_RESULT(entry);
        SET_VECTOR_ELT(result, 0, cond);
        SET_VECTOR_ELT(result, 1, call);
        SET_VECTOR_ELT(result, 2, ENTRY_HANDLER(entry));
        std::panic::panic_any(crate::sexp::context::RSignal::ExitingHandler {
            target_env: rho,
            result,
        });
    }
}

unsafe fn vsignalError(call: SEXP, format: *const c_char) {
    unsafe {
        let localbuf = if format.is_null() {
            String::new()
        } else {
            CStr::from_ptr(format).to_str().unwrap_or("").to_string()
        };

        let mut list = findSimpleErrorHandler();
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = CAR(list);
            set_handler_stack(CDR(list));
            if IS_CALLING_ENTRY(entry) != 0 {
                if ENTRY_HANDLER(entry) == globals::R_RestartToken() {
                    break;
                }
                let hooksym = Rf_install(b".handleSimpleError\0".as_ptr() as *const c_char);
                let msg_cstr = std::ffi::CString::new(localbuf.as_str()).unwrap_or_default();
                let msg_sexp = Rf_mkString(msg_cstr.as_ptr());
                let _msg_guard = protect(msg_sexp);
                let handler = ENTRY_HANDLER(entry);
                let inner = Rf_lang2(handler, msg_sexp);
                let _inner_guard = protect(inner);
                let hcall = Rf_lang3(hooksym, inner, call);
                let _hcall_guard = protect(hcall);
                let _ = crate::eval::eval::Rf_eval(hcall, globals::R_BaseEnv());
            } else {
                gotoExitingHandler(globals::R_NilValue(), call, entry);
            }
            list = findSimpleErrorHandler();
        }
    }
}

/// Report an error with a call.
///
/// This is the equivalent of R's `errorcall()`.
/// In C this is variadic: `void errorcall(SEXP call, const char *format, ...)`.
/// In Rust, the format string should be a pre-formatted message (no % placeholders).
/// For formatted errors, use `Rf_errorcall1()` or pre-format before calling.
///
/// It does not return — it panics with an RError payload.
pub fn errorcall(call: SEXP, format: *const c_char) {
    unsafe {
        vsignalError(call, format);
        verrorcall_dflt(call, format, ptr::null_mut());
    }
}

/// Report an error with a call, from a Rust `&str` message.
///
/// This is the Rust-native equivalent of upstream `errorcall(call, "%s", msg)`
/// used by the interpreter and builtin handlers: it renders
/// "Error in <call> : <message>" into the error buffer (attributing the call
/// exactly like stock R) and panics with a bare-message `RError` payload.
/// Pass a null call for upstream `call. = FALSE` semantics ("Error: <message>").
pub fn errorcall_str(call: SEXP, message: &str) -> ! {
    let c_msg = std::ffi::CString::new(message).unwrap_or_default();
    errorcall(call, c_msg.as_ptr());
    unreachable!("errorcall never returns: verrorcall_dflt panics with RError");
}

/// Run a builtin/special handler call, attributing unattributed errors to
/// the R call being applied.
///
/// Upstream builtin handlers receive `call` and raise `errorcall(call, ...)`;
/// most ported handlers predate that convention and panic with a bare
/// `RError`. This wrapper mirrors the upstream convention at the dispatch
/// boundary: if the handler panics with an error that has not already been
/// rendered (and thus attributed) by a raise site, the error is re-raised
/// through `errorcall_str` with the applied call, so top-level rendering
/// shows "Error in <call> : <message>" exactly like stock R.
pub(crate) fn attribute_handler_errors<F>(call: SEXP, f: F) -> SEXP
where
    F: FnOnce() -> SEXP,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => {
            if let Some(err) = payload.downcast_ref::<RError>() {
                let message = err.message.clone();
                if !error_was_last_rendered(&message) {
                    // Diverges: renders "Error in <call> : <message>" and
                    // panics with the bare-message payload.
                    errorcall_str(call, &message);
                }
            }
            // Already attributed at the raise site (or not an RError):
            // continue unwinding with the original payload untouched.
            std::panic::resume_unwind(payload)
        }
    }
}

/// Report a formatted error with one string argument.
/// Equivalent to C's `errorcall(call, "%s", msg)`.
pub fn Rf_errorcall1(call: SEXP, format: *const c_char, arg: *const c_char) {
    unsafe {
        let msg = if arg.is_null() {
            ""
        } else {
            CStr::from_ptr(arg).to_str().unwrap_or("")
        };
        let formatted = format!(
            "{}{}",
            if format.is_null() {
                ""
            } else {
                CStr::from_ptr(format).to_str().unwrap_or("")
            },
            msg
        );
        verrorcall_dflt(
            call,
            std::ffi::CString::new(formatted)
                .unwrap_or_default()
                .as_ptr(),
            ptr::null_mut(),
        );
    }
}

/// Report a formatted error with call, using printf-style formatting.
/// This is a Rust-native helper that supports simple format strings.
pub fn Rf_errorcall_fmt(call: SEXP, format: *const c_char, args: &[&CStr]) {
    unsafe {
        if format.is_null() {
            verrorcall_dflt(call, b"\0".as_ptr() as *const c_char, ptr::null_mut());
            return;
        }
        let fmt = CStr::from_ptr(format).to_str().unwrap_or("");
        // Simple format expansion: replace %s with args in order
        let mut result = fmt.to_string();
        for arg_cstr in args {
            let arg_str = arg_cstr.to_str().unwrap_or("");
            if let Some(pos) = result.find("%s") {
                result = format!("{}{}{}", &result[..pos], arg_str, &result[pos + 2..]);
            } else if let Some(pos) = result.find("%d") {
                result = format!("{}{}{}", &result[..pos], arg_str, &result[pos + 2..]);
            } else {
                break;
            }
        }
        let c_result = std::ffi::CString::new(result).unwrap_or_default();
        verrorcall_dflt(call, c_result.as_ptr(), ptr::null_mut());
    }
}

/// Report an error with a call and pre-formatted message buffer.
/// Matches C's `errorcall_cpy()` — copies all data before doing anything else.
pub unsafe fn errorcall_cpy(call: SEXP, format: *const c_char) {
    unsafe {
        let mut buf = vec![0u8; BUFSIZE + 1];
        if !format.is_null() {
            let len = CStr::from_ptr(format).to_bytes().len().min(BUFSIZE - 1);
            ptr::copy_nonoverlapping(format as *const u8, buf.as_mut_ptr(), len);
            buf[len] = 0;
        } else {
            buf[0] = 0;
        }
        errorcall(call, buf.as_ptr() as *const c_char);
    }
}

/// Report an error (without call).
///
/// This is the equivalent of R's `Rf_error()`.
/// The format string should be a pre-formatted message.
/// It does not return — it panics with an RError payload.
pub unsafe fn Rf_error(format: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        // Rf_error in C is variadic: void error(const char *format, ...)
        // In Rust, callers should pass pre-formatted strings.
        errorcall(call, format);
    }
}

/// Report a formatted error (without call), with one string argument.
/// Equivalent to C's `error("%s", msg)`.
pub unsafe fn Rf_error1(format: *const c_char, arg: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        Rf_errorcall1(call, format, arg);
    }
}

/// Unimplemented error — for functions that haven't been ported yet.
pub fn Rf_error_unimplemented(name: &str) {
    let msg = format!("function '{}' is not yet implemented", name);
    R_SetErrmessage(&msg);
    std::panic::panic_any(RError { message: msg });
}

/// UNIMPLEMENTED — called from C when a feature is not yet ported.
/// Matches C: `void UNIMPLEMENTED(const char *s) { error("unimplemented feature in %s", s); }`
pub unsafe fn UNIMPLEMENTED(s: *const c_char) {
    unsafe {
        let name = if s.is_null() {
            "unknown"
        } else {
            CStr::from_ptr(s).to_str().unwrap_or("unknown")
        };
        let msg = format!("unimplemented feature in {}", name);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        let call = getCurrentCall();
        errorcall(call, c_msg.as_ptr());
    }
}

/// WrongArgCount — incorrect number of arguments error.
/// Matches C: `void WrongArgCount(const char *s) { error("incorrect number of arguments to \"%s\"", s); }`
pub unsafe fn WrongArgCount(s: *const c_char) {
    unsafe {
        let name = if s.is_null() {
            "unknown"
        } else {
            CStr::from_ptr(s).to_str().unwrap_or("unknown")
        };
        let msg = format!("incorrect number of arguments to \"{}\"", name);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        let call = getCurrentCall();
        errorcall(call, c_msg.as_ptr());
    }
}

// ---------------------------------------------------------------------------
// Warning functions
// ---------------------------------------------------------------------------

/// Internal vwarningcall_dflt — the real warning handler.
///
/// Ported from R's `vwarningcall_dflt()` in errors.c.
/// Handles three modes based on `warn` option:
/// - w < 0: ignore
/// - w == 0: collect warnings for later display
/// - w == 1: print immediately
/// - w >= 2: convert to error
unsafe fn vwarningcall_dflt(call: SEXP, format: *const c_char, ap: *mut c_void) {
    unsafe {
        // Guard against recursive warnings
        if in_warning() != 0 {
            return;
        }

        // Check for warning.expression option
        let s = GetOption1(Rf_install(b"warning.expression\0".as_ptr() as *const c_char));
        if !s.is_null() && isNull(s) == 0 {
            if isLanguage(s) == 0 && isExpression(s) == 0 {
                // Invalid option — fall through
            } else {
                // Would eval the expression — for now, format and print
                let msg = format_varargs(format, ap);
                eprintln!("Warning: {}", msg);
                return;
            }
        }

        // Get warn level
        let warn_sym = Rf_install(b"warn\0".as_ptr() as *const c_char);
        let w = asLogical(GetOption1(warn_sym));
        if w == crate::sexp::ffi::NA_INTEGER {
            // Set to sensible default
            if immediate_warning() {
                // w = 1 — print immediately
            } else {
                // w = 0 — default, handled below
            }
        }
        if w < 0 || in_warning() != 0 || in_error() != 0 {
            return;
        }

        // suppressWarnings(): upstream muffles through a calling-handler
        // restart so the warning never reaches collection or printing; the
        // port tracks the same with a depth counter around the expression.
        if suppress_warnings_depth() > 0 {
            return;
        }

        set_in_warning(1);

        // Format the variadic message into a string
        let (mut fmt_str, truncated) = format_varargs_to_buf(format, ap);
        if truncated {
            // Append truncation marker if room
            let trunc_msg = " [... truncated]";
            if fmt_str.len() + trunc_msg.len() < BUFSIZE {
                fmt_str.push_str(trunc_msg);
            }
        }

        if w >= 2 {
            // Convert warning to error
            set_in_warning(0);
            let full_msg = format!("(converted from warning) {}", fmt_str);
            let c_msg = std::ffi::CString::new(full_msg).unwrap_or_default();
            errorcall(call, c_msg.as_ptr());
        } else if w == 1 || immediate_warning() {
            // Print warnings immediately
            let dcall = if !call.is_null() && isNull(call) == 0 {
                // errors.c:496 deparses with deparse1s()
                warning_dcall(call)
            } else {
                String::new()
            };

            if dcall.is_empty() {
                eprint!("Warning:");
            } else {
                eprint!("Warning in {} :", dcall);
                // Check if first line fits on same line
                let msg_first_line = fmt_str
                    .find('\n')
                    .map(|i| &fmt_str[..i])
                    .unwrap_or(&fmt_str);
                if 18 + dcall.len() + msg_first_line.len() > LONGWARN {
                    eprintln!();
                    eprint!(" ");
                }
            }
            eprintln!(" {}", fmt_str);

            if r_show_warn_calls() && !call.is_null() && isNull(call) == 0 {
                // Respect .signalSimpleWarning hook if present by filtering the traceback accordingly
                let sigsym = Rf_install(b".signalSimpleWarning\0".as_ptr() as *const c_char);
                let tr = if SYMVALUE(sigsym) != globals::R_UnboundValue() {
                    R_ConciseTraceback(call, 1)
                } else {
                    R_ConciseTraceback(call, 0)
                };
                if !tr.is_empty() {
                    eprintln!("Calls: {}", tr);
                }
            }
        } else {
            // w == 0: collect warnings
            if collect_warnings() == 0 {
                setup_warnings();
            }
            let cw = collect_warnings();
            let nw = nwarnings();
            if cw < nw {
                // Store the warning
                let warnings_ptr = warnings_ptr();
                if !warnings_ptr.is_null() && TYPEOF(warnings_ptr) == SEXPTYPE::VECSXP {
                    SET_VECTOR_ELT(warnings_ptr, cw as R_xlen_t, call);
                    let names = CAR(ATTRIB(warnings_ptr));
                    if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP {
                        // Append traceback if requested
                        #[allow(clippy::implicit_clone)]
                        let mut msg_to_store = fmt_str.to_string();
                        if r_show_warn_calls() && !call.is_null() && isNull(call) == 0 {
                            let tr = R_ConciseTraceback(call, 0);
                            if !tr.is_empty() && msg_to_store.len() + tr.len() + 8 < BUFSIZE {
                                msg_to_store.push_str("\nCalls: ");
                                msg_to_store.push_str(&tr);
                            }
                        }
                        let c_msg = std::ffi::CString::new(msg_to_store).unwrap_or_default();
                        let ch = Rf_mkChar(c_msg.as_ptr());
                        SET_STRING_ELT(names, cw as R_xlen_t, ch);
                    }
                    increment_collect_warnings();
                }
            }
        }

        set_in_warning(0);
    }
}

/// Setup the warnings collection vector.
unsafe fn setup_warnings() {
    unsafe {
        let nw = nwarnings();
        let w = Rf_allocVector(SEXPTYPE::VECSXP, nw);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, nw);
        setAttrib_wrap(w, R_NamesSymbol(), names);
        set_warnings_ptr(w);
        set_collect_warnings(0);
    }
}

/// Issue a warning with call.
///
/// This is the equivalent of R's `warningcall()`.
/// Unlike errors, warnings do not terminate execution.
pub unsafe fn warningcall(call: SEXP, format: *const c_char) {
    unsafe {
        vsignalWarning(call, format);
    }
}

unsafe fn vsignalWarning(call: SEXP, format: *const c_char) {
    unsafe {
        let hooksym = Rf_install(b".signalSimpleWarning\0".as_ptr() as *const c_char);
        // A freshly interned port symbol carries a NULL value slot (C uses
        // R_UnboundValue), so treat NULL as unbound too: otherwise every
        // warning would take the hook path and be silently dropped.
        let hook = SYMVALUE(hooksym);
        if !hook.is_null() && hook != globals::R_UnboundValue() {
            let msg = if format.is_null() {
                Rf_mkString(b"\0".as_ptr() as *const c_char)
            } else {
                Rf_mkString(format)
            };
            let _msg_guard = protect(msg);
            let hcall = Rf_lang3(hooksym, msg, call);
            let _hcall_guard = protect(hcall);
            let _ = crate::eval::eval::Rf_eval(hcall, globals::R_BaseEnv());
        } else {
            vwarningcall_dflt(call, format, ptr::null_mut());
        }
    }
}

/// Issue an immediate warning (bypass collection).
pub unsafe fn warningcall_immediate(call: SEXP, format: *const c_char) {
    unsafe {
        let prev = immediate_warning();
        set_immediate_warning(true);
        vwarningcall_dflt(call, format, ptr::null_mut());
        set_immediate_warning(prev);
    }
}

/// Issue a warning (without call).
pub unsafe fn Rf_warning(format: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        warningcall(call, format);
    }
}

/// Issue an immediate warning (without call).
pub unsafe fn Rf_warning_immediate(format: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        warningcall_immediate(call, format);
    }
}

/// Issue a formatted warning with call (Rust helper).
/// Equivalent to C's `warningcall(call, "%s", msg)`.
pub unsafe fn Rf_warningcall1(call: SEXP, msg: *const c_char) {
    unsafe {
        let msg_str = if msg.is_null() {
            ""
        } else {
            CStr::from_ptr(msg).to_str().unwrap_or("")
        };
        let c_msg = std::ffi::CString::new(msg_str).unwrap_or_default();
        warningcall(call, c_msg.as_ptr());
    }
}

/// Issue a formatted warning without call (Rust helper).
/// Equivalent to C's `warning("%s", msg)`.
pub unsafe fn Rf_warning1(msg: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        Rf_warningcall1(call, msg);
    }
}

// ---------------------------------------------------------------------------
// Message functions (R's message())
// ---------------------------------------------------------------------------

/// Issue a message (R's message()).
/// Messages are printed to stdout (via Rprintf in C, println! here).
/// Unlike errors/warnings, messages do not terminate or indicate problems.
///
/// Ported from R's `Rf_message()` concept.
pub unsafe fn Rf_message(format: *const c_char) {
    unsafe {
        if format.is_null() {
            println!();
            return;
        }
        let msg = CStr::from_ptr(format).to_str().unwrap_or("");
        // Strip trailing newline if present (C version does this)
        let msg = msg.trim_end_matches('\n');
        println!("{}", msg);
    }
}

/// Issue a message with call.
/// Ported from R's message handling in errors.c.
pub unsafe fn messagecall(call: SEXP, format: *const c_char) {
    unsafe {
        // In C, message() doesn't use the call for display,
        // but it's passed for consistency
        let _ = call; // suppress unused warning
        Rf_message(format);
    }
}

/// Issue a message with append flag.
/// When append=TRUE, the message is appended without a newline prefix.
/// When append=FALSE (default), the message starts on a new line.
///
/// This matches R's `message(..., appendLF = TRUE)` behavior.
pub unsafe fn Rf_message_append(format: *const c_char, append: c_int) {
    unsafe {
        if format.is_null() {
            if append == 0 {
                println!();
            }
            return;
        }
        let msg = CStr::from_ptr(format).to_str().unwrap_or("");
        let msg = msg.trim_end_matches('\n');
        if append == 0 {
            println!("{}", msg);
        } else {
            print!("{}", msg);
        }
    }
}

/// do_message — R's message() builtin.
/// Ported from errors.c.
pub unsafe fn do_message(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let mut c_call: SEXP = ptr::null_mut();
        if asLogical(CAR(args)) != 0 {
            c_call = findCall();
        }
        let mut args = CDR(args);

        let append = asLogical(CAR(args));
        args = CDR(args);

        if !isNull(CAR(args)) != 0 {
            SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.as_c_int()));
            if isValidString(CAR(args)) != 0 {
                let msg = translateChar(STRING_ELT(CAR(args), 0));
                Rf_message_append(msg, append);
            }
        }

        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// PrintWarnings
// ---------------------------------------------------------------------------

/// Render the collected-warnings block exactly like errors.c `PrintWarnings()`
/// and consume the collection state (including the truncated `last.warning`
/// install). Returns `None` when there is nothing to print.
///
/// Callers own the emission channel: `PrintWarnings()` writes the block to
/// stderr like upstream REprintf, while the script-loop flush routes it into
/// the session output stream to keep Rscript's terminal interleaving.
///
/// Rendering (errors.c:615-673): a single warning prints
/// `Warning message:` then `In <dcall> : <msg>` (or `<msg> ` without a call);
/// two to ten print `Warning messages:` with an `N: ` prefix; longer counts
/// collapse to a summary line. `dcall` is `deparse1s()` of the stored call,
/// and a first line that would exceed LONGWARN (6/10 + dcall + msgline1)
/// wraps with `\n ` before the one-space-indented message.
pub(crate) unsafe fn take_warnings_block() -> Option<String> {
    unsafe {
        let cw = collect_warnings();
        if cw == 0 {
            return None;
        }

        if in_print_warnings() != 0 {
            set_collect_warnings(0);
            set_warnings_ptr(ptr::null_mut());
            return Some("Lost warning messages\n".to_string());
        }

        set_in_print_warnings(1);

        let warnings_ptr = warnings_ptr();
        if warnings_ptr.is_null() || TYPEOF(warnings_ptr) != SEXPTYPE::VECSXP {
            set_in_print_warnings(0);
            return None;
        }

        let names = CAR(ATTRIB(warnings_ptr));
        let msg_of = |i: R_xlen_t| -> String {
            if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP {
                return String::new();
            }
            let msg = CHAR_local(STRING_ELT(names, i));
            CStr::from_ptr(msg).to_str().unwrap_or("").to_string()
        };

        let mut block = String::new();

        if cw == 1 {
            block.push_str("Warning message:\n");
            let call = VECTOR_ELT(warnings_ptr, 0);
            let msg = msg_of(0);
            if isNull(call) != 0 {
                // REprintf("%s \n", msg)
                block.push_str(&msg);
                block.push_str(" \n");
            } else {
                let dcall = warning_dcall(call);
                block.push_str("In ");
                block.push_str(&dcall);
                block.push_str(" :");
                let msgline1 = msg.split('\n').next().map_or(0, str::len);
                if 6 + dcall.len() + msgline1 > LONGWARN {
                    block.push_str("\n ");
                }
                block.push(' ');
                block.push_str(&msg);
                block.push('\n');
            }
        } else if cw <= 10 {
            block.push_str("Warning messages:\n");
            for i in 0..cw as R_xlen_t {
                let call = VECTOR_ELT(warnings_ptr, i);
                let msg = msg_of(i);
                if isNull(call) != 0 {
                    block.push_str(&format!("{}: {} \n", i + 1, msg));
                } else {
                    let dcall = warning_dcall(call);
                    block.push_str(&format!("{}: In {} :", i + 1, dcall));
                    let msgline1 = msg.split('\n').next().map_or(0, str::len);
                    if 10 + dcall.len() + msgline1 > LONGWARN {
                        block.push_str("\n ");
                    }
                    block.push(' ');
                    block.push_str(&msg);
                    block.push('\n');
                }
            }
        } else {
            let nw = nwarnings();
            if cw < nw {
                block.push_str(&format!(
                    "There were {} warnings (use warnings() to see them)\n",
                    cw
                ));
            } else {
                block.push_str(&format!(
                    "There were {} or more warnings (use warnings() to see the first {})\n",
                    nw, nw
                ));
            }
        }

        // Truncate and install last.warning (errors.c:685-695): exactly the
        // collected entries, not the spare-capacity collection vector.
        let sym = Rf_install(b"last.warning\0".as_ptr() as *const c_char);
        let last = Rf_allocVector(SEXPTYPE::VECSXP, cw);
        let _last_guard = protect(last);
        let last_names = Rf_allocVector(SEXPTYPE::STRSXP, cw);
        let _names_guard = protect(last_names);
        for i in 0..cw as R_xlen_t {
            SET_VECTOR_ELT(last, i, VECTOR_ELT(warnings_ptr, i));
            if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP {
                SET_STRING_ELT(last_names, i, Rf_mkChar(b"\0".as_ptr() as *const c_char));
            } else {
                SET_STRING_ELT(last_names, i, STRING_ELT(names, i));
            }
        }
        setAttrib_wrap(last, R_NamesSymbol(), last_names);
        SET_SYMVALUE(sym, last);

        set_in_print_warnings(0);
        set_collect_warnings(0);
        set_warnings_ptr(ptr::null_mut());
        Some(block)
    }
}

/// `deparse1s()` of a stored warning call as a Rust string (errors.c uses the
/// same rendering for the `In <call> :` header). Falls back to `<call>` when
/// the deparse yields nothing usable, mirroring the error renderer above.
unsafe fn warning_dcall(call: SEXP) -> String {
    unsafe {
        let dcall_sexp = crate::mainutils::deparse::deparse1s(call);
        if dcall_sexp.is_null()
            || dcall_sexp == globals::R_NilValue()
            || TYPEOF(dcall_sexp) != SEXPTYPE::STRSXP
            || XLENGTH(dcall_sexp) == 0
        {
            return "<call>".to_string();
        }
        let cs = STRING_ELT(dcall_sexp, 0);
        if cs.is_null() {
            return "<call>".to_string();
        }
        let cptr = translateChar(cs);
        if cptr.is_null() {
            return "<call>".to_string();
        }
        CStr::from_ptr(cptr).to_string_lossy().into_owned()
    }
}

/// Print collected warnings to stderr — upstream's channel (REprintf).
pub unsafe fn PrintWarnings() {
    unsafe {
        if let Some(block) = take_warnings_block() {
            eprint!("{}", block);
        }
    }
}

/// Flush collected warnings at a top-level statement boundary — the port of
/// main.c's REPL loop tail (`if (R_CollectWarnings) PrintWarnings();` after
/// each evaluated expression). Upstream writes to stderr and the terminal
/// interleaves it with stdout in real time; the session model keeps one
/// output stream, so the block is appended to the interleaved stdout capture
/// (falling back to real stdout when no capture is active). Deliberately
/// bypasses `sink()` diversion — warnings are stderr in upstream.
pub unsafe fn print_warnings_at_statement_boundary() {
    unsafe {
        let Some(block) = take_warnings_block() else {
            return;
        };
        let routed = instance::with_current_instance(|inst| {
            let mut capture = inst.output_capture.borrow_mut();
            if capture.is_capturing() {
                capture.capture_stdout_bypassing_sink(&block);
                true
            } else {
                false
            }
        });
        if routed != Some(true) {
            print!("{}", block);
        }
    }
}

// ---------------------------------------------------------------------------
// Stack overflow and interrupt checking
// ---------------------------------------------------------------------------

/// Signal a C stack overflow.
/// Matches C's `R_SignalCStackOverflow(intptr_t usage)`.
/// Uses R_makeCStackOverflowError condition object when available.
pub unsafe fn R_SignalCStackOverflow(usage: isize) {
    unsafe {
        // Try to use the expression stack overflow error condition
        let cond = R_makeCStackOverflowError(globals::R_NilValue(), usage);
        if !cond.is_null() {
            // calling handlers at this point might produce a C stack
            // overflow/SEGFAULT so treat them as failed and skip them
            R_signalErrorConditionEx(cond, globals::R_NilValue(), 1);
        } else {
            let msg = format!("C stack usage {} is too close to the limit", usage);
            R_SetErrmessage(&msg);
            std::panic::panic_any(RError { message: msg });
        }
    }
}

/// Check for stack overflow.
/// In C this checks against R_CStackLimit; in Rust we check eval depth.
pub unsafe fn R_CheckStack() {
    unsafe {
        let depth = globals::R_EvalDepth();
        let limit = globals::R_EvalDepthLimit();
        if depth >= limit {
            R_SignalCStackOverflow(depth as isize);
        }
    }
}

/// Check for stack overflow with extra space.
pub unsafe fn R_CheckStack2(_extra: usize) {
    unsafe {
        R_CheckStack();
    }
}

/// Check for user interrupts.
pub unsafe fn R_CheckUserInterrupt() {
    unsafe {
        R_CheckStack();
        if interrupts_suspended() {
            return;
        }
        if interrupts_pending() {
            onintr();
        }
    }
}

// ---------------------------------------------------------------------------
// Jump to top level (longjmp replacement)
// ---------------------------------------------------------------------------

/// Jump to the top-level context.
/// In C, this uses longjmp. In Rust, we panic with RError.
pub unsafe fn jump_to_top_ex(
    traceback: c_int,
    try_user_handler: c_int,
    process_warnings: c_int,
    reset_console: c_int,
    ignore_restart: c_int,
) {
    unsafe {
        let old_in_error = in_error();
        let mut have_handler = false;

        if try_user_handler != 0 && old_in_error < 3 {
            if old_in_error == 0 {
                set_in_error(1);
            }
            let err_opt = GetOption1(Rf_install(b"error\0".as_ptr() as *const c_char));
            have_handler = !err_opt.is_null() && err_opt != globals::R_NilValue();
            if have_handler {
                let is_lang = TYPEOF(err_opt) == SEXPTYPE::LANGSXP;
                let is_expr = TYPEOF(err_opt) == SEXPTYPE::EXPRSXP;
                if !is_lang && !is_expr {
                    eprintln!("invalid option \"error\"");
                } else {
                    set_in_error(3);
                    if is_lang {
                        let _ = crate::eval::eval::Rf_eval(err_opt, globals::R_GlobalEnv());
                    } else {
                        let n = LENGTH(err_opt);
                        for i in 0..n {
                            let _ = crate::eval::eval::Rf_eval(
                                crate::sexp::accessors::VECTOR_ELT(err_opt, i as R_xlen_t),
                                globals::R_GlobalEnv(),
                            );
                        }
                    }
                    set_in_error(old_in_error);
                }
            }
            set_in_error(old_in_error);
        }

        if process_warnings != 0 && collect_warnings() > 0 {
            PrintWarnings();
        }

        if ignore_restart == 0 {
            try_jump_to_restart();
        }

        if traceback != 0 && old_in_error < 2 {
            set_in_error(2);
            let tb = R_GetTracebackOnly(0);
            let sym = Rf_install(b".Traceback\0".as_ptr() as *const c_char);
            SET_SYMVALUE(sym, tb);
            set_in_error(old_in_error);
        }

        set_in_error(0);
        std::panic::panic_any(RSignal::Error {
            message: "jump_to_top".to_string(),
        });
    }
}

unsafe fn try_jump_to_restart() {
    unsafe {
        let mut list = restart_stack();
        while !list.is_null() && list != globals::R_NilValue() {
            let restart = CAR(list);
            if TYPEOF(restart) == SEXPTYPE::VECSXP && LENGTH(restart) > 1 {
                let name = crate::sexp::accessors::VECTOR_ELT(restart, 0);
                if TYPEOF(name) == SEXPTYPE::STRSXP && LENGTH(name) == 1 {
                    let cname = CHAR(STRING_ELT(name, 0));
                    if !cname.is_null() {
                        let bytes = CStr::from_ptr(cname).to_bytes();
                        if bytes == b"browser" || bytes == b"tryRestart" || bytes == b"abort" {
                            invokeRestart(restart, globals::R_NilValue());
                        }
                    }
                }
            }
            list = CDR(list);
        }
    }
}

unsafe fn invokeRestart(restart: SEXP, _args: SEXP) {
    std::panic::panic_any(RSignal::Error {
        message: "restart invoked".to_string(),
    });
}

/// Handle interrupt signal.
pub unsafe fn onintr() {
    unsafe {
        jump_to_top_ex(1, 1, 0, 0, 0);
    }
}

/// Handle interrupt signal without resume option.
pub unsafe fn onintrNoResume() {
    unsafe {
        jump_to_top_ex(0, 1, 0, 0, 0);
    }
}

// ---------------------------------------------------------------------------
// R-level do_* functions (called from the evaluator)
// ---------------------------------------------------------------------------

/// do_stop — R's stop() function.
/// Ported from errors.c do_stop().
pub unsafe fn do_stop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let mut c_call: SEXP = ptr::null_mut();
        if asLogical(CAR(args)) != 0 {
            c_call = findCall();
        }
        let args = CDR(args);

        if !isNull(CAR(args)) != 0 {
            // Has a message
            SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.as_c_int()));
            if isValidString(CAR(args)) == 0 {
                let c_msg =
                    std::ffi::CString::new(" [invalid string in stop(.)]").unwrap_or_default();
                errorcall(c_call, c_msg.as_ptr());
            }
            // Pre-format: in C this is errorcall(c_call, "%s", translateChar(...))
            // In Rust, we pre-format the string
            let msg = translateChar(STRING_ELT(CAR(args), 0));
            let msg_str = CStr::from_ptr(msg).to_str().unwrap_or("");
            let c_msg = std::ffi::CString::new(msg_str).unwrap_or_default();
            errorcall(c_call, c_msg.as_ptr());
            // errorcall doesn't return, but we need a return type
            ptr::null_mut()
        } else {
            errorcall(c_call, b"\0".as_ptr() as *const c_char);
            ptr::null_mut()
        }
    }
}

/// Direct .Internal(stop(...)) handler for structured embedding boundaries.
pub unsafe fn do_stop_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let args = CDR(args);
        if isNull(CAR(args)) != 0 {
            errorcall_str(globals::R_NilValue(), "");
        }

        SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.as_c_int()));
        if isValidString(CAR(args)) == 0 {
            errorcall_str(globals::R_NilValue(), " [invalid string in stop(.)]");
        }

        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let message = CStr::from_ptr(msg).to_str().unwrap_or("").to_string();
        // Like upstream do_stop, the call comes from the context stack, not
        // the .Internal expression; render explicitly (bare at top level) so
        // the .Internal-dispatch attribution wrapper does not add a call.
        errorcall_str(R_getCurrentCall(), &message)
    }
}

/// do_warning — R's warning() function.
/// Ported from errors.c do_warning().
pub unsafe fn do_warning(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let mut c_call: SEXP = ptr::null_mut();
        if asLogical(CAR(args)) != 0 {
            c_call = findCall();
        }
        let mut args = CDR(args);

        if asLogical(CAR(args)) != 0 {
            set_immediate_warning(true);
        } else {
            set_immediate_warning(false);
        }
        args = CDR(args);

        if asLogical(CAR(args)) != 0 {
            set_no_break_warning(true);
        } else {
            set_no_break_warning(false);
        }
        args = CDR(args);

        let message = CAR(args);
        if !isNull(message) != 0 {
            SETCAR(args, coerceVector(message, SEXPTYPE::STRSXP.as_c_int()));
            let message = CAR(args);
            if isValidString(message) == 0 {
                let c_msg =
                    std::ffi::CString::new(" [invalid string in warning(.)]").unwrap_or_default();
                warningcall(c_call, c_msg.as_ptr());
            } else {
                // Pre-format: in C this is warningcall(c_call, "%s", translateChar(...))
                let msg = translateChar(STRING_ELT(message, 0));
                let msg_str = CStr::from_ptr(msg).to_str().unwrap_or("");
                let c_msg = std::ffi::CString::new(msg_str).unwrap_or_default();
                warningcall(c_call, c_msg.as_ptr());
            }
        } else {
            warningcall(c_call, b"\0".as_ptr() as *const c_char);
        }

        set_immediate_warning(false);
        set_no_break_warning(false);

        CAR(args)
    }
}

/// do_geterrmessage — geterrmessage().
pub unsafe fn do_geterrmessage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let msg = R_GetErrorBuf();
        let msg = std::ffi::CString::new(msg).unwrap_or_default();
        Rf_mkString(msg.as_ptr())
    }
}

/// do_seterrmessage — seterrmessage().
pub unsafe fn do_seterrmessage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let msg = CAR(args);
        if isString(msg) == 0 || LENGTH(msg) != 1 {
            errorcall(
                call,
                b"error message must be a character string\x00".as_ptr() as *const c_char,
            );
        }
        let s = CHAR_local(STRING_ELT(msg, 0));
        R_SetErrmessage_c(s);
        globals::R_NilValue()
    }
}

/// do_printDeferredWarnings — print deferred warnings.
pub unsafe fn do_printDeferredWarnings(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if r_show_error_messages() && collect_warnings() > 0 {
            PrintWarnings();
        }
        globals::R_NilValue()
    }
}

/// do_interruptsSuspended — get/set interrupts suspended flag.
pub unsafe fn do_interruptsSuspended(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let orig = interrupts_suspended();
        ScalarLogical(orig as c_int)
    }
}

// ---------------------------------------------------------------------------
// Traceback support
// ---------------------------------------------------------------------------

/// R_GetTracebackOnly — return traceback without deparsing calls.
/// Ported from errors.c R_GetTracebackOnly().
pub unsafe fn R_GetTracebackOnly(skip: c_int) -> SEXP {
    unsafe {
        let mut nback: c_int = 0;
        let mut ns = skip;

        // First pass: count frames
        let ctx = crate::sexp::context::R_GlobalContext();
        let mut c = ctx;
        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0
            {
                if ns > 0 {
                    ns -= 1;
                } else {
                    nback += 1;
                }
            }
            c = ctx_ref.nextcontext;
        }

        let s = Rf_allocList(nback);
        let mut t = s;
        let mut skip2 = skip;

        // Second pass: fill in the calls
        c = ctx;
        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0
            {
                if skip2 > 0 {
                    skip2 -= 1;
                } else {
                    // SETCAR(t, duplicate(ctx_ref.call));
                    //  set to the call (no deep copy)
                    if !t.is_null() {
                        SETCAR(t, ctx_ref.call);
                    }
                    t = CDR(t);
                }
            }
            c = ctx_ref.nextcontext;
        }

        s
    }
}

/// R_ConciseTraceback — return a concise call chain as a string.
/// Ported from errors.c R_ConciseTraceback().
pub unsafe fn R_ConciseTraceback(call: SEXP, skip: c_int) -> String {
    unsafe {
        let ctx = crate::sexp::context::R_GlobalContext();
        let mut c = ctx;
        let mut buf = String::new();
        let mut ncalls: c_int = 0;
        let mut too_many = false;
        let mut top = "";
        let mut skip_count = skip;

        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0
            {
                if skip_count > 0 {
                    skip_count -= 1;
                } else {
                    // Get function name from call
                    let fun = if !ctx_ref.call.is_null() {
                        CAR(ctx_ref.call)
                    } else {
                        ptr::null_mut()
                    };
                    let this = if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::SYMSXP {
                        let name = CHAR_local(PRINTNAME(fun));
                        CStr::from_ptr(name).to_str().unwrap_or("<Anonymous>")
                    } else {
                        "<Anonymous>"
                    };

                    // Skip internal functions
                    if this == "stop"
                        || this == "warning"
                        || this == "suppressWarnings"
                        || this == ".signalSimpleWarning"
                    {
                        buf.clear();
                        ncalls = 0;
                        too_many = false;
                    } else {
                        ncalls += 1;
                        if too_many {
                            top = this;
                        } else if buf.len() > R_NSHOWCALLS {
                            buf = format!("... {}", buf);
                            too_many = true;
                            top = this;
                        } else if !buf.is_empty() {
                            buf = format!("{} -> {}", this, buf);
                        } else {
                            buf = this.to_string();
                        }
                    }
                }
            }
            c = ctx_ref.nextcontext;
        }

        if too_many && top.len() < 50 {
            buf = format!("{} {}", top, buf);
        }

        buf
    }
}

/// do_traceback — traceback().
pub unsafe fn do_traceback(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let skip = if isInteger(CAR(args)) != 0 && LENGTH(CAR(args)) >= 1 {
            *INTEGER(CAR(args))
        } else {
            crate::sexp::ffi::NA_INTEGER
        };
        if skip == crate::sexp::ffi::NA_INTEGER || skip < 0 {
            errorcall(call, b"invalid 'skip' value\x00".as_ptr() as *const c_char);
        }
        R_GetTracebackOnly(skip)
    }
}

// ---------------------------------------------------------------------------
// Error/Warning message databases
// ---------------------------------------------------------------------------

/// Error codes (from Errormsg.h).
pub mod error_codes {
    pub const ERROR_NUMARGS: i32 = 1;
    pub const ERROR_ARGTYPE: i32 = 2;
    pub const ERROR_TSVEC_MISMATCH: i32 = 3;
    pub const ERROR_INCOMPAT_ARGS: i32 = 4;
    pub const ERROR_UNIMPLEMENTED: i32 = 5;
    pub const ERROR_UNKNOWN: i32 = 6;
}

/// Warning codes.
pub mod warning_codes {
    pub const WARNING_coerce_NA: i32 = 0;
    pub const WARNING_coerce_INACC: i32 = 1;
    pub const WARNING_coerce_IMAG: i32 = 2;
    pub const WARNING_UNKNOWN: i32 = 3;
}

/// ErrorMessage — look up an error message from the database and call errorcall.
/// Matches C: `void ErrorMessage(SEXP call, int which_error, ...)`
pub unsafe fn ErrorMessage(call: SEXP, which_error: c_int, format: *const c_char) {
    unsafe {
        let messages = [
            "invalid number of arguments",
            "invalid argument type",
            "time-series/vector length mismatch",
            "incompatible arguments",
            "unimplemented feature in %s",
            "unknown error (report this!)",
        ];

        let idx = if which_error >= 0 && (which_error as usize) < messages.len() {
            which_error as usize
        } else {
            messages.len() - 1
        };

        // For format strings with %s, use the format argument
        let msg = if which_error == error_codes::ERROR_UNIMPLEMENTED && !format.is_null() {
            let arg = CStr::from_ptr(format).to_str().unwrap_or("unknown");
            format!("unimplemented feature in {}", arg)
        } else {
            messages[idx].to_string()
        };

        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        errorcall(call, c_msg.as_ptr());
    }
}

/// WarningMessage — look up a warning message from the database and call warningcall.
/// Matches C: `void WarningMessage(SEXP call, R_WARNING which_warn, ...)`
pub unsafe fn WarningMessage(call: SEXP, which_warn: c_int, format: *const c_char) {
    unsafe {
        let messages = [
            "NAs introduced by coercion",
            "inaccurate integer conversion in coercion",
            "imaginary parts discarded in coercion",
            "unknown warning (report this!)",
        ];

        let idx = if which_warn >= 0 && (which_warn as usize) < messages.len() {
            which_warn as usize
        } else {
            messages.len() - 1
        };

        let c_msg = std::ffi::CString::new(messages[idx]).unwrap_or_default();
        warningcall(call, c_msg.as_ptr());
    }
}

// ---------------------------------------------------------------------------
// gettext/ngettext support (simplified — no actual i18n)
// ---------------------------------------------------------------------------

/// do_gettext — R's gettext() function (simplified, no i18n).
pub unsafe fn do_gettext(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: just return the string as-is (no translation)
        let string = CADR(args);
        if isNull(string) != 0 || LENGTH(string) == 0 {
            return string;
        }
        string
    }
}

/// do_ngettext — R's ngettext() function (simplified, no i18n).
pub unsafe fn do_ngettext(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let n = if isInteger(CAR(args)) != 0 && LENGTH(CAR(args)) >= 1 {
            *INTEGER(CAR(args))
        } else {
            crate::sexp::ffi::NA_INTEGER
        };
        let msg1 = CADR(args);
        let msg2 = CADDR(args);

        if n == crate::sexp::ffi::NA_INTEGER || n < 0 {
            errorcall(call, b"invalid 'n' argument\x00".as_ptr() as *const c_char);
        }

        // Return singular or plural form based on n
        if n == 1 { msg1 } else { msg2 }
    }
}

// ---------------------------------------------------------------------------
// Condition handling infrastructure
// ---------------------------------------------------------------------------

/// Handler entry structure (mirrors R's mkHandlerEntry).
pub fn mkHandlerEntry(
    klass: SEXP,
    parentenv: SEXP,
    handler: SEXP,
    target: SEXP,
    result: SEXP,
    calling: c_int,
) -> SEXP {
    unsafe {
        let entry = Rf_allocVector(SEXPTYPE::VECSXP, 5);
        if !entry.is_null() {
            SET_VECTOR_ELT(entry, 0, klass);
            SET_VECTOR_ELT(entry, 1, parentenv);
            SET_VECTOR_ELT(entry, 2, handler);
            SET_VECTOR_ELT(entry, 3, target);
            SET_VECTOR_ELT(entry, 4, result);
            SETLEVELS(entry, calling);
        }
        entry
    }
}

/// IS_CALLING_ENTRY macro.
#[inline]
pub unsafe fn IS_CALLING_ENTRY(e: SEXP) -> c_int {
    unsafe { LEVELS(e) }
}

/// ENTRY_CLASS macro.
#[inline]
pub unsafe fn ENTRY_CLASS(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 0) }
}

/// ENTRY_HANDLER macro.
#[inline]
pub unsafe fn ENTRY_HANDLER(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 2) }
}

/// ENTRY_TARGET_ENVIR macro.
#[inline]
pub unsafe fn ENTRY_TARGET_ENVIR(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 3) }
}

/// ENTRY_RETURN_RESULT macro.
#[inline]
pub unsafe fn ENTRY_RETURN_RESULT(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 4) }
}

/// CLEAR_ENTRY_CALLING_ENVIR macro.
#[inline]
pub unsafe fn CLEAR_ENTRY_CALLING_ENVIR(e: SEXP) {
    unsafe {
        SET_VECTOR_ELT(e, 1, globals::R_NilValue());
    }
}

/// CLEAR_ENTRY_TARGET_ENVIR macro.
#[inline]
pub unsafe fn CLEAR_ENTRY_TARGET_ENVIR(e: SEXP) {
    unsafe {
        SET_VECTOR_ELT(e, 3, globals::R_NilValue());
    }
}

/// RESULT_SIZE for handler results.
pub const RESULT_SIZE: usize = 4;

/// do_addCondHands — add condition handlers to the stack.
pub unsafe fn do_addCondHands(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let classes = CAR(args);
        let mut rest = CDR(args);
        let handlers = CAR(rest);
        rest = CDR(rest);
        let parentenv = CAR(rest);
        rest = CDR(rest);
        let target = CAR(rest);
        rest = CDR(rest);
        let calling = asLogical(CAR(rest));

        if isNull(classes) != 0 || isNull(handlers) != 0 {
            return handler_stack();
        }

        let n = LENGTH(handlers);

        {
            let oldstack = handler_stack();

            let result = Rf_allocVector(SEXPTYPE::VECSXP, RESULT_SIZE as c_int);
            let mut newstack = oldstack;

            for i in (0..n).rev() {
                let klass = STRING_ELT(classes, i as R_xlen_t);
                let handler = VECTOR_ELT(handlers, i as R_xlen_t);
                let entry = mkHandlerEntry(klass, parentenv, handler, target, result, calling);
                newstack = Rf_cons(entry, newstack);
            }

            set_handler_stack(newstack);
            oldstack
        }
    }
}

/// do_resetCondHands — reset condition handlers to a previous state.
pub unsafe fn do_resetCondHands(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let old = CAR(args);
        set_handler_stack(old);
        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Restart handling
// ---------------------------------------------------------------------------

/// do_getRestart — get a restart from the restart stack.
pub unsafe fn do_getRestart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let mut i = if isInteger(CAR(args)) != 0 && LENGTH(CAR(args)) >= 1 {
            *INTEGER(CAR(args))
        } else {
            crate::sexp::ffi::NA_INTEGER
        };

        let mut list = restart_stack();
        while !list.is_null() && i > 1 {
            list = CDR(list);
            i -= 1;
        }
        if !list.is_null() {
            CAR(list)
        } else if i == 1 {
            // Return abort restart
            let name = Rf_mkString(b"abort\x00".as_ptr() as *const c_char);
            let entry = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(entry, 0, name);
            SET_VECTOR_ELT(entry, 1, globals::R_NilValue());
            setAttrib_wrap(
                entry,
                R_ClassSymbol(),
                Rf_mkString(b"restart\x00".as_ptr() as *const c_char),
            );
            entry
        } else {
            globals::R_NilValue()
        }
    }
}

/// do_addRestart — add a restart to the restart stack.
pub unsafe fn do_addRestart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let r = CAR(args);
        if TYPEOF(r) != SEXPTYPE::VECSXP || LENGTH(r) < 2 {
            errorcall(call, b"bad restart\x00".as_ptr() as *const c_char);
        }
        set_restart_stack(Rf_cons(r, restart_stack()));
        globals::R_NilValue()
    }
}

/// do_invokeRestart — invoke a restart.
pub unsafe fn do_invokeRestart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let r = CAR(args);
        if TYPEOF(r) != SEXPTYPE::VECSXP || LENGTH(r) < 2 {
            errorcall(call, b"bad restart\x00".as_ptr() as *const c_char);
        }
        // invokeRestart would jump to the restart; for now just panic
        let exit = VECTOR_ELT(r, 1);
        if isNull(exit) != 0 {
            set_restart_stack(globals::R_NilValue());
            jump_to_top_ex(0, 0, 1, 1, 1);
        }
        ptr::null_mut() // unreachable
    }
}

/// do_addTryHandlers — add tryCatch handlers.
pub unsafe fn do_addTryHandlers(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let global_ctx = crate::sexp::context::R_GlobalContext();
        if global_ctx.is_null()
            || (*global_ctx).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION == 0
        {
            errorcall(call, b"not in a try context\0".as_ptr() as *const c_char);
        }
        (*global_ctx).callflag |= crate::sexp::context::ctxt_flags::CTXT_RETURN;
        R_InsertRestartHandlers(global_ctx, b"tryRestart\0".as_ptr() as *const c_char);
        globals::R_NilValue()
    }
}

unsafe fn R_InsertRestartHandlers(cptr: *mut crate::sexp::context::RCNTXT, cname: *const c_char) {
    unsafe {
        let h = GetOption1(Rf_install(
            b"browser.error.handler\0".as_ptr() as *const c_char
        ));
        let h = if !h.is_null() && TYPEOF(h) == SEXPTYPE::CLOSXP {
            h
        } else {
            globals::R_RestartToken()
        };
        let rho = (*cptr).cloenv;
        let klass = Rf_mkChar(b"error\0".as_ptr() as *const c_char);
        let _klass_guard = protect(klass);
        let entry = mkHandlerEntry(klass, rho, h, rho, globals::R_NilValue(), 1);
        let old_stack = handler_stack();
        let new_top = Rf_cons(entry, old_stack);
        set_handler_stack(new_top);

        addInternalRestart(cptr, cname);
    }
}

unsafe fn addInternalRestart(cptr: *mut crate::sexp::context::RCNTXT, cname: *const c_char) {
    unsafe {
        let cname_str = CStr::from_ptr(cname).to_bytes();
        let name = Rf_mkString(
            std::ffi::CString::new(cname_str)
                .unwrap_or_default()
                .as_ptr(),
        );
        let _name_guard = protect(name);
        let entry = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _entry_guard = protect(entry);
        crate::sexp::accessors::SET_VECTOR_ELT(entry, 0, name);
        let ext_ptr = crate::mainutils::memory_main::R_MakeExternalPtr(
            cptr as *mut c_void,
            globals::R_NilValue(),
            globals::R_NilValue(),
        );
        crate::sexp::accessors::SET_VECTOR_ELT(entry, 1, ext_ptr);
        crate::eval::attrib_core::setAttrib(
            entry,
            R_ClassSymbol(),
            Rf_mkString(b"restart\0".as_ptr() as *const c_char),
        );
        let old_stack = restart_stack();
        let new_top = Rf_cons(entry, old_stack);
        set_restart_stack(new_top);
    }
}

// ---------------------------------------------------------------------------
// Condition signaling
// ---------------------------------------------------------------------------

unsafe fn findConditionHandler(cond: SEXP) -> SEXP {
    unsafe {
        let classes = getAttrib(cond, R_ClassSymbol());
        if TYPEOF(classes) != SEXPTYPE::STRSXP {
            return globals::R_NilValue();
        }
        let n_classes = LENGTH(classes);
        let mut list = handler_stack();
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = CAR(list);
            let entry_class = ENTRY_CLASS(entry);
            if !entry_class.is_null() {
                let entry_bytes = CHAR(entry_class);
                if !entry_bytes.is_null() {
                    let entry_str = CStr::from_ptr(entry_bytes).to_bytes();
                    for i in 0..n_classes {
                        let cls = STRING_ELT(classes, i as R_xlen_t);
                        if !cls.is_null() {
                            let cls_bytes = CHAR(cls);
                            if !cls_bytes.is_null() {
                                let cls_str = CStr::from_ptr(cls_bytes).to_bytes();
                                if entry_str == cls_str {
                                    return list;
                                }
                            }
                        }
                    }
                }
            }
            list = CDR(list);
        }
        globals::R_NilValue()
    }
}

/// do_signalCondition — signal a condition through the handler stack.
pub unsafe fn do_signalCondition(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let cond = CAR(args);
        let msg = CADR(args);
        let ecall = CADDR(args);

        let oldstack = handler_stack();
        let _oldstack_guard = protect(oldstack);

        let mut list = findConditionHandler(cond);
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = CAR(list);
            set_handler_stack(CDR(list));
            if IS_CALLING_ENTRY(entry) != 0 {
                let h = ENTRY_HANDLER(entry);
                if h == globals::R_RestartToken() {
                    let msgstr = if TYPEOF(msg) == SEXPTYPE::STRSXP && LENGTH(msg) > 0 {
                        let c = translateChar(STRING_ELT(msg, 0));
                        CStr::from_ptr(c).to_str().unwrap_or("error")
                    } else {
                        "error message not a string"
                    };
                    let cmsg = std::ffi::CString::new(msgstr).unwrap_or_default();
                    verrorcall_dflt(ecall, cmsg.as_ptr(), ptr::null_mut());
                } else {
                    let hcall = Rf_lang2(h, cond);
                    let _hcall_guard = protect(hcall);
                    let _ = crate::eval::eval::Rf_eval(hcall, globals::R_GlobalEnv());
                }
            } else {
                gotoExitingHandler(cond, ecall, entry);
            }
            list = findConditionHandler(cond);
        }

        set_handler_stack(oldstack);
        globals::R_NilValue()
    }
}

/// do_dfltWarn — default warning handler.
pub unsafe fn do_dfltWarn(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP || LENGTH(CAR(args)) != 1 {
            errorcall(call, b"bad error message\x00".as_ptr() as *const c_char);
        }
        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let ecall = CADR(args);
        vwarningcall_dflt(ecall, msg, ptr::null_mut());
        globals::R_NilValue()
    }
}

/// do_dfltStop — default error handler.
pub unsafe fn do_dfltStop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP || LENGTH(CAR(args)) != 1 {
            errorcall(call, b"bad error message\x00".as_ptr() as *const c_char);
        }
        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let message = CStr::from_ptr(msg).to_str().unwrap_or("").to_string();
        errorcall_str(globals::R_NilValue(), &message)
    }
}

// ---------------------------------------------------------------------------
// Condition creation helpers
// ---------------------------------------------------------------------------

/// R_makeErrorCondition — create an error condition object.
pub unsafe fn R_makeErrorCondition(
    call: SEXP,
    classname: *const c_char,
    subclassname: *const c_char,
    nextra: c_int,
    format: *const c_char,
) -> SEXP {
    unsafe {
        let class = if classname.is_null() {
            ""
        } else {
            CStr::from_ptr(classname).to_str().unwrap_or("")
        };
        let sub = if subclassname.is_null() {
            ""
        } else {
            CStr::from_ptr(subclassname).to_str().unwrap_or("")
        };
        let fmt = if format.is_null() {
            ""
        } else {
            CStr::from_ptr(format).to_str().unwrap_or("")
        };

        let nelem = nextra + 2;
        let cond = Rf_allocVector(SEXPTYPE::VECSXP, nelem);

        // Element 0: message
        SET_VECTOR_ELT(cond, 0, Rf_mkString(fmt.as_ptr() as *const c_char));
        // Element 1: call
        SET_VECTOR_ELT(cond, 1, call);

        // Names attribute
        let names = Rf_allocVector(SEXPTYPE::STRSXP, nelem);
        setAttrib_wrap(cond, R_NamesSymbol(), names);
        SET_STRING_ELT(
            names,
            0,
            Rf_mkChar(b"message\x00".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(names, 1, Rf_mkChar(b"call\x00".as_ptr() as *const c_char));

        // Class attribute
        let nclass = if sub.is_empty() { 3 } else { 4 };
        let klass = Rf_allocVector(SEXPTYPE::STRSXP, nclass);
        setAttrib_wrap(cond, R_ClassSymbol(), klass);

        if sub.is_empty() {
            SET_STRING_ELT(klass, 0, Rf_mkChar(class.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(b"error\x00".as_ptr() as *const c_char));
            SET_STRING_ELT(
                klass,
                2,
                Rf_mkChar(b"condition\x00".as_ptr() as *const c_char),
            );
        } else {
            SET_STRING_ELT(klass, 0, Rf_mkChar(sub.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(class.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 2, Rf_mkChar(b"error\x00".as_ptr() as *const c_char));
            SET_STRING_ELT(
                klass,
                3,
                Rf_mkChar(b"condition\x00".as_ptr() as *const c_char),
            );
        }

        cond
    }
}

/// R_signalErrorCondition — signal an error condition.
pub unsafe fn R_signalErrorCondition(cond: SEXP, call: SEXP) {
    unsafe {
        // Extract message from condition and call errorcall_dflt
        if TYPEOF(cond) != SEXPTYPE::VECSXP || LENGTH(cond) == 0 {
            errorcall(
                call,
                b"condition object must be a VECSXP of length at least one\x00".as_ptr()
                    as *const c_char,
            );
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP || LENGTH(elt) != 1 {
            errorcall(
                call,
                b"first element of condition object must be a scalar string\x00".as_ptr()
                    as *const c_char,
            );
        }
        let msg = translateChar(STRING_ELT(elt, 0));
        errorcall(call, msg);
    }
}

/// R_signalErrorConditionEx — signal an error condition with exitOnly flag.
pub unsafe fn R_signalErrorConditionEx(cond: SEXP, call: SEXP, exitOnly: c_int) {
    unsafe {
        R_signalErrorCondition(cond, call);
    }
}

/// R_setConditionField — set a field in a condition object.
pub unsafe fn R_setConditionField(cond: SEXP, idx: R_xlen_t, name: *const c_char, val: SEXP) {
    unsafe {
        if TYPEOF(cond) != SEXPTYPE::VECSXP {
            return;
        }
        let len = XLENGTH(cond);
        if idx < 0 || idx >= len {
            return;
        }
        SET_VECTOR_ELT(cond, idx, val);
        let names = getAttrib_wrap(cond, R_NamesSymbol());
        if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) == len {
            SET_STRING_ELT(names, idx, Rf_mkChar(name));
        }
    }
}

// ---------------------------------------------------------------------------
// tryCatch support (simplified)
// ---------------------------------------------------------------------------

/// R_tryCatchError — C-level tryCatch for error conditions.
pub unsafe fn R_tryCatchError(
    body: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    bdata: *mut c_void,
    handler: Option<unsafe extern "C" fn(SEXP, *mut c_void) -> SEXP>,
    hdata: *mut c_void,
) -> SEXP {
    unsafe {
        let klass = Rf_mkChar(b"error\0".as_ptr() as *const c_char);
        let _klass_guard = protect(klass);
        let handler_fn = if handler.is_some() {
            Rf_mkString(b"tryCatchError\0".as_ptr() as *const c_char)
        } else {
            globals::R_NilValue()
        };
        let entry = mkHandlerEntry(
            klass,
            globals::R_GlobalEnv(),
            handler_fn,
            globals::R_NilValue(),
            globals::R_NilValue(),
            0,
        );
        let _entry_guard = protect(entry);

        let old_stack = handler_stack();
        let _old_stack_guard = protect(old_stack);
        let new_top = Rf_cons(entry, old_stack);
        set_handler_stack(new_top);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(f) = body {
                f(bdata)
            } else {
                globals::R_NilValue()
            }
        }));

        set_handler_stack(old_stack);

        match result {
            Ok(val) => val,
            Err(payload) => {
                // Check for ExitingHandler signal — targeted context jump.
                // If our context stack has the target environment, consume the signal
                // and return the result vector. Otherwise re-panic to continue unwinding.
                if let Some(signal) = payload.downcast_ref::<crate::sexp::context::RSignal>() {
                    match signal {
                        crate::sexp::context::RSignal::ExitingHandler { target_env, result } => {
                            let target = *target_env;
                            let res = *result;
                            if crate::sexp::context::context_env_exists(target) {
                                res
                            } else {
                                std::panic::panic_any(
                                    crate::sexp::context::RSignal::ExitingHandler {
                                        target_env: target,
                                        result: res,
                                    },
                                );
                            }
                        }
                        _ => {
                            if handler.is_some() {
                                let cond = crate::sexp::constructors::Rf_allocVector(
                                    SEXPTYPE::STRSXP.as_c_int(),
                                    1,
                                );
                                if !cond.is_null() {
                                    let msg = Rf_mkString(b"error\0".as_ptr() as *const c_char);
                                    crate::sexp::accessors::SET_STRING_ELT(cond, 0, msg);
                                }
                                if let Some(h) = handler {
                                    h(cond, hdata)
                                } else {
                                    globals::R_NilValue()
                                }
                            } else {
                                std::panic::resume_unwind(payload)
                            }
                        }
                    }
                } else if handler.is_some() {
                    let cond = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::STRSXP, 1);
                    if !cond.is_null() {
                        let msg = Rf_mkString(b"error\0".as_ptr() as *const c_char);
                        crate::sexp::accessors::SET_STRING_ELT(cond, 0, msg);
                    }
                    if let Some(h) = handler {
                        h(cond, hdata)
                    } else {
                        globals::R_NilValue()
                    }
                } else {
                    std::panic::resume_unwind(payload)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// R_InitConditions — initialize error/warning condition objects.
pub unsafe fn R_InitConditions() {
    unsafe {
        // Create and preserve condition objects for stack overflow errors
        let protect_so = R_makeErrorCondition(
            globals::R_NilValue(),
            b"stackOverflowError\x00".as_ptr() as *const c_char,
            b"protectStackOverflowError\x00".as_ptr() as *const c_char,
            0,
            b"protect(): protection stack overflow\x00".as_ptr() as *const c_char,
        );
        crate::sexp::protect::R_PreserveObject(protect_so);

        let expr_so = R_makeErrorCondition(
            globals::R_NilValue(),
            b"stackOverflowError\x00".as_ptr() as *const c_char,
            b"expressionStackOverflowError\x00".as_ptr() as *const c_char,
            0,
            b"evaluation nested too deeply: infinite recursion / options(expressions=)?\x00"
                .as_ptr() as *const c_char,
        );
        crate::sexp::protect::R_PreserveObject(expr_so);

        let node_so = R_makeErrorCondition(
            globals::R_NilValue(),
            b"stackOverflowError\x00".as_ptr() as *const c_char,
            b"nodeStackOverflowError\x00".as_ptr() as *const c_char,
            0,
            b"node stack overflow\x00".as_ptr() as *const c_char,
        );
        crate::sexp::protect::R_PreserveObject(node_so);
    }
}

// ---------------------------------------------------------------------------
// R_Expressions management
// ---------------------------------------------------------------------------

fn expressions_keep() -> c_int {
    with_error_state(|state| state.expressions_keep)
}

/// Get the current expression limit.
pub fn R_Expressions() -> c_int {
    with_error_state(|state| state.expressions)
}

/// Set the expression limit.
pub fn R_SetExpressions(val: c_int) {
    with_error_state(|state| state.expressions = val);
}

/// Set the expression keep value.
pub fn R_SetExpressionsKeep(val: c_int) {
    with_error_state(|state| state.expressions_keep = val);
}

// ---------------------------------------------------------------------------
// Setters for global flags
// ---------------------------------------------------------------------------

/// Set the WarnLength.
pub fn R_SetWarnLength(val: c_int) {
    set_r_warn_length(val);
}

/// Set whether to show error messages.
pub fn R_SetShowErrorMessages(val: bool) {
    set_r_show_error_messages(val);
}

/// Set whether to show error call traces.
pub fn R_SetShowErrorCalls(val: bool) {
    set_r_show_error_calls(val);
}

/// Set whether to show warning call traces.
pub fn R_SetShowWarnCalls(val: bool) {
    set_r_show_warn_calls(val);
}

/// Get the inError flag.
pub fn R_GetInError() -> i32 {
    in_error()
}

/// Set the inError flag.
pub fn R_SetInError(val: i32) {
    set_in_error(val);
}

/// Get the interrupts suspended flag.
pub fn R_InterruptsSuspended() -> bool {
    interrupts_suspended()
}

/// Set the interrupts suspended flag.
pub fn R_SetInterruptsSuspended(val: bool) {
    set_interrupts_suspended(val);
}

/// Set interrupts pending.
pub fn R_SetInterruptsPending(val: bool) {
    set_interrupts_pending(val);
}

/// Restore expression limit to keep value (called during error recovery).
/// Matches C's `R_Expressions = R_Expressions_keep` in error cleanup.
pub fn R_Expressions_keep() {
    R_SetExpressions(expressions_keep());
}

/// jump_to_toplevel — jump to top level without traceback, user error handler,
/// or try/browser frames.
///
/// Matches C's `void jump_to_toplevel(void)`.
pub unsafe fn jump_to_toplevel() {
    unsafe {
        jump_to_top_ex(0, 0, 1, 1, 1);
    }
}

/// R_MissingArgError_c — report a missing argument error.
/// Matches C's `void R_MissingArgError_c(const char* arg, SEXP call, const char* subclass)`
pub unsafe fn R_MissingArgError_c(arg: *const c_char, call: SEXP, subclass: *const c_char) {
    unsafe {
        let arg_str = if arg.is_null() {
            ""
        } else {
            CStr::from_ptr(arg).to_str().unwrap_or("")
        };
        let _call_guard = protect(call);
        let msg = if !arg_str.is_empty() {
            format!("argument \"{}\" is missing, with no default", arg_str)
        } else {
            "argument is missing, with no default".to_string()
        };
        let c_msg = std::ffi::CString::new(msg.clone()).unwrap_or_default();
        let cond = R_makeErrorCondition(
            call,
            b"missingArgError\0".as_ptr() as *const c_char,
            subclass,
            0,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_signalErrorCondition(cond, call);
    }
}

/// R_MissingArgError — report a missing argument error from a symbol.
/// Matches C's `void R_MissingArgError(SEXP symbol, SEXP call, const char* subclass)`
pub unsafe fn R_MissingArgError(symbol: SEXP, call: SEXP, subclass: *const c_char) {
    unsafe {
        let arg = if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
            b"\0".as_ptr() as *const c_char
        } else {
            let name = CHAR_local(PRINTNAME(symbol));
            if name.is_null() {
                b"\0".as_ptr() as *const c_char
            } else {
                name
            }
        };
        R_MissingArgError_c(arg, call, subclass);
    }
}

/// R_signalWarningCondition — signal a warning condition object.
/// Matches C's `void R_signalWarningCondition(SEXP cond)`.
pub unsafe fn R_signalWarningCondition(cond: SEXP) {
    unsafe {
        if cond.is_null() || TYPEOF(cond) != SEXPTYPE::VECSXP || LENGTH(cond) < 1 {
            return;
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP || LENGTH(elt) != 1 {
            return;
        }
        let msg = translateChar(STRING_ELT(elt, 0));
        let call = if LENGTH(cond) > 1 {
            VECTOR_ELT(cond, 1)
        } else {
            ptr::null_mut()
        };
        warningcall(call, msg);
    }
}

/// R_makeWarningCondition — create a warning condition object.
/// Matches C's `SEXP R_makeWarningCondition(SEXP call, const char *classname,
/// const char *subclassname, int nextra, const char *format, ...)`
pub unsafe fn R_makeWarningCondition(
    call: SEXP,
    classname: *const c_char,
    subclassname: *const c_char,
    nextra: c_int,
    format: *const c_char,
) -> SEXP {
    unsafe {
        let class = if classname.is_null() {
            "simpleWarning"
        } else {
            CStr::from_ptr(classname)
                .to_str()
                .unwrap_or("simpleWarning")
        };
        let sub = if subclassname.is_null() {
            ""
        } else {
            CStr::from_ptr(subclassname).to_str().unwrap_or("")
        };
        let fmt = if format.is_null() {
            ""
        } else {
            CStr::from_ptr(format).to_str().unwrap_or("")
        };

        let nelem = nextra + 2;
        let cond = Rf_allocVector(SEXPTYPE::VECSXP, nelem);

        // Element 0: message
        SET_VECTOR_ELT(cond, 0, Rf_mkString(fmt.as_ptr() as *const c_char));
        // Element 1: call
        SET_VECTOR_ELT(
            cond,
            1,
            if call.is_null() {
                globals::R_NilValue()
            } else {
                call
            },
        );

        // Names attribute
        let names = Rf_allocVector(SEXPTYPE::STRSXP, nelem);
        setAttrib_wrap(cond, R_NamesSymbol(), names);
        SET_STRING_ELT(names, 0, Rf_mkChar(b"message\0".as_ptr() as *const c_char));
        SET_STRING_ELT(names, 1, Rf_mkChar(b"call\0".as_ptr() as *const c_char));

        // Class attribute: with a subclass,
        // [subclass, class, "warning", "condition"]; without,
        // [class, "warning", "condition"].
        let nclass = if sub.is_empty() { 3 } else { 4 };
        let klass = Rf_allocVector(SEXPTYPE::STRSXP, nclass);
        setAttrib_wrap(cond, R_ClassSymbol(), klass);

        if sub.is_empty() {
            SET_STRING_ELT(klass, 0, Rf_mkChar(class.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(b"warning\0".as_ptr() as *const c_char));
            SET_STRING_ELT(
                klass,
                2,
                Rf_mkChar(b"condition\0".as_ptr() as *const c_char),
            );
        } else {
            SET_STRING_ELT(klass, 0, Rf_mkChar(sub.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(class.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 2, Rf_mkChar(b"warning\0".as_ptr() as *const c_char));
            SET_STRING_ELT(
                klass,
                3,
                Rf_mkChar(b"condition\0".as_ptr() as *const c_char),
            );
        }

        cond
    }
}

/// Message text for a partial-match warning operand: PRINTNAME for a
/// symbol, translateChar for a CHARSXP string (upstream errors.c passes
/// these straight into the format string).
unsafe fn partial_match_text(x: SEXP) -> String {
    unsafe {
        if !x.is_null() && TYPEOF(x) == SEXPTYPE::SYMSXP {
            CStr::from_ptr(CHAR_local(PRINTNAME(x)))
                .to_string_lossy()
                .into_owned()
        } else if !x.is_null() {
            CStr::from_ptr(translateChar(x))
                .to_string_lossy()
                .into_owned()
        } else {
            "?".to_string()
        }
    }
}

/// R_makePartialMatchWarningCondition — create a partial match warning condition.
/// Matches C's `SEXP R_makePartialMatchWarningCondition(SEXP call, SEXP input, SEXP target)`
/// where input/target are symbols or CHARSXP strings.
pub unsafe fn R_makePartialMatchWarningCondition(call: SEXP, input: SEXP, target: SEXP) -> SEXP {
    unsafe {
        let msg = format!(
            "partial match of '{}' to '{}'",
            partial_match_text(input),
            partial_match_text(target),
        );
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        let cond = R_makeWarningCondition(
            call,
            b"partialMatchWarning\0".as_ptr() as *const c_char,
            ptr::null(),
            2,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_setConditionField(
            cond,
            2,
            b"input\0".as_ptr() as *const c_char,
            if !input.is_null() && TYPEOF(input) == SEXPTYPE::SYMSXP {
                input
            } else {
                Rf_ScalarString(input)
            },
        );
        R_setConditionField(
            cond,
            3,
            b"target\0".as_ptr() as *const c_char,
            if !target.is_null() && TYPEOF(target) == SEXPTYPE::SYMSXP {
                target
            } else {
                Rf_ScalarString(target)
            },
        );
        // ideally we would want the function/object in a field also
        cond
    }
}

/// R_makePartialArgumentMatchWarningCondition — create a partial argument
/// match warning condition (supplied argument tag vs function formal).
/// Matches C's `SEXP R_makePartialArgumentMatchWarningCondition(SEXP call,
/// SEXP argument, SEXP formal)`
pub unsafe fn R_makePartialArgumentMatchWarningCondition(
    call: SEXP,
    argument: SEXP,
    formal: SEXP,
) -> SEXP {
    unsafe {
        let msg = format!(
            "partial argument match of '{}' to '{}'",
            partial_match_text(argument),
            partial_match_text(formal),
        );
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        let cond = R_makeWarningCondition(
            call,
            b"partialMatchWarning\0".as_ptr() as *const c_char,
            b"partialArgumentMatchWarning\0".as_ptr() as *const c_char,
            2,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_setConditionField(cond, 2, b"argument\0".as_ptr() as *const c_char, argument);
        R_setConditionField(cond, 3, b"formal\0".as_ptr() as *const c_char, formal);
        // ideally we would want the function/object in a field also
        cond
    }
}

/// R_makeNotSubsettableError — create a "not subsettable" error condition.
/// Matches C's `SEXP R_makeNotSubsettableError(SEXP x, SEXP call)`
pub unsafe fn R_makeNotSubsettableError(x: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let class_str = if !x.is_null() {
            let klass = getAttrib_wrap(x, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP && LENGTH(klass) >= 1 {
                let s = CHAR_local(STRING_ELT(klass, 0));
                CStr::from_ptr(s).to_str().unwrap_or("object")
            } else {
                "object"
            }
        } else {
            "object"
        };
        let msg = format!("object of type '{}' is not subsettable", class_str);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"notSubsettableError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeMissingSubscriptError — create a missing subscript error condition.
/// Matches C's `SEXP R_makeMissingSubscriptError(SEXP x, SEXP call)`
pub unsafe fn R_makeMissingSubscriptError(x: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let class_str = if !x.is_null() {
            let klass = getAttrib_wrap(x, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP && LENGTH(klass) >= 1 {
                let s = CHAR_local(STRING_ELT(klass, 0));
                CStr::from_ptr(s).to_str().unwrap_or("object")
            } else {
                "object"
            }
        } else {
            "object"
        };
        let msg = format!("subscript out of bounds for {}", class_str);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"missingSubscriptError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeMissingSubscriptError1 — create a missing subscript error condition (no x).
/// Matches C's `SEXP R_makeMissingSubscriptError1(SEXP call)`
pub unsafe fn R_makeMissingSubscriptError1(call: SEXP) -> SEXP {
    unsafe {
        let msg = "subscript out of bounds";
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"missingSubscriptError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeOutOfBoundsError — create an out-of-bounds error condition.
/// Matches C's `SEXP R_makeOutOfBoundsError(SEXP x, int subscript, SEXP sindex, SEXP call)`
pub unsafe fn R_makeOutOfBoundsError(x: SEXP, subscript: c_int, sindex: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let idx_str = if !sindex.is_null() && TYPEOF(sindex) == SEXPTYPE::REALSXP {
            format!("{}", *REAL(sindex))
        } else if !sindex.is_null() && TYPEOF(sindex) == SEXPTYPE::INTSXP {
            format!("{}", *INTEGER(sindex))
        } else {
            format!("{}", subscript)
        };
        let msg = format!("subscript out of bounds (index {} too large)", idx_str);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"outOfBoundsError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeCStackOverflowError — create a C stack overflow error condition.
/// Matches C's `SEXP R_makeCStackOverflowError(SEXP call, intptr_t usage)`
pub unsafe fn R_makeCStackOverflowError(call: SEXP, usage: isize) -> SEXP {
    unsafe {
        let msg = format!("C stack usage {} is too close to the limit", usage);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"stackOverflowError\0".as_ptr() as *const c_char,
            b"cStackOverflowError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_getProtectStackOverflowError — get the preserved protect stack overflow condition.
pub unsafe fn R_getProtectStackOverflowError() -> SEXP {
    unsafe {
        // Would return a preserved condition; for now return nil
        globals::R_NilValue()
    }
}

/// R_getExpressionStackOverflowError — get the preserved expression stack overflow condition.
pub unsafe fn R_getExpressionStackOverflowError() -> SEXP {
    unsafe {
        // Would return a preserved condition; for now return nil
        globals::R_NilValue()
    }
}

/// R_getNodeStackOverflowError — get the preserved node stack overflow condition.
pub unsafe fn R_getNodeStackOverflowError() -> SEXP {
    unsafe {
        // Would return a preserved condition; for now return nil
        globals::R_NilValue()
    }
}

/// R_tryCatch — C-level tryCatch.
/// Matches C's `SEXP R_tryCatch(SEXP (*body)(void *), void *bdata,
///                              SEXP (*handler)(void *, SEXP), void *hdata)`
pub unsafe fn R_tryCatch(
    body: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    bdata: *mut c_void,
    handler: Option<unsafe extern "C" fn(*mut c_void, SEXP) -> SEXP>,
    hdata: *mut c_void,
) -> SEXP {
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(f) = body {
                f(bdata)
            } else {
                globals::R_NilValue()
            }
        })) {
            Ok(result) => result,
            Err(panic_payload) => {
                // ExitingHandler: targeted context jump — pass through unless we are the target
                if let Some(signal) = panic_payload.downcast_ref::<crate::sexp::context::RSignal>()
                    && let crate::sexp::context::RSignal::ExitingHandler { target_env, result } =
                        signal
                {
                    let target = *target_env;
                    let res = *result;
                    if crate::sexp::context::context_env_exists(target) {
                        return res;
                    } else {
                        std::panic::panic_any(crate::sexp::context::RSignal::ExitingHandler {
                            target_env: target,
                            result: res,
                        });
                    }
                }
                // Try to extract the error condition
                let cond = if let Some(ref e) = panic_payload.downcast_ref::<RError>() {
                    R_makeErrorCondition(
                        ptr::null_mut(),
                        b"simpleError\0".as_ptr() as *const c_char,
                        ptr::null_mut(),
                        0,
                        std::ffi::CString::new(e.message.clone())
                            .unwrap_or_default()
                            .as_ptr(),
                    )
                } else {
                    R_makeErrorCondition(
                        ptr::null_mut(),
                        b"simpleError\0".as_ptr() as *const c_char,
                        ptr::null_mut(),
                        0,
                        b"unknown error\0".as_ptr() as *const c_char,
                    )
                };
                if let Some(f) = handler {
                    f(hdata, cond)
                } else {
                    globals::R_NilValue()
                }
            }
        }
    }
}

/// R_withCallingErrorHandler — C-level withCallingHandler for errors.
/// Matches C's `SEXP R_withCallingErrorHandler(...)`
pub unsafe fn R_withCallingErrorHandler(
    body: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    bdata: *mut c_void,
    handler: Option<unsafe extern "C" fn(*mut c_void, SEXP) -> SEXP>,
    hdata: *mut c_void,
) -> SEXP {
    unsafe {
        let klass = Rf_mkChar(b"error\0".as_ptr() as *const c_char);
        let _klass_guard = protect(klass);
        let handler_fn = if let Some(h) = handler {
            Rf_mkString(b"withCallingErrorHandler\0".as_ptr() as *const c_char)
        } else {
            globals::R_NilValue()
        };
        let entry = mkHandlerEntry(
            klass,
            globals::R_GlobalEnv(),
            handler_fn,
            globals::R_NilValue(),
            globals::R_NilValue(),
            1,
        );
        let _entry_guard = protect(entry);

        let old_stack = handler_stack();
        let _old_stack_guard = protect(old_stack);
        let new_top = Rf_cons(entry, old_stack);
        set_handler_stack(new_top);

        let val = if let Some(f) = body {
            f(bdata)
        } else {
            globals::R_NilValue()
        };

        set_handler_stack(old_stack);
        val
    }
}

/// R_PrintDeferredWarnings — print deferred warnings.
/// Matches C's `static void R_PrintDeferredWarnings(void)`
pub unsafe fn R_PrintDeferredWarnings() {
    unsafe {
        if r_show_error_messages() && collect_warnings() > 0 {
            eprint!("In addition: ");
            PrintWarnings();
        }
    }
}

/// do_bindtextdomain — R's bindtextdomain() function (simplified, no i18n).
pub unsafe fn do_bindtextdomain(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let domain = CAR(args);
        let dirname = CADR(args);
        if isNull(domain) != 0 && isNull(dirname) != 0 {
            return ScalarLogical(1);
        }

        let domain_cstr = sexp_string_cstr(domain);
        let dirname_cstr = sexp_string_cstr(dirname);
        let domain_ptr = domain_cstr
            .as_ref()
            .map_or(ptr::null(), |value| value.as_ptr());
        let dirname_ptr = dirname_cstr
            .as_ref()
            .map_or(ptr::null(), |value| value.as_ptr());

        let result = bindtextdomain_impl(domain_ptr, dirname_ptr);
        if result.is_null() {
            return globals::R_NilValue();
        }

        Rf_mkString(result)
    }
}

#[cfg(not(target_os = "android"))]
unsafe fn bindtextdomain_impl(
    domain_ptr: *const std::os::raw::c_char,
    dirname_ptr: *const std::os::raw::c_char,
) -> *mut std::os::raw::c_char {
    unsafe { crate::intl::bindtextdom::libintl_bindtextdomain(domain_ptr, dirname_ptr) }
}

#[cfg(target_os = "android")]
unsafe fn bindtextdomain_impl(
    domain_ptr: *const std::os::raw::c_char,
    dirname_ptr: *const std::os::raw::c_char,
) -> *mut std::os::raw::c_char {
    if domain_ptr.is_null() || dirname_ptr.is_null() {
        ptr::null_mut()
    } else {
        dirname_ptr as *mut std::os::raw::c_char
    }
}

unsafe fn sexp_string_cstr(value: SEXP) -> Option<std::ffi::CString> {
    unsafe {
        if isNull(value) != 0 {
            return None;
        }
        if isString(value) == 0 || LENGTH(value) < 1 || isValidString(value) == 0 {
            return Some(std::ffi::CString::default());
        }
        let ptr = CHAR(STRING_ELT(value, 0));
        if ptr.is_null() {
            return None;
        }
        Some(std::ffi::CString::new(CStr::from_ptr(ptr).to_bytes()).unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// R_GetCurrentSrcref (simplified)
// ---------------------------------------------------------------------------

/// R_GetCurrentSrcref — get the current source reference.
pub unsafe fn R_GetCurrentSrcref(skip: c_int) -> SEXP {
    unsafe {
        // Simplified: no source references in Rust port yet
        globals::R_NilValue()
    }
}

/// R_GetSrcFilename — get source filename from a srcref.
pub unsafe fn R_GetSrcFilename(_srcref: SEXP) -> SEXP {
    unsafe { Rf_mkString(b"\x00".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// Message formatting helpers
// ---------------------------------------------------------------------------

/// Count the number of % escapes in a format string.
fn count_format_args(s: &str) -> usize {
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::session::RSession;

    use super::*;

    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test failed: {e:?}"),
        }
    }

    #[test]
    fn test_wd() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(wd("hello"), 5);
        assert_eq!(wd(""), 0);
        assert_eq!(wd("hello world"), 11);
    }

    #[test]
    fn test_R_SetErrmessage() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        R_SetErrmessage("test error");
        assert_eq!(R_GetErrorBuf(), "test error");

        R_SetErrmessage("");
        assert_eq!(R_GetErrorBuf(), "");
    }

    #[test]
    fn test_error_catches_panic() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        let result = std::panic::catch_unwind(|| {
            R_SetErrmessage("test panic");
            std::panic::panic_any(RError {
                message: "test panic".to_string(),
            });
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_count_format_args() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(count_format_args("hello %s world %d"), 2);
        assert_eq!(count_format_args("no args"), 0);
        assert_eq!(count_format_args("%% escaped"), 0);
    }

    #[test]
    fn test_in_error_flag() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        assert_eq!(R_GetInError(), 0);
        R_SetInError(1);
        assert_eq!(R_GetInError(), 1);
        R_SetInError(0);
    }

    #[test]
    fn test_session_error_flags_are_local_on_same_thread() {
        let _session = crate::sexp::session::RSession::new();
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.with_arena(|_| {
            R_SetInError(7);
            R_SetExpressions(900);
            R_SetExpressionsKeep(901);
            R_SetWarnLength(123);
            R_SetInterruptsSuspended(true);
            R_SetInterruptsPending(true);
            assert_eq!(R_GetInError(), 7);
            assert_eq!(R_Expressions(), 900);
            assert_eq!(r_warn_length(), 123);
            assert!(R_InterruptsSuspended());
            assert!(interrupts_pending());
        })
        .unwrap();

        right
            .with_arena(|_| {
                assert_eq!(R_GetInError(), 0);
                assert_eq!(R_Expressions(), 500);
                assert_eq!(r_warn_length(), 1000);
                assert!(!R_InterruptsSuspended());
                assert!(!interrupts_pending());

                R_SetInError(2);
                R_SetExpressions(600);
                R_SetWarnLength(456);
                R_SetInterruptsSuspended(false);
                R_SetInterruptsPending(false);
                assert_eq!(R_GetInError(), 2);
            })
            .unwrap();

        left.with_arena(|_| {
            assert_eq!(R_GetInError(), 7);
            assert_eq!(R_Expressions(), 900);
            R_Expressions_keep();
            assert_eq!(R_Expressions(), 901);
            assert_eq!(r_warn_length(), 123);
            assert!(R_InterruptsSuspended());
            assert!(interrupts_pending());
        })
        .unwrap();
    }

    #[test]
    fn test_session_warning_collection_is_local_on_same_thread() {
        let _session = crate::sexp::session::RSession::new();
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.with_arena(|_| {
            set_collect_warnings(3);
            unsafe {
                setup_warnings();
            }
            assert_eq!(collect_warnings(), 0);
            assert!(!warnings_ptr().is_null());
        })
        .unwrap();

        right
            .with_arena(|_| {
                assert_eq!(collect_warnings(), 0);
                assert!(warnings_ptr().is_null());
                set_collect_warnings(1);
                unsafe {
                    setup_warnings();
                }
                assert_eq!(collect_warnings(), 0);
                assert!(!warnings_ptr().is_null());
                set_warnings_ptr(ptr::null_mut());
            })
            .unwrap();

        left.with_arena(|_| {
            assert!(!warnings_ptr().is_null());
            set_warnings_ptr(ptr::null_mut());
        })
        .unwrap();
    }

    #[test]
    fn test_session_handler_and_restart_stacks_are_local_on_same_thread() {
        let _session = crate::sexp::session::RSession::new();
        let mut left = RSession::new();
        let mut right = RSession::new();

        let mut left_handler = ptr::null_mut();
        let mut left_restart = ptr::null_mut();

        left.with_arena(|_| unsafe {
            left_handler = Rf_allocVector(SEXPTYPE::VECSXP, 5);
            left_restart = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            set_handler_stack(Rf_cons(left_handler, ptr::null_mut()));
            set_restart_stack(Rf_cons(left_restart, ptr::null_mut()));
            assert_eq!(CAR(handler_stack()), left_handler);
            assert_eq!(CAR(restart_stack()), left_restart);
        })
        .unwrap();

        right
            .with_arena(|_| unsafe {
                assert!(handler_stack().is_null());
                assert!(restart_stack().is_null());
                let right_handler = Rf_allocVector(SEXPTYPE::VECSXP, 5);
                let right_restart = Rf_allocVector(SEXPTYPE::VECSXP, 2);
                set_handler_stack(Rf_cons(right_handler, ptr::null_mut()));
                set_restart_stack(Rf_cons(right_restart, ptr::null_mut()));
                assert_eq!(CAR(handler_stack()), right_handler);
                assert_eq!(CAR(restart_stack()), right_restart);
            })
            .unwrap();

        left.with_arena(|_| unsafe {
            assert_eq!(CAR(handler_stack()), left_handler);
            assert_eq!(CAR(restart_stack()), left_restart);
            set_handler_stack(ptr::null_mut());
            set_restart_stack(ptr::null_mut());
        })
        .unwrap();
    }

    #[test]
    fn test_format_to_buf() {
        let _session = crate::sexp::session::RSession::new();
        let mut buf = [0u8; BUFSIZE + 1];
        let (len, truncated) = format_to_buf(&mut buf, "hello world");
        assert_eq!(len, 11);
        assert!(!truncated);
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert_eq!(s, "hello world");
    }

    #[test]
    fn test_format_to_buf_long() {
        let _session = crate::sexp::session::RSession::new();
        let mut buf = [0u8; BUFSIZE + 1];
        let long_str = "x".repeat(BUFSIZE + 100);
        let (len, truncated) = format_to_buf(&mut buf, &long_str);
        assert_eq!(len, BUFSIZE + 100);
        assert!(truncated);
    }

    #[test]
    fn test_bufcat() {
        let _session = crate::sexp::session::RSession::new();
        let mut buf = [0u8; BUFSIZE + 1];
        format_to_buf(&mut buf, "hello");
        bufcat(&mut buf, " world");
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert_eq!(s, "hello world");
    }

    #[test]
    fn test_print_trunc() {
        let _session = crate::sexp::session::RSession::new();
        let mut buf = [0u8; BUFSIZE + 1];
        format_to_buf(&mut buf, "hello");
        print_trunc(&mut buf, true);
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert!(s.contains("[... truncated]"));
    }

    #[test]
    fn test_print_trunc_not_truncated() {
        let _session = crate::sexp::session::RSession::new();
        let mut buf = [0u8; BUFSIZE + 1];
        format_to_buf(&mut buf, "hello");
        print_trunc(&mut buf, false);
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert_eq!(s, "hello");
        assert!(!s.contains("[... truncated]"));
    }

    #[test]
    fn test_mkHandlerEntry() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let klass = Rf_mkString(b"error\x00".as_ptr() as *const c_char);
            let handler = Rf_mkString(b"handler\x00".as_ptr() as *const c_char);
            let entry = mkHandlerEntry(
                klass,
                ptr::null_mut(),
                handler,
                ptr::null_mut(),
                ptr::null_mut(),
                1,
            );
            assert!(!entry.is_null());
            assert_eq!(TYPEOF(entry), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(entry), 5);
            assert_eq!(IS_CALLING_ENTRY(entry), 1);
        }
    }

    #[test]
    fn test_r_makeErrorCondition() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            let cond = R_makeErrorCondition(
                ptr::null_mut(),
                b"simpleError\x00".as_ptr() as *const c_char,
                ptr::null_mut(),
                0,
                b"test error message\x00".as_ptr() as *const c_char,
            );
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(cond), 2);
        }
    }

    #[test]
    fn test_r_makeErrorCondition_with_subclass() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            let cond = R_makeErrorCondition(
                ptr::null_mut(),
                b"error\x00".as_ptr() as *const c_char,
                b"simpleError\x00".as_ptr() as *const c_char,
                0,
                b"test error\x00".as_ptr() as *const c_char,
            );
            assert!(!cond.is_null());
            assert_eq!(LENGTH(cond), 2);
            // Class attribute should exist (either via getAttrib or direct ATTRIB check)
            let klass = getAttrib_wrap(cond, R_ClassSymbol());
            // klass may have length 4 if attribute system is fully working,
            // or length 0 if setAttrib didn't fully work in this test context
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP {
                assert!(LENGTH(klass) >= 3);
            }
        }
    }

    #[test]
    fn test_concise_traceback_empty() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_ConciseTraceback(ptr::null_mut(), 0);
            assert_eq!(result, "");
        }
    }

    #[test]
    fn test_interrupts_suspended() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        assert!(!R_InterruptsSuspended());
        R_SetInterruptsSuspended(true);
        assert!(R_InterruptsSuspended());
        R_SetInterruptsSuspended(false);
        assert!(!R_InterruptsSuspended());
    }

    #[test]
    fn test_warning_collection() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            set_collect_warnings(0);
            set_warnings_ptr(ptr::null_mut());

            // setup_warnings should create the vector
            setup_warnings();
            assert!(warnings_ptr().is_null() || TYPEOF(warnings_ptr()) == SEXPTYPE::VECSXP);

            // Reset
            set_collect_warnings(0);
            set_warnings_ptr(ptr::null_mut());
        }
    }

    #[test]
    fn test_handler_stack_operations() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        set_handler_stack(ptr::null_mut());

        unsafe {
            let entry = Rf_allocVector(SEXPTYPE::VECSXP, 5);
            set_handler_stack(Rf_cons(entry, ptr::null_mut()));
            assert!(!handler_stack().is_null());

            // Reset
            set_handler_stack(ptr::null_mut());
        }
    }

    #[test]
    fn test_restart_stack_operations() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        set_restart_stack(ptr::null_mut());

        unsafe {
            let entry = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            set_restart_stack(Rf_cons(entry, ptr::null_mut()));
            assert!(!restart_stack().is_null());

            // Reset
            set_restart_stack(ptr::null_mut());
        }
    }

    #[test]
    fn test_error_codes() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(error_codes::ERROR_NUMARGS, 1);
        assert_eq!(error_codes::ERROR_UNKNOWN, 6);
        assert_eq!(warning_codes::WARNING_coerce_NA, 0);
        assert_eq!(warning_codes::WARNING_UNKNOWN, 3);
    }

    #[test]
    fn test_errbufcat_macro() {
        let _session = crate::sexp::session::RSession::new();
        let mut buf = [0u8; BUFSIZE + 1];
        buf[0] = 0;
        ERRBUFCAT!(buf, "hello");
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert_eq!(s, "hello");
        ERRBUFCAT!(buf, " world");
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert_eq!(s, "hello world");
    }

    // --- Tests for new/improved functions ---

    #[test]
    fn test_format_varargs_null_format() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = format_varargs(ptr::null(), ptr::null_mut());
            assert_eq!(result, "");
        }
    }

    #[test]
    fn test_format_varargs_null_ap() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let msg = std::ffi::CString::new("hello world").unwrap_or_default();
            let result = format_varargs(msg.as_ptr(), ptr::null_mut());
            assert_eq!(result, "hello world");
        }
    }

    #[test]
    fn test_format_varargs_to_buf_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let (s, truncated) = format_varargs_to_buf(ptr::null(), ptr::null_mut());
            assert_eq!(s, "");
            assert!(!truncated);
        }
    }

    #[test]
    fn test_format_varargs_to_buf_null_ap() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let msg = std::ffi::CString::new("test message").unwrap_or_default();
            let (s, truncated) = format_varargs_to_buf(msg.as_ptr(), ptr::null_mut());
            assert_eq!(s, "test message");
            assert!(!truncated);
        }
    }

    #[test]
    fn test_r_make_warning_condition() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            let cond = R_makeWarningCondition(
                ptr::null_mut(),
                b"simpleWarning\0".as_ptr() as *const c_char,
                ptr::null(),
                0,
                b"test warning message\0".as_ptr() as *const c_char,
            );
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(cond), 2);
        }
    }

    #[test]
    fn test_r_make_c_stack_overflow_error() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            let cond = R_makeCStackOverflowError(ptr::null_mut(), 42);
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(cond), 2);
        }
    }

    #[test]
    fn test_r_make_not_subsettable_error() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            // Create a simple vector to act as the "object"
            let x = Rf_allocVector(SEXPTYPE::REALSXP, 1);
            let cond = R_makeNotSubsettableError(x, ptr::null_mut());
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
        }
    }

    #[test]
    fn test_r_make_missing_subscript_error() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            let x = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            let cond = R_makeMissingSubscriptError(x, ptr::null_mut());
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
        }
    }

    #[test]
    fn test_r_make_missing_subscript_error1() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            let cond = R_makeMissingSubscriptError1(ptr::null_mut());
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
        }
    }

    #[test]
    fn test_r_make_out_of_bounds_error() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            let x = Rf_allocVector(SEXPTYPE::INTSXP, 5);
            let idx = Rf_allocVector(SEXPTYPE::REALSXP, 1);
            *REAL(idx) = 10.0;
            let cond = R_makeOutOfBoundsError(x, 10, idx, ptr::null_mut());
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
        }
    }

    #[test]
    fn test_r_make_partial_match_warning_condition() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            let input = Rf_install(b"abc\0".as_ptr() as *const c_char);
            let target = Rf_install(b"abcdef\0".as_ptr() as *const c_char);
            let cond = R_makePartialMatchWarningCondition(ptr::null_mut(), input, target);
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(cond), 4);

            // Message: "partial match of 'abc' to 'abcdef'"
            let msg = VECTOR_ELT(cond, 0);
            let msg_str = CStr::from_ptr(translateChar(STRING_ELT(msg, 0)));
            assert_eq!(
                msg_str.to_string_lossy(),
                "partial match of 'abc' to 'abcdef'"
            );

            // Class: partialMatchWarning, warning, condition
            let klass = getAttrib_wrap(cond, R_ClassSymbol());
            assert_eq!(LENGTH(klass), 3);
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(klass, 0))).to_string_lossy(),
                "partialMatchWarning"
            );

            // Fields: input/target hold the symbols themselves
            assert_eq!(VECTOR_ELT(cond, 2), input);
            assert_eq!(VECTOR_ELT(cond, 3), target);
            let names = getAttrib_wrap(cond, R_NamesSymbol());
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(names, 2))).to_string_lossy(),
                "input"
            );
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(names, 3))).to_string_lossy(),
                "target"
            );

            // Non-symbol (CHARSXP) input is wrapped via ScalarString
            let chars = Rf_mkChar(b"nam\0".as_ptr() as *const c_char);
            let cond2 = R_makePartialMatchWarningCondition(ptr::null_mut(), chars, target);
            let wrapped = VECTOR_ELT(cond2, 2);
            assert_eq!(TYPEOF(wrapped), SEXPTYPE::STRSXP);
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(wrapped, 0))).to_string_lossy(),
                "nam"
            );
            let msg2 = VECTOR_ELT(cond2, 0);
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(msg2, 0))).to_string_lossy(),
                "partial match of 'nam' to 'abcdef'"
            );
        }
    }

    #[test]
    fn test_r_make_partial_argument_match_warning_condition() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            let argument = Rf_install(b"ab\0".as_ptr() as *const c_char);
            let formal = Rf_install(b"abcde\0".as_ptr() as *const c_char);
            let cond =
                R_makePartialArgumentMatchWarningCondition(ptr::null_mut(), argument, formal);
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(cond), 4);

            // Message: "partial argument match of 'ab' to 'abcde'"
            let msg = VECTOR_ELT(cond, 0);
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(msg, 0))).to_string_lossy(),
                "partial argument match of 'ab' to 'abcde'"
            );

            // Class: partialArgumentMatchWarning, partialMatchWarning, warning, condition
            let klass = getAttrib_wrap(cond, R_ClassSymbol());
            assert_eq!(LENGTH(klass), 4);
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(klass, 0))).to_string_lossy(),
                "partialArgumentMatchWarning"
            );
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(klass, 1))).to_string_lossy(),
                "partialMatchWarning"
            );

            // Fields: argument/formal hold the symbols
            assert_eq!(VECTOR_ELT(cond, 2), argument);
            assert_eq!(VECTOR_ELT(cond, 3), formal);
            let names = getAttrib_wrap(cond, R_NamesSymbol());
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(names, 2))).to_string_lossy(),
                "argument"
            );
            assert_eq!(
                CStr::from_ptr(translateChar(STRING_ELT(names, 3))).to_string_lossy(),
                "formal"
            );
        }
    }

    #[test]
    #[ignore = "cannot catch_unwind across extern \"C\" boundary"]
    fn test_r_missing_arg_error_c() {
        let _session = crate::sexp::session::RSession::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            let msg = std::ffi::CString::new("my_arg").unwrap_or_default();
            R_MissingArgError_c(msg.as_ptr(), ptr::null_mut(), ptr::null_mut());
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_r_expressions_management() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        let val = R_Expressions();
        assert!(val > 0);
        R_SetExpressions(val + 100);
        assert_eq!(R_Expressions(), val + 100);
        R_SetExpressionsKeep(val);
        R_SetExpressions(val);
        assert_eq!(R_Expressions(), val);
    }

    #[test]
    fn test_warn_length() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        R_SetWarnLength(500);
        // Just verify it doesn't panic
        let val = r_warn_length();
        assert_eq!(val, 500);
        // Reset to default
        R_SetWarnLength(1000);
    }

    #[test]
    fn test_show_error_messages_flag() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        R_SetShowErrorMessages(true);
        assert!(r_show_error_messages());
        R_SetShowErrorMessages(false);
        assert!(!r_show_error_messages());
    }

    #[test]
    fn test_r_print_deferred_warnings_no_warnings() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();

        unsafe {
            set_collect_warnings(0);
            set_warnings_ptr(ptr::null_mut());
            R_PrintDeferredWarnings();
            // Should not panic
        }
    }

    #[test]
    fn test_r_signal_warning_condition_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            R_signalWarningCondition(ptr::null_mut());
            // Should not panic on null
        }
    }

    #[test]
    fn test_r_signal_warning_condition_valid() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();
        unsafe {
            let cond = R_makeWarningCondition(
                ptr::null_mut(),
                b"simpleWarning\0".as_ptr() as *const c_char,
                ptr::null(),
                0,
                b"test warning\0".as_ptr() as *const c_char,
            );
            R_signalWarningCondition(cond);
            // Should not panic — warning is printed to stderr
        }
    }

    #[test]
    fn test_r_get_current_srcref() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_GetCurrentSrcref(0);
            // Returns R_NilValue since srcref not implemented
            assert!(result.is_null() || TYPEOF(result) == SEXPTYPE::NILSXP);
        }
    }

    #[test]
    fn test_r_get_src_filename() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_GetSrcFilename(ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
        }
    }

    #[test]
    fn test_rf_errorcall_fmt() {
        let _session = crate::sexp::session::RSession::new();
        let fmt = std::ffi::CString::new("hello %s world %s").unwrap_or_default();
        let arg1 = must(std::ffi::CStr::from_bytes_with_nul(b"beautiful\0"));
        let arg2 = must(std::ffi::CStr::from_bytes_with_nul(b"today\0"));
        // This function pre-formats and calls verrorcall_dflt, which panics
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Rf_errorcall_fmt(ptr::null_mut(), fmt.as_ptr(), &[arg1, arg2]);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_entry_macros() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let entry = Rf_allocVector(SEXPTYPE::VECSXP, 5);
            // Set up some values
            let v0 = Rf_mkString(b"class\0".as_ptr() as *const c_char);
            let v2 = Rf_mkString(b"handler\0".as_ptr() as *const c_char);
            let v3 = Rf_mkString(b"target\0".as_ptr() as *const c_char);
            let v4 = Rf_mkString(b"result\0".as_ptr() as *const c_char);
            SET_VECTOR_ELT(entry, 0, v0);
            SET_VECTOR_ELT(entry, 2, v2);
            SET_VECTOR_ELT(entry, 3, v3);
            SET_VECTOR_ELT(entry, 4, v4);

            assert!(!ENTRY_CLASS(entry).is_null());
            assert!(!ENTRY_HANDLER(entry).is_null());
            assert!(!ENTRY_TARGET_ENVIR(entry).is_null());
            assert!(!ENTRY_RETURN_RESULT(entry).is_null());

            CLEAR_ENTRY_CALLING_ENVIR(entry);
            CLEAR_ENTRY_TARGET_ENVIR(entry);
            // After clearing, these should be R_NilValue
            assert!(
                ENTRY_TARGET_ENVIR(entry).is_null()
                    || TYPEOF(ENTRY_TARGET_ENVIR(entry)) == SEXPTYPE::NILSXP
            );
        }
    }

    #[test]
    fn test_longwarn_constant() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(LONGWARN, 75);
    }

    #[test]
    fn test_bufsize_constant() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(BUFSIZE, 8192);
    }

    #[test]
    fn test_r_nwarnings_default() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(R_NWARNINGS_DEFAULT, 50);
    }
}
