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
fn imported_gnu_and_or_not_preserve_vectors_na_raw_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-vector-logic/and.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(TRUE,FALSE,NA),c(TRUE,TRUE,TRUE)),c(TRUE,FALSE,NA))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(c(TRUE,FALSE,NA),TRUE),c(TRUE,FALSE,NA))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(as.raw(c(1,2,3)),as.raw(c(1,0,7))),as.raw(c(1,0,3)))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(1L,0L)),list(value=FALSE,visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(logical(0),TRUE),logical(0))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("Ops.foo<-function(e1,e2){gc();42L};identical(f(structure(TRUE,class='foo'),FALSE),42L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert!(
        session.eval("f('x',TRUE)").is_err(),
        "character operands must fail for vector AND"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-vector-logic/or.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(FALSE,TRUE,NA),c(FALSE,FALSE,FALSE)),c(FALSE,TRUE,NA))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(as.raw(c(1,2,0)),as.raw(c(4,1,8))),as.raw(c(5,3,8)))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-vector-logic/not.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(TRUE,FALSE,NA)),c(FALSE,TRUE,NA))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(as.raw(c(0,1,255))),as.raw(c(255,254,0)))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(0L)),list(value=TRUE,visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_and_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-vector-logic/and.rds");
    let words = [12, 20, 1, 20, 2, 57, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    // AND -> OR. Retained source is still x & y.
    changed[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&58_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("identical(f(c(TRUE,FALSE),c(FALSE,FALSE)),c(TRUE,FALSE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(TRUE,FALSE),TRUE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn malformed_logic_call_index_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-vector-logic/and.rds");
    let words = [12, 20, 1, 20, 2, 57, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut session = RSession::new().unwrap();
    for index in [3_i32, i32::MAX, -1] {
        let mut malformed = original.to_vec();
        malformed[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&index.to_be_bytes());
        let loaded = session.eval(&format!("f <- unserialize({})", raw_expression(&malformed)));
        if loaded.is_err() {
            assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
            continue;
        }
        assert!(
            session.eval("f(TRUE, FALSE)").is_err(),
            "malformed AND call index {index} must not run retained source"
        );
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
    }
}
