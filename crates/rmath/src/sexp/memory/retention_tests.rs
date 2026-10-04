//! Exact heap accounting; RSS is observational and never an acceptance predicate.
#![forbid(unsafe_code)]

use crate::sexp::{RSession, SEXPTYPE, Sexp, SexpMut};
use std::rc::Rc;

#[cfg(not(miri))]
fn resident_kib() -> Option<usize> {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    std::str::from_utf8(&output.stdout)
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(miri)]
fn resident_kib() -> Option<usize> {
    None
}

fn real(session: &RSession, len: usize) -> Sexp<'static> {
    session
        .owner_token()
        .unwrap()
        .node_factory()
        .allocate(|arena| {
            arena
                .alloc_vector_sexp(SEXPTYPE::REALSXP, len as i64)
                .map(|value| value.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap()
}

#[test]
fn retained_small_value_does_not_keep_collected_temporary_payloads() {
    let mut session = RSession::new_for_gc_tests();
    let mut small = SexpMut::try_from_checked(real(&session, 1)).unwrap();
    small.try_set_real_elt(0, 37.).unwrap();
    let small = small.freeze();
    let small_node = small.allocation().unwrap().clone();
    let (backing, bytes) = session
        .with_arena(|arena| (Rc::downgrade(&arena.backing), arena.allocated_bytes.clone()))
        .unwrap();
    session.owner_token().unwrap().full_gc().unwrap();
    let baseline_bytes = bytes.get();
    let baseline_nodes = session.with_arena(|arena| arena.node_count()).unwrap();
    let rss_before = resident_kib();

    // Native: 64 MiB across a 257-node graph. Miri executes the same ownership
    // transitions with small buffers, independently of process-memory tooling.
    let count = if cfg!(miri) { 4 } else { 256 };
    let width = if cfg!(miri) { 64 } else { 32_768 };
    let graph = {
        let factory = session.owner_token().unwrap().node_factory();
        let graph = factory
            .allocate(|arena| {
                arena
                    .alloc_vector_sexp(SEXPTYPE::VECSXP, count as i64)
                    .map(|value| value.as_raw())
            })
            .unwrap()
            .into_owned()
            .unwrap();
        let mut graph = SexpMut::try_from_checked(graph).unwrap();
        for index in 0..count {
            let mut child = SexpMut::try_from_checked(real(&session, width)).unwrap();
            // Commit each native page so the observed RSS includes real payload
            // residency. No detached payload lease or extra root survives here.
            for element in (0..width).step_by(if cfg!(miri) { 8 } else { 512 }) {
                child.try_set_real_elt(element as i64, 19.).unwrap();
            }
            graph
                .try_set_vector_elt(index as i64, child.freeze())
                .unwrap();
        }
        graph.freeze()
    };
    let graph_node = graph.allocation().unwrap().clone();
    let payload_bytes = count * width * size_of::<f64>();
    let peak_bytes = bytes.get();
    assert!(peak_bytes >= baseline_bytes + payload_bytes);
    let rss_peak = resident_kib();
    drop(graph);
    // Checked identities are deliberately nonowning; only `small` remains a
    // value root from this fixture while the original collector runs.
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(!graph_node.is_live());
    assert!(small_node.is_live());
    assert_eq!(small.try_real_elt(0).unwrap(), 37.);
    let after_gc_bytes = bytes.get();
    let after_gc_nodes = session.with_arena(|arena| arena.node_count()).unwrap();
    assert_eq!(after_gc_nodes, baseline_nodes);
    assert_eq!(
        after_gc_bytes,
        baseline_bytes + (count + 1) * super::NODE_BYTES,
        "only reusable node headers may remain accounted"
    );
    let rss_after_gc = resident_kib();

    session.close();
    drop(session);
    assert!(backing.upgrade().is_some());
    assert!(small_node.is_live());
    let after_session_bytes = bytes.get();
    let rss_after_session = resident_kib();
    drop(small);
    assert!(backing.upgrade().is_none());
    assert!(!small_node.is_live());
    assert_eq!(bytes.get(), 0);
    let rss_after_release = resident_kib();
    println!(
        "retention {{\"profile\":\"minimal-managed-gc\",\"payload_bytes\":{payload_bytes},\"baseline_nodes\":{baseline_nodes},\"after_gc_nodes\":{after_gc_nodes},\"baseline_bytes\":{baseline_bytes},\"peak_bytes\":{peak_bytes},\"after_gc_bytes\":{after_gc_bytes},\"after_session_bytes\":{after_session_bytes},\"after_release_bytes\":{},\"rss_kib\":[{rss_before:?},{rss_peak:?},{rss_after_gc:?},{rss_after_session:?},{rss_after_release:?}]}}",
        bytes.get()
    );
}

#[test]
fn repeated_sessions_release_every_backing_after_last_owning_value() {
    for _ in 0..8 {
        let mut session = RSession::new_for_gc_tests();
        let value = real(&session, 128);
        let node = value.allocation().unwrap().clone();
        let (backing, bytes) = session
            .with_arena(|arena| (Rc::downgrade(&arena.backing), arena.allocated_bytes.clone()))
            .unwrap();
        session.owner_token().unwrap().full_gc().unwrap();
        session.close();
        drop(session);
        assert!(backing.upgrade().is_some());
        assert!(node.is_live());
        assert!(bytes.get() >= 128 * size_of::<f64>());
        drop(value);
        assert!(backing.upgrade().is_none());
        assert!(!node.is_live());
        assert_eq!(bytes.get(), 0);
    }
}

#[test]
fn handle_cloning_and_independent_root_churn_release_exact_leases() {
    let session = RSession::new_for_gc_tests();
    let value = real(&session, 1);
    let node = value.allocation().unwrap().clone();
    let baseline_roots = node.root_count();
    assert_eq!(baseline_roots, 1);
    let factory = session.owner_token().unwrap().node_factory();
    let count = if cfg!(miri) { 8 } else { 10_000 };
    let mut cloning_ns = Vec::new();
    let mut wrapping_ns = Vec::new();
    for _ in 0..if cfg!(miri) { 1 } else { 3 } {
        let started = std::time::Instant::now();
        let handles: Vec<_> = (0..count).map(|_| value.clone()).collect();
        cloning_ns.push(started.elapsed().as_nanos());
        // Clones share one root lease; they do not add collector work or nodes.
        assert_eq!(node.root_count(), baseline_roots);
        drop(handles);
        assert_eq!(node.root_count(), baseline_roots);

        let started = std::time::Instant::now();
        let handles: Vec<_> = (0..count)
            .map(|_| factory.wrap(value.as_raw()).unwrap().into_owned().unwrap())
            .collect();
        wrapping_ns.push(started.elapsed().as_nanos());
        assert_eq!(node.root_count(), baseline_roots + count);
        session.owner_token().unwrap().full_gc().unwrap();
        assert!(node.is_live());
        drop(handles);
        assert_eq!(node.root_count(), baseline_roots);
    }
    cloning_ns.sort_unstable();
    wrapping_ns.sort_unstable();
    println!(
        "root_churn {{\"profile\":\"minimal-managed-gc\",\"count\":{count},\"node_bytes\":{},\"handle_bytes\":{},\"median_clone_ns\":{},\"median_independent_wrap_ns\":{}}}",
        super::NODE_BYTES,
        size_of::<Sexp<'static>>(),
        cloning_ns[cloning_ns.len() / 2],
        wrapping_ns[wrapping_ns.len() / 2]
    );
    drop(value);
    assert_eq!(node.root_count(), 0);
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(!node.is_live());
}
