use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn s4_summary_methods_receive_gnu_na_rm_default_and_argument_order() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(
                Vec::new(),
                std::env::temp_dir(),
            ))
        } else {
            RSession::new()
        }
        .unwrap();
        if portable {
            session.enable_browser_files();
        }
        let output = session
            .eval(include_str!("fixtures/s4-summary-na-rm-probe.R"))
            .unwrap();
        assert_eq!(
            output.trim(),
            include_str!("fixtures/s4-summary-na-rm-probe.out").trim(),
            "portable={portable}"
        );
    }
}
