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

// GETVAR n; SEQLEN call=0; RETURN.
const SEQLEN_WORDS: [i32; 6] = [12, 20, 1, 122, 0, 1];

#[test]
fn imported_gnu_seqlen_preserves_values_types_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-seqlen/seqlen.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(3L), 1:3) && identical(typeof(f(3L)), 'integer')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(3), 1:3) && identical(typeof(f(0)), 'integer') && identical(f(0), integer(0))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(3L)), list(value=1:3, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("w<-NULL; got<-withCallingHandlers(f(1:2), warning=function(wrn){w<<-conditionMessage(wrn); invokeRestart('muffleWarning')}); identical(got, 1L) && grepl('length.out', w, fixed=TRUE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    let neg = session.eval("f(-1)");
    assert!(neg.is_err(), "seq_len(-1) must error");
    assert!(
        format!("{neg:?}").contains("non-negative"),
        "GNU seq_len(-1) errors with non-negative integer, got {neg:?}"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL)); identical(g(4L), 1:4)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_seqlen_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-seqlen/seqlen.rds");
    // Flip SEQLEN=122 -> SEQALONG=121. Retained source is still seq_len(n).
    let offset = unique_stream_offset(original, &SEQLEN_WORDS);
    let mut changed = original.to_vec();
    changed[offset + 3 * 4..offset + 4 * 4].copy_from_slice(&121_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session.eval("identical(f(4), 1L)").unwrap().trim(),
        "[1] TRUE",
        "mutated SEQLEN must run seq_along(4), not retained seq_len(4)"
    );
}

#[test]
fn malformed_seqlen_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-seqlen/seqlen.rds");
    let offset = unique_stream_offset(original, &SEQLEN_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 122, 0, 1, 1, 1];
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
        session.eval("f(3L)").is_err(),
        "empty-stack SEQLEN must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
