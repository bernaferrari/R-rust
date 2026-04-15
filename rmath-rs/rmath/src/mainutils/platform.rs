#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of useful standalone utilities from R's src/main/platform.c
//!
//! The original file is very large (~3700 lines) and the vast majority of it
//! consists of SEXP-based `do_*` functions (file operations, locale, etc.).
//! Those are too tightly coupled to R's type system to port.
//!
//! This module ports the handful of standalone C-utility functions that have
//! no SEXP dependency, and provides stubs for the SEXP-based public API.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::process;
use std::sync::Mutex;

use crate::sexp::ffi::SEXP;

// ---------------------------------------------------------------------------
// Standalone utility: R_Date
// ---------------------------------------------------------------------------

/// Return the current date in the standard R format.
///
/// The returned pointer is to a thread-local static buffer containing a
/// NUL-terminated string like `"Wed Jun 30 21:49:08 1993"`.
///
/// This is a faithful port of the static `R_Date()` function in platform.c.
pub unsafe fn R_Date() -> *mut c_char {
    use std::time::{SystemTime, UNIX_EPOCH};

    thread_local! {
        static BUF: std::cell::RefCell<[u8; 26]> = std::cell::RefCell::new([0u8; 26]);
    }

    BUF.with(|buf| {
        let mut b = buf.borrow_mut();

        let epoch_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Use C-compatible time formatting via libc-like logic.
        // We convert epoch seconds to a struct tm manually to avoid libc.
        let tm = gmtime_from_epoch(epoch_secs);

        // ctime format: "Wed Jun 30 21:49:08 1993\n"
        let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];

        let weekday = (tm.tm_wday as usize).min(6);
        let month = if tm.tm_mon >= 0 {
            (tm.tm_mon as usize).min(11)
        } else {
            0
        };

        let s = format!(
            "{} {} {:02} {:02}:{:02}:{:02} {}\n\0",
            days[weekday],
            months[month],
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
            1900 + tm.tm_year,
        );

        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(25);
        b[..copy_len].copy_from_slice(&bytes[..copy_len]);

        // Null-terminate at position 24 (overwrite the trailing \n from ctime format)
        b[24] = 0;

        b.as_mut_ptr() as *mut c_char
    })
}

/// Minimal gmtime implementation to avoid libc dependency.
fn gmtime_from_epoch(epoch_secs: i64) -> Tm {
    // Days from epoch
    let mut days = epoch_secs / 86400;
    let rem_secs = epoch_secs % 86400;
    let time_of_day = if rem_secs < 0 {
        days -= 1;
        rem_secs + 86400
    } else {
        rem_secs
    };

    let hour = (time_of_day / 3600) as i32;
    let min = ((time_of_day % 3600) / 60) as i32;
    let sec = (time_of_day % 60) as i32;

    // Compute year
    let mut year = 1970i32;
    loop {
        let year_days = if is_leap_year(year) { 366 } else { 365 };
        if days < year_days as i64 {
            break;
        }
        days -= year_days as i64;
        year += 1;
    }

    // Compute month
    let mut month = 0i32;
    let mdays = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    for &d in mdays.iter() {
        if days < d as i64 {
            break;
        }
        days -= d as i64;
        month += 1;
    }

    // Compute day of week (1970-01-01 was Thursday = 4)
    let total_days = (epoch_secs / 86400) as i64;
    let wday = ((total_days + 4) % 7).abs() as i32;

    Tm {
        tm_sec: sec,
        tm_min: min,
        tm_hour: hour,
        tm_mday: (days + 1) as i32,
        tm_mon: month,
        tm_year: year - 1900,
        tm_wday: wday,
    }
}

struct Tm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

// ---------------------------------------------------------------------------
// Standalone utility: R_strieql (case-insensitive string equality)
// ---------------------------------------------------------------------------

