//! Domain binding management for GNU gettext.
//!
//! Ported from `bindtextdom.c` in the GNU gettext `intl/` library.
//! Implements `bindtextdomain()` and `bind_textdomain_codeset()`.

#![allow(non_snake_case)]

use std::ffi::CStr;
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
unsafe fn set_binding_values(
    domainname: *const c_char,
    dirnamep: *mut *const c_char,
    codesetp: *mut *const c_char,
) {
    unsafe {
        // Sanity check: empty or null domain name -> return defaults.
        if domainname.is_null() || *domainname == 0 {
            if !dirnamep.is_null() {
                *dirnamep = ptr::null();
            }
            if !codesetp.is_null() {
                *codesetp = ptr::null();
            }
            return;
        }

        let mut modified: c_int = 0;
        let mut binding: *mut binding = with_intl_runtime(|intl| intl.domain_bindings);

        // Walk the sorted linked list looking for an existing binding.
        while !binding.is_null() {
            let compare = libc_strcmp(domainname, (*binding).domainname.as_ptr());
            if compare == 0 {
                break; // Found it.
            }
            if compare < 0 {
                binding = ptr::null_mut();
                break;
            }
            binding = (*binding).next;
        }

        if !binding.is_null() {
            // --- Existing binding: update dirname ---
            if !dirnamep.is_null() {
                let dirname = *dirnamep;
                if dirname.is_null() {
                    // Return the current binding.
                    *dirnamep = (*binding).dirname;
                } else {
                    let result = (*binding).dirname;
                    if libc_strcmp(dirname, result) != 0 {
                        let new_result = if libc_strcmp(dirname, _nl_default_dirname.as_ptr()) == 0
                        {
                            _nl_default_dirname.as_ptr() as *mut c_char
                        } else {
                            c_strdup(dirname)
                        };
                        if !new_result.is_null() {
                            if (*binding).dirname
                                != (*std::ptr::addr_of!(_nl_default_dirname)).as_ptr()
                                    as *mut c_char
                                && !(*binding).dirname.is_null()
                            {
                                let layout =
                                    Layout::from_size_align(libc_strlen((*binding).dirname) + 1, 1)
                                        .unwrap_or_else(|_| Layout::new::<u8>());
                                std::alloc::dealloc((*binding).dirname as *mut u8, layout);
                            }
                            (*binding).dirname = new_result;
                            modified = 1;
                        }
                    }
                    *dirnamep = (*binding).dirname;
                }
            }

            // --- Existing binding: update codeset ---
            if !codesetp.is_null() {
                let codeset = *codesetp;
                if codeset.is_null() {
                    *codesetp = (*binding).codeset;
                } else {
                    let result = (*binding).codeset;
                    if result.is_null() || libc_strcmp(codeset, result) != 0 {
                        let new_result = c_strdup(codeset);
                        if !new_result.is_null() {
                            if !(*binding).codeset.is_null() {
                                let layout =
                                    Layout::from_size_align(libc_strlen((*binding).codeset) + 1, 1)
                                        .unwrap_or_else(|_| Layout::new::<u8>());
                                std::alloc::dealloc((*binding).codeset as *mut u8, layout);
                            }
                            (*binding).codeset = new_result;
                            modified = 1;
                        }
                    }
                    *codesetp = (*binding).codeset;
                }
            }
        } else if (dirnamep.is_null() || (*dirnamep).is_null())
            && (codesetp.is_null() || (*codesetp).is_null())
        {
            // No existing binding and nothing to set -> return defaults.
            if !dirnamep.is_null() {
                *dirnamep = (*std::ptr::addr_of!(_nl_default_dirname)).as_ptr();
            }
            if !codesetp.is_null() {
                *codesetp = ptr::null();
            }
        } else {
            // --- Create a new binding ---
            let len = libc_strlen(domainname) + 1;
            // Allocate binding + flexible array domainname at the end.
            let struct_size = std::mem::size_of::<binding>();
            let layout =
                Layout::from_size_align(struct_size + len, std::mem::align_of::<binding>())
                    .unwrap_or_else(|_| Layout::new::<u8>());
            let new_binding = std::alloc::alloc(layout) as *mut binding;

            if new_binding.is_null() {
                // Allocation failed.
                if !dirnamep.is_null() {
                    *dirnamep = ptr::null();
                }
                if !codesetp.is_null() {
                    *codesetp = ptr::null();
                }
            } else {
                ptr::copy_nonoverlapping(domainname, (*new_binding).domainname.as_mut_ptr(), len);

                // --- Set dirname ---
                if !dirnamep.is_null() {
                    let mut dirname = *dirnamep;
                    if dirname.is_null()
                        || libc_strcmp(dirname, (*std::ptr::addr_of!(_nl_default_dirname)).as_ptr())
                            == 0
                    {
                        dirname = (*std::ptr::addr_of!(_nl_default_dirname)).as_ptr();
                    } else {
                        let result = c_strdup(dirname);
                        if result.is_null() {
                            // Failed to strdup dirname.
                            std::alloc::dealloc(new_binding as *mut u8, layout);
                            if !dirnamep.is_null() {
                                *dirnamep = ptr::null();
                            }
                            if !codesetp.is_null() {
                                *codesetp = ptr::null();
                            }
                        } else {
                            dirname = result;
                        }
                    }
                    *dirnamep = dirname;
                    (*new_binding).dirname = dirname as *mut c_char;
                } else {
                    (*new_binding).dirname =
                        (*std::ptr::addr_of!(_nl_default_dirname)).as_ptr() as *mut c_char;
                }

                // --- Set codeset ---
                if !codesetp.is_null() {
                    let mut codeset = *codesetp;
                    if !codeset.is_null() {
                        let result = c_strdup(codeset);
                        if result.is_null() {
                            // Failed to strdup codeset.
                            if (*new_binding).dirname
                                != (*std::ptr::addr_of!(_nl_default_dirname)).as_ptr()
                                    as *mut c_char
                                && !(*new_binding).dirname.is_null()
                            {
                                let layout2 = Layout::from_size_align(
                                    libc_strlen((*new_binding).dirname) + 1,
                                    1,
                                )
                                .unwrap_or_else(|_| Layout::new::<u8>());
                                std::alloc::dealloc((*new_binding).dirname as *mut u8, layout2);
                            }
                            std::alloc::dealloc(new_binding as *mut u8, layout);
                            if !dirnamep.is_null() {
                                *dirnamep = ptr::null();
                            }
                            if !codesetp.is_null() {
                                *codesetp = ptr::null();
                            }
                        } else {
                            codeset = result;
                        }
                    }
                    *codesetp = codeset;
                    (*new_binding).codeset = codeset as *mut c_char;
                } else {
                    (*new_binding).codeset = ptr::null_mut();
                }

                // --- Enqueue the new binding in sorted order ---
                let head = with_intl_runtime(|intl| intl.domain_bindings);
                if head.is_null() || libc_strcmp(domainname, (*head).domainname.as_ptr()) < 0 {
                    (*new_binding).next = head;
                    with_intl_runtime(|intl| intl.domain_bindings = new_binding);
                } else {
                    let mut cur = head;
                    while !(*cur).next.is_null()
                        && libc_strcmp(domainname, (*(*cur).next).domainname.as_ptr()) > 0
                    {
                        cur = (*cur).next;
                    }
                    (*new_binding).next = (*cur).next;
                    (*cur).next = new_binding;
                }

                modified = 1;
            }
        }

        // If we modified any binding, flush the caches.
        if modified != 0 {
            with_intl_runtime(|intl| intl.msg_cat_cntr += 1);
        }
    }
}

