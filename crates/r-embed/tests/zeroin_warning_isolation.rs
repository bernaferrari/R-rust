use r_embed::{RSession, RuntimePathPolicy};

// Oracle: Homebrew GNU R 4.6.1. Each warning is signalled during the
// evaluation that produced it, including under nested optimizers and warn=2.
#[test]
fn warnings_are_scoped_to_the_callback_and_match_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/zeroin-warning-isolation/controls.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/zeroin-warning-isolation/controls.out"),
            "portable={portable}"
        );
    }
}

#[test]
fn warning_abort_does_not_contaminate_another_session() {
    for portable in [false, true] {
        let make_session = || {
            if portable {
                RSession::new_with_path_policy(RuntimePathPolicy::new(
                    Vec::new(),
                    std::env::temp_dir(),
                ))
            } else {
                RSession::new()
            }
            .unwrap()
        };
        let mut first = make_session();
        let mut second = make_session();
        let actual = first
            .eval(
                r#"
            n <- 0L
            result <- tryCatch(withCallingHandlers(optimize(function(x) {
                n <<- n + 1L; NaN
            }, c(-1, 1)), warning=function(w) stop("halted")),
            error=function(e) conditionMessage(e))
            cat(n, result, "\n")
        "#,
            )
            .unwrap();
        assert_eq!(actual, "1 halted \n", "portable={portable}");
        let actual = second
            .eval(
                r#"
            seen <- 0L
            invisible(withCallingHandlers(optimize(function(x) x*x, c(-1, 1)),
                warning=function(w) { seen <<- seen + 1L; invokeRestart("muffleWarning") }))
            cat(seen, "\n")
        "#,
            )
            .unwrap();
        assert_eq!(actual, "0 \n", "portable={portable}");
    }
}
