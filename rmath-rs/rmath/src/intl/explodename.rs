//! Port of explodename.c -- Locale name parsing.
//!
//! Split a locale name into its pieces: language, modifier, territory, codeset.
//! The locale name is destructively modified (NUL bytes are inserted).

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int};

use crate::intl::types;

/// Find the end of the language part of a locale name.
///
/// Termination symbols are '_', '.', and '@'.
///
/// # Safety
/// `name` must be a valid pointer to a NUL-terminated C string.
unsafe fn _nl_find_language(name: *mut c_char) -> *mut c_char {
    unsafe {
        let mut p = name;
        while *p != 0 && *p != b'_' as c_char && *p != b'@' as c_char && *p != b'.' as c_char {
            p = p.add(1);
        }
        p
    }
}

/// Normalize a codeset name.
///
/// In the C implementation, this converts to uppercase and replaces
/// non-alphanumeric characters with underscores. Here we provide a
/// stub that just returns a copy of the codeset.
///
/// # Safety
/// `codeset` must be a valid pointer and `name_len` must be the length
/// of the string at `codeset`.
unsafe fn _nl_normalize_codeset(codeset: *const c_char, name_len: usize) -> *mut c_char {
    unsafe {
        let layout = std::alloc::Layout::from_size_align(name_len + 1, 1).unwrap();
        let result = std::alloc::alloc(layout) as *mut c_char;
        if result.is_null() {
            return std::ptr::null_mut();
        }
        // Copy the codeset, converting to uppercase and replacing
        // non-alphanumeric characters with underscores.
        let src = std::slice::from_raw_parts(codeset as *const u8, name_len);
        let dst = std::slice::from_raw_parts_mut(result as *mut u8, name_len);
        for (i, &b) in src.iter().enumerate() {
            dst[i] = if b.is_ascii_alphanumeric() {
                b.to_ascii_uppercase()
            } else {
                b'_'
            };
        }
        *result.add(name_len) = 0;
        result
    }
}

