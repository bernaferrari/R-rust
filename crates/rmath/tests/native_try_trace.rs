#![cfg(feature = "memory-profiling")]
//! Real closure and native tryCatch frames compose into GNU's trace chain.
use rmath::android::{RSession, RValue};

#[test]
fn native_trycatch_trace_keeps_gnu_frames_around_the_real_try_closure() {
    let mut session = RSession::new();
    let output = session.eval(
        r#"local({x<-c(1L,2L);y<-x;invisible(tracemem(x));try(x[[1L]]<-3L,silent=TRUE);TRUE})"#,
    );
    assert!(
        matches!(output.typed, RValue::Logical(Some(true))),
        "{output:?}"
    );
    let lines = output
        .stdout
        .lines()
        .filter(|line| line.starts_with("tracemem["))
        .collect::<Vec<_>>();
    assert!(
        !lines.is_empty(),
        "expected actual duplicate trace: {output:?}"
    );
    assert!(
        lines
            .iter()
            .all(|line| line.contains(": doTryCatch tryCatchOne tryCatchList tryCatch try ")),
        "unexpected trace frames: {lines:?}"
    );
}
