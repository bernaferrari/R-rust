//! Pinned real-package corpus entry: fortunes 1.5-5
//! (tests/real-packages/manifest.toml). Kept in its own test target so the
//! corpus assertion set can grow per package without concurrent-tree edit
//! collisions. Loading mirrors `real_package_corpus` in embed.rs: unpacked
//! tarball under RPORT_REAL_PKG_BUNDLED, loaded through `library()`.

mod support;

use r_embed::RSession;

#[test]
fn real_package_corpus_fortunes() {
    let corpus = support::PackageCorpus::new();
    let (app, cache, bundled) = (&corpus.app, &corpus.cache, &corpus.bundled);
    let mut session = RSession::new().expect("session");
    session
        .configure_android_paths(app, cache, Some(bundled))
        .expect("paths");

    // fortunes 1.5-5 — pass: loads and all five manifest probes hold:
    // S3 print dispatch on class "fortune", read.table over the package's
    // inst/ CSV (sep/quote/colClasses, 400+ rows), rbind/data.frame
    // construction, capture.output, and $ on the S3 object.
    session
        .load_package("fortunes")
        .expect("fortunes must load");
    assert_eq!(
        session
            .eval("identical(class(fortune(10)), \"fortune\")")
            .expect("fortune class probe"),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("grepl(\"SAS\", capture.output(print(fortune(10)))[2])")
            .expect("fortune print probe"),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("is.data.frame(read.fortunes())")
            .expect("read.fortunes data.frame probe"),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("nrow(read.fortunes()) > 300")
            .expect("read.fortunes row count probe"),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("grepl(\"SAS\", fortune(10)$quote)")
            .expect("fortune quote probe"),
        "[1] TRUE"
    );

    // F6: fortune(N) is consistent with read.fortunes() row N (author).
    let f6 = session
        .eval("identical(fortune(10)$author, read.fortunes()$author[10])")
        .expect("F6 eval");
    assert_eq!(f6.trim_end(), "[1] TRUE");
}
