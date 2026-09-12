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

// GETVAR x; LOG call=0; RETURN.
const LOG_WORDS: [i32; 6] = [12, 20, 1, 116, 0, 1];

#[test]
fn imported_gnu_log_preserves_values_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-log/log.rds"),
    );
    assert_eq!(
        session
            .eval("isTRUE(all.equal(f(c(1, exp(1))), c(0, 1))) && identical(typeof(f(1)), 'double')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(1)), list(value=0, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL)); isTRUE(all.equal(g(exp(1)), 1))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    let bad = session.eval("f('a')");
    assert!(bad.is_err(), "log(\"a\") must error");
    assert!(
        format!("{bad:?}").contains("non-numeric"),
        "GNU log(\"a\") errors with non-numeric argument, got {bad:?}"
    );
}

#[test]
fn mutated_gnu_log_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-log/log.rds");
    // Flip LOG=116 -> EXP=50. Bytecode becomes exp(x); retained source is still log(x).
    let offset = unique_stream_offset(original, &LOG_WORDS);
    let mut changed = original.to_vec();
    changed[offset + 3 * 4..offset + 4 * 4].copy_from_slice(&50_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session.eval("identical(f(0), 1)").unwrap().trim(),
        "[1] TRUE",
        "mutated LOG must compute exp(0)=1, not retained log(0)"
    );
}

#[test]
fn malformed_log_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-log/log.rds");
    let offset = unique_stream_offset(original, &LOG_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 116, 0, 1, 1, 1];
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
        session.eval("f(5)").is_err(),
        "empty-stack LOG must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
