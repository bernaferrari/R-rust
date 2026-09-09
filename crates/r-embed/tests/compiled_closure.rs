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
fn pinned_gnu_constant_closure_executes_its_instruction_constant() {
    // GNU R bac583951b728e97b9786804d3b4081f0fe18df5, compiler::cmpfun,
    // saveRDS(version=2, compress=FALSE). The body source is retained for
    // deparsing, while the GNU LDCONST stream supplies the returned value.
    let bytes = include_bytes!("fixtures/gnu-constant-closure.rds");
    let values = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(&format!("f <- unserialize(as.raw(c({values}))); cat(f())"))
        .unwrap();
    assert_eq!(result.trim(), "42");
    let restored = session
        .eval("g <- unserialize(serialize(f,NULL)); cat(g())")
        .unwrap();
    assert_eq!(restored.trim(), "42");

    // The interoperability gate sets this so GNU R consumes the bytes emitted
    // by this runtime, rather than only round-tripping them locally.
    if let Ok(rscript) = std::env::var("RPORT_PINNED_RSCRIPT") {
        let encoded = session
            .eval("cat(paste(as.integer(serialize(f,NULL)),collapse=','))")
            .unwrap();
        let bytes = encoded
            .trim()
            .split(',')
            .map(|byte| byte.parse::<u8>().unwrap())
            .collect::<Vec<_>>();
        let path =
            std::env::temp_dir().join(format!("rport-gnu-constant-{}.rds", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let output = std::process::Command::new(rscript)
            .args([
                "-e",
                "f<-readRDS(commandArgs(TRUE)[1]);stopifnot(identical(f(),42L));cat('gnu-ok')",
            ])
            .arg(&path)
            .output()
            .unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(
            output.status.success(),
            "GNU R rejected emitted stream: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "gnu-ok");
    }
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
