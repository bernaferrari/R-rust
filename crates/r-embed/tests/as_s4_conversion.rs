use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn as_s4_conversions_match_pinned_gnu_without_mutating_source() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/as-s4-conversion/conversion.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/as-s4-conversion/conversion.out"),
            "portable={portable}"
        );
    }
}
