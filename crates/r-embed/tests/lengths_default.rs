use r_embed::{RSession, RuntimePathPolicy};
#[test]
fn lengths_default_shape_contract() {
    for portable in [false, true] {
        let mut s = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = s
            .eval(include_str!("fixtures/lengths-default/contract.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/lengths-default/contract.out"),
            "portable={portable}"
        );
    }
}
