use r_embed::RSession;

#[test]
fn capture_output_message_type_returns_message_text() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("identical(capture.output(message('hello'), type='message'), 'hello')")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn capture_output_split_tees_output_with_file_null() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({ x <- capture.output(cat('a\\n'), file=NULL, split=TRUE); identical(x, 'a') })",
        )
        .unwrap();
    assert_eq!(result.trim(), "a\n[1] TRUE");
}

#[test]
fn capture_output_rejects_split_message_sink() {
    let mut session = RSession::new().unwrap();
    let result = session.eval("capture.output(message('hello'), type='message', split=TRUE)");
    assert!(result.is_err());
}

#[test]
fn capture_output_nested_message_sink_does_not_duplicate_inner_capture() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                x <- capture.output(
                    capture.output(message('inner'), type='message'),
                    type='message');
                identical(x, character())
            })",
        )
        .unwrap();
    assert!(
        result.contains("inner") && result.trim_end().ends_with("[1] TRUE"),
        "{result}"
    );
}

#[test]
fn capture_output_nested_stdout_capture_returns_inner_character_print() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                x <- capture.output(capture.output(cat('inner\\n')));
                identical(x, '[1] \"inner\"')
            })",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn capture_output_forwards_the_nonselected_stream() {
    let mut session = RSession::new().unwrap();
    let message_capture = session
        .eval(
            "local({
                x <- capture.output({ cat('stdout-forward\\n'); message('message-captured') },
                                     type='message');
                identical(x, 'message-captured')
            })",
        )
        .unwrap();
    assert!(
        message_capture.contains("stdout-forward")
            && message_capture.trim_end().ends_with("[1] TRUE"),
        "stdout must pass through message capture: {message_capture}"
    );

    let output_capture = session
        .eval(
            "local({
                x <- capture.output({ cat('stdout-captured\\n'); message('message-forward') },
                                     type='output');
                identical(x, 'stdout-captured')
            })",
        )
        .unwrap();
    assert!(
        output_capture.contains("message-forward")
            && output_capture.trim_end().ends_with("[1] TRUE"),
        "message must pass through output capture: {output_capture}"
    );
}

#[test]
fn capture_output_message_sink_recovers_after_error() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                failed <- tryCatch(
                    capture.output({ message('before'); stop('boom') }, type='message'),
                    error=function(e) conditionMessage(e));
                after <- capture.output(message('after'), type='message');
                identical(c(failed, after), c('boom', 'after'))
            })",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn capture_output_preserves_blank_lines_and_type_prefixes() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval(r#"identical(capture.output(cat("\n\n")), c("", "")) && identical(capture.output(message("hello"),type="m"),"hello")"#).unwrap().trim(), "[1] TRUE");
}
