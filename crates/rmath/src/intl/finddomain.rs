#![warn(unused_unsafe)]
//! Domain finding logic for GNU gettext.
//!
//! Ported from `finddomain.c` in the GNU gettext `intl/` library.
//! Implements `_nl_find_domain()` which looks up and loads message catalogs.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Internal helpers (matching external functions declared in loadinfo.h)
// ---------------------------------------------------------------------------

/// Create / look up locale file entries in the loaded-domains list.
///
/// This corresponds to `_nl_make_l10nflist` in `l10nflist.c`.
/// For the standalone port, we provide a simplified stub that allocates a
/// `loaded_l10nfile` node if `do_allocate` is set.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
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
    if do_allocate == 0 {
        // Search-only mode: check if the list already has an entry.
        // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
        let mut run = unsafe { ptr_ref(l10nfile_list) }.map_or(ptr::null_mut(), |p| *p);
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        while let Some(n) = unsafe { ptr_ref(run) } {
            // For a simplified implementation, return the first entry if it exists.
            if !n.filename.is_null() {
                return run;
            }
            run = n.next;
        }
        return ptr::null_mut();
    }

    // Allocate mode: create a new entry and prepend it.
    // Allocate a zeroed `loaded_l10nfile`.
    let new_entry =
        unsafe { std::alloc::alloc_zeroed(std::alloc::Layout::new::<loaded_l10nfile>()) }
            as *mut loaded_l10nfile;
    if new_entry.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    if let Some(e) = unsafe { ptr_mut(new_entry) } {
        e.decided = 0;
        // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
        e.next = unsafe { ptr_ref(l10nfile_list) }.map_or(ptr::null_mut(), |p| *p);
        e.successor[0] = ptr::null_mut();
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(l10nfile_list) } {
        *p = new_entry;
    }
    new_entry
}

/// Borrow the `idx`-th entry of a node's flexible successor array.
///
/// Callers guarantee the node's allocation includes room for `idx`
/// successor pointers, matching the C flexible-array contract.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn successor_at(l: *const loaded_l10nfile, idx: usize) -> *mut loaded_l10nfile {
    // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
    unsafe { ptr_ref(l) }
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        .and_then(|n| unsafe { ptr_ref(n.successor.as_ptr().wrapping_add(idx)) })
        .map_or(ptr::null_mut(), |p| *p)
}

/// Load a message domain from its .mo file.
///
/// This corresponds to `_nl_load_domain` in `loadmsg.c`.
/// For the standalone port we provide a stub that marks the domain as decided.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn _nl_load_domain(domain: *mut loaded_l10nfile, _domainbinding: *mut binding) {
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(d) = unsafe { ptr_mut(domain) } {
        d.decided = 1;
    }
}

