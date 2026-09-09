use r_embed::RSession;

#[test]
fn invoked_restart_is_hidden_from_its_handler() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "withRestarts({ invokeRestart('foo') }, foo=function() is.null(findRestart('foo')))"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn invoking_inner_restart_keeps_outer_same_name_restart() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            "withRestarts(withRestarts(invokeRestart('foo'), foo=function() is.null(findRestart('foo'))), foo=function() 99)",
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] FALSE");
}

#[test]
fn restart_handler_arguments_and_missing_restart_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("withRestarts(invokeRestart('foo', 20), foo=function(x) x + 2)")
            .unwrap()
            .trim(),
        "[1] 22"
    );
    assert_eq!(
        session
            .eval("tryInvokeRestart('missing_restart')")
            .unwrap()
            .trim(),
        "NULL"
    );
}

#[test]
fn restart_handler_error_restores_dynamic_stack() {
    let mut session = RSession::new().unwrap();
    assert!(
        session
            .eval("withRestarts(invokeRestart('foo'), foo=function() stop('boom'))")
            .is_err()
    );
    assert_eq!(
        session.eval("is.null(findRestart('foo'))").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn outer_restart_unwinds_before_handler_and_skips_inner_continuation() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            r#"
        trace <- character()
        f <- function() {
          on.exit({gc(); trace <<- c(trace, 'cleanup')})
          invokeRestart('outer', c(20L,22L))
        }
        value <- withRestarts({
          withRestarts(f(), inner=function() 0)
          trace <- c(trace, 'wrong continuation')
        }, outer=function(x) {
          gc()
          stopifnot(is.null(findRestart('inner')), is.null(findRestart('outer')))
          trace <<- c(trace, 'handler')
          sum(x)
        })
        identical(trace,c('cleanup','handler')) && identical(value,42L)
    "#,
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn warning_frame_does_not_swallow_an_outer_restart() {
    let mut session = RSession::new().unwrap();
    let result = session.eval("withRestarts(withCallingHandlers(warning('x'),warning=function(w)invokeRestart('outer',7L)),outer=function(x)x)").unwrap();
    assert_eq!(result.trim(), "[1] 7");
}

#[test]
fn restart_arguments_preserve_language_objects_as_data() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("identical(withRestarts(invokeRestart('foo',quote(stop('must not execute'))),foo=function(x){gc();x}),quote(stop('must not execute')))").unwrap().trim(), "[1] TRUE");
}

#[test]
fn sibling_restart_extents_follow_specification_order() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            r#"
        withRestarts(invokeRestart('first'),
          first=function() {
            stopifnot(is.null(findRestart('first')), !is.null(findRestart('second')))
            invokeRestart('second',22L)
          },
          second=function(x) {
            stopifnot(is.null(findRestart('first')),is.null(findRestart('second')))
            x
          })
    "#,
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] 22");
}

#[test]
fn restart_argument_preserves_a_quoted_bound_symbol() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("x<-7;identical(withRestarts(invokeRestart('foo',quote(x)),foo=function(v)v),quote(x))").unwrap().trim(), "[1] TRUE");
}
