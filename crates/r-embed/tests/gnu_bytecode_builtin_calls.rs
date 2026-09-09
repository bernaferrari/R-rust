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

#[test]
fn imported_gnu_builtin_call_executes_and_roundtrips() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-builtin-calls/abs.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session.eval("identical(f(-4L),4L)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(-9L),9L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
#[test]
fn mutated_gnu_builtin_call_executes_over_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-bytecode-builtin-calls/abs.rds").to_vec();
    let stream: [i32; 12] = [12, 123, 0, 11, 26, 1, 20, 2, 33, 39, 0, 1];
    let encoded = stream
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect::<Vec<_>>();
    let offset = bytes
        .windows(encoded.len())
        .position(|candidate| candidate == encoded.as_slice())
        .expect("GNU builtin call stream");
    // Redirect GETBUILTIN from abs (pool index 1) to x (pool index 2). The
    // retained source still says abs(x), while the imported stream must reject
    // the non-builtin lookup path.
    bytes[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&2_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert!(
        session.eval("f(-4L)").is_err(),
        "mutated GETBUILTIN must execute instead of source fallback"
    );
}

#[test]
fn nested_builtin_frames_preserve_outer_operands_under_gc() {
    let mut s = RSession::new().unwrap();
    load(
        &mut s,
        include_bytes!("fixtures/gnu-bytecode-builtin-calls/nested.rds"),
    );
    assert_eq!(
        s.eval("local({gctorture(TRUE);on.exit(gctorture(FALSE));identical(f(-4L),4L)})")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert!(s.eval("f('not numeric')").is_err());
    assert_eq!(s.eval("f(-2L)").unwrap().trim(), "[1] 2");
    load(
        &mut s,
        include_bytes!("fixtures/gnu-bytecode-builtin-calls/residual.rds"),
    );
    assert_eq!(
        s.eval("identical(f(-4L,-9L),13L)").unwrap().trim(),
        "[1] TRUE"
    );
}
