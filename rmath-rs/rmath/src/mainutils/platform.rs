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

use crate::sexp::ffi::SEXP;
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Standalone utility: R_Date
// ---------------------------------------------------------------------------

/// Return the current date in the standard R format.
///
/// The returned pointer is to the active session's date buffer containing a
/// NUL-terminated string like `"Wed Jun 30 21:49:08 1993"`.
///
/// This is a faithful port of the static `R_Date()` function in platform.c.
pub unsafe fn R_Date() -> *mut c_char {
    use std::time::{SystemTime, UNIX_EPOCH};

    with_required_current_instance(|instance| {
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

        let b = &mut instance.startup_state.date_buf;
        b.fill(0);
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

/// Return the detected native encoding name (e.g., "UTF-8", "ASCII").
///
/// This is a port of `R_nativeEncoding()` from platform.c.
/// The encoding is initialized by `R_check_locale()`.
pub unsafe fn R_nativeEncoding() -> *const c_char {
    with_required_current_instance(|instance| instance.startup_state.native_encoding.as_ptr())
        as *const c_char
}

/// Detect and record locale/encoding information.
///
/// This is a simplified port of `R_check_locale()` from platform.c.
/// On Unix-like systems it uses `nl_langinfo(CODESET)` to detect the encoding.
/// Since we cannot call libc, this provides a reasonable default.
pub unsafe fn R_check_locale() {
    with_required_current_instance(|instance| {
        let enc = &mut instance.startup_state.native_encoding;
        enc.fill(0);
        let bytes = b"UTF-8\0";
        let len = bytes.len().min(R_CODESET_MAX);
        enc[..len].copy_from_slice(&bytes[..len]);
        enc[len] = 0;

        let cs = &mut instance.startup_state.codeset_buf;
        cs.fill(0);
        let bytes = b"UTF-8\0";
        let len = bytes.len().min(R_CODESET_MAX);
        cs[..len].copy_from_slice(&bytes[..len]);
        cs[len] = 0;
    });
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

fn platform_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

unsafe fn is_na_charsxp(s: SEXP) -> bool {
    unsafe { s.is_null() || (*s).sxpinfo.gp() & 1 != 0 }
}

unsafe fn path_at(paths: SEXP, index: usize) -> Option<String> {
    unsafe {
        let elt = crate::sexp::accessors::STRING_ELT(paths, index as crate::sexp::ffi::R_xlen_t);
        if is_na_charsxp(elt) {
            return None;
        }
        CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
            .to_str()
            .ok()
            .map(str::to_owned)
    }
}

unsafe fn octmode_scalar(mode: u32) -> SEXP {
    unsafe {
        use crate::sexp::accessors::SET_STRING_ELT;
        use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;

        let ans = Rf_ScalarInteger((mode & 0o777) as c_int);
        let _ans_guard = protect(ans);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), 1);
        let _class_guard = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"octmode".as_ptr()));
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_ClassSymbol(), class);
        ans
    }
}

unsafe fn parse_octal_mode_arg(value: SEXP) -> Option<u32> {
    unsafe {
        use crate::sexp::accessors::{INTEGER, LENGTH, REAL, TYPEOF};
        use crate::sexp::ffi::{NA_INTEGER, SEXPTYPE};
        use crate::sexp::globals::R_NilValue;

        if value.is_null() || value == R_NilValue() || LENGTH(value) == 0 {
            return None;
        }
        match TYPEOF(value) {
            t if t == SEXPTYPE::STRSXP => path_at(value, 0)
                .and_then(|text| parse_octal_mode_text(&text))
                .map(|mode| mode & 0o777),
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                let raw = *INTEGER(value);
                if raw == NA_INTEGER || raw < 0 {
                    None
                } else {
                    Some((raw as u32) & 0o777)
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let raw = *REAL(value);
                if raw.is_nan() || raw < 0.0 {
                    None
                } else {
                    Some((raw as u32) & 0o777)
                }
            }
            _ => None,
        }
    }
}

fn parse_octal_mode_text(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NA") {
        return None;
    }
    let digits = trimmed
        .strip_prefix("0o")
        .or_else(|| trimmed.strip_prefix("0O"))
        .unwrap_or(trimmed);
    u32::from_str_radix(digits, 8).ok()
}

