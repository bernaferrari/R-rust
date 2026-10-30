use r_embed::RSession;

#[test]
fn compiled_forwarded_missing_argument() {
    let mut session = RSession::new().unwrap();
    let result = session.eval("probe <- function(x) missing(x); f <- compiler::cmpfun(function(x) probe(x)); cat(f(), '\\n')").unwrap();
    assert_eq!(result, "TRUE \n");
}

#[test]
fn compiled_forwarding_matches_pinned_gnu() {
    use r_embed::RuntimePathPolicy;
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let output = session
            .eval(include_str!(
                "fixtures/missing-forwarding/compiled-diagnostic.R"
            ))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/missing-forwarding/compiled-diagnostic.out"),
            "portable={portable}"
        );
    }
}
