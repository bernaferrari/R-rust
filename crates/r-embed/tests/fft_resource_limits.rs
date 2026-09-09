use r_embed::{RResourceLimits, RSession};

#[test]
fn prime_fft_observes_time_limit_and_session_recovers() {
    let mut session = RSession::new().unwrap();
    session.eval("x<-rep(1,100003L)").unwrap();
    session
        .set_resource_limits(RResourceLimits {
            max_eval_depth: 500,
            max_execution_time_ms: 1,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        })
        .unwrap();
    let error = session.eval("fft(x)").unwrap_err();
    assert!(error.to_string().contains("time limit"), "{error}");
    session
        .set_resource_limits(RResourceLimits {
            max_eval_depth: 500,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        })
        .unwrap();
    assert_eq!(
        session
            .eval("identical(Re(fft(c(1,0,0,0))),rep(1,4))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
