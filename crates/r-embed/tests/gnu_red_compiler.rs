// Beads: rport-uewq; broader bytecode execution: rport-bq7s.4.
use r_embed::RSession;

fn load(session: &mut RSession, bytes: &[u8]) {
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    session
        .eval(&format!("f <- unserialize(as.raw(c({raw})))"))
        .unwrap();
}

fn encoded(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn unique_offset(bytes: &[u8], values: &[i32]) -> usize {
    let needle = encoded(values);
    let offsets = bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(i, window)| (window == needle).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1, "bytecode stream must occur exactly once");
    offsets[0]
}

#[test]
fn compiled_sqrt_opcode_executes_instead_of_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-red-compiler/sqrt.rds").to_vec();
    // GNU stream: version/BASEGUARD/GETVAR/SQRT. Replace SQRT with EXP while
    // retaining the original source expression (sqrt(x)).
    let offset = unique_offset(&bytes, &[12, 123, 0, 8, 20, 1, 49, 0, 1]);
    bytes[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&50_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert_eq!(
        session.eval("round(f(4), 6)").unwrap().trim(),
        "[1] 54.59815"
    );
}

#[test]
fn compiled_builtin_call_executes_through_the_guarded_builtin_path() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-red-compiler/abs.rds"),
    );
    assert_eq!(
        session.eval("identical(f(-4), 4)").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn compiled_loop_comparison_opcode_executes_instead_of_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-red-compiler/loop.rds").to_vec();
    // GNU loop stream contains LT.OP (53). Replace it with GT.OP (56): for
    // c(-1, 0, 1), the mutated program sums -1 instead of the source's 1.
    let offset = unique_offset(
        &bytes,
        &[
            12, 16, 1, 22, 2, 4, 20, 4, 11, 6, 5, 36, 20, 5, 16, 1, 53, 7,
        ],
    );
    bytes[offset + 16 * 4..offset + 17 * 4].copy_from_slice(&56_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert_eq!(session.eval("f(c(-1L, 0L, 1L))").unwrap().trim(), "[1] -1");
}
