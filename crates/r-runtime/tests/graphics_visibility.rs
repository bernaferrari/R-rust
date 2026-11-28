//! Drawing primitives must not leak a visible `NULL` into the embedding REPL.

#![cfg(feature = "renderplot-device")]

use r_runtime::embed::PureSession;

#[test]
fn graphics_generics_are_invisible_but_explicit_print_null_is_visible() {
    let mut session = PureSession::new();
    let drawing = session
        .eval_interactive("plot(1:3); lines(1:3); points(1:3)", 320, 240)
        .unwrap();
    assert!(
        drawing.output.is_empty(),
        "drawing leaked output: {drawing:?}"
    );

    let printed = session.eval_output("print(NULL)").unwrap();
    assert_eq!(printed, "NULL
");

    let visible_method = session.eval_output(
        "x <- structure(1:3, class='visibility_probe'); plot.visibility_probe <- function(x, ...) 42; plot(x)",
    ).unwrap();
    assert_eq!(visible_method, "[1] 42
");

    let invisible_method = session.eval_output(
        "plot.visibility_probe <- function(x, ...) invisible(42); plot(x)",
    ).unwrap();
    assert!(invisible_method.is_empty());
}