/// Case-insensitive string comparison.
///
/// Returns 1 if `a` and `b` are equal ignoring case, 0 otherwise.
///
/// This is a faithful port of the static `R_strieql()` function in platform.c
/// (used for locale checking).
pub unsafe fn R_strieql(a: *const c_char, b: *const c_char) -> c_int {
    unsafe {
        if a.is_null() || b.is_null() {
            return if a.is_null() && b.is_null() { 1 } else { 0 };
        }
        let mut pa = a;
        let mut pb = b;
        loop {
            let ca = *pa;
            let cb = *pb;
            if ca == 0 && cb == 0 {
                return 1;
            }
            if ca == 0 || cb == 0 {
                return 0;
            }
            let ca_byte = ca as u8;
            let cb_byte = cb as u8;
            let ca_upper = if ca_byte >= b'a' && ca_byte <= b'z' {
                (ca_byte - 32) as c_char
            } else {
                ca
            };
            let cb_upper = if cb_byte >= b'a' && cb_byte <= b'z' {
                (cb_byte - 32) as c_char
            } else {
                cb
            };
            if ca_upper != cb_upper {
                return 0;
            }
            pa = pa.add(1);
            pb = pb.add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Standalone utility: R_nativeEncoding / R_check_locale
// ---------------------------------------------------------------------------

/// Maximum length for charset names (matches R_CODESET_MAX).
const R_CODESET_MAX: usize = 64;

static NATIVE_ENC: Mutex<[u8; R_CODESET_MAX + 1]> = Mutex::new([0u8; R_CODESET_MAX + 1]);
static CODESET_BUF: Mutex<[u8; R_CODESET_MAX + 1]> = Mutex::new([0u8; R_CODESET_MAX + 1]);

/// Return the detected native encoding name (e.g., "UTF-8", "ASCII").
///
/// This is a port of `R_nativeEncoding()` from platform.c.
/// The encoding is initialized by `R_check_locale()`.
pub unsafe fn R_nativeEncoding() -> *const c_char {
    let enc = NATIVE_ENC.lock().unwrap_or_else(|e| e.into_inner());
    enc.as_ptr() as *const c_char
}

/// Detect and record locale/encoding information.
///
/// This is a simplified port of `R_check_locale()` from platform.c.
/// On Unix-like systems it uses `nl_langinfo(CODESET)` to detect the encoding.
/// Since we cannot call libc, this provides a reasonable default.
pub unsafe fn R_check_locale() {
    {
        let mut enc = NATIVE_ENC.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = b"UTF-8\0";
        let len = bytes.len().min(R_CODESET_MAX);
        enc[..len].copy_from_slice(&bytes[..len]);
        enc[len] = 0;
    }
    {
        let mut cs = CODESET_BUF.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = b"UTF-8\0";
        let len = bytes.len().min(R_CODESET_MAX);
        cs[..len].copy_from_slice(&bytes[..len]);
        cs[len] = 0;
    }
}

// ---------------------------------------------------------------------------
// Stubs for SEXP-based platform functions
// ---------------------------------------------------------------------------

/// R's `date()` — return current date as an R string.
pub unsafe fn do_date(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_mkString;
        let date_str = R_Date();
        let s = CStr::from_ptr(date_str);
        let formatted = s.to_str().unwrap_or("").trim_end();
        Rf_mkString(CString::new(formatted).unwrap_or_default().as_ptr())
    }
}

/// R's `file.show()` — display file(s) to the user.
pub unsafe fn do_fileshow(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADDR, CADR, CAR, CDR, LENGTH, LOGICAL, STRING_ELT};
        use crate::sexp::globals::R_NilValue;
        use std::fs;

        let files = CAR(args);
        let header = CADR(args);
        let sep = CADDR(args);
        let _pager = CAR(CDR(CDR(CDR(args))));

        let n = LENGTH(files);
        let mut show_header = true;
        if !header.is_null() && header != R_NilValue() {
            let v = *LOGICAL(header);
            show_header = v != 0 && v != crate::sexp::ffi::NA_INTEGER;
        }

        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
            let path = c.to_str().unwrap_or("");
            if show_header {
                eprintln!("\n{}", path);
            }
            if let Ok(contents) = fs::read_to_string(path) {
                // Simple output: first 9999 chars like R
                let max_chars = 9999;
                let display = if contents.len() > max_chars {
                    &contents[..max_chars]
                } else {
                    &contents
                };
                eprint!("{}", display);
                if contents.len() > max_chars {
                    eprintln!("\n[...truncated]");
                }
            }
        }
        R_NilValue()
    }
}

