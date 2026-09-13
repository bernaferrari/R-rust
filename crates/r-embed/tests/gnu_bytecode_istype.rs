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
fn imported_gnu_istype_opcodes_match_type_object_and_factor_edges() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-istype/null.rds"),
    );
    assert_eq!(
        session.eval("identical(f(NULL), TRUE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(1L), FALSE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(invisible(NULL))), list(value=TRUE, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-istype/logical.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(TRUE, NA, FALSE)), TRUE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(1L), FALSE)").unwrap().trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-istype/integer.rds"),
    );
    assert_eq!(
        session.eval("identical(f(1:3), TRUE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(factor(c('a','b'))), FALSE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(1), FALSE)").unwrap().trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-istype/double.rds"),
    );
    assert_eq!(
        session.eval("identical(f(1), TRUE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(1L), FALSE)").unwrap().trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-istype/complex.rds"),
    );
    assert_eq!(
        session.eval("identical(f(1i), TRUE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(1), FALSE)").unwrap().trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-istype/character.rds"),
    );
    assert_eq!(
        session.eval("identical(f('x'), TRUE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(quote(x)), FALSE)").unwrap().trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-istype/symbol.rds"),
    );
    assert_eq!(
        session.eval("identical(f(quote(x)), TRUE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f('x'), FALSE)").unwrap().trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-istype/object.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(structure(1L, class='foo')), TRUE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(1L), FALSE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(factor(1)), TRUE)").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_isnull_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-istype/null.rds");
    let words = [12, 20, 1, 75, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    // ISNULL -> ISLOGICAL. Retained source is still is.null(x).
    changed[offset + 3 * 4..offset + 4 * 4].copy_from_slice(&76_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session.eval("identical(f(TRUE), TRUE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(NULL), FALSE)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(FALSE),TRUE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn malformed_istype_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-istype/null.rds");
    let words = [12, 20, 1, 75, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    // Keep the 5-int code vector length: version, ISNULL, RETURN, then two
    // extra RETURNs. ISNULL is well-framed and the CFG stack is empty.
    let replacement = [12_i32, 75, 1, 1, 1];
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
        session.eval("f(NULL)").is_err(),
        "empty-stack ISNULL must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
