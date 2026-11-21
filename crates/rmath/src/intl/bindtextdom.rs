#![warn(unused_unsafe)]
//! Domain binding management for GNU gettext.
//!
//! Ported from `bindtextdom.c` in the GNU gettext `intl/` library.
//! Implements `bindtextdomain()` and `bind_textdomain_codeset()`.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Specifies the directory name and/or output codeset to be used for the
/// given message domain.
///
/// If `dirnamep` is non-null and `*dirnamep` is null, the current dirname
/// binding is returned through `*dirnamep`.  If `*dirnamep` is non-null, the
/// binding is set to that value.  The same logic applies to `codesetp`.
///
/// This faithfully reproduces the C `set_binding_values` logic.
fn set_binding_values(
    domainname: *const c_char,
    dirnamep: *mut *const c_char,
    codesetp: *mut *const c_char,
) {
    // Sanity check: empty or null domain name -> return defaults.
    if cstr(domainname).map_or(true, |s| s.to_bytes().is_empty()) {
        if let Some(p) = ptr_mut(dirnamep) {
            *p = ptr::null();
        }
        if let Some(p) = ptr_mut(codesetp) {
            *p = ptr::null();
        }
        return;
    }

    let mut modified: c_int = 0;
    let mut binding: *mut binding = with_intl_runtime(|intl| intl.domain_bindings);

    // Walk the sorted linked list looking for an existing binding.
    while let Some(b) = ptr_mut(binding) {
        let compare = libc_strcmp(domainname, b.domainname.as_ptr());
        if compare == 0 {
            break; // Found it.
        }
        if compare < 0 {
            binding = ptr::null_mut();
            break;
        }
        binding = b.next;
    }

    if let Some(b) = ptr_mut(binding) {
        // --- Existing binding: update dirname ---
        if !dirnamep.is_null() {
            let dirname = ptr_ref(dirnamep).map_or(ptr::null(), |p| *p);
            if dirname.is_null() {
                // Return the current binding.
                if let Some(p) = ptr_mut(dirnamep) {
                    *p = b.dirname;
                }
            } else {
                let result = b.dirname;
                if libc_strcmp(dirname, result) != 0 {
                    let new_result = if libc_strcmp(dirname, _nl_default_dirname.as_ptr()) == 0 {
                        _nl_default_dirname.as_ptr() as *mut c_char
                    } else {
                        c_strdup(dirname)
                    };
                    if !new_result.is_null() {
                        if b.dirname != _nl_default_dirname.as_ptr() as *mut c_char
                            && !b.dirname.is_null()
                        {
                            c_free(b.dirname);
                        }
                        b.dirname = new_result;
                        modified = 1;
                    }
                }
                if let Some(p) = ptr_mut(dirnamep) {
                    *p = b.dirname;
                }
            }
        }

        // --- Existing binding: update codeset ---
        if !codesetp.is_null() {
            let codeset = ptr_ref(codesetp).map_or(ptr::null(), |p| *p);
            if codeset.is_null() {
                if let Some(p) = ptr_mut(codesetp) {
                    *p = b.codeset;
                }
            } else {
                let result = b.codeset;
                if result.is_null() || libc_strcmp(codeset, result) != 0 {
                    let new_result = c_strdup(codeset);
                    if !new_result.is_null() {
                        if !b.codeset.is_null() {
                            c_free(b.codeset);
                        }
                        b.codeset = new_result;
                        modified = 1;
                    }
                }
                if let Some(p) = ptr_mut(codesetp) {
                    *p = b.codeset;
                }
            }
        }
    } else if ptr_ref(dirnamep).map_or(true, |p| (*p).is_null())
        && ptr_ref(codesetp).map_or(true, |p| (*p).is_null())
    {
        // No existing binding and nothing to set -> return defaults.
        if let Some(p) = ptr_mut(dirnamep) {
            *p = _nl_default_dirname.as_ptr();
        }
        if let Some(p) = ptr_mut(codesetp) {
            *p = ptr::null();
        }
    } else {
        // --- Create a new binding ---
        let len = libc_strlen(domainname) + 1;
        // Allocate binding + flexible array domainname at the end.
        let struct_size = std::mem::size_of::<binding>();
        let layout = Layout::from_size_align(struct_size + len, std::mem::align_of::<binding>())
            .unwrap_or_else(|_| Layout::new::<u8>());
        // Allocate `struct_size + len` bytes: the binding plus its name.
        let new_binding = unsafe { std::alloc::alloc(layout) } as *mut binding;

        if new_binding.is_null() {
            // Allocation failed.
            if let Some(p) = ptr_mut(dirnamep) {
                *p = ptr::null();
            }
            if let Some(p) = ptr_mut(codesetp) {
                *p = ptr::null();
            }
        } else if let Some(nb) = ptr_mut(new_binding) {
            if let (Some(dst), Some(src)) = (
                slice_mut(nb.domainname.as_mut_ptr() as *mut u8, len),
                slice_ref(domainname as *const u8, len),
            ) {
                dst.copy_from_slice(src);
            }

            // --- Set dirname ---
            if !dirnamep.is_null() {
                let mut dirname = ptr_ref(dirnamep).map_or(ptr::null(), |p| *p);
                if dirname.is_null() || libc_strcmp(dirname, _nl_default_dirname.as_ptr()) == 0 {
                    dirname = _nl_default_dirname.as_ptr();
                } else {
                    let result = c_strdup(dirname);
                    if result.is_null() {
                        // Failed to strdup dirname; free the new binding.
                        unsafe { std::alloc::dealloc(new_binding as *mut u8, layout) };
                        if let Some(p) = ptr_mut(dirnamep) {
                            *p = ptr::null();
                        }
                        if let Some(p) = ptr_mut(codesetp) {
                            *p = ptr::null();
                        }
                    } else {
                        dirname = result;
                    }
                }
                if let Some(p) = ptr_mut(dirnamep) {
                    *p = dirname;
                }
                nb.dirname = dirname as *mut c_char;
            } else {
                nb.dirname = _nl_default_dirname.as_ptr() as *mut c_char;
            }

            // --- Set codeset ---
            if !codesetp.is_null() {
                let mut codeset = ptr_ref(codesetp).map_or(ptr::null(), |p| *p);
                if !codeset.is_null() {
                    let result = c_strdup(codeset);
                    if result.is_null() {
                        // Failed to strdup codeset; free the new binding.
                        if nb.dirname != _nl_default_dirname.as_ptr() as *mut c_char
                            && !nb.dirname.is_null()
                        {
                            c_free(nb.dirname);
                        }
                        unsafe { std::alloc::dealloc(new_binding as *mut u8, layout) };
                        if let Some(p) = ptr_mut(dirnamep) {
                            *p = ptr::null();
                        }
                        if let Some(p) = ptr_mut(codesetp) {
                            *p = ptr::null();
                        }
                    } else {
                        codeset = result;
                    }
                }
                if let Some(p) = ptr_mut(codesetp) {
                    *p = codeset;
                }
                nb.codeset = codeset as *mut c_char;
            } else {
                nb.codeset = ptr::null_mut();
            }

            // --- Enqueue the new binding in sorted order ---
            let head = with_intl_runtime(|intl| intl.domain_bindings);
            let head_name = ptr_ref(head).map_or(ptr::null(), |h| h.domainname.as_ptr());
            if head_name.is_null() || libc_strcmp(domainname, head_name) < 0 {
                nb.next = head;
                with_intl_runtime(|intl| intl.domain_bindings = new_binding);
            } else {
                let mut cur = head;
                while let Some(c) = ptr_mut(cur) {
                    let next_name = ptr_ref(c.next).map_or(ptr::null(), |n| n.domainname.as_ptr());
                    if next_name.is_null() || libc_strcmp(domainname, next_name) <= 0 {
                        break;
                    }
                    cur = c.next;
                }
                nb.next = ptr_mut(cur).map_or(ptr::null_mut(), |c| c.next);
                if let Some(c) = ptr_mut(cur) {
                    c.next = new_binding;
                }
            }

            modified = 1;
        }
    }

    // If we modified any binding, flush the caches.
    if modified != 0 {
        with_intl_runtime(|intl| intl.msg_cat_cntr += 1);
    }
}

