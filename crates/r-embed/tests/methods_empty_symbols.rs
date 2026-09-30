use r_embed::RSession;

/// GNU methods does not bind `rep` or `c`. An empty-symbol binding under
/// either name would shadow `base::rep` / `base::c` for non-function lookup
/// inside methods closures.
#[test]
fn methods_namespace_does_not_bind_empty_c_or_rep() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            r#"
            local({
                invisible(require(methods, quietly = TRUE))
                ns <- asNamespace("methods")
                rep_bound <- exists("rep", envir = ns, inherits = FALSE)
                c_bound <- exists("c", envir = ns, inherits = FALSE)
                f <- function() rep(1:2, each = 2)
                environment(f) <- ns
                rep_ok <- identical(f(), base::rep(1:2, each = 2))
                if (rep_bound || c_bound || !rep_ok) {
                    describe <- function(name) {
                        tryCatch({
                            value <- get(name, envir = ns, inherits = FALSE)
                            paste0(
                                typeof(value),
                                ":",
                                paste(as.character(value), collapse = ",")
                            )
                        }, error = function(e) paste0("ERR:", conditionMessage(e)))
                    }
                    stop(sprintf(
                        "exists rep=%s c=%s rep_ok=%s getrep=%s getc=%s",
                        rep_bound, c_bound, rep_ok, describe("rep"), describe("c")
                    ))
                }
                TRUE
            })
            "#,
        )
        .unwrap();
    assert!(
        result.contains("TRUE"),
        "methods namespace must not bind empty rep/c, and rep(1:2, each=2) must match base::rep: {result}"
    );
}
