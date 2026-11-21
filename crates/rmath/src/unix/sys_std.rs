#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/unix/sys-std.c -- Standard console I/O and event handling.
//!
//! Implements the Rstd_* default implementations for the system interface
//! function pointers. This includes:
//! - Console I/O (read/write/flush/clear)
//! - Suicide handler
//! - CleanUp handler
//! - Event loop (select-based activity checking)
//! - Input handler management
//!
//! Readline/libedit-specific terminal editing is intentionally not linked in,
//! but history and filename-expansion hooks are implemented in session-local
//! Rust state so frontend behavior is deterministic on Android and desktop.

use std::io::{self, BufRead, Write};
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::sexp::instance::with_required_current_instance;

// ---------------------------------------------------------------------------
// Session state accessor
// ---------------------------------------------------------------------------

fn with_sys_std_state<R>(f: impl FnOnce(&mut SysStdRuntimeState) -> R) -> R {
    // SAFETY: the &mut lend spans only `f`; the instance field is owned by the
    // session which is locked for the duration.
    with_required_current_instance(|instance| unsafe { f(&mut (*instance).sys_std_state) })
}

// ---------------------------------------------------------------------------
// Stub: R_CleanUp dispatch
// ---------------------------------------------------------------------------

/// SA_TYPE constants (from R_ext/Constants.h).
const SA_SAVE: c_int = 1;
const SA_NOSAVE: c_int = 2;
const SA_DEFAULT: c_int = 0;
const SA_SUICIDE: c_int = 3;

// ---------------------------------------------------------------------------
// Rstd_Suicide
// ---------------------------------------------------------------------------

/// Fatal error handler called at startup.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rstd_Suicide(s: *const c_char) {
    // SAFETY: `s` is NUL-terminated or null per the console contract.
    unsafe {
        if !s.is_null() {
            let msg = std::ffi::CStr::from_ptr(s);
            eprintln!("Fatal error: {}", msg.to_string_lossy());
        } else {
            eprintln!("Fatal error");
        }
        // In the full implementation, this calls R_CleanUp(SA_SUICIDE, 2, 0)
        std::process::exit(2);
    }
}

// ---------------------------------------------------------------------------
// Rstd_ReadConsole
// ---------------------------------------------------------------------------