/// Split a locale name NAME into its pieces: language, modifier, territory, codeset.
///
/// NAME gets destructively modified: NUL bytes are inserted.
/// *LANGUAGE gets assigned NAME. Each of *MODIFIER, *TERRITORY, *CODESET
/// gets assigned either a pointer into the old NAME string, or NULL.
/// *NORMALIZED_CODESET gets assigned the expanded *CODESET, if it is
/// different from *CODESET; this one is dynamically allocated and must
/// be freed by the caller.
///
/// The return value is a bitmask where each bit corresponds to one filled-in value:
/// - XPG_MODIFIER    for *MODIFIER
/// - XPG_TERRITORY   for *TERRITORY
/// - XPG_CODESET     for *CODESET
/// - XPG_NORM_CODESET for *NORMALIZED_CODESET
///
/// # Safety
/// `name` must be a valid mutable pointer to a NUL-terminated C string.
/// All output pointers must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _nl_explode_name(
    name: *mut c_char,
    language: *mut *const c_char,
    modifier: *mut *const c_char,
    territory: *mut *const c_char,
    codeset: *mut *const c_char,
    normalized_codeset: *mut *const c_char,
) -> c_int {
    unsafe {
        let mut mask: c_int = 0;

        // Initialize all output pointers to NULL.
        *modifier = std::ptr::null();
        *territory = std::ptr::null();
        *codeset = std::ptr::null();
        *normalized_codeset = std::ptr::null();

        // Determine the language part first.
        // Termination symbols are '_', '.', and '@'.
        *language = name;
        let mut cp: *mut c_char = _nl_find_language(name);

        if cp == *language as *mut c_char {
            // Language has to be specified. Use this entry as-is without exploding.
            // Perhaps it is an alias.
            let mut scan = *language as *mut c_char;
            while *scan != 0 {
                scan = scan.add(1);
            }
            cp = scan;
        } else {
            // Check for territory after '_'.
            if *cp == b'_' as c_char {
                *cp = 0;
                cp = cp.add(1);
                *territory = cp;

                while *cp != 0 && *cp != b'.' as c_char && *cp != b'@' as c_char {
                    cp = cp.add(1);
                }

                mask |= types::XPG_TERRITORY;
            }

            // Check for codeset after '.'.
            if *cp == b'.' as c_char {
                *cp = 0;
                cp = cp.add(1);
                *codeset = cp;

                while *cp != 0 && *cp != b'@' as c_char {
                    cp = cp.add(1);
                }

                mask |= types::XPG_CODESET;

                // Normalize the codeset if non-empty.
                let codeset_start = *codeset as *mut c_char;
                if cp != codeset_start && *codeset_start != 0 {
                    let codeset_len = (cp as isize - codeset_start as isize) as usize;
                    let norm = _nl_normalize_codeset(codeset_start, codeset_len);
                    if norm.is_null() {
                        return -1;
                    }

                    // Compare normalized with original.
                    let norm_cstr = std::ffi::CStr::from_ptr(norm);
                    let orig_cstr = std::ffi::CStr::from_ptr(codeset_start);
                    if norm_cstr.to_bytes() == orig_cstr.to_bytes() {
                        // Same, free the normalized copy.
                        let len = norm_cstr.to_bytes().len() + 1;
                        let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
                        std::alloc::dealloc(norm as *mut u8, layout);
                    } else {
                        *normalized_codeset = norm;
                        mask |= types::XPG_NORM_CODESET;
                    }
                }
            }
        }

        // Check for modifier after '@'.
        if *cp == b'@' as c_char {
            *cp = 0;
            cp = cp.add(1);
            *modifier = cp;

            if *cp != 0 {
                mask |= types::XPG_MODIFIER;
            }
        }

        // Clear territory flag if territory is empty.
        if !(*territory).is_null() && **territory == 0 {
            mask &= !types::XPG_TERRITORY;
        }

        // Clear codeset flag if codeset is empty.
        if !(*codeset).is_null() && **codeset == 0 {
            mask &= !types::XPG_CODESET;
        }

        mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_language() {
        unsafe {
            let mut name = b"en\0".to_vec();
            let mut lang: *const c_char = std::ptr::null();
            let mut mod_: *const c_char = std::ptr::null();
            let mut terr: *const c_char = std::ptr::null();
            let mut cs: *const c_char = std::ptr::null();
            let mut norm_cs: *const c_char = std::ptr::null();

            let mask = _nl_explode_name(
                name.as_mut_ptr() as *mut c_char,
                &mut lang,
                &mut mod_,
                &mut terr,
                &mut cs,
                &mut norm_cs,
            );

            assert_eq!(mask, 0);
            assert!(!lang.is_null());
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap(), "en");
        }
    }

    #[test]
    fn test_language_territory() {
        unsafe {
            let mut name = b"en_US\0".to_vec();
            let mut lang: *const c_char = std::ptr::null();
            let mut mod_: *const c_char = std::ptr::null();
            let mut terr: *const c_char = std::ptr::null();
            let mut cs: *const c_char = std::ptr::null();
            let mut norm_cs: *const c_char = std::ptr::null();

            let mask = _nl_explode_name(
                name.as_mut_ptr() as *mut c_char,
                &mut lang,
                &mut mod_,
                &mut terr,
                &mut cs,
                &mut norm_cs,
            );

            assert_eq!(mask, types::XPG_TERRITORY);
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap(), "en");
            assert_eq!(std::ffi::CStr::from_ptr(terr).to_str().unwrap(), "US");
        }
    }

    #[test]
    fn test_language_territory_codeset() {
        unsafe {
            let mut name = b"en_US.UTF-8\0".to_vec();
            let mut lang: *const c_char = std::ptr::null();
            let mut mod_: *const c_char = std::ptr::null();
            let mut terr: *const c_char = std::ptr::null();
            let mut cs: *const c_char = std::ptr::null();
            let mut norm_cs: *const c_char = std::ptr::null();

            let mask = _nl_explode_name(
                name.as_mut_ptr() as *mut c_char,
                &mut lang,
                &mut mod_,
                &mut terr,
                &mut cs,
                &mut norm_cs,
            );

            assert_eq!(
                mask,
                types::XPG_TERRITORY | types::XPG_CODESET | types::XPG_NORM_CODESET
            );
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap(), "en");
            assert_eq!(std::ffi::CStr::from_ptr(terr).to_str().unwrap(), "US");
            assert_eq!(std::ffi::CStr::from_ptr(cs).to_str().unwrap(), "UTF-8");
            // Normalized codeset should be set since UTF-8 != UTF_8
            assert!(!norm_cs.is_null());
            assert_eq!(std::ffi::CStr::from_ptr(norm_cs).to_str().unwrap(), "UTF_8");

            // Clean up normalized codeset.
            let len = std::ffi::CStr::from_ptr(norm_cs).to_bytes().len() + 1;
            let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
            std::alloc::dealloc(norm_cs as *mut u8, layout);
        }
    }

    #[test]
    fn test_full_locale() {
        unsafe {
            let mut name = b"de_DE.ISO-8859-1@euro\0".to_vec();
            let mut lang: *const c_char = std::ptr::null();
            let mut mod_: *const c_char = std::ptr::null();
            let mut terr: *const c_char = std::ptr::null();
            let mut cs: *const c_char = std::ptr::null();
            let mut norm_cs: *const c_char = std::ptr::null();

            let mask = _nl_explode_name(
                name.as_mut_ptr() as *mut c_char,
                &mut lang,
                &mut mod_,
                &mut terr,
                &mut cs,
                &mut norm_cs,
            );

            assert_eq!(
                mask,
                types::XPG_TERRITORY
                    | types::XPG_CODESET
                    | types::XPG_MODIFIER
                    | types::XPG_NORM_CODESET
            );
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap(), "de");
            assert_eq!(std::ffi::CStr::from_ptr(terr).to_str().unwrap(), "DE");
            assert_eq!(std::ffi::CStr::from_ptr(cs).to_str().unwrap(), "ISO-8859-1");
            assert_eq!(std::ffi::CStr::from_ptr(mod_).to_str().unwrap(), "euro");
            assert!(!norm_cs.is_null());

            // Clean up normalized codeset.
            let len = std::ffi::CStr::from_ptr(norm_cs).to_bytes().len() + 1;
            let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
            std::alloc::dealloc(norm_cs as *mut u8, layout);
        }
    }

    #[test]
    fn test_empty_language_returns_zero() {
        unsafe {
            let mut name = b"\0".to_vec();
            let mut lang: *const c_char = std::ptr::null();
            let mut mod_: *const c_char = std::ptr::null();
            let mut terr: *const c_char = std::ptr::null();
            let mut cs: *const c_char = std::ptr::null();
            let mut norm_cs: *const c_char = std::ptr::null();

            let mask = _nl_explode_name(
                name.as_mut_ptr() as *mut c_char,
                &mut lang,
                &mut mod_,
                &mut terr,
                &mut cs,
                &mut norm_cs,
            );

            // Empty name: language == cp, so cp is set to end of string, mask stays 0.
            assert_eq!(mask, 0);
        }
    }

    #[test]
    fn test_language_modifier_only() {
        unsafe {
            let mut name = b"pt@BR\0".to_vec();
            let mut lang: *const c_char = std::ptr::null();
            let mut mod_: *const c_char = std::ptr::null();
            let mut terr: *const c_char = std::ptr::null();
            let mut cs: *const c_char = std::ptr::null();
            let mut norm_cs: *const c_char = std::ptr::null();

            let mask = _nl_explode_name(
                name.as_mut_ptr() as *mut c_char,
                &mut lang,
                &mut mod_,
                &mut terr,
                &mut cs,
                &mut norm_cs,
            );

            assert_eq!(mask, types::XPG_MODIFIER);
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap(), "pt");
            assert_eq!(std::ffi::CStr::from_ptr(mod_).to_str().unwrap(), "BR");
        }
    }

    #[test]
    fn test_normalize_codeset_simple() {
        unsafe {
            let cs = b"utf-8\0";
            let result = _nl_normalize_codeset(cs.as_ptr() as *const c_char, 5);
            assert!(!result.is_null());
            let s = std::ffi::CStr::from_ptr(result);
            assert_eq!(s.to_str().unwrap(), "UTF_8");

            let layout = std::alloc::Layout::from_size_align(6, 1).unwrap();
            std::alloc::dealloc(result as *mut u8, layout);
        }
    }
}
