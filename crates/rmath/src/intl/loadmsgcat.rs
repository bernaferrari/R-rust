#![warn(unused_unsafe)]
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
use std::fs::File;
use std::io::Read as IoRead;
use std::os::raw::{c_char, c_ulong, c_void};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Helper: read a .mo file from disk (no mmap)
// ---------------------------------------------------------------------------

/// Read the entire contents of a .mo file into a malloc'd buffer.
///
/// Returns a pointer to the buffer and sets `*sizep` to the file size.
/// Returns null on failure.
fn read_mo_file(filename: *const c_char, sizep: *mut usize) -> *mut c_char {
    let (Some(name), Some(out_size)) = (cstr(filename), ptr_mut(sizep)) else {
        return ptr::null_mut();
    };
    let Ok(path) = name.to_str() else {
        return ptr::null_mut();
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
    let layout = Layout::from_size_align(size, 1).unwrap_or(Layout::new::<u8>());
    // Allocate `size` bytes for the catalog contents.
    let buf = unsafe { alloc::alloc(layout) } as *mut c_char;
    if buf.is_null() {
        return ptr::null_mut();
    }
    if let Some(dst) = slice_mut(buf as *mut u8, size) {
        dst.copy_from_slice(&data);
    }
    *out_size = size;
    buf
}

/// Read an unaligned u32 from the packed binary .mo catalog.
///
/// `p` must point to at least 4 readable bytes inside the catalog.
fn mo_u32(p: *const u8) -> nls_uint32 {
    // Catalog fields are byte-packed; `p` is valid per the caller's bounds.
    unsafe { ptr::read_unaligned(p as *const nls_uint32) }
}

/// Free a buffer previously allocated with `layout`.
fn free_buffer(p: *mut u8, layout: Layout) {
    // Free the block this module allocated with the matching layout.
    unsafe { alloc::dealloc(p, layout) }
}

// ---------------------------------------------------------------------------
// Helper: get system-dependent string segments (stub)
// ---------------------------------------------------------------------------

/// Get the segments for system-dependent strings.
///
/// In the C implementation, this looks for the "sysdep" field in the .mo
/// file header. For the standalone port, we return NULL (no sysdep strings).
fn get_sysdep_string_segments(
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
pub fn _nl_load_domain(domain_file: *mut loaded_l10nfile, _domainbinding: *mut binding) {
    let Some(df) = ptr_mut(domain_file) else {
        return;
    };

    let filename = df.filename;
    if filename.is_null() {
        df.decided = 1;
        df.data = ptr::null();
        return;
    }

    // Read the .mo file.
    let mut file_size: usize = 0;
    let data = read_mo_file(filename, &mut file_size);
    if data.is_null() {
        df.decided = 1;
        df.data = ptr::null();
        return;
    }
    let data_layout = Layout::from_size_align(file_size, 1).unwrap_or(Layout::new::<u8>());

    // Allocate a zeroed loaded_domain structure.
    let domain_layout = Layout::new::<loaded_domain>();
    let domain = unsafe { alloc::alloc_zeroed(domain_layout) } as *mut loaded_domain;
    if domain.is_null() {
        // Free the `file_size`-byte catalog buffer from read_mo_file.
        free_buffer(data as *mut u8, data_layout);
        df.decided = 1;
        df.data = ptr::null();
        return;
    }
    let Some(d) = ptr_mut(domain) else {
        return;
    };

    d.data = data;
    d.use_mmap = 0;
    d.mmap_size = file_size;
    d.must_swap = 0;
    d.malloced = ptr::null_mut();

    // Validate the magic number.
    if file_size < 24 {
        // File too small to be a valid .mo file; free both buffers.
        free_buffer(data as *mut u8, data_layout);
        free_buffer(domain as *mut u8, domain_layout);
        df.decided = 1;
        df.data = ptr::null();
        return;
    }

    let magic = mo_u32(data as *const u8);

    if magic == MO_MAGIC {
        d.must_swap = 0;
    } else if magic == MO_MAGIC_SWAPPED {
        d.must_swap = 1;
    } else {
        // Invalid magic number; free both buffers.
        free_buffer(data as *mut u8, data_layout);
        free_buffer(domain as *mut u8, domain_layout);
        df.decided = 1;
        df.data = ptr::null();
        return;
    }

    // Read the header fields.
    let read_u32 = |offset: usize| -> nls_uint32 {
        let val = mo_u32(data.wrapping_add(offset) as *const u8);
        if d.must_swap != 0 {
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

    d.nstrings = nstrings;
    d.orig_tab = data.wrapping_add(orig_table_offset as usize) as *const string_desc;
    d.trans_tab = data.wrapping_add(trans_table_offset as usize) as *const string_desc;
    d.hash_size = hash_table_size;
    d.hash_tab = if hash_table_size > 0 {
        data.wrapping_add(hash_table_offset as usize) as *const nls_uint32
    } else {
        ptr::null()
    };
    d.must_swap_hash_tab = 0;

    // Handle system-dependent strings (stub).
    d.n_sysdep_strings = 0;
    d.orig_sysdep_tab = ptr::null();
    d.trans_sysdep_tab = ptr::null();

    // Get the null entry (metadata) from the catalog.
    let nullentry = if nstrings > 0 {
        let tab = d.orig_tab as *const u8;
        let mut null_length = mo_u32(tab);
        let mut null_offset = mo_u32(tab.wrapping_add(4));
        if d.must_swap != 0 {
            null_length = SWAP(null_length);
            null_offset = SWAP(null_offset);
        }
        if null_length == 0 {
            ptr::null()
        } else {
            data.wrapping_add(null_offset as usize)
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

    d.plural = plural;
    d.nplurals = nplurals;

    // Initialize conversions.
    d.conversions = ptr::null_mut();
    d.nconversions = 0;

    // Get sysdep segments.
    let _segments = get_sysdep_string_segments(domain, nullentry);

    // Mark as loaded.
    df.data = domain as *const c_void;
    df.decided = 1;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_domain_null_file() {
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

    #[test]
    fn test_load_domain_missing_file() {
        unsafe {
            let fname = b"/nonexistent/path/messages.mo\0";
            let layout =
                Layout::from_size_align(fname.len(), 1).unwrap_or_else(|e| panic!("layout: {e:?}"));
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
        let mut size: usize = 0;
        let result = read_mo_file(ptr::null(), &mut size);
        assert!(result.is_null());
    }
}
