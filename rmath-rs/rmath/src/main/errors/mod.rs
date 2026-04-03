#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/errors.c -- error handling utilities.
//!
//! This module provides real error/warning handling using `std::panic::catch_unwind`
//! with a custom `RError` panic payload, replacing C's setjmp/longjmp mechanism.
//!
//! Safe wrappers are provided for all public API items so that callers do not need
//! `unsafe` blocks. The unsafe originals live in the submodules (error, warning,
//! format, conditions).

pub mod conditions;
pub mod error;
pub mod format;
#[cfg(test)]
mod tests;
pub mod warning;

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

// Re-export the safe item
pub use format::wd;

// ---------------------------------------------------------------------------
// Safe wrappers for format::* items (pub(crate) API)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn getCurrentCall() -> crate::sexp::ffi::SEXP {
    format::getCurrentCall()
}

#[inline]
pub(crate) fn findCall() -> crate::sexp::ffi::SEXP {
    format::findCall()
}

#[inline]
pub(crate) fn checkArity(op: crate::sexp::ffi::SEXP, args: crate::sexp::ffi::SEXP) {
    format::checkArity(op, args)
}

#[inline]
pub(crate) fn isNull(s: crate::sexp::ffi::SEXP) -> c_int {
    format::isNull(s)
}

#[inline]
pub(crate) fn isValidString(s: crate::sexp::ffi::SEXP) -> c_int {
    format::isValidString(s)
}

#[inline]
pub(crate) fn isString(s: crate::sexp::ffi::SEXP) -> c_int {
    format::isString(s)
}

#[inline]
pub(crate) fn isLogical(s: crate::sexp::ffi::SEXP) -> c_int {
    format::isLogical(s)
}

#[inline]
pub(crate) fn isInteger(s: crate::sexp::ffi::SEXP) -> c_int {
    format::isInteger(s)
}

#[inline]
pub(crate) fn isReal(s: crate::sexp::ffi::SEXP) -> c_int {
    format::isReal(s)
}

#[inline]
pub(crate) fn isFunction(s: crate::sexp::ffi::SEXP) -> c_int {
    format::isFunction(s)
}

#[inline]
pub(crate) fn isLanguage(s: crate::sexp::ffi::SEXP) -> c_int {
    format::isLanguage(s)
}

#[inline]
pub(crate) fn isExpression(s: crate::sexp::ffi::SEXP) -> c_int {
    format::isExpression(s)
}

#[inline]
pub(crate) fn asLogical(s: crate::sexp::ffi::SEXP) -> c_int {
    format::asLogical(s)
}

#[inline]
pub(crate) fn CHAR_local(s: crate::sexp::ffi::SEXP) -> *const c_char {
    format::CHAR_local(s)
}

#[inline]
pub(crate) fn translateChar(s: crate::sexp::ffi::SEXP) -> *const c_char {
    format::translateChar(s)
}

#[inline]
pub(crate) fn GetOption1(sym: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    format::GetOption1(sym)
}

#[inline]
pub(crate) fn getAttrib_wrap(
    x: crate::sexp::ffi::SEXP,
    which: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    format::getAttrib_wrap(x, which)
}

#[inline]
pub(crate) fn setAttrib_wrap(
    x: crate::sexp::ffi::SEXP,
    which: crate::sexp::ffi::SEXP,
    value: crate::sexp::ffi::SEXP,
) {
    format::setAttrib_wrap(x, which, value)
}

#[inline]
pub(crate) fn ScalarLogical(x: c_int) -> crate::sexp::ffi::SEXP {
    format::ScalarLogical(x)
}

#[inline]
pub(crate) fn ScalarInteger(x: c_int) -> crate::sexp::ffi::SEXP {
    format::ScalarInteger(x)
}

#[inline]
pub(crate) fn length(x: crate::sexp::ffi::SEXP) -> c_int {
    format::length(x)
}

