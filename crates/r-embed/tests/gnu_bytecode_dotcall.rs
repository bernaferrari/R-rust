//! GNU R 4.6.1 DOTCALL=119 for unnamed `.Call` with ≤16 args.
//!
//! Oracle: pinned trunk plus Homebrew 4.6.1.  The native symbol is
//! intentionally missing so the test asserts the opcode ran (GNU .Call
//! error) rather than BCMISMATCH / source fallback.

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

fn unique_stream_offset(bytes: &[u8], words: &[i32]) -> usize {
    let encoded = words
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect::<Vec<_>>();
    let offsets = bytes
        .windows(encoded.len())
        .enumerate()
        .filter_map(|(offset, candidate)| (candidate == encoded).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1, "fixture must contain one exact stream");
    offsets[0]
}

// LDCONST name; DOTCALL call=0 nargs=0; RETURN
const ZERO_WORDS: [i32; 7] = [12, 16, 1, 119, 0, 0, 1];
// LDCONST name; GETVAR x; DOTCALL call=0 nargs=1; RETURN
const ONE_WORDS: [i32; 9] = [12, 16, 1, 20, 2, 119, 0, 1, 1];

#[test]
fn imported_gnu_dotcall_zero_runs_call_not_mismatch() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dotcall/zero.rds"),
    );
    let err = session
        .eval("f()")
        .expect_err("missing native symbol must error");
    let text = err.to_string();
    assert!(
        !text.contains("BCMISMATCH"),
        "DOTCALL must execute, not refuse: {text}"
    );
    assert!(
        text.contains("rportDotcallZero") || text.contains(".Call") || text.contains("NULL"),
        "{text}"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn imported_gnu_dotcall_one_runs_call_not_mismatch() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dotcall/one.rds"),
    );
    let err = session
        .eval("f(1L)")
        .expect_err("missing native symbol must error");
    let text = err.to_string();
    assert!(
        !text.contains("BCMISMATCH"),
        "DOTCALL must execute, not refuse: {text}"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn malformed_dotcall_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-dotcall/zero.rds");
    let offset = unique_stream_offset(original, &ZERO_WORDS);
    let mut malformed = original.to_vec();
    // Drop the LDCONST so DOTCALL sees an empty stack.
    let replacement = [12_i32, 119, 0, 0, 1];
    for (i, word) in replacement.iter().enumerate() {
        let at = offset + i * 4;
        malformed[at..at + 4].copy_from_slice(&word.to_be_bytes());
    }

    let mut session = RSession::new().unwrap();
    let loaded = session.eval(&format!("f <- unserialize({})", raw_expression(&malformed)));
    if loaded.is_err() {
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
        return;
    }
    assert!(
        session.eval("f()").is_err(),
        "empty-stack DOTCALL must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn fixture_contains_expected_dotcall_streams() {
    let zero = include_bytes!("fixtures/gnu-bytecode-dotcall/zero.rds");
    let one = include_bytes!("fixtures/gnu-bytecode-dotcall/one.rds");
    let _ = unique_stream_offset(zero, &ZERO_WORDS);
    let _ = unique_stream_offset(one, &ONE_WORDS);
}
