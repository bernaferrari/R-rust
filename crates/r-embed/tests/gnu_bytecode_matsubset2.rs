use r_embed::RSession;

fn raw_expression(bytes: &[u8]) -> String {
    format!(
        "as.raw(c({}))",
        bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
    )
}

fn load(session: &mut RSession, bytes: &[u8]) {
    session
        .eval(&format!("f <- unserialize({})", raw_expression(bytes)))
        .unwrap();
}

fn unique_stream_offset(bytes: &[u8], words: &[i32]) -> usize {
    let encoded = words.iter().flat_map(|word| word.to_be_bytes()).collect::<Vec<_>>();
    let offsets = bytes
        .windows(encoded.len())
        .enumerate()
        .filter_map(|(offset, candidate)| (candidate == encoded).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1, "fixture must contain one exact instruction stream");
    offsets[0]
}

#[test]
fn imported_gnu_matsubset2_extracts_matrix_element() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-matsubset2/matsubset2.rds"),
    );
    assert_eq!(
        session.eval("m<-matrix(1:6,2,3); identical(f(m), 1L)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(withVisible(f(m)), list(value=1L, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("d<-structure(matrix(1:4,2,2), class='foo'); `[[.foo`<-function(x,i,j) paste0(i,j); identical(f(d), '11')").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_matsubset2_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-matsubset2/matsubset2.rds");
    // GETVAR x; STARTSUBSET2_N; LDCONST 1L; LDCONST 1L; MATSUBSET2; RETURN.
    // Flip the first LDCONST pool index 2 -> 1 so retained [[1L,1L]] becomes [[x,1L]].
    let words = [12, 20, 1, 110, 0, 12, 16, 2, 16, 2, 107, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 7 * 4..offset + 8 * 4].copy_from_slice(&1_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert!(
        session.eval("f(matrix(1:6,2,3))").is_err(),
        "mutated LDCONST must execute instead of retained [[1,1]]"
    );
}

#[test]
fn malformed_matsubset2_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-matsubset2/matsubset2.rds");
    let words = [12, 20, 1, 110, 0, 12, 16, 2, 16, 2, 107, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 107, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(matrix(1:6,2,3))").is_err(),
        "empty-stack MATSUBSET2 must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
