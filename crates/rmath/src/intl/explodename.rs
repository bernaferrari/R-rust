#![warn(unused_unsafe)]
//! Decompose locale names without modifying the caller's bytes.

use std::ffi::CStr;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct LocaleParts<'a> {
    pub language: &'a [u8],
    pub territory: Option<&'a [u8]>,
    pub codeset: Option<&'a [u8]>,
    pub modifier: Option<&'a [u8]>,
    pub normalized_codeset: Option<Vec<u8>>,
}

pub(super) fn explode_name(name: &CStr) -> LocaleParts<'_> {
    let name = name.to_bytes();
    let mut parts = LocaleParts {
        language: name,
        territory: None,
        codeset: None,
        modifier: None,
        normalized_codeset: None,
    };
    let end = name
        .iter()
        .position(|&b| matches!(b, b'_' | b'.' | b'@'))
        .unwrap_or(name.len());
    // A leading separator makes the whole name an alias.
    if end == 0 {
        return parts;
    }
    parts.language = &name[..end];
    let mut rest = &name[end..];
    if let Some(tail) = rest.strip_prefix(b"_") {
        let end = tail
            .iter()
            .position(|&b| matches!(b, b'.' | b'@'))
            .unwrap_or(tail.len());
        parts.territory = (end != 0).then_some(&tail[..end]);
        rest = &tail[end..];
    }
    if let Some(tail) = rest.strip_prefix(b".") {
        let end = tail.iter().position(|&b| b == b'@').unwrap_or(tail.len());
        if end != 0 {
            let codeset = &tail[..end];
            let normalized = normalize_codeset(codeset);
            parts.codeset = Some(codeset);
            if normalized != codeset {
                parts.normalized_codeset = Some(normalized);
            }
        }
        rest = &tail[end..];
    }
    parts.modifier = rest.strip_prefix(b"@").filter(|value| !value.is_empty());
    parts
}

fn normalize_codeset(codeset: &[u8]) -> Vec<u8> {
    codeset
        .iter()
        .map(|&byte| {
            if byte.is_ascii_alphanumeric() {
                byte.to_ascii_uppercase()
            } else {
                b'_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_language() {
        let parts = explode_name(c"en");
        assert_eq!(parts.language, b"en");
        assert_eq!(parts.territory, None);
        assert_eq!(parts.codeset, None);
        assert_eq!(parts.modifier, None);
        assert_eq!(parts.normalized_codeset, None);
    }

    #[test]
    fn test_language_territory() {
        let parts = explode_name(c"en_US");
        assert_eq!(parts.language, b"en");
        assert_eq!(parts.territory, Some(b"US".as_slice()));
        assert_eq!(parts.codeset, None);
        assert_eq!(parts.modifier, None);
    }

    #[test]
    fn test_language_territory_codeset() {
        let parts = explode_name(c"en_US.UTF-8");
        assert_eq!(parts.language, b"en");
        assert_eq!(parts.territory, Some(b"US".as_slice()));
        assert_eq!(parts.codeset, Some(b"UTF-8".as_slice()));
        assert_eq!(parts.normalized_codeset, Some(b"UTF_8".to_vec()));
        assert_eq!(parts.modifier, None);
    }

    #[test]
    fn test_full_locale() {
        let parts = explode_name(c"de_DE.ISO-8859-1@euro");
        assert_eq!(parts.language, b"de");
        assert_eq!(parts.territory, Some(b"DE".as_slice()));
        assert_eq!(parts.codeset, Some(b"ISO-8859-1".as_slice()));
        assert_eq!(parts.modifier, Some(b"euro".as_slice()));
        assert!(parts.normalized_codeset.is_some());
    }

    #[test]
    fn test_empty_language() {
        let parts = explode_name(c"");
        assert_eq!(parts.language, b"");
        assert_eq!(parts.territory, None);
        assert_eq!(parts.codeset, None);
        assert_eq!(parts.modifier, None);
        assert_eq!(parts.normalized_codeset, None);
    }

    #[test]
    fn test_language_modifier_only() {
        let parts = explode_name(c"pt@BR");
        assert_eq!(parts.language, b"pt");
        assert_eq!(parts.territory, None);
        assert_eq!(parts.codeset, None);
        assert_eq!(parts.modifier, Some(b"BR".as_slice()));
    }

    #[test]
    fn test_normalize_codeset_simple() {
        assert_eq!(normalize_codeset(b"utf-8"), b"UTF_8");
    }

    #[test]
    fn unchanged_codeset_with_modifier_needs_no_normalized_copy() {
        let parts = explode_name(c"en.UTF8@variant");
        assert_eq!(parts.codeset, Some(b"UTF8".as_slice()));
        assert_eq!(parts.modifier, Some(b"variant".as_slice()));
        assert_eq!(parts.normalized_codeset, None);
    }

    #[test]
    fn empty_components_are_absent_and_leading_separator_is_alias() {
        let parts = explode_name(c"en_.@");
        assert_eq!(parts.language, b"en");
        assert_eq!(parts.territory, None);
        assert_eq!(parts.codeset, None);
        assert_eq!(parts.modifier, None);

        assert_eq!(explode_name(c"@alias").language, b"@alias");
    }
}
