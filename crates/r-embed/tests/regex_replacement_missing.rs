//! Missing replacements depend on an actual match in the selected mode.
use r_embed::{RSession, RuntimePathPolicy};

macro_rules! missing_case {
    ($name:ident, $code:expr) => {
        #[test]
        fn $name() {
            let mut session =
                RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
            assert_eq!(session.eval($code).unwrap(), "[1] TRUE\n");
            assert_eq!(session.eval("1+1").unwrap(), "[1] 2\n");
        }
    };
}

missing_case!(
    missing_perl_lookbehind_replacement,
    r#"identical(gsub("(?<=a)b", NA_character_, c("ab", "cb"), perl=TRUE), c(NA_character_, "cb"))"#
);
missing_case!(
    missing_perl_zero_width_replacement,
    r#"identical(gsub("a*", NA_character_, c("bbb", ""), perl=TRUE), c(NA_character_, NA_character_))"#
);
missing_case!(
    missing_ere_zero_width_replacement,
    r#"identical(sub("^", NA_character_, c("ab", "")), c(NA_character_, NA_character_))"#
);
missing_case!(
    missing_fixed_metacharacter_replacement,
    r#"identical(gsub(".", NA_character_, c("a.b", "axb"), fixed=TRUE), c(NA_character_, "axb"))"#
);
missing_case!(
    missing_fixed_unbalanced_pattern_replacement,
    r#"identical(sub("[", NA_character_, "[", fixed=TRUE), NA_character_)"#
);
missing_case!(
    missing_raw_byte_replacement,
    r#"identical(sub("a", NA_character_, c("a", "b"), useBytes=TRUE), c(NA_character_, "b"))"#
);
missing_case!(
    missing_raw_byte_pattern,
    r#"identical(sub(NA_character_, "x", c("a", "b", NA_character_), useBytes=TRUE), rep(NA_character_, 3L))"#
);
missing_case!(
    missing_replacement_retains_names,
    r#"identical(sub("a", NA_character_, c(first="a", second="b", absent=NA_character_)), c(first=NA_character_, second="b", absent=NA_character_))"#
);
missing_case!(
    missing_replacement_nonmatch_control,
    r#"identical(sub("a", NA_character_, c("b", NA_character_), perl=TRUE), c("b", NA_character_))"#
);
missing_case!(
    literal_na_replacement_control,
    r#"identical(sub("a", "NA", c("a", "b"), useBytes=TRUE), c("NA", "b"))"#
);

#[test]
fn complete_missing_replacement_contract() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!(
                "fixtures/regex-replacement-missing-contract.R"
            ))
            .unwrap(),
        "[1] TRUE\n"
    );
}
