#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/localecharset.c — locale to charset mapping.
//!
//! Provides locale2charset() which maps locale strings to encoding names.
//! On macOS, all real locales without encoding are UTF-8.

use std::ffi::CStr;

use crate::sexp::instance::with_required_current_instance;

// ---------------------------------------------------------------------------
// Known encoding mappings (from R's `known[]` table)
// ---------------------------------------------------------------------------

/// Mapping from lowercase encoding name to canonical NUL-terminated name.
/// All values include a trailing NUL byte so they can be returned as C strings.
static KNOWN: &[(&[u8], &[u8])] = &[
    (b"iso88591", b"ISO8859-1\0"),
    (b"iso88592", b"ISO8859-2\0"),
    (b"iso88593", b"ISO8859-3\0"),
    (b"iso88596", b"ISO8859-6\0"),
    (b"iso88597", b"ISO8859-7\0"),
    (b"iso88598", b"ISO8859-8\0"),
    (b"iso88599", b"ISO8859-9\0"),
    (b"iso885910", b"ISO8859-10\0"),
    (b"iso885913", b"ISO8859-13\0"),
    (b"iso885914", b"ISO8859-14\0"),
    (b"iso885915", b"ISO8859-15\0"),
    (b"cp1251", b"CP1251\0"),
    (b"cp1255", b"CP1255\0"),
    (b"eucjp", b"EUC-JP\0"),
    (b"euckr", b"EUC-KR\0"),
    (b"euctw", b"EUC-TW\0"),
    (b"georgianps", b"GEORGIAN-PS\0"),
    (b"koi8u", b"KOI8-U\0"),
    (b"tcvn", b"TCVN\0"),
    (b"big5", b"BIG5\0"),
    (b"gb2312", b"GB2312\0"),
    (b"gb18030", b"GB18030\0"),
    (b"gbk", b"GBK\0"),
    (b"tis-620", b"TIS-620\0"),
    (b"sjis", b"SHIFT_JIS\0"),
    (b"euccn", b"GB2312\0"),
    (b"big5-hkscs", b"BIG5-HKSCS\0"),
    // macOS-specific entries
    (b"iso8859-1", b"ISO8859-1\0"),
    (b"iso8859-2", b"ISO8859-2\0"),
    (b"iso8859-4", b"ISO8859-4\0"),
    (b"iso8859-7", b"ISO8859-7\0"),
    (b"iso8859-9", b"ISO8859-9\0"),
    (b"iso8859-13", b"ISO8859-13\0"),
    (b"iso8859-15", b"ISO8859-15\0"),
    (b"koi8-u", b"KOI8-U\0"),
    (b"koi8-r", b"KOI8-R\0"),
    (b"pt154", b"PT154\0"),
    (b"us-ascii", b"ASCII\0"),
    (b"armscii-8", b"ARMSCII-8\0"),
    (b"iscii-dev", b"ISCII-DEV\0"),
    (b"big5hkscs", b"BIG5-HKSCS\0"),
];

// ---------------------------------------------------------------------------
// locale2charset — the main function
// ---------------------------------------------------------------------------