#[inline]
pub(crate) fn classgets(
    x: crate::sexp::ffi::SEXP,
    klass: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    format::classgets(x, klass)
}

// ---------------------------------------------------------------------------
// Safe wrappers for error::* items (public API)
// ---------------------------------------------------------------------------

/// Report an error (without call).
pub fn Rf_error(format: *const c_char) {
    unsafe { error::Rf_error(format) }
}

/// Report a formatted error (without call), with one string argument.
pub fn Rf_error1(format: *const c_char, arg: *const c_char) {
    error::Rf_error1(format, arg)
}

/// Unimplemented error -- for functions that haven't been ported yet.
pub fn Rf_error_unimplemented(name: &str) {
    error::Rf_error_unimplemented(name)
}

/// Report a formatted error with one string argument and call.
pub fn Rf_errorcall1(call: crate::sexp::ffi::SEXP, format: *const c_char, arg: *const c_char) {
    error::Rf_errorcall1(call, format, arg)
}

/// Report a formatted error with call, using printf-style formatting.
pub fn Rf_errorcall_fmt(call: crate::sexp::ffi::SEXP, format: *const c_char, args: &[&CStr]) {
    error::Rf_errorcall_fmt(call, format, args)
}

/// Check argument count matches the primitive's arity.
pub fn Rf_checkArityCall(
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    call: crate::sexp::ffi::SEXP,
) {
    unsafe { error::Rf_checkArityCall(op, args, call) }
}

/// UNIMPLEMENTED -- called from C when a feature is not yet ported.
pub fn UNIMPLEMENTED(s: *const c_char) {
    unsafe { error::UNIMPLEMENTED(s) }
}

/// WrongArgCount -- incorrect number of arguments error.
pub fn WrongArgCount(s: *const c_char) {
    unsafe { error::WrongArgCount(s) }
}

/// Report an error with a call.
pub fn errorcall(call: crate::sexp::ffi::SEXP, format: *const c_char) {
    unsafe { error::errorcall(call, format) }
}

/// Report an error with a call and pre-formatted message buffer.
pub fn errorcall_cpy(call: crate::sexp::ffi::SEXP, format: *const c_char) {
    unsafe { error::errorcall_cpy(call, format) }
}

/// R's error printf (prints to stderr).
pub fn REprintf(format: *const c_char) {
    unsafe { error::REprintf(format) }
}

/// Signal an error condition.
pub fn R_SignalError(call: crate::sexp::ffi::SEXP, format: *const c_char) {
    unsafe { error::R_SignalError(call, format) }
}

/// Check for stack overflow.
pub fn R_CheckStack() {
    unsafe { error::R_CheckStack() }
}

/// Check for stack overflow with extra space.
pub fn R_CheckStack2(extra: usize) {
    unsafe { error::R_CheckStack2(extra) }
}

/// Check for user interrupts.
pub fn R_CheckUserInterrupt() {
    unsafe { error::R_CheckUserInterrupt() }
}

/// Jump to the top-level context.
pub fn jump_to_top_ex(swap: c_int, eval: c_int, print: c_int, reset: c_int, skip: c_int) {
    unsafe { error::jump_to_top_ex(swap, eval, print, reset, skip) }
}

/// Jump to top level without traceback, user error handler.
pub fn jump_to_toplevel() {
    unsafe { error::jump_to_toplevel() }
}

/// Handle interrupt signal.
pub fn onintr() {
    unsafe { error::onintr() }
}

/// Handle interrupt signal without resume option.
pub fn onintrNoResume() {
    unsafe { error::onintrNoResume() }
}

/// Report a missing argument error from a symbol.
pub fn R_MissingArgError(
    symbol: crate::sexp::ffi::SEXP,
    call: crate::sexp::ffi::SEXP,
    subclass: *const c_char,
) {
    unsafe { error::R_MissingArgError(symbol, call, subclass) }
}

