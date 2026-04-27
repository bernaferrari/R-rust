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
use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_ulong};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Internal constants
// ---------------------------------------------------------------------------

/// Maximum depth of locale aliasing.
const MAX_LOCALE_ALIAS_DEPTH: c_int = 10;

/// Size of the message cache.
const MSGCTRN_SIZE: usize = 256;

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// Cache for looked-up messages to avoid repeated catalog lookups.
///
/// Each entry maps (msgid, domain, category) -> translation.
thread_local! { static _nl_msg_cache: Cell<[*mut c_char; MSGCTRN_SIZE]> = Cell::new([ptr::null_mut(); MSGCTRN_SIZE]); }

/// Cache of domain data (keyed by domain binding hash).
thread_local! { static _nl_domain_cache: Cell<[*mut loaded_l10nfile; 64]> = Cell::new([ptr::null_mut(); 64]); }

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Duplicate a C string.
unsafe fn c_strdup(s: *const c_char) -> *mut c_char {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }
        let len = CStr::from_ptr(s).to_bytes().len() + 1;
        let layout = Layout::from_size_align(len, 1).unwrap_or_else(|_| Layout::new::<u8>());
        let ptr = std::alloc::alloc(layout) as *mut c_char;
        if !ptr.is_null() {
            ptr::copy_nonoverlapping(s, ptr, len);
        }
        ptr
    }
}

/// Check if two C strings are equal.
unsafe fn c_streq(a: *const c_char, b: *const c_char) -> bool {
    unsafe {
        if a.is_null() && b.is_null() {
            return true;
        }
        if a.is_null() || b.is_null() {
            return false;
        }
        let ca = CStr::from_ptr(a);
        let cb = CStr::from_ptr(b);
        ca == cb
    }
}