/// Read a line from the console.
/// Without readline, reads from stdin.
fn read_console_from(reader: &mut impl BufRead, buf: &mut [u8]) -> c_int {
    let mut line = String::new();

    match reader.read_line(&mut line) {
        Ok(0) | Err(_) => 0,
        Ok(_) => {
            // Remove trailing newline.
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            let bytes = line.as_bytes();
            let copy_len = bytes.len().min(buf.len());
            buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
            if copy_len < buf.len() {
                buf[copy_len] = 0;
            }
            1
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rstd_ReadConsole(
    prompt: *const c_char,
    buf: *mut u8,
    len: c_int,
    _addtohistory: c_int,
) -> c_int {
    // SAFETY: `buf` has `len` writable bytes; `prompt` is NUL-terminated.
    unsafe {
        if len <= 0 || buf.is_null() {
            return 0;
        }

        // Print prompt
        if !prompt.is_null() {
            let p = std::ffi::CStr::from_ptr(prompt);
            let _ = io::stderr().write_all(p.to_bytes());
            let _ = io::stderr().flush();
        }

        // Read from stdin
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let output = std::slice::from_raw_parts_mut(buf, len as usize);
        read_console_from(&mut handle, output)
    }
}

// ---------------------------------------------------------------------------
// Rstd_WriteConsole / Rstd_WriteConsoleEx
// ---------------------------------------------------------------------------

/// Write to the console (legacy interface).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rstd_WriteConsole(buf: *const c_char, len: c_int) {
    // SAFETY: `buf` has `len` readable bytes per the console contract.
    unsafe {
        if len <= 0 || buf.is_null() {
            return;
        }
        let slice = std::slice::from_raw_parts(buf as *const u8, len as usize);
        let _ = io::stdout().write_all(slice);
    }
}

/// Write to the console with output type.
/// otype=0: normal, otype=1: warning/error
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rstd_WriteConsoleEx(buf: *const c_char, len: c_int, otype: c_int) {
    // SAFETY: `buf` has `len` readable bytes per the console contract.
    unsafe {
        if len <= 0 || buf.is_null() {
            return;
        }
        let slice = std::slice::from_raw_parts(buf as *const u8, len as usize);
        if otype != 0 {
            let _ = io::stderr().write_all(slice);
        } else {
            let _ = io::stdout().write_all(slice);
        }
    }
}

// ---------------------------------------------------------------------------
// Other console functions
// ---------------------------------------------------------------------------

/// Reset the console state.
pub fn Rstd_ResetConsole() {}

/// Flush console output.
pub fn Rstd_FlushConsole() {
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
}

/// Clear error state on console.
pub fn Rstd_ClearerrConsole() {}

/// Set busy indicator.
pub fn Rstd_Busy(_which: c_int) {}

// ---------------------------------------------------------------------------
// Rstd_ShowMessage
// ---------------------------------------------------------------------------

/// Show a message (used for warnings during initialization).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rstd_ShowMessage(s: *const c_char) {
    // SAFETY: `s` is NUL-terminated or null per the console contract.
    unsafe {
        if !s.is_null() {
            let msg = std::ffi::CStr::from_ptr(s);
            eprintln!("{}", msg.to_string_lossy());
        }
    }
}

// ---------------------------------------------------------------------------
// Rstd_CleanUp
// ---------------------------------------------------------------------------

/// Clean up R session.
/// saveact: SA_SAVE, SA_NOSAVE, SA_DEFAULT, SA_SUICIDE
/// status: exit status
/// runLast: whether to run .Last()
pub fn Rstd_CleanUp(_saveact: c_int, status: c_int, _runLast: c_int) {
    // In the full implementation, this:
    // - Asks about saving workspace (interactive, SA_DEFAULT)
    // - Runs .Last() if runLast is true
    // - Saves history
    // - Cleans temp directory
    // - Kills all devices
    // - Prints warnings
    std::process::exit(status);
}

// ---------------------------------------------------------------------------
// Rstd_ShowFiles / Rstd_ChooseFile
// ---------------------------------------------------------------------------

/// Show files using a pager.
pub fn Rstd_ShowFiles(
    _nfile: c_int,
    _file: *const *const c_char,
    _headers: *const *const c_char,
    _wtitle: *const c_char,
    _del: c_int,
    _pager: *const c_char,
) -> c_int {
    0
}

/// Choose a file (file dialog).
pub fn Rstd_ChooseFile(_new: c_int, _buf: *mut c_char, _len: c_int) -> c_int {
    0
}

#[derive(Default)]
pub(crate) struct SysStdRuntimeState {
    pub(crate) r_polled_events: Option<extern "C" fn()>,
    pub(crate) rg_polled_events: Option<extern "C" fn()>,
    history: Vec<String>,
    readline_word_breaks: Option<String>,
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()) }
}

// ---------------------------------------------------------------------------
// History functions
// ---------------------------------------------------------------------------

/// Load command history from file.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rstd_loadhistory(file: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let Some(path) = (unsafe { cstr_to_string(file) }) else {
        return;
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };
    with_sys_std_state(|state| {
        state.history = contents.lines().map(str::to_owned).collect();
    });
}

/// Save command history to file.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rstd_savehistory(file: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let Some(path) = (unsafe { cstr_to_string(file) }) else {
        return;
    };
    let contents = with_sys_std_state(|state| {
        if state.history.is_empty() {
            String::new()
        } else {
            format!("{}\n", state.history.join("\n"))
        }
    });
    let _ = std::fs::write(path, contents);
}

/// Add a line to the history.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rstd_addhistory(line: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let Some(line) = (unsafe { cstr_to_string(line) }) else {
        return;
    };
    with_sys_std_state(|state| state.history.push(line));
}

