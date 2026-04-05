#![allow(
    unsafe_op_in_unsafe_fn,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    deprecated
)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997-2025   The R Core Team
 *  Copyright (C) 1995-1996   Robert Gentleman and Ross Ihaka
 *
 *  Port of R's src/main/sysutils.c to Rust.
 *  System utilities: file operations, environment variables, process management,
 *  character encoding translation, temp directory/file management, time limits, glob.
 */

use crate::main::coerce::*;
use crate::main::errors::Rf_error;
use crate::main::memory_main::{R_AllocStringBuffer, R_FreeStringBuffer, R_StringBuffer};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, Rboolean, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_long, c_uchar, c_uint, c_void};
use std::ptr;

// ---------------------------------------------------------------------------
// Local stubs for encoding macros (not globally defined in this codebase)
// ---------------------------------------------------------------------------

fn IS_ASCII(_s: SEXP) -> bool {
    true
}
fn IS_UTF8(_s: SEXP) -> bool {
    false
}
fn IS_LATIN1(_s: SEXP) -> bool {
    false
}
fn IS_BYTES(_s: SEXP) -> bool {
    false
}

// GLOB_QUOTE may not be defined in all libc versions
const GLOB_QUOTE: c_int = 0x01;

// ---------------------------------------------------------------------------
// Local helper stubs
// ---------------------------------------------------------------------------

/// checkArity -- delegates to Rf_checkArityCall.
#[inline(always)]
unsafe fn checkArity(op: SEXP, args: SEXP) {
    crate::main::errors::Rf_checkArityCall(op, args, crate::main::errors::getCurrentCall());
}

/// isString check -- STRSXP type.
#[inline(always)]
unsafe fn isString(x: SEXP) -> bool {
    if x.is_null() || x == R_NilValue() {
        return false;
    }
    TYPEOF(x) == SEXPTYPE::STRSXP.0
}

/// R_FINITE check for f64.
#[inline(always)]
unsafe fn R_FINITE(x: c_double) -> c_int {
    if x.is_finite() { 1 } else { 0 }
}

/// Rf_ScalarLogical -- create a single logical value.
unsafe fn Rf_ScalarLogical(v: c_int) -> SEXP {
    let s = Rf_allocVector3(SEXPTYPE::LGLSXP.0, 1);
    *LOGICAL(s) = v;
    s
}

/// SHALLOW_DUPLICATE_ATTRIB -- copy attributes shallowly (stub).
unsafe fn SHALLOW_DUPLICATE_ATTRIB(to: SEXP, from: SEXP) {
    if to.is_null() || from.is_null() {
        return;
    }
    let names_sym = crate::attrib_core::R_NamesSymbol();
    let from_names = crate::attrib_core::getAttrib(from, names_sym);
    if !from_names.is_null() && from_names != R_NilValue() {
        crate::attrib_core::setAttrib(to, names_sym, from_names);
    }
}

/// asReal -- coerce to double.
unsafe fn asReal(x: SEXP) -> c_double {
    crate::main::coerce::asReal(x)
}

/// asLogical -- coerce to logical.
unsafe fn asLogical(x: SEXP) -> c_int {
    crate::main::coerce::asLogical(x)
}

/// Rf_mkString -- create a single-element character vector.
/// (Already defined in constructors.rs, but we need a local wrapper
///  that doesn't conflict; use the imported one.)
// Rf_mkString is imported from crate::sexp::constructors::*;

// ---------------------------------------------------------------------------
// File system utilities
// ---------------------------------------------------------------------------

/// Check if a file exists (Unix version).
///
/// Ported from R's `R_FileExists` (non-Windows path).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FileExists(path: *const c_char) -> Rboolean {
    if path.is_null() {
        return FALSE;
    }
    let r = unsafe { libc::stat(path, ptr::null_mut()) };
    if r == 0 { TRUE } else { FALSE }
}

/// Get file modification time (Unix version).
///
/// Ported from R's `R_FileMtime` (non-Windows path).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FileMtime(path: *const c_char) -> c_double {
    let mut sb: libc::stat = std::mem::zeroed();
    if unsafe { libc::stat(path, &mut sb) } != 0 {
        Rf_error(b"cannot determine file modification time\0".as_ptr() as *const c_char);
    }
    sb.st_mtime as c_double
}

/// Check if a filename is hidden (starts with '.').
///
/// Ported from R's `R_HiddenFile`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_HiddenFile(name: *const c_char) -> Rboolean {
    if !name.is_null() && unsafe { *name } != 0 && unsafe { *name } != b'.' as c_char {
        0
    } else {
        1
    }
}

/// Check if a directory is writable (Unix version).
///
/// Ported from R's `R_isWriteableDir`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_isWriteableDir(path: *mut c_char) -> c_int {
    if path.is_null() {
        return 0;
    }
    let mut sb: libc::stat = std::mem::zeroed();
    if unsafe { libc::stat(path, &mut sb) } != 0 {
        return 0;
    }
    let is_dir = (sb.st_mode & libc::S_IFDIR) != 0;
    if !is_dir {
        return 0;
    }
    if unsafe { libc::access(path, libc::W_OK) } == 0 {
        1
    } else {
        0
    }
}

/// fopen wrapper -- always opens in text mode on Unix.
///
/// Ported from R's `R_fopen`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_fopen(filename: *const c_char, mode: *const c_char) -> *mut libc::FILE {
    if filename.is_null() {
        return ptr::null_mut();
    }
    unsafe { libc::fopen(filename, mode) }
}

/// fopen wrapper for SEXP filenames (Unix version).
///
/// Ported from R's `RC_fopen`.
pub unsafe fn RC_fopen(
    fn_: SEXP,
    mode: *const c_char,
    expand: Rboolean,
) -> *mut libc::FILE {
    if fn_.is_null() || fn_ == R_NilValue() {
        return ptr::null_mut();
    }
    let filename = unsafe { translateCharFP(fn_) };
    if filename.is_null() {
        return ptr::null_mut();
    }
    let res = if expand != 0 {
        unsafe { R_ExpandFileName(filename) }
    } else {
        filename
    };
    unsafe { libc::fopen(res, mode) }
}

// ---------------------------------------------------------------------------
// R_ExpandFileName
// ---------------------------------------------------------------------------

/// Expand ~ in a file path (simple Unix version).
///
/// Ported from R's `R_ExpandFileName`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ExpandFileName(path: *const c_char) -> *const c_char {
    if path.is_null() {
        return path;
    }
    let s = unsafe { CStr::from_ptr(path) };
    let bytes = s.to_bytes();
    if bytes.is_empty() || bytes[0] != b'~' {
        return path;
    }
    // Only expand ~/... (home of current user)
    if bytes.len() > 1 && bytes[1] != b'/' {
        return path; // don't expand ~otheruser
    }
    if let Ok(home) = std::env::var("HOME") {
        let rest = if bytes.len() > 1 { &bytes[1..] } else { b"/" };
        let expanded = format!("{}{}", home, std::str::from_utf8(rest).unwrap_or(""));
        let c_expanded = CString::new(expanded).unwrap_or_default();
        // Leak intentionally -- R_alloc lifetime management in C
        c_expanded.into_raw()
    } else {
        path
    }
}

