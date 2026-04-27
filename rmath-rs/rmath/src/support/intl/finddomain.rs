//! Domain finding logic for GNU gettext.
//!
//! Ported from `finddomain.c` in the GNU gettext `intl/` library.
//! Implements `_nl_find_domain()` which looks up and loads message catalogs.

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::ptr;

use super::types::*;

thread_local! { static _nl_loaded_domains: RefCell<*mut loaded_l10nfile> = RefCell::new(ptr::null_mut()); }

// ---------------------------------------------------------------------------
// Internal helpers (matching external functions declared in loadinfo.h)
// ---------------------------------------------------------------------------

/// Create / look up locale file entries in the loaded-domains list.
///
/// This corresponds to `_nl_make_l10nflist` in `l10nflist.c`.
/// For the standalone port, we provide a simplified stub that allocates a
/// `loaded_l10nfile` node if `do_allocate` is set.
unsafe fn _nl_make_l10nflist(
    l10nfile_list: *mut *mut loaded_l10nfile,
    _dirlist: *const c_char,
    _dirlist_len: usize,
    _mask: c_int,
    _language: *const c_char,
    _territory: *const c_char,
    _codeset: *const c_char,
    _normalized_codeset: *const c_char,
    _modifier: *const c_char,
    _filename: *const c_char,
    do_allocate: c_int,
) -> *mut loaded_l10nfile {
    unsafe {
        if do_allocate == 0 {
            // Search-only mode: check if the list already has an entry.
            let mut run = *l10nfile_list;
            while !run.is_null() {
                // For a simplified implementation, return the first entry if it exists.
                if !(*run).filename.is_null() {
                    return run;
                }
                run = (*run).next;
            }
            return ptr::null_mut();
        }

        // Allocate mode: create a new entry and prepend it.
        let new_entry =
            std::alloc::alloc(std::alloc::Layout::new::<loaded_l10nfile>()) as *mut loaded_l10nfile;
        if new_entry.is_null() {
            return ptr::null_mut();
        }
        ptr::write_bytes(new_entry, 0, 1);
        (*new_entry).decided = 0;
        (*new_entry).next = *l10nfile_list;
        (*new_entry).successor[0] = ptr::null_mut();
        *l10nfile_list = new_entry;
        new_entry
    }
}

/// Load a message domain from its .mo file.
///
/// This corresponds to `_nl_load_domain` in `loadmsg.c`.
/// For the standalone port we provide a stub that marks the domain as decided.
unsafe fn _nl_load_domain(domain: *mut loaded_l10nfile, _domainbinding: *mut binding) {
    unsafe {
        if !domain.is_null() {
            (*domain).decided = 1;
        }
    }
}

/// Split a locale name into its components.
///
/// This corresponds to `_nl_explode_name` in `explodename.c`.
/// Returns a bitmask of XPG_* flags, or -1 on error (OOM).
unsafe fn _nl_explode_name(
    _name: *mut c_char,
    language: *mut *const c_char,
    modifier: *mut *const c_char,
    territory: *mut *const c_char,
    codeset: *mut *const c_char,
    normalized_codeset: *mut *const c_char,
) -> c_int {
    unsafe {
        // Simplified stub: just set language to the whole name, others to null.
        if !language.is_null() {
            *language = _name;
        }
        if !modifier.is_null() {
            *modifier = ptr::null();
        }
        if !territory.is_null() {
            *territory = ptr::null();
        }
        if !codeset.is_null() {
            *codeset = ptr::null();
        }
        if !normalized_codeset.is_null() {
            *normalized_codeset = ptr::null();
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return a data structure describing the message catalog for the given
/// DOMAINNAME and LOCALE, respecting the currently established bindings.
///
/// This is the Rust port of `_nl_find_domain()` from `finddomain.c`.
pub unsafe fn _nl_find_domain(
    dirname: *const c_char,
    locale: *mut c_char,
    domainname: *const c_char,
    domainbinding: *mut binding,
) -> *mut loaded_l10nfile {
    unsafe {
        let mut retval: *mut loaded_l10nfile;
        let mut language: *const c_char = ptr::null();
        let mut modifier: *const c_char = ptr::null();
        let mut territory: *const c_char = ptr::null();
        let mut codeset: *const c_char = ptr::null();
        let mut normalized_codeset: *const c_char = ptr::null();
        let mask: c_int;

        let dirname_len = if dirname.is_null() {
            0
        } else {
            CStr::from_ptr(dirname).to_bytes().len() + 1
        };

        retval = _nl_make_l10nflist(
            _nl_loaded_domains.with(|v| std::ptr::addr_of_mut!(*v.borrow_mut())),
            dirname,
            dirname_len,
            0,
            locale,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            domainname,
            0,
        );

        if !retval.is_null() {
            // We already know about this locale.
            if (*retval).decided <= 0 {
                _nl_load_domain(retval, domainbinding);
            }

            if !(*retval).data.is_null() {
                return retval;
            }

            // Try successor entries.
            let mut cnt: c_int = 0;
            while !(*retval).successor[cnt as usize].is_null() {
                if (*(*retval).successor[cnt as usize]).decided <= 0 {
                    _nl_load_domain((*retval).successor[cnt as usize], domainbinding);
                }
                if !(*(*retval).successor[cnt as usize]).data.is_null() {
                    break;
                }
                cnt += 1;
            }

            return retval;
        }

        // Explode the locale name into its components.
        mask = _nl_explode_name(
            locale,
            &mut language,
            &mut modifier,
            &mut territory,
            &mut codeset,
            &mut normalized_codeset,
        );
        if mask == -1 {
            // Out of memory.
            return ptr::null_mut();
        }

        retval = _nl_make_l10nflist(
            _nl_loaded_domains.with(|v| std::ptr::addr_of_mut!(*v.borrow_mut())),
            dirname,
            dirname_len,
            mask,
            language,
            territory,
            codeset,
            normalized_codeset,
            modifier,
            domainname,
            1,
        );

        if retval.is_null() {
            // Out of memory.
            return ptr::null_mut();
        }

        if (*retval).decided <= 0 {
            _nl_load_domain(retval, domainbinding);
        }
        if (*retval).data.is_null() {
            let mut cnt: c_int = 0;
            while !(*retval).successor[cnt as usize].is_null() {
                if (*(*retval).successor[cnt as usize]).decided <= 0 {
                    _nl_load_domain((*retval).successor[cnt as usize], domainbinding);
                }
                if !(*(*retval).successor[cnt as usize]).data.is_null() {
                    break;
                }
                cnt += 1;
            }
        }

        // Free the normalized_codeset if it was dynamically allocated.
        if (mask & XPG_NORM_CODESET) != 0 && !normalized_codeset.is_null() {
            let len = CStr::from_ptr(normalized_codeset).to_bytes().len() + 1;
            let layout = std::alloc::Layout::from_size_align(len, 1)
                .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
            std::alloc::dealloc(normalized_codeset as *mut u8, layout);
        }

        retval
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_domain_null_locale() {
        unsafe {
            let dirname = b"/usr/share/locale\0".as_ptr() as *const c_char;
            let domain = b"messages\0".as_ptr() as *const c_char;
            let result = _nl_find_domain(dirname, ptr::null_mut(), domain, ptr::null_mut());
            // With a null locale, explode_name returns 0, and we should
            // get a non-null result when do_allocate is set.
            assert!(!result.is_null());
        }
    }
}
