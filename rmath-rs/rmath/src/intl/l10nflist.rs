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
use std::ffi::CStr;
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
unsafe fn __argz_count(argz: *const c_char, len: usize) -> usize {
    unsafe {
        if argz.is_null() || len == 0 {
            return 0;
        }
        let mut count = 0usize;
        let mut remaining = len;
        let mut p = argz;
        while remaining > 0 {
            let s = CStr::from_ptr(p);
            let part_len = s.to_bytes_with_nul().len();
            p = p.add(part_len);
            remaining -= part_len;
            count += 1;
        }
        count
    }
}

/// Convert an argz vector to a printable string by replacing NUL bytes
/// with the given separator character.
unsafe fn __argz_stringify(argz: *mut c_char, len: usize, sep: c_char) {
    unsafe {
        if argz.is_null() || len == 0 {
            return;
        }
        let mut remaining = len;
        let mut p = argz;
        while remaining > 0 {
            let s = CStr::from_ptr(p);
            let part_len = s.to_bytes_with_nul().len();
            p = p.add(part_len);
            remaining -= part_len;
            if remaining > 0 {
                *p.sub(1) = sep;
            }
        }
    }
}

/// Return the next entry in an argz vector after `entry`.
///
/// If `entry` is null, returns the first entry. Returns null after the last.
unsafe fn __argz_next(argz: *const c_char, argz_len: usize, entry: *const c_char) -> *const c_char {
    unsafe {
        if entry.is_null() {
            if argz_len > 0 && !argz.is_null() {
                return argz;
            }
            return ptr::null();
        }
        let end = if argz.is_null() {
            ptr::null()
        } else {
            argz.add(argz_len)
        };
        if entry < end {
            let next = CStr::from_ptr(entry).to_bytes_with_nul().len();
            let next_ptr = entry.add(next);
            if next_ptr >= end {
                return ptr::null();
            }
            return next_ptr;
        }
        ptr::null()
    }
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

/// Copy a C string, returning a pointer to the NUL terminator.
unsafe fn stpcpy(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    unsafe {
        let mut d = dest;
        let mut s = src;
        while *s != 0 {
            *d = *s;
            d = d.add(1);
            s = s.add(1);
        }
        *d = 0;
        d
    }
}

/// Check whether a path is absolute (Unix).
#[cfg(unix)]
unsafe fn IS_ABSOLUTE_PATH(p: *const c_char) -> bool {
    !p.is_null() && unsafe { *p == b'/' as c_char }
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
pub unsafe fn _nl_make_l10nflist(
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
    unsafe {
        if language.is_null() || filename.is_null() {
            return ptr::null_mut();
        }

        let lang_len = CStr::from_ptr(language).to_bytes().len();
        let file_len = CStr::from_ptr(filename).to_bytes().len();

        // If language contains an absolute path, ignore dirlist.
        let effective_dirlist_len = if IS_ABSOLUTE_PATH(language) {
            0
        } else {
            dirlist_len
        };

        let territory_len = if (mask & XPG_TERRITORY) != 0 && !territory.is_null() {
            CStr::from_ptr(territory).to_bytes().len()
        } else {
            0
        };
        let codeset_len = if (mask & XPG_CODESET) != 0 && !codeset.is_null() {
            CStr::from_ptr(codeset).to_bytes().len()
        } else {
            0
        };
        let norm_codeset_len = if (mask & XPG_NORM_CODESET) != 0 && !normalized_codeset.is_null() {
            CStr::from_ptr(normalized_codeset).to_bytes().len()
        } else {
            0
        };
        let modifier_len = if (mask & XPG_MODIFIER) != 0 && !modifier.is_null() {
            CStr::from_ptr(modifier).to_bytes().len()
        } else {
            0
        };

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

        // Allocate room for the full file name.
        let abs_filename =
            alloc::alloc(Layout::from_size_align(total_len, 1).expect("unwrap on None/Err"))
                as *mut c_char;
        if abs_filename.is_null() {
            return ptr::null_mut();
        }

        // Construct file name.
        let mut cp = abs_filename;

        if effective_dirlist_len > 0 && !dirlist.is_null() {
            ptr::copy_nonoverlapping(dirlist, cp, effective_dirlist_len);
            __argz_stringify(cp, effective_dirlist_len, PATH_SEPARATOR);
            cp = cp.add(effective_dirlist_len);
            *cp.sub(1) = b'/' as c_char;
        }

        cp = stpcpy(cp, language);

        if (mask & XPG_TERRITORY) != 0 && !territory.is_null() {
            *cp = b'_' as c_char;
            cp = cp.add(1);
            cp = stpcpy(cp, territory);
        }
        if (mask & XPG_CODESET) != 0 && !codeset.is_null() {
            *cp = b'.' as c_char;
            cp = cp.add(1);
            cp = stpcpy(cp, codeset);
        }
        if (mask & XPG_NORM_CODESET) != 0 && !normalized_codeset.is_null() {
            *cp = b'.' as c_char;
            cp = cp.add(1);
            cp = stpcpy(cp, normalized_codeset);
        }
        if (mask & XPG_MODIFIER) != 0 && !modifier.is_null() {
            *cp = b'@' as c_char;
            cp = cp.add(1);
            cp = stpcpy(cp, modifier);
        }

        *cp = b'/' as c_char;
        cp = cp.add(1);
        stpcpy(cp, filename);

        // Look in list of already loaded domains.
        let mut lastp = l10nfile_list;
        let mut retval: *mut loaded_l10nfile = ptr::null_mut();

        let mut scan = *l10nfile_list;
        while !scan.is_null() {
            if !(*scan).filename.is_null() {
                let abs_cstr = CStr::from_ptr(abs_filename);
                let scan_cstr = CStr::from_ptr((*scan).filename);
                let compare = abs_cstr.cmp(scan_cstr);
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
                        lastp = &mut (*scan).next;
                    }
                }
            }
            scan = (*scan).next;
        }

        if !retval.is_null() || do_allocate == 0 {
            let layout = Layout::from_size_align(total_len, 1).expect("unwrap on None/Err");
            alloc::dealloc(abs_filename as *mut u8, layout);
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
        let entry_layout =
            Layout::from_size_align(entry_size, std::mem::align_of::<loaded_l10nfile>())
                .expect("unwrap on None/Err");
        retval = alloc::alloc(entry_layout) as *mut loaded_l10nfile;
        if retval.is_null() {
            let layout = Layout::from_size_align(total_len, 1).expect("unwrap on None/Err");
            alloc::dealloc(abs_filename as *mut u8, layout);
            return ptr::null_mut();
        }

        ptr::write_bytes(retval, 0, 1);
        (*retval).filename = abs_filename;
        (*retval).decided = if dirlist_count > 1 { 1 } else { 0 }
            | if (mask & XPG_CODESET) != 0 && (mask & XPG_NORM_CODESET) != 0 {
                1
            } else {
                0
            };
        (*retval).data = ptr::null();
        (*retval).next = *lastp;
        *lastp = retval;

        retval
    }
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
pub unsafe fn _nl_normalize_codeset(codeset: *const c_char, name_len: usize) -> *const c_char {
    unsafe {
        if codeset.is_null() || name_len == 0 {
            let layout = Layout::from_size_align(1, 1).expect("unwrap on None/Err");
            let p = alloc::alloc(layout) as *mut c_char;
            if !p.is_null() {
                *p = 0;
            }
            return p;
        }

        let src = std::slice::from_raw_parts(codeset as *const u8, name_len);

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
        let layout = Layout::from_size_align(alloc_len, 1).expect("unwrap on None/Err");
        let retval = alloc::alloc(layout) as *mut c_char;
        if retval.is_null() {
            return ptr::null();
        }

        let dst = std::slice::from_raw_parts_mut(retval as *mut u8, alloc_len);
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

        retval
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        unsafe {
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
