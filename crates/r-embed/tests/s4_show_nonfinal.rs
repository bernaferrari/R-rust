use r_embed::RSession;

/// GNU R 4.6.1: non-final `obj` whose `show()` calls `stop("boom-show")`
/// prints that error, then `Execution halted`, exits 1, and does not run
/// the next statement.
#[test]
fn nonfinal_show_stop_aborts_following_expression() {
    let mut session = RSession::new().unwrap();
    let failed = session.eval(
        r#"
        saw <- FALSE
        ran <- FALSE
        setClass("Foo", slots = c(x = "numeric"))
        setMethod("show", "Foo", function(object) {
            saw <<- TRUE
            stop("boom-show")
        })
        obj <- new("Foo", x = 1)
        obj
        ran <<- TRUE
        "#,
    );
    let message = failed
        .expect_err("non-final S4 auto-print must abort when show() stops")
        .to_string();
    assert!(
        message.contains("boom-show"),
        "show error was not visible: {message}"
    );

    let later = session.eval(r#"cat(saw, ran, "\n")"#).unwrap();
    assert_eq!(
        later.trim(),
        "TRUE FALSE",
        "show side effect or the following statement did not match: {later}"
    );
}

/// GNU R 4.6.1: `tryCatch(show(new("Outer", ...)))` prints
/// `An object of class "Outer"`, `Slot "i":`, then `CAUGHT inner-boom`,
/// and exits 0.
#[test]
fn nested_show_stop_stays_inside_trycatch() {
    let mut session = RSession::new().unwrap();
    let out = session
        .eval(
            r#"
            setClass("Inner", slots = c(a = "numeric"))
            setMethod("show", "Inner", function(object) stop("inner-boom"))
            setClass("Outer", slots = c(i = "Inner", y = "numeric"))
            tryCatch(
                show(new("Outer", i = new("Inner", a = 1), y = 2)),
                error = function(e) cat("CAUGHT", conditionMessage(e))
            )
            "#,
        )
        .expect("tryCatch around show() must keep an inner stop()");
    let out = out.trim();
    assert!(
        out.contains("An object of class \"Outer\""),
        "outer header missing: {out}"
    );
    assert!(out.contains("Slot \"i\":"), "inner slot header missing: {out}");
    assert!(out.contains("CAUGHT"), "handler did not run: {out}");
    assert!(out.contains("inner-boom"), "caught message missing: {out}");
}

/// GNU R 4.6.1 auto-print of `methods::getClassDef("standardGeneric")` includes
/// `Class "standardGeneric" [package "methods"]`,
/// `Class "genericFunction", directly`, and
/// `Known Subclasses: "standardGenericWithTrace"`.
/// `new("envRefClass")` prints
/// `Reference class object of class "envRefClass"`.
/// Both are non-final here, so a following assignment must still run.
#[test]
fn class_def_and_env_ref_class_auto_print_succeed() {
    let mut session = RSession::new().unwrap();
    let class_def = session
        .eval(
            r#"
            methods::getClassDef("standardGeneric")
            class_def_ran <- TRUE
            "#,
        )
        .expect("getClassDef auto-print must not abort the script");
    let class_def = class_def.trim();
    assert!(
        class_def.contains("Class \"standardGeneric\" [package \"methods\"]"),
        "class def: {class_def}"
    );
    assert!(
        class_def.contains("Class \"genericFunction\", directly"),
        "class def: {class_def}"
    );
    assert!(
        class_def.contains("Known Subclasses: \"standardGenericWithTrace\""),
        "class def: {class_def}"
    );
    assert_eq!(
        session.eval("class_def_ran").unwrap().trim(),
        "[1] TRUE",
        "assignment after getClassDef did not run"
    );

    let ref_class = session
        .eval(
            r#"
            new("envRefClass")
            ref_ran <- TRUE
            "#,
        )
        .expect("new(\"envRefClass\") auto-print must not abort the script");
    assert!(
        ref_class
            .trim()
            .contains("Reference class object of class \"envRefClass\""),
        "envRefClass: {ref_class}"
    );
    assert_eq!(
        session.eval("ref_ran").unwrap().trim(),
        "[1] TRUE",
        "assignment after new(\"envRefClass\") did not run"
    );
}

/// GNU R 4.6.1 auto-print of `data.frame(a=1)` is `  a` / `1 1`.
#[test]
fn data_frame_auto_print_shows_column() {
    let mut session = RSession::new().unwrap();
    let shown = session
        .eval(
            r#"
            data.frame(a = 1)
            df_ran <- TRUE
            "#,
        )
        .expect("data.frame auto-print must stay on the success path");
    let shown = shown.trim();
    assert!(
        shown.contains("a\n1 1"),
        "data.frame column was not shown: {shown}"
    );
    assert_eq!(session.eval("df_ran").unwrap().trim(), "[1] TRUE");
}
