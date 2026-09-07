//! GC-torture × serialize round-trip under Miri: evidence for the
//! `with_arena` reentrancy discipline (P1). `gctorture(TRUE)` forces a
//! full mark/sweep on every allocation, so the serialize callbacks —
//! which run inside `with_arena` lends — exercise collector reentry
//! maximally. A P1 violation here surfaces as Miri aliasing UB.
//!
//! The payload is deliberately MINIMAL: under Miri, torture multiplies
//! the interpreter cost of every allocation by the cost of a full
//! mark/sweep, so this test is sized to complete inside the nightly
//! job's budget. The full-corpus torture differential (native, fast)
//! lives in scripts/gc_torture_stress.sh.

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
    // Minimal allocating round-trip through the serialize paths: a small
    // expression vector with a language element, saved and reloaded.
    let out = eval_script(
        &mut session,
        r#"
e <- expression(1L, "x", 1 + 2)
f <- tempfile()
save(list = "e", file = f, ascii = TRUE)
rm(e)
loaded <- load(f, envir = globalenv())
cat(loaded, "|", length(e), "|", typeof(e[[3]]), "\n")
"#,
    );
    eval_script(&mut session, "gctorture(FALSE)");
    assert!(
        out.contains("e | 3 | language"),
        "tortured round-trip output wrong: {out}"
    );
}
