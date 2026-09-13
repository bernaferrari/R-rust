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
    assert_eq!(
        offsets.len(),
        1,
        "fixture must contain one exact instruction stream"
    );
    offsets[0]
}

// GETVAR x; SEQALONG call=0; RETURN.
const SEQALONG_WORDS: [i32; 6] = [12, 20, 1, 121, 0, 1];

#[test]
fn imported_gnu_seqalong_preserves_values_types_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-seqalong/seqalong.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(letters[1:3]), 1:3) && identical(typeof(f(letters[1:3])), 'integer')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(list()), integer(0)) && identical(typeof(f(NULL)), 'integer')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(5), 1L) && identical(f(c(10, 20, 30)), 1:3)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(1:3)), list(value=1:3, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL)); identical(g(c('a','b')), 1:2)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_seqalong_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-seqalong/seqalong.rds");
    // Flip SEQALONG=121 -> SEQLEN=122. Retained source is still seq_along(x).
    let offset = unique_stream_offset(original, &SEQALONG_WORDS);
    let mut changed = original.to_vec();
    changed[offset + 3 * 4..offset + 4 * 4].copy_from_slice(&122_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session.eval("identical(f(4), 1:4)").unwrap().trim(),
        "[1] TRUE",
        "mutated SEQALONG must run seq_len(4), not retained seq_along(4)"
    );
}

#[test]
fn malformed_seqalong_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-seqalong/seqalong.rds");
    let offset = unique_stream_offset(original, &SEQALONG_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 121, 0, 1, 1, 1];
    for (i, word) in replacement.iter().enumerate() {
        let at = offset + i * 4;
        malformed[at..at + 4].copy_from_slice(&word.to_be_bytes());
    }

    let mut session = RSession::new().unwrap();
    let loaded = session.eval(&format!(
        "f <- unserialize({})",
        raw_expression(&malformed)
    ));
    if loaded.is_err() {
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
        return;
    }
    assert!(
        session.eval("f(letters[1:3])").is_err(),
        "empty-stack SEQALONG must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