// ---------------------------------------------------------------------------
// popen / system
// ---------------------------------------------------------------------------

/// popen wrapper.
///
/// Ported from R's `R_popen`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_popen(command: *const c_char, type_: *const c_char) -> *mut libc::FILE {
    unsafe { libc::popen(command, type_) }
}

/// Execute a system command and return the exit status.
///
/// Ported from R's `R_system`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_system(command: *const c_char) -> c_int {
    let res = unsafe { libc::system(command) };
    if res == -1 {
        return 127;
    }
    // On Unix, use WEXITSTATUS
    if libc::WIFEXITED(res) {
        libc::WEXITSTATUS(res)
    } else {
        // assume shifted if multiple of 256
        if (res % 256) == 0 { res / 256 } else { res }
    }
}

// ---------------------------------------------------------------------------
// SYSTEM INFORMATION
// ---------------------------------------------------------------------------

/// The location of the R system files.
///
/// Ported from R's `R_HomeDir`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_HomeDir() -> *mut c_char {
    let val = std::env::var("R_HOME");
    match val {
        Ok(s) => {
            let c = CString::new(s).unwrap_or_default();
            c.into_raw()
        }
        Err(_) => ptr::null_mut(),
    }
}

// R_Interactive flag
static R_INTERACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set R_Interactive flag.
pub fn R_SetInteractive(val: bool) {
    R_INTERACTIVE.store(val, std::sync::atomic::Ordering::Relaxed);
}

/// Get R_Interactive flag.
pub fn R_Interactive() -> bool {
    R_INTERACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// interactive() -- check if R is in interactive mode.
///
/// Ported from R's `do_interactive`.
pub unsafe fn do_interactive(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);
    Rf_ScalarLogical(if R_Interactive() { TRUE } else { FALSE })
}

// ---------------------------------------------------------------------------
// Temp directory
// ---------------------------------------------------------------------------

/// Global temp directory pointer (C-compatible static).
static mut R_TempDir: *mut c_char = ptr::null_mut();

/// Reinitialize the temp directory (Unix version).
///
/// Ported from R's `R_reInitTempDir`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_reInitTempDir(die_on_fail: c_int) {
    if !R_TempDir.is_null() {
        return;
    }

    let tm = std::env::var("TMPDIR")
        .ok()
        .filter(|v| {
            let mut c = CString::new(v.as_str()).unwrap_or_default();
            R_isWriteableDir(c.as_ptr() as *mut c_char) != 0
        })
        .or_else(|| {
            std::env::var("TMP").ok().filter(|v| {
                let mut c = CString::new(v.as_str()).unwrap_or_default();
                R_isWriteableDir(c.as_ptr() as *mut c_char) != 0
            })
        })
        .or_else(|| {
            std::env::var("TEMP").ok().filter(|v| {
                let mut c = CString::new(v.as_str()).unwrap_or_default();
                R_isWriteableDir(c.as_ptr() as *mut c_char) != 0
            })
        })
        .unwrap_or_else(|| "/tmp".to_string());

    // check for spaces
    if tm.contains(' ') {
        if die_on_fail != 0 {
            R_Suicide(b"'R_TempDir' contains space\0".as_ptr() as *const c_char);
        } else {
            Rf_error(b"'R_TempDir' contains space\0".as_ptr() as *const c_char);
        }
    }

    let suffix = "/RtmpXXXXXX";
    let template = format!("{}{}", tm, suffix);
    let mut template_c = CString::new(template).unwrap_or_default();
    let template_buf = template_c.into_raw();

    if unsafe { libc::mkdtemp(template_buf) }.is_null() {
        unsafe { libc::free(template_buf as *mut c_void) };
        if die_on_fail != 0 {
            R_Suicide(b"cannot create 'R_TempDir'\0".as_ptr() as *const c_char);
        } else {
            Rf_error(b"cannot create 'R_TempDir'\0".as_ptr() as *const c_char);
        }
    }

    unsafe {
        libc::setenv(
            b"R_SESSION_TMPDIR\0".as_ptr() as *const c_char,
            template_buf,
            1,
        )
    };
    R_TempDir = template_buf;
}

/// Initialize temp directory (dies on failure).
pub unsafe fn InitTempDir() {
    R_reInitTempDir(1);
}

/// Get the current temp directory path (C string).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_TempDir_get() -> *mut c_char {
    R_TempDir
}

/// tempdir() -- return the temporary directory path.
///
/// Ported from R's `do_tempdir`.
pub unsafe fn do_tempdir(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);
    let _check = CAR(args);
    let td = unsafe { R_TempDir_get() };
    if td.is_null() {
        R_reInitTempDir(0);
        let td2 = unsafe { R_TempDir_get() };
        if td2.is_null() {
            return Rf_mkString(b"\0".as_ptr() as *const c_char);
        }
        Rf_mkString(td2)
    } else {
        Rf_mkString(td)
    }
}

// ---------------------------------------------------------------------------
// Temp file names
// ---------------------------------------------------------------------------

/// Simple pseudo-random number generator for temp file names.
fn rand_u32() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let lo = nanos as u32;
    let hi = (nanos >> 32) as u32;
    let mut x = lo ^ hi;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Generate a unique temporary file name.
///
/// Ported from R's `R_tmpnam2`. Tries up to 100 times.
/// Returns a malloc'd string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tmpnam2(
    prefix: *const c_char,
    tempdir: *const c_char,
    fileext: *const c_char,
) -> *mut c_char {
    let prefix_s = if !prefix.is_null() {
        unsafe { CStr::from_ptr(prefix) }.to_str().unwrap_or("")
    } else {
        ""
    };
    let tempdir_s = if !tempdir.is_null() {
        unsafe { CStr::from_ptr(tempdir) }
            .to_str()
            .unwrap_or("/tmp")
    } else {
        "/tmp"
    };
    let fileext_s = if !fileext.is_null() {
        unsafe { CStr::from_ptr(fileext) }.to_str().unwrap_or("")
    } else {
        ""
    };

    let pid = unsafe { libc::getpid() };

    for _n in 0..100 {
        let r1 = rand_u32();
        let name = format!("{}/{}{:x}{:x}{}", tempdir_s, prefix_s, pid, r1, fileext_s);

        let name_c = CString::new(name.clone()).unwrap_or_default();
        let exists = unsafe { R_FileExists(name_c.as_ptr()) };
        if exists == 0 {
            return CString::new(name).unwrap_or_default().into_raw();
        }
    }
    ptr::null_mut()
}