/// Get the current locale for a category from the environment.
///
/// Follows the POSIX precedence: LC_ALL, LC_xxx, LANG.
unsafe fn gl_locale_name_posix(category: c_int) -> *mut c_char {
    unsafe {
        // Try LC_ALL first.
        if let Ok(val) = std::env::var("LC_ALL") {
            if let Ok(cstr) = CString::new(val.as_str()) {
                let layout = Layout::from_size_align(cstr.as_bytes_with_nul().len(), 1)
                    .unwrap_or_else(|_| Layout::new::<u8>());
                let ptr = std::alloc::alloc(layout) as *mut c_char;
                if !ptr.is_null() {
                    ptr::copy_nonoverlapping(
                        cstr.as_ptr(),
                        ptr as *mut libc::c_char,
                        cstr.as_bytes_with_nul().len(),
                    );
                    return ptr;
                }
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

        if let Ok(val) = std::env::var(cat_name) {
            if let Ok(cstr) = CString::new(val.as_str()) {
                let layout = Layout::from_size_align(cstr.as_bytes_with_nul().len(), 1)
                    .unwrap_or_else(|_| Layout::new::<u8>());
                let ptr = std::alloc::alloc(layout) as *mut c_char;
                if !ptr.is_null() {
                    ptr::copy_nonoverlapping(
                        cstr.as_ptr(),
                        ptr as *mut libc::c_char,
                        cstr.as_bytes_with_nul().len(),
                    );
                    return ptr;
                }
            }
        }

        // Try LANG.
        if let Ok(val) = std::env::var("LANG") {
            if let Ok(cstr) = CString::new(val.as_str()) {
                let layout = Layout::from_size_align(cstr.as_bytes_with_nul().len(), 1)
                    .unwrap_or_else(|_| Layout::new::<u8>());
                let ptr = std::alloc::alloc(layout) as *mut c_char;
                if !ptr.is_null() {
                    ptr::copy_nonoverlapping(
                        cstr.as_ptr(),
                        ptr as *mut libc::c_char,
                        cstr.as_bytes_with_nul().len(),
                    );
                    return ptr;
                }
            }
        }

        // Default to "C".
        c_strdup(b"C\0".as_ptr() as *const c_char)
    }
}

/// Get the locale name, trying various sources.
unsafe fn gl_locale_name(category: c_int, _categoryname: *const c_char) -> *mut c_char {
    unsafe {
        // Try the platform-specific method first.
        let platform_locale = super::localename::_nl_locale_name(category) as *mut c_char;
        if !platform_locale.is_null() && !c_streq(platform_locale, b"C\0".as_ptr() as *const c_char)
        {
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
        if let Ok(val) = std::env::var("LANGUAGE") {
            if let Ok(cstr) = CString::new(val.as_str()) {
                let layout = Layout::from_size_align(cstr.as_bytes_with_nul().len(), 1)
                    .unwrap_or_else(|_| Layout::new::<u8>());
                let ptr = std::alloc::alloc(layout) as *mut c_char;
                if !ptr.is_null() {
                    ptr::copy_nonoverlapping(
                        cstr.as_ptr(),
                        ptr as *mut libc::c_char,
                        cstr.as_bytes_with_nul().len(),
                    );
                    return ptr;
                }
            }
        }

        // Fall back to "C".
        c_strdup(b"C\0".as_ptr() as *const c_char)
    }
}

/// Get the language preferences (for LANGUAGE env var).
unsafe fn gl_locale_name_language_pref() -> *mut c_char {
    unsafe {
        let prefs = super::langprefs::_nl_language_preferences_default();
        if !prefs.is_null() {
            return c_strdup(prefs);
        }

        // Try LANGUAGE env var.
        if let Ok(val) = std::env::var("LANGUAGE") {
            if let Ok(cstr) = CString::new(val.as_str()) {
                let layout = Layout::from_size_align(cstr.as_bytes_with_nul().len(), 1)
                    .unwrap_or_else(|_| Layout::new::<u8>());
                let ptr = std::alloc::alloc(layout) as *mut c_char;
                if !ptr.is_null() {
                    ptr::copy_nonoverlapping(
                        cstr.as_ptr(),
                        ptr as *mut libc::c_char,
                        cstr.as_bytes_with_nul().len(),
                    );
                    return ptr;
                }
            }
        }

        ptr::null_mut()
    }
}

/// Split a colon-separated locale list and look up messages for each.
///
/// This implements the LANGUAGE variable support where multiple locale
/// names can be specified, separated by colons.
unsafe fn lookup_in_language_list(
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
    domainname: *const c_char,
    category: c_int,
    binding: *mut binding,
    dirname: *const c_char,
    language_list: *const c_char,
) -> *mut c_char {
    unsafe {
        if language_list.is_null() {
            return ptr::null_mut();
        }

        let list = CStr::from_ptr(language_list).to_bytes();
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
}

/// Internal lookup function for a single locale.
unsafe fn dcigettext_internal(
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
    domainname: *const c_char,
    _category: c_int,
    binding: *mut binding,
    dirname: *const c_char,
    locale: *mut c_char,
) -> *mut c_char {
    unsafe {
        if msgid.is_null() {
            return ptr::null_mut();
        }

        // Use the default domain if none specified.
        let effective_domain = if !domainname.is_null() {
            domainname
        } else {
            _nl_current_default_domain.with(|v| v.get())
        };

        if effective_domain.is_null() {
            return msgid as *mut c_char;
        }

        // Use the default dirname if none specified.
        let effective_dirname = if !dirname.is_null() {
            dirname
        } else if !binding.is_null() && !(*binding).dirname.is_null() {
            (*binding).dirname
        } else {
            _nl_default_dirname.as_ptr()
        };

        // Find the message catalog for this domain and locale.
        let domain_file = super::finddomain::_nl_find_domain(
            effective_dirname,
            locale,
            effective_domain,
            binding,
        );

        if domain_file.is_null() || (*domain_file).data.is_null() {
            return msgid as *mut c_char;
        }

        let domain_data = (*domain_file).data as *const loaded_domain;
        if domain_data.is_null() {
            return msgid as *mut c_char;
        }

        // Look up the message in the catalog.
        let translation = lookup_message_in_domain(domain_data, msgid, msgid_plural, n);

        if translation.is_null() {
            return msgid as *mut c_char;
        }

        translation
    }
}

/// Look up a message in a loaded domain.
///
/// Uses the hash table for fast lookup, falling back to linear search.
unsafe fn lookup_message_in_domain(
    domain: *const loaded_domain,
    msgid: *const c_char,
    msgid_plural: *const c_char,
    n: c_ulong,
) -> *mut c_char {
    unsafe {
        if domain.is_null() || msgid.is_null() {
            return ptr::null_mut();
        }

        let nstrings = (*domain).nstrings;
        if nstrings == 0 {
            return ptr::null_mut();
        }

        let is_plural = !msgid_plural.is_null();

        // Use hash table for lookup if available.
        if (*domain).hash_size > 0 && !(*domain).hash_tab.is_null() {
            let hash_val = super::hash_string::__hash_string(msgid);
            let mut idx = (hash_val as nls_uint32) % (*domain).hash_size;
            let incr = 1u32 + (hash_val as nls_uint32) % ((*domain).hash_size - 1);

            for _ in 0..(*domain).hash_size {
                let hash_entry = if (*domain).must_swap_hash_tab != 0 {
                    let val = ptr::read_unaligned((*domain).hash_tab.add(idx as usize));
                    SWAP(val)
                } else {
                    ptr::read_unaligned((*domain).hash_tab.add(idx as usize))
                };

                if hash_entry == 0 {
                    // Empty slot, not found.
                    break;
                }

                let string_idx = hash_entry - 1;
                if string_idx < nstrings {
                    let orig_entry = &*(*domain).orig_tab.add(string_idx as usize);
                    let _orig_length = if (*domain).must_swap != 0 {
                        SWAP(orig_entry.length)
                    } else {
                        orig_entry.length
                    };
                    let orig_offset = if (*domain).must_swap != 0 {
                        SWAP(orig_entry.offset)
                    } else {
                        orig_entry.offset
                    };

                    let orig_str = (*domain).data.add(orig_offset as usize);

                    // Compare the msgid with the original string.
                    let orig_cstr = CStr::from_ptr(orig_str);
                    let msgid_cstr = CStr::from_ptr(msgid);

                    if orig_cstr == msgid_cstr {
                        // Found! Get the translation.
                        let trans_entry = &*(*domain).trans_tab.add(string_idx as usize);
                        let trans_length = if (*domain).must_swap != 0 {
                            SWAP(trans_entry.length)
                        } else {
                            trans_entry.length
                        };
                        let trans_offset = if (*domain).must_swap != 0 {
                            SWAP(trans_entry.offset)
                        } else {
                            trans_entry.offset
                        };

                        if trans_length == 0 {
                            return ptr::null_mut();
                        }

                        let trans_str = (*domain).data.add(trans_offset as usize);

                        if is_plural {
                            // For plural forms, find the correct form.
                            let plural_form = super::plural_exp::plural_eval((*domain).plural, n);
                            if plural_form == 0 {
                                // First form is at trans_str, separated by NUL from subsequent forms.
                                return trans_str as *mut c_char;
                            } else {
                                // Skip to the N+1th form.
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
                                // Not enough plural forms, return the last one.
                                return msgid as *mut c_char;
                            }
                        } else {
                            return trans_str as *mut c_char;
                        }
                    }
                }

                idx = (idx + incr) % (*domain).hash_size;
            }
        } else {
            // Linear search through the original strings.
            for i in 0..nstrings as usize {
                let orig_entry = &*(*domain).orig_tab.add(i);
                let _orig_length = if (*domain).must_swap != 0 {
                    SWAP(orig_entry.length)
                } else {
                    orig_entry.length
                };
                let orig_offset = if (*domain).must_swap != 0 {
                    SWAP(orig_entry.offset)
                } else {
                    orig_entry.offset
                };

                let orig_str = (*domain).data.add(orig_offset as usize);
                let orig_cstr = CStr::from_ptr(orig_str);
                let msgid_cstr = CStr::from_ptr(msgid);

                if orig_cstr == msgid_cstr {
                    let trans_entry = &*(*domain).trans_tab.add(i);
                    let trans_length = if (*domain).must_swap != 0 {
                        SWAP(trans_entry.length)
                    } else {
                        trans_entry.length
                    };
                    let trans_offset = if (*domain).must_swap != 0 {
                        SWAP(trans_entry.offset)
                    } else {
                        trans_entry.offset
                    };

                    if trans_length == 0 {
                        return ptr::null_mut();
                    }

                    return (*domain).data.add(trans_offset as usize) as *mut c_char;
                }
            }
        }

        ptr::null_mut()
    }
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
    unsafe {
        if msgid.is_null() {
            return ptr::null_mut();
        }

        // Empty msgid -> return it as-is.
        if *msgid == 0 {
            return msgid as *mut c_char;
        }

        // Determine the effective domain.
        let effective_domain = if !domainname.is_null() {
            domainname
        } else {
            _nl_current_default_domain.with(|v| v.get())
        };

        if effective_domain.is_null() {
            return msgid as *mut c_char;
        }

        // Find the binding for this domain.
        let mut binding: *mut binding = _nl_domain_bindings.with(|v| v.get());
        while !binding.is_null() {
            if c_streq((*binding).domainname.as_ptr(), effective_domain) {
                break;
            }
            binding = (*binding).next;
        }

        // Get the dirname from the binding.
        let dirname = if !binding.is_null() && !(*binding).dirname.is_null() {
            (*binding).dirname
        } else {
            _nl_default_dirname.as_ptr()
        };

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
        if !locale.is_null() {
            let layout = Layout::from_size_align(CStr::from_ptr(locale).to_bytes().len() + 1, 1)
                .unwrap_or_else(|_| Layout::new::<u8>());
            std::alloc::dealloc(locale as *mut u8, layout);
        }

        if result.is_null() {
            msgid as *mut c_char
        } else {
            result
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
    fn test_dcigettext_null_msgid() {
        unsafe {
            let result = libintl_dcigettext(ptr::null(), ptr::null(), ptr::null(), 1, LC_MESSAGES);
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_dcigettext_empty_msgid() {
        unsafe {
            let msgid = b"\0" as *const u8 as *const c_char;
            let result = libintl_dcigettext(ptr::null(), msgid, ptr::null(), 1, LC_MESSAGES);
            assert_eq!(result, msgid as *mut c_char);
        }
    }

    #[test]
    fn test_dcigettext_returns_msgid() {
        unsafe {
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
    }

    #[test]
    fn test_dcigettext_plural() {
        unsafe {
            let msgid1 = b"file\0" as *const u8 as *const c_char;
            let msgid2 = b"files\0" as *const u8 as *const c_char;
            let result = libintl_dcigettext(ptr::null(), msgid1, msgid2, 1, LC_MESSAGES);
            // With no catalog loaded, should return msgid1 for n=1.
            assert_eq!(result, msgid1 as *mut c_char);
        }
    }

    #[test]
    fn test_c_streq() {
        unsafe {
            let a = b"hello\0".as_ptr() as *const c_char;
            let b = b"hello\0".as_ptr() as *const c_char;
            let c = b"world\0".as_ptr() as *const c_char;
            assert!(c_streq(a, b));
            assert!(!c_streq(a, c));
            assert!(c_streq(ptr::null(), ptr::null()));
            assert!(!c_streq(a, ptr::null()));
        }
    }
}
