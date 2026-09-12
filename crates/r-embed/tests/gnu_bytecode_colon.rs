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

// LDCONST 1; GETVAR n; COLON call=0; RETURN.
const COLON_WORDS: [i32; 8] = [12, 16, 1, 20, 2, 120, 0, 1];

#[test]
fn imported_gnu_colon_preserves_values_types_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-colon/colon.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(5L), 1:5) && identical(typeof(f(5L)), 'integer')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(5), 1:5) && identical(typeof(f(5)), 'integer')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(typeof(f(5.5)), 'integer') && identical(f(5.5), 1:5)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(3L)), list(value=1:3, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    let na_err = session.eval("f('a')");
    assert!(na_err.is_err(), "1:\"a\" must error");
    assert!(
        format!("{na_err:?}").contains("NA/NaN"),
        "GNU 1:\"a\" errors with NA/NaN argument, got {na_err:?}"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-colon/colon-const.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(), 1:5) && identical(typeof(f()), 'integer')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f()), list(value=1:5, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-colon/colon-range.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(2L, 6L), 2:6) && identical(typeof(f(2L, 6L)), 'integer')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(typeof(f(0.5, 3.5)), 'double') && isTRUE(all.equal(f(0.5, 3.5), c(0.5, 1.5, 2.5, 3.5)))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("fa<-factor(c('a','b')); fb<-factor(c('x','y')); got<-f(fa, fb); identical(as.character(got), c('a:x','b:y')) && identical(levels(got), c('a:x','a:y','b:x','b:y'))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL)); identical(g(3L, 1L), 3:1)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_colon_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-colon/colon.rds");
    // Flip COLON=120 -> ADD=44. Bytecode becomes 1+n; retained source is still 1:n.
    let offset = unique_stream_offset(original, &COLON_WORDS);
    let mut changed = original.to_vec();
    changed[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&44_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session.eval("identical(f(5L), 1 + 5L)").unwrap().trim(),
        "[1] TRUE",
        "mutated COLON must compute 1+n, not retained 1:n"
    );
}

#[test]
fn malformed_colon_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-colon/colon.rds");
    let offset = unique_stream_offset(original, &COLON_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 120, 0, 1, 1, 1, 1, 1];
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
        session.eval("f(5L)").is_err(),
        "empty-stack COLON must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
