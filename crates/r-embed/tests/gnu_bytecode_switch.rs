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
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(f('a'),2L)&&identical(g('a'),2L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_switch_supports_numeric_selectors() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-switch/numeric.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session
            .eval("identical(c(f(1L),f(2L)),c(10L,20L))&&is.null(f(9L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_switch_preserves_missing_and_default_errors() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-switch/missing.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session.eval("identical(f('z'),0L)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("isTRUE(tryCatch(f(character(0)),error=function(e)TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_switch_preserves_string_fallthrough() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-switch/fallthrough.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session
            .eval("identical(c(f('a'),f('b'),f('z')),c(2L,2L,0L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_switch_results_are_visible() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-switch/visibility.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session
            .eval(
                "identical(c(withVisible(f('a'))$visible,withVisible(f('z'))$visible),c(TRUE,TRUE))"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_switch_rejects_invalid_targets_before_execution() {
    let original = include_bytes!("fixtures/gnu-bytecode-switch/switch.rds");
    let stream = [
        12_i32, 20, 1, 102, 0, 2, 6, 7, 17, 15, 1, 16, 3, 1, 16, 4, 1, 16, 5, 1,
    ]
    .iter()
    .flat_map(|x| x.to_be_bytes())
    .collect::<Vec<_>>();
    let at = original
        .windows(stream.len())
        .position(|x| x == stream)
        .unwrap();
    for (operand, replacement) in [(5, 3), (6, 3), (7, 3), (7, 999)] {
        let mut bytes = original.to_vec();
        bytes[at + operand * 4..at + (operand + 1) * 4]
            .copy_from_slice(&i32::to_be_bytes(replacement));
        let mut session = RSession::new().unwrap();
        assert!(
            session
                .eval(&format!("unserialize({})", raw_expression(&bytes)))
                .is_err()
        );
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
    }
    // A valid integer vector whose first jump enters an operand, not an opcode.
    let vector = [13_i32, 3, 11, 14, 17]
        .iter()
        .flat_map(|x| x.to_be_bytes())
        .collect::<Vec<_>>();
    let hits = original
        .windows(vector.len())
        .enumerate()
        .filter_map(|(i, x)| (x == vector).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(hits.len(), 1);
    let mut bytes = original.to_vec();
    bytes[hits[0] + 8..hits[0] + 12].copy_from_slice(&12_i32.to_be_bytes());
    let mut session = RSession::new().unwrap();
    assert!(
        session
            .eval(&format!("unserialize({})", raw_expression(&bytes)))
            .is_err()
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
