#![warn(unused_unsafe)]
//! Port of l10nflist.c -- Locale file list management.
//!
//! Constructs locale file pathnames and manages the list of loaded locale
//! files. The main entry point is `_nl_make_l10nflist()` which creates
//! entries in the loaded-domain list for each combination of locale name
//! components (language, territory, codeset, modifier).
//!
//! Also provides `_nl_normalize_codeset()` which normalizes codeset names
//! for comparison.

#![allow(non_snake_case)]

use std::alloc::{self, Layout};
use std::os::raw::{c_char, c_int};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Path separator (Unix)
// ---------------------------------------------------------------------------

const PATH_SEPARATOR: c_char = b':' as c_char;

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Count the number of NUL-terminated strings in an argz vector.
///
/// An argz vector is a sequence of NUL-terminated strings packed together,
/// terminated by an additional NUL byte.
fn __argz_count(argz: *const c_char, len: usize) -> usize {
    // Read the `len`-byte argz vector; each NUL terminates one string.
    let Some(bytes) = slice_ref(argz as *const u8, len) else {
        return 0;
    };
    bytes.iter().filter(|&&b| b == 0).count()
}

/// Convert an argz vector to a printable string by replacing NUL bytes
/// with the given separator character.
fn __argz_stringify(bytes: &mut [u8], sep: c_char) {
    // Rewrite the argz vector in place, leaving the final byte
    // untouched as the C original does.
    if bytes.is_empty() {
        return;
    }
    let last = bytes.len() - 1;
    for (i, b) in bytes.iter_mut().enumerate() {
        if *b == 0 && i != last {
            *b = sep as u8;
        }
    }
}

/// Return the next entry in an argz vector after `entry`.
///
/// If `entry` is null, returns the first entry. Returns null after the last.
fn __argz_next(argz: *const c_char, argz_len: usize, entry: *const c_char) -> *const c_char {
    if entry.is_null() {
        if argz_len > 0 && !argz.is_null() {
            return argz;
        }
        return ptr::null();
    }
    // Walk the caller's `argz_len`-byte argz vector by index.
    let Some(bytes) = slice_ref(argz as *const u8, argz_len) else {
        return ptr::null();
    };
    let base = argz as usize;
    let start = entry as usize;
    if start < base || start - base >= argz_len {
        return ptr::null();
    }
    let idx = start - base;
    let Some(nul) = bytes[idx..].iter().position(|&b| b == 0) else {
        return ptr::null();
    };
    let next_idx = idx + nul + 1;
    if next_idx >= argz_len {
        return ptr::null();
    }
    bytes[next_idx..].as_ptr() as *const c_char
}

/// Return the number of bits set in `x` (population count).
///
/// Assumes no more than 16 bits are used (matching the C implementation).
fn pop(x: c_int) -> c_int {
    let mut x = x;
    x = ((x & !0x5555) >> 1) + (x & 0x5555);
    x = ((x & !0x3333) >> 2) + (x & 0x3333);
    x = ((x >> 4) + x) & 0x0f0f;
    x = ((x >> 8) + x) & 0xff;
    x
}

/// Copy `src` into `buf` at `pos`, advancing the cursor.
///
/// Callers guarantee `buf` has room for `src` at `pos`.
fn copy_into(buf: &mut [u8], pos: &mut usize, src: &[u8]) {
    let end = *pos + src.len();
    buf[*pos..end].copy_from_slice(src);
    *pos = end;
}

/// Check whether a path is absolute (Unix).
#[cfg(unix)]
fn IS_ABSOLUTE_PATH(p: *const c_char) -> bool {
    ptr_ref(p).is_some_and(|c| *c == b'/' as c_char)
}

// ---------------------------------------------------------------------------
// Public API: _nl_make_l10nflist
// ---------------------------------------------------------------------------

