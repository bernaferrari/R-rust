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

fn assert_fixture(
    fixture: &[u8],
    expected: &str,
    opcode: i32,
    mutated_opcode: i32,
    mutated_expected: &str,
) {
    let mut session = RSession::new().unwrap();
    session
        .eval(&format!("target <- function(a) identical(a,{expected})"))
        .unwrap();
    load(&mut session, fixture);
    assert_eq!(session.eval("f()").unwrap().trim(), "[1] TRUE");

    let stream = [12, 23, 1, opcode, 38, 0, 1];
    let encoded = stream
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect::<Vec<_>>();
    let offset = fixture
        .windows(encoded.len())
        .position(|candidate| candidate == encoded.as_slice())
        .expect("special argument stream");
    let mut mutated = fixture.to_vec();
    mutated[offset + 3 * 4..offset + 4 * 4].copy_from_slice(&mutated_opcode.to_be_bytes());
    load(&mut session, &mutated);
    assert_eq!(session.eval("f()").unwrap().trim(), mutated_expected);
}

#[test]
fn imported_gnu_special_constant_arguments_execute_over_retained_source() {
    assert_fixture(
        include_bytes!("fixtures/gnu-bytecode-special-constant-args/pushnullarg.rds"),
        "NULL",
        35,
        36,
        "[1] FALSE",
    );
    assert_fixture(
        include_bytes!("fixtures/gnu-bytecode-special-constant-args/pushtruearg.rds"),
        "TRUE",
        36,
        37,
        "[1] FALSE",
    );
    assert_fixture(
        include_bytes!("fixtures/gnu-bytecode-special-constant-args/pushfalsearg.rds"),
        "FALSE",
        37,
        36,
        "[1] FALSE",
    );
}
