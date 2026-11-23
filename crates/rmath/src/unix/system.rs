#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Per-session Unix console callbacks and file descriptor limits.

use std::env;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};

use crate::sexp::instance::with_required_current_instance;

// ---------------------------------------------------------------------------
// Function pointer types for system interface
// ---------------------------------------------------------------------------

type PtrSuicide = Option<unsafe extern "C" fn(*const c_char)>;
type PtrShowMessage = Option<unsafe extern "C" fn(*const c_char)>;
// Read-console callbacks are installed as `unsafe extern "C" fn` by callers
// outside this module, so the slot keeps that type.
type PtrReadConsole = Option<unsafe extern "C" fn(*const c_char, *mut u8, c_int, c_int) -> c_int>;
type PtrWriteConsole = Option<unsafe extern "C" fn(*const c_char, c_int)>;
type PtrWriteConsoleEx = Option<unsafe extern "C" fn(*const c_char, c_int, c_int)>;
type PtrResetConsole = Option<extern "C" fn()>;
type PtrFlushConsole = Option<extern "C" fn()>;
type PtrClearerrConsole = Option<extern "C" fn()>;
type PtrBusy = Option<extern "C" fn(c_int)>;
type PtrCleanUp = Option<extern "C" fn(c_int, c_int, c_int)>;
type PtrShowFiles = Option<
    unsafe extern "C" fn(
        c_int,
        *const *const c_char,
        *const *const c_char,
        *const c_char,
        c_int,
        *const c_char,
    ) -> c_int,
>;
type PtrChooseFile = Option<unsafe extern "C" fn(c_int, *mut c_char, c_int) -> c_int>;
type PtrHistory = Option<unsafe extern "C" fn(*const c_char)>;
type PtrEditFiles = Option<
    unsafe extern "C" fn(c_int, *const *const c_char, *const *const c_char, *const c_char) -> c_int,
>;

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub(crate) struct UnixSystemCallbacks {
    suicide: PtrSuicide,
    show_message: PtrShowMessage,
    read_console: PtrReadConsole,
    write_console: PtrWriteConsole,
    write_console_ex: PtrWriteConsoleEx,
    reset_console: PtrResetConsole,
    flush_console: PtrFlushConsole,
    clearerr_console: PtrClearerrConsole,
    busy: PtrBusy,
    cleanup: PtrCleanUp,
    show_files: PtrShowFiles,
    choose_file: PtrChooseFile,
    load_history: PtrHistory,
    save_history: PtrHistory,
    add_history: PtrHistory,
    edit_files: PtrEditFiles,
}

pub(crate) struct UnixSystemRuntimeState {
    history_file: CString,
    history_size: c_int,
    callbacks: UnixSystemCallbacks,
}

impl Default for UnixSystemRuntimeState {
    fn default() -> Self {
        Self {
            history_file: CString::new(".Rhistory").unwrap(),
            history_size: 512,
            callbacks: UnixSystemCallbacks::default(),
        }
    }
}

fn with_system_state<R>(f: impl FnOnce(&mut UnixSystemRuntimeState) -> R) -> R {
    // P1: the &mut UnixSystemRuntimeState lend spans only `f`; callers read
    // or write plain callback/config values without allocating or entering
    // the interpreter.
    with_required_current_instance(|instance| f(unsafe { &mut (*instance).unix_system_state }))
}

#[cfg(test)]
pub(crate) fn set_read_console_callback_for_tests(
    callback: Option<unsafe extern "C" fn(*const c_char, *mut u8, c_int, c_int) -> c_int>,
) {
    with_system_state(|state| {
        state.callbacks.read_console = callback;
    });
}