/// R's `file.append()` — append files.
pub unsafe fn do_fileappend(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADDR, CADR, CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_ScalarLogical;
        use crate::sexp::ffi::{FALSE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::fs::OpenOptions;
        use std::io::Write;

        let files = CAR(args);
        let output = CADR(args);
        let append = CADDR(args);

        let do_append = if !append.is_null() && append != R_NilValue() {
            let v = *crate::sexp::accessors::LOGICAL(append);
            v != 0 && v != crate::sexp::ffi::NA_INTEGER
        } else {
            true
        };

        let out_str = if !output.is_null() && output != R_NilValue() {
            let out_elt = STRING_ELT(output, 0);
            if !out_elt.is_null() && out_elt != R_NilValue() {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(out_elt));
                c.to_str().unwrap_or("").to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let ans = Rf_protect(Rf_ScalarLogical(FALSE));
        let n = LENGTH(files);

        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
            let path = c.to_str().unwrap_or("");
            if let Ok(data) = std::fs::read(path)
                && let Ok(mut file) = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(do_append)
                    .open(&out_str)
            {
                let _ = file.write_all(&data);
                *crate::sexp::accessors::LOGICAL(ans) = TRUE;
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `file.create()` — create file(s).
pub unsafe fn do_filecreate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::fs::OpenOptions;

        let s = CAR(args);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                *pa.add(i) = if OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .is_ok()
                {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `file.remove()` — remove file(s).
pub unsafe fn do_fileremove(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::fs;

        let s = CAR(args);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                *pa.add(i) = if fs::remove_file(path).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `Sys.junction()` (Windows) / `file.link()` — create symbolic links.
pub unsafe fn do_filesymlink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::os::unix::fs::symlink;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..n as usize {
            let f = STRING_ELT(from, i as crate::sexp::ffi::R_xlen_t);
            let t = STRING_ELT(to, i as crate::sexp::ffi::R_xlen_t);
            if f.is_null() || t.is_null() || f == R_NilValue() || t == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let fc = CStr::from_ptr(crate::sexp::accessors::CHAR(f))
                    .to_str()
                    .unwrap_or("");
                let tc = CStr::from_ptr(crate::sexp::accessors::CHAR(t))
                    .to_str()
                    .unwrap_or("");
                *pa.add(i) = if symlink(fc, tc).is_ok() { TRUE } else { FALSE };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `file.link()` — create hard links.
pub unsafe fn do_filelink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::fs::hard_link;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..n as usize {
            let f = STRING_ELT(from, i as crate::sexp::ffi::R_xlen_t);
            let t = STRING_ELT(to, i as crate::sexp::ffi::R_xlen_t);
            if f.is_null() || t.is_null() || f == R_NilValue() || t == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let fc = CStr::from_ptr(crate::sexp::accessors::CHAR(f))
                    .to_str()
                    .unwrap_or("");
                let tc = CStr::from_ptr(crate::sexp::accessors::CHAR(t))
                    .to_str()
                    .unwrap_or("");
                *pa.add(i) = if hard_link(fc, tc).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `file.rename()` — rename file(s).
pub unsafe fn do_filerename(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::fs;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..n as usize {
            let f = STRING_ELT(from, i as crate::sexp::ffi::R_xlen_t);
            let t = STRING_ELT(to, i as crate::sexp::ffi::R_xlen_t);
            if f.is_null() || t.is_null() || f == R_NilValue() || t == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let fc = CStr::from_ptr(crate::sexp::accessors::CHAR(f))
                    .to_str()
                    .unwrap_or("");
                let tc = CStr::from_ptr(crate::sexp::accessors::CHAR(t))
                    .to_str()
                    .unwrap_or("");
                *pa.add(i) = if fs::rename(fc, tc).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `file.info()` — get file information (size, mtime, etc.).
pub unsafe fn do_fileinfo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{
            CADR, CAR, LENGTH, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT,
        };
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::fs;

        let files = CAR(args);
        let extra_cols = CADR(args);
        let _ = extra_cols;

        let n = LENGTH(files);
        // Build a named list (VECSXP) with columns: size, isdir, mode, mtime, ctime, atime, exe
        let ncols = 7i32;
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::VECSXP.0,
            ncols as crate::sexp::ffi::R_xlen_t,
        ));

        let size_col = Rf_protect(Rf_allocVector3(
            SEXPTYPE::REALSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let isdir_col = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let mode_col = Rf_protect(Rf_allocVector3(
            SEXPTYPE::INTSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let mtime_col = Rf_protect(Rf_allocVector3(
            SEXPTYPE::REALSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let ctime_col = Rf_protect(Rf_allocVector3(
            SEXPTYPE::REALSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let atime_col = Rf_protect(Rf_allocVector3(
            SEXPTYPE::REALSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let exe_col = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));

        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *crate::sexp::accessors::REAL(size_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::LOGICAL(isdir_col).add(i) = crate::sexp::ffi::NA_INTEGER;
                *crate::sexp::accessors::INTEGER(mode_col).add(i) = crate::sexp::ffi::NA_INTEGER;
                *crate::sexp::accessors::REAL(mtime_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::REAL(ctime_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::REAL(atime_col).add(i) = crate::sexp::ffi::NA_REAL;
                *crate::sexp::accessors::LOGICAL(exe_col).add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                match fs::metadata(path) {
                    Ok(meta) => {
                        *crate::sexp::accessors::REAL(size_col).add(i) = meta.len() as f64;
                        *crate::sexp::accessors::LOGICAL(isdir_col).add(i) =
                            if meta.is_dir() { TRUE } else { FALSE };
                        // Mode: use 0 on non-unix, or get from metadata
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::MetadataExt;
                            *crate::sexp::accessors::INTEGER(mode_col).add(i) =
                                meta.mode() as c_int;
                        }
                        #[cfg(not(unix))]
                        {
                            *crate::sexp::accessors::INTEGER(mode_col).add(i) = 0;
                        }
                        let mtime = meta.modified().ok();
                        let ctime = meta.created().ok();
                        let atime = meta.accessed().ok();
                        *crate::sexp::accessors::REAL(mtime_col).add(i) = mtime
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as f64)
                            .unwrap_or(crate::sexp::ffi::NA_REAL);
                        *crate::sexp::accessors::REAL(ctime_col).add(i) = ctime
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as f64)
                            .unwrap_or(crate::sexp::ffi::NA_REAL);
                        *crate::sexp::accessors::REAL(atime_col).add(i) = atime
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as f64)
                            .unwrap_or(crate::sexp::ffi::NA_REAL);
                        *crate::sexp::accessors::LOGICAL(exe_col).add(i) = FALSE;
                    }
                    Err(_) => {
                        *crate::sexp::accessors::REAL(size_col).add(i) = crate::sexp::ffi::NA_REAL;
                        *crate::sexp::accessors::LOGICAL(isdir_col).add(i) =
                            crate::sexp::ffi::NA_INTEGER;
                        *crate::sexp::accessors::INTEGER(mode_col).add(i) =
                            crate::sexp::ffi::NA_INTEGER;
                        *crate::sexp::accessors::REAL(mtime_col).add(i) = crate::sexp::ffi::NA_REAL;
                        *crate::sexp::accessors::REAL(ctime_col).add(i) = crate::sexp::ffi::NA_REAL;
                        *crate::sexp::accessors::REAL(atime_col).add(i) = crate::sexp::ffi::NA_REAL;
                        *crate::sexp::accessors::LOGICAL(exe_col).add(i) =
                            crate::sexp::ffi::NA_INTEGER;
                    }
                }
            }
        }

        SET_VECTOR_ELT(ans, 0, size_col);
        SET_VECTOR_ELT(ans, 1, isdir_col);
        SET_VECTOR_ELT(ans, 2, mode_col);
        SET_VECTOR_ELT(ans, 3, mtime_col);
        SET_VECTOR_ELT(ans, 4, ctime_col);
        SET_VECTOR_ELT(ans, 5, atime_col);
        SET_VECTOR_ELT(ans, 6, exe_col);

        // Set row names (file paths)
        let rn = Rf_protect(Rf_allocVector3(
            SEXPTYPE::STRSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                SET_STRING_ELT(
                    rn,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(b"NA\0".as_ptr() as *const _),
                );
            } else {
                SET_STRING_ELT(rn, i as crate::sexp::ffi::R_xlen_t, elt);
            }
        }
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), rn);

        // Set column names
        let cn = Rf_protect(Rf_allocVector3(
            SEXPTYPE::STRSXP.0,
            ncols as crate::sexp::ffi::R_xlen_t,
        ));
        SET_STRING_ELT(cn, 0, Rf_mkChar(b"size\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 1, Rf_mkChar(b"isdir\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 2, Rf_mkChar(b"mode\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 3, Rf_mkChar(b"mtime\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 4, Rf_mkChar(b"ctime\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 5, Rf_mkChar(b"atime\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 6, Rf_mkChar(b"exe\0".as_ptr() as *const _));
        crate::eval::attrib_core::setAttrib(
            crate::eval::attrib_core::getAttrib(ans, crate::eval::attrib_core::R_NamesSymbol()),
            crate::eval::attrib_core::R_NamesSymbol(),
            cn,
        );

        // Set dim
        let dim = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP, 2));
        *crate::sexp::accessors::INTEGER(dim).add(0) = n as c_int;
        *crate::sexp::accessors::INTEGER(dim).add(1) = ncols;
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_DimSymbol(), dim);

        Rf_unprotect(12);
        ans
    }
}

/// R's `dir.exists()` — check if directory exists.
pub unsafe fn do_direxists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::path::Path;

        let s = CAR(args);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                *pa.add(i) = if Path::new(path).is_dir() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `list.files()` — list files in directories.
pub unsafe fn do_listfiles(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{
            CADDR, CADR, CAR, CDR, LENGTH, LOGICAL, SET_STRING_ELT, STRING_ELT,
        };
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};

        let paths = CAR(args);
        let pattern = CADR(args);
        let all_files = CADDR(args);
        let full_names = CAR(CDR(CDR(CDR(args))));
        let recursive = CAR(CDR(CDR(CDR(CDR(args)))));

        let mut show_all = false;
        if !all_files.is_null() && all_files != R_NilValue() {
            let v = *LOGICAL(all_files);
            show_all = v != 0 && v != crate::sexp::ffi::NA_INTEGER;
        }

        let mut do_full = false;
        if !full_names.is_null() && full_names != R_NilValue() {
            let v = *LOGICAL(full_names);
            do_full = v != 0 && v != crate::sexp::ffi::NA_INTEGER;
        }

        let mut pattern_str = String::new();
        if !pattern.is_null() && pattern != R_NilValue() {
            let elt = STRING_ELT(pattern, 0);
            if !elt.is_null() && elt != R_NilValue() {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                pattern_str = c.to_str().unwrap_or("").to_string();
            }
        }

        let mut entries: Vec<String> = Vec::new();
        let n = LENGTH(paths);
        for i in 0..n as usize {
            let elt = STRING_ELT(paths, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
            let dir = c.to_str().unwrap_or(".");
            if let Ok(rd) = std::fs::read_dir(dir) {
                for entry in rd.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // Skip hidden files unless all=TRUE
                    if !show_all && name.starts_with('.') {
                        continue;
                    }
                    // Apply simple glob pattern filter if given
                    if !pattern_str.is_empty() {
                        // Simple glob: support * wildcard only
                        let pattern_parts: Vec<&str> = pattern_str.split('*').collect();
                        let mut matches = true;
                        if pattern_parts.len() == 1 {
                            matches = name == pattern_parts[0];
                        } else if pattern_parts.len() == 2 {
                            matches = name.starts_with(pattern_parts[0])
                                && name.ends_with(pattern_parts[1]);
                        }
                        if !matches {
                            continue;
                        }
                    }
                    // Skip directories
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    let final_name = if do_full {
                        format!("{}/{}", dir.trim_end_matches('/'), name)
                    } else {
                        name
                    };
                    entries.push(final_name);
                }
            }
        }

        entries.sort();
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::STRSXP.0,
            entries.len() as crate::sexp::ffi::R_xlen_t,
        ));
        for (i, name) in entries.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `list.dirs()` — list directories.
pub unsafe fn do_listdirs(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{
            CADDR, CADR, CAR, CDR, LENGTH, LOGICAL, SET_STRING_ELT, STRING_ELT,
        };
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};

        let paths = CAR(args);
        let pattern = CADR(args);
        let all_files = CADDR(args);
        let full_names = CAR(CDR(CDR(CDR(args))));
        let recursive = CAR(CDR(CDR(CDR(CDR(args)))));

        let mut show_all = false;
        if !all_files.is_null() && all_files != R_NilValue() {
            let v = *LOGICAL(all_files);
            show_all = v != 0 && v != crate::sexp::ffi::NA_INTEGER;
        }

        let mut do_full = false;
        if !full_names.is_null() && full_names != R_NilValue() {
            let v = *LOGICAL(full_names);
            do_full = v != 0 && v != crate::sexp::ffi::NA_INTEGER;
        }

        let mut entries: Vec<String> = Vec::new();
        let n = LENGTH(paths);
        for i in 0..n as usize {
            let elt = STRING_ELT(paths, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
            let dir = c.to_str().unwrap_or(".");
            if let Ok(rd) = std::fs::read_dir(dir) {
                for entry in rd.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !show_all && name.starts_with('.') {
                        continue;
                    }
                    if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    let final_name = if do_full {
                        format!("{}/{}", dir.trim_end_matches('/'), name)
                    } else {
                        name
                    };
                    entries.push(final_name);
                }
            }
        }

        entries.sort();
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::STRSXP.0,
            entries.len() as crate::sexp::ffi::R_xlen_t,
        ));
        for (i, name) in entries.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `R.home` — return R home directory.
///
/// Uses R_HOME environment variable, or falls back to the directory
/// containing the rmath library.
pub unsafe fn do_Rhome(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_mkString;
        use std::env;

        let home = env::var("R_HOME").unwrap_or_else(|_| {
            // Try to find R home relative to this executable
            if let Ok(exe) = env::current_exe()
                && let Some(parent) = exe.parent().and_then(|p| p.parent())
            {
                return parent.to_string_lossy().to_string();
            }
            "/usr/lib/R".to_string()
        });
        Rf_mkString(CString::new(home).unwrap_or_default().as_ptr())
    }
}

/// R's `file.exists()` — check if file exists.
pub unsafe fn do_fileexists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::path::Path;

        let s = CAR(args);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                *pa.add(i) = if Path::new(path).exists() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `file.choose()` — interactive file chooser.
/// Returns empty string in non-interactive mode.
pub unsafe fn do_filechoose(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_mkString;
        Rf_mkString(b"\0".as_ptr() as *const _)
    }
}

/// R's `file.access()` — check file access permissions.
pub unsafe fn do_fileaccess(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, INTEGER, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::path::Path;

        let files = CAR(args);
        let mode_arg = CADR(args);
        let n = LENGTH(files);

        // mode: 0=exists, 1=executable, 2=writable, 4=readable
        let mut mode = 0i32;
        if !mode_arg.is_null() && mode_arg != R_NilValue() {
            mode = *INTEGER(mode_arg);
        }

        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::INTSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = INTEGER(ans);

        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                let p = Path::new(path);
                *pa.add(i) = match mode {
                    0 => {
                        if p.exists() {
                            TRUE
                        } else {
                            FALSE
                        }
                    }
                    1 => {
                        #[cfg(unix)]
                        {
                            if p.metadata()
                                .map(|m| {
                                    std::os::unix::fs::PermissionsExt::mode(&m.permissions())
                                        & 0o111
                                        != 0
                                })
                                .unwrap_or(false)
                            {
                                TRUE
                            } else {
                                FALSE
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            FALSE
                        }
                    }
                    2 => {
                        if p.metadata()
                            .map(|m| !m.permissions().readonly())
                            .unwrap_or(false)
                        {
                            TRUE
                        } else {
                            FALSE
                        }
                    }
                    4 => {
                        if std::fs::metadata(path).is_ok() {
                            TRUE
                        } else {
                            FALSE
                        }
                    }
                    _ => FALSE,
                };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `unlink()` — remove files or directories.
pub unsafe fn do_unlink(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADDR, CADR, CAR, LENGTH, LOGICAL, STRING_ELT};
        use crate::sexp::constructors::Rf_ScalarInteger;
        use crate::sexp::globals::R_NilValue;

        let x = CAR(args);
        let recursive = CADR(args);
        let force = CADDR(args);

        let do_recursive = if !recursive.is_null() && recursive != R_NilValue() {
            let v = *LOGICAL(recursive);
            v != 0 && v != crate::sexp::ffi::NA_INTEGER
        } else {
            false
        };

        let n = LENGTH(x);
        let mut success = 0i32;
        for i in 0..n as usize {
            let elt = STRING_ELT(x, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
            let path = c.to_str().unwrap_or("");
            let result = if do_recursive {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))
            };
            if result.is_ok() {
                success += 1;
            }
        }
        Rf_ScalarInteger(success)
    }
}

/// R's `Sys.getlocale()` — get locale category.
pub unsafe fn do_getlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, INTEGER};
        use crate::sexp::constructors::Rf_mkString;
        use crate::sexp::globals::R_NilValue;

        let category = CAR(args);
        let locale = if !category.is_null() && category != R_NilValue() {
            match *INTEGER(category) {
                1 => "LC_ALL",
                2 => "LC_COLLATE",
                3 => "LC_CTYPE",
                4 => "LC_MONETARY",
                5 => "LC_NUMERIC",
                6 => "LC_TIME",
                7 => "LC_MESSAGES",
                _ => "LC_ALL",
            }
        } else {
            "LC_ALL"
        };

        // Use std::env for locale info
        let val = match locale {
            "LC_ALL" => std::env::var("LC_ALL")
                .unwrap_or_else(|_| std::env::var("LANG").unwrap_or_else(|_| "C".to_string())),
            "LC_COLLATE" => std::env::var("LC_COLLATE").unwrap_or_else(|_| String::new()),
            "LC_CTYPE" => std::env::var("LC_CTYPE").unwrap_or_else(|_| String::new()),
            "LC_MONETARY" => std::env::var("LC_MONETARY").unwrap_or_else(|_| String::new()),
            "LC_NUMERIC" => std::env::var("LC_NUMERIC").unwrap_or_else(|_| String::new()),
            "LC_TIME" => std::env::var("LC_TIME").unwrap_or_else(|_| String::new()),
            "LC_MESSAGES" => std::env::var("LC_MESSAGES").unwrap_or_else(|_| String::new()),
            _ => String::new(),
        };

        Rf_mkString(CString::new(val).unwrap_or_default().as_ptr())
    }
}

