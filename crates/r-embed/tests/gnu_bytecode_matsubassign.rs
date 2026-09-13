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
fn imported_gnu_matsubassign_matches_matrix_drop_and_object_edges() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-matsubassign/matsubassign-const.rds"),
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m, 9L), matrix(c(1L,2L,9L,4L,5L,6L),2,3))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(withVisible(f(m, 9L)), list(value=matrix(c(1L,2L,9L,4L,5L,6L),2,3), visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(matrix(1:4,2,2), class='foo'); `[<-.foo`<-function(x,i,j,value) paste0(i,j,value); identical(f(d, 9L), '129')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-matsubassign/matsubassign.rds"),
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m, 2L, 3L, 8L), matrix(c(1L,2L,3L,4L,5L,8L),2,3))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); got<-f(m, 1L, 1:3, 0L); identical(dim(got), c(2L,3L)) && identical(got[1,], c(0L,0L,0L)) && identical(got[2,], c(2L,4L,6L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3,dimnames=list(c('r1','r2'), c('c1','c2','c3'))); identical(f(m, 'r1', 'c3', 7L)['r1','c3'], 7L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_matsubassign_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-matsubassign/matsubassign-const.rds");
    // GETVAR v; STARTASSIGN x; STARTSUBASSIGN_N; LDCONST 1L; LDCONST 2L; MATSUBASSIGN.
    // Flip the first LDCONST pool index 6 -> 7 so retained x[1L,2L] becomes x[2L,2L].
    let words = [12, 20, 1, 61, 2, 105, 4, 14, 16, 6, 16, 7, 87, 4, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 9 * 4..offset + 10 * 4].copy_from_slice(&7_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("m<-matrix(1:6,2,3); identical(f(m, 9L), matrix(c(1L,2L,3L,9L,5L,6L),2,3))")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated LDCONST must assign [2,2], not retained [1,2]"
    );
}

#[test]
fn malformed_matsubassign_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-matsubassign/matsubassign-const.rds");
    let words = [12, 20, 1, 61, 2, 105, 4, 14, 16, 6, 16, 7, 87, 4, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    // Keep the 20-int code vector length: version, MATSUBASSIGN, RETURN, padding.
    let replacement = [
        12_i32, 87, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ];
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
        session.eval("f(matrix(1:6,2,3), 9L)").is_err(),
        "empty-stack MATSUBASSIGN must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
