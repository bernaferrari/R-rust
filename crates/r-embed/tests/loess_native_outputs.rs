use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn loess_native_buffers_match_independent_gnu_outputs() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap();
    session.enable_browser_files();
    session
        .import_file(
            "loess-native-oracle.rds",
            include_bytes!("fixtures/gnu-loess-native-outputs.rds"),
        )
        .unwrap();
    session
        .eval("expected <- readRDS('loess-native-oracle.rds')")
        .unwrap();
    session
        .eval(include_str!("fixtures/loess-native-outputs-input.R"))
        .unwrap();
    assert_eq!(session.eval("actual <- loess_native_outputs(); comparison <- all.equal(actual, expected, tolerance=1e-9); if (!isTRUE(comparison)) stop(paste(comparison, collapse='; ')); identical(length(actual), 40L)").unwrap(), "[1] TRUE\n");
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