/// Split a locale name into its components.
///
/// This corresponds to `_nl_explode_name` in `explodename.c`.
/// Returns a bitmask of XPG_* flags, or -1 on error (OOM).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn _nl_explode_name(
    _name: *mut c_char,
    language: *mut *const c_char,
    modifier: *mut *const c_char,
    territory: *mut *const c_char,
    codeset: *mut *const c_char,
    normalized_codeset: *mut *const c_char,
) -> c_int {
    // Simplified stub: just set language to the whole name, others to null.
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(language) } {
        *p = _name;
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(modifier) } {
        *p = ptr::null();
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(territory) } {
        *p = ptr::null();
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(codeset) } {
        *p = ptr::null();
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(normalized_codeset) } {
        *p = ptr::null();
    }
    0
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return a data structure describing the message catalog for the given
/// DOMAINNAME and LOCALE, respecting the currently established bindings.
///
/// This is the Rust port of `_nl_find_domain()` from `finddomain.c`.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn _nl_find_domain(
    dirname: *const c_char,
    locale: *mut c_char,
    domainname: *const c_char,
    domainbinding: *mut binding,
) -> *mut loaded_l10nfile {
    let mut language: *const c_char = ptr::null();
    let mut modifier: *const c_char = ptr::null();
    let mut territory: *const c_char = ptr::null();
    let mut codeset: *const c_char = ptr::null();
    let mut normalized_codeset: *const c_char = ptr::null();
    let mask: c_int;

    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let dirname_len = unsafe { cstr(dirname) }.map_or(0, |s| s.to_bytes().len() + 1);

    // SAFETY: forwarded raw pointers satisfy `_nl_make_l10nflist`'s contract per this function's own `# Safety` requirements.
    let mut retval = unsafe {
        _nl_make_l10nflist(
            with_intl_runtime(|intl| &mut intl.loaded_domains as *mut *mut loaded_l10nfile),
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
        )
    };

    if !retval.is_null() {
        // We already know about this locale.
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        if unsafe { ptr_ref(retval) }.is_some_and(|r| r.decided <= 0) {
            // SAFETY: forwarded raw pointers satisfy `_nl_load_domain`'s contract per this function's own `# Safety` requirements.
            unsafe { _nl_load_domain(retval, domainbinding) };
        }

        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        if unsafe { ptr_ref(retval) }.is_some_and(|r| !r.data.is_null()) {
            return retval;
        }

        // Try successor entries.
        let mut cnt: c_int = 0;
        loop {
            // SAFETY: forwarded raw pointers satisfy `successor_at`'s contract per this function's own `# Safety` requirements.
            let succ = unsafe { successor_at(retval, cnt as usize) };
            if succ.is_null() {
                break;
            }
            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
            if unsafe { ptr_ref(succ) }.is_some_and(|s| s.decided <= 0) {
                // SAFETY: forwarded raw pointers satisfy `_nl_load_domain`'s contract per this function's own `# Safety` requirements.
                unsafe { _nl_load_domain(succ, domainbinding) };
            }
            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
            if unsafe { ptr_ref(succ) }.is_some_and(|s| !s.data.is_null()) {
                break;
            }
            cnt += 1;
        }

        return retval;
    }

    // Explode the locale name into its components.
    // SAFETY: forwarded raw pointers satisfy `_nl_explode_name`'s contract per this function's own `# Safety` requirements.
    mask = unsafe {
        _nl_explode_name(
            locale,
            &mut language,
            &mut modifier,
            &mut territory,
            &mut codeset,
            &mut normalized_codeset,
        )
    };
    if mask == -1 {
        // Out of memory.
        return ptr::null_mut();
    }

    // SAFETY: forwarded raw pointers satisfy `_nl_make_l10nflist`'s contract per this function's own `# Safety` requirements.
    retval = unsafe {
        _nl_make_l10nflist(
            with_intl_runtime(|intl| &mut intl.loaded_domains as *mut *mut loaded_l10nfile),
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
        )
    };

    if retval.is_null() {
        // Out of memory.
        return ptr::null_mut();
    }

    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    if unsafe { ptr_ref(retval) }.is_some_and(|r| r.decided <= 0) {
        // SAFETY: forwarded raw pointers satisfy `_nl_load_domain`'s contract per this function's own `# Safety` requirements.
        unsafe { _nl_load_domain(retval, domainbinding) };
    }
    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    if unsafe { ptr_ref(retval) }.map_or(true, |r| r.data.is_null()) {
        let mut cnt: c_int = 0;
        loop {
            // SAFETY: forwarded raw pointers satisfy `successor_at`'s contract per this function's own `# Safety` requirements.
            let succ = unsafe { successor_at(retval, cnt as usize) };
            if succ.is_null() {
                break;
            }
            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
            if unsafe { ptr_ref(succ) }.is_some_and(|s| s.decided <= 0) {
                // SAFETY: forwarded raw pointers satisfy `_nl_load_domain`'s contract per this function's own `# Safety` requirements.
                unsafe { _nl_load_domain(succ, domainbinding) };
            }
            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
            if unsafe { ptr_ref(succ) }.is_some_and(|s| !s.data.is_null()) {
                break;
            }
            cnt += 1;
        }
    }

    // Free the normalized_codeset if it was dynamically allocated.
    if (mask & XPG_NORM_CODESET) != 0 && !normalized_codeset.is_null() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { c_free(normalized_codeset as *mut c_char) };
    }

    retval
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_domain_null_locale() {
        let _session = crate::sexp::session::RSession::new();

        let dirname = b"/usr/share/locale\0".as_ptr() as *const c_char;
        let domain = b"messages\0".as_ptr() as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { _nl_find_domain(dirname, ptr::null_mut(), domain, ptr::null_mut()) };
        // With a null locale, explode_name returns 0, and we should
        // get a non-null result when do_allocate is set.
        assert!(!result.is_null());
    }
}
