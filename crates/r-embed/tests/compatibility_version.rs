use r_embed::{RSession, RValue, RuntimePathPolicy};

fn portable_session() -> RSession {
    RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap()
}

#[test]
fn version_entry_points_use_the_pinned_gnu_compatibility_target() {
    let mut session = portable_session();
    session.eval("v<-R.Version()").unwrap();
    for check in [
        "identical(v$major,'4')",
        "identical(v$minor,'7.0')",
        "identical(as.character(getRversion()),'4.7.0')",
        "identical(R.version$version.string,R.version.string)",
        "identical(v$version.string,R.version.string)",
        "identical(v$year,'2026')",
        "identical(v$month,'08')",
        "identical(v$day,'27')",
        "identical(v[['svn rev']],'90451')",
        "grepl('Rust Port',R.version.string,fixed=TRUE)",
    ] {
        assert_eq!(
            session
                .eval_result(check)
                .unwrap_or_else(|error| panic!("{check}: {error}"))
                .value,
            RValue::Logical(Some(true)),
            "{check}"
        );
    }
}

#[test]
fn original_gnu_writer_and_minimum_reader_versions_are_preserved() {
    let mut session = portable_session();
    assert_eq!(
        session
            .eval_result(
                "a<-strsplit(rawToChar(serialize(1L,NULL,ascii=TRUE,version=2L)),'\\n',fixed=TRUE)[[1L]]; \
                 b<-strsplit(rawToChar(serialize(1L,NULL,ascii=TRUE,version=3L)),'\\n',fixed=TRUE)[[1L]]; \
                 identical(a[3:4],c('263936','131840')) && identical(b[3:4],c('263936','197888'))",
            )
            .unwrap()
            .value,
        RValue::Logical(Some(true)),
    );
}

#[test]
fn numeric_version_format_and_character_conversion_match_gnu_missing_and_empty_values() {
    let mut session = portable_session();
    session.eval("v<-structure(list(c(4L,7L,0L),integer()),class='numeric_version',names=c('target','missing'))").unwrap();
    for check in [
        "identical(format(v),c(target='4.7.0',missing=NA_character_))",
        "identical(as.character(v),c('4.7.0',NA_character_))",
        "identical(as.character(structure(list(),class='numeric_version')),character())",
        "identical(as.vector(NULL,'character'),character())",
        "identical(as.vector(NULL,'integer'),integer())",
        "identical(as.vector(NULL,'double'),double())",
        "identical(as.vector(NULL,'logical'),logical())",
        "identical(as.vector(NULL,'complex'),complex())",
        "identical(as.vector(NULL,'raw'),raw())",
        "identical(as.vector(NULL,'any'),NULL)",
        "identical(as.vector(NULL,'list'),list())",
        "identical(as.vector(NULL,'pairlist'),NULL)",
        "identical(as.vector(NULL,'expression'),expression(NULL))",
        "{empty<-character(); empty[logical()]<-NULL; identical(empty,character())}",
    ] {
        assert_eq!(
            session
                .eval_result(check)
                .unwrap_or_else(|error| panic!("{check}: {error}"))
                .value,
            RValue::Logical(Some(true)),
            "{check}"
        );
    }
}
