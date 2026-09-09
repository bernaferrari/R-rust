use r_embed::RSession;

#[test]
fn explicit_restart_exits_are_distinct_dynamic_environments() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "withRestarts({rs <- computeRestarts(); identical(c(is.environment(rs[[1]]$exit), is.environment(rs[[2]]$exit), identical(rs[[1]]$exit, rs[[2]]$exit)),c(TRUE,TRUE,FALSE))}, a=function() 1, b=function() 2)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn implicit_abort_restart_has_gnu_shape() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("r <- findRestart('abort'); c(identical(length(r),2L), is.null(names(r)), is.null(r$exit))")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE");
}

#[test]
fn restart_metadata_survives_collection_at_every_allocation() {
    let mut session = RSession::new().unwrap();
    let result = session.eval(
        "f<-function(){gctorture(TRUE);on.exit(gctorture(FALSE));withRestarts({r<-computeRestarts();gc();identical(c(is.environment(r[[1]]$exit),is.null(r[[2]]$exit)),c(TRUE,TRUE))},again=function()42L)};f()"
    ).unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn implicit_abort_unwinds_to_host_without_becoming_an_r_error() {
    let mut session = RSession::new().unwrap();
    for argument in ["'abort'", "findRestart('abort')"] {
        let code = format!(
            "trace<-character();f<-function(){{on.exit({{gc();trace<<-c(trace,'cleanup')}});tryCatch(invokeRestart({argument}),error=function(e)trace<<-c(trace,'caught'))}};f();trace<-c(trace,'wrong continuation')"
        );
        assert!(
            session
                .eval(&code)
                .unwrap_err()
                .to_string()
                .contains("execution aborted")
        );
        assert_eq!(
            session.eval("identical(trace,'cleanup')").unwrap().trim(),
            "[1] TRUE"
        );
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
    }
    assert_eq!(
        session
            .eval("withRestarts(invokeRestart('abort'),abort=function()42L)")
            .unwrap()
            .trim(),
        "[1] 42"
    );
}
