use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn public_get_and_get0_preserve_lazy_modes_and_inheritance() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!("fixtures/get-lazy-mode-public-contract.R"))
                .unwrap(),
            include_str!("fixtures/get-lazy-mode-public-contract.out"),
            "portable={portable}"
        );
        assert_eq!(session.eval("1 + 1").unwrap(), "[1] 2\n");
    }
}
