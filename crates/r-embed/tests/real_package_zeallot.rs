//! Real-package corpus: zeallot 0.2.0 (tests/real-packages/manifest.toml).
//!
//! zeallot is the destructuring-assignment axis of the corpus: custom infix
//! `%<-%`/`%->%` operators, `destructure()` S3 dispatch (default /
//! data.frame / summary.lm methods), nested `c()` LHS patterns, named
//! element matching, collector/skip syntax, and environment-assignment
//! semantics — the operator captures its LHS via `substitute()`, unpacks it
//! against the value, and replays the bindings into the caller's frame
//! (`parent.frame()` through a forced argument promise) with
//! `eval(call("<-", name, quote(value)), envir)`. Error paths build classed
//! conditions through `errorCondition()` + `sys.calls()` attribution.
//! These probes are pinned against GNU R 4.7.0 (all TRUE on the oracle).

mod support;

use r_embed::RSession;

#[test]
fn real_package_corpus_zeallot() {
    let corpus = support::PackageCorpus::new();
    let (app, cache, bundled) = (&corpus.app, &corpus.cache, &corpus.bundled);
    let mut session = RSession::new().expect("session");
    session
        .configure_android_paths(app, cache, Some(bundled))
        .expect("paths");

    // zeallot 0.2.0 — pass: loads and all six oracle-pinned probes hold.
    session.load_package("zeallot").expect("zeallot must load");

    // Z1: flat unpacking of an atomic vector into caller-frame bindings.
    assert_eq!(
        session
            .eval("c(x, y) %<-% c(1, 2); x + y == 3")
            .expect("Z1 flat unpack"),
        "[1] TRUE"
    );
    // Z2: nested c() LHS pattern unpacks nested list values recursively.
    assert_eq!(
        session
            .eval("c(a, c(b, d)) %<-% list(1, list(2, 3)); a + b + d == 6")
            .expect("Z2 nested unpack"),
        "[1] TRUE"
    );
    // Z3: RHS names do not rebind LHS positionals (element-wise matching).
    assert_eq!(
        session
            .eval("c(n1, n2) %<-% c(a=1, b=2); n1 == 1 && n2 == 2")
            .expect("Z3 named RHS"),
        "[1] TRUE"
    );
    // Z4: short value raises the classed zeallot condition with the exact
    // oracle message (conditionMessage through tryCatch).
    assert_eq!(
        session
            .eval("tryCatch({ c(x, y) %<-% c(1) }, error = function(e) conditionMessage(e))")
            .expect("Z4 error message"),
        "[1] \"missing value for variable `y`\""
    );
    // Z5: right-to-left operator mirrors %<-%.
    assert_eq!(
        session
            .eval("c(1, 2) %->% c(p, q); q == 2")
            .expect("Z5 %->% operator"),
        "[1] TRUE"
    );
    // Z6: destructure() S3 dispatch — data.frame method splits columns.
    assert_eq!(
        session
            .eval("c(da, db) %<-% data.frame(a=11, b=22); da == 11 && db == 22")
            .expect("Z6 data.frame destructure"),
        "[1] TRUE"
    );
}
