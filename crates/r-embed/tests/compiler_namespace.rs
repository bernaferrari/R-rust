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
fn compiler_cmpfun_preserves_source_and_executes_user_calls() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval(
            &("user_fun <- function(x) x * 2L; f <- function(x) user_fun(x); ".to_owned()
                + "g <- compiler::cmpfun(f); "
                + "identical(g(3L), 6L) && identical(f(3L), 6L) && "
                + "identical(formals(g), formals(f)) && identical(environment(g), environment(f))"),
        )
        .expect("public compiler accepts calls to user functions");
    assert_eq!(result, "[1] TRUE\n");

    let original_body = session
        .eval("identical(body(f), quote(user_fun(x)))")
        .expect("original body query");
    assert_eq!(original_body, "[1] TRUE\n");
}

#[test]
fn compiler_cmpfun_preserves_builtin_identity_and_accepts_gnu_options() {
    let mut session = RSession::new().expect("session");
    let builtin = session
        .eval("identical(compiler::cmpfun(sum), sum)")
        .expect("builtin compiler call");
    assert_eq!(builtin, "[1] TRUE\n");

    let options = session
        .eval(
            &("g <- compiler::cmpfun(function(x) x, ".to_owned()
                + "options=list(optimize=3)); identical(g(7L), 7L)"),
        )
        .expect("valid GNU compiler options");
    assert_eq!(options, "[1] TRUE\n");

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