pub(crate) fn current_file_creation_umask() -> u32 {
    with_required_current_instance(|instance| instance.file_creation_umask & 0o777)
}

#[cfg(unix)]
pub(crate) fn set_path_mode(path: &str, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o777))
}

#[cfg(not(unix))]
pub(crate) fn set_path_mode(_path: &str, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn create_file_with_session_umask(path: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let mode = 0o666 & !current_file_creation_umask();
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .and_then(|_| set_path_mode(path, mode))
}

#[cfg(not(unix))]
pub(crate) fn create_file_with_session_umask(path: &str) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
}

unsafe fn platform_tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = crate::sexp::accessors::TAG(cell);
        if tag.is_null() || tag == crate::sexp::globals::R_NilValue() {
            return None;
        }
        let pname = crate::sexp::accessors::PRINTNAME(tag);
        if pname.is_null() {
            return None;
        }
        let chars = crate::sexp::accessors::CHAR(pname);
        if chars.is_null() {
            None
        } else {
            CStr::from_ptr(chars).to_str().ok().map(str::to_owned)
        }
    }
}

fn append_file(destination: &str, source: &str) -> bool {
    let Ok(metadata) = std::fs::metadata(source) else {
        return false;
    };
    if metadata.is_dir() {
        return false;
    }

    let Ok(mut input) = std::fs::File::open(source) else {
        return false;
    };
    let Ok(mut output) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(destination)
    else {
        return false;
    };

    std::io::copy(&mut input, &mut output).is_ok()
}

/// R's `file.append(file1, file2)` — append source files in `file2` to destinations in `file1`.
pub unsafe fn do_fileappend(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADR, CAR, LENGTH, LOGICAL, TYPEOF};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};

        let file1 = CAR(args);
        let file2 = CADR(args);
        if TYPEOF(file1) != SEXPTYPE::STRSXP.as_c_int() {
            platform_error("invalid 'file1' argument");
        }
        if TYPEOF(file2) != SEXPTYPE::STRSXP.as_c_int() {
            platform_error("invalid 'file2' argument");
        }

        let n1 = LENGTH(file1) as usize;
        let n2 = LENGTH(file2) as usize;
        if n1 == 0 {
            platform_error("nothing to append to");
        }
        if n2 == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), 0);
        }
        let n = n1.max(n2);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let out = LOGICAL(ans);

        for i in 0..n {
            let ok = match (path_at(file1, i % n1), path_at(file2, i % n2)) {
                (Some(destination), Some(source)) => append_file(&destination, &source),
                _ => false,
            };
            *out.add(i) = if ok { TRUE } else { FALSE };
        }
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

        let s = CAR(args);
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for i in 0..LENGTH(s) as usize {
            let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                *pa.add(i) = if create_file_with_session_umask(path).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        ans
    }
}

