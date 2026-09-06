//! GC-torture × serialize round-trip under Miri: evidence for the
//! `with_arena` reentrancy discipline (P1). `gctorture(TRUE)` forces a
//! full mark/sweep on every allocation, so the serialize file/memory
//! callbacks — which run inside `with_arena` lends — exercise collector
//! reentry maximally. A P1 violation here surfaces as Miri aliasing UB.

use rmath::sexp::session::RSession;

fn eval_script(session: &mut RSession, code: &str) -> String {
    let (result, output, _visible) = session.eval_script_with_output_capture(code);
    result.expect("script must eval");
    output.stdout
}

#[test]
fn serialize_roundtrip_under_gctorture() {
    let mut session = RSession::new();
    eval_script(&mut session, "gctorture(TRUE)");
    // Serialized-expression round-trip (exercises OutStringVec/InStringVec
    // and the Rfile callbacks through allocating paths).
    let out = eval_script(
        &mut session,
        r#"
e <- expression(a = 1L, b = "x", c = TRUE, d = 2.5, e = 1 + 2, f = quote(foo(bar = 3)))
stopifnot(identical(typeof(as.list(e)[[5]]), "language"))
f <- tempfile()
save(list = "e", file = f, ascii = TRUE)
rm(e)
loaded <- load(f, envir = globalenv())
stopifnot(identical(loaded, "e"))
cat(paste(vapply(as.list(e), typeof, ""), collapse = "|"), "\n")
"#,
    );
    eval_script(&mut session, "gctorture(FALSE)");
    assert!(
        out.contains("integer|character|logical|double|language|language"),
        "tortured round-trip output wrong: {out}"
    );
}
