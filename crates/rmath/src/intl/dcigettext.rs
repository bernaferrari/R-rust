#![warn(unused_unsafe)]
//! Port of dcigettext.c -- Core gettext lookup logic.
//!
//! This is the main implementation file for GNU gettext. It implements
//! `libintl_dcigettext()` which looks up a message in the message catalog
//! for the given domain, locale, and category.
//!
//! The C version is ~1800 lines with complex logic for:
//! - Locale environment variable handling
//! - Locale aliasing
//! - Plural form selection
//! - Message catalog lookup via hash tables
//! - Charset conversion
//!
//! For the standalone Rust port, we provide a reasonable implementation
//! that covers the core lookup logic with stubs for the most complex parts.

#![allow(non_snake_case)]

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_ulong};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Allocate a NUL-terminated copy of a `CString` using `std::alloc`.
fn alloc_cstring(s: &CString) -> *mut c_char {
    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    unsafe { c_strdup(s.as_ptr()) }
}

/// Check if two C strings are equal.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn c_streq(a: *const c_char, b: *const c_char) -> bool {
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    unsafe { cstr(a) == cstr(b) }
}

/// Get the current locale for a category from the environment.
///
/// Follows the POSIX precedence: LC_ALL, LC_xxx, LANG.
fn gl_locale_name_posix(category: c_int) -> *mut c_char {
    // Try LC_ALL first.
    if let Ok(val) = std::env::var("LC_ALL")
        && let Ok(cstr) = CString::new(val.as_str())
    {
        let p = alloc_cstring(&cstr);
        if !p.is_null() {
            return p;
        }
    }

    let cat_name = match category {
        0 => "LC_CTYPE",
        1 => "LC_NUMERIC",
        2 => "LC_TIME",
        3 => "LC_COLLATE",
        4 => "LC_MONETARY",
        5 => "LC_MESSAGES",
        6 => "LC_ALL",
        _ => "LC_ALL",
    };

    if let Ok(val) = std::env::var(cat_name)
        && let Ok(cstr) = CString::new(val.as_str())
    {
        let p = alloc_cstring(&cstr);
        if !p.is_null() {
            return p;
        }
    }

    // Try LANG.
    if let Ok(val) = std::env::var("LANG")
        && let Ok(cstr) = CString::new(val.as_str())
    {
        let p = alloc_cstring(&cstr);
        if !p.is_null() {
            return p;
        }
    }

    // Default to "C".
    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    unsafe { c_strdup(b"C\0".as_ptr() as *const c_char) }
}

/// Get the locale name, trying various sources.
fn gl_locale_name(category: c_int, _categoryname: *const c_char) -> *mut c_char {
    // Try the platform-specific method first.
    let platform_locale = super::localename::_nl_locale_name(category) as *mut c_char;
    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    if !platform_locale.is_null()
        && !unsafe { c_streq(platform_locale, b"C\0".as_ptr() as *const c_char) }
    {
        return platform_locale;
    }
    if !platform_locale.is_null() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { c_free(platform_locale) };
    }

    // Try POSIX environment variables.
    let posix_locale = gl_locale_name_posix(category);
    if !posix_locale.is_null() {
        return posix_locale;
    }

    // Try LANGUAGE environment variable (for gettext).
    if let Ok(val) = std::env::var("LANGUAGE")
        && let Ok(cstr) = CString::new(val.as_str())
    {
        let p = alloc_cstring(&cstr);
        if !p.is_null() {
            return p;
        }
    }

    // Fall back to "C".
    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    unsafe { c_strdup(b"C\0".as_ptr() as *const c_char) }
}

