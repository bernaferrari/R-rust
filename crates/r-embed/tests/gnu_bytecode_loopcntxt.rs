//! GNU R 4.6.1 STARTLOOPCNTXT/ENDLOOPCNTXT with eval() loop bodies.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R version 4.6.1 (2026-06-24)`).
//! A loop body containing eval() forces the loop-context opcodes even
//! without a non-local return; these fixtures pin the plain-loop case that
//! `gnu_gap_returnjmp` extends with RETURNJMP jumps.

use r_embed::RSession;

fn raw_expression(bytes: &[u8]) -> String {
    format!(
        "as.raw(c({}))",
        bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn load(session: &mut RSession, bytes: &[u8]) {
    session
        .eval(&format!("f <- unserialize({})", raw_expression(bytes)))
        .unwrap();
}

#[test]
fn imported_gnu_eval_for_loop_context_runs_to_completion() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-loopcntxt/eval-for.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(3:1), 0L) && identical(f(integer()), 0L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_eval_repeat_loop_context_returns_value() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-loopcntxt/eval-repeat.rds"),
    );
    assert_eq!(
        session.eval("identical(f(42L), 42L)").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn interpreted_eval_loop_bodies_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "g <- function(x) { for (i in x) eval(quote(NULL)); 0L };                 identical(g(3:1), 0L) && identical(g(integer()), 0L)"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval(
                "h <- function(x) { repeat { eval(quote(NULL)); return(x) } };                 identical(h(42L), 42L)"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
