use r_embed::{RSession, RuntimePathPolicy};
#[test]
fn parallel_extrema_match_gnu_plain_numeric_contract() {
    for portable in [false, true] {
        let mut s = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = s
            .eval(include_str!("fixtures/parallel-extrema/contract.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/parallel-extrema/contract.out"),
            "portable={portable}"
        );
    }
}
