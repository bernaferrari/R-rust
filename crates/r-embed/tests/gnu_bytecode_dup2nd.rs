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

// GETVAR v; STARTASSIGN x; DUP2ND; DOLLAR; SWAP; STARTSUBASSIGN_N;
// LDCONST; VECSUBASSIGN; DOLLARGETS; ENDASSIGN; POP; GETVAR x; RETURN.
const DUP2ND_WORDS: [i32; 26] = [
    12, 20, 1, 61, 2, 101, 73, 4, 5, 100, 105, 7, 17, 16, 9, 86, 7, 74, 10, 5, 62, 2, 4, 20, 2, 1,
];

#[test]
fn imported_gnu_dup2nd_assigns_through_dollar_subset() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-dup2nd/dup2nd.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(list(a=c(1L,2L)), 9L)$a, c(9L,2L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("got<-f(list(a=c(1L,2L), b=3L), 8L); identical(got$a, c(8L,2L)) && identical(got$b, 3L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(list(a=c(1L,2L)), 4L))$visible, TRUE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("d<-structure(list(a=c(1L,2L)), class='foo'); `$.foo`<-function(x,name) x[[name]]; `$<-.foo`<-function(x,name,value) { x[[name]]<-value; x }; identical(f(d, 9L)$a, c(9L,2L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL)); identical(g(list(a=c(1L,2L)), 5L)$a, c(5L,2L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_dup2nd_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-dup2nd/dup2nd.rds");
    // Flip DOLLARGETS field symbol 5 (a) -> 2 (x). Retained source still assigns x$a[1].
    let offset = unique_stream_offset(original, &DUP2ND_WORDS);
    let mut changed = original.to_vec();
    changed[offset + 19 * 4..offset + 20 * 4].copy_from_slice(&2_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("got<-f(list(a=c(1L,2L)), 9L); identical(got$a, c(1L,2L)) && identical(got$x, c(9L,2L))")
            .unwrap()
            .trim(),
        "[1] TRUE",
        "mutated DOLLARGETS must write x$x, not retained x$a"
    );
}

#[test]
fn malformed_dup2nd_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-dup2nd/dup2nd.rds");
    let offset = unique_stream_offset(original, &DUP2ND_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [
        12_i32, 101, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
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
        session.eval("f(list(a=c(1L,2L)), 9L)").is_err(),
        "empty-stack DUP2ND must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
