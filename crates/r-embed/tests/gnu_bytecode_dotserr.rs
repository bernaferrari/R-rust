//! GNU R 4.6.1 DOTSERR=60 for `...` used outside a dots context.

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

// GNU emits DOTSERR with no RETURN (tail error).
const DOTSERR_WORDS: [i32; 2] = [12, 60];

#[test]
fn imported_gnu_dotserr_uses_gnu_message() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dotserr/dotserr.rds"),
    );
    let err = session
        .eval("f()")
        .expect_err("misplaced ... must error");
    let text = err.to_string();
    assert!(
        text.contains("'...' used in an incorrect context"),
        "{text}"
    );
    assert!(!text.contains("BCMISMATCH"), "{text}");
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn mutated_gnu_dotserr_runs_the_stream_not_the_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dotserr/dotserr.rds");
    let offset = unique_stream_offset(original, &DOTSERR_WORDS);
    let mut changed = original.to_vec();
    // DOTSERR=60 -> RETURN=1 on an empty stack. Validator rejects the
    // stream (or eval errors without the retained `...` message).
    changed[offset + 4..offset + 8].copy_from_slice(&1_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    let loaded = session.eval(&format!(
        "f <- unserialize({})",
        raw_expression(&changed)
    ));
    if let Err(err) = loaded {
        assert!(
            err.to_string().contains("RETURN") || err.to_string().contains("stack"),
            "{err}"
        );
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
        return;
    }
    let err = session.eval("f()").expect_err("mutated RETURN must error");
    let text = err.to_string();
    assert!(
        !text.contains("'...' used in an incorrect context"),
        "mutated stream must not run retained DOTSERR source: {text}"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
