//! GNU R default-optimize bytecode: DDVAL, DODOTS, MAKECLOSURE, CALLSPECIAL.
//!
//! Oracle: pinned trunk `bac583951b728e97b9786804d3b4081f0fe18df5`
//! (r90451) plus Homebrew 4.6.1.  Default `compiler::cmpfun` (optimize=2)
//! emits these opcodes for `..N` references, `f(...)` splicing, nested
//! `function()` definitions, and special-function calls.  Before the
//! adapter grew these arms, such streams silently fell back to the
//! retained source expression; the opcode-mutation checks prove the
//! instruction stream itself now executes.

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

/// Locate the unique byte encoding of `words` inside `bytes`.
fn unique_stream_offset(bytes: &[u8], words: &[i32]) -> usize {
    let encoded: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
    let hits: Vec<usize> = bytes
        .windows(encoded.len())
        .enumerate()
        .filter(|(_, candidate)| *candidate == encoded.as_slice())
        .map(|(offset, _)| offset)
        .collect();
    assert_eq!(hits.len(), 1, "fixture must contain one exact instruction");
    hits[0]
}

#[test]
fn imported_gnu_ddval_reads_dots_cells() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dots/ddval.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(1,2,3), 7), c(6, 7))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(c(1,2,3), 7, 8, 9), c(6, 7))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    // ..1 with no ... supplied errors like GNU ddfindVar.
    let bad = session.eval("f(c(1,2))");
    assert!(bad.is_err(), "f(c(1,2)) without dots must error");
}

#[test]
fn mutated_gnu_ddval_runs_the_stream_not_the_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dots/ddval.rds");
    // The real DDVAL instruction: PUSHARG(33), DDVAL(21), const 5, PUSHARG.
    let offset = unique_stream_offset(original, &[33, 21, 5, 33]);
    // DDVAL(21) becomes GETVAR(20): the dots lookup degrades into an
    // ordinary lookup that cannot find `..1`.
    let mut changed = original.to_vec();
    changed[offset + 4..offset + 8].copy_from_slice(&20_i32.to_be_bytes());
    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    let result = session.eval("f(c(1,2,3), 7)");
    assert!(
        result.is_err(),
        "mutated DDVAL must not fall back to the retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn imported_gnu_dodots_splices_lazy_dots() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dots/dodots.rds"),
    );
    // Formals are (a, ...): f(1, 2, 3) means sum(2, 3).
    assert_eq!(
        session.eval("identical(f(1, 2, 3), 5)").unwrap().trim(),
        "[1] TRUE"
    );
    // Dots are lazily promised: each side effect fires exactly once, in
    // order, when sum() forces them.
    assert_eq!(
        session
            .eval("n <- 0; g <- function() { n <<- n + 1; n }; identical(f(0, g(), g()), 3) && identical(n, 2)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_makeclosure_runs_the_stream_not_the_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dots/makeclosure.rds");
    // MAKECLOSURE(41) with its constant index becomes GETVAR(20): the
    // formals/body vector is not a symbol, so execution must fail.
    let needle = 41_i32.to_be_bytes();
    let hits: Vec<usize> = original
        .windows(4)
        .enumerate()
        .filter(|(_, candidate)| *candidate == needle.as_ref())
        .map(|(offset, _)| offset)
        .collect();
    assert_eq!(hits.len(), 1, "fixture must contain one MAKECLOSURE opcode");
    let mut changed = original.to_vec();
    changed[hits[0]..hits[0] + 4].copy_from_slice(&20_i32.to_be_bytes());
    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    let result = session.eval("f(41)");
    assert!(
        result.is_err(),
        "mutated MAKECLOSURE must not fall back to the retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn imported_gnu_makeclosure_builds_nested_closure_in_frame() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dots/makeclosure.rds"),
    );
    assert_eq!(
        session.eval("identical(f(41), 42)").unwrap().trim(),
        "[1] TRUE"
    );
    // The nested closure captures the caller frame.
    assert_eq!(
        session
            .eval("h <- compiler::cmpfun(function(x) { g <- function() x; g() + 1 }); identical(h(1), 2)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_callspecial_runs_special_primitives() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dots/callspecial.rds"),
    );
    // `=`(a, 1) is frame-local like GNU: `a` must not leak to the caller.
    assert_eq!(
        session
            .eval("identical(f(1), 2) && identical(exists('a'), FALSE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