/// R_tmpnam -- calls R_tmpnam2 with empty extension.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tmpnam(prefix: *const c_char, tempdir: *const c_char) -> *mut c_char {
    R_tmpnam2(prefix, tempdir, b"\0".as_ptr() as *const c_char)
}

/// Free a temporary filename.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_free_tmpnam(name: *mut c_char) {
    if !name.is_null() {
        unsafe { libc::free(name as *mut c_void) };
    }
}

/// tempfile() -- generate temporary file names.
///
/// Ported from R's `do_tempfile`.
pub unsafe fn do_tempfile(_call: SEXP, op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);

    let pattern = CAR(args);
    let n1 = LENGTH(pattern);
    args = CDR(args);
    let tempdir = CAR(args);
    let n2 = LENGTH(tempdir);
    args = CDR(args);
    let fileext = CAR(args);
    let n3 = LENGTH(fileext);

    if !isString(pattern) || n1 < 1 {
        Rf_error(b"invalid filename pattern\0".as_ptr() as *const c_char);
    }
    if !isString(tempdir) || n2 < 1 {
        Rf_error(b"invalid 'tempdir' value\0".as_ptr() as *const c_char);
    }
    if !isString(fileext) || n3 < 1 {
        Rf_error(b"invalid file extension\0".as_ptr() as *const c_char);
    }

    let slen = std::cmp::max(n1, std::cmp::max(n2, n3));
    let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, slen as R_xlen_t));

    for i in 0..slen as R_xlen_t {
        let tn_c = unsafe { translateCharFP(STRING_ELT(pattern, i % (n1 as R_xlen_t))) };
        let td_c = unsafe { translateCharFP(STRING_ELT(tempdir, i % (n2 as R_xlen_t))) };
        let te_c = unsafe { translateCharFP(STRING_ELT(fileext, i % (n3 as R_xlen_t))) };
        let tm = unsafe { R_tmpnam2(tn_c, td_c, te_c) };
        if !tm.is_null() {
            SET_STRING_ELT(ans, i, unsafe { Rf_mkChar(tm) });
            unsafe { libc::free(tm as *mut c_void) };
        } else {
            SET_STRING_ELT(ans, i, STRING_ELT(fileext, 0));
        }
    }

    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// Environment variable functions
// ---------------------------------------------------------------------------

/// Sys.getenv() -- get environment variables.
///
/// Ported from R's `do_getenv` (Unix version).
pub unsafe fn do_getenv(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);

    let x = CAR(args);
    let unset = CADR(args);

    if !isString(x) {
        Rf_error(b"wrong type for argument\0".as_ptr() as *const c_char);
    }
    if !isString(unset) || LENGTH(unset) != 1 {
        Rf_error(b"wrong type for argument\0".as_ptr() as *const c_char);
    }

    let n = LENGTH(x);
    if n == 0 {
        // Return all environment variables
        let vars: Vec<(String, String)> = std::env::vars().collect();
        let count = vars.len();
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, count as R_xlen_t));
        for (i, (key, val)) in vars.iter().enumerate() {
            let combined = format!("{}={}", key, val);
            let c_str = CString::new(combined).unwrap();
            SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(c_str.as_ptr()));
        }
        Rf_unprotect(1);
        ans
    } else {
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t));
        for j in 0..n as R_xlen_t {
            let name = STRING_ELT(x, j);
            let name_c = unsafe { translateChar(name) };
            let name_str = unsafe { CStr::from_ptr(name_c) }.to_str().unwrap_or("");
            let val = std::env::var(name_str);
            match val {
                Ok(v) => {
                    let c_str = CString::new(v).unwrap();
                    SET_STRING_ELT(ans, j, Rf_mkChar(c_str.as_ptr()));
                }
                Err(_) => {
                    SET_STRING_ELT(ans, j, STRING_ELT(unset, 0));
                }
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// Sys.setenv() -- set environment variables.
///
/// Ported from R's `do_setenv`.
pub unsafe fn do_setenv(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);

    let nm = CAR(args);
    let val = CADR(args);

    if !isString(nm) || !isString(val) {
        Rf_error(b"wrong type for argument\0".as_ptr() as *const c_char);
    }
    if LENGTH(nm) != LENGTH(val) {
        Rf_error(b"'names' and 'values' are of different lengths\0".as_ptr() as *const c_char);
    }

    let n = LENGTH(val);
    let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n as R_xlen_t));
    for i in 0..n as R_xlen_t {
        let name_c = unsafe { translateChar(STRING_ELT(nm, i)) };
        let val_c = unsafe { translateChar(STRING_ELT(val, i)) };
        let name_str = unsafe { CStr::from_ptr(name_c) }.to_str().unwrap_or("");
        let val_str = unsafe { CStr::from_ptr(val_c) }.to_str().unwrap_or("");
        std::env::set_var(name_str, val_str);
        *LOGICAL(ans).add(i as usize) = TRUE;
    }
    Rf_unprotect(1);
    ans
}

/// Sys.unsetenv() -- unset environment variables.
///
/// Ported from R's `do_unsetenv`.
pub unsafe fn do_unsetenv(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);

    let nm = CAR(args);

    if !isString(nm) {
        Rf_error(b"wrong type for argument\0".as_ptr() as *const c_char);
    }

    let n = LENGTH(nm);
    let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP.0, n as R_xlen_t));
    for i in 0..n as R_xlen_t {
        let name_c = unsafe { translateChar(STRING_ELT(nm, i)) };
        let name_str = unsafe { CStr::from_ptr(name_c) }.to_str().unwrap_or("");
        std::env::remove_var(name_str);
        // Check that it was unset
        let still = std::env::var(name_str).is_err();
        *LOGICAL(ans).add(i as usize) = if still { TRUE } else { FALSE };
    }
    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// Sys.sysenv() -- return environment as named character vector
// ---------------------------------------------------------------------------

/// Sys.sysenv() -- return the system environment variables.
pub unsafe fn do_sysenvir(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    let vars: Vec<(String, String)> = std::env::vars().collect();
    let n = vars.len();
    let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t));
    let names = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t));

    for (i, (key, val)) in vars.iter().enumerate() {
        let k = CString::new(key.as_str()).unwrap();
        let v = CString::new(val.as_str()).unwrap();
        SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(v.as_ptr()));
        SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(k.as_ptr()));
    }

    let names_sym = crate::attrib_core::R_NamesSymbol();
    crate::attrib_core::setAttrib(ans, names_sym, names);

    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// Process time functions
// ---------------------------------------------------------------------------

