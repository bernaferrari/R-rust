use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn anyna_nested_dispatch_contract() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/anyna-dispatch/contract.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/anyna-dispatch/contract.out"),
            "portable={portable}"
        );
    }
}
