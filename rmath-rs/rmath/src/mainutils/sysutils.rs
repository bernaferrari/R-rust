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

    let pid = std::process::id();

    for _ in 0..100 {
        let r1 = rand_u32();
        let name = format!("{}/{}{:x}{:x}{}", tempdir, prefix, pid, r1, fileext);

        if !R_FileExists(&name) {
            return Some(name);
        }
    }

    None
}

/// Simple pseudo-random number generator for temp file names.
fn rand_u32() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Simple mixing: use lower and upper 32 bits
    let lo = nanos as u32;
    let hi = (nanos >> 32) as u32;
    let mut x = lo ^ hi;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

// ---------------------------------------------------------------------------
// SEXP-dependent functions
// ---------------------------------------------------------------------------

use std::os::raw::{c_char, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

/// checkArity — stub, no-op.
#[inline(always)]
unsafe fn checkArity(_op: SEXP, _args: SEXP) {}

/// isString check — STRSXP type.
#[inline(always)]
unsafe fn isString(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        TYPEOF(x) == SEXPTYPE::STRSXP.0
    }
}

#[inline(always)]
unsafe fn translateChar(s: SEXP) -> *const c_char { unsafe {
    crate::sexp::accessors::translateChar(s)
}}

/// R_Interactive flag. Set to false (non-interactive mode).
static R_INTERACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set R_Interactive flag.
pub fn R_SetInteractive(val: bool) {
    R_INTERACTIVE.store(val, std::sync::atomic::Ordering::Relaxed);
}

/// Get R_Interactive flag.
pub fn R_Interactive() -> bool {
    R_INTERACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Sys.getenv() — get environment variables.
///
/// .Internal(Sys.getenv(x, unset))
/// If x has length 0, returns all environment variables.
/// Otherwise, looks up each name in x, returning the value or `unset` if not found.
pub unsafe fn do_getenv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let x = CAR(args);
        let unset = CADR(args);

        if !isString(x) {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "wrong type for argument".to_string(),
            });
        }
        if !isString(unset) || LENGTH(unset) != 1 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "wrong type for argument".to_string(),
            });
        }

        let n = LENGTH(x);
        if n == 0 {
            // Return all environment variables
            let vars: Vec<_> = std::env::vars().collect();
            let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, vars.len() as R_xlen_t));
            for (i, (key, val)) in vars.iter().enumerate() {
                let combined = format!("{}={}", key, val);
                let c_str = std::ffi::CString::new(combined).unwrap_or_default();
                SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(c_str.as_ptr()));
            }
            Rf_unprotect(1);
            ans
        } else {
            let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t));
            for j in 0..n as R_xlen_t {
                let name = STRING_ELT(x, j);
                let name_c = CHAR(name);
                let name_str = std::ffi::CStr::from_ptr(name_c).to_str().unwrap_or("");
                if let Ok(val) = std::env::var(name_str) {
                    let c_str = std::ffi::CString::new(val).unwrap_or_default();
                    SET_STRING_ELT(ans, j, Rf_mkChar(c_str.as_ptr()));
                } else {
                    SET_STRING_ELT(ans, j, STRING_ELT(unset, 0));
                }
            }
            Rf_unprotect(1);
            ans
        }
    }
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

        let n = LENGTH(val);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n as R_xlen_t));
        for i in 0..n as R_xlen_t {
            let name_c = CHAR(STRING_ELT(nm, i));
            let val_c = CHAR(STRING_ELT(val, i));
            let name_str = std::ffi::CStr::from_ptr(name_c).to_str().unwrap_or("");
            let val_str = std::ffi::CStr::from_ptr(val_c).to_str().unwrap_or("");
            std::env::set_var(name_str, val_str);
            *LOGICAL(ans).add(i as usize) = TRUE;
        }
        Rf_unprotect(1);
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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n as R_xlen_t));
        for i in 0..n as R_xlen_t {
            let name_c = CHAR(STRING_ELT(nm, i));
            let name_str = std::ffi::CStr::from_ptr(name_c).to_str().unwrap_or("");
            std::env::remove_var(name_str);
            *LOGICAL(ans).add(i as usize) = TRUE;
        }
        Rf_unprotect(1);
        ans
    }
}

/// interactive() — check if R is in interactive mode.
pub unsafe fn do_interactive(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(if R_Interactive() { TRUE } else { FALSE }) }
}

