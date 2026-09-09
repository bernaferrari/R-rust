//! S3 Ops dispatch for relational primitives.

use r_embed::RSession;

#[test]
fn relational_specific_methods_run_before_default_comparison() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            r#"
            `<.foo` <- function(e1, e2) "specific"
            x <- structure(1, class = "foo")
            paste(x < 2, x > 2, sep = "|")
            "#,
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] \"specific|FALSE\"");
}

#[test]
fn relational_ops_group_method_handles_all_comparison_operators() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            r#"
            Ops.foo <- function(e1, e2) paste(.Generic, class(e1), class(e2), sep = ":")
            x <- structure(1, class = "foo")
            paste(x < 2, x == 2, x != 2, sep = "|")
            "#,
        )
        .unwrap();
    assert_eq!(
        value.trim(),
        "[1] \"<:foo:numeric|==:foo:numeric|!=:foo:numeric\""
    );
}
