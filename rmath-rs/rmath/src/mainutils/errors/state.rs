#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Error/warning state: ErrorState accessors, constants, error buffer and
//! errmessage state, and R_Expressions management.

use super::helpers::translateChar;
use super::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Total line length before splitting in warnings/errors.
pub(super) const LONGWARN: usize = 75;

/// Default maximum warnings collected.
pub(super) const R_NWARNINGS_DEFAULT: c_int = 50;

/// Buffer size for error/warning messages.
pub const BUFSIZE: usize = 8192;

/// Number of characters shown in concise tracebacks.
pub(super) static R_NSHOWCALLS: usize = 512;

/// Maximum number of calls shown in concise traceback.
pub(super) static R_MAXCALLS: c_int = 50;

pub(super) fn with_error_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut ErrorState) -> R,
{
    instance::with_required_current_instance(|instance| f(&mut instance.error_state))
}

pub(super) fn r_warn_length() -> c_int {
    with_error_state(|state| state.warn_length)
}

pub(super) fn set_r_warn_length(val: c_int) {
    with_error_state(|state| state.warn_length = val);
}

pub(super) fn r_show_error_messages() -> bool {
    with_error_state(|state| state.show_error_messages)
}

pub(super) fn set_r_show_error_messages(val: bool) {
    with_error_state(|state| state.show_error_messages = val);
}

pub(super) fn r_show_error_calls() -> bool {
    with_error_state(|state| state.show_error_calls)
}

pub(super) fn set_r_show_error_calls(val: bool) {
    with_error_state(|state| state.show_error_calls = val);
}

/// 1-based index of the top-level expression a session script loop is
/// currently evaluating (0 = no script position is active).
pub fn toplevel_expr_no() -> usize {
    instance::with_current_instance(|inst| inst.error_state.toplevel_expr_no).unwrap_or(0)
}

pub fn set_toplevel_expr_no(no: usize) {
    instance::with_current_instance(|inst| inst.error_state.toplevel_expr_no = no);
}

/// Call attributed to warnings raised while it is set (null = no
/// override). Installed for the duration of a base-constructor builtin
/// whose upstream shape is a closure wrapping `.Internal`, so warnings
/// raised inside attribute to the wrapper's call (errors.c renders them
/// through the closure's context).
pub fn warning_call_override() -> SEXP {
    instance::with_current_instance(|inst| inst.error_state.warning_call)
        .unwrap_or(std::ptr::null_mut())
}

/// Install the warning-call attribution override, returning the previous
/// value so the caller can restore it (guard style).
pub fn set_warning_call_override(call: SEXP) -> SEXP {
    let mut previous = std::ptr::null_mut();
    instance::with_required_current_instance(|inst| {
        previous = inst.error_state.warning_call;
        inst.error_state.warning_call = call;
    });
    previous
}

pub(super) fn last_rendered_message() -> Option<String> {
    with_error_state(|state| state.last_rendered_message.clone())
}

pub(super) fn set_last_rendered_message(message: Option<String>) {
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

pub(super) fn r_show_warn_calls() -> bool {
    with_error_state(|state| state.show_warn_calls)
}

pub(super) fn set_r_show_warn_calls(val: bool) {
    with_error_state(|state| state.show_warn_calls = val);
}

pub(super) fn in_error() -> c_int {
    with_error_state(|state| state.in_error)
}

pub(super) fn set_in_error(val: c_int) {
    with_error_state(|state| state.in_error = val);
}

pub(super) fn in_warning() -> c_int {
    with_error_state(|state| state.in_warning)
}

pub(super) fn set_in_warning(val: c_int) {
    with_error_state(|state| state.in_warning = val);
}

pub(super) fn in_print_warnings() -> c_int {
    with_error_state(|state| state.in_print_warnings)
}

pub(super) fn set_in_print_warnings(val: c_int) {
    with_error_state(|state| state.in_print_warnings = val);
}

pub(super) fn immediate_warning() -> bool {
    with_error_state(|state| state.immediate_warning)
}

pub(super) fn set_immediate_warning(val: bool) {
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

/// Depth of active `suppressMessages()` frames (see
/// `ErrorState::suppress_messages`).
pub(crate) fn suppress_messages_depth() -> c_int {
    with_error_state(|state| state.suppress_messages)
}

pub(crate) fn enter_suppress_messages() {
    with_error_state(|state| state.suppress_messages += 1);
}

pub(crate) fn exit_suppress_messages() {
    with_error_state(|state| state.suppress_messages -= 1);
}

pub(super) fn set_no_break_warning(val: bool) {
    with_error_state(|state| state.no_break_warning = val);
}

pub(super) fn interrupts_suspended() -> bool {
    with_error_state(|state| state.interrupts_suspended)
}

pub(super) fn set_interrupts_suspended(val: bool) {
    with_error_state(|state| state.interrupts_suspended = val);
}

pub(super) fn interrupts_pending() -> bool {
    with_error_state(|state| state.interrupts_pending)
}

pub(super) fn set_interrupts_pending(val: bool) {
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

pub(super) fn set_collect_warnings(val: c_int) {
    with_error_state(|state| state.collect_warnings = val);
}

pub(super) fn increment_collect_warnings() {
    with_error_state(|state| state.collect_warnings += 1);
}

pub(super) fn nwarnings() -> c_int {
    with_error_state(|state| state.nwarnings)
}

pub(super) fn warnings_ptr() -> SEXP {
    with_error_state(|state| state.warnings)
}

pub(super) fn set_warnings_ptr(val: SEXP) {
    with_error_state(|state| state.warnings = val);
}

pub(super) fn handler_stack() -> SEXP {
    with_error_state(|state| state.handler_stack)
}

pub(super) fn set_handler_stack(val: SEXP) {
    with_error_state(|state| state.handler_stack = val);
}

pub(super) fn restart_stack() -> SEXP {
    with_error_state(|state| state.restart_stack)
}

pub(super) fn set_restart_stack(val: SEXP) {
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
// Error buffer access (matching C's errbuf)
// ---------------------------------------------------------------------------

/// Rstrncpy: like strncpy, but guaranteed to null-terminate.
pub(super) fn r_strncpy(dest: &mut [u8], src: &[u8], n: usize) {
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

// Re-exported so the `errors::tests` module keeps using the macro after the
// split (macro_rules! is textually scoped).
#[cfg(test)]
pub(crate) use ERRBUFCAT;

// ---------------------------------------------------------------------------
// R_Expressions management
// ---------------------------------------------------------------------------

pub(super) fn expressions_keep() -> c_int {
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