/// tempdir() — return the temporary directory path.
pub unsafe fn do_tempdir(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);
        let _check = CAR(args);
        let temp_dir = std::env::temp_dir();
        let temp_str = temp_dir.to_string_lossy().to_string();
        Rf_mkString(
            std::ffi::CString::new(temp_str)
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// tempfile() — generate temporary file names.
///
/// .Internal(tempfile(pattern, tempdir, fileext))
pub unsafe fn do_tempfile(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        checkArity(_op, args);

        let pattern = CAR(args);
        let n1 = LENGTH(pattern);
        args = CDR(args);
        let tempdir = CAR(args);
        let n2 = LENGTH(tempdir);
        args = CDR(args);
        let fileext = CAR(args);
        let n3 = LENGTH(fileext);

        if !isString(pattern) || n1 < 1 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "invalid filename pattern".to_string(),
            });
        }
        if !isString(tempdir) || n2 < 1 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "invalid 'tempdir' value".to_string(),
            });
        }
        if !isString(fileext) || n3 < 1 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "invalid file extension".to_string(),
            });
        }

        let slen = std::cmp::max(n1, std::cmp::max(n2, n3));
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, slen as R_xlen_t));

        for i in 0..slen as R_xlen_t {
            let tn_c = CHAR(STRING_ELT(pattern, i % (n1 as R_xlen_t)));
            let td_c = CHAR(STRING_ELT(tempdir, i % (n2 as R_xlen_t)));
            let te_c = CHAR(STRING_ELT(fileext, i % (n3 as R_xlen_t)));
            let tn = std::ffi::CStr::from_ptr(tn_c).to_str().unwrap_or("");
            let td = std::ffi::CStr::from_ptr(td_c).to_str().unwrap_or("");
            let te = std::ffi::CStr::from_ptr(te_c).to_str().unwrap_or("");

            if let Some(name) = R_tmpnam2(tn, td, te) {
                let c_str = std::ffi::CString::new(name).unwrap_or_default();
                SET_STRING_ELT(ans, i, Rf_mkChar(c_str.as_ptr()));
            } else {
                SET_STRING_ELT(ans, i, STRING_ELT(fileext, 0));
            }
        }

        Rf_unprotect(1);
        ans
    }
}

/// R_system — execute a system command and return the exit status.
///
/// Ported from R's R_system() in sysutils.c.
pub fn R_system(command: &str) -> c_int {
    use std::process::Command;

    match Command::new("sh").arg("-c").arg(command).status() {
        Ok(status) => status.code().unwrap_or(127),
        Err(_) => 127,
    }
}

/// Sys.info() — return system information as a named list.
/// Note: canonical version lives in unix/sys_unix.rs; this is a
/// module-private version.
pub(crate) unsafe fn do_sysinfo_mainutils(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Return a list with basic system info
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, 5));

        // sysname
        let sysname = Rf_mkString(
            std::ffi::CString::new("Darwin")
                .unwrap_or_default()
                .as_ptr(),
        );
        SET_VECTOR_ELT(ans, 0, sysname);

        // release
        let release = Rf_mkString(std::ffi::CString::new("").unwrap_or_default().as_ptr());
        SET_VECTOR_ELT(ans, 1, release);

        // version
        let version = Rf_mkString(std::ffi::CString::new("").unwrap_or_default().as_ptr());
        SET_VECTOR_ELT(ans, 2, version);

        // nodename
        let hn = get_hostname();
        let nodename = Rf_mkString(
            std::ffi::CString::new(hn.as_str())
                .unwrap_or_default()
                .as_ptr(),
        );
        SET_VECTOR_ELT(ans, 3, nodename);

        // machine
        let machine = Rf_mkString(
            std::ffi::CString::new("x86_64")
                .unwrap_or_default()
                .as_ptr(),
        );
        SET_VECTOR_ELT(ans, 4, machine);

        // Set names
        let names = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, 5));
        SET_STRING_ELT(
            names,
            0,
            Rf_mkChar(
                std::ffi::CString::new("sysname")
                    .unwrap_or_default()
                    .as_ptr(),
            ),
        );
        SET_STRING_ELT(
            names,
            1,
            Rf_mkChar(
                std::ffi::CString::new("release")
                    .unwrap_or_default()
                    .as_ptr(),
            ),
        );
        SET_STRING_ELT(
            names,
            2,
            Rf_mkChar(
                std::ffi::CString::new("version")
                    .unwrap_or_default()
                    .as_ptr(),
            ),
        );
        SET_STRING_ELT(
            names,
            3,
            Rf_mkChar(
                std::ffi::CString::new("nodename")
                    .unwrap_or_default()
                    .as_ptr(),
            ),
        );
        SET_STRING_ELT(
            names,
            4,
            Rf_mkChar(
                std::ffi::CString::new("machine")
                    .unwrap_or_default()
                    .as_ptr(),
            ),
        );

        let names_sym = crate::eval::attrib_core::R_NamesSymbol();
        crate::eval::attrib_core::setAttrib(ans, names_sym, names);

        Rf_unprotect(2);
        ans
    }
}

/// Sys.sysenv() — return the system environment variables.
pub unsafe fn do_sysenvir(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Return all environment variables as a named character vector
        let vars: Vec<(String, String)> = std::env::vars().collect();
        let n = vars.len();
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t));
        let names = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t));

        for (i, (key, val)) in vars.iter().enumerate() {
            let k = std::ffi::CString::new(key.as_str()).unwrap_or_default();
            let v = std::ffi::CString::new(val.as_str()).unwrap_or_default();
            SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(v.as_ptr()));
            SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(k.as_ptr()));
        }

        let names_sym = crate::eval::attrib_core::R_NamesSymbol();
        crate::eval::attrib_core::setAttrib(ans, names_sym, names);

        Rf_unprotect(2);
        ans
    }
}

/// Get the hostname of the current machine.
fn get_hostname() -> String {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
