#![warn(unused_unsafe)]
//! Port of explodename.c -- Locale name parsing.
//!
//! Split a locale name into its pieces: language, modifier, territory, codeset.
//! The locale name is destructively modified (NUL bytes are inserted).

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::intl::types;
use crate::intl::types::{c_free, cstr, ptr_mut, slice_mut, slice_ref};

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
    let layout = std::alloc::Layout::from_size_align(name_len + 1, 1)
        .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
    // Allocate `name_len + 1` bytes for the normalized codeset.
    let result = unsafe { std::alloc::alloc(layout) } as *mut c_char;
    if result.is_null() {
        return std::ptr::null_mut();
    }
    // Copy the codeset, converting to uppercase and replacing
    // non-alphanumeric characters with underscores.
    // SAFETY: forwarded raw pointers satisfy `slice_ref`'s contract per this function's own `# Safety` requirements.
    let src = unsafe { slice_ref(codeset as *const u8, name_len) }.unwrap_or(&[]);
    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
    let Some(dst) = (unsafe { slice_mut(result as *mut u8, name_len + 1) }) else {
        return result;
    };
    for (d, &b) in dst.iter_mut().zip(src.iter()) {
        *d = if b.is_ascii_alphanumeric() {
            b.to_ascii_uppercase()
        } else {
            b'_'
        };
    }
    dst[name_len] = 0;
    result
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
pub unsafe fn _nl_explode_name(
    name: *mut c_char,
    language: *mut *const c_char,
    modifier: *mut *const c_char,
    territory: *mut *const c_char,
    codeset: *mut *const c_char,
    normalized_codeset: *mut *const c_char,
) -> c_int {
    let set_out = |p: *mut *const c_char, v: *const c_char| {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        if let Some(s) = unsafe { ptr_mut(p) } {
            *s = v;
        }
    };

    // Initialize all output pointers to NULL.
    set_out(modifier, ptr::null());
    set_out(territory, ptr::null());
    set_out(codeset, ptr::null());
    set_out(normalized_codeset, ptr::null());
    set_out(language, name);

    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(name_len) = unsafe { cstr(name) }.map(|s| s.to_bytes().len()) else {
        return 0;
    };
    // Rewrite the caller's NUL-terminated buffer in place, inserting
    // NUL separators between components.
    // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
    let Some(buf) = (unsafe { slice_mut(name as *mut u8, name_len + 1) }) else {
        return 0;
    };

    let mut mask: c_int = 0;
    let mut territory_start: Option<usize> = None;
    let mut codeset_start: Option<usize> = None;

    // Determine the language part first.
    // Termination symbols are '_', '.', and '@'.
    let mut cp = buf[..name_len]
        .iter()
        .position(|&b| matches!(b, b'_' | b'@' | b'.'))
        .unwrap_or(name_len);

    if cp == 0 {
        // Language has to be specified. Use this entry as-is without exploding.
        // Perhaps it is an alias.
        cp = name_len;
    } else {
        // Check for territory after '_'.
        if buf[cp] == b'_' {
            buf[cp] = 0;
            cp += 1;
            territory_start = Some(cp);
            set_out(territory, buf[cp..].as_ptr() as *const c_char);

            while buf[cp] != 0 && buf[cp] != b'.' && buf[cp] != b'@' {
                cp += 1;
            }

            mask |= types::XPG_TERRITORY;
        }

        // Check for codeset after '.'.
        if buf[cp] == b'.' {
            buf[cp] = 0;
            cp += 1;
            let start = cp;
            codeset_start = Some(start);
            set_out(codeset, buf[start..].as_ptr() as *const c_char);

            while buf[cp] != 0 && buf[cp] != b'@' {
                cp += 1;
            }

            mask |= types::XPG_CODESET;

            // Normalize the codeset if non-empty.
            if cp != start && buf[start] != 0 {
                let codeset_len = cp - start;
                let norm =
                    // SAFETY: forwarded raw pointers satisfy `_nl_normalize_codeset`'s contract per this function's own `# Safety` requirements.
                    unsafe { _nl_normalize_codeset(buf[start..].as_ptr() as *const c_char, codeset_len) };
                if norm.is_null() {
                    return -1;
                }

                // Compare normalized with original.
                // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                let same = unsafe { cstr(norm) }
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    .zip(unsafe { cstr(buf[start..].as_ptr() as *const c_char) })
                    .is_some_and(|(a, b)| a.to_bytes() == b.to_bytes());
                if same {
                    // Same, free the normalized copy.
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    unsafe { c_free(norm) };
                } else {
                    set_out(normalized_codeset, norm);
                    mask |= types::XPG_NORM_CODESET;
                }
            }
        }
    }

    // Check for modifier after '@'.
    if buf[cp] == b'@' {
        buf[cp] = 0;
        cp += 1;
        set_out(modifier, buf[cp..].as_ptr() as *const c_char);

        if buf[cp] != 0 {
            mask |= types::XPG_MODIFIER;
        }
    }

    // Clear territory flag if territory is empty.
    if let Some(i) = territory_start
        && buf[i] == 0
    {
        mask &= !types::XPG_TERRITORY;
    }

    // Clear codeset flag if codeset is empty.
    if let Some(i) = codeset_start
        && buf[i] == 0
    {
        mask &= !types::XPG_CODESET;
    }

    mask
}

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
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap_or(""), "en");
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
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap_or(""), "en");
            assert_eq!(std::ffi::CStr::from_ptr(terr).to_str().unwrap_or(""), "US");
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
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap_or(""), "en");
            assert_eq!(std::ffi::CStr::from_ptr(terr).to_str().unwrap_or(""), "US");
            assert_eq!(std::ffi::CStr::from_ptr(cs).to_str().unwrap_or(""), "UTF-8");
            // Normalized codeset should be set since UTF-8 != UTF_8
            assert!(!norm_cs.is_null());
            assert_eq!(
                std::ffi::CStr::from_ptr(norm_cs).to_str().unwrap_or(""),
                "UTF_8"
            );

            // Clean up normalized codeset.
            let len = std::ffi::CStr::from_ptr(norm_cs).to_bytes().len() + 1;
            let layout = must(std::alloc::Layout::from_size_align(len, 1));
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
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap_or(""), "de");
            assert_eq!(std::ffi::CStr::from_ptr(terr).to_str().unwrap_or(""), "DE");
            assert_eq!(
                std::ffi::CStr::from_ptr(cs).to_str().unwrap_or(""),
                "ISO-8859-1"
            );
            assert_eq!(
                std::ffi::CStr::from_ptr(mod_).to_str().unwrap_or(""),
                "euro"
            );
            assert!(!norm_cs.is_null());

            // Clean up normalized codeset.
            let len = std::ffi::CStr::from_ptr(norm_cs).to_bytes().len() + 1;
            let layout = must(std::alloc::Layout::from_size_align(len, 1));
            std::alloc::dealloc(norm_cs as *mut u8, layout);
        }
    }

    #[test]
    fn test_empty_language_returns_zero() {
        let mut name = b"\0".to_vec();
        let mut lang: *const c_char = std::ptr::null();
        let mut mod_: *const c_char = std::ptr::null();
        let mut terr: *const c_char = std::ptr::null();
        let mut cs: *const c_char = std::ptr::null();
        let mut norm_cs: *const c_char = std::ptr::null();

        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let mask = unsafe {
            _nl_explode_name(
                name.as_mut_ptr() as *mut c_char,
                &mut lang,
                &mut mod_,
                &mut terr,
                &mut cs,
                &mut norm_cs,
            )
        };

        // Empty name: language == cp, so cp is set to end of string, mask stays 0.
        assert_eq!(mask, 0);
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
            assert_eq!(std::ffi::CStr::from_ptr(lang).to_str().unwrap_or(""), "pt");
            assert_eq!(std::ffi::CStr::from_ptr(mod_).to_str().unwrap_or(""), "BR");
        }
    }

    #[test]
    fn test_normalize_codeset_simple() {
        unsafe {
            let cs = b"utf-8\0";
            let result = _nl_normalize_codeset(cs.as_ptr() as *const c_char, 5);
            assert!(!result.is_null());
            let s = std::ffi::CStr::from_ptr(result);
            assert_eq!(s.to_str().unwrap_or(""), "UTF_8");

            let layout = must(std::alloc::Layout::from_size_align(6, 1));
            std::alloc::dealloc(result as *mut u8, layout);
        }
    }
}