/// R's `file.remove()` — remove file(s).
pub unsafe fn do_fileremove(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::fs;

        let mut paths = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let s = CAR(current);
            if s.is_null() || s == R_NilValue() {
                paths.push(None);
            } else {
                for i in 0..LENGTH(s) as usize {
                    let elt = STRING_ELT(s, i as crate::sexp::ffi::R_xlen_t);
                    if elt.is_null() || elt == R_NilValue() {
                        paths.push(None);
                    } else {
                        let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                        paths.push(Some(c.to_str().unwrap_or("").to_string()));
                    }
                }
            }
            current = CDR(current);
        }
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            paths.len() as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let pa = crate::sexp::accessors::LOGICAL(ans);

        for (i, path) in paths.iter().enumerate() {
            if let Some(path) = path {
                *pa.add(i) = if fs::remove_file(path).is_ok() {
                    TRUE
                } else {
                    FALSE
                };
            } else {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            }
        }
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
        use std::os::unix::fs::symlink;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
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
        use std::fs::hard_link;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
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
        use std::fs;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
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
        use std::fs;

        let files = CAR(args);
        let extra_cols = CADR(args);
        let _ = extra_cols;

        let n = LENGTH(files);
        // Build a named list (VECSXP) with columns: size, isdir, mode, mtime, ctime, atime, exe
        let ncols = 7i32;
        let mut guards = Vec::new();
        let ans = Rf_allocVector3(
            SEXPTYPE::VECSXP.as_c_int(),
            ncols as crate::sexp::ffi::R_xlen_t,
        );
        guards.push(protect(ans));

        let size_col = Rf_allocVector3(
            SEXPTYPE::REALSXP.as_c_int(),
            n as crate::sexp::ffi::R_xlen_t,
        );
        guards.push(protect(size_col));
        let isdir_col =
            Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        guards.push(protect(isdir_col));
        let mode_col =
            Rf_allocVector3(SEXPTYPE::INTSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        guards.push(protect(mode_col));
        let mode_class = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), 1);
        guards.push(protect(mode_class));
        SET_STRING_ELT(mode_class, 0, Rf_mkChar(c"octmode".as_ptr()));
        crate::eval::attrib_core::setAttrib(
            mode_col,
            crate::eval::attrib_core::R_ClassSymbol(),
            mode_class,
        );
        let mtime_col = Rf_allocVector3(
            SEXPTYPE::REALSXP.as_c_int(),
            n as crate::sexp::ffi::R_xlen_t,
        );
        guards.push(protect(mtime_col));
        let ctime_col = Rf_allocVector3(
            SEXPTYPE::REALSXP.as_c_int(),
            n as crate::sexp::ffi::R_xlen_t,
        );
        guards.push(protect(ctime_col));
        let atime_col = Rf_allocVector3(
            SEXPTYPE::REALSXP.as_c_int(),
            n as crate::sexp::ffi::R_xlen_t,
        );
        guards.push(protect(atime_col));
        let exe_col = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        guards.push(protect(exe_col));

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
                                (meta.mode() & 0o777) as c_int;
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
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        guards.push(protect(rn));
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

        // Set column names
        let cn = Rf_allocVector3(
            SEXPTYPE::STRSXP.as_c_int(),
            ncols as crate::sexp::ffi::R_xlen_t,
        );
        guards.push(protect(cn));
        SET_STRING_ELT(cn, 0, Rf_mkChar(b"size\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 1, Rf_mkChar(b"isdir\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 2, Rf_mkChar(b"mode\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 3, Rf_mkChar(b"mtime\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 4, Rf_mkChar(b"ctime\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 5, Rf_mkChar(b"atime\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 6, Rf_mkChar(b"exe\0".as_ptr() as *const _));
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_RowNamesSymbol(), rn);

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        guards.push(protect(class));
        SET_STRING_ELT(class, 0, Rf_mkChar(b"data.frame\0".as_ptr() as *const _));
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_ClassSymbol(), class);

        ans
    }
}

/// R's `file.size(...)` — return file sizes in bytes.
pub unsafe fn do_filesize(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, REAL, STRING_ELT};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        let files = CAR(args);
        let n = LENGTH(files);
        let ans = Rf_allocVector3(
            SEXPTYPE::REALSXP.as_c_int(),
            n as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let out = REAL(ans);
        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            *out.add(i) = if elt.is_null() || elt == R_NilValue() {
                crate::sexp::ffi::NA_REAL
            } else {
                let path = CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                    .to_str()
                    .unwrap_or("");
                std::fs::metadata(path)
                    .map(|meta| meta.len() as f64)
                    .unwrap_or(crate::sexp::ffi::NA_REAL)
            };
        }
        ans
    }
}

/// R's `file.mtime(...)` — return modification times as POSIXct.
pub unsafe fn do_filemtime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, REAL, SET_STRING_ELT, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        let files = CAR(args);
        let n = LENGTH(files);
        let ans = Rf_allocVector3(
            SEXPTYPE::REALSXP.as_c_int(),
            n as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        let out = REAL(ans);
        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            *out.add(i) = if elt.is_null() || elt == R_NilValue() {
                crate::sexp::ffi::NA_REAL
            } else {
                let path = CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                    .to_str()
                    .unwrap_or("");
                std::fs::metadata(path)
                    .ok()
                    .and_then(|meta| meta.modified().ok())
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| {
                        duration.as_secs() as f64 + duration.subsec_nanos() as f64 / 1e9
                    })
                    .unwrap_or(crate::sexp::ffi::NA_REAL)
            };
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _class_guard = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(b"POSIXct\0".as_ptr() as *const _));
        SET_STRING_ELT(class, 1, Rf_mkChar(b"POSIXt\0".as_ptr() as *const _));
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_ClassSymbol(), class);
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
        use std::path::Path;

        let s = CAR(args);
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
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
        ans
    }
}

