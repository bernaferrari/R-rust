//! Real-package corpus: rematch 2.0.0 (tests/real-packages/manifest.toml).
//!
//! rematch is the regex-capture axis of the corpus: re_match / re_match_all
//! wrap regexpr / gregexpr with perl = TRUE and walk the capture attributes
//! those builtins attach — capture.start / capture.length matrices with
//! dimnames list(NULL, capture.names), the PCRE2_UNSET arithmetic (0/0 for
//! non-participating groups, -1/-1 rows for non-matching text, one -1 row
//! on gregexpr's no-match elements), gregexpr's global multi-match
//! iteration, vapply folding per-group substring columns into the result
//! matrix, and cbind/rbind/colnames shaping of the character-matrix result.
//! These probes are pinned against GNU R 4.7.0 (all TRUE on the oracle;
//! argument order is re_match(pattern, text)).

use r_embed::RSession;

#[test]
fn real_package_corpus_rematch() {
    // SAFETY: test-process setup before any threads exist.
    let bundled = std::env::var("RPORT_REAL_PKG_BUNDLED")
        .unwrap_or_else(|_| "/tmp/pkgprobe/bundled".to_string());
    let app =
        std::env::var("RPORT_REAL_PKG_APP").unwrap_or_else(|_| "/tmp/pkgprobe/app".to_string());
    let cache =
        std::env::var("RPORT_REAL_PKG_CACHE").unwrap_or_else(|_| "/tmp/pkgprobe/cache".to_string());
    let mut session = RSession::new().expect("session");
    session
        .configure_android_paths(&app, &cache, Some(&bundled))
        .expect("paths");

    // rematch 2.0.0 — pass: loads and all four oracle-pinned probes hold.
    session.load_package("rematch").expect("rematch must load");

    session
        .eval_script(
            r#"
m <- re_match("(?<word>[a-z]+)(?<num>[0-9]+)", "abc123")
m2 <- re_match("(?<word>[a-z]+)(?<num>[0-9]+)", "xyz")
ma <- re_match_all("(?<w>[a-z])(?<n>[0-9])", c("a1 b2", "zz"))
"#,
        )
        .expect("rematch captures must extract");

    // RM1: named groups become colnamed matrix columns after .match.
    assert_eq!(
        session
            .eval(
                r#"identical(colnames(m), c(".match","word","num")) && m[1,"word"]=="abc" && m[1,"num"]=="123""#
            )
            .expect("RM1 named capture columns"),
        "[1] TRUE"
    );
    // RM2: non-matching text yields NA in .match and every group column.
    assert_eq!(
        session
            .eval(r#"is.na(m2[1,"num"]) && is.na(m2[1,".match"])"#)
            .expect("RM2 no-match NAs"),
        "[1] TRUE"
    );
    // RM3: re_match_all returns one 0-or-more-row matrix per text element.
    assert_eq!(
        session
            .eval(
                r#"is.list(ma) && length(ma)==2 && nrow(ma[[1]])==2 && ma[[1]][1,"w"]=="a" && nrow(ma[[2]])==0"#
            )
            .expect("RM3 all-matches matrices"),
        "[1] TRUE"
    );
    // RM4: the result is a character matrix.
    assert_eq!(
        session
            .eval(r#"is.matrix(m) && is.character(m)"#)
            .expect("RM4 character matrix"),
        "[1] TRUE"
    );
}
