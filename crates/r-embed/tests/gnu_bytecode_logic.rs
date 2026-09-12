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
fn imported_gnu_and_or_preserve_na_short_circuit_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-logic/and.rds"),
    );
    assert_eq!(
        session
            .eval("identical(c(f(TRUE,TRUE),f(TRUE,FALSE),f(FALSE,TRUE),f(NA,FALSE),f(TRUE,NA),f(NA,TRUE)),c(TRUE,FALSE,FALSE,FALSE,NA,NA))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("side<-0L;identical(f(FALSE,{side<<-1L;TRUE}),FALSE)&&identical(side,0L)")
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
            .eval("identical(f(TRUE,logical(0)),NA)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert!(
        session.eval("f(TRUE,c(TRUE,FALSE))").is_err(),
        "length > 1 second operand must fail after first is TRUE"
    );
    assert!(
        session.eval("f('x',TRUE)").is_err(),
        "non-numeric first operand must fail"
    );
    assert!(
        session.eval("f(TRUE,'y')").is_err(),
        "non-numeric second operand must fail when evaluated"
    );
    assert_eq!(
        session.eval("identical(f(FALSE,'y'),FALSE)").unwrap().trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-logic/or.rds"),
    );
    assert_eq!(
        session
            .eval("identical(c(f(FALSE,FALSE),f(FALSE,TRUE),f(TRUE,FALSE),f(TRUE,NA),f(FALSE,NA),f(NA,FALSE)),c(FALSE,TRUE,TRUE,TRUE,NA,NA))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("side<-0L;identical(f(TRUE,{side<<-1L;FALSE}),TRUE)&&identical(side,0L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_and_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-logic/and.rds");
    let words = [12, 20, 1, 88, 0, 10, 20, 2, 89, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    // AND1ST/AND2ND -> OR1ST/OR2ND. Retained source is still x && y.
    changed[offset + 3 * 4..offset + 4 * 4].copy_from_slice(&90_i32.to_be_bytes());
    changed[offset + 8 * 4..offset + 9 * 4].copy_from_slice(&91_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(
        session
            .eval("identical(c(f(TRUE,FALSE),f(FALSE,TRUE),f(FALSE,FALSE)),c(TRUE,TRUE,FALSE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(FALSE,TRUE),TRUE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn malformed_and_targets_fail_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-logic/and.rds");
    let words = [12, 20, 1, 88, 0, 10, 20, 2, 89, 0, 1];
    let offset = unique_stream_offset(original, &words);
    let mut session = RSession::new().unwrap();
    for target in [9_i32, i32::MAX, -1] {
        let mut malformed = original.to_vec();
        malformed[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&target.to_be_bytes());
        assert!(
            session
                .eval(&format!("unserialize({})", raw_expression(&malformed)))
                .is_err(),
            "malformed AND1ST target {target} must not run retained source"
        );
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
    }
}
