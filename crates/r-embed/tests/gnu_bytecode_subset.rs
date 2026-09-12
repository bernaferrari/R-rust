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
fn imported_gnu_subset_n_matches_vector_list_and_object_edges() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-subset/subset.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(1:3, 2L), 2L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(list(a=7L,b=8L), 2L), list(b=8L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(c(a=1L,b=2L), 'a'), c(a=1L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(1:2, 1L)), list(value=1L, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(1:3, class='foo'); `[.foo`<-function(x,i) paste0('m',i); identical(f(d, 2L), 'm2')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-subset/subset-const.rds"),
    );
    assert_eq!(
        session.eval("identical(f(c(7L,8L)), 7L)").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_subset2_n_extracts_list_elements() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-subset/subset2.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(list(a=9L,b=8L), 1L), 9L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(list(a=9L), 'a'), 9L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_vecsubassign_assigns_through_startassign() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-subset/subassign.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(1L,2L), 1L, 8L), c(8L,2L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("got<-f(list(a=1L,b=2L), 2L, 9L); identical(got[[2]], 9L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(1:2,2L,4L),c(1L,4L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_subset_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-subset/subset.rds");
    // GETVAR x; STARTSUBSET_N call=0 label=10; GETVAR_MISSOK i; VECSUBSET; RETURN.
    // Flip VECSUBSET=84 to VECSUBSET2=106 so retained x[i] becomes x[[i]].
    let words = [12, 20, 1, 104, 0, 10, 92, 2, 84, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 8 * 4..offset + 9 * 4].copy_from_slice(&106_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("identical(f(list(a=7L,b=8L), 2L), 8L)")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated VECSUBSET must extract [[i]], not retained [i]"
    );
}

#[test]
fn mutated_gnu_subassign_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-subset/subassign.rds");
    // GETVAR v; STARTASSIGN x; STARTSUBASSIGN_N; GETVAR_MISSOK i=6; VECSUBASSIGN.
    // Flip the index symbol 6 -> 2 (x). Retained source still assigns x[i].
    let words = [12, 20, 1, 61, 2, 105, 4, 12, 92, 6, 86, 4, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    // const 1 is v, so the mutated stream does x[v] <- v.
    changed[offset + 9 * 4..offset + 10 * 4].copy_from_slice(&1_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("got<-f(c(1L,2L), 1L, 3L); identical(got, c(1L,2L,3L))")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated GETVAR_MISSOK must index with v, not retained i"
    );
}

#[test]
fn malformed_subset_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-subset/subset.rds");
    let words = [12, 20, 1, 104, 0, 10, 92, 2, 84, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    // Keep the 11-int code vector length: version, STARTSUBSET_N, RETURN, padding.
    let replacement = [12_i32, 104, 0, 4, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(1:3, 1L)").is_err(),
        "empty-stack STARTSUBSET_N must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