// ---------------------------------------------------------------------------
// Public system interface functions (dispatch through pointers)
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_Suicide(s: *const c_char) {
    if let Some(f) = with_system_state(|state| state.callbacks.suicide) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(s) };
    }
    std::process::exit(2);
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_ShowMessage(s: *const c_char) {
    if let Some(f) = with_system_state(|state| state.callbacks.show_message) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(s) };
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_ReadConsole(
    prompt: *const c_char,
    buf: *mut u8,
    len: c_int,
    addtohistory: c_int,
) -> c_int {
    if let Some(f) = with_system_state(|state| state.callbacks.read_console) {
        // SAFETY: callbacks are registered as `unsafe extern "C" fn`; the caller
        // upholds the prompt/buffer contract of the R_ReadConsole ABI.
        unsafe { f(prompt, buf, len, addtohistory) }
    } else {
        0
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_WriteConsole(buf: *const c_char, len: c_int) {
    let callbacks = with_system_state(|state| state.callbacks);
    if let Some(f) = callbacks.write_console {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(buf, len) };
    } else if let Some(f) = callbacks.write_console_ex {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(buf, len, 0) };
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_WriteConsoleEx(buf: *const c_char, len: c_int, otype: c_int) {
    let callbacks = with_system_state(|state| state.callbacks);
    if let Some(f) = callbacks.write_console {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(buf, len) };
    } else if let Some(f) = callbacks.write_console_ex {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(buf, len, otype) };
    }
}

pub fn R_ResetConsole() {
    if let Some(f) = with_system_state(|state| state.callbacks.reset_console) {
        f();
    }
}

pub fn R_FlushConsole() {
    if let Some(f) = with_system_state(|state| state.callbacks.flush_console) {
        f();
    }
}

pub fn R_ClearerrConsole() {
    if let Some(f) = with_system_state(|state| state.callbacks.clearerr_console) {
        f();
    }
}

pub fn R_Busy(which: c_int) {
    if let Some(f) = with_system_state(|state| state.callbacks.busy) {
        f(which);
    }
}

pub fn R_CleanUp(saveact: c_int, status: c_int, runLast: c_int) {
    if let Some(f) = with_system_state(|state| state.callbacks.cleanup) {
        f(saveact, status, runLast);
    }
    std::process::exit(status);
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_ShowFiles(
    nfile: c_int,
    file: *const *const c_char,
    headers: *const *const c_char,
    wtitle: *const c_char,
    del: c_int,
    pager: *const c_char,
) -> c_int {
    if let Some(f) = with_system_state(|state| state.callbacks.show_files) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(nfile, file, headers, wtitle, del, pager) }
    } else {
        0
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_ChooseFile(new: c_int, buf: *mut c_char, len: c_int) -> c_int {
    if let Some(f) = with_system_state(|state| state.callbacks.choose_file) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(new, buf, len) }
    } else {
        0
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_EditFiles(
    nfile: c_int,
    file: *const *const c_char,
    title: *const *const c_char,
    editor: *const c_char,
) -> c_int {
    if let Some(f) = with_system_state(|state| state.callbacks.edit_files) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(nfile, file, title, editor) }
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// R_setupHistory
// ---------------------------------------------------------------------------

pub fn R_setupHistory() {
    let history_file = env::var("R_HISTFILE")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| CString::new(s).ok())
        .unwrap_or_else(|| CString::new(".Rhistory").unwrap());

    let history_size = env::var("R_HISTSIZE")
        .ok()
        .and_then(|s| s.parse::<c_int>().ok())
        .filter(|&val| val >= 0)
        .unwrap_or(512);

    with_system_state(|state| {
        state.history_file = history_file;
        state.history_size = history_size;
    });
}

pub fn R_GetFDLimit() -> c_int {
    // SAFETY: `rlim` is a valid out-pointer for getrlimit.
    unsafe {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) == 0 {
                let lim = rlim.rlim_cur as usize;
                if lim > c_int::MAX as usize {
                    return c_int::MAX;
                }
                return lim as c_int;
            }
        }
        #[cfg(target_os = "macos")]
        {
            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) == 0 {
                let lim = rlim.rlim_cur as usize;
                if lim > c_int::MAX as usize {
                    return c_int::MAX;
                }
                return lim as c_int;
            }
        }
        -1
    }
}

