//! GNU R bytecode link/visibility opcodes: VISIBLE, INCLNKSTK, DECLNKSTK,
//! GETINTLBUILTIN (and the INCLNK/DECLNK pair for pre-version-12 streams).
//!
//! Oracle: pinned trunk `bac583951b728e97b9786804d3b4081f0fe18df5`
//! (r90451).  VISIBLE is emitted for `(` in tail position, INCLNKSTK /
//! DECLNKSTK protect stack values across complex assignments inside
//! argument lists, and GETINTLBUILTIN resolves `.Internal()` builtins.

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
fn imported_gnu_visible_paren_expression_keeps_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-links/visible.rds"),
    );
    assert_eq!(
        session.eval("identical(f(1), 2)").unwrap().trim(),
        "[1] TRUE"
    );
    // `(x + 1)` in tail position must stay visible like GNU.
    assert_eq!(
        session
            .eval("identical(withVisible(f(1)), list(value=2, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_inclnkstk_protects_stack_across_assignment() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-links/incnkstk.rds"),
    );
    assert_eq!(
        session
            .eval("x <- list(a=0); identical(f(x), c(1, 2)) && identical(x$a, 1)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_intlbuiltin_runs_internal_builtin() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-links/intlbuiltin.rds"),
    );
    // .Internal(Sys.getpid()) returns the visible session pid.
    assert_eq!(
        session
            .eval("p <- f(NULL); is.numeric(p) && length(p) == 1 && p > 0 && withVisible(f(NULL))$visible")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn interpreted_paren_and_internal_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("g <- compiler::cmpfun(function(x) (x + 1)); identical(g(1), 2)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    // rport's own compiler keeps `(` transparent like GNU.
    assert_eq!(
        session
            .eval("g2 <- compiler::cmpfun(function(x) (x)); identical(g2(7), 7)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
