use r_embed::{RSession, RuntimePathPolicy};
use std::path::PathBuf;

#[test]
fn installed_stats_startup_keeps_synchronization_locals_private() {
    let library =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/installed-stats-library");
    // Use the ordinary constructor and default package attachment with a
    // real installed source-package fixture, independent of host libraries.
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(vec![library], std::env::temp_dir()))
            .unwrap();
    let names = session.global_binding_names().unwrap();
    for name in ["f", "env", "tab"] {
        assert!(
            !names.iter().any(|candidate| candidate == name),
            "{names:?}"
        );
    }
    assert_eq!(
        session
            .eval(
                r#"local({
            namespace <- asNamespace("stats")
            attached <- as.environment("package:stats")
            stopifnot(identical(get("fixture_marker", namespace), "installed-stats-fixture"),
                      identical(get("reorder", attached), get("reorder", baseenv())),
                      identical(get("xtabs", attached), get("xtabs", baseenv())),
                      identical(get("diff.ts", namespace), get("diff.ts", baseenv())),
                      identical(get("diff.ts", get(".__S3MethodsTable__.", namespace)),
                                get("diff.ts", baseenv())))
            TRUE
        })"#
            )
            .unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(
        session
            .eval_script(include_str!(
                "../../../tests/conformance/cases/154_ls_all_names.R"
            ))
            .unwrap()
            .output,
        include_str!("../../../tests/conformance/golden/154_ls_all_names.out")
    );
    session.eval_interactive("7L", 32, 32).unwrap();
    assert_eq!(
        session
            .eval_script("identical(.Last.value, 7L)")
            .unwrap()
            .output,
        "[1] TRUE\n"
    );
}
