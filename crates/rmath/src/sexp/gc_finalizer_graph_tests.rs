use super::*;
use crate::sexp::{
    accessors::{CAR, SET_VECTOR_ELT, SETCDR, VECTOR_ELT},
    ffi::SEXP,
    memory::with_arena,
    protect::ProtectGuard,
    session::RSession,
};
use std::{cell::Cell, ffi::c_void};

thread_local! {
    static FINALIZER_RUNS: Cell<usize> = const { Cell::new(0) };
    static RESURRECTION_SYMBOL: Cell<SEXP> = const { Cell::new(std::ptr::null_mut()) };
}

unsafe extern "C" fn count_finalizer(_: *mut c_void) {
    FINALIZER_RUNS.set(FINALIZER_RUNS.get() + 1);
}

struct Graph {
    key: SEXP,
    child: SEXP,
    value: SEXP,
    cycle: SEXP,
}

fn graph() -> (Graph, ProtectGuard<'static>) {
    unsafe {
        with_arena(|arena| {
            let nil = crate::sexp::globals::R_NilValue();
            let key = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
            let child = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
            let value = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            *crate::sexp::accessors::INTEGER(value) = 73;
            let cycle = arena.cons(value, nil, nil);
            SETCDR(cycle, cycle);
            SET_VECTOR_ELT(child, 0, value);
            SET_VECTOR_ELT(child, 1, cycle);
            (*key).data.extptr_mut()[1] = child.cast();
            (*key).data.extptr_mut()[2] = cycle.cast();
            // Root before ending the lend, including any deferred callbacks.
            let root = crate::sexp::protect::protect(key);
            (
                Graph {
                    key,
                    child,
                    value,
                    cycle,
                },
                root,
            )
        })
    }
}

fn assert_graph_live(session: &RSession, graph: &Graph) {
    for node in [graph.key, graph.child, graph.value, graph.cycle] {
        assert!(
            session.sexp(node).is_some(),
            "finalizer graph node was reclaimed"
        );
    }
    for node in [graph.key, graph.child, graph.value, graph.cycle] {
        assert!(
            crate::sexp::memory::arena_node_marked(node),
            "retained graph was not marked in this collection"
        );
    }
    unsafe {
        assert_eq!(
            crate::mainutils::memory_main::R_ExternalPtrProtected(graph.key),
            graph.child
        );
        assert_eq!(VECTOR_ELT(graph.child, 0), graph.value);
        assert_eq!(VECTOR_ELT(graph.child, 1), graph.cycle);
        assert_eq!(CAR(graph.cycle), graph.value);
        assert_eq!(crate::sexp::accessors::CDR(graph.cycle), graph.cycle);
    }
    assert_eq!(session.sexp(graph.value).unwrap().integer_elt(0), Some(73));
}

enum Collection {
    Full,
    Minor,
    Torture,
}
fn collect(collection: &Collection, session: &RSession) -> (usize, usize) {
    match collection {
        Collection::Full => full_gc(),
        Collection::Minor => minor_gc(),
        Collection::Torture => run_gc_cycle_in(
            session.owner_token().unwrap().as_ptr(),
            do_torture_mark_sweep_in,
        ),
    }
}

fn discovery(collection: Collection) {
    let session = RSession::new_for_gc_tests();
    let (graph, root) = graph();
    FINALIZER_RUNS.set(0);
    unsafe { crate::mainutils::memory_main::R_RegisterCFinalizerEx(graph.key, count_finalizer, 0) };
    if matches!(collection, Collection::Torture) {
        full_gc();
    }
    drop(root);
    let (promoted, _) = collect(&collection, &session);
    assert_eq!(FINALIZER_RUNS.get(), 0, "discovery should defer execution");
    assert_graph_live(&session, &graph);
    if !matches!(collection, Collection::Torture) {
        assert!(
            promoted >= 4,
            "newly retained graph was not included in promotion counts"
        );
    }
    for node in [graph.key, graph.child, graph.value, graph.cycle] {
        assert_eq!(
            session.sexp(node).unwrap().header().sxpinfo.gcgen(),
            Generation::Old as u8,
        );
    }
    full_gc();
    assert_graph_live(&session, &graph);
    unsafe { crate::mainutils::memory_main::R_RunPendingFinalizers() };
    assert_eq!(FINALIZER_RUNS.get(), 1);
    unsafe { crate::mainutils::memory_main::R_RunPendingFinalizers() };
    assert_eq!(FINALIZER_RUNS.get(), 1);
    full_gc();
    for node in [graph.key, graph.child, graph.value, graph.cycle] {
        assert!(
            session.sexp(node).is_none(),
            "finalized unreachable cycle leaked"
        );
    }
}

#[test]
fn newly_ready_finalizer_graph_survives_full_collection() {
    discovery(Collection::Full);
}
#[test]
fn newly_ready_finalizer_graph_survives_minor_collection() {
    discovery(Collection::Minor);
}
#[test]
fn newly_ready_finalizer_graph_survives_torture_collection() {
    discovery(Collection::Torture);
}

#[test]
fn newly_ready_finalizer_can_resurrect_its_transitive_child_graph() {
    unsafe extern "C" fn resurrect(pointer: *mut c_void) {
        let key = pointer.cast();
        let _root = unsafe { crate::sexp::protect::protect(key) };
        let child = unsafe { crate::mainutils::memory_main::R_ExternalPtrProtected(key) };
        let symbol = RESURRECTION_SYMBOL.get();
        unsafe {
            crate::sexp::envir::defineVar(symbol, child, crate::sexp::globals::R_GlobalEnv())
        };
        FINALIZER_RUNS.set(FINALIZER_RUNS.get() + 1);
    }
    let session = RSession::new_for_gc_tests();
    RESURRECTION_SYMBOL
        .set(unsafe { crate::sexp::symbol::Rf_install(c"resurrected_graph".as_ptr()) });
    let (graph, root) = graph();
    FINALIZER_RUNS.set(0);
    unsafe { crate::mainutils::memory_main::R_RegisterCFinalizerEx(graph.key, resurrect, 0) };
    drop(root);
    full_gc();
    assert_graph_live(&session, &graph);
    unsafe { crate::mainutils::memory_main::R_RunPendingFinalizers() };
    full_gc();
    assert_eq!(FINALIZER_RUNS.get(), 1);
    assert!(session.sexp(graph.key).is_none());
    let child = session.sexp(graph.child).unwrap();
    assert_eq!(child.vector_elt(0).unwrap().integer_elt(0), Some(73));
    assert_eq!(
        child.vector_elt(1).unwrap().car().unwrap().integer_elt(0),
        Some(73)
    );
    unsafe {
        crate::sexp::envir::defineVar(
            RESURRECTION_SYMBOL.get(),
            crate::sexp::globals::R_NilValue(),
            crate::sexp::globals::R_GlobalEnv(),
        );
    }
    drop(child);
    full_gc();
    assert!(session.sexp(graph.child).is_none());
    assert!(session.sexp(graph.cycle).is_none());
}