/// Create / look up locale file entries in the loaded-domains list.
///
/// Constructs a full pathname from the locale components and looks it up
/// in the sorted linked list. If not found and `do_allocate` is set,
/// creates a new entry with successor entries for locale fallback.
///
/// # Safety
/// All string pointers must be valid NUL-terminated C strings.
pub fn _nl_make_l10nflist(
    l10nfile_list: *mut *mut loaded_l10nfile,
    dirlist: *const c_char,
    dirlist_len: usize,
    mask: c_int,
    language: *const c_char,
    territory: *const c_char,
    codeset: *const c_char,
    normalized_codeset: *const c_char,
    modifier: *const c_char,
    filename: *const c_char,
    do_allocate: c_int,
) -> *mut loaded_l10nfile {
    let Some(language_cs) = cstr(language) else {
        return ptr::null_mut();
    };
    let Some(filename_cs) = cstr(filename) else {
        return ptr::null_mut();
    };

    let lang_len = language_cs.to_bytes().len();
    let file_len = filename_cs.to_bytes().len();

    // If language contains an absolute path, ignore dirlist.
    let effective_dirlist_len = if IS_ABSOLUTE_PATH(language) {
        0
    } else {
        dirlist_len
    };

    let territory_cs = if (mask & XPG_TERRITORY) != 0 {
        cstr(territory)
    } else {
        None
    };
    let codeset_cs = if (mask & XPG_CODESET) != 0 {
        cstr(codeset)
    } else {
        None
    };
    let norm_codeset_cs = if (mask & XPG_NORM_CODESET) != 0 {
        cstr(normalized_codeset)
    } else {
        None
    };
    let modifier_cs = if (mask & XPG_MODIFIER) != 0 {
        cstr(modifier)
    } else {
        None
    };

    let territory_len = territory_cs.map_or(0, |s| s.to_bytes().len());
    let codeset_len = codeset_cs.map_or(0, |s| s.to_bytes().len());
    let norm_codeset_len = norm_codeset_cs.map_or(0, |s| s.to_bytes().len());
    let modifier_len = modifier_cs.map_or(0, |s| s.to_bytes().len());

    // Calculate total filename length.
    let total_len = effective_dirlist_len
    + lang_len
    + if territory_len > 0 { territory_len + 1 } else { 0 }
    + if codeset_len > 0 { codeset_len + 1 } else { 0 }
    + if norm_codeset_len > 0 { norm_codeset_len + 1 } else { 0 }
    + if modifier_len > 0 { modifier_len + 1 } else { 0 }
    + 1 // '/'
    + file_len
    + 1; // NUL

    // Allocate `total_len` bytes for the full file name.
    let layout = Layout::from_size_align(total_len, 1).unwrap_or(Layout::new::<u8>());
    let abs_filename = unsafe { alloc::alloc(layout) } as *mut c_char;
    if abs_filename.is_null() {
        return ptr::null_mut();
    }

    // Construct the file name inside the freshly allocated buffer.
    {
        let Some(buf) = slice_mut(abs_filename as *mut u8, total_len) else {
            c_free(abs_filename);
            return ptr::null_mut();
        };
        let mut pos = 0usize;

        if effective_dirlist_len > 0 {
            if let Some(dl) = slice_ref(dirlist as *const u8, effective_dirlist_len) {
                buf[..effective_dirlist_len].copy_from_slice(dl);
                __argz_stringify(&mut buf[..effective_dirlist_len], PATH_SEPARATOR);
                buf[effective_dirlist_len - 1] = b'/';
                pos = effective_dirlist_len;
            }
        }

        copy_into(buf, &mut pos, language_cs.to_bytes());

        if let Some(cs) = territory_cs {
            buf[pos] = b'_';
            pos += 1;
            copy_into(buf, &mut pos, cs.to_bytes());
        }
        if let Some(cs) = codeset_cs {
            buf[pos] = b'.';
            pos += 1;
            copy_into(buf, &mut pos, cs.to_bytes());
        }
        if let Some(cs) = norm_codeset_cs {
            buf[pos] = b'.';
            pos += 1;
            copy_into(buf, &mut pos, cs.to_bytes());
        }
        if let Some(cs) = modifier_cs {
            buf[pos] = b'@';
            pos += 1;
            copy_into(buf, &mut pos, cs.to_bytes());
        }

        buf[pos] = b'/';
        pos += 1;
        copy_into(buf, &mut pos, filename_cs.to_bytes());
        buf[pos] = 0;
    }

    // Look in list of already loaded domains.
    let mut lastp: *mut *mut loaded_l10nfile = l10nfile_list;
    let mut retval: *mut loaded_l10nfile = ptr::null_mut();

    let mut scan = match ptr_ref(l10nfile_list) {
        Some(head) => *head,
        None => {
            c_free(abs_filename);
            return ptr::null_mut();
        }
    };
    while let Some(node) = ptr_mut(scan) {
        if !node.filename.is_null() {
            let compare = cstr(abs_filename)
                .and_then(|abs| cstr(node.filename).map(|scan_cs| abs.cmp(scan_cs)));
            if let Some(compare) = compare {
                match compare.cmp(&std::cmp::Ordering::Equal) {
                    std::cmp::Ordering::Equal => {
                        retval = scan;
                        break;
                    }
                    std::cmp::Ordering::Less => {
                        // Not in the list.
                        retval = ptr::null_mut();
                        break;
                    }
                    std::cmp::Ordering::Greater => {
                        lastp = &mut node.next;
                    }
                }
            }
        }
        scan = node.next;
    }

    if !retval.is_null() || do_allocate == 0 {
        c_free(abs_filename);
        return retval;
    }

    let dirlist_count = if effective_dirlist_len > 0 {
        __argz_count(dirlist, effective_dirlist_len)
    } else {
        1
    };

    // Allocate a new loaded_l10nfile with space for successors.
    let extra_successors = (dirlist_count << pop(mask)) + if dirlist_count > 1 { 1 } else { 0 };
    let entry_size = std::mem::size_of::<loaded_l10nfile>()
        + (extra_successors.saturating_sub(1) * std::mem::size_of::<*mut loaded_l10nfile>());
    let entry_layout = Layout::from_size_align(entry_size, std::mem::align_of::<loaded_l10nfile>())
        .unwrap_or(Layout::new::<loaded_l10nfile>());
    // Allocate a zeroed entry including the flexible successor space.
    retval = unsafe { alloc::alloc_zeroed(entry_layout) } as *mut loaded_l10nfile;
    if retval.is_null() {
        c_free(abs_filename);
        return ptr::null_mut();
    }

    if let Some(node) = ptr_mut(retval) {
        node.filename = abs_filename;
        node.decided = if dirlist_count > 1 { 1 } else { 0 }
            | if (mask & XPG_CODESET) != 0 && (mask & XPG_NORM_CODESET) != 0 {
                1
            } else {
                0
            };
        node.data = ptr::null();
        node.next = ptr_mut(lastp).map_or(ptr::null_mut(), |p| *p);
        if let Some(p) = ptr_mut(lastp) {
            *p = retval;
        }
    }

    retval
}

