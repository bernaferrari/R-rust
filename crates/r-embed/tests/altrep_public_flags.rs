use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn attribute_free_s4_vectors_preserve_flags_across_serialization_versions() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    let actual = session.eval(r#"local({
        for (value in list(1:3, seq(1,3,by=0.5), integer(0), c(7L,8L))) {
            x <- asS4(value)
            cat(typeof(x), length(x), is.null(attributes(x)), isS4(x), is.object(x), "\n")
            for (version in c(2L,3L)) {
                y <- unserialize(serialize(x,NULL,version=version))
                cat(version, typeof(y), is.null(attributes(y)), isS4(y), is.object(y), identical(x,y), "\n")
            }
        }
    })"#).unwrap();
    assert_eq!(
        actual,
        concat!(
            "integer 3 TRUE TRUE FALSE \n2 integer TRUE TRUE FALSE TRUE \n3 integer TRUE TRUE FALSE TRUE \n",
            "double 5 TRUE TRUE FALSE \n2 double TRUE TRUE FALSE TRUE \n3 double TRUE TRUE FALSE TRUE \n",
            "integer 0 TRUE TRUE FALSE \n2 integer TRUE TRUE FALSE TRUE \n3 integer TRUE TRUE FALSE TRUE \n",
            "integer 2 TRUE TRUE FALSE \n2 integer TRUE TRUE FALSE TRUE \n3 integer TRUE TRUE FALSE TRUE \n",
        )
    );
}

#[test]
fn s4_and_plain_node_types_match_pinned_gnu_across_stream_formats() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    let actual = session
        .eval(include_str!("fixtures/s4-public-flags/roundtrip.R"))
        .unwrap();
    assert_eq!(
        actual,
        include_str!("fixtures/s4-public-flags/roundtrip.out")
    );
}

#[test]
fn original_reader_accepts_independent_gnu_s4_node_bundles() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    session.enable_browser_files();
    for bytes in [
        include_bytes!("fixtures/s4-public-flags/gnu-s4-v2.rds").as_slice(),
        include_bytes!("fixtures/s4-public-flags/gnu-s4-v3.rds").as_slice(),
    ] {
        session.import_file("gnu-s4.rds", bytes).unwrap();
        let output = session.eval("x<-readRDS('gnu-s4.rds'); cat(length(x),all(vapply(x,isS4,logical(1))),isS4(x),'\\n')").unwrap();
        assert_eq!(output, "14 TRUE FALSE \n");
    }
}
