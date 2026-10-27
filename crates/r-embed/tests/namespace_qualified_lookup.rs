use r_embed::RSession;

const NAMESPACE_QUALIFIED_LOOKUP_SCRIPT: &str = r##"
    stopifnot(inherits(try(base::lm, silent=TRUE), "try-error"))
    stopifnot(inherits(try(base:::lm, silent=TRUE), "try-error"))
    stopifnot(inherits(try(graphics::log, silent=TRUE), "try-error"))
    stopifnot(identical(base::as.list, as.list))
    stopifnot(identical(base:::as.list, as.list))
    stopifnot(identical(base::.S3PrimitiveGenerics,
                        get(".S3PrimitiveGenerics", asNamespace("base"))))
    stopifnot(identical(stats::lm, stats:::lm))
    stopifnot(inherits(try(stats::.approxfun, silent=TRUE), "try-error"))
    stopifnot(is.function(stats:::.approxfun))
    cat("namespace-qualified-lookup: PASS\n")
    "##;

fn assert_namespace_qualified_lookup(session: &mut RSession) {
    assert_eq!(
        session.eval(NAMESPACE_QUALIFIED_LOOKUP_SCRIPT).unwrap(),
        "namespace-qualified-lookup: PASS\n"
    );
}

#[test]
fn native_namespace_qualified_lookup_obeys_exports() {
    let mut session = RSession::new().unwrap();
    assert_namespace_qualified_lookup(&mut session);
}

#[test]
fn portable_namespace_qualified_lookup_obeys_exports() {
    let mut session = RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(
        Vec::new(),
        std::env::temp_dir(),
    ))
    .unwrap();
    assert_namespace_qualified_lookup(&mut session);
}
