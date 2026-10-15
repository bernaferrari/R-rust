//! Public filename serialization through the actual bounded session store.
use r_embed::{RSession, RValue, RuntimePathPolicy};
// Independent pinned GNU bac583951b728e97b9786804d3b4081f0fe18df5 (r90451):
// x<-list(x=c(1L,2L,3L),message="café",missing=NA_real_)
// saveRDS(x,path,compress=FALSE); saveRDS(x,path)
const GNU_PLAIN: &[u8] = include_bytes!("fixtures/gnu-browser-rds/plain.rds");
const GNU_GZIP: &[u8] = include_bytes!("fixtures/gnu-browser-rds/gzip.rds");

const VALUE: &str = "list(x=c(1L,2L,3L),message='café',missing=NA_real_)";
fn portable() -> RSession {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    session.enable_browser_files();
    session
}
fn check(session: &mut RSession, code: &str) {
    assert_eq!(
        session
            .eval_result(code)
            .unwrap_or_else(|error| panic!("{code}: {error}"))
            .value,
        RValue::Logical(Some(true)),
        "{code}"
    );
}
#[test]
fn browser_serialization_reads_independent_gnu_plain_and_gzip_files() {
    let mut session = portable();
    for (name, bytes) in [("gnu-plain.rds", GNU_PLAIN), ("gnu-gzip.rds", GNU_GZIP)] {
        session.import_file(name, bytes).unwrap();
        check(
            &mut session,
            &format!("identical(readRDS('{name}'),{VALUE})"),
        );
    }
}
#[test]
fn browser_serialization_roundtrips_codecs_invisibly_between_isolated_sessions() {
    let mut left = portable();
    let mut right = portable();
    for compress in ["FALSE", "TRUE", "'gzip'", "'bzip2'", "'xz'"] {
        check(
            &mut left,
            &format!(
                "{{saved<-withVisible(saveRDS({VALUE},'saved.rds',compress={compress}));is.null(saved$value)&&!saved$visible&&identical(readRDS('saved.rds'),{VALUE})}}"
            ),
        );
        let bytes = left.export_file("saved.rds").unwrap();
        assert!(!bytes.is_empty());
        assert!(right.export_file("saved.rds").is_err());
        right.import_file("received.rds", &bytes).unwrap();
        check(
            &mut right,
            &format!("identical(readRDS('received.rds'),{VALUE})"),
        );
    }
}
#[test]
fn browser_serialization_failed_writes_preserve_existing_bytes_and_recover() {
    let mut session = portable();
    session.import_file("saved.rds", &[7, 8]).unwrap();
    assert!(
        session
            .eval("saveRDS(raw(1048576),'saved.rds',compress=FALSE)")
            .is_err()
    );
    assert_eq!(session.export_file("saved.rds").unwrap(), [7, 8]);
    assert!(
        session
            .eval("saveRDS(1L,'../outside.rds',compress=FALSE)")
            .is_err()
    );
    assert!(session.eval("readRDS('missing.rds')").is_err());
    assert!(session.eval("readRDS('saved.rds')").is_err());
    check(
        &mut session,
        "{saveRDS(41L,'saved.rds',compress=FALSE);identical(readRDS('saved.rds')+1L,42L)}",
    );
}
#[test]
fn browser_serialization_never_reads_host_files_when_browser_mode_is_enabled() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), GNU_PLAIN).unwrap();
    let mut session = portable();
    let path = file
        .path()
        .to_str()
        .unwrap()
        .replace('\\', "\\\\")
        .replace('\'', "\\'");
    assert!(session.eval(&format!("readRDS('{path}')")).is_err());
    check(&mut session, "identical(1L+1L,2L)");
}
