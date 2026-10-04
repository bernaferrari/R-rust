//! Focused translated accessor contracts and an explicit debug-profile timing gate.

use super::*;
use crate::sexp::memory::RArena;
use crate::sexp::session::RSession;
use std::hint::black_box;
use std::time::Instant;

#[test]
#[ignore = "explicit native before/after timing gate; not a correctness test"]
fn translated_header_access_microbenchmark() {
    const ITERATIONS: usize = 20_000;
    const SAMPLES: usize = 7;
    let session = RSession::new_for_gc_tests();
    let mut first = RArena::new();
    let mut second = RArena::new();
    let left = first.alloc_node(SEXPTYPE::LISTSXP);
    let neighbor = first.alloc_node(SEXPTYPE::LISTSXP);
    let distant = second.alloc_node(SEXPTYPE::LISTSXP);
    session.with_active(|| {
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        for (name, pointers) in [
            ("same-page", [left, neighbor]),
            ("different-page", [left, distant]),
            ("singleton", [nil, nil]),
        ] {
            let mut nanos = Vec::with_capacity(SAMPLES);
            for _ in 0..SAMPLES {
                let start = Instant::now();
                for index in 0..ITERATIONS {
                    let pointer = black_box(pointers[index % pointers.len()]);
                    // SAFETY: both arena facades and the original managed
                    // singleton bank stay alive; no callback or mutation runs.
                    unsafe {
                        black_box(TYPEOF(pointer));
                        black_box(CAR(pointer));
                        black_box(CDR(pointer));
                        black_box(TAG(pointer));
                    }
                }
                nanos.push(start.elapsed().as_nanos());
            }
            nanos.sort_unstable();
            println!(
                "header-access {name}: iterations={ITERATIONS} samples={SAMPLES} median-ns={} ns-per-four-reads={:.2}",
                nanos[SAMPLES / 2],
                nanos[SAMPLES / 2] as f64 / ITERATIONS as f64,
            );
        }
    });
}

#[test]
fn owned_checked_header_reads_use_the_original_parent_domain() {
    let session = RSession::new_for_gc_tests();
    let mut first = RArena::new();
    let mut second = RArena::new();
    let parent = first.alloc_node(SEXPTYPE::LISTSXP);
    let child = first.alloc_vector(SEXPTYPE::INTSXP, 1);
    let foreign = second.alloc_vector(SEXPTYPE::INTSXP, 1);
    // SAFETY: both facades own their exact registered slots and no callback
    // or payload loan crosses these initialized fixture writes.
    unsafe {
        SET_INTEGER_ELT(child, 0, 42);
        SET_INTEGER_ELT(foreign, 0, 77);
        SETCAR(parent, child);
    }
    let input = ptr::without_provenance_mut::<SexprecCore>(parent.addr());
    session.with_active(|| unsafe {
        assert_eq!(TYPEOF(input), SEXPTYPE::LISTSXP.0);
        assert_eq!(CAR(input), child);
        assert_eq!(INTEGER_ELT(CAR(input), 0), 42);
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SETCAR(input, foreign);
        }));
        assert!(rejected.is_err());
        assert_eq!(CAR(input), child);
    });
}

#[test]
fn owned_checked_header_reads_reject_stale_graph_generations() {
    let mut arena = RArena::new();
    let parent = arena.alloc_node(SEXPTYPE::LISTSXP);
    let child = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
    let original = arena.node_token(child).unwrap();
    // SAFETY: fixture owns the graph and intentionally invalidates the child
    // at the allocator seam; no checked read is made through the old payload.
    unsafe {
        SETCAR(parent, child);
        arena.free_node(child);
    }
    let reused = arena.alloc_node(SEXPTYPE::REALSXP);
    assert_eq!(reused, child, "exercise an actual reused physical slot");
    assert!(!original.is_live());
    assert!(crate::sexp::memory::checked_snapshot(reused, &original).is_none());
    assert_eq!(unsafe { TYPEOF(reused) }, SEXPTYPE::REALSXP.0);
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        CAR(parent);
    }));
    assert!(
        rejected.is_err(),
        "saved graph identity must not adopt slot reuse"
    );
}

#[test]
fn owned_checked_header_reads_retain_the_original_singleton_bank() {
    let session = RSession::new_for_gc_tests();
    let factory = crate::sexp::object::SessionNodeFactory::new(session.owner_token().unwrap());
    let original = factory.domain().nil();
    crate::sexp::globals::close_immutable_singletons_for_test();
    let current = unsafe { crate::sexp::globals::R_NilValue() };
    assert_ne!(original.as_raw(), current);
    let input = ptr::without_provenance_mut::<SexprecCore>(original.as_raw().addr());
    session.with_active(|| unsafe {
        assert_eq!(TYPEOF(input), SEXPTYPE::NILSXP.0);
        assert_eq!(CAR(input).addr(), original.as_raw().addr());
        assert_eq!(CDR(input).addr(), original.as_raw().addr());
        assert_eq!(TAG(input).addr(), original.as_raw().addr());
    });
}

#[test]
fn owned_checked_header_reads_never_admit_unregistered_or_retired_storage() {
    let unregistered = Box::new(SexprecCore::new(SEXPTYPE::INTSXP));
    let pointer = ptr::from_ref(unregistered.as_ref()).cast_mut();
    let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        TYPEOF(pointer);
    }));
    assert!(denied.is_err(), "alignment grants no read authority");
    let pointer = {
        let mut arena = RArena::new();
        arena.alloc_node(SEXPTYPE::LISTSXP)
    };
    // This address is only a lookup key after facade retirement; translated
    // access must reject it before any canonical Cell can be read.
    let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        TYPEOF(pointer);
    }));
    assert!(denied.is_err());
    assert_eq!(unsafe { TYPEOF(ptr::null_mut()) }, 0);
    assert!(unsafe { CAR(ptr::null_mut()) }.is_null());
}

#[test]
fn owned_checked_header_admission_captures_one_exact_generation_and_canonical_cell() {
    let mut arena = RArena::new();
    let pointer = arena.alloc_node(SEXPTYPE::INTSXP);
    let input = ptr::without_provenance_mut::<SexprecCore>(pointer.addr());
    let (canonical, original, snapshot) = crate::sexp::memory::checked_header(input).unwrap();
    assert_eq!(canonical, pointer);
    assert_eq!(snapshot.sxpinfo.type_of(), SEXPTYPE::INTSXP);
    // SAFETY: admission recovered the exact live original Cell pointer,
    // while the owning facade remains alive and no payload borrow exists.
    // Strict provenance must reject returning the address-only input here.
    assert_eq!(unsafe { (*canonical).sxpinfo.type_of() }, SEXPTYPE::INTSXP);
    unsafe { arena.free_node(pointer) };
    assert!(crate::sexp::memory::checked_header(input).is_none());
    let replacement = arena.alloc_node(SEXPTYPE::REALSXP);
    assert_eq!(replacement, pointer);
    let (_, fresh, new_snapshot) = crate::sexp::memory::checked_header(input).unwrap();
    assert_ne!(original.id(), fresh.id());
    assert_eq!(snapshot.sxpinfo.type_of(), SEXPTYPE::INTSXP);
    assert_eq!(new_snapshot.sxpinfo.type_of(), SEXPTYPE::REALSXP);
    assert!(crate::sexp::memory::checked_snapshot(input, &original).is_none());
    drop(arena);
    assert!(crate::sexp::memory::checked_header(input).is_none());
}