/// Report a missing argument error (C string version).
pub fn R_MissingArgError_c(
    arg: *const c_char,
    call: crate::sexp::ffi::SEXP,
    subclass: *const c_char,
) {
    unsafe { error::R_MissingArgError_c(arg, call, subclass) }
}

/// Check that the first argument's name matches the expected formal parameter.
pub fn check1arg(arg: crate::sexp::ffi::SEXP, call: crate::sexp::ffi::SEXP, formal: *const c_char) {
    error::check1arg(arg, call, formal)
}

/// Look up an error message from the database and call errorcall.
pub fn ErrorMessage(call: crate::sexp::ffi::SEXP, which_error: c_int, format: *const c_char) {
    unsafe { error::ErrorMessage(call, which_error, format) }
}

// Re-export extern "C" do_* functions for FunTab (needs extern "C" fn pointers).
pub use error::do_geterrmessage;
pub use error::do_interruptsSuspended;
pub use error::do_seterrmessage;
pub use error::do_stop;

// ---------------------------------------------------------------------------
// Safe wrappers for warning::* items (public API)
// ---------------------------------------------------------------------------

/// Issue a warning (without call).
pub fn Rf_warning(format: *const c_char) {
    unsafe { warning::Rf_warning(format) }
}

/// Issue a formatted warning without call (Rust helper).
pub fn Rf_warning1(msg: *const c_char) {
    warning::Rf_warning1(msg)
}

/// Issue a formatted warning with call (Rust helper).
pub fn Rf_warningcall1(call: crate::sexp::ffi::SEXP, msg: *const c_char) {
    warning::Rf_warningcall1(call, msg)
}

/// Issue an immediate warning (bypass collection).
pub fn Rf_warning_immediate(format: *const c_char) {
    unsafe { warning::Rf_warning_immediate(format) }
}

/// Issue a warning with call.
pub fn warningcall(call: crate::sexp::ffi::SEXP, format: *const c_char) {
    unsafe { warning::warningcall(call, format) }
}

/// Issue an immediate warning with call.
pub fn warningcall_immediate(call: crate::sexp::ffi::SEXP, format: *const c_char) {
    unsafe { warning::warningcall_immediate(call, format) }
}

/// Issue a message (R's message()).
pub fn Rf_message(format: *const c_char) {
    unsafe { warning::Rf_message(format) }
}

/// Issue a message with append flag.
pub fn Rf_message_append(format: *const c_char, append: c_int) {
    unsafe { warning::Rf_message_append(format, append) }
}

/// Issue a message with call.
pub fn messagecall(call: crate::sexp::ffi::SEXP, format: *const c_char) {
    warning::messagecall(call, format)
}

/// R's message() builtin.
pub fn do_message(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { warning::do_message(call, op, args, rho) }
}

/// Print collected warnings.
pub fn PrintWarnings() {
    warning::PrintWarnings()
}

// Re-export extern "C" do_* functions for FunTab (needs extern "C" fn pointers).
pub use warning::do_bindtextdomain;
pub use warning::do_gettext;
pub use warning::do_ngettext;
pub use warning::do_printDeferredWarnings;
pub use warning::do_warning;

/// Print deferred warnings.
pub fn R_PrintDeferredWarnings() {
    warning::R_PrintDeferredWarnings()
}

/// Return traceback without deparsing calls.
pub fn R_GetTracebackOnly(skip: c_int) -> crate::sexp::ffi::SEXP {
    unsafe { warning::R_GetTracebackOnly(skip) }
}

/// Return a concise call chain as a string.
pub fn R_ConciseTraceback(call: crate::sexp::ffi::SEXP, skip: c_int) -> String {
    warning::R_ConciseTraceback(call, skip)
}

/// Get the current source reference.
pub fn R_GetCurrentSrcref(skip: c_int) -> crate::sexp::ffi::SEXP {
    unsafe { warning::R_GetCurrentSrcref(skip) }
}

