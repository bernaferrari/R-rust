use r_embed::RSession;
fn load(session: &mut RSession, bytes: &[u8]) {
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    session
        .eval(&format!("f<-unserialize(as.raw(c({raw})))"))
        .unwrap();
}
#[test]
fn gnu_unary_and_power_preserve_types_vectors_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-unary/negative.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(1L,NA_integer_,-3L)),c(-1L,NA_integer_,3L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("withVisible(f(invisible(2)))$visible")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("Ops.foo<-function(e1,e2){gc();42L};identical(f(structure(1,class='foo')),42L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-unary/positive.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(1L,NA_integer_,-3L)),c(1L,NA_integer_,-3L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-unary/power.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(c(2L,3L,NA_integer_),2L),c(4,9,NA_real_))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(c(NA_real_,NaN,0,1),0),c(1,1,1,1))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
#[test]
fn unary_opcode_executes_instead_of_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-bytecode-unary/negative.rds").to_vec();
    let stream = [12_i32, 20, 1, 42, 0, 1]
        .iter()
        .flat_map(|x| x.to_be_bytes())
        .collect::<Vec<_>>();
    let offsets = bytes
        .windows(stream.len())
        .enumerate()
        .filter_map(|(i, x)| (x == stream).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1);
    let at = offsets[0] + 3 * 4;
    bytes[at..at + 4].copy_from_slice(&43_i32.to_be_bytes());
    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(c(2L,3L)),c(2L,3L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