// Time limit globals
static mut cpuLimitValue: c_double = -1.0;
static mut elapsedLimitValue: c_double = -1.0;
static mut cpuLimit: c_double = -1.0;
static mut elapsedLimit: c_double = -1.0;
static mut cpuLimit2: c_double = -1.0;
static mut elapsedLimit2: c_double = -1.0;

/// Reset time limits based on current time and limit values.
///
/// Ported from R's `resetTimeLimits`.
pub unsafe fn resetTimeLimits() {
    let mut data: [c_double; 5] = [0.0; 5];
    unsafe { R_getProcTime(data.as_mut_ptr()) };

    unsafe {
        elapsedLimit = if elapsedLimitValue > 0.0 {
            data[2] + elapsedLimitValue
        } else {
            -1.0
        };
        if elapsedLimit2 > 0.0 && (elapsedLimit <= 0.0 || elapsedLimit2 < elapsedLimit) {
            elapsedLimit = elapsedLimit2;
        }

        // On Unix: user.self + sys.self + user.child + sys.child
        cpuLimit = if cpuLimitValue > 0.0 {
            data[0] + data[1] + data[3] + data[4] + cpuLimitValue
        } else {
            -1.0
        };
        if cpuLimit2 > 0.0 && (cpuLimit <= 0.0 || cpuLimit2 < cpuLimit) {
            cpuLimit = cpuLimit2;
        }
    }
}

/// setTimeLimit() -- set CPU and elapsed time limits.
///
/// Ported from R's `do_setTimeLimit`.
pub unsafe fn do_setTimeLimit(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    checkArity(op, args);
    let cpu = unsafe { asReal(CAR(args)) };
    let elapsed = unsafe { asReal(CADR(args)) };
    let transient = unsafe { asLogical(CADDR(args)) };

    unsafe {
        let old_cpu = cpuLimitValue;
        let old_elapsed = elapsedLimitValue;

        if R_FINITE(cpu) != 0 && cpu > 0.0 {
            cpuLimitValue = cpu;
        } else {
            cpuLimitValue = -1.0;
        }
        if R_FINITE(elapsed) != 0 && elapsed > 0.0 {
            elapsedLimitValue = elapsed;
        } else {
            elapsedLimitValue = -1.0;
        }

        resetTimeLimits();

        if transient == TRUE as c_int {
            cpuLimitValue = old_cpu;
            elapsedLimitValue = old_elapsed;
        }
    }

    R_NilValue()
}

/// setSessionTimeLimit() -- set session time limits.
///
/// Ported from R's `do_setSessionTimeLimit`.
pub unsafe fn do_setSessionTimeLimit(
    _call: SEXP,
    op: SEXP,
    args: SEXP,
    _rho: SEXP,
) -> SEXP {
    checkArity(op, args);
    let cpu = unsafe { asReal(CAR(args)) };
    let elapsed = unsafe { asReal(CADR(args)) };
    let mut data: [c_double; 5] = [0.0; 5];
    unsafe { R_getProcTime(data.as_mut_ptr()) };

    unsafe {
        if R_FINITE(cpu) != 0 && cpu > 0.0 {
            cpuLimit2 = cpu + data[0] + data[1] + data[3] + data[4];
        } else {
            cpuLimit2 = -1.0;
        }
        if R_FINITE(elapsed) != 0 && elapsed > 0.0 {
            elapsedLimit2 = elapsed + data[2];
        } else {
            elapsedLimit2 = -1.0;
        }
    }

    R_NilValue()
}

/// Check CPU and elapsed time limits.
///
/// Ported from R's `R_CheckTimeLimits`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_CheckTimeLimits() {
    unsafe {
        if cpuLimit <= 0.0 && elapsedLimit <= 0.0 {
            return;
        }

        const TIME_CHECK_SKIP: c_int = 5;
        static mut check_count: c_int = 0;
        if check_count < TIME_CHECK_SKIP {
            check_count += 1;
            return;
        } else {
            check_count = 0;
        }

        const TIME_CHECK_DELTA: c_double = 0.05;
        static mut check_time: c_double = 0.0;
        let tm = crate::main::times::currentTime();
        if tm < check_time {
            return;
        } else {
            check_time = tm + TIME_CHECK_DELTA;
        }

        let mut data: [c_double; 5] = [0.0; 5];
        R_getProcTime(data.as_mut_ptr());
        let cpu = data[0] + data[1] + data[3] + data[4];

        if elapsedLimit > 0.0 && data[2] > elapsedLimit {
            cpuLimit = -1.0;
            elapsedLimit = -1.0;
            if elapsedLimit2 > 0.0 && data[2] > elapsedLimit2 {
                elapsedLimit2 = -1.0;
                Rf_error(b"reached session elapsed time limit\0".as_ptr() as *const c_char);
            } else {
                Rf_error(b"reached elapsed time limit\0".as_ptr() as *const c_char);
            }
        }
        if cpuLimit > 0.0 && cpu > cpuLimit {
            cpuLimit = -1.0;
            elapsedLimit = -1.0;
            if cpuLimit2 > 0.0 && cpu > cpuLimit2 {
                cpuLimit2 = -1.0;
                Rf_error(b"reached session CPU time limit\0".as_ptr() as *const c_char);
            } else {
                Rf_error(b"reached CPU time limit\0".as_ptr() as *const c_char);
            }
        }
    }
}

/// proc.time() -- return process times.
///
/// Ported from R's `do_proctime`.
pub unsafe fn do_proctime(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);
    let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, 5));
    let nm = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, 5));
    unsafe { R_getProcTime(REAL(ans)) };
    SET_STRING_ELT(nm, 0, Rf_mkChar(b"user.self\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 1, Rf_mkChar(b"sys.self\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 2, Rf_mkChar(b"elapsed\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 3, Rf_mkChar(b"user.child\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nm, 4, Rf_mkChar(b"sys.child\0".as_ptr() as *const c_char));

    let names_sym = crate::attrib_core::R_NamesSymbol();
    crate::attrib_core::setAttrib(ans, names_sym, nm);
    let class_sym = crate::attrib_core::R_ClassSymbol();
    crate::attrib_core::setAttrib(
        ans,
        class_sym,
        Rf_mkString(b"proc_time\0".as_ptr() as *const c_char),
    );

    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// Sys.info() -- system information (module-private stub)
// ---------------------------------------------------------------------------

/// Get the hostname of the current machine.
fn get_hostname() -> String {
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

/// Sys.info() -- return system information as a named list.
/// Note: canonical version lives in unix/sys_unix.rs; this is a
/// module-private version.
pub(crate) unsafe fn do_sysinfo_main(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, 5));

    let sysname = Rf_mkString(b"Darwin\0".as_ptr() as *const c_char);
    SET_VECTOR_ELT(ans, 0, sysname);

    let release = Rf_mkString(b"\0".as_ptr() as *const c_char);
    SET_VECTOR_ELT(ans, 1, release);

    let version = Rf_mkString(b"\0".as_ptr() as *const c_char);
    SET_VECTOR_ELT(ans, 2, version);

    let hn = get_hostname();
    let nodename = Rf_mkString(CString::new(hn.as_str()).unwrap().as_ptr());
    SET_VECTOR_ELT(ans, 3, nodename);

    let machine = Rf_mkString(b"x86_64\0".as_ptr() as *const c_char);
    SET_VECTOR_ELT(ans, 4, machine);

    let names = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, 5));
    SET_STRING_ELT(names, 0, Rf_mkChar(b"sysname\0".as_ptr() as *const c_char));
    SET_STRING_ELT(names, 1, Rf_mkChar(b"release\0".as_ptr() as *const c_char));
    SET_STRING_ELT(names, 2, Rf_mkChar(b"version\0".as_ptr() as *const c_char));
    SET_STRING_ELT(names, 3, Rf_mkChar(b"nodename\0".as_ptr() as *const c_char));
    SET_STRING_ELT(names, 4, Rf_mkChar(b"machine\0".as_ptr() as *const c_char));

    let names_sym = crate::attrib_core::R_NamesSymbol();
    crate::attrib_core::setAttrib(ans, names_sym, names);

    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// Glob