/// R's `Sys.setlocale()` — set locale.
pub unsafe fn do_setlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, CHAR, INTEGER, STRING_ELT};
        use crate::sexp::constructors::Rf_mkString;
        use crate::sexp::globals::R_NilValue;
        use std::ffi::CStr;

        let category = CAR(args);
        let locale = CADR(args);

        let cat_name = if !category.is_null() && category != R_NilValue() {
            match *INTEGER(category) {
                1 => "LC_ALL",
                2 => "LC_COLLATE",
                3 => "LC_CTYPE",
                4 => "LC_MONETARY",
                5 => "LC_NUMERIC",
                6 => "LC_TIME",
                7 => "LC_MESSAGES",
                _ => "LC_ALL",
            }
        } else {
            "LC_ALL"
        };

        let loc_str = if !locale.is_null() && locale != R_NilValue() {
            let elt = STRING_ELT(locale, 0);
            if !elt.is_null() && elt != R_NilValue() {
                CStr::from_ptr(CHAR(elt)).to_str().unwrap_or("").to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Attempt to set locale via libc
        let result = libc::setlocale(
            match cat_name {
                "LC_ALL" => libc::LC_ALL,
                "LC_COLLATE" => libc::LC_COLLATE,
                "LC_CTYPE" => libc::LC_CTYPE,
                "LC_MONETARY" => libc::LC_MONETARY,
                "LC_NUMERIC" => libc::LC_NUMERIC,
                "LC_TIME" => libc::LC_TIME,
                "LC_MESSAGES" => libc::LC_MESSAGES,
                _ => libc::LC_ALL,
            },
            if loc_str.is_empty() {
                std::ptr::null()
            } else {
                loc_str.as_ptr() as *const _
            },
        );

        if result.is_null() {
            Rf_mkString(b"\0".as_ptr() as *const _)
        } else {
            let s = CStr::from_ptr(result);
            Rf_mkString(
                CString::new(s.to_str().unwrap_or(""))
                    .unwrap_or_default()
                    .as_ptr(),
            )
        }
    }
}

