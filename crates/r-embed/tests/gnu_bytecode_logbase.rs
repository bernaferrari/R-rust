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

// GETVAR x; LDCONST 10; LOGBASE call=0; RETURN.
const LOGBASE_WORDS: [i32; 8] = [12, 20, 1, 16, 2, 117, 0, 1];

#[test]
fn imported_gnu_logbase_preserves_values_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-logbase/logbase.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(1,10,100)), c(0,1,2)) && identical(typeof(f(100)), 'double')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(100)), list(value=2, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL)); identical(g(10), 1)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    let bad_x = session.eval("f('a')");
    assert!(bad_x.is_err(), "log(\"a\", 10) must error");
    assert!(
        format!("{bad_x:?}").contains("non-numeric"),
        "GNU log(\"a\", 10) errors with non-numeric argument, got {bad_x:?}"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-logbase/logbase-var.rds"),
    );
    assert_eq!(
        session.eval("identical(f(100, 10), 2)").unwrap().trim(),
        "[1] TRUE"
    );
    let bad_base = session.eval("f(10, 'x')");
    assert!(bad_base.is_err(), "log(10, \"x\") must error");
    assert!(
        format!("{bad_base:?}").contains("non-numeric"),
        "GNU log(10, \"x\") errors with non-numeric argument, got {bad_base:?}"
    );
}

#[test]
fn mutated_gnu_logbase_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-logbase/logbase.rds");
    // Flip LOGBASE=117 -> DIV=47. Bytecode becomes x/10; retained source is still log(x, 10).
    let offset = unique_stream_offset(original, &LOGBASE_WORDS);
    let mut changed = original.to_vec();
    changed[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&47_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session.eval("identical(f(100), 10)").unwrap().trim(),
        "[1] TRUE",
        "mutated LOGBASE must compute 100/10=10, not retained log(100, 10)=2"
    );
}

#[test]
fn malformed_logbase_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-logbase/logbase.rds");
    let offset = unique_stream_offset(original, &LOGBASE_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 117, 0, 1, 1, 1, 1, 1];
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
        session.eval("f(10)").is_err(),
        "empty-stack LOGBASE must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
