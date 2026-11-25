#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/sysutils.c — system utility functions.
//!
//! This module ports the standalone system/file utility functions that don't
//! require SEXP or R interpreter internals.
//!
//! Ported standalone functions:
//!   R_HiddenFile, R_strieql, R_HomeDir, R_free_tmpnam,
//!   R_FileExists (Unix), R_FileMtime (Unix)

use crate::sexp::ffi::{FALSE, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use std::env;
use std::fs;
use std::path::Path;

// ---------------------------------------------------------------------------
// File system utilities
// ---------------------------------------------------------------------------

/// Check if a file exists.
///
/// Ported from R's `R_FileExists` (Unix version).
pub fn R_FileExists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Get file modification time.
///
/// Ported from R's `R_FileMtime` (Unix version).
/// Returns the modification time as seconds since epoch, or None on error.
pub fn R_FileMtime(path: &str) -> Option<f64> {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
        })
}

/// Check if a filename is hidden (starts with '.').
///
/// Ported from R's `R_HiddenFile`.
pub fn R_HiddenFile(name: &str) -> bool {
    name.starts_with('.')
}

// ---------------------------------------------------------------------------
// String utilities
// ---------------------------------------------------------------------------

/// Case-insensitive string equality check.
///
/// Ported from R's `R_strieql`.
pub fn R_strieql(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

// ---------------------------------------------------------------------------
// Environment utilities
// ---------------------------------------------------------------------------

/// Get the R home directory from the R_HOME environment variable.
///
/// Ported from R's `R_HomeDir`.
pub fn R_HomeDir() -> Option<String> {
    env::var("R_HOME").ok()
}

// ---------------------------------------------------------------------------
// Temp file utilities
// ---------------------------------------------------------------------------

/// Free a temporary filename allocated by R_tmpnam2.
///
/// Ported from R's `R_free_tmpnam`.
pub fn R_free_tmpnam(_name: String) {
    // In Rust, the String is freed by Drop automatically.
    // This function exists for API compatibility.
}

/// Generate a unique temporary file name.
///
/// Ported from R's `R_tmpnam2`. Tries up to 100 times to find an unused name.
pub fn R_tmpnam2(prefix: &str, tempdir: &str, fileext: &str) -> Option<String> {
    let prefix = if prefix.is_empty() { "" } else { prefix };
    let fileext = if fileext.is_empty() { "" } else { fileext };

    let pid = super::platform::process_id();

    for _ in 0..100 {
        let r1 = next_tempfile_serial()?;
        let name = format!("{}/{}{:08x}{:016x}{}", tempdir, prefix, pid, r1, fileext);

        let exists = if super::browser_files::enabled() {
            super::browser_files::contains_current(&name)
                || super::browser_files::dir_exists_current(&name)
        } else {
            R_FileExists(&name)
        };
        if !exists {
            return Some(name);
        }
    }

    None
}

/// Process-wide name allocation is independent of every session's R RNG.
/// A per-session counter collides before either session creates its file.
fn next_tempfile_serial() -> Option<usize> {
    use std::sync::atomic::AtomicUsize;
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    crate::sexp::atomic_counter::update!(&NEXT, |value: usize| value.checked_add(1)).ok()
}

// ---------------------------------------------------------------------------
// SEXP-dependent functions
// ---------------------------------------------------------------------------

use std::os::raw::{c_char, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

/// checkArity — stub, no-op.
#[inline(always)]
fn checkArity(op: SEXP, args: SEXP) {
    crate::mainutils::relop::checkArity(op, args)
}

/// isString check — STRSXP type.
#[inline(always)]
fn isString(x: SEXP) -> bool {
    {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        TYPEOF(x) == SEXPTYPE::STRSXP
    }
}

#[inline(always)]
unsafe fn translateChar(s: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(s) }
}

/// Set R_Interactive flag.
pub fn R_SetInteractive(val: bool) {
    let flag = if val { TRUE } else { FALSE };
    crate::sexp::instance::with_repl_flags(|flags| flags.interactive.set(flag));
}

/// Get R_Interactive flag.
pub fn R_Interactive() -> bool {
    crate::sexp::instance::with_repl_flags(|flags| flags.interactive.get() != FALSE)
        .unwrap_or(false)
}


/// Sys.setenv() — set environment variables.
///
/// .Internal(Sys.setenv(nm, val))
pub unsafe fn do_setenv(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let nm = CAR(args);
        let val = CADR(args);

        if !isString(nm) || !isString(val) {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "wrong type for argument".to_string(),
            });
        }
        if LENGTH(nm) != LENGTH(val) {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "'names' and 'values' are of different lengths".to_string(),
            });
        }

        // Keep the legacy .Internal entry point behind the same host policy
        // as the public Sys.setenv implementation.  Without this check a
        // default session could mutate the process environment by bypassing
        // the capability-gated builtin.
        let allowed =
            crate::sexp::instance::required_current_capabilities().allow_environment_mutation;

        let n = LENGTH(val);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n as R_xlen_t);
        let _ans_guard = protect(ans);
        for i in 0..n as R_xlen_t {
            let name_c = CHAR(STRING_ELT(nm, i));
            let val_c = CHAR(STRING_ELT(val, i));
            let name_str = std::ffi::CStr::from_ptr(name_c).to_str().unwrap_or("");
            let val_str = std::ffi::CStr::from_ptr(val_c).to_str().unwrap_or("");
            if allowed {
                crate::sexp::instance::with_current_instance(|inst| {
                    (*inst)
                        .session_env
                        .borrow_mut()
                        .insert(name_str.to_string(), val_str.to_string());
                });
                if name_str == "TZ" {
                    // GNU tzcode: changing TZ invalidates the cached local zone.
                    crate::tzone::set_timezone_override(None);
                }
                *LOGICAL(ans).add(i as usize) = TRUE;
            } else {
                *LOGICAL(ans).add(i as usize) = FALSE;
            }
        }
        ans
    }
}

