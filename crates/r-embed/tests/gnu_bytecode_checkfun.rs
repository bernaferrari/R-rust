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
fn imported_gnu_checkfun_callable_argument_executes_and_roundtrips() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-checkfun/callable-argument.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session
            .eval("h<-function(z) z+1L;identical(f(h,4L),5L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval(
                "local({gctorture(TRUE);on.exit(gctorture(FALSE));g<-unserialize(serialize(f,NULL));identical(g(abs,-9L),9L)})",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("tryCatch(f(1,4L),error=function(e) TRUE)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_checkfun_stream_does_not_fall_back_to_source() {
    let bytes = include_bytes!("fixtures/gnu-bytecode-checkfun/callable-argument.rds");
    let stream: [i32; 9] = [12, 20, 1, 28, 29, 2, 38, 0, 1];
    let encoded = stream
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect::<Vec<_>>();
    let offset = bytes
        .windows(encoded.len())
        .position(|candidate| candidate == encoded.as_slice())
        .expect("GNU CHECKFUN stream");
    let mut mutated = bytes.to_vec();
    // POP the callable in place of CHECKFUN. The retained source still calls
    // f(x), but the imported stream must reject the now-unframed MAKEPROM.
    mutated[offset + 3 * 4..offset + 4 * 4].copy_from_slice(&4_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    assert!(
        session
            .eval(&format!("f <- unserialize({})", raw_expression(&mutated)))
            .is_err(),
        "mutated CHECKFUN stream must not use retained source"
    );
}
