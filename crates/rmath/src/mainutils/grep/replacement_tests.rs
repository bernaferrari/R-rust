#![forbid(unsafe_code)]
//! Pinned GNU R bac583 replacement contract, UTF8 child locale; byte profile separate.
use super::{ere_replace, perl_replace};
macro_rules! case {
    ($name:ident, $perl:expr, $pattern:expr, $text:expr, $replacement:expr, $global:expr, $expected:expr) => {
        #[test]
        fn $name() {
            let result = if $perl {
                perl_replace($pattern, $text, $replacement, $global, false)
            } else {
                ere_replace($pattern, $text, $replacement, $global, false)
            };
            assert_eq!(result.as_deref(), Some($expected));
        }
    };
}
case!(
    replacement_contract_01_switch,
    true,
    "(a)(Bc)",
    "aBc",
    "\\U\\1\\L\\2",
    false,
    "Abc"
);
case!(
    replacement_contract_02_literal_preserved,
    true,
    "(a)(Bc)",
    "aBc",
    "lit\\U\\1MiXeD\\2",
    false,
    "litAMiXeDBC"
);
case!(
    replacement_contract_03_end_mode,
    true,
    "(a)(Bc)",
    "aBc",
    "\\Uab\\1\\Ecd\\2",
    false,
    "abAcdBc"
);
case!(
    replacement_contract_04_switch_and_reset,
    true,
    "(a)(Bc)",
    "aBc",
    "\\U\\1\\L\\2\\E\\1",
    false,
    "Abca"
);
case!(
    replacement_contract_05_missing_group,
    true,
    "(a)",
    "a",
    "\\U\\9\\1",
    false,
    "A"
);
case!(
    replacement_contract_06_unset_group,
    true,
    "(a)?(b)",
    "b",
    "\\U\\1\\2",
    false,
    "B"
);
case!(
    replacement_contract_07_per_match_reset,
    true,
    "([a-z])",
    "ab",
    "\\U\\1:\\L\\1",
    true,
    "A:aB:b"
);
case!(
    replacement_contract_08_escaped_slash_control,
    true,
    "(a)",
    "a",
    "\\\\U\\1",
    false,
    "\\Ua"
);
case!(
    replacement_contract_09_unsupported_single_case_control,
    true,
    "(a)",
    "a",
    "\\u\\1",
    false,
    "ua"
);
case!(
    replacement_contract_10_trailing_escape,
    true,
    "(a)",
    "a",
    "x\\",
    false,
    "x"
);
case!(
    replacement_contract_11_ere_trailing_escape,
    false,
    "(a)",
    "a",
    "x\\",
    false,
    "x"
);
case!(
    replacement_contract_12_ere_mode_letters_control,
    false,
    "(a)",
    "a",
    "\\U\\1\\E",
    false,
    "UaE"
);
case!(
    replacement_contract_13_zero_group_control,
    true,
    "(a)",
    "a",
    "\\0",
    false,
    "0"
);
case!(
    replacement_contract_14_boundary_empty_groups,
    true,
    "\\b",
    "a b",
    "\\U\\1",
    true,
    "a b"
);
case!(
    replacement_contract_15_ascii_word_boundary_control,
    true,
    "\\b",
    "éa λb",
    "X",
    true,
    "éXaX λXbX"
);
case!(
    replacement_contract_16_literal_backspace,
    true,
    "\u{8}",
    "a\u{8}b",
    "Z",
    true,
    "aZb"
);
case!(
    replacement_contract_17_unicode_simple_upper,
    true,
    "(éßᾀ)",
    "éßᾀ",
    "\\U\\1",
    false,
    "Éßᾈ"
);
case!(
    replacement_contract_18_unicode_simple_lower,
    true,
    "(İΣẞ)",
    "İΣẞ",
    "\\L\\1",
    false,
    "iσß"
);
case!(
    replacement_contract_19_unicode_supplementary,
    true,
    "(𐐨)",
    "𐐨",
    "\\U\\1",
    false,
    "𐐀"
);

#[test]
fn replacement_contract_20_bytes_upper_preserves_non_ascii_capture() {
    let result = super::perl_replace_with_case(
        "(é)",
        "é",
        r"\U\1",
        false,
        false,
        super::ReplacementCase::Bytes,
    );
    assert_eq!(result.as_deref(), Some("é"));
}
#[test]
fn replacement_contract_21_bytes_lower_preserves_non_ascii_capture() {
    let result = super::perl_replace_with_case(
        "(ẞ)",
        "ẞ",
        r"\L\1",
        false,
        false,
        super::ReplacementCase::Bytes,
    );
    assert_eq!(result.as_deref(), Some("ẞ"));
}
#[test]
fn replacement_contract_22_mode_resets_before_first_reference() {
    let result = perl_replace("([a-z])", "ab", r"\1\U", true, false);
    assert_eq!(result.as_deref(), Some("ab"));
}

#[test]
fn replacement_contract_23_literal_byte_parser_preserves_opaque_data_and_modes() {
    for (input, perl, expected) in [
        (&b"x\\"[..], true, &b"x"[..]),
        (&b"x\\"[..], false, &b"x"[..]),
        (&b"\\Ufoo\\1\\E"[..], true, &b"foo"[..]),
        (&b"\\Ufoo\\1\\E"[..], false, &b"UfooE"[..]),
        (&b"\\\\U"[..], true, &b"\\U"[..]),
        (&b"\\0\\q"[..], true, &b"0q"[..]),
        (&[255, b'\\', 254][..], true, &[255, 254][..]),
    ] {
        assert_eq!(&*super::literal_replacement_bytes(input, perl), expected);
    }
}