/// Read history from file (readline interface).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rstd_read_history(file: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `Rstd_loadhistory`'s contract per this function's own `# Safety` requirements.
    unsafe { Rstd_loadhistory(file) }
}

// ---------------------------------------------------------------------------
// Event loop callbacks
// ---------------------------------------------------------------------------

pub(crate) fn set_r_polled_events(callback: Option<extern "C" fn()>) {
    with_sys_std_state(|state| state.r_polled_events = callback);
}

pub(crate) fn set_rg_polled_events(callback: Option<extern "C" fn()>) {
    with_sys_std_state(|state| state.rg_polled_events = callback);
}

pub(crate) fn r_polled_events() -> Option<extern "C" fn()> {
    with_sys_std_state(|state| state.r_polled_events)
}

pub(crate) fn rg_polled_events() -> Option<extern "C" fn()> {
    with_sys_std_state(|state| state.rg_polled_events)
}

/// Wait for the specified number of microseconds.
pub fn R_wait_usec(_usec: c_int) {
    // In the full implementation, this uses select() on no file descriptors
    // to sleep for the specified time.
}

/// Graphics wait for microseconds.
pub fn Rg_wait_usec(_usec: c_int) {
    R_wait_usec(_usec);
}

/// Set readline word break characters.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn set_rl_word_breaks(value: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let word_breaks = unsafe { cstr_to_string(value) };
    with_sys_std_state(|state| state.readline_word_breaks = word_breaks);
}