/// R's `list.files()` — list files in directories.
pub unsafe fn do_listfiles(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, LOGICAL, SET_STRING_ELT, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        fn logical_arg(value: SEXP, default: bool) -> bool {
            unsafe {
                if value.is_null() || value == R_NilValue() || LENGTH(value) == 0 {
                    return default;
                }
                let v = *LOGICAL(value);
                v != 0 && v != crate::sexp::ffi::NA_INTEGER
            }
        }

        fn push_dir_dots(base: &str, full_names: bool, entries: &mut Vec<String>) {
            if full_names {
                entries.push(format!("{}/.", base.trim_end_matches('/')));
                entries.push(format!("{}/..", base.trim_end_matches('/')));
            } else {
                entries.push(".".to_string());
                entries.push("..".to_string());
            }
        }

        #[derive(Clone, Copy)]
        struct ListFilesOptions<'a> {
            pattern: Option<&'a str>,
            all_files: bool,
            full_names: bool,
            recursive: bool,
            ignore_case: bool,
            include_dirs: bool,
        }

        fn pattern_matches(name: &str, options: ListFilesOptions<'_>) -> bool {
            options.pattern.is_none_or(|pattern| {
                crate::mainutils::grep::ere_is_match(pattern, name, options.ignore_case)
            })
        }

        fn collect_list_files(
            base: &std::path::Path,
            relative: &std::path::Path,
            options: ListFilesOptions<'_>,
            entries: &mut Vec<String>,
        ) {
            let current = base.join(relative);
            let Ok(read_dir) = std::fs::read_dir(&current) else {
                return;
            };
            let mut children = Vec::new();
            for entry in read_dir.flatten() {
                children.push(entry);
            }
            children.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

            for entry in children {
                let name = entry.file_name().to_string_lossy().to_string();
                if !options.all_files && name.starts_with('.') {
                    continue;
                }

                let child_rel = relative.join(&name);
                let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
                let display = child_rel.to_string_lossy().replace('\\', "/");
                let returned = if options.full_names {
                    base.join(&child_rel).to_string_lossy().to_string()
                } else {
                    display.clone()
                };

                if options.recursive {
                    if is_dir {
                        if options.include_dirs && pattern_matches(&display, options) {
                            entries.push(returned);
                        }
                        collect_list_files(base, &child_rel, options, entries);
                    } else if pattern_matches(&display, options) {
                        entries.push(returned);
                    }
                } else if pattern_matches(&name, options) {
                    entries.push(returned);
                }
            }
        }

        let mut paths = R_NilValue();
        let mut pattern = R_NilValue();
        let mut all_files = R_NilValue();
        let mut full_names = R_NilValue();
        let mut recursive = R_NilValue();
        let mut ignore_case = R_NilValue();
        let mut include_dirs = R_NilValue();
        let mut no_dotdot = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match platform_tag_name(current).as_deref() {
                Some("path") => paths = value,
                Some("pattern") => pattern = value,
                Some("all.files") => all_files = value,
                Some("full.names") => full_names = value,
                Some("recursive") => recursive = value,
                Some("ignore.case") => ignore_case = value,
                Some("include.dirs") => include_dirs = value,
                Some("no..") => no_dotdot = value,
                Some(_) => {}
                None => {
                    match positional {
                        0 => paths = value,
                        1 => pattern = value,
                        2 => all_files = value,
                        3 => full_names = value,
                        4 => recursive = value,
                        5 => ignore_case = value,
                        6 => include_dirs = value,
                        7 => no_dotdot = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let pattern_string = if pattern.is_null() || pattern == R_NilValue() || LENGTH(pattern) == 0
        {
            None
        } else {
            let elt = STRING_ELT(pattern, 0);
            if elt.is_null() || elt == R_NilValue() || elt == crate::sexp::globals::R_NaString() {
                Some(String::new())
            } else {
                Some(
                    CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                        .to_str()
                        .unwrap_or("")
                        .to_string(),
                )
            }
        };
        let options = ListFilesOptions {
            pattern: pattern_string.as_deref(),
            all_files: logical_arg(all_files, false),
            full_names: logical_arg(full_names, false),
            recursive: logical_arg(recursive, false),
            ignore_case: logical_arg(ignore_case, false),
            include_dirs: logical_arg(include_dirs, false),
        };
        let omit_dotdot = logical_arg(no_dotdot, false);

        let mut entries: Vec<String> = Vec::new();
        let mut visit_path = |path: String| {
            if options.all_files && !omit_dotdot && !options.recursive && pattern_string.is_none() {
                push_dir_dots(&path, options.full_names, &mut entries);
            }
            collect_list_files(
                std::path::Path::new(&path),
                std::path::Path::new(""),
                options,
                &mut entries,
            );
        };

        if paths.is_null() || paths == R_NilValue() || LENGTH(paths) == 0 {
            visit_path(".".to_string());
        } else {
            for i in 0..LENGTH(paths) as usize {
                if let Some(path) = path_at(paths, i) {
                    visit_path(path);
                }
            }
        }

        entries.sort();
        let ans = Rf_allocVector3(
            SEXPTYPE::STRSXP.as_c_int(),
            entries.len() as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        for (i, name) in entries.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        ans
    }
}

/// R's `list.dirs()` — list directories.
pub unsafe fn do_listdirs(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, LOGICAL, SET_STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        fn logical_arg(value: SEXP, default: bool) -> bool {
            unsafe {
                if value.is_null() || value == R_NilValue() || LENGTH(value) == 0 {
                    return default;
                }
                let v = *LOGICAL(value);
                v != 0 && v != crate::sexp::ffi::NA_INTEGER
            }
        }

        fn collect_dirs(
            base: &std::path::Path,
            relative: &std::path::Path,
            full_names: bool,
            recursive: bool,
            include_self: bool,
            out: &mut Vec<String>,
        ) {
            if include_self {
                out.push(if full_names {
                    base.join(relative).to_string_lossy().to_string()
                } else {
                    relative.to_string_lossy().to_string()
                });
            }

            let current = base.join(relative);
            let Ok(read_dir) = std::fs::read_dir(&current) else {
                return;
            };
            let mut children = Vec::new();
            for entry in read_dir.flatten() {
                if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                    children.push(entry.file_name().to_string_lossy().to_string());
                }
            }
            children.sort();

            for child in children {
                let child_rel = relative.join(&child);
                if recursive {
                    collect_dirs(base, &child_rel, full_names, true, true, out);
                } else {
                    out.push(if full_names {
                        base.join(&child_rel).to_string_lossy().to_string()
                    } else {
                        child_rel.to_string_lossy().to_string()
                    });
                }
            }
        }

        let mut paths = R_NilValue();
        let mut full_names = R_NilValue();
        let mut recursive = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match platform_tag_name(current).as_deref() {
                Some("path") => paths = value,
                Some("full.names") => full_names = value,
                Some("recursive") => recursive = value,
                Some(_) => {}
                None => {
                    match positional {
                        0 => paths = value,
                        1 => full_names = value,
                        2 => recursive = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let do_full = logical_arg(full_names, true);
        let do_recursive = logical_arg(recursive, true);
        let mut entries: Vec<String> = Vec::new();

        if paths.is_null() || paths == R_NilValue() || LENGTH(paths) == 0 {
            collect_dirs(
                std::path::Path::new("."),
                std::path::Path::new(""),
                do_full,
                do_recursive,
                do_recursive,
                &mut entries,
            );
        } else {
            for i in 0..LENGTH(paths) as usize {
                if let Some(path) = path_at(paths, i) {
                    collect_dirs(
                        std::path::Path::new(&path),
                        std::path::Path::new(""),
                        do_full,
                        do_recursive,
                        do_recursive,
                        &mut entries,
                    );
                }
            }
        }

        let ans = Rf_allocVector3(
            SEXPTYPE::STRSXP.as_c_int(),
            entries.len() as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
        for (i, name) in entries.iter().enumerate() {
            SET_STRING_ELT(
                ans,
                i as crate::sexp::ffi::R_xlen_t,
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
        }
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
        use std::path::Path;

        let s = CAR(args);
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
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
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;
        use std::path::Path;

        let files = CAR(args);
        let mode_arg = CADR(args);
        let n = LENGTH(files);

        // mode: 0=exists, 1=executable, 2=writable, 4=readable
        let mut mode = 0i32;
        if !mode_arg.is_null() && mode_arg != R_NilValue() {
            mode = *INTEGER(mode_arg);
        }

        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = INTEGER(ans);

        for i in 0..n as usize {
            let elt = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                *pa.add(i) = crate::sexp::ffi::NA_INTEGER;
            } else {
                let c = CStr::from_ptr(crate::sexp::accessors::CHAR(elt));
                let path = c.to_str().unwrap_or("");
                let p = Path::new(path);
                let allowed = match mode {
                    0 => {
                        if p.exists() {
                            true
                        } else {
                            false
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
                                true
                            } else {
                                false
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            false
                        }
                    }
                    2 => {
                        if p.metadata()
                            .map(|m| !m.permissions().readonly())
                            .unwrap_or(false)
                        {
                            true
                        } else {
                            false
                        }
                    }
                    4 => {
                        if std::fs::metadata(path).is_ok() {
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                *pa.add(i) = if allowed { 0 } else { -1 };
            }
        }
        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), files);
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

        let lc = libc::localeconv();
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP, 7);
        let _ans_guard = protect(ans);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 7);
        let _names_guard = protect(names);

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
        ans
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

    use super::*;

    #[test]
    fn platform_scratch_buffers_are_session_local() {
        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            let first_date = R_Date();
            R_check_locale();
            let first_encoding = R_nativeEncoding();
            assert_eq!(CStr::from_ptr(first_encoding).to_bytes(), b"UTF-8");

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            let second_date = R_Date();
            let second_encoding_before = R_nativeEncoding();
            assert_eq!(CStr::from_ptr(second_encoding_before).to_bytes(), b"");
            R_check_locale();
            let second_encoding = R_nativeEncoding();
            assert_eq!(CStr::from_ptr(second_encoding).to_bytes(), b"UTF-8");

            assert_ne!(first_date, second_date);
            assert_ne!(first_encoding, second_encoding);

            set_current_instance(&mut first);
            assert_eq!(CStr::from_ptr(R_nativeEncoding()).to_bytes(), b"UTF-8");

            clear_current_instance();
        }
    }
}

/// R's `path.expand()` — expand file paths (~ and environment variables).
pub unsafe fn do_pathexpand(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        let s = CAR(args);
        let n = LENGTH(s);
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);

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
        ans
    }
}

/// R's `capabilities()` — query platform capabilities.
pub unsafe fn do_capabilities(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::SET_STRING_ELT;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};

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
            "Rprof",
            "profmem",
            "cairo",
            "ICU",
            "long.double",
            "libcurl",
        ];
        let n = names.len();
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _cn_guard = protect(cn);

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
                "Rprof" => FALSE,
                "profmem" => FALSE,
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
        use std::fs;

        let s = CAR(args);
        let recursive = CADR(args);
        let showWarnings = CDR(CDR(args));
        let _ = showWarnings; // suppress unused warning
        let ans = Rf_allocVector3(
            SEXPTYPE::LGLSXP.as_c_int(),
            LENGTH(s) as crate::sexp::ffi::R_xlen_t,
        );
        let _ans_guard = protect(ans);
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
                *pa.add(i) = if result
                    .and_then(|_| set_path_mode(path, 0o777 & !current_file_creation_umask()))
                    .is_ok()
                {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
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
        use std::fs;

        let from = CAR(args);
        let to = CADR(args);
        let n = LENGTH(from);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
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
        ans
    }
}

