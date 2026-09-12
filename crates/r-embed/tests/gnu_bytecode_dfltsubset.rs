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
fn imported_gnu_empty_subset_copies_vector_and_dispatches_objects() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dfltsubset/empty.rds"),
    );
    assert_eq!(
        session.eval("identical(f(1:3), 1:3)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(list(a=1L,b=2L)), list(a=1L,b=2L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(1:2)), list(value=1:2, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(1:3, class='foo'); `[.foo`<-function(x,i) 'missing'; identical(f(d), 'missing')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_matrix_missing_subset_keeps_or_drops_dims() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dfltsubset/matrix-missing.rds"),
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m), m)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dfltsubset/row-missing.rds"),
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m, 1L), c(1L,3L,5L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_dfltsubset_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dfltsubset/empty.rds");
    // GETVAR x; STARTSUBSET call=0 label=8; DOMISSING; DFLTSUBSET; RETURN.
    // Flip DOMISSING=30 to LDFALSE=19 so retained x[] becomes x[FALSE].
    let words = [12, 20, 1, 63, 0, 8, 30, 64, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&19_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("identical(f(1:3), integer(0))")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated DOMISSING must index with FALSE, not retained missing"
    );
}

#[test]
fn malformed_startsubset_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-dfltsubset/empty.rds");
    let words = [12, 20, 1, 63, 0, 8, 30, 64, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    // Keep the 9-int code vector length: version, STARTSUBSET, RETURN, padding.
    let replacement = [12_i32, 63, 0, 4, 1, 1, 1, 1, 1];
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
        session.eval("f(1:3)").is_err(),
        "empty-stack STARTSUBSET must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
