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
fn pinned_gnu_scalar_closures_preserve_type_visibility_and_gnu_interop() {
    let cases: [(&str, &[u8], &str, &str, &str); 3] = [
        (
            "gnu-null-closure.rds",
            include_bytes!("fixtures/gnu-null-closure.rds"),
            "cat(is.null(f()))",
            "TRUE",
            "is.null(f())",
        ),
        (
            "gnu-true-closure.rds",
            include_bytes!("fixtures/gnu-true-closure.rds"),
            "cat(typeof(f()),f())",
            "logical TRUE",
            "identical(f(),TRUE)",
        ),
        (
            "gnu-false-closure.rds",
            include_bytes!("fixtures/gnu-false-closure.rds"),
            "cat(typeof(f()),f())",
            "logical FALSE",
            "identical(f(),FALSE)",
        ),
    ];
    let mut session = RSession::new().unwrap();
    for (fixture, bytes, check, expected, gnu_check) in cases {
        let values = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        session
            .eval(&format!("f <- unserialize(as.raw(c({values})))"))
            .unwrap();
        assert_eq!(session.eval(check).unwrap().trim(), expected);
        let restored_check = check.replace("f()", "g()");
        assert_eq!(
            session
                .eval(&format!(
                    "g <- unserialize(serialize(f,NULL)); {restored_check}"
                ))
                .unwrap()
                .trim(),
            expected
        );
        session
            .eval("stopifnot(withVisible(f())$visible, withVisible(g())$visible)")
            .unwrap();

        if let Ok(rscript) = std::env::var("RPORT_PINNED_RSCRIPT") {
            let encoded = session
                .eval("cat(paste(as.integer(serialize(f,NULL)),collapse=','))")
                .unwrap();
            let emitted = encoded
                .trim()
                .split(',')
                .map(|byte| byte.parse::<u8>().unwrap())
                .collect::<Vec<_>>();
            let path = std::env::temp_dir().join(format!(
                "rport-gnu-scalar-{}-{}.rds",
                std::process::id(),
                fixture.replace('.', "-")
            ));
            std::fs::write(&path, emitted).unwrap();
            let output = std::process::Command::new(&rscript)
                .args([
                    "-e",
                    &format!(
                        "f<-readRDS(commandArgs(TRUE)[1]);stopifnot({gnu_check});cat('gnu-ok')"
                    ),
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

#[test]
fn malformed_gnu_constant_indexes_cannot_escape_through_source_fallback() {
    let original = include_bytes!("fixtures/gnu-constant-closure.rds");
    let prefix = [0, 0, 0, 12, 0, 0, 0, 16];
    let offsets: Vec<_> = original
        .windows(prefix.len())
        .enumerate()
        .filter_map(|(i, words)| (words == prefix).then_some(i))
        .collect();
    assert_eq!(offsets.len(), 1);
    let mut session = RSession::new().unwrap();
    for index in [-1_i32, i32::MAX] {
        let mut bytes = original.to_vec();
        bytes[offsets[0] + 8..offsets[0] + 12].copy_from_slice(&index.to_be_bytes());
        let values = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        assert!(
            session
                .eval(&format!("unserialize(as.raw(c({values})))"))
                .is_err()
        );
        assert_eq!(session.eval("1 + 1").unwrap(), "[1] 2");
    }
}

#[test]
fn gnu_literal_instruction_wins_over_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-true-closure.rds").to_vec();
    let stream = [0, 0, 0, 12, 0, 0, 0, 18, 0, 0, 0, 1];
    let offsets: Vec<_> = bytes
        .windows(stream.len())
        .enumerate()
        .filter_map(|(i, words)| (words == stream).then_some(i))
        .collect();
    assert_eq!(offsets.len(), 1);
    // Keep the compiler's TRUE source metadata but replace LDTRUE with LDFALSE.
    bytes[offsets[0] + 7] = 19;
    let values = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut s = RSession::new().unwrap();
    assert_eq!(
        s.eval(&format!("f <- unserialize(as.raw(c({values}))); f()"))
            .unwrap(),
        "[1] FALSE"
    );
    assert_eq!(
        s.eval("g <- unserialize(serialize(f,NULL)); g()").unwrap(),
        "[1] FALSE"
    );
}