/// Get source filename from a srcref.
pub fn R_GetSrcFilename(srcref: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { warning::R_GetSrcFilename(srcref) }
}

/// Look up a warning message from the database and call warningcall.
pub fn WarningMessage(call: crate::sexp::ffi::SEXP, which_warn: c_int, format: *const c_char) {
    unsafe { warning::WarningMessage(call, which_warn, format) }
}

// ---------------------------------------------------------------------------
// Safe wrappers for conditions::* items (public API)
// ---------------------------------------------------------------------------

/// Create an error condition object.
pub fn R_makeErrorCondition(
    call: crate::sexp::ffi::SEXP,
    classname: *const c_char,
    subclassname: *const c_char,
    nextra: c_int,
    format: *const c_char,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_makeErrorCondition(call, classname, subclassname, nextra, format) }
}

/// Signal an error condition.
pub fn R_signalErrorCondition(cond: crate::sexp::ffi::SEXP, call: crate::sexp::ffi::SEXP) {
    unsafe { conditions::R_signalErrorCondition(cond, call) }
}

/// Signal an error condition with exitOnly flag.
pub fn R_signalErrorConditionEx(
    cond: crate::sexp::ffi::SEXP,
    call: crate::sexp::ffi::SEXP,
    exitOnly: c_int,
) {
    unsafe { conditions::R_signalErrorConditionEx(cond, call, exitOnly) }
}

/// Set a field in a condition object.
pub fn R_setConditionField(
    cond: crate::sexp::ffi::SEXP,
    idx: crate::sexp::ffi::R_xlen_t,
    name: *const c_char,
    val: crate::sexp::ffi::SEXP,
) {
    unsafe { conditions::R_setConditionField(cond, idx, name, val) }
}

/// C-level tryCatch for error conditions.
pub fn R_tryCatchError(
    body: Option<unsafe extern "C" fn(*mut c_void) -> crate::sexp::ffi::SEXP>,
    bdata: *mut c_void,
    handler: Option<
        unsafe extern "C" fn(crate::sexp::ffi::SEXP, *mut c_void) -> crate::sexp::ffi::SEXP,
    >,
    hdata: *mut c_void,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_tryCatchError(body, bdata, handler, hdata) }
}

/// C-level tryCatch.
pub fn R_tryCatch(
    body: Option<unsafe extern "C" fn(*mut c_void) -> crate::sexp::ffi::SEXP>,
    bdata: *mut c_void,
    handler: Option<
        unsafe extern "C" fn(*mut c_void, crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP,
    >,
    hdata: *mut c_void,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_tryCatch(body, bdata, handler, hdata) }
}

/// C-level withCallingHandler for errors.
pub fn R_withCallingErrorHandler(
    body: Option<unsafe extern "C" fn(*mut c_void) -> crate::sexp::ffi::SEXP>,
    bdata: *mut c_void,
    handler: Option<
        unsafe extern "C" fn(*mut c_void, crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP,
    >,
    hdata: *mut c_void,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_withCallingErrorHandler(body, bdata, handler, hdata) }
}

/// Create a warning condition object.
pub fn R_makeWarningCondition(
    call: crate::sexp::ffi::SEXP,
    classname: *const c_char,
    nextra: c_int,
    format: *const c_char,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_makeWarningCondition(call, classname, nextra, format) }
}

/// Create a partial match warning condition.
pub fn R_makePartialMatchWarningCondition(
    call: crate::sexp::ffi::SEXP,
    argument: crate::sexp::ffi::SEXP,
    formal: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_makePartialMatchWarningCondition(call, argument, formal) }
}

/// Create a "not subsettable" error condition.
pub fn R_makeNotSubsettableError(
    x: crate::sexp::ffi::SEXP,
    call: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_makeNotSubsettableError(x, call) }
}

