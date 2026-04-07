//! Port of loadmsgcat.c -- Message catalog loading.
//!
//! Loads a .mo (Machine Object) message catalog file into memory and sets up
//! the hash table for fast string lookup. The .mo file format is defined by
//! the GNU gettext project.
//!
//! For the standalone Rust port, we provide a simplified implementation that
//! handles the basic .mo file format with stubs for mmap and iconv.

#![allow(non_snake_case)]

use std::alloc::{self, Layout};
use std::cell::Cell;
use std::ffi::CStr;
use std::fs::File;
use std::io::Read as IoRead;
use std::os::raw::{c_char, c_ulong, c_void};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

thread_local! { static _nl_msg_cat_cntr_lock: Cell<[u8; 0]> = Cell::new([]); }

// ---------------------------------------------------------------------------
// Helper: read a .mo file from disk (no mmap)
// ---------------------------------------------------------------------------

/// Read the entire contents of a .mo file into a malloc'd buffer.
///
/// Returns a pointer to the buffer and sets `*sizep` to the file size.
/// Returns null on failure.
unsafe fn read_mo_file(filename: *const c_char, sizep: *mut usize) -> *mut c_char {
    unsafe {
        if filename.is_null() || sizep.is_null() {
            return ptr::null_mut();
        }

        let path = match CStr::from_ptr(filename).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        };

        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return ptr::null_mut(),
        };

        let mut data = Vec::new();
        if file.read_to_end(&mut data).is_err() {
            return ptr::null_mut();
        }

        // Add a NUL terminator at the end for safety.
        data.push(0);

        let size = data.len();
        let layout = Layout::from_size_align(size, 1).expect("unwrap on None/Err");
        let buf = alloc::alloc(layout) as *mut c_char;
        if buf.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, size);
        *sizep = size;
        buf
    }
}

// ---------------------------------------------------------------------------
// Helper: get system-dependent string segments (stub)
// ---------------------------------------------------------------------------

/// Get the segments for system-dependent strings.
///
/// In the C implementation, this looks for the "sysdep" field in the .mo
/// file header. For the standalone port, we return NULL (no sysdep strings).
unsafe fn get_sysdep_string_segments(
    _domain: *mut loaded_domain,
    _nullentry: *const c_char,
) -> *const c_ulong {
    ptr::null()
}

// ---------------------------------------------------------------------------
// Public API: _nl_load_domain
// ---------------------------------------------------------------------------