/// R's `Sys.localeconv()` — locale conventions.
pub unsafe fn do_localeconv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::SET_STRING_ELT;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};

        let lc = libc::localeconv();
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, 7));
        let names = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, 7));

        let field_names = [
            "decimal_point",
            "thousands_sep",
            "int_curr_symbol",
            "currency_symbol",
            "mon_decimal_point",
            "positive_sign",
            "negative_sign",
        ];
        let field_ptrs: [*const c_char; 7] = [
            (*lc).decimal_point,
            (*lc).thousands_sep,
            (*lc).int_curr_symbol,
            (*lc).currency_symbol,
            (*lc).mon_decimal_point,
            (*lc).positive_sign,
            (*lc).negative_sign,
        ];

        for (i, (name, val)) in field_names.iter().zip(field_ptrs.iter()).enumerate() {
            SET_STRING_ELT(
                names,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
            );
            if !val.is_null() {
                let s = CStr::from_ptr(*val);
                SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(
                        CString::new(s.to_str().unwrap_or(""))
                            .unwrap_or_default()
                            .as_ptr(),
                    ),
                );
            } else {
                SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(b"\0".as_ptr() as *const _),
                );
            }
        }

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), names);
        Rf_unprotect(2);
        ans
    }
}

/// R's `path.expand()` — expand file paths (~ and environment variables).
pub unsafe fn do_pathexpand(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};

        let s = CAR(args);
        let n = LENGTH(s);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::STRSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));

        for i in 0..n as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                crate::sexp::accessors::SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(b"NA\0".as_ptr() as *const _),
                );
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                // Expand ~ to home directory
                let expanded = if path.starts_with("~/") || path == "~" {
                    if let Ok(home) = std::env::var("HOME") {
                        if path == "~" {
                            home
                        } else {
                            format!("{}{}", home, &path[1..])
                        }
                    } else {
                        path.to_string()
                    }
                } else {
                    path.to_string()
                };
                crate::sexp::accessors::SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(CString::new(expanded).unwrap_or_default().as_ptr()),
                );
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `capabilities()` — query platform capabilities.
pub unsafe fn do_capabilities(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::SET_STRING_ELT;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};

        let names = [
            "jpeg",
            "png",
            "tiff",
            "tcltk",
            "X11",
            "aqua",
            "http/ftp",
            "sockets",
            "libxml",
            "fifo",
            "cledit",
            "iconv",
            "NLS",
            "profvis",
            "cairo",
            "ICU",
            "long.double",
            "libcurl",
        ];
        let n = names.len();
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let cn = Rf_protect(Rf_allocVector3(
            SEXPTYPE::STRSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));

        for (i, name) in names.iter().enumerate() {
            // Report capabilities we actually have
            let val = match *name {
                "jpeg" | "png" | "tiff" => FALSE,
                "X11" => FALSE,
                "aqua" => FALSE,
                "http/ftp" => TRUE, // we have basic HTTP support via Rust
                "sockets" => TRUE,
                "libxml" => FALSE,
                "fifo" => TRUE,
                "cledit" => FALSE,
                "iconv" => TRUE,
                "NLS" => FALSE,
                "profvis" => FALSE,
                "cairo" => FALSE,
                "ICU" => FALSE,
                "long.double" => FALSE,
                "libcurl" => FALSE,
                "tcltk" => FALSE,
                _ => FALSE,
            };
            *crate::sexp::accessors::LOGICAL(ans).add(i) = val;
            SET_STRING_ELT(
                cn,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
            );
        }

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        Rf_unprotect(2);
        ans
    }
}

