use r_embed::RSession;

fn load(s: &mut RSession, bytes: &[u8]) -> Result<String, r_embed::RSessionError> {
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    s.eval(&format!("f<-unserialize(as.raw(c({raw})))"))
}

#[test]
fn gnu_dup_executes_and_preserves_integer_na_and_gc_roots() {
    let mut s = RSession::new().unwrap();
    load(
        &mut s,
        include_bytes!("fixtures/gnu-bytecode-dup/duplicate.rds"),
    )
    .unwrap();
    assert_eq!(
        s.eval("identical(f(c(2L,NA_integer_)),c(4L,NA_integer_))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        s.eval("Ops.foo<-function(e1,e2){gc();42L};identical(f(structure(1,class='foo')),42L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn gnu_dup_uses_stream_and_rejects_underflow_recoverably() {
    let bytes = include_bytes!("fixtures/gnu-bytecode-dup/duplicate.rds");
    let stream = [12_i32, 20, 1, 5, 44, 0, 1]
        .iter()
        .flat_map(|x| x.to_be_bytes())
        .collect::<Vec<_>>();
    let offsets = bytes
        .windows(stream.len())
        .enumerate()
        .filter_map(|(i, x)| (x == stream).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1);
    let at = offsets[0];
    let mut changed = bytes.to_vec();
    changed[at + 16..at + 20].copy_from_slice(&46_i32.to_be_bytes());
    let mut s = RSession::new().unwrap();
    load(&mut s, &changed).unwrap();
    assert_eq!(s.eval("identical(f(3L),9L)").unwrap().trim(), "[1] TRUE");
    let invisible = [12_i32, 20, 1, 15, 5, 4, 1]
        .iter()
        .flat_map(|x| x.to_be_bytes())
        .collect::<Vec<_>>();
    changed[at..at + stream.len()].copy_from_slice(&invisible);
    load(&mut s, &changed).unwrap();
    assert_eq!(
        s.eval("identical(withVisible(f(3L)),list(value=3L,visible=FALSE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    changed[at + 4..at + 8].copy_from_slice(&5_i32.to_be_bytes());
    let result = load(&mut s, &changed).and_then(|_| s.eval("f(3L)"));
    assert!(result.is_err());
    assert_eq!(s.eval("1+1").unwrap().trim(), "[1] 2");
}
