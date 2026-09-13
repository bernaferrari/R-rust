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
fn imported_gnu_assign2_dollar_writes_parent_binding() {
    let mut session = RSession::new().unwrap();
    session.eval("x <- list(a=0L, b=2L)").unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-assign2/dollar.rds"),
    );
    assert_eq!(
        session.eval("identical(f(), list(a=1L, b=2L))").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(x, list(a=1L, b=2L))").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_assign2_subset_writes_parent_binding() {
    let mut session = RSession::new().unwrap();
    session.eval("x <- c(1L, 2L)").unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-assign2/subset.rds"),
    );
    assert_eq!(
        session.eval("identical(f(1L), c(8L, 2L))").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(x, c(8L, 2L))").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_assign2_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-assign2/dollar.rds");
    // LDCONST 1L; STARTASSIGN2 x; DOLLARGETS; ENDASSIGN2 x; POP; GETVAR x; RETURN.
    // Flip ENDASSIGN2 symbol 2 (x) -> 5 (a) so retained x$a <<- 1L writes a.
    let words = [12, 16, 1, 96, 2, 74, 4, 5, 97, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 9 * 4..offset + 10 * 4].copy_from_slice(&5_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    session.eval("x <- list(a=0L); rm(list='a')").unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("{ invisible(f()); exists('a') && identical(a, list(a=1L)) }")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated ENDASSIGN2 must write a, not retained x"
    );
}

#[test]
fn malformed_startassign2_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-assign2/dollar.rds");
    let words = [12, 16, 1, 96, 2, 74, 4, 5, 97, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 96, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
    for (i, word) in replacement.iter().enumerate() {
        let at = offset + i * 4;
        malformed[at..at + 4].copy_from_slice(&word.to_be_bytes());
    }

    let mut session = RSession::new().unwrap();
    session.eval("x <- list(a=0L)").unwrap();
    let loaded = session.eval(&format!(
        "f <- unserialize({})",
        raw_expression(&malformed)
    ));
    if loaded.is_err() {
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
        return;
    }
    assert!(
        session.eval("f()").is_err(),
        "empty-stack STARTASSIGN2 must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
