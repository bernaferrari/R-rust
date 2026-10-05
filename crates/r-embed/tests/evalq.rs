use r_embed::RSession;

#[test]
fn evalq_preserves_expression_and_uses_caller_environment_by_default() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("x <- 10; local({ y <- 2; c(evalq(x + y), evalq(x <- 3), x) })")
        .unwrap();
    assert!(
        result.split_whitespace().collect::<Vec<_>>() == ["[1]", "12", "3", "3"],
        "{result}"
    );
}

#[test]
fn evalq_evaluates_environment_and_enclos_arguments() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("e <- new.env(); e$x <- 7; evalq(x + 1, envir=e, enclos=baseenv())")
        .unwrap();
    assert!(result.contains("[1] 8"), "{result}");
}

#[test]
fn evalq_supports_list_pairlist_named_and_partial_arguments() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                list_case <- local({ z <- 2; evalq(x + z, list(x=1)) });
                pair_case <- local({ z <- 2; evalq(x + z, as.pairlist(list(x=1))) });
                e <- new.env(); e$x <- 4;
                c(list_case, pair_case,
                  evalq(expr=x+1, envir=e),
                  evalq(env=e, ex=x+1),
                  evalq(x+1, e))
            })",
        )
        .unwrap();
    assert!(result.contains("[1] 3 3 5 5 5"), "{result}");
}

#[test]
fn evalq_null_enclos_return_visibility_and_invalid_environment_match_gnu() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                null_value <- evalq(pi, NULL, NULL);
                return_value <- (function() { evalq(return(7)); 9 })();
                visible <- withVisible(evalq(1))$visible && !withVisible(evalq(invisible(1)))$visible;
                invalid <- tryCatch(evalq(1, envir=TRUE),
                                    error=function(e) conditionMessage(e));
                c(abs(null_value-pi)<1e-12, return_value==9,
                  visible,
                  invalid == \"invalid 'envir' argument of type 'logical'\")
            })",
        )
        .unwrap();
    assert!(result.contains("[1] TRUE TRUE TRUE TRUE"), "{result}");
}

#[test]
fn evalq_survives_gctorture_with_explicit_environment() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                gctorture(TRUE); on.exit(gctorture(FALSE));
                e <- new.env(); e$x <- 8; evalq(x + 1, e)
            })",
        )
        .unwrap();
    assert!(result.contains("[1] 9"), "{result}");
}

#[test]
fn evalq_numeric_frames_are_relative_to_its_caller() {
    let mut s = RSession::new().unwrap();
    let value=s.eval("x<-99;f<-function(){x<-7;g<-function(){x<-8;c(evalq(x,-1),evalq(x,-2),evalq(x,0),evalq(x,1))};g()};result<-f();identical(result,c(8,7,99,7))").unwrap();
    assert_eq!(value.trim(), "[1] TRUE");
}

#[test]
fn eval_and_evalq_invalid_environment_types_match_pinned_gnu() {
    let code = r#"for(value in list(TRUE,'x',quote(x),function()1,expression(x),as.raw(1),1+1i,NA_integer_,NA_real_)) {
    expected <- paste0("invalid 'envir' argument of type '",typeof(value),"'")
    for(fun in list(eval,evalq)) {
        stopifnot(identical(tryCatch(fun(1,envir=value),error=function(e)conditionMessage(e)),expected))
    }
}
stopifnot(identical(tryCatch(eval(1,envir=c(1,2)),error=function(e)conditionMessage(e)),"numeric 'envir' arg not of length one"))
cat('eval admission: complete type family passed\n')"#;
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(Vec::new(), "/tmp"))
            .unwrap(),
    ] {
        assert_eq!(
            session.eval(code).unwrap(),
            "eval admission: complete type family passed\n"
        );
    }
}
