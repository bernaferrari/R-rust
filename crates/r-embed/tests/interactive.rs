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
