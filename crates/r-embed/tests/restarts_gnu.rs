use r_embed::RSession;

#[test]
fn invoked_restart_is_not_visible_inside_its_handler() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("withRestarts({invokeRestart(\"foo\")}, foo=function() is.null(findRestart(\"foo\")))")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn later_restart_remains_visible_to_earlier_handler() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("withRestarts(invokeRestart(\"a\"), a=function() !is.null(findRestart(\"b\")), b=function() 1)")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}
