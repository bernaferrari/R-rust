use r_embed::RSession;

#[test]
fn compiler_cmpfun_evaluates_supported_closure() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval("f <- function(x) x + 1; g <- compiler::cmpfun(f); g(2)")
        .expect("supported compiler call");
    assert!(result.contains("[1] 3"), "{result}");
}

#[test]
fn compiler_cmpfun_keeps_original_on_unsupported_body_and_recovers() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval(
            &("f <- function(x) user_fun(x); ".to_owned()
                + "tryCatch(compiler::cmpfun(f), error=function(e) 'recovered')"),
        )
        .expect("unsupported compiler error should be catchable");
    assert!(result.contains("recovered"), "{result}");

    let body_type = session.eval("typeof(body(f))").expect("body query");
    assert!(body_type.contains("language"), "{body_type}");
}

#[test]
fn compiler_cmpfun_preserves_builtin_identity_and_rejects_options() {
    let mut session = RSession::new().expect("session");
    let builtin = session
        .eval("typeof(compiler::cmpfun(sum))")
        .expect("builtin compiler call");
    assert!(builtin.contains("builtin"), "{builtin}");

    let options = session
        .eval(
            &("tryCatch(compiler::cmpfun(function(x) x, ".to_owned()
                + "options=list(optimize=3)), error=function(e) 'options rejected')"),
        )
        .expect("unsupported options should be catchable");
    assert!(options.contains("options rejected"), "{options}");

    let invalid = session
        .eval("tryCatch(compiler::cmpfun(1), error=function(e) 'invalid rejected')")
        .expect("invalid input should be catchable");
    assert!(invalid.contains("invalid rejected"), "{invalid}");
}

#[test]
fn compiler_cmpfun_matches_named_formals_before_positional_arguments() {
    let mut session = RSession::new().expect("session");
    for call in [
        "compiler::cmpfun(f=function(x) x + 1, NULL)",
        "compiler::cmpfun(options=NULL, function(x) x + 1)",
        "compiler::cmpfun(function(x) x + 1, opt=NULL)",
    ] {
        let result = session.eval(&format!("g <- {call}; g(2)")).expect(call);
        assert!(result.contains("[1] 3"), "{call}: {result}");
    }
    let duplicate = session.eval(
        "tryCatch(compiler::cmpfun(function(x) x, options=NULL, opt=NULL), error=function(e) 'duplicate rejected')"
    ).expect("duplicate is recoverable");
    assert!(duplicate.contains("duplicate rejected"), "{duplicate}");
}
