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
fn imported_gnu_dollar_matches_list_environment_and_atomic_edges() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dollar/dollar.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(list(a=1L,b=2L)), 1L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(list(aa=7L)), 7L)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("identical(f(list(b=1L)), NULL)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("e<-new.env();e$a<-3L;identical(f(e),3L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert!(
        session.eval("f(1:3)").is_err(),
        "atomic $ must error instead of falling back to retained source"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(list(a=1L))), list(value=1L, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(list(a=1L), class='foo'); `$.foo`<-function(x,name) paste0('m',name); identical(f(d), 'ma')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_dollargets_assigns_through_startassign() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dollar/dollargets.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(list(b=2L), 9L), list(b=2L, a=9L)) || identical(f(list(b=2L), 9L)$a, 9L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("got<-f(list(a=1L), 8L);identical(got$a,8L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(list(), 4L)$a,4L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_dollar_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dollar/dollar.rds");
    // GETVAR x; DOLLAR call=0 symbol=2; RETURN. Flip symbol index 2 -> 1 so
    // the field becomes x, while retained source is still x$a.
    let words = [12, 20, 1, 73, 0, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&1_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session.eval("identical(f(list(a=1L)), NULL)").unwrap().trim(),
        "[1] TRUE",
        "mutated DOLLAR must read x$x, not retained x$a"
    );
    assert_eq!(
        session.eval("identical(f(list(x=5L)), 5L)").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn malformed_dollar_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-dollar/dollar.rds");
    let words = [12, 20, 1, 73, 0, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    // Keep the 7-int code vector length: version, DOLLAR, RETURN, then three
    // extra RETURNs. DOLLAR is well-framed and the CFG stack is empty.
    let replacement = [12_i32, 73, 0, 1, 1, 1, 1];
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
        session.eval("f(list(a=1L))").is_err(),
        "empty-stack DOLLAR must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
#[test]
fn mutated_gnu_dollargets_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dollar/dollargets.rds");
    // GETVAR v; STARTASSIGN x; DOLLARGETS call=4 symbol=5; ENDASSIGN x; ...
    let words = [12, 20, 1, 61, 2, 74, 4, 5, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    // Field a (const 5) -> x (const 2). Retained source still assigns x$a.
    changed[offset + 7 * 4..offset + 8 * 4].copy_from_slice(&2_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("got<-f(list(), 4L); is.null(got$a) && identical(got$x, 4L)")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated DOLLARGETS must assign x$x instead of retained x$a"
    );
}
