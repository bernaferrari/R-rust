//! Genuine evaluator calls and exact independently captured GNU color results.
use crate::sexp::RSession;

#[test]
fn numeric_color_evaluator_matches_pinned_gnu_lexical_and_rounding_controls() {
    let mut session = RSession::new_without_default_packages();
    let mut cases = 0;
    for row in include_str!("numeric-oracle.tsv").lines() {
        let (hex, expected) = row.split_once('|').unwrap();
        let token = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let token = String::from_utf8(token).unwrap();
        let quoted = serde_json::to_string(&token).unwrap();
        let code = format!("col2rgb({quoted}, alpha=TRUE)");
        let (result, _, _) = session.eval_code_with_output_capture(&code);
        if expected == "error" {
            let error = result.expect_err(&code);
            assert!(
                error.message.contains("invalid color specification"),
                "{code}: {}",
                error.message
            );
        } else {
            let color = result.unwrap_or_else(|error| panic!("{code}: {}", error.message));
            let expected = expected
                .split(',')
                .map(|channel| channel.parse::<i32>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(color.len(), expected.len() as i64, "{code}");
            for (offset, expected) in expected.into_iter().enumerate() {
                assert_eq!(
                    color.try_integer_elt(offset as i64).unwrap(),
                    expected,
                    "{code}"
                );
            }
        }
        cases += 1;
    }
    assert_eq!(cases, 42, "every pinned neighbor is exercised");
}

#[test]
fn numeric_color_zero_spelling_preserves_each_callers_background_contract() {
    let session = RSession::new_without_default_packages();
    session.with_protected(|| {
        for token in [c"0", c"00", c"0.0", c"0x0", c"0.1", c"1e-100"] {
            assert_eq!(
                unsafe { super::converters::str2col(token.as_ptr(), 0xff030201) },
                0xff030201
            );
        }
        for token in [c"00", c"0.0", c"0x0", c"0.1", c"1e-100"] {
            assert_eq!(
                unsafe { super::inR_GE_str2col(token.as_ptr()) },
                super::R_TRANWHITE
            );
        }
    });
}

#[test]
fn numeric_color_invalid_or_unrepresentable_indices_are_recoverable_r_errors() {
    let mut session = RSession::new_without_default_packages();
    for token in [
        "0x.",
        "0x1.2.3",
        "1e309",
        "0x1p1024",
        "2147483648",
        "0x80000000",
    ] {
        let code = format!("col2rgb('{}')", token);
        let (result, _, _) = session.eval_code_with_output_capture(&code);
        assert!(
            result
                .unwrap_err()
                .message
                .contains("invalid color specification"),
            "{code}"
        );
        let (result, _, _) = session.eval_code_with_output_capture("col2rgb('1.9999999999999998')");
        let color = result.unwrap();
        for index in 0..3 {
            assert_eq!(color.try_integer_elt(index).unwrap(), 0);
        }
    }
}
