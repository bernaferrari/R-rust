#![warn(unused_unsafe)]
//! Port of localename.c -- Locale name handling.
//!
//! Determines the name of the currently selected locale for a given
//! category from the environment, following the POSIX precedence
//! LC_ALL, the per-category variable, then LANG.

#![allow(non_snake_case, dead_code)]

use std::{env, ffi::CString, os::raw::c_int};

fn category_env_name(category: c_int) -> &'static str {
    match category {
        0 => "LC_CTYPE",
        1 => "LC_NUMERIC",
        2 => "LC_TIME",
        3 => "LC_COLLATE",
        4 => "LC_MONETARY",
        5 => "LC_MESSAGES",
        6 => "LC_ALL",
        _ => "LC_ALL",
    }
}

/// Determine the name of the currently selected locale for the given
/// category, checking LC_ALL, then the category variable, then LANG.
/// Defaults to "C" when none is set.
pub fn locale_name(category: c_int) -> CString {
    ["LC_ALL", category_env_name(category), "LANG"]
        .into_iter()
        .find_map(|name| env::var(name).ok().and_then(|value| CString::new(value).ok()))
        .unwrap_or_else(|| c"C".to_owned())
}

/// Canonicalize a locale name.
///
/// Takes the locale name bytes without the NUL terminator and normalizes
/// the language part to lowercase and the territory part to uppercase.
pub fn canonicalize_locale_name(buf: &mut [u8]) {
    let len = buf.len();

    // Normalize: convert language part to lowercase, territory to uppercase.
    let mut i = 0usize;
    // Lowercase the language part.
    while i < len && buf[i] != b'_' && buf[i] != b'.' && buf[i] != b'@' {
        buf[i] = buf[i].to_ascii_lowercase();
        i += 1;
    }

    if i < len && buf[i] == b'_' {
        i += 1;
        // Uppercase the territory part.
        while i < len && buf[i] != b'.' && buf[i] != b'@' {
            buf[i] = buf[i].to_ascii_uppercase();
            i += 1;
        }
    }

    // Leave codeset and modifier as-is.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_name_returns() {
        let name = locale_name(super::super::types::LC_MESSAGES);
        // Returns either the environment locale or "C", as an owned,
        // NUL-terminated string.
        assert!(!name.as_bytes().is_empty());
        assert_eq!(name.as_bytes_with_nul().last(), Some(&0));
    }

    #[test]
    fn test_canonicalize_simple() {
        let mut buf = b"en_us".to_vec();
        canonicalize_locale_name(&mut buf);
        assert_eq!(buf, b"en_US");
    }

    #[test]
    fn test_canonicalize_with_codeset() {
        let mut buf = b"en_us.utf-8".to_vec();
        canonicalize_locale_name(&mut buf);
        assert_eq!(buf, b"en_US.utf-8");
    }

    #[test]
    fn test_canonicalize_empty() {
        canonicalize_locale_name(&mut []);
        // Should not crash.
    }

    #[test]
    fn test_canonicalize_with_modifier() {
        let mut buf = b"de_de.ISO-8859-1@euro".to_vec();
        canonicalize_locale_name(&mut buf);
        assert_eq!(buf, b"de_DE.ISO-8859-1@euro");
    }
}