/// Create a missing subscript error condition.
pub fn R_makeMissingSubscriptError(
    x: crate::sexp::ffi::SEXP,
    call: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_makeMissingSubscriptError(x, call) }
}

/// Create a missing subscript error condition (no x).
pub fn R_makeMissingSubscriptError1(call: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_makeMissingSubscriptError1(call) }
}

/// Create an out-of-bounds error condition.
pub fn R_makeOutOfBoundsError(
    x: crate::sexp::ffi::SEXP,
    subscript: c_int,
    sindex: crate::sexp::ffi::SEXP,
    call: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_makeOutOfBoundsError(x, subscript, sindex, call) }
}

/// Create a C stack overflow error condition.
pub fn R_makeCStackOverflowError(
    call: crate::sexp::ffi::SEXP,
    usage: isize,
) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_makeCStackOverflowError(call, usage) }
}

/// Get the preserved protect stack overflow condition.
pub fn R_getProtectStackOverflowError() -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_getProtectStackOverflowError() }
}

/// Get the preserved expression stack overflow condition.
pub fn R_getExpressionStackOverflowError() -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_getExpressionStackOverflowError() }
}

/// Get the preserved node stack overflow condition.
pub fn R_getNodeStackOverflowError() -> crate::sexp::ffi::SEXP {
    unsafe { conditions::R_getNodeStackOverflowError() }
}

/// Initialize error/warning condition objects.
pub fn R_InitConditions() {
    unsafe { conditions::R_InitConditions() }
}

/// Create a condition handler entry.
pub fn mkHandlerEntry(
    klass: crate::sexp::ffi::SEXP,
    parentenv: crate::sexp::ffi::SEXP,
    handler: crate::sexp::ffi::SEXP,
    target: crate::sexp::ffi::SEXP,
    result: crate::sexp::ffi::SEXP,
    calling: c_int,
) -> crate::sexp::ffi::SEXP {
    conditions::mkHandlerEntry(klass, parentenv, handler, target, result, calling)
}

/// IS_CALLING_ENTRY macro.
#[inline]
pub fn IS_CALLING_ENTRY(e: crate::sexp::ffi::SEXP) -> c_int {
    unsafe { conditions::IS_CALLING_ENTRY(e) }
}

/// ENTRY_CLASS macro.
#[inline]
pub fn ENTRY_CLASS(e: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::ENTRY_CLASS(e) }
}

/// ENTRY_HANDLER macro.
#[inline]
pub fn ENTRY_HANDLER(e: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::ENTRY_HANDLER(e) }
}

/// ENTRY_TARGET_ENVIR macro.
#[inline]
pub fn ENTRY_TARGET_ENVIR(e: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::ENTRY_TARGET_ENVIR(e) }
}

/// ENTRY_RETURN_RESULT macro.
#[inline]
pub fn ENTRY_RETURN_RESULT(e: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { conditions::ENTRY_RETURN_RESULT(e) }
}

/// CLEAR_ENTRY_CALLING_ENVIR macro.
#[inline]
pub fn CLEAR_ENTRY_CALLING_ENVIR(e: crate::sexp::ffi::SEXP) {
    unsafe { conditions::CLEAR_ENTRY_CALLING_ENVIR(e) }
}

/// CLEAR_ENTRY_TARGET_ENVIR macro.
#[inline]
pub fn CLEAR_ENTRY_TARGET_ENVIR(e: crate::sexp::ffi::SEXP) {
    unsafe { conditions::CLEAR_ENTRY_TARGET_ENVIR(e) }
}

/// RESULT_SIZE for handler results.
pub use conditions::RESULT_SIZE;

// Re-export extern "C" do_* functions for FunTab (needs extern "C" fn pointers).
pub use conditions::do_addCondHands;
pub use conditions::do_addRestart;
pub use conditions::do_addTryHandlers;
pub use conditions::do_dfltStop;
pub use conditions::do_dfltWarn;
pub use conditions::do_getRestart;
pub use conditions::do_invokeRestart;
pub use conditions::do_resetCondHands;
pub use conditions::do_signalCondition;
pub use conditions::do_traceback;

