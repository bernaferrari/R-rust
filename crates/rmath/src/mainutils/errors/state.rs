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
    instance::with_required_current_instance(|instance| unsafe { f(&mut (*instance).error_state) })
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
    instance::with_current_instance(|inst| unsafe { (*inst).error_state.toplevel_expr_no })
        .unwrap_or(0)
}

pub fn set_toplevel_expr_no(no: usize) {
    instance::with_current_instance(|inst| unsafe { (*inst).error_state.toplevel_expr_no = no });
}

/// Call attributed to warnings raised while it is set (null = no
/// override). Installed for the duration of a base-constructor builtin
/// whose upstream shape is a closure wrapping `.Internal`, so warnings
/// raised inside attribute to the wrapper's call (errors.c renders them
/// through the closure's context).
pub fn warning_call_override() -> SEXP {
    instance::with_current_instance(|inst| unsafe { (*inst).error_state.warning_call.as_raw() })
        .unwrap_or(std::ptr::null_mut())
}

/// Retain the original runtime and previous attribution through cleanup.
pub struct WarningCallGuard {
    owner: crate::sexp::owner::OwnerPin,
    previous: Option<crate::sexp::instance::RuntimeValue>,
}

impl Drop for WarningCallGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            // Cleanup restores the original physical runtime even if a callback
            // revoked it or changed the ambient session.
            unsafe {
                (*self.owner.as_ptr()).error_state.warning_call = previous;
            }
        }
    }
}

pub(super) fn error_scope_pin() -> crate::sexp::owner::OwnerPin {
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    owner
        .pin()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .unwrap_or_else(|| crate::sexp::context::r_error("error scope requires a managed runtime"))
}

pub fn warning_call_guard(call: SEXP) -> WarningCallGuard {
    let owner = error_scope_pin();
    let next = unsafe { crate::sexp::instance::RuntimeValue::from_raw_in(owner.as_ptr(), call) };
    let previous =
        unsafe { std::mem::replace(&mut (*owner.as_ptr()).error_state.warning_call, next) };
    WarningCallGuard {
        owner,
        previous: Some(previous),
    }
}

