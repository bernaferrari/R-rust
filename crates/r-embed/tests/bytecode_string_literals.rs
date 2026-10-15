use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn imported_bytecode_string_literals_duplicate_before_value_or_body_mutation() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    session.enable_browser_files();
    session
        .import_file(
            "literals.rds",
            include_bytes!("fixtures/gnu-bytecode-string-literals.rds"),
        )
        .unwrap();
    session
        .eval("functions <- readRDS('literals.rds'); invisible(NULL)")
        .unwrap();
    assert_eq!(
        session
            .eval(include_str!(
                "fixtures/bytecode-string-cow-public-contract.R"
            ))
            .unwrap(),
        "[1] TRUE\n"
    );
}
