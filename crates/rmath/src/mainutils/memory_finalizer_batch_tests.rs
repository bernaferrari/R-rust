use super::*;
use crate::sexp::{
    gengc::full_gc, heap::CheckedNode, instance, memory::with_arena, protect::ProtectGuard,
    session::RSession,
};
use std::cell::{Cell, RefCell};

thread_local! {
    static RUNS: Cell<usize> = const { Cell::new(0) };
    static LOST_GRAPH: Cell<bool> = const { Cell::new(false) };
    static LOST_RUNNING_GUARD: Cell<bool> = const { Cell::new(false) };
    static GRAPH_NODES: RefCell<Vec<CheckedNode>> = const { RefCell::new(Vec::new()) };
    static OWNER_TO_DROP: RefCell<Option<RSession>> = const { RefCell::new(None) };
}

fn reset_observations() {
    RUNS.set(0);
    LOST_GRAPH.set(false);
    LOST_RUNNING_GUARD.set(false);
    GRAPH_NODES.with(|nodes| nodes.borrow_mut().clear());
}

fn graph() -> (SEXP, ProtectGuard<'static>) {
    // SAFETY: the active minimal fixture owns these nodes. Root the key before
    // ending the allocation lend, including any deferred collection boundary.
    unsafe {
        with_arena(|arena| {
            let key = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
            let child = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
            let value = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            *crate::sexp::accessors::INTEGER(value) = 73;
            arena.set_reference_element(child, 0, value).unwrap();
            crate::mainutils::memory_main::R_SetExternalPtrProtected(key, child);
            crate::mainutils::memory_main::R_SetExternalPtrTag(key, R_NilValue());
            GRAPH_NODES.with(|nodes| {
                nodes
                    .borrow_mut()
                    .extend([key, child, value].map(|node| arena.node_token(node).unwrap()));
            });
            (key, crate::sexp::protect::protect(key))
        })
    }
}

unsafe extern "C" fn collect_and_check_batch(_: *mut c_void) {
    let run = RUNS.get();
    RUNS.set(run + 1);
    if run == 0 {
        full_gc();
    }
    // Check metadata only. A broken runner may already have freed its own
    // argument; the red regression must detect this without dereferencing it.
    let lost = GRAPH_NODES.with(|nodes| nodes.borrow().iter().any(|node| !node.is_live()));
    LOST_GRAPH.set(LOST_GRAPH.get() || lost);
}

unsafe extern "C" fn count_only(_: *mut c_void) {
    RUNS.set(RUNS.get() + 1);
}

unsafe extern "C" fn nested_runner(_: *mut c_void) {
    RUNS.set(RUNS.get() + 1);
    // SAFETY: the outer invocation owns the active instance and is running.
    unsafe { R_RunPendingFinalizers() };
    LOST_RUNNING_GUARD.set(!with_memory_state(|state| state.running_finalizers));
}

unsafe extern "C" fn destroy_owner(_: *mut c_void) {
    RUNS.set(RUNS.get() + 1);
    let owner = OWNER_TO_DROP.with(|slot| slot.borrow_mut().take());
    drop(owner);
}

#[test]
fn ready_finalizer_batch_retains_current_and_later_transitive_graphs_during_gc() {
    let session = RSession::new_for_gc_tests();
    reset_observations();
    session.with_active(|| {
        let (first, first_root) = graph();
        let (second, second_root) = graph();
        unsafe {
            R_RegisterCFinalizer(first, collect_and_check_batch);
            R_RegisterCFinalizer(second, collect_and_check_batch);
        }
        drop(first_root);
        drop(second_root);
        full_gc();
        assert!(with_memory_state(|state| state
            .pending_finalizers
            .iter()
            .all(PendingFinalizer::is_ready)));
        unsafe { R_RunPendingFinalizers() };
        assert_eq!(RUNS.get(), 2);
        assert!(
            !LOST_GRAPH.get(),
            "ready batch graph reclaimed during callback collection"
        );
        assert!(with_memory_state(|state| state
            .pending_finalizers
            .is_empty()));
        full_gc();
        assert!(GRAPH_NODES.with(|nodes| nodes.borrow().iter().all(|node| !node.is_live())));
    });
    reset_observations();
}

