use r_embed::{RSession, RuntimePathPolicy};
#[test]
fn scalar_logical_shared_parser_matches_gnu_public_values_and_dispatch() {
    for portable in [false, true] {
        let mut s = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            s.eval(include_str!("fixtures/scalar-logical/contract.R"))
                .unwrap(),
            include_str!("fixtures/scalar-logical/contract.out"),
            "portable={portable}"
        );
    }
}
