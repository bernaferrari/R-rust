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
fn imported_gnu_names_setter_call_assigns_and_is_visible() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-setter-call/names.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(1:3, c('a','b','c')), {x<-1:3; names(x)<-c('a','b','c'); x})")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(1:2, c('p','q'))), list(value={x<-1:2; names(x)<-c('p','q'); x}, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_attr_setter_call_assigns_and_is_visible() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-setter-call/attr.rds"),
    );
    assert_eq!(
        session
            .eval("identical(attr(f(1:2, 9L), 'a'), 9L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(1:2, 9L)), list(value={x<-1:2; attr(x,'a')<-9L; x}, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn gnu_setter_call_assigns_names_on_returned_value() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-setter-call/names.rds"),
    );
    assert_eq!(
        session
            .eval("identical(names(f(1:3, c('a','b','c'))), c('a','b','c'))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn gnu_setter_call_closure_replacement_uses_active_frame() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-setter-call/names.rds"),
    );
    assert_eq!(
        session
            .eval("e<-new.env(parent=baseenv()); e$`names<-`<-function(x, value) { attr(x,'marked')<-value; x }; environment(f)<-e; identical(attr(f(1:2, 'ok'), 'marked'), 'ok')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_names_setter_executes_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-setter-call/names.rds");
    // GETVAR v; STARTASSIGN x; GETFUN names<-; PUSHNULLARG; SETTER_CALL;
    // ENDASSIGN; POP; GETVAR x; RETURN.
    // Redirect GETFUN from names<- to x. Retained source still does names(x) <- v.
    let words = [12, 20, 1, 61, 2, 23, 4, 35, 98, 6, 1, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    changed[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&2_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert!(
        session.eval("f(1:3, c('a','b','c'))").is_err(),
        "mutated GETFUN must execute instead of falling back to retained names<-"
    );
}

#[test]
fn mutated_gnu_attr_setter_executes_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-setter-call/attr.rds");
    // GETVAR v; STARTASSIGN x; GETFUN attr<-; PUSHNULLARG; PUSHCONSTARG;
    // SETTER_CALL; ENDASSIGN; POP; GETVAR x; RETURN.
    let words = [12, 20, 1, 61, 2, 23, 4, 35, 34, 6, 98, 7, 1, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    // Redirect GETFUN from attr<- to x. Retained source still does attr(x, "a") <- v.
    changed[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&2_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert!(
        session.eval("f(1:2, 9L)").is_err(),
        "mutated GETFUN must execute instead of falling back to retained attr<-"
    );
}

#[test]
fn malformed_setter_call_missing_frame_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-setter-call/names.rds");
    let words = [12, 20, 1, 61, 2, 23, 4, 35, 98, 6, 1, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 98, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(1:3, c('a','b','c'))").is_err(),
        "missing-frame SETTER_CALL must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn malformed_setter_call_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-setter-call/names.rds");
    let words = [12, 20, 1, 61, 2, 23, 4, 35, 98, 6, 1, 62, 2, 4, 20, 2, 1];
    let offset = unique_stream_offset(original, &words);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 23, 0, 98, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
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
        session.eval("f(1:3, c('a','b','c'))").is_err(),
        "empty-stack SETTER_CALL must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
