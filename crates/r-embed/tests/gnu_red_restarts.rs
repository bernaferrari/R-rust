// Beads: rport-rm1k; broader restart fidelity: rport-dl8y.
use r_embed::RSession;

// Expectations in this file come from the pinned GNU R oracle
// bac583951b728e97b9786804d3b4081f0fe18df5.

#[test]
fn compute_restarts_includes_implicit_abort() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("withRestarts(length(computeRestarts()), foo=function() 1)")
        .unwrap();
    // GNU R reports the explicit foo restart plus implicit abort.
    assert_eq!(result.trim(), "[1] 2");
}

#[test]
fn find_restart_exposes_implicit_abort() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("c(!is.null(findRestart('abort')), isRestart(findRestart('abort')))")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE");
}

#[test]
fn explicit_restart_has_exit_environment() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("withRestarts(is.environment(findRestart('foo')$exit), foo=function() 1)")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}