/// Map a locale string to a character encoding name.
///
/// This is the equivalent of R's `locale2charset()` from localecharset.c.
/// Note: the C-visible `locale2charset` symbol is exported from
/// `cport::localecharset`; this is the module-private counterpart.
///
/// # Arguments
/// * `locale` - The locale string (e.g., "en_US.UTF-8"). If NULL or "NULL",
///   uses the current locale from setlocale.
///
/// # Returns
/// * "ASCII" for C/POSIX locales
/// * "UTF-8" for macOS locales without encoding part
/// * The appropriate encoding name otherwise
pub unsafe fn locale2charset(locale: *const std::os::raw::c_char) -> *const std::os::raw::c_char {
    unsafe {
        let locale_str = if locale.is_null() || {
            let s = CStr::from_ptr(locale).to_str().unwrap_or("");
            s == "NULL"
        } {
            // Get current locale
            match CStr::from_ptr(libc::setlocale(libc::LC_CTYPE, std::ptr::null())).to_str() {
                Ok(s) => s,
                Err(_) => return b"ASCII\0".as_ptr() as *const std::os::raw::c_char,
            }
        } else {
            match CStr::from_ptr(locale).to_str() {
                Ok(s) => s,
                Err(_) => return b"ASCII\0".as_ptr() as *const std::os::raw::c_char,
            }
        };

        if locale_str.is_empty() || locale_str == "C" || locale_str == "POSIX" {
            return b"ASCII\0".as_ptr() as *const std::os::raw::c_char;
        }

        // Separate language_locale.encoding
        let (la_loc, enc) = if let Some(dot_pos) = locale_str.rfind('.') {
            let (la, en) = locale_str.split_at(dot_pos);
            (la, &en[1..])
        } else {
            // No encoding part — on macOS, this means UTF-8
            return b"UTF-8\0".as_ptr() as *const std::os::raw::c_char;
        };

        // Check for UTF-8 variants
        if enc.eq_ignore_ascii_case("UTF-8") || enc.eq_ignore_ascii_case("UTF8") {
            return b"UTF-8\0".as_ptr() as *const std::os::raw::c_char;
        }

        if enc.is_empty() {
            return b"UTF-8\0".as_ptr() as *const std::os::raw::c_char;
        }

        // Look up encoding in known table
        let enc_lower: Vec<u8> = enc.bytes().map(|b| b.to_ascii_lowercase()).collect();
        for (name, value) in KNOWN.iter() {
            if enc_lower == *name {
                return value.as_ptr() as *const std::os::raw::c_char;
            }
        }

        // Check for cp- prefix
        if enc_lower.starts_with(b"cp-") {
            let cp_num = &enc[3..];
            let result = format!("CP{}", cp_num);
            return with_required_current_instance(|instance| {
                let buf = &mut instance.startup_state.locale_charset_buf;
                buf.fill(0);
                let bytes = result.as_bytes();
                let len = bytes.len().min(buf.len() - 1);
                buf[..len].copy_from_slice(&bytes[..len]);
                buf.as_ptr() as *const std::os::raw::c_char
            });
        }

        // Fallback for euc encoding based on language
        if enc_lower == b"euc" {
            if la_loc.starts_with("ja") {
                return b"EUC-JP\0".as_ptr() as *const std::os::raw::c_char;
            } else if la_loc.starts_with("ko") {
                return b"EUC-KR\0".as_ptr() as *const std::os::raw::c_char;
            } else if la_loc.starts_with("zh") {
                return b"GB2312\0".as_ptr() as *const std::os::raw::c_char;
            }
        }

        // Default fallback
        b"ASCII\0".as_ptr() as *const std::os::raw::c_char
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

    #[test]
    fn test_c_locale() {
        unsafe {
            let c_locale = c"C";
            let result = CStr::from_ptr(locale2charset(c_locale.as_ptr()));
            assert_eq!(result.to_str().unwrap_or(""), "ASCII");
        }
    }

    #[test]
    fn test_posix_locale() {
        unsafe {
            let locale = c"POSIX";
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap_or(""), "ASCII");
        }
    }

    #[test]
    fn test_utf8_locale() {
        unsafe {
            let locale = c"en_US.UTF-8";
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap_or(""), "UTF-8");
        }
    }

    #[test]
    fn test_macos_locale_no_encoding() {
        unsafe {
            let locale = c"en_US";
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap_or(""), "UTF-8");
        }
    }

    #[test]
    fn test_null_locale() {
        unsafe {
            let result = CStr::from_ptr(locale2charset(std::ptr::null()));
            assert_eq!(result.to_str().unwrap_or(""), "ASCII");
        }
    }

    #[test]
    fn test_known_encoding_iso88591() {
        unsafe {
            let locale = c"en_US.ISO8859-1";
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap_or(""), "ISO8859-1");
        }
    }

    #[test]
    fn test_cp_charset_buffer_is_session_local() {
        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            let first_locale = c"en_US.cp-1252";
            let first_ptr = locale2charset(first_locale.as_ptr());
            assert_eq!(CStr::from_ptr(first_ptr).to_str().unwrap_or(""), "CP1252");

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            let second_locale = c"en_US.cp-932";
            let second_ptr = locale2charset(second_locale.as_ptr());
            assert_eq!(CStr::from_ptr(second_ptr).to_str().unwrap_or(""), "CP932");
            assert_ne!(first_ptr, second_ptr);

            set_current_instance(&mut first);
            assert_eq!(CStr::from_ptr(first_ptr).to_str().unwrap_or(""), "CP1252");

            clear_current_instance();
        }
    }
}
