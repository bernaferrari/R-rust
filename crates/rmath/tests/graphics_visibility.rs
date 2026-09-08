//! Drawing primitives must not leak a visible `NULL` into the embedding REPL.

#![cfg(feature = "renderplot-device")]

use r_graphics_engine::Scene;
use rmath::android::RSession;

#[test]
fn graphics_generics_are_invisible_but_explicit_print_null_is_visible() {
    let mut session = RSession::new();
    let mut scene = Scene::new(320, 240);
    let drawing = session
        .eval_script_with_renderplot_backend("plot(1:3); lines(1:3); points(1:3)", &mut scene);
    assert!(
        drawing.output.is_empty(),
        "drawing leaked output: {drawing:?}"
    );

    let printed = session.eval("print(NULL)");
    assert_eq!(printed.output, "NULL\n");

    let visible_method = session.eval(
        "x <- structure(1:3, class='visibility_probe'); plot.visibility_probe <- function(x, ...) 42; plot(x)",
    );
    assert_eq!(visible_method.output, "[1] 42");

    let invisible_method =
        session.eval("plot.visibility_probe <- function(x, ...) invisible(42); plot(x)");
    assert!(invisible_method.output.is_empty());
}