#[test]
fn ready_finalizer_batch_roots_later_r_function_before_first_c_callback() {
    let session = RSession::new_for_gc_tests();
    reset_observations();
    session.with_active(|| {
        let (first, first_root) = graph();
        let (second, second_root) = graph();
        // SAFETY: construct a real one-argument closure under a single lend.
        // Its body is nil, requiring no base package bootstrap to evaluate.
        let (function, function_root) = unsafe {
            with_arena(|arena| {
                let nil = R_NilValue();
                let formal = arena.cons(
                    crate::sexp::globals::R_MissingArg(),
                    nil,
                    crate::sexp::symbol::Rf_install(c"key".as_ptr()),
                );
                let function = arena.alloc_node(SEXPTYPE::CLOSXP);
                crate::sexp::accessors::SET_FORMALS(function, formal);
                crate::sexp::accessors::SET_BODY(function, nil);
                crate::sexp::accessors::SET_CLOENV(function, R_GlobalEnv());
                GRAPH_NODES.with(|nodes| {
                    nodes
                        .borrow_mut()
                        .extend([formal, function].map(|node| arena.node_token(node).unwrap()));
                });
                (function, crate::sexp::protect::protect(function))
            })
        };
        unsafe {
            R_RegisterCFinalizer(first, collect_and_check_batch);
            R_RegisterFinalizer(second, function);
        }
        drop(first_root);
        drop(second_root);
        drop(function_root);
        full_gc();
        unsafe { R_RunPendingFinalizers() };
        assert_eq!(RUNS.get(), 1);
        assert!(
            !LOST_GRAPH.get(),
            "later R finalizer function reclaimed by earlier callback"
        );
        assert!(with_memory_state(|state| state
            .pending_finalizers
            .is_empty()));
    });
    reset_observations();
}

#[test]
fn real_r_finalizer_call_survives_torture_before_evaluation() {
    let session = RSession::new_for_gc_tests();
    reset_observations();
    session.with_active(|| unsafe {
        let (key, key_root) = graph();
        let assignment =
            crate::eval::primitive::make_primitive_binding("<<-", SEXPTYPE::SPECIALSXP);
        let assignment_root = crate::sexp::protect::protect(assignment);
        let argument = crate::sexp::symbol::Rf_install(c"key".as_ptr());
        let observation = crate::sexp::symbol::Rf_install(c"finalizer_observation".as_ptr());
        // A real R closure writes its argument into the global environment.
        // A swallowed evaluator error cannot masquerade as successful dispatch.
        let (function, function_root) = with_arena(|arena| {
            let nil = R_NilValue();
            let formal = arena.cons(crate::sexp::globals::R_MissingArg(), nil, argument);
            let rhs = arena.cons(argument, nil, nil);
            let lhs = arena.cons(observation, rhs, nil);
            let body = arena.cons(assignment, lhs, nil);
            crate::sexp::accessors::SET_TYPEOF(body, SEXPTYPE::LANGSXP.as_c_int());
            let function = arena.alloc_node(SEXPTYPE::CLOSXP);
            crate::sexp::accessors::SET_FORMALS(function, formal);
            crate::sexp::accessors::SET_BODY(function, body);
            crate::sexp::accessors::SET_CLOENV(function, R_GlobalEnv());
            (function, crate::sexp::protect::protect(function))
        });
        R_RegisterFinalizer(key, function);
        drop(key_root);
        drop(function_root);
        drop(assignment_root);
        full_gc();
        let collections = crate::sexp::gengc::get_gc_stats().collections;
        // Isolate the call-construction boundary: the first forced collection
        // disarms torture before unrelated evaluator allocations begin.
        crate::sexp::gengc::register_gc_callback(Box::new(|_| {
            with_memory_state(|state| {
                state.gc_force_gap = 0;
                state.gc_force_wait = 0;
            });
            RUNS.set(RUNS.get() + 1);
        }));
        R_gc_torture(1, 1, 0);
        R_RunPendingFinalizers();
        assert_eq!(RUNS.get(), 1, "call construction did not force collection");
        assert_eq!(
            crate::sexp::gengc::get_gc_stats().collections,
            collections + 1
        );
        assert_eq!(
            session
                .eval(observation)
                .expect("R finalizer body did not execute"),
            key
        );
        assert!(with_memory_state(|state| state
            .pending_finalizers
            .is_empty()));
        assert!(GRAPH_NODES.with(|nodes| nodes.borrow().iter().all(CheckedNode::is_live)));
    });
    reset_observations();
}