/// Sys.unsetenv() — unset environment variables.
///
/// .Internal(Sys.unsetenv(nm))
pub unsafe fn do_unsetenv(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let nm = CAR(args);

        if !isString(nm) {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "wrong type for argument".to_string(),
            });
        }

        let n = LENGTH(nm);
        let allowed =
            crate::sexp::instance::required_current_capabilities().allow_environment_mutation;
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n as R_xlen_t);
        let _ans_guard = protect(ans);
        for i in 0..n as R_xlen_t {
            let name_c = CHAR(STRING_ELT(nm, i));
            let name_str = std::ffi::CStr::from_ptr(name_c).to_str().unwrap_or("");
            if allowed {
                crate::sexp::instance::with_current_instance(|inst| {
                    (*inst).session_env.borrow_mut().remove(name_str);
                });
                if name_str == "TZ" {
                    crate::tzone::set_timezone_override(None);
                }
                *LOGICAL(ans).add(i as usize) = TRUE;
            } else {
                *LOGICAL(ans).add(i as usize) = FALSE;
            }
        }
        ans
    }
}




/// R_system — execute a system command and return the exit status.
///
/// Ported from R's R_system() in sysutils.c.
pub fn R_system(command: &str) -> c_int {
    use std::process::Command;

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    crate::sexp::instance::apply_session_env(&mut cmd);
    match cmd.status() {
        Ok(status) => status.code().unwrap_or(127),
        Err(_) => 127,
    }
}



/// Get the hostname of the current machine.
fn get_hostname() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::ffi::CStr;
        let mut buf = [0u8; 256];
        unsafe {
            if libc::gethostname(buf.as_mut_ptr() as *mut c_char, buf.len()) == 0 {
                CStr::from_ptr(buf.as_ptr() as *const c_char)
                    .to_str()
                    .unwrap_or("unknown")
                    .to_string()
            } else {
                "unknown".to_string()
            }
        }
    }
    // wasm32: no hostname in the sandbox; report the same neutral
    // "unknown" the native failure path uses.
    #[cfg(target_arch = "wasm32")]
    {
        "unknown".to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internal_env_mutators_respect_default_host_policy() {
        // Preserve the actual host value; this test never mutates process state.
        let name = "RPORT_INTERNAL_SETENV_POLICY_TEST";
        let before = std::env::var_os(name);
        let session = crate::sexp::session::RSession::new();
        session.with_active(|| unsafe {
            let nm = crate::sexp::constructors::mk_string_str("RPORT_INTERNAL_SETENV_POLICY_TEST");
            let _nm = protect(nm);
            let val = crate::sexp::constructors::mk_string_str("leak");
            let _val = protect(val);
            let tail = crate::sexp::constructors::Rf_cons(val, crate::sexp::globals::R_NilValue());
            let _tail = protect(tail);
            let set_args = crate::sexp::constructors::Rf_cons(nm, tail);
            let _set_args = protect(set_args);
            let unset_args =
                crate::sexp::constructors::Rf_cons(nm, crate::sexp::globals::R_NilValue());
            let _unset_args = protect(unset_args);
            let set_result = do_setenv(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                set_args,
                std::ptr::null_mut(),
            );
            assert_eq!(crate::sexp::accessors::TYPEOF(set_result), SEXPTYPE::LGLSXP);
            assert_eq!(crate::sexp::accessors::LOGICAL_ELT(set_result, 0), FALSE);
            assert_eq!(std::env::var_os(name), before);
            let unset_result = do_unsetenv(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                unset_args,
                std::ptr::null_mut(),
            );
            assert_eq!(
                crate::sexp::accessors::TYPEOF(unset_result),
                SEXPTYPE::LGLSXP
            );
            assert_eq!(crate::sexp::accessors::LOGICAL_ELT(unset_result, 0), FALSE);
            assert_eq!(std::env::var_os(name), before);
        });
    }

    #[test]
    fn test_R_HiddenFile() {
        assert!(R_HiddenFile(".gitignore"));
        assert!(R_HiddenFile("."));
        assert!(!R_HiddenFile("README"));
        assert!(!R_HiddenFile("Cargo.toml"));
    }

    #[test]
    fn test_R_strieql() {
        assert!(R_strieql("hello", "HELLO"));
        assert!(R_strieql("Hello", "hello"));
        assert!(R_strieql("", ""));
        assert!(!R_strieql("hello", "world"));
        assert!(!R_strieql("hello", "hell"));
    }

    #[test]
    fn test_R_FileExists() {
        // The current directory should exist
        assert!(R_FileExists("."));
        // A non-existent file should not exist
        assert!(!R_FileExists("/tmp/nonexistent_file_12345"));
    }

    #[test]
    fn test_R_HomeDir() {
        // May or may not be set depending on environment
        let _ = R_HomeDir();
    }

    #[test]
    fn test_R_tmpnam2() {
        let dir = std::env::temp_dir();
        let tempdir = dir.to_string_lossy();
        let name = R_tmpnam2("test", &tempdir, ".tmp");
        assert!(name.is_some());
        let name = name.unwrap_or_else(|| panic!("unexpected None in test"));
        assert!(name.contains("test"));
        assert!(
            std::path::Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
        );
    }
}