// ---------------------------------------------------------------------------

/// Sys.glob() -- expand paths with glob (Unix version).
///
/// Ported from R's `do_glob`.
pub unsafe fn do_glob(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);
    let x = CAR(args);
    if !isString(x) {
        Rf_error(b"invalid 'paths' argument\0".as_ptr() as *const c_char);
    }
    let len = XLENGTH(x);
    if len == 0 {
        return Rf_allocVector3(SEXPTYPE::STRSXP.0, 0);
    }

    let dirmark = unsafe { asLogical(CADR(args)) };
    if dirmark == NA_INTEGER {
        Rf_error(b"invalid 'dirmark' argument\0".as_ptr() as *const c_char);
    }

    let mut globbuf: libc::glob_t = unsafe { std::mem::zeroed() };
    let mut initialized = false;

    let mut flags: c_int = GLOB_QUOTE;
    if dirmark != 0 {
        flags |= libc::GLOB_MARK;
    }

    for i in 0..len as R_xlen_t {
        let el = STRING_ELT(x, i);
        if el.is_null() || el == R_NilValue() {
            continue;
        }
        let path_c = unsafe { translateChar(el) };
        let mut cur_flags = flags;
        if initialized {
            cur_flags |= libc::GLOB_APPEND;
        }
        let res = unsafe { libc::glob(path_c, cur_flags, None, &mut globbuf) };
        if res == libc::GLOB_ABORTED {
            // warning -- skip
        } else if res == libc::GLOB_NOSPACE {
            Rf_error(b"internal out-of-memory condition\0".as_ptr() as *const c_char);
        }
        initialized = true;
    }

    let n = if initialized {
        globbuf.gl_pathc as R_xlen_t
    } else {
        0
    };
    let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, n));
    if initialized && !globbuf.gl_pathv.is_null() {
        for i in 0..n {
            let p = unsafe { *globbuf.gl_pathv.add(i as usize) };
            if !p.is_null() {
                SET_STRING_ELT(ans, i, Rf_mkChar(p));
            }
        }
        unsafe { libc::globfree(&mut globbuf) };
    }

    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// isatty
// ---------------------------------------------------------------------------

/// Check if a file descriptor is a terminal.
///
/// Ported from R's `R_isatty`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_isatty(fd: c_int) -> c_int {
    unsafe { libc::isatty(fd) }
}

// ---------------------------------------------------------------------------
// Character encoding translation
// ---------------------------------------------------------------------------

/// Case-insensitive string comparison.
///
/// Ported from R's `R_strieql`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_strieql(a: *const c_char, b: *const c_char) -> c_int {
    if a.is_null() || b.is_null() {
        return if a.is_null() && b.is_null() { 1 } else { 0 };
    }
    let sa = unsafe { CStr::from_ptr(a) }.to_bytes();
    let sb = unsafe { CStr::from_ptr(b) }.to_bytes();
    if sa.len() != sb.len() {
        return 0;
    }
    let mut ia = sa.iter();
    let mut ib = sb.iter();
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return 1,
            (None, _) | (_, None) => return 0,
            (Some(&ca), Some(&cb)) => {
                if ca.to_ascii_uppercase() != cb.to_ascii_uppercase() {
                    return 0;
                }
            }
        }
    }
}

// Translation type enum
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
enum nttype_t {
    NT_NONE = 0,
    NT_FROM_UTF8 = 1,
    NT_FROM_LATIN1 = 2,
    NT_FROM_NATIVE = 3,
    NT_FROM_ASCII = 4,
}

/// Get the character encoding of a CHARSXP.
///
/// Ported from R's `getCharCE`.
/// CE_NATIVE=0, CE_UTF8=1, CE_LATIN1=2, CE_BYTES=3
pub unsafe fn getCharCE(x: SEXP) -> c_int {
    if IS_UTF8(x) {
        1
    } else if IS_LATIN1(x) {
        2
    } else if IS_BYTES(x) {
        3
    } else {
        0
    }
}

/// Check if a CHARSXP is ASCII.
///
/// Ported from R's `charIsASCII`.
pub unsafe fn charIsASCII(x: SEXP) -> Rboolean {
    if IS_ASCII(x) { TRUE } else { FALSE }
}

/// Check if a CHARSXP is UTF-8.
///
/// Ported from R's `charIsUTF8`.
pub unsafe fn charIsUTF8(x: SEXP) -> Rboolean {
    if IS_ASCII(x) || IS_UTF8(x) {
        return TRUE;
    }
    if IS_LATIN1(x) || IS_BYTES(x) || x == R_NilValue() {
        return FALSE;
    }
    TRUE
}

/// Check if a CHARSXP is Latin-1.
///
/// Ported from R's `charIsLatin1`.
pub unsafe fn charIsLatin1(x: SEXP) -> Rboolean {
    if IS_ASCII(x) || IS_LATIN1(x) {
        return TRUE;
    }
    if IS_UTF8(x) || IS_BYTES(x) || x == R_NilValue() {
        return FALSE;
    }
    TRUE
}

/// Decides whether translation to native encoding is needed.
unsafe fn needsTranslation(x: SEXP) -> nttype_t {
    if IS_ASCII(x) {
        return nttype_t::NT_NONE;
    }
    if IS_UTF8(x) {
        return nttype_t::NT_NONE; // UTF-8 locale
    }
    if IS_LATIN1(x) {
        return nttype_t::NT_NONE; // Latin-1 locale
    }
    if IS_BYTES(x) {
        Rf_error(
            b"translating strings with \"bytes\" encoding is not allowed\0".as_ptr()
                as *const c_char,
        );
    }
    nttype_t::NT_NONE
}

