//! GNU R 4.6.1 RETURNJMP / nested return / on.exit / eval-loop jumps.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R Under development (unstable) (2026-08-27 r90451)` / 4.6.1).
//! Beads: rport-dl8y (first_jump_target / endcontext continuation).

use r_embed::RSession;

fn raw_expression(bytes: &[u8]) -> String {
    format!(
        "as.raw(c({}))",
        bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
    )
}

fn load(session: &mut RSession, bytes: &[u8]) {
    session
        .eval(&format!("f <- unserialize({})", raw_expression(bytes)))
        .unwrap();
}

#[test]
fn on_exit_return_replaces_function_result() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("f <- function() { on.exit(return(99)); 1 }; identical(f(), 99)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("f <- function() { on.exit(return(99)); return(1) }; identical(f(), 99)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn intervening_on_exit_return_cancels_restart_jump() {
    // GNU first_jump_target lands on `inner` (it has on.exit), endcontext
    // clears jumptarget, and return() from on.exit replaces the restart.
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "outer <- function() {
                   withRestarts({ inner() }, done = function() 'restarted')
                 }
                 inner <- function() {
                   on.exit(return('from-onexit'))
                   invokeRestart('done')
                 }
                 identical(outer(), 'from-onexit')"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn intervening_on_exit_sees_its_own_call() {
    // While GNU runs the intermediate on.exit, sys.call() is inner(),
    // not helper() / invokeRestart().
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "trace <- character()
                 outer <- function() {
                   withRestarts({ inner() }, escape = function(v) v)
                 }
                 inner <- function() {
                   on.exit(trace <<- c(trace, paste(deparse(sys.call()), collapse='')))
                   helper()
                 }
                 helper <- function() invokeRestart('escape', 7)
                 identical(outer(), 7) && identical(trace, 'inner()')"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn eval_next_and_break_target_enclosing_loop() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "f <- function() {
                   x <- c()
                   for (i in 1:3) {
                     eval(quote({
                       if (i == 2) next
                       x <- c(x, i)
                     }))
                   }
                   x
                 }
                 identical(f(), c(1L, 3L))"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval(
                "f <- function() {
                   x <- c()
                   for (i in 1:3) {
                     eval(quote({
                       if (i == 2) break
                       x <- c(x, i)
                     }))
                   }
                   x
                 }
                 identical(f(), 1L)"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn trycatch_next_and_break_target_enclosing_loop() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "f <- function() {
                   x <- c()
                   for (i in 1:3) {
                     tryCatch({
                       if (i == 2) next
                       x <- c(x, i)
                     }, error = function(e) NULL)
                   }
                   x
                 }
                 identical(f(), c(1L, 3L))"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval(
                "f <- function() {
                   x <- c()
                   for (i in 1:3) {
                     tryCatch({
                       if (i == 2) break
                       x <- c(x, i)
                     }, error = function(e) NULL)
                   }
                   x
                 }
                 identical(f(), 1L)"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_returnjmp_from_eval_loop_matches_gnu() {
    // GNU emits STARTFOR + STARTLOOPCNTXT + RETURNJMP + STEPFOR.
    // rport currently rejects the STEPFOR framing of that mix.
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-returnjmp/returnjmp-eval-loop.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(-1L, 2L, 3L)), 2L) && identical(f(integer()), 0L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_returnjmp_from_repeat_matches_gnu() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-returnjmp/returnjmp-repeat.rds"),
    );
    assert_eq!(session.eval("identical(f(7L), 7L)").unwrap().trim(), "[1] TRUE");
}
