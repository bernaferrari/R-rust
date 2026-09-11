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
        .eval(&format!("f<-unserialize({})", raw_expression(bytes)))
        .unwrap();
}

#[test]
fn imported_gnu_switch_dispatches_named_and_default_branches() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-switch/switch.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session
            .eval("identical(c(f('a'),f('b'),f('z')),c(1L,2L,0L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_switch_instruction_runs_over_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-bytecode-switch/switch.rds").to_vec();
    let stream: [i32; 20] = [
        12, 20, 1, 102, 0, 2, 6, 7, 17, 15, 1, 16, 3, 1, 16, 4, 1, 16, 5, 1,
    ];
    let encoded = stream
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect::<Vec<_>>();
    let offset = bytes
        .windows(encoded.len())
        .position(|candidate| candidate == encoded.as_slice())
        .expect("GNU SWITCH stream");
    // Change only the first branch's LDCONST operand. The retained source says
    // the "a" branch is 1L; executing the imported stream now returns 2L.
    bytes[offset + 12 * 4..offset + 13 * 4].copy_from_slice(&4_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert_eq!(
        session.eval("identical(f('a'),2L)").unwrap().trim(),
        "[1] TRUE"
    );
}
