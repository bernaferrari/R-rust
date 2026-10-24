#![forbid(unsafe_code)]
use super::*;

const SUBJECTS: [&str; 9] = ["é", "a", "Ω", "一", "ß", "٢", "\u{a0}", " ", "2"];
fn check(pattern: &str, expected: [bool; 9]) {
    let regex =
        PerlRegex::compile(pattern, false).unwrap_or_else(|error| panic!("{pattern}: {error}"));
    assert_eq!(
        SUBJECTS.map(|s| regex.find(s).is_some()),
        expected,
        "{pattern}"
    );
}
#[test]
fn perl_class_policy_default_word_is_ascii_while_subject_is_unicode() {
    check(
        r"^\w+$",
        [false, true, false, false, false, false, false, false, true],
    );
}
#[test]
fn perl_class_policy_default_digit_is_ascii_while_subject_is_unicode() {
    check(
        r"^\d+$",
        [false, false, false, false, false, false, false, false, true],
    );
}
#[test]
fn perl_class_policy_default_space_is_ascii_while_subject_is_unicode() {
    check(
        r"^\s+$",
        [false, false, false, false, false, false, false, true, false],
    );
}
#[test]
fn perl_class_policy_ucp_word_uses_unicode_properties() {
    check(
        r"(*UCP)^\w+$",
        [true, true, true, true, true, true, false, false, true],
    );
}
#[test]
fn perl_class_policy_ucp_digit_uses_unicode_properties() {
    check(
        r"(*UCP)^\d+$",
        [false, false, false, false, false, true, false, false, true],
    );
}
#[test]
fn perl_class_policy_ucp_space_uses_unicode_properties() {
    check(
        r"(*UCP)^\s+$",
        [false, false, false, false, false, false, true, true, false],
    );
}
#[test]
fn perl_class_policy_dot_still_consumes_one_unicode_scalar() {
    check(r"^.$", [true; 9]);
}
#[test]
fn perl_class_policy_inline_case_flag_control() {
    check(r"(?i)^k$", [false; 9]);
}

#[test]
fn perl_class_policy_unicode_pattern_corpus_matches_both_gnu_locales() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../r-embed/tests/fixtures/perl-character-class-kernel-corpus.json"
    ))
    .unwrap();
    let subjects = corpus["subjects"].as_array().unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for row in corpus["cases"].as_array().unwrap() {
        if row["bytes"].as_bool().unwrap() {
            continue;
        }
        checked += 1;
        assert_eq!(row["c"], row["utf8"], "Unicode oracle profiles agree");
        let pattern = row["pattern"].as_str().unwrap();
        let expected = row["utf8"].as_str().unwrap();
        let actual = match PerlRegex::compile(pattern, false) {
            Ok(regex) => subjects
                .iter()
                .map(|s| {
                    if regex.find(s.as_str().unwrap()).is_some() {
                        '1'
                    } else {
                        '0'
                    }
                })
                .collect::<String>(),
            Err(_) => String::from("ERROR"),
        };
        if actual != expected {
            failures.push((pattern, actual, expected));
        }
    }
    assert_eq!(checked, 182);
    assert!(
        failures.is_empty(),
        "GNU Unicode dialect mismatches: {failures:?}"
    );
}

#[test]
#[cfg(not(miri))]
fn perl_class_policy_ucp_matches_complete_scalar_oracle() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../r-embed/tests/fixtures/perl-ucp-all-scalars.json"
    ))
    .unwrap();
    let classes = oracle["ranges"].as_object().unwrap();
    assert_eq!(classes.len(), 14);
    let mut failures = Vec::new();
    for (kind, values) in classes {
        let pattern = format!("(*UCP)\\A[[:{kind}:]]\\z");
        let regex = PerlRegex::compile(&pattern, false).unwrap();
        let ranges = values
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r[0].as_u64().unwrap() as u32, r[1].as_u64().unwrap() as u32))
            .collect::<Vec<_>>();
        let mut index = 0;
        let mut checked = 0;
        for point in 1..=0x10ffff {
            let Some(ch) = char::from_u32(point) else {
                continue;
            };
            checked += 1;
            while index < ranges.len() && point > ranges[index].1 {
                index += 1;
            }
            let expected = index < ranges.len() && point >= ranges[index].0;
            let mut buffer = [0; 4];
            let actual = regex.find(ch.encode_utf8(&mut buffer)).is_some();
            if expected != actual && failures.len() < 40 {
                failures.push((kind.clone(), point, actual, expected));
            }
        }
        assert_eq!(checked, oracle["scalar_count"].as_u64().unwrap());
        // NUL cannot occur in an R string; retain the Unicode-category control.
        assert_eq!(
            regex.find("\0").is_some(),
            matches!(kind.as_str(), "ascii" | "cntrl")
        );
    }
    assert!(
        failures.is_empty(),
        "GNU UCP scalar mismatches: {failures:x?}"
    );
}

#[test]
fn perl_class_policy_global_progression_stops_at_terminal_offset_after_a_match() {
    for (pattern, text, spans) in [
        (r"\b", "éa", vec![(2, 2)]),
        (r"\b", "a", vec![(0, 0)]),
        ("$", "éa", vec![(3, 3)]),
        ("a*", "a", vec![(0, 1)]),
        ("$", "", vec![(0, 0)]),
        ("b*", "a", vec![(0, 0)]),
        (r"(?<zero>\b)", "éa", vec![(2, 2)]),
    ] {
        let captures = mode_captures_all(pattern, text, false, false).unwrap();
        let actual = captures
            .iter()
            .map(|row| {
                let whole = row[0].unwrap();
                (whole.start, whole.end)
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, spans, "pattern={pattern:?}, text={text:?}");
        if pattern.contains("zero") {
            let group = captures[0][1].unwrap();
            assert_eq!((group.start, group.end), (2, 2));
        }
    }
}
