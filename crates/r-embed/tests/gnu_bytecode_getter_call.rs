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

// GETVAR v; STARTASSIGN x; GETFUN names; PUSHNULLARG; GETTER_CALL;
// SWAP; STARTSUBASSIGN_N; LDCONST; VECSUBASSIGN; GETFUN names<-;
// PUSHNULLARG; SETTER_CALL; ENDASSIGN; POP; GETVAR x; RETURN.
const NAMES_WORDS: [i32; 30] = [
    12, 20, 1, 61, 2, 23, 4, 35, 99, 6, 100, 105, 7, 18, 16, 9, 86, 7, 23, 10, 35, 98, 12, 13, 62,
    2, 4, 20, 2, 1,
];

// GETVAR v; STARTASSIGN x; GETFUN attr; PUSHNULLARG; PUSHCONSTARG;
// GETTER_CALL; SWAP; STARTSUBASSIGN_N; LDCONST; VECSUBASSIGN;
// GETFUN attr<-; PUSHNULLARG; PUSHCONSTARG; SETTER_CALL; ENDASSIGN;
// POP; GETVAR x; RETURN.
const ATTR_WORDS: [i32; 34] = [
    12, 20, 1, 61, 2, 23, 4, 35, 34, 6, 99, 7, 100, 105, 8, 20, 16, 10, 86, 8, 23, 11, 35, 34, 6,
    98, 13, 14, 62, 2, 4, 20, 2, 1,
];

#[test]
fn imported_gnu_names_getter_call_assigns_and_is_visible() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-getter-call/names.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(a=1,b=2,c=3), 'z'), {x<-c(a=1,b=2,c=3); names(x)[1]<-'z'; x})")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(c(a=1,b=2), 'p')), list(value={x<-c(a=1,b=2); names(x)[1]<-'p'; x}, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_attr_getter_call_assigns_and_is_visible() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-getter-call/attr.rds"),
    );
    assert_eq!(
        session
            .eval("identical(attr(f(structure(1:3, a=c('p','q','r')), 'z'), 'a'), c('z','q','r'))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(structure(1:2, a=c('p','q')), 'z')), list(value={x<-structure(1:2, a=c('p','q')); attr(x,'a')[1]<-'z'; x}, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn gnu_getter_call_does_not_mutate_shared_names() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-getter-call/names.rds"),
    );
    assert_eq!(
        session
            .eval("n<-c('a','b','c'); x<-1:3; names(x)<-n; r<-f(x,'z'); identical(n, c('a','b','c')) && identical(names(r), c('z','b','c'))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("x<-c(a=1,b=2,c=3); y<-x; r<-f(x,'z'); identical(names(y), c('a','b','c')) && identical(names(r), c('z','b','c'))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn gnu_getter_call_closure_getter_uses_active_frame() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-getter-call/names.rds"),
    );
    assert_eq!(
        session
            .eval("e<-new.env(parent=baseenv()); e$names<-function(x) c('u','v'); e[['names<-']]<-function(x, value) { attr(x,'marked')<-value; x }; environment(f)<-e; identical(attr(f(1:2, 'ok'), 'marked'), c('ok','v'))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_names_getter_executes_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-getter-call/names.rds");
    // Redirect GETFUN from names to x. Retained source still does names(x)[1] <- v.
    let offset = unique_stream_offset(original, &NAMES_WORDS);
    let mut changed = original.to_vec();
    changed[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&2_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert!(
        session.eval("f(c(a=1,b=2,c=3), 'z')").is_err(),
        "mutated GETFUN must execute instead of falling back to retained names()"
    );
}

#[test]
fn mutated_gnu_attr_getter_executes_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-getter-call/attr.rds");
    let offset = unique_stream_offset(original, &ATTR_WORDS);
    let mut changed = original.to_vec();
    // Redirect GETFUN from attr to x. Retained source still does attr(x, "a")[1] <- v.
    changed[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&2_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert!(
        session
            .eval("f(structure(1:3, a=c('p','q','r')), 'z')")
            .is_err(),
        "mutated GETFUN must execute instead of falling back to retained attr()"
    );
}

#[test]
fn malformed_getter_call_missing_frame_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-getter-call/names.rds");
    let offset = unique_stream_offset(original, &NAMES_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 99, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(c(a=1,b=2,c=3), 'z')").is_err(),
        "missing-frame GETTER_CALL must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn malformed_swap_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-getter-call/names.rds");
    let offset = unique_stream_offset(original, &NAMES_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 100, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(c(a=1,b=2,c=3), 'z')").is_err(),
        "empty-stack SWAP must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
