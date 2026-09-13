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
fn imported_gnu_matsubset_matches_matrix_and_object_edges() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-matsubset/matsubset-const.rds"),
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m), 3L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(withVisible(f(m)), list(value=3L, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(matrix(1:4,2,2), class='foo'); `[.foo`<-function(x,i,j) paste0(i,j); identical(f(d), '12')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-matsubset/matsubset.rds"),
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m, 2L, 3L), 6L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3,dimnames=list(c('r1','r2'), c('c1','c2','c3'))); identical(f(m, 'r1', 'c3'), 5L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_subset_n_matches_array_edges() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-matsubset/subset-n.rds"),
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
            .eval("d<-structure(array(1:8,c(2,2,2)), class='foo'); `[.foo`<-function(x,i,j,k) paste0(i,j,k); identical(f(d), '123')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_matsubset_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-matsubset/matsubset-const.rds");
    // GETVAR x; STARTSUBSET_N call=0 label=12; LDCONST 1L; LDCONST 2L; MATSUBSET; RETURN.
    // Flip the first LDCONST pool index 2 -> 3 so retained x[1L,2L] becomes x[2L,2L].
    let words = [12, 20, 1, 104, 0, 12, 16, 2, 16, 3, 85, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 7 * 4..offset + 8 * 4].copy_from_slice(&3_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m), 4L)")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated LDCONST must index [2,2], not retained [1,2]"
    );
}

#[test]
fn mutated_gnu_subset_n_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-matsubset/subset-n.rds");
    // GETVAR x; STARTSUBSET_N; LDCONST 1L; LDCONST 2L; LDCONST 3L; SUBSET_N rank=3; RETURN.
    // Flip the last LDCONST pool index 4 -> 3 so retained x[1L,2L,3L] becomes x[1L,2L,2L].
    let words = [12, 20, 1, 104, 0, 15, 16, 2, 16, 3, 16, 4, 112, 0, 3, 1];
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
        "mutated LDCONST must index [1,2,2], not retained [1,2,3]"
    );
}

#[test]
fn malformed_matsubset_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-matsubset/matsubset-const.rds");
    let words = [12, 20, 1, 104, 0, 12, 16, 2, 16, 3, 85, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    // Keep the 13-int code vector length: version, MATSUBSET, RETURN, padding.
    let replacement = [12_i32, 85, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(matrix(1:6,2,3))").is_err(),
        "empty-stack MATSUBSET must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
