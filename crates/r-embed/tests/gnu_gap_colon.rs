//! GNU R 4.6.1 colon / seq_along / seq_len behavior.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R Under development (unstable) (2026-08-27 r90451)` / 4.6.1).
//! Imported fixtures are uncompressed XDR v2 from `compiler::cmpfun`.
//! Assertions use GNU results; failures document remaining gaps.

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

fn unique_stream_offset(bytes: &[u8], words: &[i32]) -> usize {
    let encoded = words.iter().flat_map(|word| word.to_be_bytes()).collect::<Vec<_>>();
    let offsets = bytes
        .windows(encoded.len())
        .enumerate()
        .filter_map(|(offset, candidate)| (candidate == encoded).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1, "fixture must contain one exact instruction stream");
    offsets[0]
}

#[test]
fn interpreted_colon_values_and_typeof_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("identical(1:5, 1:5) && typeof(1:5) == 'integer'").unwrap().trim(), "[1] TRUE");
    assert_eq!(session.eval("identical(5:1, as.integer(c(5,4,3,2,1))) && typeof(5:1) == 'integer'").unwrap().trim(), "[1] TRUE");
    // GNU constant-folds 1:5.5 to integer 1:5.
    assert_eq!(session.eval("identical(1:5.5, 1:5) && typeof(1:5.5) == 'integer'").unwrap().trim(), "[1] TRUE");
    assert_eq!(session.eval("identical(1:5, seq_len(5))").unwrap().trim(), "[1] TRUE");
}

#[test]
fn interpreted_seq_along_and_seq_len_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("identical(seq_along(letters[1:3]), 1:3) && typeof(seq_along(letters[1:3])) == 'integer'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(seq_len(4), 1:4) && typeof(seq_len(4)) == 'integer'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(seq_len(0), integer(0)) && typeof(seq_len(0)) == 'integer'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(seq_along(NULL), integer(0)) && typeof(seq_along(NULL)) == 'integer'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(seq_along(c(a=10,b=20)), 1:2) && is.null(names(seq_along(c(a=10,b=20))))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn rport_cmpfun_colon_and_seq_match_gnu_values() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("g <- compiler::cmpfun(function(x) x:3L); identical(g(1L), 1:3) && typeof(g(1L)) == 'integer'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g <- compiler::cmpfun(function(x) seq_along(x)); identical(g(letters[1:3]), 1:3)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g <- compiler::cmpfun(function(n) seq_len(n)); identical(g(4), 1:4)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_constant_folded_colon_runs() {
    // Control: GNU emits LDCONST of 1:5, not COLON.OP.
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-colon/colon-const.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(), 1:5) && typeof(f()) == 'integer'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_colon_opcode_matches_gnu_values() {
    // GETVAR x; LDCONST 3L; COLON; RETURN.
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-colon/colon-var.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(1L), 1:3) && typeof(f(1L)) == 'integer'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_colon_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-gap-colon/colon-var.rds");
    // version, GETVAR 1, LDCONST 2, COLON 0, RETURN.
    let words = [12, 20, 1, 16, 2, 120, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    // COLON.OP (120) -> ADD.OP (44). Retained source is still x:3L.
    changed[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&44_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session.eval("identical(f(1L), 4L)").unwrap().trim(),
        "[1] TRUE",
        "mutated COLON->ADD must execute instead of retained x:3L"
    );
}

#[test]
fn imported_gnu_seq_along_opcode_matches_gnu_values() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-colon/seq-along.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(letters[1:3]), 1:3) && typeof(f(letters[1:3])) == 'integer'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_seq_len_opcode_matches_gnu_values() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-colon/seq-len.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(4), 1:4) && typeof(f(4)) == 'integer'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
