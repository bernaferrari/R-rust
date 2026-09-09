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
        "fixture must contain one GNU SETVAR stream"
    );
    offsets[0]
}

fn load(session: &mut RSession, bytes: &[u8]) {
    session
        .eval(&format!("f <- unserialize({})", raw_expression(bytes)))
        .unwrap();
}

#[test]
fn imported_gnu_setvar_assigns_and_roundtrips() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-assignment/setvar.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session.eval("identical(f(41L),41L)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL));identical(g(9L),9L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_setvar_executes_over_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-bytecode-assignment/setvar.rds").to_vec();
    let stream = [12, 20, 1, 22, 2, 4, 20, 2, 1];
    let offset = stream_offset(&bytes, &stream);
    // Retained source still assigns y <- x and returns y. Redirect SETVAR to
    // x; the imported stream must then leave y unbound and error.
    bytes[offset + 4 * 4..offset + 5 * 4].copy_from_slice(&1_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert!(
        session.eval("f(41L)").is_err(),
        "mutated SETVAR must execute instead of falling back to retained source"
    );
}

#[test]
fn imported_gnu_setvar2_superassigns_and_mutation_executes() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-assignment/setvar2.rds");
    let mut session = RSession::new().unwrap();
    session.eval("rm(list='y')").unwrap();
    load(&mut session, fixture);
    assert_eq!(
        session.eval("identical(f(41L),41L)").unwrap().trim(),
        "[1] TRUE"
    );
    assert_eq!(session.eval("identical(y,41L)").unwrap().trim(), "[1] TRUE");

    let mut bytes = fixture.to_vec();
    let stream = [12, 20, 1, 95, 2, 4, 20, 2, 1];
    let offset = stream_offset(&bytes, &stream);
    // SETVAR2 x leaves y unbound; retained source still uses y <<- x.
    bytes[offset + 4 * 4..offset + 5 * 4].copy_from_slice(&1_i32.to_be_bytes());
    session.eval("rm(list='y')").unwrap();
    load(&mut session, &bytes);
    assert!(
        session.eval("f(41L)").is_err(),
        "mutated SETVAR2 must execute instead of falling back to retained source"
    );
}

#[test]
fn gnu_setvar2_uses_nearest_enclosing_binding() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-assignment/setvar2.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    let result = session
        .eval(
            "e1<-new.env(parent=.GlobalEnv);e2<-new.env(parent=e1);e1$y<-0L;environment(f)<-e2;identical(f(7L),7L)&&identical(e1$y,7L)&&!exists('y',.GlobalEnv,inherits=FALSE)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn gnu_setvar2_rejects_locked_binding_and_recovers() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-assignment/setvar2.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    let result = session
        .eval(
            "e<-new.env(parent=.GlobalEnv);e$y<-0L;lockBinding('y',e);environment(f)<-e;a<-tryCatch(f(1L),error=function(err)'locked');unlockBinding('y',e);identical(a,'locked')&&identical(f(2L),2L)&&identical(e$y,2L)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn gnu_setvar2_survives_gctorture_and_serialization() {
    let fixture = include_bytes!("fixtures/gnu-bytecode-assignment/setvar2.rds");
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    let result = session
        .eval(
            "local({gctorture(TRUE);on.exit(gctorture(FALSE));g<-unserialize(serialize(f,NULL));identical(g(9L),9L)})",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}
