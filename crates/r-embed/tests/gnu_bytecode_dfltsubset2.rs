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

#[test]
fn imported_gnu_empty_subset2_errors_on_default_and_dispatches_objects() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dfltsubset2/empty.rds"),
    );
    assert!(
        session.eval("f(list(1L,2L))").is_err(),
        "default x[[]] is a missing subscript"
    );
    assert_eq!(
        session
            .eval("d<-structure(list(1L,2L), class='foo'); `[[.foo`<-function(x,i) 'got'; identical(f(d), 'got')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(list(1L,2L), class='foo'); `[[.foo`<-function(x,i) 'got'; identical(withVisible(f(d)), list(value='got', visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_dfltsubset2_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dfltsubset2/empty.rds");
    // GETVAR x; STARTSUBSET2 call=0 label=8; DOMISSING; DFLTSUBSET2; RETURN.
    // Flip DOMISSING=30 to LDTRUE=18 so retained x[[]] becomes x[[TRUE]].
    let words = [12, 20, 1, 69, 0, 8, 30, 70, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&18_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("identical(f(list(1L,2L)), 1L)")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated DOMISSING must extract through TRUE, not retained missing"
    );
}

#[test]
fn malformed_startsubset2_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-dfltsubset2/empty.rds");
    let words = [12, 20, 1, 69, 0, 8, 30, 70, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 69, 0, 4, 1, 1, 1, 1, 1];
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
        session.eval("f(list(1L,2L))").is_err(),
        "empty-stack STARTSUBSET2 must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