/// R's `l10n_info()` — localization information.
pub unsafe fn do_l10n_info(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{SET_STRING_ELT, SET_VECTOR_ELT};
        use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};

        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _ans_guard = protect(ans);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
        let _cn_guard = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(b"MBCS\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 1, Rf_mkChar(b"UTF-8\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 2, Rf_mkChar(b"Latin-1\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 3, Rf_mkChar(b"codeset\0".as_ptr() as *const _));

        SET_VECTOR_ELT(ans, 0, Rf_ScalarLogical(TRUE));
        SET_VECTOR_ELT(ans, 1, Rf_ScalarLogical(TRUE));
        SET_VECTOR_ELT(ans, 2, Rf_ScalarLogical(FALSE));
        SET_VECTOR_ELT(
            ans,
            3,
            crate::sexp::constructors::Rf_mkString(c"UTF-8".as_ptr()),
        );

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        ans
    }
}

/// R's `Sys.chmod()` — change file permissions.
pub unsafe fn do_syschmod(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, LENGTH, LOGICAL};
        use crate::sexp::constructors::Rf_allocVector3;
        use crate::sexp::ffi::{FALSE, SEXPTYPE, TRUE};
        use crate::sexp::globals::R_NilValue;
        use std::fs;

        let mut paths = R_NilValue();
        let mut mode = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match platform_tag_name(current).as_deref() {
                Some("paths") => paths = value,
                Some("mode") => mode = value,
                Some("use_umask") => {}
                Some(_) => {}
                None => {
                    match positional {
                        0 => paths = value,
                        1 => mode = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let n = LENGTH(paths);
        let mode_val = parse_octal_mode_arg(mode).unwrap_or(0o777);

        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let pa = LOGICAL(ans);

        for i in 0..n as usize {
            if let Some(path) = path_at(paths, i) {
                let result = fs::set_permissions(
                    path,
                    std::os::unix::fs::PermissionsExt::from_mode(mode_val as u32),
                );
                *pa.add(i) = if result.is_ok() { TRUE } else { FALSE };
            } else {
                *pa.add(i) = FALSE;
            }
        }
        ans
    }
}

/// R's `Sys.umask()` — set file creation mask.
pub unsafe fn do_sysumask(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR};
        use crate::sexp::globals::R_NilValue;

        let mut mode = R_NilValue();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match platform_tag_name(current).as_deref() {
                Some("mode") | None => {
                    if mode == R_NilValue() {
                        mode = value;
                    }
                }
                Some(_) => {}
            }
            current = CDR(current);
        }

        let old = with_required_current_instance(|instance| {
            let old = instance.file_creation_umask & 0o777;
            if let Some(new_mode) = parse_octal_mode_arg(mode) {
                instance.file_creation_umask = new_mode & 0o777;
            }
            old
        });
        octmode_scalar(old)
    }
}

