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

#[test]
fn serialize_null_refhook_falls_back_to_normal_environment_serialization() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("e<-new.env();e$x<-7L;r<-serialize(e,NULL,refhook=function(x)NULL);identical(unserialize(r)$x,7L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn unserialize_refhook_can_replace_an_environment_and_preserve_identity() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("e<-new.env();r<-serialize(e,NULL,refhook=function(x)'token');replacement<-new.env();identical(unserialize(r,refhook=function(x)replacement),replacement)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn persistent_hook_fixture_restores_through_the_callback() {
    let mut session = RSession::new().unwrap();
    let bytes = include_bytes!("fixtures/gnu-persistence-hooks/persistent-token.rds");
    assert_eq!(
        session
            .eval(&format!(
                "replacement<-new.env();identical(unserialize({},refhook=function(x){{stopifnot(identical(x,'token'));replacement}}),replacement)",
                raw_expression(bytes)
            ))
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn serialize_rejects_invalid_and_empty_refhook_results() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("e<-new.env();a<-tryCatch(serialize(e,NULL,refhook=function(x)1L),error=function(err)TRUE);b<-tryCatch(serialize(e,NULL,refhook=function(x)character()),error=function(err)TRUE);identical(c(a,b),c(TRUE,TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn unserialize_without_a_restore_hook_reports_the_gnu_error() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("e<-new.env();r<-serialize(e,NULL,refhook=function(x)'token');identical(tryCatch(unserialize(r),error=function(err)conditionMessage(err)), 'no restore method available')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn repeated_references_invoke_hooks_and_restore_to_one_identity() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("e<-new.env();hits<-0L;r<-serialize(list(e,e),NULL,refhook=function(x){hits<<-hits+1L;'token'});replacement<-new.env();z<-unserialize(r,refhook=function(x)replacement);identical(c(hits,identical(z[[1]],z[[2]])),c(2L,TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn refhook_survives_gc_and_ascii_version_two_serialization() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("e<-new.env();gctorture(TRUE);r<-serialize(e,NULL,ascii=TRUE,version=2,refhook=function(x){invisible(gc());'token'});gctorture(FALSE);replacement<-new.env();is.raw(r)&&identical(unserialize(r,refhook=function(x)replacement),replacement)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn atomic_values_do_not_invoke_refhook_and_all_formats_restore() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("hits<-0L;invisible(serialize(list(1:3,'text',TRUE),NULL,refhook=function(x){hits<<-hits+1L;'token'}));identical(hits,0L)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    for (ascii, xdr) in [("FALSE", "TRUE"), ("FALSE", "FALSE"), ("TRUE", "TRUE")] {
        for version in [2, 3] {
            let code = format!(
                "e<-new.env();r<-serialize(e,NULL,ascii={ascii},xdr={xdr},version={version},refhook=function(x)'token');replacement<-new.env();is.raw(r)&&identical(unserialize(r,refhook=function(x)replacement),replacement)"
            );
            assert_eq!(session.eval(&code).unwrap().trim(), "[1] TRUE", "{code}");
        }
    }
}

#[test]
fn restore_hooks_can_return_null_or_quoted_language_values() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("local({r<-serialize(new.env(),NULL,refhook=function(x)'token');identical(unserialize(r,refhook=function(x)NULL),NULL)&&identical(unserialize(r,refhook=function(x)quote(a+b)),quote(a+b))})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn hooks_exclude_special_environments_but_not_ordinary_package_name_bindings() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("local({hits<-0L;invisible(serialize(list(globalenv(),baseenv(),emptyenv(),1L),NULL,refhook=function(x){hits<<-hits+1L;'token'}));e<-new.env();e$.packageName<-'ordinary';invisible(serialize(e,NULL,refhook=function(x){hits<<-hits+1L;'token'}));identical(hits,1L)})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn persistent_writer_matches_the_gnu_wire_fixture() {
    let mut session = RSession::new().unwrap();
    let bytes = include_bytes!("fixtures/gnu-persistence-hooks/persistent-token.rds");
    // Writer version/native encoding metadata differs; compare the actual
    // persistent-record payload after the version-3 header on each side.
    let encoding_len = i32::from_be_bytes(bytes[14..18].try_into().unwrap()) as usize;
    let payload = &bytes[18 + encoding_len..];
    let value = session.eval(&format!("r<-serialize(new.env(),NULL,version=3,refhook=function(x)'token');identical(r[-(1:18)],{})", raw_expression(payload))).unwrap();
    assert_eq!(value.trim(), "[1] TRUE", "port bytes: {}", session.eval("as.integer(r)").unwrap());
}