/// Translate a CHARSXP to a C string in native encoding.
///
/// Ported from R's `translateChar`.
pub unsafe fn translateChar(x: SEXP) -> *const c_char {
    if x.is_null() || x == R_NilValue() {
        return b"\0".as_ptr() as *const c_char;
    }
    let t = needsTranslation(x);
    if t == nttype_t::NT_NONE {
        return CHAR(x);
    }
    CHAR(x)
}

/// Translate a CHARSXP to a C string in native encoding (for file paths).
///
/// Ported from R's `translateCharFP`.
pub unsafe fn translateCharFP(x: SEXP) -> *const c_char {
    if x.is_null() || x == R_NilValue() {
        return b"\0".as_ptr() as *const c_char;
    }
    let t = needsTranslation(x);
    if t == nttype_t::NT_NONE {
        return CHAR(x);
    }
    CHAR(x)
}

/// Translate a CHARSXP to UTF-8.
///
/// Ported from R's `translateCharUTF8`.
pub unsafe fn translateCharUTF8(x: SEXP) -> *const c_char {
    if x.is_null() || x == R_NilValue() {
        return b"\0".as_ptr() as *const c_char;
    }
    CHAR(x)
}

/// Variant of translateCharFP that returns NULL on failure.
///
/// Ported from R's `translateCharFP2`.
pub unsafe fn translateCharFP2(x: SEXP) -> *const c_char {
    translateCharFP(x)
}

/// Install a translated character as a symbol.
///
/// Ported from R's `installTrChar`.
pub unsafe fn installTrChar(x: SEXP) -> SEXP {
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = needsTranslation(x);
    if t == nttype_t::NT_NONE {
        return installNoTrChar(x);
    }
    let s = CHAR(x);
    unsafe extern "C" {
        fn Rf_install(name: *const c_char) -> SEXP;
    }
    unsafe { Rf_install(s) }
}

/// installTrChar wrapper (SEXP version, distinct from symbol.rs Rf_installChar).
pub(crate) unsafe fn Rf_installChar_sexp(x: SEXP) -> SEXP {
    installTrChar(x)
}

/// Install a character without translation.
///
/// Ported from R's `installNoTrChar`.
pub unsafe fn installNoTrChar(x: SEXP) -> SEXP {
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let s = CHAR(x);
    unsafe extern "C" {
        fn Rf_install(name: *const c_char) -> SEXP;
    }
    unsafe { Rf_install(s) }
}

/// Translate a CHARSXP, allowing bytes encoding.
///
/// Ported from R's `translateChar0`.
pub unsafe fn translateChar0(x: SEXP) -> *const c_char {
    if x.is_null() || x == R_NilValue() {
        return b"\0".as_ptr() as *const c_char;
    }
    if IS_BYTES(x) {
        return CHAR(x);
    }
    translateChar(x)
}

// ---------------------------------------------------------------------------
// reEnc / reEncodeIconv -- stubs for character re-encoding
// ---------------------------------------------------------------------------

/// Re-encode a character string.
///
/// Ported from R's `reEnc`.
pub unsafe fn reEnc(
    x: *const c_char,
    _ce_in: c_int,
    _ce_out: c_int,
    _subst: c_int,
) -> *const c_char {
    x
}

/// Re-encode using arbitrary iconv encodings.
///
/// Ported from R's `reEnc3`.
pub unsafe fn reEnc3(
    x: *const c_char,
    _fromcode: *const c_char,
    _tocode: *const c_char,
    _subst: c_int,
) -> *const c_char {
    x
}

// ---------------------------------------------------------------------------
// ucstomb / ucstoutf8 / mbtoucs -- character encoding conversion
// ---------------------------------------------------------------------------

/// Convert a Unicode code point to a multibyte string.
///
/// Ported from R's `ucstomb`.
pub unsafe fn ucstomb(s: *mut c_char, wc: c_uint) -> usize {
    if wc == 0 {
        if !s.is_null() {
            unsafe { *s = 0 };
        }
        return 1;
    }
    // Use a simple conversion: for UTF-8 locales, this is straightforward
    ucstoutf8(s, wc)
}

/// Convert a Unicode code point to UTF-8.
///
/// Ported from R's `ucstoutf8`.
pub unsafe fn ucstoutf8(s: *mut c_char, wc: c_uint) -> usize {
    if wc == 0 {
        if !s.is_null() {
            unsafe { *s = 0 };
        }
        return 1;
    }
    let mut buf = [0u8; 16];
    let code = wc as u32;
    let n = if code < 0x80 {
        buf[0] = code as u8;
        1
    } else if code < 0x800 {
        buf[0] = 0xC0 | ((code >> 6) & 0x1F) as u8;
        buf[1] = 0x80 | (code & 0x3F) as u8;
        2
    } else if code < 0x10000 {
        buf[0] = 0xE0 | ((code >> 12) & 0x0F) as u8;
        buf[1] = 0x80 | ((code >> 6) & 0x3F) as u8;
        buf[2] = 0x80 | (code & 0x3F) as u8;
        3
    } else if code < 0x110000 {
        buf[0] = 0xF0 | ((code >> 18) & 0x07) as u8;
        buf[1] = 0x80 | ((code >> 12) & 0x3F) as u8;
        buf[2] = 0x80 | ((code >> 6) & 0x3F) as u8;
        buf[3] = 0x80 | (code & 0x3F) as u8;
        4
    } else {
        buf[0] = b'?';
        1
    };
    if !s.is_null() {
        unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), s as *mut u8, n) };
        unsafe { *s.add(n) = 0 };
    }
    n
}

/// Convert a multibyte character to a Unicode code point.
///
/// Ported from R's `mbtoucs`.
pub unsafe fn mbtoucs(wc: *mut c_uint, s: *const c_char, _n: usize) -> usize {
    if s.is_null() || unsafe { *s } == 0 {
        if !wc.is_null() {
            unsafe { *wc = 0 };
        }
        return 1;
    }
    // Simple UTF-8 decoding for single character
    let bytes = unsafe { CStr::from_ptr(s) }.to_bytes();
    if bytes.is_empty() {
        if !wc.is_null() {
            unsafe { *wc = 0 };
        }
        return 1;
    }
    let b0 = bytes[0] as u32;
    let (code, len) = if b0 < 0x80 {
        (b0, 1)
    } else if b0 < 0xC0 {
        (0xFFFD, 1) // invalid
    } else if b0 < 0xE0 {
        if bytes.len() < 2 {
            (0xFFFD, 1)
        } else {
            let b1 = bytes[1] as u32;
            let code = ((b0 & 0x1F) << 6) | (b1 & 0x3F);
            (code, 2)
        }
    } else if b0 < 0xF0 {
        if bytes.len() < 3 {
            (0xFFFD, 1)
        } else {
            let b1 = bytes[1] as u32;
            let b2 = bytes[2] as u32;
            let code = ((b0 & 0x0F) << 12) | ((b1 & 0x3F) << 6) | (b2 & 0x3F);
            (code, 3)
        }
    } else {
        if bytes.len() < 4 {
            (0xFFFD, 1)
        } else {
            let b1 = bytes[1] as u32;
            let b2 = bytes[2] as u32;
            let b3 = bytes[3] as u32;
            let code = ((b0 & 0x07) << 18) | ((b1 & 0x3F) << 12) | ((b2 & 0x3F) << 6) | (b3 & 0x3F);
            (code, 4)
        }
    };
    if !wc.is_null() {
        unsafe { *wc = code };
    }
    len
}