/// Get the language preferences (for LANGUAGE env var).
fn gl_locale_name_language_pref() -> *mut c_char {
    let prefs = super::langprefs::_nl_language_preferences_default();
    if !prefs.is_null() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        return unsafe { c_strdup(prefs) };
    }

    // Try LANGUAGE env var.
    if let Ok(val) = std::env::var("LANGUAGE")
        && let Ok(cstr) = CString::new(val.as_str())
    {
        let p = alloc_cstring(&cstr);
        if !p.is_null() {
            return p;
        }
    }

    ptr::null_mut()
}

/// Split a colon-separated locale list and look up messages for each.
///
/// This implements the LANGUAGE variable support where multiple locale
/// names can be specified, separated by colons.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn lookup_in_language_list(
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
    domainname: *const c_char,
    category: c_int,
    dirname: *const c_char,
    language_list: *const c_char,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(list) = (unsafe { cstr(language_list) }) else {
        return ptr::null_mut();
    };
    let list = list.to_bytes();
    let mut start = 0;

    while start < list.len() {
        // Find the next colon or end of string.
        let end = list[start..]
            .iter()
            .position(|&b| b == b':')
            .map(|p| start + p)
            .unwrap_or(list.len());

        if end > start {
            // Create a NUL-terminated copy of the locale name.
            let mut locale_bytes = list[start..end].to_vec();
            locale_bytes.push(0);
            let locale = locale_bytes.as_mut_ptr() as *mut c_char;

            // Try to find the message in this locale.
            // SAFETY: forwarded raw pointers satisfy `dcigettext_internal`'s contract per this function's own `# Safety` requirements.
            let result = unsafe {
                dcigettext_internal(msgid, msgid_plural, n, domainname, category, dirname, locale)
            };
            // SAFETY: forwarded raw pointers satisfy `c_streq`'s contract per this function's own `# Safety` requirements.
            if !result.is_null() && !unsafe { c_streq(result, msgid) } {
                return result;
            }
        }

        start = end + 1;
    }

    ptr::null_mut()
}

/// Internal lookup function for a single locale.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn dcigettext_internal(
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
    domainname: *const c_char,
    _category: c_int,
    dirname: *const c_char,
    locale: *mut c_char,
) -> *mut c_char {
    if msgid.is_null() {
        return ptr::null_mut();
    }

    // Use the default domain if none specified.
    let effective_domain = if !domainname.is_null() {
        domainname
    } else {
        with_intl_runtime(|intl| intl.current_default_domain)
    };

    if effective_domain.is_null() {
        return msgid as *mut c_char;
    }

    // Use the default dirname if none specified.
    let effective_dirname = if dirname.is_null() {
        _nl_default_dirname.as_ptr()
    } else {
        dirname
    };

    // Find the message catalog for this domain and locale.
    let domain_file =
        // SAFETY: forwarded raw pointers satisfy `_nl_find_domain`'s contract per this function's own `# Safety` requirements.
        unsafe { super::finddomain::_nl_find_domain(effective_dirname, locale, effective_domain) };

    // SAFETY: `_nl_find_domain` returns a live node or null. Populated
    // nodes store a `Catalog` from the loader, valid for this lookup.
    let catalog = unsafe {
        ptr_ref(domain_file)
            .and_then(|file| ptr_ref(file.data.cast::<super::loadmsgcat::Catalog>()))
    };
    // SAFETY: `msgid` is non-null and NUL-terminated for the call.
    let (Some(catalog), Some(id)) = (catalog, unsafe { cstr(msgid) }) else {
        return msgid as *mut c_char;
    };
    catalog
        .lookup(id, (!msgid_plural.is_null()).then_some(n))
        .map_or(msgid as *mut c_char, |translation| {
            translation.as_ptr().cast_mut()
        })
}

// ---------------------------------------------------------------------------
// Public API: libintl_dcigettext
// ---------------------------------------------------------------------------

