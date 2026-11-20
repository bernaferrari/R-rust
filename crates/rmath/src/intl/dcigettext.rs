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

use std::alloc::Layout;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_ulong};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Allocate a NUL-terminated copy of a `CString` using `std::alloc`.
fn alloc_cstring(s: &CString) -> *mut c_char {
    let bytes = s.as_bytes_with_nul();
    let layout = Layout::from_size_align(bytes.len(), 1).unwrap_or_else(|_| Layout::new::<u8>());
    // Allocate `bytes.len()` bytes and copy the string, NUL included.
    unsafe {
        let p = std::alloc::alloc(layout);
        if !p.is_null() {
            ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        }
        p as *mut c_char
    }
}

/// Check if two C strings are equal.
fn c_streq(a: *const c_char, b: *const c_char) -> bool {
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    cstr(a) == cstr(b)
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
    c_strdup(b"C\0".as_ptr() as *const c_char)
}

/// Get the locale name, trying various sources.
fn gl_locale_name(category: c_int, _categoryname: *const c_char) -> *mut c_char {
    // Try the platform-specific method first.
    let platform_locale = super::localename::_nl_locale_name(category) as *mut c_char;
    if !platform_locale.is_null() && !c_streq(platform_locale, b"C\0".as_ptr() as *const c_char) {
        return platform_locale;
    }
    if !platform_locale.is_null() {
        c_free(platform_locale);
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
    c_strdup(b"C\0".as_ptr() as *const c_char)
}

/// Get the language preferences (for LANGUAGE env var).
fn gl_locale_name_language_pref() -> *mut c_char {
    let prefs = super::langprefs::_nl_language_preferences_default();
    if !prefs.is_null() {
        return c_strdup(prefs);
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
fn lookup_in_language_list(
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
    domainname: *const c_char,
    category: c_int,
    binding: *mut binding,
    dirname: *const c_char,
    language_list: *const c_char,
) -> *mut c_char {
    let Some(list) = cstr(language_list) else {
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
            let result = dcigettext_internal(
                msgid,
                msgid_plural,
                n,
                domainname,
                category,
                binding,
                dirname,
                locale,
            );
            if !result.is_null() && !c_streq(result, msgid) {
                return result;
            }
        }

        start = end + 1;
    }

    ptr::null_mut()
}

/// Internal lookup function for a single locale.
fn dcigettext_internal(
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
    domainname: *const c_char,
    _category: c_int,
    binding: *mut binding,
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
    let effective_dirname = if !dirname.is_null() {
        dirname
    } else {
        ptr_ref(binding)
            .map(|b| b.dirname as *const c_char)
            .filter(|d| !d.is_null())
            .unwrap_or(_nl_default_dirname.as_ptr())
    };

    // Find the message catalog for this domain and locale.
    let domain_file = super::finddomain::_nl_find_domain(
        effective_dirname,
        locale,
        effective_domain,
        binding,
    );

    let domain_data = ptr_ref(domain_file)
        .map(|f| f.data as *const loaded_domain)
        .filter(|d| !d.is_null());
    let Some(domain_data) = domain_data else {
        return msgid as *mut c_char;
    };

    // Look up the message in the catalog.
    let translation = lookup_message_in_domain(domain_data, msgid, msgid_plural, n);

    if translation.is_null() {
        msgid as *mut c_char
    } else {
        translation
    }
}

/// Look up a message in a loaded domain.
///
/// Uses the hash table for fast lookup, falling back to linear search.
fn lookup_message_in_domain(
    domain: *const loaded_domain,
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
) -> *mut c_char {
    let Some(dom) = ptr_ref(domain) else {
        return ptr::null_mut();
    };
    let Some(msgid_cstr) = cstr(msgid) else {
        return ptr::null_mut();
    };

    let nstrings = dom.nstrings;
    if nstrings == 0 {
        return ptr::null_mut();
    }

    let is_plural = !msgid_plural.is_null();
    let must_swap = dom.must_swap;
    let data = dom.data;
    let orig_tab = dom.orig_tab;
    let trans_tab = dom.trans_tab;

    // Read one table entry; indexes are bounded by `nstrings`.
    let table_entry = |tab: *const string_desc, i: usize| -> (nls_uint32, nls_uint32) {
        // Read the `string_desc` at index `i` in the catalog's table.
        let entry = unsafe { &*tab.add(i) };
        let length = if must_swap != 0 {
            SWAP(entry.length)
        } else {
            entry.length
        };
        let offset = if must_swap != 0 {
            SWAP(entry.offset)
        } else {
            entry.offset
        };
        (length, offset)
    };

    // Use hash table for lookup if available.
    if dom.hash_size > 0 && !dom.hash_tab.is_null() {
        let hash_tab = dom.hash_tab;
        let hash_size = dom.hash_size;
        let must_swap_hash_tab = dom.must_swap_hash_tab;
        let hash_val = super::hash_string::__hash_string(msgid);
        let mut idx = (hash_val as nls_uint32) % hash_size;
        let incr = 1u32 + (hash_val as nls_uint32) % (hash_size - 1);

        for _ in 0..hash_size {
            // Read the hash entry at `idx` from the `hash_size`-entry table.
            let hash_entry = unsafe {
                let val = ptr::read_unaligned(hash_tab.add(idx as usize));
                if must_swap_hash_tab != 0 {
                    SWAP(val)
                } else {
                    val
                }
            };

            if hash_entry == 0 {
                // Empty slot, not found.
                break;
            }

            let string_idx = hash_entry - 1;
            if string_idx < nstrings {
                let (_orig_length, orig_offset) = table_entry(orig_tab, string_idx as usize);
                // Point into the catalog string blob at `orig_offset`.
                let orig_str = unsafe { data.add(orig_offset as usize) };
                if cstr(orig_str) == Some(msgid_cstr) {
                    // Found! Get the translation.
                    let (trans_length, trans_offset) = table_entry(trans_tab, string_idx as usize);

                    if trans_length == 0 {
                        return ptr::null_mut();
                    }

                    // Point into the catalog string blob at `trans_offset`.
                    let trans_str = unsafe { data.add(trans_offset as usize) };

                    if is_plural {
                        // For plural forms, find the correct form.
                        let plural_form = super::plural_exp::plural_eval(dom.plural, n);
                        if plural_form == 0 {
                            // First form is at trans_str, separated by NUL from subsequent forms.
                            return trans_str as *mut c_char;
                        }
                        // Walk the NUL-separated plural forms within the
                        // translation string.
                        unsafe {
                            let mut p = trans_str;
                            let mut remaining = plural_form as usize;
                            while remaining > 0 && *p != 0 {
                                // Skip to next NUL.
                                while *p != 0 {
                                    p = p.add(1);
                                }
                                p = p.add(1);
                                remaining -= 1;
                            }
                            if *p != 0 {
                                return p as *mut c_char;
                            }
                        }
                        // Not enough plural forms, return the last one.
                        return msgid as *mut c_char;
                    }
                    return trans_str as *mut c_char;
                }
            }

            idx = (idx + incr) % hash_size;
        }
    } else {
        // Linear search through the original strings.
        for i in 0..nstrings as usize {
            let (_orig_length, orig_offset) = table_entry(orig_tab, i);
            // Point into the catalog string blob at `orig_offset`.
            let orig_str = unsafe { data.add(orig_offset as usize) };

            if cstr(orig_str) == Some(msgid_cstr) {
                let (trans_length, trans_offset) = table_entry(trans_tab, i);

                if trans_length == 0 {
                    return ptr::null_mut();
                }

                // Point into the catalog string blob at `trans_offset`.
                return unsafe { data.add(trans_offset as usize) as *mut c_char };
            }
        }
    }

    ptr::null_mut()
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
pub fn libintl_dcigettext(
    domainname: *const c_char,
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
    category: c_int,
) -> *mut c_char {
    let Some(msgid_cstr) = cstr(msgid) else {
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

    // Find the binding for this domain.
    let mut binding: *mut binding = with_intl_runtime(|intl| intl.domain_bindings);
    while let Some(b) = ptr_ref(binding) {
        if c_streq(b.domainname.as_ptr(), effective_domain) {
            break;
        }
        binding = b.next;
    }

    // Get the dirname from the binding.
    let dirname = ptr_ref(binding)
        .map(|b| b.dirname as *const c_char)
        .filter(|d| !d.is_null())
        .unwrap_or(_nl_default_dirname.as_ptr());

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

    let result = lookup_in_language_list(
        msgid,
        msgid_plural,
        n,
        effective_domain,
        category,
        binding,
        dirname,
        locale,
    );

    // Free locale string if we allocated it.
    if let Some(loc) = cstr(locale) {
        let len = loc.to_bytes().len() + 1;
        let layout = Layout::from_size_align(len, 1).unwrap_or_else(|_| Layout::new::<u8>());
        // Free the `len`-byte locale string allocated above.
        unsafe { std::alloc::dealloc(locale as *mut u8, layout) };
    }

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
        let result = libintl_dcigettext(ptr::null(), ptr::null(), ptr::null(), 1, LC_MESSAGES);
        assert!(result.is_null());
    }

    #[test]
    fn test_dcigettext_empty_msgid() {
        let msgid = b"\0" as *const u8 as *const c_char;
        let result = libintl_dcigettext(ptr::null(), msgid, ptr::null(), 1, LC_MESSAGES);
        assert_eq!(result, msgid as *mut c_char);
    }

    #[test]
    fn test_dcigettext_returns_msgid() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = b"hello world\0" as *const u8 as *const c_char;
        let result = libintl_dcigettext(
            b"nonexistent_domain\0" as *const u8 as *const c_char,
            msgid,
            ptr::null(),
            1,
            LC_MESSAGES,
        );
        // With no catalog loaded, should return msgid.
        assert_eq!(result, msgid as *mut c_char);
    }

    #[test]
    fn test_dcigettext_plural() {
        let _session = crate::sexp::session::RSession::new();

        let msgid1 = b"file\0" as *const u8 as *const c_char;
        let msgid2 = b"files\0" as *const u8 as *const c_char;
        let result = libintl_dcigettext(ptr::null(), msgid1, msgid2, 1, LC_MESSAGES);
        // With no catalog loaded, should return msgid1 for n=1.
        assert_eq!(result, msgid1 as *mut c_char);
    }

    #[test]
    fn test_c_streq() {
        let a = b"hello\0".as_ptr() as *const c_char;
        let b = b"hello\0".as_ptr() as *const c_char;
        let c = b"world\0".as_ptr() as *const c_char;
        assert!(c_streq(a, b));
        assert!(!c_streq(a, c));
        assert!(c_streq(ptr::null(), ptr::null()));
        assert!(!c_streq(a, ptr::null()));
    }
}