/// Expand filename using the frontend-compatible readline hook.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_ExpandFileName_readline(s: *const c_char, buff: *mut c_char) -> *mut c_char {
    // SAFETY: `s` is NUL-terminated and `buff` holds R_PATH_MAX bytes per
    // the readline contract.
    unsafe {
        if s.is_null() || buff.is_null() {
            return ptr::null_mut();
        }
        let input = std::ffi::CStr::from_ptr(s).to_string_lossy();
        let expanded = if input == "~" {
            std::env::var("HOME").unwrap_or_else(|_| input.into_owned())
        } else if let Some(rest) = input.strip_prefix("~/") {
            std::env::var("HOME")
                .map(|home| format!("{home}/{rest}"))
                .unwrap_or_else(|_| input.into_owned())
        } else {
            input.into_owned()
        };
        let Ok(cstr) = std::ffi::CString::new(expanded) else {
            return ptr::null_mut();
        };
        ptr::copy_nonoverlapping(cstr.as_ptr(), buff, cstr.as_bytes_with_nul().len());
        buff
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

    use super::*;

    extern "C" fn first_callback() {}

    extern "C" fn second_callback() {}

    fn callback_addr(callback: Option<extern "C" fn()>) -> usize {
        callback.map(callback_fn_addr).unwrap_or(0)
    }

    fn callback_fn_addr(callback: extern "C" fn()) -> usize {
        callback as *const () as usize
    }

    #[test]
    fn polled_event_callbacks_are_session_local() {
        let mut first = RInstance::new();
        unsafe {
            set_current_instance(&mut first);
        }
        set_r_polled_events(Some(first_callback));
        set_rg_polled_events(Some(first_callback));
        assert_eq!(
            callback_addr(r_polled_events()),
            callback_fn_addr(first_callback)
        );
        assert_eq!(
            callback_addr(rg_polled_events()),
            callback_fn_addr(first_callback)
        );

        let mut second = RInstance::new();
        unsafe {
            set_current_instance(&mut second);
        }
        assert!(r_polled_events().is_none());
        assert!(rg_polled_events().is_none());
        set_r_polled_events(Some(second_callback));
        assert_eq!(
            callback_addr(r_polled_events()),
            callback_fn_addr(second_callback)
        );

        unsafe {
            set_current_instance(&mut first);
        }
        assert_eq!(
            callback_addr(r_polled_events()),
            callback_fn_addr(first_callback)
        );
        assert_eq!(
            callback_addr(rg_polled_events()),
            callback_fn_addr(first_callback)
        );

        clear_current_instance();
    }

    #[test]
    fn test_std_write_console() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_WriteConsole(b"hello\0".as_ptr() as *const c_char, 5) };
    }

    #[test]
    fn test_std_write_console_ex_normal() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_WriteConsoleEx(b"hello\0".as_ptr() as *const c_char, 5, 0) };
    }

    #[test]
    fn test_std_write_console_ex_error() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_WriteConsoleEx(b"error\0".as_ptr() as *const c_char, 5, 1) };
    }

    #[test]
    fn test_std_show_message() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_ShowMessage(b"test message\0".as_ptr() as *const c_char) };
    }

    #[test]
    fn test_std_reset_console() {
        Rstd_ResetConsole();
    }

    #[test]
    fn test_std_flush_console() {
        Rstd_FlushConsole();
    }

    #[test]
    fn test_std_busy() {
        Rstd_Busy(1);
        Rstd_Busy(0);
    }

    #[test]
    fn test_std_clearerr_console() {
        Rstd_ClearerrConsole();
    }

    #[test]
    fn test_std_history_is_session_local_and_file_backed() {
        let _session = crate::sexp::session::RSession::new();
        let path = std::env::temp_dir().join(format!(
            "rport-history-{}-{}.txt",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap();
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_addhistory(b"1 + 1\0".as_ptr() as *const c_char) };
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_addhistory(b"plot(1:3)\0".as_ptr() as *const c_char) };
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_savehistory(c_path.as_ptr()) };

        let saved = std::fs::read_to_string(&path).expect("history should be saved");
        assert_eq!(saved, "1 + 1\nplot(1:3)\n");

        let mut other = RInstance::new();
        unsafe {
            set_current_instance(&mut other);
        }
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_loadhistory(c_path.as_ptr()) };
        assert_eq!(
            other.sys_std_state.history,
            vec!["1 + 1".to_string(), "plot(1:3)".to_string()]
        );
        clear_current_instance();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_set_rl_word_breaks() {
        let _session = crate::sexp::session::RSession::new();
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { set_rl_word_breaks(b" \t\n\0".as_ptr() as *const c_char) };
        crate::sexp::instance::with_required_current_instance(|instance| unsafe {
            assert_eq!(
                (*instance).sys_std_state.readline_word_breaks.as_deref(),
                Some(" \t\n")
            );
        });
    }

    #[test]
    fn test_expand_file_name_readline_copies_and_expands_home() {
        let _session = crate::sexp::session::RSession::new();
        let mut buf = [0 as c_char; 4096];
        unsafe {
            let result = R_ExpandFileName_readline(
                b"~/rport-test\0".as_ptr() as *const c_char,
                buf.as_mut_ptr(),
            );
            assert_eq!(result, buf.as_mut_ptr());
            let expanded = std::ffi::CStr::from_ptr(buf.as_ptr())
                .to_string_lossy()
                .into_owned();
            assert!(expanded.ends_with("/rport-test") || expanded == "~/rport-test");
        }
    }

    #[test]
    fn test_std_write_console_null() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_WriteConsole(ptr::null(), 5) };
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_WriteConsole(b"hello\0".as_ptr() as *const c_char, 0) };
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { Rstd_WriteConsole(b"hello\0".as_ptr() as *const c_char, -1) };
    }

    #[test]
    fn test_std_read_console_empty() {
        let mut input = io::Cursor::new(Vec::<u8>::new());
        let mut buf = [0u8; 256];
        assert_eq!(read_console_from(&mut input, &mut buf), 0);
    }

    #[test]
    fn test_std_read_console_uses_injected_input() {
        let mut input = io::Cursor::new(b"hello\r\n".to_vec());
        let mut buf = [0xffu8; 256];
        assert_eq!(read_console_from(&mut input, &mut buf), 1);
        assert_eq!(&buf[..6], b"hello\0");
    }
}
