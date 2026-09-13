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
fn imported_gnu_empty_subassign_replaces_vector_list_and_dispatches_objects() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dfltsubassign/empty.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(1L,2L,3L), 9L), c(9L,9L,9L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(list(a=1L,b=2L), 8L), list(a=8L,b=8L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(1:2, 3L)), list(value=c(3L,3L), visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(1:3, class='foo'); `[<-.foo`<-function(x,i,value) 'assigned'; identical(f(d, 9L), 'assigned')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_matrix_missing_subassign_fills_or_replaces_rows() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dfltsubassign/matrix-missing.rds"),
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m, 0L), matrix(0L,2,3))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dfltsubassign/row-missing.rds"),
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m, 1L, 8L), rbind(c(8L,8L,8L), c(2L,4L,6L)))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_dfltsubassign_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dfltsubassign/empty.rds");
    // GETVAR v; STARTASSIGN x; STARTSUBASSIGN call=4 label=10; DOMISSING;
    // DFLTSUBASSIGN; ENDASSIGN; POP; GETVAR x; RETURN.
    // Flip DOMISSING=30 to LDFALSE=19 so retained x[] <- v becomes x[FALSE] <- v.
    let words = [12, 20, 1, 61, 2, 65, 4, 10, 30, 66, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 8 * 4..offset + 9 * 4].copy_from_slice(&19_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("identical(f(c(1L,2L,3L), 9L), c(1L,2L,3L))")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated DOMISSING must assign through FALSE, not retained missing"
    );
}

#[test]
fn malformed_startsubassign_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-dfltsubassign/empty.rds");
    let words = [12, 20, 1, 61, 2, 65, 4, 10, 30, 66, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    // Keep the 16-int code vector length: version, STARTSUBASSIGN, RETURN, padding.
    let replacement = [12_i32, 65, 0, 4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(c(1L,2L), 9L)").is_err(),
        "empty-stack STARTSUBASSIGN must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