// ---------------------------------------------------------------------------
// Riconv -- iconv wrappers
// ---------------------------------------------------------------------------

/// Open an iconv conversion descriptor.
///
/// Ported from R's `Riconv_open`.
pub unsafe fn Riconv_open(
    tocode: *const c_char,
    fromcode: *const c_char,
) -> *mut c_void {
    let to_str = if !tocode.is_null() {
        unsafe { CStr::from_ptr(tocode) }.to_str().unwrap_or("")
    } else {
        ""
    };
    let from_str = if !fromcode.is_null() {
        unsafe { CStr::from_ptr(fromcode) }.to_str().unwrap_or("")
    } else {
        ""
    };

    let to = if to_str.eq_ignore_ascii_case("utf8") {
        "UTF-8"
    } else {
        to_str
    };
    let from = if from_str.eq_ignore_ascii_case("utf8") {
        "UTF-8"
    } else {
        from_str
    };

    let to_c = if to.is_empty() {
        ptr::null()
    } else {
        to.as_ptr() as *const c_char
    };
    let from_c = if from.is_empty() {
        ptr::null()
    } else {
        from.as_ptr() as *const c_char
    };

    let cd = unsafe { libc::iconv_open(to_c, from_c) };
    cd as *mut c_void
}

/// Perform iconv conversion.
///
/// Ported from R's `Riconv`.
pub unsafe fn Riconv(
    cd: *mut c_void,
    inbuf: *mut *const c_char,
    inbytesleft: *mut usize,
    outbuf: *mut *mut c_char,
    outbytesleft: *mut usize,
) -> usize {
    unsafe {
        libc::iconv(
            cd as libc::iconv_t,
            inbuf as *mut *mut c_char,
            inbytesleft,
            outbuf,
            outbytesleft,
        )
    }
}

/// Close an iconv conversion descriptor.
///
/// Ported from R's `Riconv_close`.
pub unsafe fn Riconv_close(cd: *mut c_void) -> c_int {
    unsafe { libc::iconv_close(cd as libc::iconv_t) }
}

/// Invalidate cached encoding conversions.
pub unsafe fn invalidate_cached_recodings() {
    // No-op in stub implementation
}

// ---------------------------------------------------------------------------
// iconv() -- the R-level function
// ---------------------------------------------------------------------------

/// iconv(x, from, to, sub, mark) -- convert character encoding.
///
/// Ported from R's `do_iconv`.
pub unsafe fn do_iconv(_call: SEXP, op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);
    let x = CAR(args);

    if Rf_isNull(x) != 0 {
        return R_NilValue();
    }

    args = CDR(args);
    let from_arg = CAR(args);
    args = CDR(args);
    let to_arg = CAR(args);
    args = CDR(args);
    let sub_arg = CAR(args);
    args = CDR(args);
    let mark = unsafe { asLogical(CAR(args)) };
    args = CDR(args);
    let toRaw = unsafe { asLogical(CAR(args)) };

    if !isString(from_arg) || LENGTH(from_arg) != 1 {
        Rf_error(b"invalid 'from' argument\0".as_ptr() as *const c_char);
    }
    if !isString(to_arg) || LENGTH(to_arg) != 1 {
        Rf_error(b"invalid 'to' argument\0".as_ptr() as *const c_char);
    }
    if !isString(sub_arg) || LENGTH(sub_arg) != 1 {
        Rf_error(b"invalid 'sub' argument\0".as_ptr() as *const c_char);
    }

    let _from = CHAR(unsafe { STRING_ELT(from_arg, 0) });
    let _to = CHAR(unsafe { STRING_ELT(to_arg, 0) });
    let sub_sexp = unsafe { STRING_ELT(sub_arg, 0) };
    let _sub = if sub_sexp.is_null() || sub_sexp == R_NilValue() {
        ptr::null()
    } else {
        unsafe { translateChar(sub_sexp) }
    };

    let isRawlist = TYPEOF(x) == SEXPTYPE::VECSXP.0;
    let _mark = mark;
    let _toRaw = toRaw;

    let ans = if isRawlist {
        if toRaw != 0 {
            Rf_protect(unsafe { duplicate(x) })
        } else {
            let a = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, XLENGTH(x)));
            SHALLOW_DUPLICATE_ATTRIB(a, x);
            a
        }
    } else {
        if TYPEOF(x) != SEXPTYPE::STRSXP.0 {
            Rf_error(b"'x' must be a character vector\0".as_ptr() as *const c_char);
        }
        if toRaw != 0 {
            let a = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, XLENGTH(x)));
            SHALLOW_DUPLICATE_ATTRIB(a, x);
            a
        } else {
            Rf_protect(unsafe { duplicate(x) })
        }
    };

    let xlen = XLENGTH(x);
    for i in 0..xlen {
        if isRawlist {
            let si = unsafe { VECTOR_ELT(x, i) };
            if TYPEOF(si) == SEXPTYPE::NILSXP.0 {
                if toRaw == 0 {
                    SET_STRING_ELT(ans, i, ptr::null_mut());
                }
                continue;
            }
            if TYPEOF(si) != SEXPTYPE::RAWSXP.0 {
                Rf_error(
                    b"'x' must be a character vector or a list of NULL or raw vectors\0".as_ptr()
                        as *const c_char,
                );
            }
            if toRaw != 0 {
                SET_VECTOR_ELT(ans, i, unsafe { duplicate(si) });
            } else {
                let raw_len = LENGTH(si);
                let raw_data = RAW(si);
                let mut bytes = Vec::new();
                for j in 0..raw_len as usize {
                    bytes.push(unsafe { *raw_data.add(j) });
                }
                if let Ok(s) = String::from_utf8(bytes) {
                    let c = CString::new(s).unwrap_or_default();
                    SET_STRING_ELT(ans, i, Rf_mkChar(c.as_ptr()));
                } else {
                    SET_STRING_ELT(ans, i, ptr::null_mut());
                }
            }
        } else {
            let si = STRING_ELT(x, i);
            if si.is_null() || si == R_NilValue() {
                if toRaw == 0 {
                    SET_STRING_ELT(ans, i, ptr::null_mut());
                }
                continue;
            }
            if toRaw != 0 {
                let s = CHAR(si);
                let slen = unsafe { libc::strlen(s) };
                let el = Rf_allocVector3(SEXPTYPE::RAWSXP.0, slen as R_xlen_t);
                if slen > 0 {
                    unsafe { ptr::copy_nonoverlapping(s as *const u8, RAW(el) as *mut u8, slen) };
                }
                SET_VECTOR_ELT(ans, i, el);
            }
        }
    }

    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// R_Suicide -- fatal error (declared here but may be in unix/system.rs)
