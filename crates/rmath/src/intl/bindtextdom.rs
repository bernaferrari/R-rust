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
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn set_binding_values(
    domainname: *const c_char,
    dirnamep: *mut *const c_char,
    codesetp: *mut *const c_char,
) {
    // Sanity check: empty or null domain name -> return defaults.
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    if unsafe { cstr(domainname) }.map_or(true, |s| s.to_bytes().is_empty()) {
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(p) = unsafe { ptr_mut(dirnamep) } {
            *p = ptr::null();
        }
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(p) = unsafe { ptr_mut(codesetp) } {
            *p = ptr::null();
        }
        return;
    }

    let mut modified: c_int = 0;
    let mut binding: *mut binding = with_intl_runtime(|intl| intl.domain_bindings);

    // Walk the sorted linked list looking for an existing binding.
    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    while let Some(b) = unsafe { ptr_mut(binding) } {
        // SAFETY: forwarded raw pointers satisfy `libc_strcmp`'s contract per this function's own `# Safety` requirements.
        let compare = unsafe { libc_strcmp(domainname, b.domainname.as_ptr()) };
        if compare == 0 {
            break; // Found it.
        }
        if compare < 0 {
            binding = ptr::null_mut();
            break;
        }
        binding = b.next;
    }

    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    if let Some(b) = unsafe { ptr_mut(binding) } {
        // --- Existing binding: update dirname ---
        if !dirnamep.is_null() {
            // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
            let dirname = unsafe { ptr_ref(dirnamep) }.map_or(ptr::null(), |p| *p);
            if dirname.is_null() {
                // Return the current binding.
                // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                if let Some(p) = unsafe { ptr_mut(dirnamep) } {
                    *p = b.dirname;
                }
            } else {
                let result = b.dirname;
                // SAFETY: forwarded raw pointers satisfy `libc_strcmp`'s contract per this function's own `# Safety` requirements.
                if unsafe { libc_strcmp(dirname, result) } != 0 {
                    // SAFETY: forwarded raw pointers satisfy `libc_strcmp`'s contract per this function's own `# Safety` requirements.
                    let new_result =
                        if unsafe { libc_strcmp(dirname, _nl_default_dirname.as_ptr()) } == 0 {
                            _nl_default_dirname.as_ptr() as *mut c_char
                        } else {
                            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                            unsafe { c_strdup(dirname) }
                        };
                    if !new_result.is_null() {
                        if b.dirname != _nl_default_dirname.as_ptr() as *mut c_char
                            && !b.dirname.is_null()
                        {
                            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                            unsafe { c_free(b.dirname) };
                        }
                        b.dirname = new_result;
                        modified = 1;
                    }
                }
                // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                if let Some(p) = unsafe { ptr_mut(dirnamep) } {
                    *p = b.dirname;
                }
            }
        }

        // --- Existing binding: update codeset ---
        if !codesetp.is_null() {
            // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
            let codeset = unsafe { ptr_ref(codesetp) }.map_or(ptr::null(), |p| *p);
            if codeset.is_null() {
                // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                if let Some(p) = unsafe { ptr_mut(codesetp) } {
                    *p = b.codeset;
                }
            } else {
                let result = b.codeset;
                // SAFETY: forwarded raw pointers satisfy `libc_strcmp`'s contract per this function's own `# Safety` requirements.
                if result.is_null() || unsafe { libc_strcmp(codeset, result) } != 0 {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    let new_result = unsafe { c_strdup(codeset) };
                    if !new_result.is_null() {
                        if !b.codeset.is_null() {
                            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                            unsafe { c_free(b.codeset) };
                        }
                        b.codeset = new_result;
                        modified = 1;
                    }
                }
                // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                if let Some(p) = unsafe { ptr_mut(codesetp) } {
                    *p = b.codeset;
                }
            }
        }
    // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
    } else if unsafe { ptr_ref(dirnamep) }.map_or(true, |p| (*p).is_null())
        // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
        && unsafe { ptr_ref(codesetp) }.map_or(true, |p| (*p).is_null())
    {
        // No existing binding and nothing to set -> return defaults.
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(p) = unsafe { ptr_mut(dirnamep) } {
            *p = _nl_default_dirname.as_ptr();
        }
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(p) = unsafe { ptr_mut(codesetp) } {
            *p = ptr::null();
        }
    } else {
        // --- Create a new binding ---
        // SAFETY: forwarded raw pointers satisfy `libc_strlen`'s contract per this function's own `# Safety` requirements.
        let len = unsafe { libc_strlen(domainname) } + 1;
        // Allocate binding + flexible array domainname at the end.
        let struct_size = std::mem::size_of::<binding>();
        let layout = Layout::from_size_align(struct_size + len, std::mem::align_of::<binding>())
            .unwrap_or_else(|_| Layout::new::<u8>());
        // Allocate `struct_size + len` bytes: the binding plus its name.
        let new_binding = unsafe { std::alloc::alloc(layout) } as *mut binding;

        if new_binding.is_null() {
            // Allocation failed.
            // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
            if let Some(p) = unsafe { ptr_mut(dirnamep) } {
                *p = ptr::null();
            }
            // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
            if let Some(p) = unsafe { ptr_mut(codesetp) } {
                *p = ptr::null();
            }
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        } else if let Some(nb) = unsafe { ptr_mut(new_binding) } {
            if let (Some(dst), Some(src)) = (
                // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
                unsafe { slice_mut(nb.domainname.as_mut_ptr() as *mut u8, len) },
                // SAFETY: forwarded raw pointers satisfy `slice_ref`'s contract per this function's own `# Safety` requirements.
                unsafe { slice_ref(domainname as *const u8, len) },
            ) {
                dst.copy_from_slice(src);
            }

            // --- Set dirname ---
            if !dirnamep.is_null() {
                // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
                let mut dirname = unsafe { ptr_ref(dirnamep) }.map_or(ptr::null(), |p| *p);
                // SAFETY: forwarded raw pointers satisfy `libc_strcmp`'s contract per this function's own `# Safety` requirements.
                if dirname.is_null()
                    || unsafe { libc_strcmp(dirname, _nl_default_dirname.as_ptr()) } == 0
                {
                    dirname = _nl_default_dirname.as_ptr();
                } else {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    let result = unsafe { c_strdup(dirname) };
                    if result.is_null() {
                        // Failed to strdup dirname; free the new binding.
                        unsafe { std::alloc::dealloc(new_binding as *mut u8, layout) };
                        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                        if let Some(p) = unsafe { ptr_mut(dirnamep) } {
                            *p = ptr::null();
                        }
                        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                        if let Some(p) = unsafe { ptr_mut(codesetp) } {
                            *p = ptr::null();
                        }
                    } else {
                        dirname = result;
                    }
                }
                // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                if let Some(p) = unsafe { ptr_mut(dirnamep) } {
                    *p = dirname;
                }
                nb.dirname = dirname as *mut c_char;
            } else {
                nb.dirname = _nl_default_dirname.as_ptr() as *mut c_char;
            }

            // --- Set codeset ---
            if !codesetp.is_null() {
                // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
                let mut codeset = unsafe { ptr_ref(codesetp) }.map_or(ptr::null(), |p| *p);
                if !codeset.is_null() {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    let result = unsafe { c_strdup(codeset) };
                    if result.is_null() {
                        // Failed to strdup codeset; free the new binding.
                        if nb.dirname != _nl_default_dirname.as_ptr() as *mut c_char
                            && !nb.dirname.is_null()
                        {
                            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                            unsafe { c_free(nb.dirname) };
                        }
                        unsafe { std::alloc::dealloc(new_binding as *mut u8, layout) };
                        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                        if let Some(p) = unsafe { ptr_mut(dirnamep) } {
                            *p = ptr::null();
                        }
                        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                        if let Some(p) = unsafe { ptr_mut(codesetp) } {
                            *p = ptr::null();
                        }
                    } else {
                        codeset = result;
                    }
                }
                // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                if let Some(p) = unsafe { ptr_mut(codesetp) } {
                    *p = codeset;
                }
                nb.codeset = codeset as *mut c_char;
            } else {
                nb.codeset = ptr::null_mut();
            }

            // --- Enqueue the new binding in sorted order ---
            let head = with_intl_runtime(|intl| intl.domain_bindings);
            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
            let head_name = unsafe { ptr_ref(head) }.map_or(ptr::null(), |h| h.domainname.as_ptr());
            // SAFETY: forwarded raw pointers satisfy `libc_strcmp`'s contract per this function's own `# Safety` requirements.
            if head_name.is_null() || unsafe { libc_strcmp(domainname, head_name) } < 0 {
                nb.next = head;
                with_intl_runtime(|intl| intl.domain_bindings = new_binding);
            } else {
                let mut cur = head;
                // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                while let Some(c) = unsafe { ptr_mut(cur) } {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    let next_name =
                        unsafe { ptr_ref(c.next) }.map_or(ptr::null(), |n| n.domainname.as_ptr());
                    // SAFETY: forwarded raw pointers satisfy `libc_strcmp`'s contract per this function's own `# Safety` requirements.
                    if next_name.is_null() || unsafe { libc_strcmp(domainname, next_name) } <= 0 {
                        break;
                    }
                    cur = c.next;
                }
                // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                nb.next = unsafe { ptr_mut(cur) }.map_or(ptr::null_mut(), |c| c.next);
                // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                if let Some(c) = unsafe { ptr_mut(cur) } {
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
///
/// SOUNDNESS GAP (tracked): `domainname` and `dirname` must each be null or a
/// NUL-terminated string, but this keeps a safe signature because
/// `mainutils::errors::messages` (owned by another change) calls it without
/// `unsafe`. Once that caller wraps the call, this should become `unsafe fn`.
pub fn libintl_bindtextdomain(domainname: *const c_char, dirname: *const c_char) -> *mut c_char {
    let mut dirname_mut: *const c_char = dirname;
    // SAFETY: not enforced by the signature -- see the doc comment above.
    unsafe { set_binding_values(domainname, &mut dirname_mut, ptr::null_mut()) };
    dirname_mut as *mut c_char
}