fn error_value(value: SEXP) -> crate::sexp::instance::RuntimeValue {
    instance::with_required_current_instance(|instance| unsafe {
        crate::sexp::instance::RuntimeValue::from_raw_in(instance, value)
    })
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

/// Record whether the current error was raised with an empty call
/// (`errorcall(R_NilValue)` / `call. = FALSE`).
pub fn set_error_call_less(call_less: bool) {
    with_error_state(|state| state.call_less = call_less);
}

/// Consume the call-less flag set by the most recent `verrorcall_dflt`.
pub fn take_error_call_less() -> bool {
    with_error_state(|state| {
        let flag = state.call_less;
        state.call_less = false;
        flag
    })
}

/// Record the call attributed to the error about to unwind.
///
/// `explicit` is the applied language object (GNU `errorcall(call)` from
/// matchArgs / builtin dispatch). `stop()` records the same slot with
/// `explicit = false` via `verrorcall_dflt` so tryCatch can still
/// fabricate `doTryCatch` when the raise is in the tryCatch frame.
pub fn record_error_call(call: SEXP, explicit: bool) {
    let (nframe, stored) = unsafe {
        let nframe = crate::eval::context::framedepth(crate::sexp::context::R_GlobalContext());
        let stored = if call.is_null() {
            crate::sexp::globals::R_NilValue()
        } else {
            call
        };
        (nframe, stored)
    };
    let stored = error_value(stored);
    with_error_state(|state| {
        // First explicit applied-call wins. A later explicit from
        // attribute_handler_errors (the tryCatch builtin itself) must not
        // replace matchArgs/Math1 attribution when an inner handler misses.
        if state.last_error_call_explicit {
            return;
        }
        state.last_error_call = stored;
        state.last_error_call_explicit = explicit;
        state.last_error_nframe = nframe;
    });
}

/// Consume the raise-site call recorded by [`record_error_call`].
pub fn take_recorded_error_call() -> Option<(crate::sexp::object::Sexp<'static>, bool, i32)> {
    with_error_state(|state| {
        if state.last_error_call.is_null() {
            return None;
        }
        let recorded = (
            state
                .last_error_call
                .take_owned()
                .expect("occupied error call"),
            state.last_error_call_explicit,
            state.last_error_nframe,
        );
        state.last_error_call_explicit = false;
        state.last_error_nframe = 0;
        Some(recorded)
    })
}

pub(crate) fn push_try_catch_nframe(nframe: i32) {
    with_error_state(|state| state.try_catch_nframes.push(nframe));
}

pub(crate) fn pop_try_catch_nframe() {
    with_error_state(|state| {
        state.try_catch_nframes.pop();
    });
}

pub(crate) fn try_catch_entry_nframe() -> Option<i32> {
    with_error_state(|state| state.try_catch_nframes.last().copied())
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
thread_local! {
    static SUPPRESS_WARNING_CLASSES: std::cell::RefCell<Vec<Option<Vec<String>>>> =
        std::cell::RefCell::new(Vec::new());
}

/// `None` muffles every warning. `Some` muffles only those classes.
pub(crate) fn push_suppress_warning_classes(classes: Option<Vec<String>>) {
    SUPPRESS_WARNING_CLASSES.with(|stack| stack.borrow_mut().push(classes));
}

pub(crate) fn pop_suppress_warning_classes() {
    SUPPRESS_WARNING_CLASSES.with(|stack| {
        stack.borrow_mut().pop();
    });
}

pub(crate) fn warning_class_suppressed(classes: &[String]) -> bool {
    if suppress_warnings_depth() <= 0 {
        return false;
    }
    SUPPRESS_WARNING_CLASSES.with(|stack| {
        let stack = stack.borrow();
        let Some(filter) = stack.last() else {
            return true;
        };
        match filter {
            None => true,
            Some(wanted) => classes
                .iter()
                .any(|class| wanted.iter().any(|want| want == class)),
        }
    })
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
thread_local! {
    static SUPPRESS_MESSAGE_CLASSES: std::cell::RefCell<Vec<Option<Vec<String>>>> =
        std::cell::RefCell::new(Vec::new());
}

pub(crate) fn push_suppress_message_classes(classes: Option<Vec<String>>) {
    SUPPRESS_MESSAGE_CLASSES.with(|stack| stack.borrow_mut().push(classes));
}

pub(crate) fn pop_suppress_message_classes() {
    SUPPRESS_MESSAGE_CLASSES.with(|stack| {
        stack.borrow_mut().pop();
    });
}

pub(crate) fn message_class_suppressed(classes: &[String]) -> bool {
    if suppress_messages_depth() <= 0 {
        return false;
    }
    SUPPRESS_MESSAGE_CLASSES.with(|stack| {
        let stack = stack.borrow();
        let Some(filter) = stack.last() else {
            return true;
        };
        match filter {
            None => true,
            Some(wanted) => classes
                .iter()
                .any(|class| wanted.iter().any(|want| want == class)),
        }
    })
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

/// Drop warnings collected after `n` so a muffling catcher (assertWarning)
/// does not leak them at the next statement boundary.
pub(crate) fn restore_collect_warnings(n: c_int) {
    set_collect_warnings(n);
}

pub(super) fn increment_collect_warnings() {
    with_error_state(|state| state.collect_warnings += 1);
}

pub(super) fn nwarnings() -> c_int {
    with_error_state(|state| state.nwarnings)
}

pub(crate) fn warnings_ptr() -> SEXP {
    with_error_state(|state| state.warnings.as_raw())
}

pub(super) fn set_warnings_ptr(val: SEXP) {
    let value = error_value(val);
    with_error_state(|state| state.warnings = value);
}

pub(super) fn handler_stack() -> SEXP {
    with_error_state(|state| state.handler_stack.as_raw())
}

pub(super) fn set_handler_stack(val: SEXP) {
    let value = error_value(val);
    with_error_state(|state| state.handler_stack = value);
}

pub(super) fn restart_stack() -> SEXP {
    with_error_state(|state| state.restart_stack.as_raw())
}

pub(super) fn set_restart_stack(val: SEXP) {
    let value = error_value(val);
    with_error_state(|state| state.restart_stack = value);
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
    instance::with_current_instance(|instance| unsafe {
        let state = &(*instance).error_state;
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

#[cfg(test)]
mod owned_error_scope_tests {
    use super::*;
    use crate::sexp::{instance::RuntimeValue, session::RSession};

    #[test]
    fn owned_error_consumption_moves_root_before_clearing_slot() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let value = owner.node_factory().character("recorded call").unwrap();
            let original = crate::sexp::memory::checked_projection(value.as_raw())
                .unwrap()
                .1;
            record_error_call(value.as_raw(), true);
            drop(value);
            let (recorded, explicit, _) = take_recorded_error_call().unwrap();
            assert!(explicit);
            assert!(take_recorded_error_call().is_none());
            owner.full_gc().unwrap();
            assert!(original.is_live());
            assert!(recorded.try_char_eq(b"recorded call").unwrap());
            drop(recorded);
            owner.full_gc().unwrap();
            assert!(!original.is_live());
        });
    }

    #[test]
    fn owned_error_attribution_restores_original_runtime_after_revocation_and_drop() {
        let mut original = RSession::new_for_gc_tests();
        let weak = original.owner_token().unwrap().weak_owner().unwrap();
        let pin = weak.pin().unwrap();
        let factory = weak.node_factory().unwrap();
        let previous = factory.character("previous call").unwrap();
        let original_node = crate::sexp::memory::checked_projection(previous.as_raw())
            .unwrap()
            .1;
        unsafe {
            (*pin.as_ptr()).error_state.warning_call =
                RuntimeValue::from_owned(previous.into_owned().unwrap());
        }
        let next = factory.character("override").unwrap();
        let warning = warning_call_guard(next.as_raw());
        let mathlib = super::super::mathlib_warning_call_guard(next.as_raw());
        drop(next);
        original.gc();
        assert!(original_node.is_live(), "guard owns displaced call");
        original.close();
        drop(original);
        let other = RSession::new_for_gc_tests();
        let other_before = warning_call_override();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _warning = warning;
            let _mathlib = mathlib;
            panic!("unwind attribution");
        }));
        assert!(result.is_err());
        assert_eq!(warning_call_override(), other_before);
        let restored = unsafe { (*pin.as_ptr()).error_state.warning_call.owned().unwrap() };
        assert!(restored.try_char_eq(b"previous call").unwrap());
        assert!(unsafe { (*pin.as_ptr()).error_state.mathlib_warning_call.is_null() });
        drop(pin);
        assert_eq!(weak.allocation_strong_count(), 0);
        assert!(restored.try_char_eq(b"previous call").unwrap());
        drop(other);
    }
}
