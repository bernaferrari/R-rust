use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn pairlist_names_match_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let output = session
            .eval(include_str!("fixtures/names-pairlist/controls.R"))
            .unwrap();
        let expected = include_str!("fixtures/names-pairlist/controls.out");
        for (actual, expected) in output.lines().zip(expected.lines()) {
            assert_eq!(actual, expected, "portable={portable}");
        }
        assert_eq!(
            output.lines().count(),
            expected.lines().count(),
            "portable={portable}"
        );
    }
}

#[test]
#[ignore = "pairlist and language names semantics tracked by rport-n39t1"]
fn pairlist_target_names_match_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let output = session
            .eval(include_str!("fixtures/names-pairlist-target/controls.R"))
            .unwrap();
        let expected = include_str!("fixtures/names-pairlist-target/controls.out");
        let actual: Vec<_> = output.lines().collect();
        let expected: Vec<_> = expected.lines().collect();
        let differences: Vec<_> = actual
            .iter()
            .zip(&expected)
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(actual.len(), expected.len(), "portable={portable}");
        assert!(
            differences.is_empty(),
            "portable={portable}: {differences:#?}"
        );
    }
}

#[test]
fn pairlist_replacement_distinguishes_na_string_from_missing() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let output = session
            .eval(
                r#"
            x <- pairlist(a=1,b=2); names(x) <- c("NA", NA_character_)
            y <- quote(f(a=1,b=2)); names(y) <- c("ignored", "NA", NA_character_)
            cat(paste(capture.output(dput(x)), collapse=" "), "\n", sep="")
            cat(paste(capture.output(dput(y)), collapse=" "), "\n", sep="")
        "#,
            )
            .unwrap();
        assert_eq!(
            output, "pairlist(`NA` = 1, 2)\nf(`NA` = 1, 2)\n",
            "portable={portable}"
        );
    }
}
