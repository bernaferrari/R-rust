use r_embed::{RSession, RuntimePathPolicy};
#[test]
fn recursive_anyna_nested_public_contract() {
    for portable in [false, true] {
        let mut s = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = s
            .eval(include_str!("fixtures/anyna-recursive/contract.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/anyna-recursive/contract.out"),
            "portable={portable}"
        );
    }
}
