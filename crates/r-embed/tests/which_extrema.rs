use r_embed::{RSession, RuntimePathPolicy};
#[test]
fn which_extrema_match_gnu_names_coercion_and_transform() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/which-extrema/contract.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/which-extrema/contract.out"),
            "portable={portable}"
        );
    }
}
