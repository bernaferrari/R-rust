//! Real-package corpus: R6 2.6.1 (tests/real-packages/manifest.toml).
//!
//! R6 is the environment-semantics axis of the corpus: portable class
//! generators build objects out of nested binding/enclosing environments
//! (new.env(parent=), environment(fn)<-, list2env, makeActiveBinding for
//! active bindings and self/private, `<<-` walking env chains, super via
//! enclosing-env links, clone by rebuilding the env graph) plus S3 print
//! dispatch on classed generators. These probes are pinned against GNU R
//! 4.7.0 (all TRUE on the oracle).

use r_embed::RSession;

#[test]
fn real_package_corpus_r6() {
    let bundled = std::env::var("RPORT_REAL_PKG_BUNDLED")
        .unwrap_or_else(|_| "/tmp/pkgprobe/bundled".to_string());
    let app = std::env::var("RPORT_REAL_PKG_APP")
        .unwrap_or_else(|_| "/tmp/pkgprobe/app".to_string());
    let cache = std::env::var("RPORT_REAL_PKG_CACHE")
        .unwrap_or_else(|_| "/tmp/pkgprobe/cache".to_string());
    let mut session = RSession::new().expect("session");
    session
        .configure_android_paths(&app, &cache, Some(&bundled))
        .expect("paths");

    // R6 2.6.1 — pass: loads and all seven oracle-pinned probes hold.
    session.load_package("R6").expect("R6 must load");

    session
        .eval_script(
            r#"
Q <- R6Class("Queue",
  public = list(
    items = NULL,
    initialize = function() { self$items <- c() },
    add = function(x) {
      self$items <- c(self$items, x)
      invisible(self)
    }
  ),
  active = list(
    size = function() length(self$items)
  )
)

Counter <- R6Class("Counter",
  public = list(
    n = 0L,
    inc = function() {
      self$n <- self$n + 1L
      invisible(self)
    }
  )
)

Base <- R6Class("Base",
  public = list(
    greet_base = function() "base"
  )
)

Derived <- R6Class("Derived",
  inherit = Base,
  public = list(
    greet = function() paste(super$greet_base(), "derived")
  )
)

q <- Q$new()$add(1)$add(2)
c1 <- Counter$new()$inc()$inc()
"#,
        )
        .expect("R6 classes must define");

    // R1: active bindings read through the object env: q$size == 2.
    assert_eq!(
        session.eval("q$size == 2").expect("R1 active binding size"),
        "[1] TRUE"
    );
    // R2: field mutation via self$ persists in the public binding env.
    assert_eq!(
        session
            .eval("identical(q$items, c(1, 2))")
            .expect("R2 items accumulated"),
        "[1] TRUE"
    );
    // R3: self-assignment through chained method calls.
    assert_eq!(
        session.eval("c1$n == 2L").expect("R3 counter increments"),
        "[1] TRUE"
    );
    // R4: super dispatch through the enclosing-env chain.
    assert_eq!(
        session
            .eval("Derived$new()$greet() == \"base derived\"")
            .expect("R4 super dispatch"),
        "[1] TRUE"
    );
    // R5: clone copies the environment graph, carrying state.
    assert_eq!(
        session
            .eval("identical(c1$clone()$n, 2L)")
            .expect("R5 clone state"),
        "[1] TRUE"
    );
    // R6: S3 class attribute marks R6 objects.
    assert_eq!(
        session.eval("is.R6(q)").expect("R6 is.R6"),
        "[1] TRUE"
    );
    // R7: print dispatches to the generator's S3 print method.
    assert_eq!(
        session
            .eval("grepl(\"Queue\", capture.output(print(Q))[1])")
            .expect("R7 print.R6ClassGenerator"),
        "[1] TRUE"
    );
}