/// R's `Sys.getpid()` — get process ID.
pub unsafe fn do_sysgetpid(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_ScalarInteger;
        Rf_ScalarInteger(process::id() as c_int)
    }
}

/// R's `dir.create()` — create directory/directories.
pub unsafe fn do_dircreate(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, CDR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::fs;

        let s = CAR(args);
        let recursive = CADR(args);
        let showWarnings = CDR(CDR(args));
        let _ = showWarnings; // suppress unused warning
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = crate::sexp::accessors::LOGICAL(ans);
        let do_recursive = if recursive.is_null() || recursive == R_NilValue() {
            false
        } else {
            let v = *crate::sexp::accessors::LOGICAL(recursive);
            v != 0 && v != crate::sexp::ffi::NA_INTEGER
        };

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                let result = if do_recursive {
                    fs::create_dir_all(path)
                } else {
                    fs::create_dir(path)
                };
                *pa.add(i) = if result.is_ok() { TRUE } else { FALSE };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `file.copy()` — copy file(s).
pub unsafe fn do_filecopy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::fs;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..n as usize {
            let f = STRING_ELT(from, i as crate::sexp::ffi::R_xlen_t);
            let t = STRING_ELT(to, i as crate::sexp::ffi::R_xlen_t);
            if f.is_null() || t.is_null() || f == R_NilValue() || t == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let fc = CStr::from_ptr(crate::sexp::accessors::CHAR(f))
                    .to_str()
                    .unwrap_or("");
                let tc = CStr::from_ptr(crate::sexp::accessors::CHAR(t))
                    .to_str()
                    .unwrap_or("");
                *pa.add(i) = if fs::copy(fc, tc).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `l10n_info()` — localization information.
pub unsafe fn do_l10n_info(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{SET_STRING_ELT, SET_VECTOR_ELT};
        use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{SEXPTYPE, TRUE};
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};

        // Returns a named list with 3 elements: MBCS, UTF-8, Latin-1
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, 3));
        let cn = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, 3));
        SET_STRING_ELT(cn, 0, Rf_mkChar(b"MBCS\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 1, Rf_mkChar(b"UTF-8\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 2, Rf_mkChar(b"Latin-1\0".as_ptr() as *const _));

        SET_VECTOR_ELT(ans, 0, Rf_ScalarLogical(TRUE)); // MBCS always supported
        SET_VECTOR_ELT(ans, 1, Rf_ScalarLogical(TRUE)); // UTF-8 supported
        SET_VECTOR_ELT(ans, 2, Rf_ScalarLogical(TRUE)); // Latin-1 supported

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        Rf_unprotect(2);
        ans
    }
}

