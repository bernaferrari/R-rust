use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn names_value_metadata_matches_pinned_gnu() {
    let mut failures = Vec::new();
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/names-metadata/controls.R"))
            .unwrap();
        let expected = include_str!("fixtures/names-metadata/controls.out");
        assert_eq!(
            actual.lines().count(),
            expected.lines().count(),
            "portable={portable}"
        );
        for (index, (actual, expected)) in actual.lines().zip(expected.lines()).enumerate() {
            if actual != expected {
                failures.push(format!(
                    "portable={portable}, row={index}: actual={actual}, expected={expected}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches; first rows:\n{}",
        failures.len(),
        failures
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
