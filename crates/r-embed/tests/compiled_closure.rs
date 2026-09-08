use r_embed::RSession;

#[test]
fn pinned_gnu_compiled_closure_runs_through_retained_source() {
    // GNU R bac583951b728e97b9786804d3b4081f0fe18df5, compiler::cmpfun,
    // saveRDS(version=2, compress=FALSE). Expected values computed by GNU R.
    let bytes = include_bytes!("fixtures/gnu-compiled-closure.rds");
    let values = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(&format!(
            "f <- unserialize(as.raw(c({values}))); cat(f(3),f(-5))"
        ))
        .unwrap();
    assert_eq!(result.trim(), "10 3");
}

#[test]
fn compiled_closure_preserves_capture_defaults_nested_functions_and_loops() {
    let bytes = include_bytes!("fixtures/gnu-compiled-captured.rds");
    let values = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(&format!(
            "g <- unserialize(as.raw(c({values}))); cat(g(),g(4))"
        ))
        .unwrap();
    assert_eq!(result.trim(), "9 22");
    let roundtrip = session.eval("gctorture(TRUE); restored <- unserialize(serialize(g,NULL)); answer <- c(restored(),restored(4)); gctorture(FALSE); cat(answer)").unwrap();
    assert_eq!(roundtrip.trim(), "9 22");
}

#[test]
fn compiled_closure_import_survives_gc_and_truncation_errors() {
    let bytes = include_bytes!("fixtures/gnu-compiled-closure.rds");
    let values = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut session = RSession::new().unwrap();
    session
        .eval(&format!("data <- as.raw(c({values}))"))
        .unwrap();
    assert!(session.eval("unserialize(data[1:100])").is_err());
    let result = session.eval("gctorture(TRUE); f <- unserialize(data); answer <- f(3); gctorture(FALSE); cat(answer)").unwrap();
    assert_eq!(result.trim(), "10");
}

#[test]
fn environment_roundtrip_preserves_cycles_bindings_and_lock() {
    let mut session = RSession::new().unwrap();
    let result = session.eval("e <- new.env(parent=baseenv(), hash=FALSE); e$x <- 7; e$self <- e; lockEnvironment(e); saved <- serialize(e,NULL); copy <- unserialize(saved); cat(copy$x,identical(copy$self,copy),environmentIsLocked(copy))").unwrap();
    assert_eq!(result.trim(), "7 TRUE TRUE");
    assert!(session.eval("copy$extra <- 1").is_err());
}

#[test]
fn unsupported_binding_semantics_fail_instead_of_being_erased() {
    let mut session = RSession::new().unwrap();
    session
        .eval("e <- new.env(); e$x <- 1; lockBinding('x',e)")
        .unwrap();
    assert!(session.eval("serialize(e,NULL)").is_err());
    let result = session.eval("cat(2+2)").unwrap();
    assert_eq!(result.trim(), "4");
}
