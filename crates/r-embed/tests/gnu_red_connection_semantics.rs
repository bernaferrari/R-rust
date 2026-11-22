//! GNU behavior probes: expected results recorded in fixtures/gnu-differential-wave9/oracle.json.
//! Normal assertions intentionally expose unresolved parity gaps; Beads: rport-g41y.

use r_embed::RSession;

#[test]
fn nested_sink_inside_capture_bypasses_outer_capture() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                p <- tempfile();
                captured <- capture.output({ sink(p); cat('inner\\n'); sink() });
                identical(captured, character()) && identical(readLines(p, warn=FALSE), 'inner')
            })",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn write_lines_auto_opens_and_closes_unopened_file_connection() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                p <- tempfile(); con <- file(p);
                writeLines('w', con);
                !isOpen(con) && identical(readLines(p, warn=FALSE), 'w')
            })",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn read_lines_auto_opens_and_closes_unopened_file_connection() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "local({
                p <- tempfile(); writeLines('r', p);
                con <- file(p); value <- readLines(con);
                identical(c(value, isOpen(con)), c('r', FALSE))
            })",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn read_char_closes_auto_opened_connection_when_warning_unwinds() {
    let mut session = RSession::new().unwrap();
    let result = session.eval("local({
        p <- tempfile(); writeBin(as.raw(c(97, 0, 98)), p);
        con <- file(p);
        value <- tryCatch(readChar(con, 3L), warning=function(w) 'warning');
        closed <- !isOpen(con); close(con); unlink(p);
        identical(value, 'warning') && closed
    })").unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}