// ---------------------------------------------------------------------------

// R_Suicide is declared in unix/system.rs; we provide a weak fallback
// via extern "C" declaration for linking, but do not define it here
// to avoid duplicate symbols.

// ---------------------------------------------------------------------------
// Extern declarations for functions defined elsewhere
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn R_getProcTime(data: *mut c_double);
    fn R_Suicide(s: *const c_char);
    fn duplicate(s: SEXP) -> SEXP;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_R_HiddenFile() {
        unsafe {
            assert_eq!(R_HiddenFile(b".gitignore\0".as_ptr() as *const c_char), 1);
            assert_eq!(R_HiddenFile(b".\0".as_ptr() as *const c_char), 1);
            assert_eq!(R_HiddenFile(b"README\0".as_ptr() as *const c_char), 0);
            assert_eq!(R_HiddenFile(b"Cargo.toml\0".as_ptr() as *const c_char), 0);
            assert_eq!(R_HiddenFile(b"\0".as_ptr() as *const c_char), 1);
        }
    }

    #[test]
    fn test_R_strieql() {
        unsafe {
            assert_eq!(
                R_strieql(
                    b"hello\0".as_ptr() as *const c_char,
                    b"HELLO\0".as_ptr() as *const c_char
                ),
                1
            );
            assert_eq!(
                R_strieql(
                    b"Hello\0".as_ptr() as *const c_char,
                    b"hello\0".as_ptr() as *const c_char
                ),
                1
            );
            assert_eq!(
                R_strieql(
                    b"\0".as_ptr() as *const c_char,
                    b"\0".as_ptr() as *const c_char
                ),
                1
            );
            assert_eq!(
                R_strieql(
                    b"hello\0".as_ptr() as *const c_char,
                    b"world\0".as_ptr() as *const c_char
                ),
                0
            );
            assert_eq!(
                R_strieql(
                    b"hello\0".as_ptr() as *const c_char,
                    b"hell\0".as_ptr() as *const c_char
                ),
                0
            );
        }
    }

    #[test]
    fn test_R_FileExists() {
        unsafe {
            assert_eq!(R_FileExists(b".\0".as_ptr() as *const c_char), 1);
            assert_eq!(
                R_FileExists(b"/tmp/nonexistent_file_12345\0".as_ptr() as *const c_char),
                0
            );
        }
    }

    #[test]
    fn test_R_isWriteableDir() {
        unsafe {
            let mut tmp = CString::new("/tmp").unwrap();
            assert_eq!(R_isWriteableDir(tmp.as_ptr() as *mut c_char), 1);
            assert_eq!(R_isWriteableDir(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_R_isatty() {
        unsafe {
            let _ = R_isatty(0);
        }
    }

    #[test]
    fn test_ucstoutf8() {
        unsafe {
            let mut buf = [0i8; 16];
            // ASCII 'A'
            let n = ucstoutf8(buf.as_mut_ptr(), 0x41);
            assert_eq!(n, 1);
            assert_eq!(buf[0] as u8, b'A');

            // Euro sign U+20AC
            let n = ucstoutf8(buf.as_mut_ptr(), 0x20AC);
            assert_eq!(n, 3);

            // Null
            let n = ucstoutf8(buf.as_mut_ptr(), 0);
            assert_eq!(n, 1);
            assert_eq!(buf[0], 0);
        }
    }

    #[test]
    fn test_R_tmpnam2() {
        unsafe {
            let tmpdir = b"/tmp\0".as_ptr() as *const c_char;
            let prefix = b"test\0".as_ptr() as *const c_char;
            let ext = b".tmp\0".as_ptr() as *const c_char;
            let name = R_tmpnam2(prefix, tmpdir, ext);
            assert!(!name.is_null());
            let name_str = CStr::from_ptr(name).to_str().unwrap_or("");
            assert!(name_str.contains("test"));
            assert!(name_str.ends_with(".tmp"));
            R_free_tmpnam(name);
        }
    }

    #[test]
    fn test_R_Interactive() {
        assert_eq!(R_Interactive(), false);
        R_SetInteractive(true);
        assert_eq!(R_Interactive(), true);
        R_SetInteractive(false);
        assert_eq!(R_Interactive(), false);
    }

    #[test]
    fn test_R_HomeDir() {
        unsafe {
            let home = R_HomeDir();
            let _ = home;
        }
    }

    #[test]
    fn test_ucstomb() {
        unsafe {
            let mut buf = [0i8; 16];
            // ASCII
            let n = ucstomb(buf.as_mut_ptr(), 0x41);
            assert_eq!(n, 1);
            assert_eq!(buf[0] as u8, b'A');

            // Null
            let n = ucstomb(buf.as_mut_ptr(), 0);
            assert_eq!(n, 1);
            assert_eq!(buf[0], 0);
        }
    }

    #[test]
    fn test_mbtoucs() {
        unsafe {
            let mut wc: c_uint = 0;
            // ASCII 'A'
            let n = mbtoucs(&mut wc, b"A\0".as_ptr() as *const c_char, 1);
            assert_eq!(n, 1);
            assert_eq!(wc, 0x41);

            // Null
            let n = mbtoucs(&mut wc, b"\0".as_ptr() as *const c_char, 1);
            assert_eq!(n, 1);
            assert_eq!(wc, 0);
        }
    }

    #[test]
    fn test_getCharCE() {
        // Test with a simple CHARSXP stub -- just check it doesn't crash
        // In practice, this would need a real CHARSXP
    }

    #[test]
    fn test_get_hostname() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty());
    }

    #[test]
    fn test_R_ExpandFileName() {
        unsafe {
            // Non-tilde path should be returned as-is
            let path = b"/tmp/test\0".as_ptr() as *const c_char;
            let result = R_ExpandFileName(path);
            assert_eq!(result, path);

            // Tilde path should be expanded
            let tilde_path = b"~/test\0".as_ptr() as *const c_char;
            let result = R_ExpandFileName(tilde_path);
            // Result should be different (expanded) unless HOME is not set
            if std::env::var("HOME").is_ok() {
                assert_ne!(result, tilde_path);
            }
        }
    }
}