/// Load a message domain from its .mo file.
///
/// Reads the .mo file, validates its header, sets up the string tables
/// and hash table, and extracts the plural expression.
///
/// # Safety
/// `domain_file` must be a valid pointer to a `loaded_l10nfile` struct.
/// `domainbinding` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _nl_load_domain(
    domain_file: *mut loaded_l10nfile,
    _domainbinding: *mut binding,
) {
    unsafe {
        if domain_file.is_null() {
            return;
        }

        let filename = (*domain_file).filename;
        if filename.is_null() {
            (*domain_file).decided = 1;
            (*domain_file).data = ptr::null();
            return;
        }

        // Read the .mo file.
        let mut file_size: usize = 0;
        let data = read_mo_file(filename, &mut file_size);
        if data.is_null() {
            (*domain_file).decided = 1;
            (*domain_file).data = ptr::null();
            return;
        }

        // Allocate the loaded_domain structure.
        let domain_layout = Layout::new::<loaded_domain>();
        let domain = alloc::alloc(domain_layout) as *mut loaded_domain;
        if domain.is_null() {
            let layout = Layout::from_size_align(file_size, 1).expect("unwrap on None/Err");
            alloc::dealloc(data as *mut u8, layout);
            (*domain_file).decided = 1;
            (*domain_file).data = ptr::null();
            return;
        }
        ptr::write_bytes(domain, 0, 1);

        (*domain).data = data;
        (*domain).use_mmap = 0;
        (*domain).mmap_size = file_size;
        (*domain).must_swap = 0;
        (*domain).malloced = ptr::null_mut();

        // Validate the magic number.
        if file_size < 24 {
            // File too small to be a valid .mo file.
            alloc::dealloc(
                data as *mut u8,
                Layout::from_size_align(file_size, 1).expect("unwrap on None/Err"),
            );
            alloc::dealloc(domain as *mut u8, domain_layout);
            (*domain_file).decided = 1;
            (*domain_file).data = ptr::null();
            return;
        }

        let magic = ptr::read_unaligned(data as *const nls_uint32);

        if magic == MO_MAGIC {
            (*domain).must_swap = 0;
        } else if magic == MO_MAGIC_SWAPPED {
            (*domain).must_swap = 1;
        } else {
            // Invalid magic number.
            alloc::dealloc(
                data as *mut u8,
                Layout::from_size_align(file_size, 1).expect("unwrap on None/Err"),
            );
            alloc::dealloc(domain as *mut u8, domain_layout);
            (*domain_file).decided = 1;
            (*domain_file).data = ptr::null();
            return;
        }

        // Read the header fields.
        let read_u32 = |offset: usize| -> nls_uint32 {
            let val = ptr::read_unaligned(data.add(offset) as *const nls_uint32);
            if (*domain).must_swap != 0 {
                SWAP(val)
            } else {
                val
            }
        };

        let _revision = read_u32(4);
        let nstrings = read_u32(8);
        let orig_table_offset = read_u32(12);
        let trans_table_offset = read_u32(16);
        let hash_table_size = read_u32(20);
        let hash_table_offset = read_u32(24);

        (*domain).nstrings = nstrings;
        (*domain).orig_tab = data.add(orig_table_offset as usize) as *const string_desc;
        (*domain).trans_tab = data.add(trans_table_offset as usize) as *const string_desc;
        (*domain).hash_size = hash_table_size;
        (*domain).hash_tab = if hash_table_size > 0 {
            data.add(hash_table_offset as usize) as *const nls_uint32
        } else {
            ptr::null()
        };
        (*domain).must_swap_hash_tab = 0;

        // Handle system-dependent strings (stub).
        (*domain).n_sysdep_strings = 0;
        (*domain).orig_sysdep_tab = ptr::null();
        (*domain).trans_sysdep_tab = ptr::null();

        // Get the null entry (metadata) from the catalog.
        let nullentry = if nstrings > 0 {
            let null_length = if (*domain).must_swap != 0 {
                SWAP((*(*domain).orig_tab).length)
            } else {
                (*(*domain).orig_tab).length
            };
            let null_offset = if (*domain).must_swap != 0 {
                SWAP((*(*domain).orig_tab).offset)
            } else {
                (*(*domain).orig_tab).offset
            };
            if null_length == 0 {
                ptr::null()
            } else {
                data.add(null_offset as usize)
            }
        } else {
            ptr::null()
        };

        // Get the plural expression and nplurals from the null entry.
        let mut plural: *const expression = ptr::null();
        let mut nplurals: c_ulong = 2;

        if !nullentry.is_null() {
            super::plural_exp::libintl_gettext_extract_plural(
                nullentry,
                &mut plural,
                &mut nplurals,
            );
        } else {
            // Fall back to Germanic plural.
            super::plural_exp::libintl_gettext_extract_plural(
                ptr::null(),
                &mut plural,
                &mut nplurals,
            );
        }

        (*domain).plural = plural;
        (*domain).nplurals = nplurals;

        // Initialize conversions.
        (*domain).conversions = ptr::null_mut();
        (*domain).nconversions = 0;

        // Get sysdep segments.
        let _segments = get_sysdep_string_segments(domain, nullentry);

        // Mark as loaded.
        (*domain_file).data = domain as *const c_void;
        (*domain_file).decided = 1;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_domain_null_file() {
        unsafe {
            let mut l10nfile = loaded_l10nfile {
                filename: ptr::null(),
                decided: 0,
                data: ptr::null(),
                next: ptr::null_mut(),
                successor: [ptr::null_mut()],
            };
            _nl_load_domain(&mut l10nfile, ptr::null_mut());
            assert_eq!(l10nfile.decided, 1);
            assert!(l10nfile.data.is_null());
        }
    }

    #[test]
    fn test_load_domain_missing_file() {
        unsafe {
            let fname = b"/nonexistent/path/messages.mo\0";
            let layout = Layout::from_size_align(fname.len(), 1).unwrap();
            let fname_buf = alloc::alloc(layout) as *mut c_char;
            ptr::copy_nonoverlapping(fname.as_ptr() as *const c_char, fname_buf, fname.len());

            let mut l10nfile = loaded_l10nfile {
                filename: fname_buf,
                decided: 0,
                data: ptr::null(),
                next: ptr::null_mut(),
                successor: [ptr::null_mut()],
            };
            _nl_load_domain(&mut l10nfile, ptr::null_mut());
            assert_eq!(l10nfile.decided, 1);
            assert!(l10nfile.data.is_null());

            // Cleanup filename.
            alloc::dealloc(fname_buf as *mut u8, layout);
        }
    }

    #[test]
    fn test_read_mo_file_null() {
        unsafe {
            let mut size: usize = 0;
            let result = read_mo_file(ptr::null(), &mut size);
            assert!(result.is_null());
        }
    }
}
