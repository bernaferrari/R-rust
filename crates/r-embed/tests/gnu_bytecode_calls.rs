use r_embed::RSession;

fn load(session: &mut RSession, bytes: &[u8]) {
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    session
        .eval(&format!("f<-unserialize(as.raw(c({raw})))"))
        .unwrap();
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
        "fixture must contain one nested promise stream"
    );
    offsets[0]
}

#[test]
fn gnu_getfun_makepromise_call_forces_nested_bytecode() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-calls/identity-promise.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session.eval("identical(f(41L),41L)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g('ok'),'ok')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn gnu_call_executes_mutated_promise_stream_over_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-bytecode-calls/identity-promise.rds").to_vec();
    let nested = stream_offset(&bytes, &[12, 20, 0, 1]);
    // The retained source still says identity(x). Change only the nested
    // promise bytecode to return the symbol x, proving GETFUN/MAKEPROM/CALL and force
    // evaluation execute the imported instruction stream.
    bytes[nested + 4..nested + 8].copy_from_slice(&16_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert_eq!(
        session.eval("identical(f(41L),quote(x))").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(99L),quote(x))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    if let Some(oracle) = std::env::var_os("RPORT_PINNED_RSCRIPT") {
        let encoded = session
            .eval("cat(paste(as.integer(serialize(f,NULL)),collapse=','))")
            .unwrap();
        let bytes: Vec<u8> = encoded
            .trim()
            .split(',')
            .map(|byte| byte.parse().unwrap())
            .collect();
        let path =
            std::env::temp_dir().join(format!("rport-nested-call-{}.rds", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let result = std::process::Command::new(oracle).args(["--vanilla","-e","f<-readRDS(commandArgs(TRUE)[1]);stopifnot(identical(f(41L),quote(x)));cat('gnu-ok')"]).arg(&path).output().unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(
            result.status.success(),
            "GNU rejected nested bytecode: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&result.stdout), "gnu-ok");
    }
}

#[test]
fn compiled_calls_preserve_lazy_arguments_call_context_and_primitive_rebinding() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-calls/identity-promise.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    for (code, expected) in [
        (
            "identity<-function(z)42L;f(stop('must stay lazy'))",
            "[1] 42",
        ),
        (
            "identity<-function(z)deparse(sys.call());f(41L)",
            "[1] \"identity(x)\"",
        ),
        (
            "identity<-function(z)deparse(substitute(z));f(41L)",
            "[1] \"x\"",
        ),
        ("identity<-abs;f(-2)", "[1] 2"),
        ("identity<-quote;identical(f(41L),quote(x))", "[1] TRUE"),
    ] {
        assert_eq!(session.eval(code).unwrap().trim(), expected, "{code}");
    }
}

#[test]
fn compiled_call_arguments_survive_gc_and_preserve_outer_stack_values() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-calls/pair-promise.rds"),
    );
    session
        .eval("target<-function(a,b){gc();a+b};x<-41L;y<-1L")
        .unwrap();
    assert_eq!(
        session.eval("gctorture(TRUE);f(x,y)").unwrap().trim(),
        "[1] 42"
    );
    session.eval("gctorture(FALSE)").unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-calls/call-add.rds"),
    );
    assert_eq!(session.eval("f(41L)").unwrap().trim(), "[1] 42");
}