/// Core gettext message lookup.
///
/// Looks up MSGID in the DOMAINNAME message catalog for the given CATEGORY
/// locale. If MSGID_PLURAL is non-null, performs plural form lookup.
///
/// This is the main entry point called by `dcgettext()` and `dcngettext()`.
///
/// # Safety
/// `msgid` must be a valid pointer to a NUL-terminated C string.
/// Other string pointers may be NULL.
pub unsafe fn libintl_dcigettext(
    domainname: *const c_char,
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
    category: c_int,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(msgid_cstr) = (unsafe { cstr(msgid) }) else {
        return ptr::null_mut();
    };

    // Empty msgid -> return it as-is.
    if msgid_cstr.to_bytes().is_empty() {
        return msgid as *mut c_char;
    }

    // Determine the effective domain.
    let effective_domain = if !domainname.is_null() {
        domainname
    } else {
        with_intl_runtime(|intl| intl.current_default_domain)
    };

    if effective_domain.is_null() {
        return msgid as *mut c_char;
    }

    // Get the dirname binding for this domain, or the default directory.
    // SAFETY: `effective_domain` is non-null and NUL-terminated for the call.
    let dirname = super::bindtextdom::bind_domain(unsafe { cstr(effective_domain) }, None);

    // Get the locale.
    let mut locale: *mut c_char = ptr::null_mut();

    // Try LANGUAGE environment variable first (gettext convention).
    let lang_pref = gl_locale_name_language_pref();
    if !lang_pref.is_null() {
        locale = lang_pref;
    }

    if locale.is_null() {
        locale = gl_locale_name(category, b"LC_MESSAGES\0".as_ptr() as *const c_char);
    }

    if locale.is_null() {
        return msgid as *mut c_char;
    }

    // SAFETY: forwarded raw pointers satisfy `lookup_in_language_list`'s contract per this function's own `# Safety` requirements.
    let result = unsafe {
        lookup_in_language_list(
            msgid,
            msgid_plural,
            n,
            effective_domain,
            category,
            dirname
                .as_deref()
                .map_or(_nl_default_dirname.as_ptr(), std::ffi::CStr::as_ptr),
            locale,
        )
    };

    // Free the locale string allocated via `c_strdup` above.
    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    unsafe { c_free(locale) };

    if result.is_null() {
        msgid as *mut c_char
    } else {
        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dcigettext_null_msgid() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result =
            unsafe { libintl_dcigettext(ptr::null(), ptr::null(), ptr::null(), 1, LC_MESSAGES) };
        assert!(result.is_null());
    }

    #[test]
    fn test_dcigettext_empty_msgid() {
        let msgid = b"\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { libintl_dcigettext(ptr::null(), msgid, ptr::null(), 1, LC_MESSAGES) };
        assert_eq!(result, msgid as *mut c_char);
    }

    #[test]
    fn test_dcigettext_returns_msgid() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = b"hello world\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe {
            libintl_dcigettext(
                b"nonexistent_domain\0" as *const u8 as *const c_char,
                msgid,
                ptr::null(),
                1,
                LC_MESSAGES,
            )
        };
        // With no catalog loaded, should return msgid.
        assert_eq!(result, msgid as *mut c_char);
    }

    #[test]
    fn test_dcigettext_plural() {
        let _session = crate::sexp::session::RSession::new();

        let msgid1 = b"file\0" as *const u8 as *const c_char;
        let msgid2 = b"files\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { libintl_dcigettext(ptr::null(), msgid1, msgid2, 1, LC_MESSAGES) };
        // With no catalog loaded, should return msgid1 for n=1.
        assert_eq!(result, msgid1 as *mut c_char);
    }

    #[test]
    fn test_c_streq() {
        let a = b"hello\0".as_ptr() as *const c_char;
        let b = b"hello\0".as_ptr() as *const c_char;
        let c = b"world\0".as_ptr() as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert!(unsafe { c_streq(a, b) });
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert!(!unsafe { c_streq(a, c) });
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert!(unsafe { c_streq(ptr::null(), ptr::null()) });
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert!(!unsafe { c_streq(a, ptr::null()) });
    }
}