/// Signal a warning condition object.
pub fn R_signalWarningCondition(cond: crate::sexp::ffi::SEXP) {
    unsafe { conditions::R_signalWarningCondition(cond) }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Total line length before splitting in warnings/errors.
pub const LONGWARN: usize = 75;

/// Default maximum warnings collected.
pub const R_NWARNINGS_DEFAULT: std::os::raw::c_int = 50;

/// Buffer size for error/warning messages.
pub const BUFSIZE: usize = 8192;

// ---------------------------------------------------------------------------
// Error state globals (pub(crate) for submodule access)
// ---------------------------------------------------------------------------

use crate::sexp::ffi::SexprecCore;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};

/// Maximum length for warning messages.
pub(crate) static R_WARN_LENGTH: AtomicI32 = AtomicI32::new(1000);

/// Whether to show error messages.
pub(crate) static R_SHOW_ERROR_MESSAGES: AtomicBool = AtomicBool::new(true);

/// Whether to show error call traces.
pub(crate) static R_SHOW_ERROR_CALLS: AtomicBool = AtomicBool::new(false);

/// Whether to show warning call traces.
pub(crate) static R_SHOW_WARN_CALLS: AtomicBool = AtomicBool::new(false);

/// Number of characters shown in concise tracebacks.
pub(crate) static R_NSHOWCALLS: usize = 512;

/// Maximum number of calls shown in concise traceback.
pub(crate) static R_MAXCALLS: std::os::raw::c_int = 50;

/// Whether we're inside an error handler.
pub(crate) static IN_ERROR: AtomicI32 = AtomicI32::new(0);
/// Whether we're inside a warning handler.
pub(crate) static IN_WARNING: AtomicI32 = AtomicI32::new(0);
/// Whether we're printing warnings.
pub(crate) static IN_PRINT_WARNINGS: AtomicI32 = AtomicI32::new(0);
/// Whether to issue warnings immediately.
pub(crate) static IMMEDIATE_WARNING: AtomicBool = AtomicBool::new(false);
/// Whether to suppress break on warnings.
pub(crate) static NO_BREAK_WARNING: AtomicBool = AtomicBool::new(false);

/// Whether interrupts are suspended.
pub(crate) static R_INTERRUPTS_SUSPENDED: AtomicBool = AtomicBool::new(false);
/// Whether interrupts are pending.
pub(crate) static R_INTERRUPTS_PENDING: AtomicBool = AtomicBool::new(false);

/// Number of warnings collected so far.
pub(crate) static R_COLLECT_WARNINGS: AtomicI32 = AtomicI32::new(0);
/// Maximum number of warnings to collect.
pub(crate) static R_NWARNINGS: AtomicI32 = AtomicI32::new(R_NWARNINGS_DEFAULT);
/// R_Warnings: the vector of collected warning calls.
pub(crate) static R_WARNINGS: AtomicPtr<SexprecCore> = AtomicPtr::new(ptr::null_mut());

/// Expression limit.
static R_EXPRESSIONS: AtomicI32 = AtomicI32::new(500);
/// Expression keep value (for error recovery).
static R_EXPRESSIONS_KEEP: AtomicI32 = AtomicI32::new(500);

// ---------------------------------------------------------------------------
// Error buffer (thread-local)
// ---------------------------------------------------------------------------

thread_local! {
    pub(crate) static ERRBUF: std::cell::RefCell<[u8; BUFSIZE + 1]> =
        std::cell::RefCell::new([0u8; BUFSIZE + 1]);
}

// ---------------------------------------------------------------------------
// Handler/restart stacks (thread-local)
// ---------------------------------------------------------------------------