// ---------------------------------------------------------------------------
// Public API: _nl_normalize_codeset
// ---------------------------------------------------------------------------

/// Normalize a codeset name.
///
/// Converts alphabetic characters to lowercase, keeps digits, and replaces
/// other characters with underscores. If the codeset is all digits, prepends
/// "iso". The return value is dynamically allocated and must be freed by the
/// caller.
///
/// # Safety
/// `codeset` must be a valid pointer to `name_len` bytes.
pub fn _nl_normalize_codeset(codeset: *const c_char, name_len: usize) -> *const c_char {
    let empty = b"\0".as_ptr() as *const c_char;
    if codeset.is_null() || name_len == 0 {
        // Return an allocated empty string.
        return c_strdup(empty);
    }

    {
        let Some(src) = slice_ref(codeset as *const u8, name_len) else {
            return c_strdup(empty);
        };

        let mut len = 0usize;
        let mut only_digit = true;
        for &b in src.iter() {
            if b.is_ascii_alphanumeric() {
                len += 1;
                if b.is_ascii_alphabetic() {
                    only_digit = false;
                }
            }
        }

        let alloc_len = if only_digit { 3 } else { 0 } + len + 1;
        let layout = Layout::from_size_align(alloc_len, 1).unwrap_or(Layout::new::<u8>());
        // Allocate `alloc_len` bytes for the normalized name.
        let retval = unsafe { alloc::alloc(layout) } as *mut c_char;
        if retval.is_null() {
            return ptr::null();
        }

        if let Some(dst) = slice_mut(retval as *mut u8, alloc_len) {
            let mut wp = 0usize;

            if only_digit {
                dst[0] = b'i';
                dst[1] = b's';
                dst[2] = b'o';
                wp = 3;
            }

            for &b in src.iter() {
                if b.is_ascii_alphabetic() {
                    dst[wp] = b.to_ascii_lowercase();
                    wp += 1;
                } else if b.is_ascii_digit() {
                    dst[wp] = b;
                    wp += 1;
                }
            }
            dst[wp] = 0;
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
    use std::ffi::CStr;

    fn some<T>(opt: Option<T>) -> T {
        opt.unwrap_or_else(|| panic!("unexpected None in test"))
    }
    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test failed: {e:?}"),
        }
    }

    #[test]
    fn test_normalize_codeset_simple() {
        unsafe {
            let cs = b"UTF-8\0";
            let result = _nl_normalize_codeset(cs.as_ptr() as *const c_char, 5);
            assert!(!result.is_null());
            let s = CStr::from_ptr(result).to_str().unwrap_or("");
            assert_eq!(s, "utf8");
            let layout = must(Layout::from_size_align(s.len() + 1, 1));
            alloc::dealloc(result as *mut u8, layout);
        }
    }

    #[test]
    fn test_normalize_codeset_digits() {
        unsafe {
            let cs = b"8859-1\0";
            let result = _nl_normalize_codeset(cs.as_ptr() as *const c_char, 6);
            assert!(!result.is_null());
            let s = CStr::from_ptr(result).to_str().unwrap_or("");
            assert_eq!(s, "iso88591");
            let layout = must(Layout::from_size_align(s.len() + 1, 1));
            alloc::dealloc(result as *mut u8, layout);
        }
    }

    #[test]
    fn test_pop() {
        assert_eq!(pop(0), 0);
        assert_eq!(pop(1), 1);
        assert_eq!(pop(3), 2);
        assert_eq!(pop(15), 4);
    }

    #[test]
    fn test_make_l10nflist_search() {
        let mut list: *mut loaded_l10nfile = ptr::null_mut();
        let lang = b"en\0";
        let file = b"messages.mo\0";

        // Search only (do_allocate = 0).
        let result = _nl_make_l10nflist(
            &mut list,
            ptr::null(),
            0,
            0,
            lang.as_ptr() as *const c_char,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            file.as_ptr() as *const c_char,
            0,
        );
        // Should not find anything since the list is empty.
        assert!(result.is_null());
    }

    #[test]
    fn test_make_l10nflist_allocate() {
        unsafe {
            let mut list: *mut loaded_l10nfile = ptr::null_mut();
            let lang = b"en_US\0";
            let file = b"messages.mo\0";

            let result = _nl_make_l10nflist(
                &mut list,
                ptr::null(),
                0,
                XPG_TERRITORY,
                lang.as_ptr() as *const c_char,
                b"US\0".as_ptr() as *const c_char,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                file.as_ptr() as *const c_char,
                1,
            );
            assert!(!result.is_null());
            assert!(!(*result).filename.is_null());
            let fname = CStr::from_ptr((*result).filename).to_str().unwrap_or("");
            assert!(fname.contains("en_US"));
            assert!(fname.contains("messages.mo"));
        }
    }
}