/// R's `Sys.readlink()` -- read symbolic link target.
pub unsafe fn do_readlink(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, LENGTH, SET_STRING_ELT, STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;

        let paths = CAR(args);
        let n = LENGTH(paths);
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);

        for i in 0..n as usize {
            let elt = STRING_ELT(paths, i as crate::sexp::ffi::R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                SET_STRING_ELT(
                    ans,
                    i as crate::sexp::ffi::R_xlen_t,
                    crate::sexp::globals::R_NaString(),
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
                        let value = if std::fs::metadata(path).is_ok() {
                            Rf_mkChar(c"".as_ptr())
                        } else {
                            crate::sexp::globals::R_NaString()
                        };
                        SET_STRING_ELT(ans, i as crate::sexp::ffi::R_xlen_t, value);
                    }
                }
            }
        }
        ans
    }
}

/// R's `Cstack_info()` — C stack usage information.
pub unsafe fn do_Cstack_info(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{INTEGER, SET_STRING_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;

        let ans = Rf_allocVector3(SEXPTYPE::INTSXP, 4);
        let _ans_guard = protect(ans);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
        let _cn_guard = protect(cn);
        SET_STRING_ELT(cn, 0, Rf_mkChar(b"size\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 1, Rf_mkChar(b"current\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 2, Rf_mkChar(b"direction\0".as_ptr() as *const _));
        SET_STRING_ELT(cn, 3, Rf_mkChar(b"eval_depth\0".as_ptr() as *const _));

        let values = INTEGER(ans);
        *values.add(0) = 8 * 1024 * 1024;
        *values.add(1) = 0;
        *values.add(2) = 1;
        *values.add(3) = 0;

        crate::eval::attrib_core::setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), cn);
        ans
    }
}

/// R's `extSoftVersion()` — external software version information.
pub unsafe fn do_eSoftVersion(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::SET_STRING_ELT;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::SEXPTYPE;

        let fields = [
            ("zlib", "yes"),
            ("bzlib", "yes"),
            ("xz", "yes"),
            ("libdeflate", ""),
            ("PCRE", "yes"),
            ("ICU", "yes"),
            ("TRE", "yes"),
            ("iconv", "yes"),
            ("readline", "yes"),
            ("BLAS", ""),
        ];

        let n = fields.len();
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _ans_guard = protect(ans);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP.as_c_int(), n as crate::sexp::ffi::R_xlen_t);
        let _cn_guard = protect(cn);

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
