use r_embed::RSession;

/// GNU R 4.6.1 auto-prints a final `new("Foo", x=1)` through showDefault.
/// A final value used to render as `[unknown; length=0]`.
/// `paste(capture.output(show(obj)))` is itself auto-printed as a character
/// vector (quotes and newlines escaped), so the comparison cats that text.
#[test]
fn final_s4_auto_print_matches_show_default() {
    let mut session = RSession::new().unwrap();
    let sum = session.eval("1+1").unwrap();
    assert_eq!(sum.trim(), "[1] 2");

    let shown = session
        .eval(
            r#"
            setClass("Foo", slots = c(x = "numeric"))
            obj <- new("Foo", x = 1)
            cat(paste(capture.output(show(obj)), collapse = "\n"))
            "#,
        )
        .unwrap();
    let auto = session
        .eval(
            r#"
            obj <- new("Foo", x = 1)
            obj
            "#,
        )
        .unwrap();
    let shown = shown.trim();
    let auto = auto.trim();
    assert!(
        shown.contains("An object of class \"Foo\""),
        "show: {shown}"
    );
    assert!(
        auto.contains("An object of class \"Foo\""),
        "auto-print: {auto}"
    );
    assert!(auto.contains("Slot \"x\":"), "auto-print: {auto}");
    assert!(auto.contains("[1] 1"), "auto-print: {auto}");

    // tryCatch around new() does not include auto-print of the returned object.
    let wrapped = session.eval(
        r#"
        setMethod("show", "Foo", function(object) stop("boom-show"))
        tryCatch(new("Foo", x = 1), error = function(e) conditionMessage(e))
        "#,
    );
    let wrapped_message = match &wrapped {
        Ok(text) => text.clone(),
        Err(err) => err.to_string(),
    };
    assert!(
        wrapped_message.contains("boom-show"),
        "show error was not visible: {wrapped_message}"
    );

    let failed = session.eval(
        r#"
        new("Foo", x = 1)
        "#,
    );
    let message = failed
        .expect_err("final S4 expression must fail eval when show() stops")
        .to_string();
    assert!(
        message.contains("boom-show"),
        "show error was not visible: {message}"
    );
}