// ---------------------------------------------------------------------------
// FFI-exported functions
// ---------------------------------------------------------------------------

/// Specify that the DOMAINNAME message catalog will be found in DIRNAME
/// rather than in the system locale data base.
///
/// Returns the current dirname for the domain (may be the newly set value).
pub unsafe fn libintl_bindtextdomain(
    domainname: *const c_char,
    dirname: *const c_char,
) -> *mut c_char {
    unsafe {
        let mut dirname_mut: *const c_char = dirname;
        set_binding_values(domainname, &mut dirname_mut, ptr::null_mut());
        dirname_mut as *mut c_char
    }
}

/// Specify the character encoding in which messages from the DOMAINNAME
/// message catalog will be returned.
///
/// Returns the current codeset for the domain (may be the newly set value).
pub unsafe fn libintl_bind_textdomain_codeset(
    domainname: *const c_char,
    codeset: *const c_char,
) -> *mut c_char {
    unsafe {
        let mut codeset_mut: *const c_char = codeset;
        set_binding_values(domainname, ptr::null_mut(), &mut codeset_mut);
        codeset_mut as *mut c_char
    }
}

// ---------------------------------------------------------------------------
// Helper functions (private, replacing libc / string.h)
// ---------------------------------------------------------------------------

use std::alloc::Layout;

/// Equivalent of `strlen` for a NUL-terminated C string.
unsafe fn libc_strlen(s: *const c_char) -> usize {
    unsafe {
        if s.is_null() {
            return 0;
        }
        CStr::from_ptr(s).to_bytes().len()
    }
}

/// Equivalent of `strcmp`.
unsafe fn libc_strcmp(a: *const c_char, b: *const c_char) -> c_int {
    unsafe {
        if a.is_null() && b.is_null() {
            return 0;
        }
        if a.is_null() {
            return -1;
        }
        if b.is_null() {
            return 1;
        }
        let ca = CStr::from_ptr(a).to_bytes();
        let cb = CStr::from_ptr(b).to_bytes();
        // Lexicographic comparison.
        for (ba, bb) in ca.iter().zip(cb.iter()) {
            if *ba != *bb {
                return (*ba as c_int) - (*bb as c_int);
            }
        }
        (ca.len() as c_int) - (cb.len() as c_int)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_libc_strcmp_equal() {
        unsafe {
            let a = b"hello\0".as_ptr() as *const c_char;
            let b = b"hello\0".as_ptr() as *const c_char;
            assert_eq!(libc_strcmp(a, b), 0);
        }
    }

    #[test]
    fn test_libc_strcmp_less() {
        unsafe {
            let a = b"abc\0".as_ptr() as *const c_char;
            let b = b"abd\0".as_ptr() as *const c_char;
            assert!(libc_strcmp(a, b) < 0);
        }
    }

    #[test]
    fn test_libc_strcmp_greater() {
        unsafe {
            let a = b"abd\0".as_ptr() as *const c_char;
            let b = b"abc\0".as_ptr() as *const c_char;
            assert!(libc_strcmp(a, b) > 0);
        }
    }

    #[test]
    fn test_libc_strlen() {
        unsafe {
            let s = b"hello\0".as_ptr() as *const c_char;
            assert_eq!(libc_strlen(s), 5);
        }
    }

    #[test]
    fn test_libc_strlen_null() {
        unsafe {
            assert_eq!(libc_strlen(ptr::null()), 0);
        }
    }

    #[test]
    fn test_bindtextdomain_null_domain() {
        unsafe {
            let result = libintl_bindtextdomain(ptr::null(), ptr::null());
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_bindtextdomain_empty_domain() {
        unsafe {
            let empty = b"\0".as_ptr() as *const c_char;
            let result = libintl_bindtextdomain(empty, ptr::null());
            assert!(result.is_null());
        }
    }
}
