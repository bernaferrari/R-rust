#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
use super::*;

/// Return the current date in the standard R format.
///
/// The returned pointer is to the active session's date buffer containing a
/// NUL-terminated string like `"Wed Jun 30 21:49:08 1993"`.
///
/// This is a faithful port of the static `R_Date()` function in platform.c.
pub unsafe fn R_Date() -> *mut c_char {
    use std::time::{SystemTime, UNIX_EPOCH};

    with_required_current_instance(|instance| unsafe {
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

        let b = &mut (*instance).startup_state.date_buf;
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
    with_required_current_instance(|instance| unsafe {
        (*instance).startup_state.native_encoding.as_ptr()
    }) as *const c_char
}

/// Detect and record locale/encoding information.
///
/// This is a simplified port of `R_check_locale()` from platform.c.
/// On Unix-like systems it uses `nl_langinfo(CODESET)` to detect the encoding.
/// Since we cannot call libc, this provides a reasonable default.
pub unsafe fn R_check_locale() {
    with_required_current_instance(|instance| unsafe {
        let enc = &mut (*instance).startup_state.native_encoding;
        enc.fill(0);
        let bytes = b"UTF-8\0";
        let len = bytes.len().min(R_CODESET_MAX);
        enc[..len].copy_from_slice(&bytes[..len]);
        enc[len] = 0;

        let cs = &mut (*instance).startup_state.codeset_buf;
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

fn emit_file_show(text: &str) {
    if crate::sexp::output::is_capturing() {
        crate::sexp::output::capture_stdout(text);
    } else {
        print!("{text}");
    }
}

/// Print store bytes. A missing or non-UTF-8 key never opens a host file.
fn show_stored_file(path: &str) {
    match crate::mainutils::browser_files::read_current(path) {
        Some(bytes) => match String::from_utf8(bytes) {
            Ok(contents) => {
                emit_file_show(&contents);
                if !contents.ends_with('\n') {
                    emit_file_show("\n");
                }
                emit_file_show("\n");
            }
            Err(err) => {
                emit_file_show(&format!("Cannot open file '{path}': {err}\n\n"));
            }
        },
        None => {
            emit_file_show(&format!(
                "Cannot open file '{path}': browser file not found\n\n"
            ));
        }
    }
}

/// R's `file.show()` — display file(s) to the user.
pub unsafe fn do_fileshow(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{
            CADDDR, CADDR, CADR, CAR, CDR, INTEGER, LENGTH, STRING_ELT, TYPEOF,
        };
        use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXPTYPE};
        use crate::sexp::globals::R_NilValue;

        let files = CAR(args);
        let headers = CADR(args);
        let title = CADDR(args);
        let delete_file = CADDDR(args);
        let pager = CAR(CDR(CDR(CDR(CDR(args)))));

        if files.is_null()
            || files == R_NilValue()
            || TYPEOF(files) != SEXPTYPE::STRSXP
            || LENGTH(files) < 1
        {
            platform_error("invalid filename specification");
        }

        let n = LENGTH(files);
        if headers.is_null()
            || headers == R_NilValue()
            || TYPEOF(headers) != SEXPTYPE::STRSXP
            || LENGTH(headers) != n
        {
            platform_error("invalid 'headers' argument");
        }
        if title.is_null() || title == R_NilValue() || TYPEOF(title) != SEXPTYPE::STRSXP {
            platform_error("invalid 'title' argument");
        }
        if pager.is_null() || pager == R_NilValue() || TYPEOF(pager) != SEXPTYPE::STRSXP {
            platform_error("invalid 'pager' argument");
        }

        let delete_file = if !delete_file.is_null()
            && delete_file != R_NilValue()
            && LENGTH(delete_file) > 0
            && (TYPEOF(delete_file) == SEXPTYPE::LGLSXP || TYPEOF(delete_file) == SEXPTYPE::INTSXP)
        {
            let value = *INTEGER(delete_file);
            value != FALSE && value != NA_INTEGER
        } else {
            false
        };

        for i in 0..n {
            let file = STRING_ELT(files, i as crate::sexp::ffi::R_xlen_t);
            if is_na_charsxp(file) {
                platform_error("invalid filename specification");
            }
            let header = STRING_ELT(headers, i as crate::sexp::ffi::R_xlen_t);
            if is_na_charsxp(header) {
                platform_error("invalid 'headers' argument");
            }
        }
        if LENGTH(title) > 0 && is_na_charsxp(STRING_ELT(title, 0)) {
            platform_error("invalid 'title' argument");
        }
        if LENGTH(pager) > 0 && is_na_charsxp(STRING_ELT(pager, 0)) {
            platform_error("invalid 'pager' argument");
        }

        let browser_store = crate::mainutils::browser_files::enabled();
        for i in 0..n {
            let header = path_at(headers, i as usize).unwrap_or_default();
            if !header.is_empty() {
                crate::sexp::output::capture_stdout(&format!("{header}\n\n"));
            }
            if let Some(path) = path_at(files, i as usize) {
                if browser_store {
                    show_stored_file(&path);
                } else {
                    match std::fs::read_to_string(&path) {
                        Ok(contents) => {
                            let mut out = contents;
                            if !out.ends_with('\n') {
                                out.push('\n');
                            }
                            out.push('\n');
                            crate::sexp::output::capture_stdout(&out);
                        }
                        Err(err) => {
                            crate::sexp::output::capture_stdout(&format!(
                                "Cannot open file '{path}': {err}\n\n"
                            ));
                        }
                    }
                }
            }
        }

        if delete_file {
            for i in 0..n {
                if let Some(path) = path_at(files, i as usize) {
                    if browser_store {
                        let _ = crate::mainutils::browser_files::remove_current(&path);
                    } else {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }
        R_NilValue()
    }
}

pub(crate) fn platform_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

unsafe fn is_na_charsxp(s: SEXP) -> bool {
    unsafe { s.is_null() || (*s).sxpinfo.gp() & 1 != 0 }
}

pub(crate) unsafe fn path_at(paths: SEXP, index: usize) -> Option<String> {
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

pub(crate) unsafe fn octmode_scalar(mode: u32) -> SEXP {
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

pub(crate) unsafe fn parse_octal_mode_arg(value: SEXP) -> Option<u32> {
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
    with_required_current_instance(|instance| unsafe { (*instance).file_creation_umask & 0o777 })
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

pub(crate) unsafe fn platform_tag_name(cell: SEXP) -> Option<String> {
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