thread_local! {
    /// Stack of condition handlers (list of handler entries).
    pub static R_HANDLER_STACK: std::cell::RefCell<crate::sexp::ffi::SEXP> =
        std::cell::RefCell::new(ptr::null_mut());

    /// Stack of restarts (list of restart entries).
    pub static R_RESTART_STACK: std::cell::RefCell<crate::sexp::ffi::SEXP> =
        std::cell::RefCell::new(ptr::null_mut());
}

// ---------------------------------------------------------------------------
// Error buffer access
// ---------------------------------------------------------------------------

/// Get the current error buffer contents as a string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_curErrorBuf() -> *const std::os::raw::c_char {
    ERRBUF.with(|buf| {
        let buf = buf.borrow();
        buf.as_ptr() as *const std::os::raw::c_char
    })
}

/// Get the current error buffer contents as a Rust String.
pub fn R_GetErrorBuf() -> String {
    ERRBUF.with(|buf| {
        let buf = buf.borrow();
        let len = buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE);
        String::from_utf8_lossy(&buf[..len]).into_owned()
    })
}

/// Set the error message buffer (Rust).
pub fn R_SetErrmessage(s: &str) {
    ERRBUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        let bytes = s.as_bytes();
        let len = bytes.len().min(BUFSIZE - 1);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf[len] = 0;
    })
}

/// Set the error message buffer (C FFI).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetErrmessage_c(s: *const std::os::raw::c_char) {
    unsafe {
        if s.is_null() {
            return;
        }
        let str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
        R_SetErrmessage(str);
    }
}

// ---------------------------------------------------------------------------
// R_Expressions management
// ---------------------------------------------------------------------------

/// Get the current expression limit.
pub fn R_Expressions() -> std::os::raw::c_int {
    R_EXPRESSIONS.load(Ordering::Relaxed)
}

/// Set the expression limit.
pub fn R_SetExpressions(val: std::os::raw::c_int) {
    R_EXPRESSIONS.store(val, Ordering::Relaxed);
}

/// Set the expression keep value.
pub fn R_SetExpressionsKeep(val: std::os::raw::c_int) {
    R_EXPRESSIONS_KEEP.store(val, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Setters for global flags
// ---------------------------------------------------------------------------

/// Set the WarnLength.
pub fn R_SetWarnLength(val: std::os::raw::c_int) {
    R_WARN_LENGTH.store(val, Ordering::Relaxed);
}

/// Set whether to show error messages.
pub fn R_SetShowErrorMessages(val: bool) {
    R_SHOW_ERROR_MESSAGES.store(val, Ordering::Relaxed);
}

/// Set whether to show error call traces.
pub fn R_SetShowErrorCalls(val: bool) {
    R_SHOW_ERROR_CALLS.store(val, Ordering::Relaxed);
}

/// Set whether to show warning call traces.
pub fn R_SetShowWarnCalls(val: bool) {
    R_SHOW_WARN_CALLS.store(val, Ordering::Relaxed);
}

/// Get the inError flag.
pub fn R_GetInError() -> i32 {
    IN_ERROR.load(Ordering::Relaxed)
}

/// Set the inError flag.
pub fn R_SetInError(val: i32) {
    IN_ERROR.store(val, Ordering::Relaxed);
}

/// Get the interrupts suspended flag.
pub fn R_InterruptsSuspended() -> bool {
    R_INTERRUPTS_SUSPENDED.load(Ordering::Relaxed)
}

/// Set the interrupts suspended flag.
pub fn R_SetInterruptsSuspended(val: bool) {
    R_INTERRUPTS_SUSPENDED.store(val, Ordering::Relaxed);
}

/// Set interrupts pending.
pub fn R_SetInterruptsPending(val: bool) {
    R_INTERRUPTS_PENDING.store(val, Ordering::Relaxed);
}

/// Restore expression limit to keep value (called during error recovery).
pub fn R_Expressions_keep() {
    R_EXPRESSIONS.store(
        R_EXPRESSIONS_KEEP.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
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
