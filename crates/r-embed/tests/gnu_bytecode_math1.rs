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

// GETVAR x; MATH1 call=0, fun=6 (sin); RETURN.
const SIN_WORDS: [i32; 7] = [12, 20, 1, 118, 0, 6, 1];

#[test]
fn imported_gnu_math1_preserves_values_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-math1/sin.rds"),
    );
    assert_eq!(
        session
            .eval("isTRUE(all.equal(f(pi/2), 1)) && identical(typeof(f(0)), 'double')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(0)), list(value=0, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL)); isTRUE(all.equal(g(0), 0))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-math1/expm1.rds"),
    );
    assert_eq!(
        session
            .eval("isTRUE(all.equal(f(1), exp(1)-1)) && identical(withVisible(f(0)), list(value=0, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_math1_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-math1/sin.rds");
    // Flip MATH1 fun index sin=6 -> cos=5. GNU checks CAR(call)==math1funs[i];
    // retained source is still sin(x), so a source fallback would return 0.
    let offset = unique_stream_offset(original, &SIN_WORDS);
    let mut changed = original.to_vec();
    changed[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&5_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    let err = session.eval("f(0)");
    assert!(
        err.is_err(),
        "mutated MATH1 index must not run retained sin(x), got {err:?}"
    );
    assert!(
        format!("{err:?}").to_lowercase().contains("mismatch"),
        "GNU MATH1 index mutation errors with compiler/interpreter mismatch, got {err:?}"
    );
}

#[test]
fn malformed_math1_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-math1/sin.rds");
    let offset = unique_stream_offset(original, &SIN_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 118, 0, 6, 1, 1, 1];
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
        session.eval("f(0)").is_err(),
        "empty-stack MATH1 must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
