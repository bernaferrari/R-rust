use r_embed::{RSession, RuntimePathPolicy};

fn session() -> RSession {
    RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap()
}

#[test]
fn optim_workspace_refusal_and_callback_error_release_the_reservation() {
    let mut session = session();
    let fit = "optim(c(2, 3), function(x) sum((x - 1)^2), method = 'L-BFGS-B')";
    session
        .eval(&format!(
            "invisible({fit}); subject <- rep(1, 10000L); calls <- 0L; gc(); invisible(NULL)"
        ))
        .unwrap();
    let before = session.memory_usage().unwrap();
    let mut limits = session.resource_limits();
    limits.max_alloc_bytes = before.total_bytes + 2 * 1024 * 1024;
    session.set_resource_limits(limits).unwrap();

    let oversized = "optim(subject, function(x) { calls <<- calls + 1L; sum(x^2) },
                           method = 'L-BFGS-B', control = list(lmm = 400L))";
    let failing_callback =
        "optim(rep(1, 32L), function(x) stop('objective failed'), method = 'BFGS')";
    let expect_error = |session: &mut RSession, source: &str, message: &str| {
        let error = session.eval(source).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    };
    expect_error(&mut session, oversized, "transient memory");
    expect_error(&mut session, failing_callback, "objective failed");
    assert_eq!(session.eval("calls").unwrap(), "[1] 0\n");
    // Each oversized attempt admits several 10,000-element scratch vectors
    // before refusing its matrix. Repeating the attempt would exhaust the
    // original cap and disrupt smaller fits if those vectors were retained.
    for _ in 0..12 {
        expect_error(&mut session, oversized, "transient memory");
        expect_error(&mut session, failing_callback, "objective failed");
    }
    assert_eq!(
        session.eval(&format!("({fit})$convergence")).unwrap(),
        "[1] 0\n"
    );
    assert_eq!(
        session.resource_limits().max_alloc_bytes,
        limits.max_alloc_bytes
    );
}

#[test]
fn bounded_optimizer_callbacks_can_start_their_own_fit() {
    let mut session = session();
    let result = session
        .eval(
            "inner.value <- NA_real_
         target <- 1:3
         f <- function(x) {
           if (is.na(inner.value)) {
             inner <- optim(c(6,7), function(y) sum((y-1)^2), function(y) 2*(y-1),
                            method = 'L-BFGS-B', lower = c(2, 0))
             inner.value <<- inner$value
           }
           sum((x - target)^2)
         }
         fit <- optim(c(5, 6, 7), f, function(x) 2*(x-target), method = 'L-BFGS-B',
                      lower = c(0, 0, 4))
         stopifnot(fit$convergence == 0L, abs(inner.value - 1) < 1e-8,
                   max(abs(fit$par - c(1, 2, 4))) < 1e-6)
         cat('both fits converged')",
        )
        .unwrap();
    assert_eq!(result, "both fits converged");
}
