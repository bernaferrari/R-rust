use super::*;
use crate::sexp::session::RSession;

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn assert_float(actual: f64, expected: &[u8], case: &str) {
    let wanted = u64::from_le_bytes(expected.try_into().unwrap());
    let value = f64::from_bits(wanted);
    if value.is_nan() {
        assert!(actual.is_nan(), "{case}: {actual:?} is not NaN");
        // GNU ISNA recognizes its payload in both signaling and quiet NaNs.
        let is_na = |bits: u64| bits & 0x0007_ffff_ffff_ffff == 1954;
        assert_eq!(
            is_na(actual.to_bits()),
            is_na(wanted),
            "{case}: NA/NaN classification"
        );
    } else {
        assert_eq!(actual.to_bits(), wanted, "{case}: floating-point bits");
    }
}

#[test]
fn gnu_number_text_coercion_matches_independent_value_and_warning_table() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let mut mismatches = Vec::new();
        for row in include_str!("fixtures/number-coercion.tsv").lines() {
            let parts: Vec<_> = row.split('|').collect();
            assert_eq!(parts.len(), 4);
            let input = bytes(parts[0]);
            let expected = bytes(parts[2]);
            let text = String::from_utf8(input.clone()).unwrap();
            let character = factory.character(&text).unwrap().into_owned().unwrap();
            let mut warning = 0;
            let expected_warning = match parts[3] {
                "" => 0,
                "NAs introduced by coercion" => WARN_NA,
                "NAs introduced by coercion to integer range" => WARN_INT_NA,
                other => panic!("unmapped original warning: {other}"),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                match parts[1] {
                    "double" => assert_float(
                        RealFromString(character.as_raw(), &mut warning),
                        &expected,
                        &text,
                    ),
                    "integer" => assert_eq!(
                        IntegerFromString(character.as_raw(), &mut warning),
                        i32::from_le_bytes(expected.try_into().unwrap()),
                        "{text:?}"
                    ),
                    "complex" => {
                        let value = ComplexFromString(character.as_raw(), &mut warning);
                        assert_float(value.r, &expected[..8], &text);
                        assert_float(value.i, &expected[8..], &text);
                        let input = std::ffi::CString::new(input.clone()).unwrap();
                        let mut c_warning = 0;
                        let c_value = ComplexFromStringC(input.as_ptr(), &mut c_warning);
                        assert_float(c_value.r, &expected[..8], &text);
                        assert_float(c_value.i, &expected[8..], &text);
                        assert_eq!(c_warning, expected_warning, "{text:?}: C-string warning");
                    }
                    _ => panic!("unknown original mode"),
                }
                assert_eq!(warning, expected_warning, "{text:?}: warning contract");
            }));
            if result.is_err() {
                mismatches.push(format!("{:?} {}", text, parts[1]));
            }
        }
        assert!(
            mismatches.is_empty(),
            "original value/warning mismatches: {mismatches:?}"
        );
    });
}