// ---------------------------------------------------------------------------
// FFI-exported functions
// ---------------------------------------------------------------------------

/// Specify that the DOMAINNAME message catalog will be found in DIRNAME
/// rather than in the system locale data base.
///
/// Returns the current dirname for the domain (may be the newly set value).
pub fn libintl_bindtextdomain(domainname: *const c_char, dirname: *const c_char) -> *mut c_char {
    let mut dirname_mut: *const c_char = dirname;
    set_binding_values(domainname, &mut dirname_mut, ptr::null_mut());
    dirname_mut as *mut c_char
}

/// Specify the character encoding in which messages from the DOMAINNAME
/// message catalog will be returned.
///
/// Returns the current codeset for the domain (may be the newly set value).
pub fn libintl_bind_textdomain_codeset(
    domainname: *const c_char,
    codeset: *const c_char,
) -> *mut c_char {
    let mut codeset_mut: *const c_char = codeset;
    set_binding_values(domainname, ptr::null_mut(), &mut codeset_mut);
    codeset_mut as *mut c_char
}

// ---------------------------------------------------------------------------
// Helper functions (private, replacing libc / string.h)
// ---------------------------------------------------------------------------

use std::alloc::Layout;

/// Equivalent of `strlen` for a NUL-terminated C string.
fn libc_strlen(s: *const c_char) -> usize {
    cstr(s).map_or(0, |s| s.to_bytes().len())
}

