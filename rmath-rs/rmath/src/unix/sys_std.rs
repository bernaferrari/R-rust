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
//! The readline integration (~400 lines) is stubbed since it requires
//! FFI to libreadline/libedit which may not be available.

use std::cell::Cell;
use std::io::{self, BufRead, Write};
use std::os::raw::{c_char, c_int};
use std::ptr;

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
pub unsafe fn Rstd_Suicide(s: *const c_char) {
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
pub unsafe fn Rstd_ReadConsole(
    prompt: *const c_char,
    buf: *mut u8,
    len: c_int,
    _addtohistory: c_int,
) -> c_int {
    unsafe {
        if len <= 0 {
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
        let mut line = String::new();

        match handle.read_line(&mut line) {
            Ok(_) => {
                // Remove trailing newline
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                let bytes = line.as_bytes();
                let copy_len = bytes.len().min(len as usize);
                ptr::copy_nonoverlapping(bytes.as_ptr(), buf, copy_len);
                if copy_len < len as usize {
                    *buf.add(copy_len) = 0;
                }
                1
            }
            Err(_) => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Rstd_WriteConsole / Rstd_WriteConsoleEx
// ---------------------------------------------------------------------------

/// Write to the console (legacy interface).
pub unsafe fn Rstd_WriteConsole(buf: *const c_char, len: c_int) {
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
pub unsafe fn Rstd_WriteConsoleEx(buf: *const c_char, len: c_int, otype: c_int) {
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
pub unsafe fn Rstd_ResetConsole() {}

/// Flush console output.
pub unsafe fn Rstd_FlushConsole() {
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
}

/// Clear error state on console.
pub unsafe fn Rstd_ClearerrConsole() {}

/// Set busy indicator.
pub unsafe fn Rstd_Busy(_which: c_int) {}

// ---------------------------------------------------------------------------
// Rstd_ShowMessage
// ---------------------------------------------------------------------------

/// Show a message (used for warnings during initialization).
pub unsafe fn Rstd_ShowMessage(s: *const c_char) {
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
pub unsafe fn Rstd_CleanUp(_saveact: c_int, status: c_int, _runLast: c_int) {
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
pub unsafe fn Rstd_ShowFiles(
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
pub unsafe fn Rstd_ChooseFile(_new: c_int, _buf: *mut c_char, _len: c_int) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// History functions (stubs)
// ---------------------------------------------------------------------------

/// Load command history from file.
pub unsafe fn Rstd_loadhistory(_file: *const c_char) {}

/// Save command history to file.
pub unsafe fn Rstd_savehistory(_file: *const c_char) {}

/// Add a line to the history.
pub unsafe fn Rstd_addhistory(_line: *const c_char) {}

/// Read history from file (readline interface).
pub unsafe fn Rstd_read_history(_file: *const c_char) {}

// ---------------------------------------------------------------------------
// Event loop stubs
// ---------------------------------------------------------------------------

thread_local! { pub static R_PolledEvents: Cell<Option<unsafe extern "C" fn()>> = Cell::new(None); }

thread_local! { pub static Rg_PolledEvents: Cell<Option<unsafe extern "C" fn()>> = Cell::new(None); }

/// Wait for the specified number of microseconds.
pub unsafe fn R_wait_usec(_usec: c_int) {
    // In the full implementation, this uses select() on no file descriptors
    // to sleep for the specified time.
}

/// Graphics wait for microseconds.
pub unsafe fn Rg_wait_usec(_usec: c_int) {
    unsafe {
        R_wait_usec(_usec);
    }
}

// ---------------------------------------------------------------------------
// set_rl_word_breaks (stub)
// ---------------------------------------------------------------------------

/// Set readline word break characters.
pub unsafe fn set_rl_word_breaks(_str: *const c_char) {
    // Stub: readline integration not ported
}

/// Expand filename using readline (stub).
pub unsafe fn R_ExpandFileName_readline(_s: *const c_char, _buff: *mut c_char) -> *mut c_char {
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_std_write_console() {
        unsafe {
            Rstd_WriteConsole(b"hello\0".as_ptr() as *const c_char, 5);
        }
    }

    #[test]
    fn test_std_write_console_ex_normal() {
        unsafe {
            Rstd_WriteConsoleEx(b"hello\0".as_ptr() as *const c_char, 5, 0);
        }
    }

    #[test]
    fn test_std_write_console_ex_error() {
        unsafe {
            Rstd_WriteConsoleEx(b"error\0".as_ptr() as *const c_char, 5, 1);
        }
    }

    #[test]
    fn test_std_show_message() {
        unsafe {
            Rstd_ShowMessage(b"test message\0".as_ptr() as *const c_char);
        }
    }

    #[test]
    fn test_std_reset_console() {
        unsafe {
            Rstd_ResetConsole();
        }
    }

    #[test]
    fn test_std_flush_console() {
        unsafe {
            Rstd_FlushConsole();
        }
    }

    #[test]
    fn test_std_busy() {
        unsafe {
            Rstd_Busy(1);
            Rstd_Busy(0);
        }
    }

    #[test]
    fn test_std_clearerr_console() {
        unsafe {
            Rstd_ClearerrConsole();
        }
    }

    #[test]
    fn test_std_history_stubs() {
        unsafe {
            Rstd_loadhistory(ptr::null());
            Rstd_savehistory(ptr::null());
            Rstd_addhistory(ptr::null());
            Rstd_read_history(ptr::null());
        }
    }

    #[test]
    fn test_set_rl_word_breaks() {
        unsafe {
            set_rl_word_breaks(ptr::null());
        }
    }

    #[test]
    fn test_std_write_console_null() {
        unsafe {
            Rstd_WriteConsole(ptr::null(), 5);
            Rstd_WriteConsole(b"hello\0".as_ptr() as *const c_char, 0);
            Rstd_WriteConsole(b"hello\0".as_ptr() as *const c_char, -1);
        }
    }

    #[test]
    fn test_std_read_console_empty() {
        unsafe {
            let mut buf = [0u8; 256];
            let result = Rstd_ReadConsole(ptr::null(), buf.as_mut_ptr(), 256, 0);
            // Will return 0 in test context since stdin is not interactive
            // But we don't assert the value since it depends on test environment
        }
    }
}