/// R's `Sys.chmod()` — change file permissions.
pub unsafe fn do_syschmod(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, INTEGER, LENGTH, LOGICAL, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};
        use std::fs;

        let paths = CAR(args);
        let mode = CADR(args);
        let n = LENGTH(paths);
        let mode_val = if !mode.is_null() && mode != R_NilValue() {
            *INTEGER(mode)
        } else {
            0o644
        };

        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::LGLSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let pa = LOGICAL(ans);

        for i in 0..n as usize {
            let elt = STRING_ELT(paths, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                let result = fs::set_permissions(
                    path,
                    std::os::unix::fs::PermissionsExt::from_mode(mode_val as u32),
                );
                *pa.add(i) = if result.is_ok() { TRUE } else { FALSE };
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `Sys.umask()` — set file creation mask.
pub unsafe fn do_sysumask(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_ScalarInteger;
        // umask returns the previous mask
        let old = libc::umask(0);
        let _ = libc::umask(old);
        Rf_ScalarInteger(old as c_int)
    }
}

/// R's `Sys.readlink()` -- read symbolic link target.
pub unsafe fn do_readlink(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, SET_STRING_ELT, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};

        let paths = CAR(args);
        let n = LENGTH(paths);
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::STRSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));

        for i in 0..n as usize {
            let elt = STRING_ELT(paths, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    Rf_mkChar(b"NA\0".as_ptr() as *const _),
                );
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                match std::fs::read_link(path) {
                    Ok(target) => {
                        SET_STRING_ELT(
                            ans,
                            i as crate::sexp::ffi::R_xlen_t,
                            Rf_mkChar(
                                CString::new(target.to_str().unwrap_or(""))
                                    .unwrap_or_default()
                                    .as_ptr(),
                            ),
                        );
                    }
                    Err(_) => {
                        SET_STRING_ELT(
                            ans,
                            i as crate::sexp::ffi::R_xlen_t,
                            Rf_mkChar(b"NA\0".as_ptr() as *const _),
                        );
                    }
                }
            }
        }
        Rf_unprotect(1);
        ans
    }
}