#[test]
fn nested_pending_runner_keeps_outer_running_flag_set() {
    let session = RSession::new_for_gc_tests();
    reset_observations();
    session.with_active(|| {
        let (key, root) = graph();
        unsafe { R_RegisterCFinalizer(key, nested_runner) };
        drop(root);
        full_gc();
        unsafe { R_RunPendingFinalizers() };
        assert_eq!(RUNS.get(), 1);
        assert!(
            !LOST_RUNNING_GUARD.get(),
            "nested no-op cleared outer running flag"
        );
        assert!(!with_memory_state(|state| state.running_finalizers));
    });
    reset_observations();
}

#[test]
fn ready_batch_root_claim_failure_keeps_queue_and_releases_partial_roots() {
    let session = RSession::new_for_gc_tests();
    reset_observations();
    session.with_active(|| {
        let (first, first_root) = graph();
        let (second, second_root) = graph();
        unsafe {
            R_RegisterCFinalizer(first, count_only);
            R_RegisterCFinalizer(second, count_only);
        }
        drop(first_root);
        drop(second_root);
        full_gc();
        let owner = session.owner_token().unwrap().as_ptr();
        let (checkpoint, root_count) = unsafe {
            let table = &(*owner).root_table;
            let checkpoint = table.checkpoint();
            let roots = table.checked_entries_snapshot().len();
            // First claim succeeds; the second must fail without publishing.
            table.set_next_generation_for_test(u64::MAX - 1);
            (checkpoint, roots)
        };
        let failure = std::panic::catch_unwind(|| unsafe { R_RunPendingFinalizers() });
        unsafe { (*owner).root_table.set_next_generation_for_test(checkpoint) };
        assert!(
            failure.is_err(),
            "runner did not claim roots for its ready batch"
        );
        assert_eq!(RUNS.get(), 0);
        assert!(!with_memory_state(|state| state.running_finalizers));
        assert_eq!(with_memory_state(|state| state.pending_finalizers.len()), 2);
        assert_eq!(
            unsafe { (*owner).root_table.checked_entries_snapshot().len() },
            root_count
        );
        full_gc();
        assert!(GRAPH_NODES.with(|nodes| nodes.borrow().iter().all(CheckedNode::is_live)));
        unsafe { R_RunPendingFinalizers() };
        assert_eq!(RUNS.get(), 2, "retry lost or duplicated a queued finalizer");
    });
    reset_observations();
}

#[test]
fn ready_batch_stops_after_callback_destroys_its_original_owner() {
    let session = RSession::new_for_gc_tests();
    reset_observations();
    let (first, first_root) = graph();
    let (second, second_root) = graph();
    unsafe {
        R_RegisterCFinalizer(first, destroy_owner);
        R_RegisterCFinalizer(second, count_only);
    }
    drop(first_root);
    drop(second_root);
    full_gc();
    OWNER_TO_DROP.with(|slot| {
        *slot.borrow_mut() = Some(session);
    });
    let result = std::panic::catch_unwind(|| unsafe { R_RunPendingFinalizers() });
    assert!(
        result.is_ok(),
        "runner accessed ambient state after owner teardown"
    );
    assert_eq!(
        RUNS.get(),
        1,
        "callback dispatched after original owner was destroyed"
    );
    assert!(instance::with_current_instance(|owner| owner).is_none());
    assert!(GRAPH_NODES.with(|nodes| nodes.borrow().iter().all(|node| !node.is_live())));
    reset_observations();
}
