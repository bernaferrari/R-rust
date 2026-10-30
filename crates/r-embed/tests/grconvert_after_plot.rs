use r_embed::{RSession, RuntimePathPolicy};

// Pinned GNU R PNG oracle, 504 pixels at 72 device units per inch.
#[test]
fn character_units_after_plot_match_gnu() {
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
        session
            .record_scene(include_str!("fixtures/grconvert-after-plot/controls.R"), 504, 504)
            .unwrap();
        let actual = session.eval("cat(postplot_result, sep='\\n')").unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/grconvert-after-plot/controls.out"),
            "portable={portable}"
        );
    }
}