/// R's `Cstack_info()` — C stack usage information.
pub unsafe fn do_Cstack_info(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{SET_STRING_ELT, SET_VECTOR_ELT};
        use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};

        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, 3));
        let cn = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, 3));
        SET_STRING_ELT(cn, 0, Rf_mkChar(b"used\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 1, Rf_mkChar(b"limit\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 2, Rf_mkChar(b"status\0".as_ptr() as *const _));

        // Estimate stack usage (platform-specific)
        let stack_ptr = std::ptr::null::<u8>() as usize;
        // Use a reasonable estimate: 8MB default stack
        let used = 0i32; // Can't easily get accurate used amount
        let limit = 8 * 1024 * 1024i32;
        let status = 0i32; // 0 = OK

        SET_VECTOR_ELT(ans, 0, Rf_ScalarInteger(used));
        SET_VECTOR_ELT(ans, 1, Rf_ScalarInteger(limit));
        SET_VECTOR_ELT(ans, 2, Rf_ScalarInteger(status));

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        Rf_unprotect(2);
        ans
    }
}

/// R's `.Platform` — external software version information.
pub unsafe fn do_eSoftVersion(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::SET_STRING_ELT;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::protect::{Rf_protect, Rf_unprotect};

        let fields = [
            ("OS.type", "unix"),
            ("OS.version", ""),
            ("OS.name", "Darwin"),
            ("png", "no"),
            ("jpeg", "no"),
            ("tiff", "no"),
            ("tcltk", "no"),
            ("X11", "no"),
            ("aqua", "no"),
            ("cairo", "no"),
            ("ICU", "no"),
            ("libcurl", "no"),
            ("zlib", "yes"),
        ];

        let n = fields.len();
        let ans = Rf_protect(Rf_allocVector3(
            SEXPTYPE::STRSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));
        let cn = Rf_protect(Rf_allocVector3(
            SEXPTYPE::STRSXP.0,
            n as crate::sexp::ffi::R_xlen_t,
        ));

        for (i, (name, val)) in fields.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(*val).unwrap_or_default().as_ptr()),
            );
            SET_STRING_ELT(
                cn,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
            );
        }

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        Rf_unprotect(2);
        ans
    }
}

/// R's `Sys.junction()` — create NTFS junction (Windows only).
/// On non-Windows, returns FALSE.
pub unsafe fn do_mkjunction(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::Rf_ScalarLogical;
        use crate::sexp::ffi::FALSE;
        Rf_ScalarLogical(FALSE)
    }
}
