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
    assert_eq!(offsets.len(), 1, "fixture must contain one GNU call stream");
    offsets[0]
}

fn load(session: &mut RSession, bytes: &[u8]) {
    session
        .eval(&format!("f <- unserialize({})", raw_expression(bytes)))
        .unwrap();
}

#[test]
fn imported_gnu_settag_matches_reversed_named_arguments() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-named-calls/reversed-tags.rds");
    let mut session = RSession::new().unwrap();
    session
        .eval("target <- function(a,b) paste(a,b,sep='/')")
        .unwrap();
    load(&mut session, fixture);
    // Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 evaluates this
    // imported stream as [1] "left/right". The source is target(b=y,a=x),
    // so positional argument construction would incorrectly produce right/left.
    assert_eq!(
        session.eval("f('left','right')").unwrap().trim(),
        "[1] \"left/right\""
    );
}

#[test]
fn imported_gnu_settag_executes_over_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-bytecode-named-calls/reversed-tags.rds").to_vec();
    let stream = [12, 23, 1, 29, 2, 31, 3, 29, 4, 31, 5, 38, 0, 1];
    let offset = stream_offset(&bytes, &stream);
    // Redirect the first SETTAG from b to a. The retained source still has
    // b=y,a=x; executing the mutated stream must now reject duplicate `a`.
    bytes[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&5_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    session
        .eval("target <- function(a,b) paste(a,b,sep='/')")
        .unwrap();
    load(&mut session, &bytes);
    assert!(
        session.eval("f('left','right')").is_err(),
        "mutated SETTAG must execute instead of falling back to retained source"
    );
}

#[test]
fn nested_named_promises_survive_gctorture_and_gnu_roundtrip() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-named-calls/nested-reversed-tags.rds");
    let mut session = RSession::new().unwrap();
    session
        .eval("target <- function(a,b) paste(a,b,sep='/')")
        .unwrap();
    load(&mut session, fixture);
    let result = session
        .eval(
            "local({gctorture(TRUE); on.exit(gctorture(FALSE)); g<-unserialize(serialize(f,NULL)); identical(g('left','right'),'left/right')})",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}
