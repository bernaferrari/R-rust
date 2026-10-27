use r_embed::{RSession, RuntimePathPolicy};

fn session() -> RSession {
    RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap()
}

const FORMAT: &str = "invisible(format(rep(1+2i,1024L)))";

#[test]
fn repeated_complex_format_releases_primitive_transient_workspace() {
    let mut session = session();
    session.eval(FORMAT).unwrap();
    let before = session.memory_usage().unwrap().workspace_bytes;
    for _ in 0..8 {
        session.eval(FORMAT).unwrap();
    }
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before,
        "completed formatting primitives must release their C-stack-style R_alloc buffers"
    );
}

#[test]
fn nested_complex_format_unwind_releases_only_its_transient_workspace() {
    let mut session = session();
    session.eval(FORMAT).unwrap();
    let error = session
        .eval("stop('transient-workspace-marker',call.=FALSE)")
        .unwrap_err();
    assert!(error.to_string().contains("transient-workspace-marker"));
    let before = session.memory_usage().unwrap().workspace_bytes;
    let error = session
        .eval("local({format(rep(1+2i,1024L));stop('transient-workspace-marker',call.=FALSE)})")
        .unwrap_err();
    assert!(error.to_string().contains("transient-workspace-marker"));
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before,
        "unwinding the enclosing primitive must release the nested formatting scratch"
    );
    assert_eq!(session.eval("1+1").unwrap(), "[1] 2\n");
}
