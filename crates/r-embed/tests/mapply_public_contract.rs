use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn mapply_public_matching_recycling_simplification_and_return_match_gnu() {
    let source = include_str!("fixtures/mapply-public-contract.R");
    let expected = include_str!("fixtures/mapply-public-contract.out");
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session.eval(source).unwrap(),
            expected,
            "portable={portable}"
        );
        session.close();
    }
}
