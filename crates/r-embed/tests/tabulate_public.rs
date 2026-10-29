use r_embed::{RSession, RuntimePathPolicy};
#[test]
fn tabulate_matches_gnu_validation_and_counts() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/tabulate-public/contract.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/tabulate-public/contract.out"),
            "portable={portable}"
        );
    }
}