pub fn R_EnsureFDLimit(desired: c_int) -> c_int {
    // SAFETY: `rlim` is a valid out/in pointer for getrlimit/setrlimit.
    unsafe {
        #[cfg(unix)]
        {
            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) != 0 {
                return -1;
            }

            let cur = rlim.rlim_cur as c_int;
            let desired_usize = desired as libc::rlim_t;

            if rlim.rlim_cur == libc::RLIM_INFINITY || rlim.rlim_cur >= desired_usize {
                return desired;
            }

            if rlim.rlim_max == libc::RLIM_INFINITY || rlim.rlim_max >= desired_usize {
                rlim.rlim_cur = desired_usize;
            } else {
                rlim.rlim_cur = rlim.rlim_max;
            }

            if libc::setrlimit(libc::RLIMIT_NOFILE, &rlim) != 0 {
                return cur;
            }

            rlim.rlim_cur as c_int
        }
        #[cfg(not(unix))]
        {
            let _ = desired;
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utils_flushconsole_dispatches_session_callback_and_checks_arity() {
        std::thread_local! {
            static FLUSHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }
        extern "C" fn flush() {
            FLUSHES.set(FLUSHES.get() + 1);
            crate::sexp::gengc::full_gc();
        }
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| {
            FLUSHES.set(0);
            with_system_state(|state| state.callbacks.flush_console = Some(flush));
            let routine = crate::library::utils::lookup("C_flushconsole").unwrap();
            unsafe {
                assert_eq!(
                    routine.invoke_call(&[]).unwrap(),
                    crate::sexp::globals::R_NilValue()
                );
                assert_eq!(FLUSHES.get(), 1);
                assert!(
                    routine
                        .invoke_call(&[crate::sexp::globals::R_NilValue()])
                        .is_err()
                );
                assert_eq!(FLUSHES.get(), 1);
            }
            with_system_state(|state| state.callbacks.flush_console = None);
        });
    }

    #[test]
    fn missing_show_message_callback_is_a_noop() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| {
            assert!(with_system_state(|state| state.callbacks.show_message.is_none()));
            // SAFETY: the static C string is valid for the call.
            unsafe { R_ShowMessage(c"test".as_ptr()) };
        });
    }

    #[test]
    fn test_setup_history() {
        let _session = crate::sexp::session::RSession::new();
        R_setupHistory();
        with_system_state(|state| {
            assert_eq!(state.history_file.to_string_lossy(), ".Rhistory");
            assert_eq!(state.history_size, 512);
        });
    }

    #[test]
    fn unix_system_runtime_state_is_session_local() {
        use crate::sexp::instance::{RInstance, replace_current_instance};

        let mut first = RInstance::new();
        let mut second = RInstance::new();

        unsafe extern "C" fn show_message(_message: *const c_char) {}
        unsafe {
            let previous = replace_current_instance(Some(&mut first as *mut RInstance));
            R_setupHistory();
            first.unix_system_state.history_size = 1024;
            first.unix_system_state.callbacks.show_message = Some(show_message);
            replace_current_instance(previous);

            let previous = replace_current_instance(Some(&mut second as *mut RInstance));
            R_setupHistory();
            second.unix_system_state.history_size = 2048;
            second.unix_system_state.callbacks.show_message = None;
            replace_current_instance(previous);
        }

        assert_eq!(first.unix_system_state.history_size, 1024);
        assert!(first.unix_system_state.callbacks.show_message.is_some());
        assert_eq!(second.unix_system_state.history_size, 2048);
        assert!(second.unix_system_state.callbacks.show_message.is_none());
    }

    #[test]
    fn test_get_fd_limit() {
        let limit = R_GetFDLimit();
        #[cfg(unix)]
        assert!(limit > 0);
    }

    #[test]
    fn test_ensure_fd_limit() {
        let result = R_EnsureFDLimit(256);
        #[cfg(unix)]
        assert!(result > 0 || result == -1);
    }

}
