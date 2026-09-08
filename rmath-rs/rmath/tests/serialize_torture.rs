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

use rmath::android::{RSession, RValue};

fn eval_script(session: &mut RSession, code: &str) -> String {
    let result = session.eval(code);
    assert!(
        !matches!(result.typed, RValue::Error(_)),
        "{}",
        result.output
    );
    result.output
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

#[test]
fn nested_closures_on_exit_survive_collection() {
    let mut session = RSession::new();
    let out = eval_script(
        &mut session,
        "f <- function() { on.exit(cat('outer')); g <- function() { on.exit(cat('inner')); gc(); 7L }; g() }; cat(f())",
    );
    assert!(out.contains("innerouter7"), "{out}");
}
