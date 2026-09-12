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
fn imported_gnu_subset2_n_extracts_array_element() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-subset2-n/subset2-n-const.rds"),
    );
    assert_eq!(
        session
            .eval("a<-array(1:24,c(2,3,4)); identical(f(a), 15L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("a<-array(1:24,c(2,3,4)); identical(withVisible(f(a)), list(value=15L, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(array(1:8,c(2,2,2)), class='foo'); `[[.foo`<-function(x,i,j,k) paste0(i,j,k); identical(f(d), '123')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-subset2-n/subset2-n.rds"),
    );
    assert_eq!(
        session
            .eval("a<-array(1:24,c(2,3,4)); identical(f(a, 2L, 3L, 4L), 24L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("a<-array(1:24,c(2,3,4),dimnames=list(c('r1','r2'), c('c1','c2','c3'), c('d1','d2','d3','d4'))); identical(f(a, 'r1', 'c3', 'd2'), 11L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_subset2_n_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-subset2-n/subset2-n-const.rds");
    // GETVAR x; STARTSUBSET2_N; LDCONST 1L; LDCONST 2L; LDCONST 3L;
    // SUBSET2_N call=0 rank=3; RETURN.
    // Flip the last LDCONST pool index 4 -> 3 so retained [[1,2,3]] becomes [[1,2,2]].
    let words = [12, 20, 1, 110, 0, 15, 16, 2, 16, 3, 16, 4, 113, 0, 3, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 11 * 4..offset + 12 * 4].copy_from_slice(&3_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("a<-array(1:24,c(2,3,4)); identical(f(a), 9L)")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated LDCONST must index [[1,2,2]], not retained [[1,2,3]]"
    );
}

#[test]
fn malformed_subset2_n_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-subset2-n/subset2-n-const.rds");
    let words = [12, 20, 1, 110, 0, 15, 16, 2, 16, 3, 16, 4, 113, 0, 3, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 113, 0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(array(1:24,c(2,3,4)))").is_err(),
        "empty-stack SUBSET2_N must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
