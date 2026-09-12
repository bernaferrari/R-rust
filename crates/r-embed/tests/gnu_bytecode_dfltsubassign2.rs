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
fn imported_gnu_empty_subassign2_dispatches_objects_and_errors_on_default() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dfltsubassign2/empty.rds"),
    );
    assert!(
        session.eval("f(list(1L,2L), 8L)").is_err(),
        "default x[[]] <- v is a missing subscript"
    );
    assert_eq!(
        session
            .eval("d<-structure(list(1L,2L), class='foo'); `[[<-.foo`<-function(x,i,value) 'assigned'; identical(f(d, 9L), 'assigned')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(list(1L,2L), class='foo'); `[[<-.foo`<-function(x,i,value) 'assigned'; identical(withVisible(f(d, 9L)), list(value='assigned', visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_dfltsubassign2_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dfltsubassign2/empty.rds");
    // GETVAR v; STARTASSIGN x; STARTSUBASSIGN2 call=4 label=10; DOMISSING;
    // DFLTSUBASSIGN2; ENDASSIGN; POP; GETVAR x; RETURN.
    // Flip DOMISSING=30 to LDTRUE=18 so retained x[[]] <- v becomes x[[TRUE]] <- v.
    let words = [12, 20, 1, 61, 2, 71, 4, 10, 30, 72, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 8 * 4..offset + 9 * 4].copy_from_slice(&18_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("identical(f(list(1L,2L), 8L), list(8L,2L))")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated DOMISSING must assign through TRUE, not retained missing"
    );
}

#[test]
fn malformed_startsubassign2_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-dfltsubassign2/empty.rds");
    let words = [12, 20, 1, 61, 2, 71, 4, 10, 30, 72, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    // Keep the 16-int code vector length: version, STARTSUBASSIGN2, RETURN, padding.
    let replacement = [12_i32, 71, 0, 4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(list(1L,2L), 9L)").is_err(),
        "empty-stack STARTSUBASSIGN2 must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
