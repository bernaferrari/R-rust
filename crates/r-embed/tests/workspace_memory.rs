use r_embed::{RSession, RuntimePathPolicy};

fn session() -> RSession {
    RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap()
}

#[test]
fn native_workspace_reservations_share_limits_resize_atomically_and_release_on_unwind() {
    let mut session = session();
    let before = session.memory_usage().unwrap();
    let mut limits = session.resource_limits();
    limits.max_alloc_bytes = before.total_bytes + 4096;
    session.set_resource_limits(limits).unwrap();
    let mut first = session.reserve_workspace(2048).unwrap();
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes + 2048
    );
    assert!(session.reserve_workspace(2049).is_err());
    first.try_resize(4096).unwrap();
    assert!(first.try_resize(4097).is_err());
    assert_eq!(first.bytes(), 4096);
    assert!(first.try_resize(usize::MAX).is_err());
    assert_eq!(first.bytes(), 4096);
    limits.max_alloc_bytes = before.total_bytes + 1024;
    session.set_resource_limits(limits).unwrap();
    assert!(first.try_resize(4097).is_err());
    first.try_resize(512).unwrap();
    drop(first);
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _workspace = session.reserve_workspace(1024).unwrap();
        panic!("workspace cleanup probe");
    }));
    assert!(result.is_err());
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
    let mut retained = session.reserve_workspace(100).unwrap();
    session.close();
    assert!(retained.try_resize(101).is_err());
    drop(retained);
}

#[test]
fn canvas_admission_happens_before_allocation_and_evaluation_and_recovers() {
    let mut session = session();
    let before = session.memory_usage().unwrap();
    let mut limits = session.resource_limits();
    limits.max_alloc_bytes = before.total_bytes + 4096;
    session.set_resource_limits(limits).unwrap();
    let error = session
        .render_with_dimensions("canvas_side_effect <- TRUE; plot(1)", 1024, 1024)
        .unwrap_err();
    assert!(error.to_string().contains("workspace"), "{error}");
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
    limits.max_alloc_bytes = 0;
    session.set_resource_limits(limits).unwrap();
    assert_eq!(
        session
            .eval("exists('canvas_side_effect', inherits=FALSE)")
            .unwrap(),
        "[1] FALSE\n"
    );
    let png = session
        .render_with_dimensions("plot(1:3)", 100, 100)
        .unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
    session.eval_interactive("plot(1:3)", 100, 100).unwrap();
    assert!(session.memory_usage().unwrap().workspace_bytes > before.workspace_bytes);
    session.eval_interactive("lines(3:1)", 100, 100).unwrap();
}

#[test]
fn garbage_collected_nodes_are_reusable_while_live_node_cap_stays_enforced() {
    let mut session = session();
    let mut limits = session.resource_limits();
    limits.max_arena_nodes = session.arena_stats().active_nodes + 5000;
    session.set_resource_limits(limits).unwrap();
    // Temporary objects are explicitly collected; their work must not poison
    // subsequent host requests while the live-node cap stays unchanged.
    for _ in 0..20 {
        assert_eq!(
            session
                .eval("local({for(i in 1:30) new.env(); gc(); TRUE})")
                .unwrap(),
            "[1] TRUE\n"
        );
    }
    assert!(session.eval("as.pairlist(rep(1L, 10000))").is_err());
    assert_eq!(
        session.resource_limits().max_arena_nodes,
        limits.max_arena_nodes
    );
}

#[test]
fn unserialize_copy_and_decoded_value_compete_for_one_memory_budget_and_recover() {
    let mut session = session();
    session
        .eval("payload <- serialize(integer(20000L), NULL); gc(); invisible(NULL)")
        .unwrap();
    let before = session.memory_usage().unwrap();
    let mut limits = session.resource_limits();
    limits.max_alloc_bytes = before.total_bytes + 100_000;
    session.set_resource_limits(limits).unwrap();
    assert!(
        session.eval("invisible(unserialize(payload))").is_err(),
        "raw input copy and decoded vector passed independent byte limits"
    );
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
    limits.max_alloc_bytes = 0;
    session.set_resource_limits(limits).unwrap();
    assert_eq!(
        session.eval("length(unserialize(payload))").unwrap(),
        "[1] 20000\n"
    );
}

#[test]
fn serialize_native_output_and_final_raw_value_share_admission_and_recover() {
    let mut session = session();
    session
        .eval("payload <- raw(80000L); gc(); invisible(NULL)")
        .unwrap();
    let before = session.memory_usage().unwrap();
    let mut limits = session.resource_limits();
    limits.max_alloc_bytes = before.total_bytes + 205_000;
    session.set_resource_limits(limits).unwrap();
    assert!(
        session.eval("invisible(serialize(payload, NULL))").is_err(),
        "native writer storage and final R raw vector escaped aggregate admission"
    );
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
    limits.max_alloc_bytes = 0;
    session.set_resource_limits(limits).unwrap();
    assert_eq!(
        session
            .eval("identical(unserialize(serialize(payload,NULL)), payload)")
            .unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
}
