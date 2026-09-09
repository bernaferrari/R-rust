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
fn stream_offset(bytes: &[u8], words: &[i32]) -> usize {
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
        "fixture must contain one GNU PUSHCONSTARG stream"
    );
    offsets[0]
}

fn load(session: &mut RSession, bytes: &[u8]) {
    session
        .eval(&format!("f <- unserialize({})", raw_expression(bytes)))
        .unwrap();
}

#[test]
fn imported_gnu_pushconstarg_preserves_literal_argument_and_roundtrips() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-constant-args/pushconstarg.rds");
    let mut session = RSession::new().unwrap();
    session
        .eval("target <- function(a,b) identical(a,1L)")
        .unwrap();
    load(&mut session, fixture);
    assert_eq!(session.eval("f('value')").unwrap().trim(), "[1] TRUE");
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));g('value')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_pushconstarg_executes_over_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-bytecode-constant-args/pushconstarg.rds").to_vec();
    let stream = [12, 23, 1, 34, 2, 29, 3, 38, 0, 1];
    let offset = stream_offset(&bytes, &stream);
    // Replace literal constant 1L with the retained pool's target symbol.
    // The source still says target(1L,x), so execution must return FALSE.
    bytes[offset + 4 * 4..offset + 5 * 4].copy_from_slice(&1_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    session
        .eval("target <- function(a,b) identical(a,1L)")
        .unwrap();
    load(&mut session, &bytes);
    assert_eq!(session.eval("f('value')").unwrap().trim(), "[1] FALSE");
}
