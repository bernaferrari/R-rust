use r_embed::{RSession, RuntimePathPolicy};

// GNU R 4.6.1 PNG devices before plot.new(), including changed margins.
#[test]
fn conversion_before_first_frame_matches_gnu() {
    for portable in [false, true] {
        for (width, height, prefix, expected) in [
            (
                504,
                504,
                "",
                include_str!("fixtures/grconvert-before-plot/controls.out"),
            ),
            (
                504,
                504,
                "par(cex=1.2);\n",
                include_str!("fixtures/grconvert-before-plot/cex_only.out"),
            ),
            (
                240,
                180,
                "par(mar=c(1,1.2,1,1),oma=rep(.5,4),cex=1.2,mex=.8);\n",
                include_str!("fixtures/grconvert-before-plot/nonsquare.out"),
            ),
        ] {
            let mut session = if portable {
                RSession::new_with_path_policy(RuntimePathPolicy::new(
                    Vec::new(),
                    std::env::temp_dir(),
                ))
            } else {
                RSession::new()
            }
            .unwrap();
            let source = format!(
                "{prefix}{}",
                include_str!("fixtures/grconvert-before-plot/controls.R")
            );
            session.record_scene(&source, width, height).unwrap();
            let actual = session.eval("cat(preplot_result, sep='\\n')").unwrap();
            assert_eq!(
                actual, expected,
                "portable={portable}, {width}x{height}, {prefix}"
            );
        }
    }
}
