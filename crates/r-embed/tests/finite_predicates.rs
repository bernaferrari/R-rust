use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn finite_predicates_match_gnu_for_supported_and_unsupported_types() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/finite-predicates/contract.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/finite-predicates/contract.out"),
            "portable={portable}"
        );
    }
}

#[test]
fn finite_predicates_preserve_shapes_isolation_and_s3_s4_dispatch() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/finite-predicates/neighbors.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/finite-predicates/neighbors.out"),
            "portable={portable}"
        );
    }
}