/// Equivalent of `strcmp`.
fn libc_strcmp(a: *const c_char, b: *const c_char) -> c_int {
    let (Some(x), Some(y)) = (cstr(a), cstr(b)) else {
        // Match strcmp on null pointers: null < non-null.
        return match (a.is_null(), b.is_null()) {
            (true, true) => 0,
            (true, false) => -1,
            (false, true) => 1,
            (false, false) => unreachable!(),
        };
    };
    let ca = x.to_bytes();
    let cb = y.to_bytes();
    // Lexicographic comparison.
    for (ba, bb) in ca.iter().zip(cb.iter()) {
        if *ba != *bb {
            return (*ba as c_int) - (*bb as c_int);
        }
    }
    (ca.len() as c_int) - (cb.len() as c_int)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_libc_strcmp_equal() {
        let a = b"hello\0".as_ptr() as *const c_char;
        let b = b"hello\0".as_ptr() as *const c_char;
        assert_eq!(libc_strcmp(a, b), 0);
    }

    #[test]
    fn test_libc_strcmp_less() {
        let a = b"abc\0".as_ptr() as *const c_char;
        let b = b"abd\0".as_ptr() as *const c_char;
        assert!(libc_strcmp(a, b) < 0);
    }

    #[test]
    fn test_libc_strcmp_greater() {
        let a = b"abd\0".as_ptr() as *const c_char;
        let b = b"abc\0".as_ptr() as *const c_char;
        assert!(libc_strcmp(a, b) > 0);
    }

    #[test]
    fn test_libc_strlen() {
        let s = b"hello\0".as_ptr() as *const c_char;
        assert_eq!(libc_strlen(s), 5);
    }

    #[test]
    fn test_libc_strlen_null() {
        assert_eq!(libc_strlen(ptr::null()), 0);
    }

    #[test]
    fn test_bindtextdomain_null_domain() {
        let result = libintl_bindtextdomain(ptr::null(), ptr::null());
        assert!(result.is_null());
    }

    #[test]
    fn test_bindtextdomain_empty_domain() {
        let empty = b"\0".as_ptr() as *const c_char;
        let result = libintl_bindtextdomain(empty, ptr::null());
        assert!(result.is_null());
    }
}
