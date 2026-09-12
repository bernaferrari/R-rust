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
fn imported_gnu_vecsubassign2_assigns_through_startassign() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-vecsubassign2/vecsubassign2.rds"),
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
            .eval("got<-f(list(a=1L,b=2L), 2L, 9L); identical(got[[2]], 9L) && identical(got[[1]], 1L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(list(a=1L), 'a', 7L)[['a']], 7L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(list(1L,2L), 1L, 3L)), list(value=list(3L,2L), visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(list(1L,2L), class='foo'); `[[<-.foo`<-function(x,i,value) paste0(i,value); identical(f(d, 2L, 9L), '29')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL)); identical(g(list(1L,2L), 2L, 4L)[[2]], 4L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_vecsubassign2_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-vecsubassign2/vecsubassign2.rds");
    // GETVAR v; STARTASSIGN x; STARTSUBASSIGN2_N; GETVAR_MISSOK i=6; VECSUBASSIGN2.
    // Flip the index symbol 6 -> 1 (v). Retained source still assigns x[[i]].
    let words = [12, 20, 1, 61, 2, 111, 4, 12, 92, 6, 108, 4, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    // const 1 is v, so the mutated stream does x[[v]] <- v.
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
fn malformed_vecsubassign2_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-vecsubassign2/vecsubassign2.rds");
    let words = [12, 20, 1, 61, 2, 111, 4, 12, 92, 6, 108, 4, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    // Keep the 18-int code vector length: version, VECSUBASSIGN2, RETURN, padding.
    let replacement = [
        12_i32, 108, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
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
        session.eval("f(list(1L,2L), 1L, 9L)").is_err(),
        "empty-stack VECSUBASSIGN2 must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
