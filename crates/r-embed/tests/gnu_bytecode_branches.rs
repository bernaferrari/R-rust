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

fn unique_stream_offset(bytes: &[u8], words: &[i32]) -> usize {
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
        "fixture must contain one exact instruction stream"
    );
    offsets[0]
}

#[test]
fn pinned_getvar_and_branch_streams_execute_with_gnu_types_and_visibility() {
    let mut session = RSession::new().unwrap();

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-branches/identity.rds"),
    );
    assert_eq!(
        session
            .eval("v <- withVisible(f(41L)); cat(typeof(v$value),v$value,v$visible)")
            .unwrap()
            .trim(),
        "integer 41 TRUE"
    );
    assert_eq!(
        session
            .eval("v <- withVisible(f(invisible(1L))); cat(v$value,v$visible)")
            .unwrap()
            .trim(),
        "1 FALSE"
    );
    assert!(
        session.eval("f()").is_err(),
        "missing x must not become NULL"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-branches/branch.rds"),
    );
    assert_eq!(
        session
            .eval("a<-withVisible(f(TRUE));b<-withVisible(f(FALSE));cat(typeof(a$value),a$value,a$visible,typeof(b$value),b$value,b$visible)")
            .unwrap()
            .trim(),
        "integer 1 TRUE integer 2 TRUE"
    );
    for condition in ["logical(0)", "NA", "c(TRUE,FALSE)"] {
        assert!(
            session.eval(&format!("f({condition})")).is_err(),
            "GNU BRIFNOT condition contract must reject {condition}"
        );
    }

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-branches/noelse.rds"),
    );
    assert_eq!(
        session
            .eval("v<-withVisible(f(FALSE));cat(is.null(v$value),v$visible)")
            .unwrap()
            .trim(),
        "TRUE FALSE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-branches/unbound.rds"),
    );
    assert!(
        session.eval("f()").is_err(),
        "unbound GETVAR must remain an error"
    );
}

#[test]
fn branch_instructions_win_over_retained_source_and_roundtrip_to_gnu() {
    let original = include_bytes!("fixtures/gnu-bytecode-branches/branch.rds");
    let words = [12, 20, 1, 3, 0, 9, 16, 2, 1, 16, 3, 1];
    let offset = unique_stream_offset(original, &words);
    let mut changed = original.to_vec();
    // Change only the true-arm LDCONST operand from pool entry 2 (1L) to 3
    // (2L); the retained source expression still says `if (x) 1L else 2L`.
    changed[offset + 7 * 4..offset + 8 * 4].copy_from_slice(&3_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    assert_eq!(session.eval("cat(f(TRUE),f(FALSE))").unwrap().trim(), "2 2");
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));cat(g(TRUE),g(FALSE))")
            .unwrap()
            .trim(),
        "2 2"
    );

    if let Some(oracle) = std::env::var_os("RPORT_PINNED_RSCRIPT") {
        let encoded = session
            .eval("cat(paste(as.integer(serialize(f,NULL)),collapse=','))")
            .unwrap();
        let bytes = encoded
            .trim()
            .split(',')
            .map(|byte| byte.parse::<u8>().unwrap())
            .collect::<Vec<_>>();
        let path = std::env::temp_dir().join(format!(
            "rport-gnu-bytecode-branch-{}.rds",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        let result = std::process::Command::new(oracle)
            .args([
                "--vanilla",
                "-e",
                "f<-readRDS(commandArgs(TRUE)[1]);stopifnot(identical(f(TRUE),2L),identical(f(FALSE),2L));cat('gnu-ok')",
            ])
            .arg(&path)
            .output()
            .unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(
            result.status.success(),
            "GNU R rejected emitted bytecode: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&result.stdout), "gnu-ok");
    }
}

#[test]
fn malformed_branch_targets_fail_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-branches/branch.rds");
    let words = [12, 20, 1, 3, 0, 9, 16, 2, 1, 16, 3, 1];
    let offset = unique_stream_offset(original, &words);
    let mut session = RSession::new().unwrap();

    for target in [7_i32, i32::MAX, -1] {
        let mut malformed = original.to_vec();
        malformed[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&target.to_be_bytes());
        assert!(
            session
                .eval(&format!("unserialize({})", raw_expression(&malformed)))
                .is_err(),
            "malformed target {target} must not run retained source"
        );
        assert_eq!(session.eval("1+1").unwrap(), "[1] 2");
    }
}
