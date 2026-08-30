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
pub unsafe fn ErrorMessage(call: SEXP, which_error: c_int, format: *const c_char) {
    unsafe {
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
        let msg = if which_error == error_codes::ERROR_UNIMPLEMENTED && !format.is_null() {
            let arg = CStr::from_ptr(format).to_str().unwrap_or("unknown");
            format!("unimplemented feature in {}", arg)
        } else {
            messages[idx].to_string()
        };

        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        errorcall(call, c_msg.as_ptr());
    }
}

/// WarningMessage — look up a warning message from the database and call warningcall.
/// Matches C: `void WarningMessage(SEXP call, R_WARNING which_warn, ...)`
pub unsafe fn WarningMessage(call: SEXP, which_warn: c_int, format: *const c_char) {
    unsafe {
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
        warningcall(call, c_msg.as_ptr());
    }
}

// ---------------------------------------------------------------------------
// gettext/ngettext support (simplified — no actual i18n)
// ---------------------------------------------------------------------------

/// do_gettext — R's gettext() function (simplified, no i18n).
pub unsafe fn do_gettext(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: just return the string as-is (no translation)
        let string = CADR(args);
        if isNull(string) != 0 || LENGTH(string) == 0 {
            return string;
        }
        string
    }
}

/// do_ngettext — R's ngettext() function (simplified, no i18n).
pub unsafe fn do_ngettext(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let n = if isInteger(CAR(args)) != 0 && LENGTH(CAR(args)) >= 1 {
            *INTEGER(CAR(args))
        } else {
            crate::sexp::ffi::NA_INTEGER
        };
        let msg1 = CADR(args);
        let msg2 = CADDR(args);

        if n == crate::sexp::ffi::NA_INTEGER || n < 0 {
            errorcall(call, b"invalid 'n' argument\x00".as_ptr() as *const c_char);
        }

        // Return singular or plural form based on n
        if n == 1 { msg1 } else { msg2 }
    }
}

/// do_bindtextdomain — R's bindtextdomain() function (simplified, no i18n).
pub unsafe fn do_bindtextdomain(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let domain = CAR(args);
        let dirname = CADR(args);
        if isNull(domain) != 0 && isNull(dirname) != 0 {
            return ScalarLogical(1);
        }

        let domain_cstr = sexp_string_cstr(domain);
        let dirname_cstr = sexp_string_cstr(dirname);
        let domain_ptr = domain_cstr
            .as_ref()
            .map_or(ptr::null(), |value| value.as_ptr());
        let dirname_ptr = dirname_cstr
            .as_ref()
            .map_or(ptr::null(), |value| value.as_ptr());

        let result = bindtextdomain_impl(domain_ptr, dirname_ptr);
        if result.is_null() {
            return globals::R_NilValue();
        }

        Rf_mkString(result)
    }
}

#[cfg(not(target_os = "android"))]
unsafe fn bindtextdomain_impl(
    domain_ptr: *const std::os::raw::c_char,
    dirname_ptr: *const std::os::raw::c_char,
) -> *mut std::os::raw::c_char {
    unsafe { crate::intl::bindtextdom::libintl_bindtextdomain(domain_ptr, dirname_ptr) }
}

#[cfg(target_os = "android")]
unsafe fn bindtextdomain_impl(
    domain_ptr: *const std::os::raw::c_char,
    dirname_ptr: *const std::os::raw::c_char,
) -> *mut std::os::raw::c_char {
    if domain_ptr.is_null() || dirname_ptr.is_null() {
        ptr::null_mut()
    } else {
        dirname_ptr as *mut std::os::raw::c_char
    }
}

unsafe fn sexp_string_cstr(value: SEXP) -> Option<std::ffi::CString> {
    unsafe {
        if isNull(value) != 0 {
            return None;
        }
        if isString(value) == 0 || LENGTH(value) < 1 || isValidString(value) == 0 {
            return Some(std::ffi::CString::default());
        }
        let ptr = CHAR(STRING_ELT(value, 0));
        if ptr.is_null() {
            return None;
        }
        Some(std::ffi::CString::new(CStr::from_ptr(ptr).to_bytes()).unwrap_or_default())
    }
}
