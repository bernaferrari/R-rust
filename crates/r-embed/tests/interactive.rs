use r_embed::RSession;

#[test]
fn interactive_evaluation_combines_text_and_plot_once() {
    let mut session = RSession::new().unwrap();
    session.eval("counter <- 0").unwrap();
    let result = session
        .eval_interactive(
            "cat('hello\\n'); counter <- counter + 1; x <- 40; plot(1:2, 2:3); x + 2",
            320,
            240,
        )
        .unwrap();
    assert!(result.output.contains("hello"), "{}", result.output);
    assert!(result.output.contains("[1] 42"), "{}", result.output);
    assert!(result.png.as_ref().is_some_and(|png| !png.is_empty()));
    assert_eq!(session.eval("x").unwrap(), "[1] 40");
    assert_eq!(session.eval("counter").unwrap(), "[1] 1");
    let invisible = session.eval_interactive("assigned <- 9", 320, 240).unwrap();
    assert!(invisible.output.is_empty(), "{}", invisible.output);
}

#[test]
fn interactive_evaluation_omits_png_without_drawing_and_recovers_after_error() {
    let mut session = RSession::new().unwrap();
    let text = session
        .eval_interactive("cat('text\\n'); 1 + 1", 320, 240)
        .unwrap();
    assert!(text.output.contains("text"));
    assert!(text.png.is_none());

    assert!(
        session
            .eval_interactive("plot(1:2); stop('boom')", 320, 240)
            .is_err()
    );
    let recovered = session.eval_interactive("2 + 2", 320, 240).unwrap();
    assert!(recovered.output.contains("[1] 4"), "{}", recovered.output);
    assert!(recovered.png.is_none());
}

#[test]
fn interactive_evaluation_error_keeps_partial_output_and_scene() {
    let mut session = RSession::new().unwrap();
    let error =
        session.eval_interactive("cat('partial\\n'); plot(1:2, 1:2); stop('boom')", 320, 240);
    let (message, output, png) = match error {
        Err(r_embed::RSessionError::EvalErrorWithOutput {
            message,
            output,
            png,
        }) => (message, output, png),
        other => panic!("expected partial interactive error, got {other:?}"),
    };
    assert!(message.contains("boom"));
    assert!(output.contains("partial"), "{output}");
    assert!(png.is_some(), "drawing before the error should be retained");

    let recovered = session.eval_interactive("lines(1:2, 2:1)", 320, 240);
    assert!(
        recovered.is_ok(),
        "session should recover after partial error"
    );
    assert!(recovered.unwrap().png.is_some());
}

#[test]
fn interactive_graphics_scene_persists_across_calls() {
    let mut session = RSession::new().unwrap();
    let first = session
        .eval_interactive("plot(1:2, 1:2)", 320, 240)
        .unwrap();
    let first_png = first.png.expect("plot should produce an image");

    let second = session
        .eval_interactive("lines(1:2, 2:1, col = 'red')", 320, 240)
        .unwrap();
    let second_png = second.png.expect("lines should produce an image");
    assert_ne!(first_png, second_png, "later drawing must update the scene");
    let mut reference = RSession::new().unwrap();
    let combined = reference
        .eval_interactive("plot(1:2, 1:2); lines(1:2, 2:1, col = 'red')", 320, 240)
        .unwrap()
        .png
        .unwrap();
    assert_eq!(
        second_png, combined,
        "split commands must retain every prior layer"
    );
}

#[test]
fn invalid_interactive_canvas_does_not_execute_code() {
    let mut session = RSession::new().unwrap();
    assert!(session.eval_interactive("x <- 9", u32::MAX, 240).is_err());
    let output = session.eval_interactive("exists('x')", 320, 240).unwrap();
    assert!(output.output.contains("FALSE"));
}

#[test]
fn embedded_session_cannot_mutate_process_environment_by_default() {
    let key = "RPORT_EMBED_ISOLATION_REGRESSION";
    let before = std::env::var_os(key);
    let mut session = RSession::new().unwrap();
    let result = session.eval_interactive(
        "cat(Sys.setenv(RPORT_EMBED_ISOLATION_REGRESSION = 'changed')); cat(Sys.unsetenv('RPORT_EMBED_ISOLATION_REGRESSION'))",
        320, 240,
    ).unwrap();
    assert!(result.output.contains("FALSEFALSE"), "{}", result.output);
    assert_eq!(std::env::var_os(key), before);
}

#[test]
fn interactive_scene_budget_overflow_is_recoverable() {
    let mut session = RSession::new().unwrap();
    let text = "x".repeat(4096);
    let code = format!("plot(1:2, 1:2); for (i in 1:5000) text(1, 1, '{text}')");
    let overflow = session.eval_interactive(&code, 320, 240);
    assert!(
        matches!(overflow, Err(r_embed::RSessionError::RenderError(ref message)) if message.contains("16 MiB")),
        "expected retained scene budget error, got {overflow:?}"
    );

    let recovered = session
        .eval_interactive("plot(1:2, 2:1)", 320, 240)
        .unwrap();
    assert!(recovered.png.is_some());
}