/// Specify the character encoding in which messages from the DOMAINNAME
/// message catalog will be returned.
///
/// Returns the current codeset for the domain (may be the newly set value).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn libintl_bind_textdomain_codeset(
    domainname: *const c_char,
    codeset: *const c_char,
) -> *mut c_char {
    let mut codeset_mut: *const c_char = codeset;
    // SAFETY: forwarded raw pointers satisfy `set_binding_values`'s contract per this function's own `# Safety` requirements.
    unsafe { set_binding_values(domainname, ptr::null_mut(), &mut codeset_mut) };
    codeset_mut as *mut c_char
}

// ---------------------------------------------------------------------------
// Helper functions (private, replacing libc / string.h)
// ---------------------------------------------------------------------------

use std::alloc::Layout;

/// Equivalent of `strlen` for a NUL-terminated C string.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn libc_strlen(s: *const c_char) -> usize {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    unsafe { cstr(s) }.map_or(0, |s| s.to_bytes().len())
}

/// Equivalent of `strcmp`.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn libc_strcmp(a: *const c_char, b: *const c_char) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let (Some(x), Some(y)) = (unsafe { cstr(a) }, unsafe { cstr(b) }) else {
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
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert_eq!(unsafe { libc_strcmp(a, b) }, 0);
    }

    #[test]
    fn test_libc_strcmp_less() {
        let a = b"abc\0".as_ptr() as *const c_char;
        let b = b"abd\0".as_ptr() as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert!(unsafe { libc_strcmp(a, b) } < 0);
    }

    #[test]
    fn test_libc_strcmp_greater() {
        let a = b"abd\0".as_ptr() as *const c_char;
        let b = b"abc\0".as_ptr() as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert!(unsafe { libc_strcmp(a, b) } > 0);
    }

    #[test]
    fn test_libc_strlen() {
        let s = b"hello\0".as_ptr() as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert_eq!(unsafe { libc_strlen(s) }, 5);
    }

    #[test]
    fn test_libc_strlen_null() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert_eq!(unsafe { libc_strlen(ptr::null()) }, 0);
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
