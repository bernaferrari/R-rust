#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Error/warning message databases (ErrorMessage/WarningMessage) and
//! gettext/ngettext/bindtextdomain support.

use super::*;

// ---------------------------------------------------------------------------
// Error/Warning message databases
// ---------------------------------------------------------------------------

/// Error codes (from Errormsg.h).
pub mod error_codes {
    pub const ERROR_NUMARGS: i32 = 1;
    pub const ERROR_ARGTYPE: i32 = 2;
    pub const ERROR_TSVEC_MISMATCH: i32 = 3;
    pub const ERROR_INCOMPAT_ARGS: i32 = 4;
    pub const ERROR_UNIMPLEMENTED: i32 = 5;
    pub const ERROR_UNKNOWN: i32 = 6;
}

/// Warning codes.
pub mod warning_codes {
    pub const WARNING_coerce_NA: i32 = 0;
    pub const WARNING_coerce_INACC: i32 = 1;
    pub const WARNING_coerce_IMAG: i32 = 2;
    pub const WARNING_UNKNOWN: i32 = 3;
}

/// ErrorMessage — look up an error message from the database and call errorcall.
/// Matches C: `void ErrorMessage(SEXP call, int which_error, ...)`
pub fn ErrorMessage(call: SEXP, which_error: c_int, format: Option<&CStr>) {
    {
        let messages = [
            "invalid number of arguments",
            "invalid argument type",
            "time-series/vector length mismatch",
            "incompatible arguments",
            "unimplemented feature in %s",
            "unknown error (report this!)",
        ];

        let idx = if which_error >= 0 && (which_error as usize) < messages.len() {
            which_error as usize
        } else {
            messages.len() - 1
        };

        // For format strings with %s, use the format argument
        let msg = if which_error == error_codes::ERROR_UNIMPLEMENTED
            && let Some(format) = format
        {
            let arg = format.to_str().unwrap_or("unknown");
            format!("unimplemented feature in {}", arg)
        } else {
            messages[idx].to_string()
        };

        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        errorcall_cstr(call, &c_msg);
    }
}

/// WarningMessage — look up a warning message from the database and call warningcall.
/// Matches C: `void WarningMessage(SEXP call, R_WARNING which_warn, ...)`
pub fn WarningMessage(call: SEXP, which_warn: c_int, format: Option<&CStr>) {
    {
        let messages = [
            "NAs introduced by coercion",
            "inaccurate integer conversion in coercion",
            "imaginary parts discarded in coercion",
            "unknown warning (report this!)",
        ];

        let idx = if which_warn >= 0 && (which_warn as usize) < messages.len() {
            which_warn as usize
        } else {
            messages.len() - 1
        };

        let c_msg = std::ffi::CString::new(messages[idx]).unwrap_or_default();
        warningcall_cstr(call, &c_msg);
    }
}

// ---------------------------------------------------------------------------
// gettext/ngettext support (simplified — no actual i18n)
// ---------------------------------------------------------------------------

/// do_gettext — R's gettext() function (simplified, no i18n).
pub fn do_gettext(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        // Simplified: just return the string as-is (no translation)
        let string = CADR(args);
        if isNull(string) != 0 || LENGTH(string) == 0 {
            return string;
        }
        string
    }
}

/// do_ngettext — R's ngettext() function (simplified, no i18n).
pub fn do_ngettext(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let n = {
            // GNU asInteger(): the staged stop.R wrapper reaches this
            // .Internal with `n` still an unforced promise.
            let raw = CAR(args);
            let value = if TYPEOF(raw) == SEXPTYPE::PROMSXP {
                crate::sexp::envir::forcePromise(raw)
            } else {
                raw
            };
            if (isInteger(value) != 0 || TYPEOF(value) == SEXPTYPE::REALSXP) && LENGTH(value) >= 1 {
                crate::mainutils::coerce::asInteger(value)
            } else {
                crate::sexp::ffi::NA_INTEGER
            }
        };
        let msg1 = {
            let v = CADR(args);
            if TYPEOF(v) == SEXPTYPE::PROMSXP {
                crate::sexp::envir::forcePromise(v)
            } else {
                v
            }
        };
        let msg2 = {
            let v = CADDR(args);
            if TYPEOF(v) == SEXPTYPE::PROMSXP {
                crate::sexp::envir::forcePromise(v)
            } else {
                v
            }
        };

        if n == crate::sexp::ffi::NA_INTEGER || n < 0 {
            errorcall_cstr(call, c"invalid 'n' argument");
        }

        // Return singular or plural form based on n
        if n == 1 { msg1 } else { msg2 }
    }
}

/// do_bindtextdomain — R's bindtextdomain() function (simplified, no i18n).
pub fn do_bindtextdomain(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let domain = CAR(args);
        let dirname = CADR(args);
        if isNull(domain) != 0 && isNull(dirname) != 0 {
            return ScalarLogical(1);
        }

        let domain_cstr = sexp_string_cstr(domain);
        let dirname_cstr = sexp_string_cstr(dirname);
        let result = bindtextdomain_impl(domain_cstr.as_deref(), dirname_cstr.as_deref());
        if result.is_null() {
            return globals::R_NilValue();
        }

        Rf_mkString(result)
    }
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn bindtextdomain_impl(domain: Option<&CStr>, dirname: Option<&CStr>) -> *mut std::os::raw::c_char {
    // SAFETY: each pointer is null or borrowed from a live `CStr` for the call.
    unsafe {
        crate::intl::bindtextdom::libintl_bindtextdomain(
            domain.map_or(ptr::null(), CStr::as_ptr),
            dirname.map_or(ptr::null(), CStr::as_ptr),
        )
    }
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn bindtextdomain_impl(domain: Option<&CStr>, dirname: Option<&CStr>) -> *mut std::os::raw::c_char {
    match (domain, dirname) {
        (Some(_), Some(dirname)) => dirname.as_ptr() as *mut std::os::raw::c_char,
        _ => ptr::null_mut(),
    }
}

fn sexp_string_cstr(value: SEXP) -> Option<std::ffi::CString> {
    if isNull(value) != 0 {
        return None;
    }
    if isString(value) == 0 || LENGTH(value) < 1 || isValidString(value) == 0 {
        return Some(std::ffi::CString::default());
    }
    let ptr = char_cstring(STRING_ELT(value, 0));
    if ptr.is_none() {
        return None;
    }
    Some(std::ffi::CString::new(ptr.as_deref().unwrap_or_default().to_bytes()).unwrap_or_default())
}
