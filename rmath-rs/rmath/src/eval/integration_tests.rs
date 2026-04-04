//! Integration tests for the eval pipeline.

use crate::error::{catch_r, REvalError};
use crate::eval::eval::Rf_eval;
use crate::sexp::builder::{int_vec, logical_vec, real_vec, string_vec, PairlistBuilder};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_GlobalEnv, R_NilValue};
use crate::sexp::memory::RArena;
use crate::sexp::protect::{R_ProtectCount, Rf_protect, Rf_unprotect};
use crate::sexp::safe::{PairlistIter, Sexp};
use crate::sexp::session::RSession;
use std::ptr;

#[test]
fn test_builder_int_vec() {
    let vec = int_vec(&[1, 2, 3, 4, 5]).unwrap();
    assert_eq!(vec.len(), 5);
    assert_eq!(vec.integer_elt(0), Some(1));
    assert_eq!(vec.integer_elt(4), Some(5));
}

#[test]
fn test_builder_real_vec() {
    let vec = real_vec(&[1.0, 2.0, 3.0]).unwrap();
    assert_eq!(vec.len(), 3);
    assert!((vec.real_elt(0).unwrap() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_builder_logical_vec() {
    let vec = logical_vec(&[true, false, true]).unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec.logical_elt(0), Some(1));
    assert_eq!(vec.logical_elt(1), Some(0));
}

#[test]
fn test_builder_string_vec() {
    let vec = string_vec(&["hello", "world"]).unwrap();
    assert_eq!(vec.len(), 2);
    assert!(vec.string_elt(0).is_some());
    assert!(vec.string_elt(1).is_some());
}

#[test]
fn test_session_lifecycle() {
    let mut session = RSession::new();
    assert!(session.is_active());
    assert!(session.global_env().is_some());
    session.close();
    assert!(!session.is_active());
}

#[test]
fn test_eval_null_returns_nil() {
    let result = catch_r(|| unsafe { Rf_eval(ptr::null_mut(), R_GlobalEnv()) });
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), unsafe { R_NilValue() });
}

#[test]
fn test_self_evaluating_types() {
    let mut arena = RArena::new();

    // INTSXP should return itself
    let int_vec = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
    let result = unsafe { Rf_eval(int_vec, R_GlobalEnv()) };
    assert_eq!(result, int_vec);

    // REALSXP should return itself
    let real_vec = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
    let result = unsafe { Rf_eval(real_vec, R_GlobalEnv()) };
    assert_eq!(result, real_vec);

    // NILSXP should return itself
    let nil = unsafe { R_NilValue() };
    let result = unsafe { Rf_eval(nil, R_GlobalEnv()) };
    assert_eq!(result, nil);
}

#[test]
fn test_sexp_safe_wrapper_full_lifecycle() {
    // Create vectors using builders
    let int_v = int_vec(&[10, 20, 30]).unwrap();
    let real_v = real_vec(&[1.5, 2.5]).unwrap();

    // Test type predicates
    assert!(int_v.is_vector());
    assert!(int_v.is_atomic());
    assert!(real_v.is_vector());
    assert!(real_v.is_atomic());

    // Test element access
    assert_eq!(int_v.integer_elt(0), Some(10));
    assert!((real_v.real_elt(0).unwrap() - 1.5).abs() < f64::EPSILON);

    // Test mutation
    assert!(int_v.set_integer_elt(0, 99));
    assert_eq!(int_v.integer_elt(0), Some(99));

    // Test slice views
    let int_v2 = int_vec(&[1, 2, 3, 4]).unwrap();
    let slice = int_v2.as_integer_slice().unwrap();
    assert_eq!(slice, &[1, 2, 3, 4]);

    // Test iterators
    let int_v3 = int_vec(&[1, 2, 3, 4]).unwrap();
    let values: Vec<_> = int_v3.iter_integer().collect();
    assert_eq!(values, vec![1, 2, 3, 4]);
}

#[test]
fn test_gc_integration() {
    // Run GC and verify it doesn't panic
    let session = RSession::new();
    session.gc();

    // Create some objects and run GC
    let _int_v = int_vec(&[1, 2, 3, 4, 5]).unwrap();
    let _real_v = real_vec(&[1.0, 2.0, 3.0]).unwrap();
    session.gc();
}

#[test]
fn test_protect_stack_integration() {
    let depth_before = R_ProtectCount();
    unsafe {
        Rf_protect(ptr::null_mut());
        Rf_protect(ptr::null_mut());
        Rf_protect(ptr::null_mut());
    }
    assert_eq!(R_ProtectCount(), depth_before);
    unsafe {
        Rf_unprotect(3);
    }
    assert_eq!(R_ProtectCount(), depth_before);
}

#[test]
fn test_pairlist_builder_and_iteration() {
    let mut arena = RArena::new();
    let a = arena.alloc_node(SEXPTYPE::INTSXP);
    let b = arena.alloc_node(SEXPTYPE::REALSXP);
    let c = arena.alloc_node(SEXPTYPE::LGLSXP);

    let list = PairlistBuilder::new()
        .push_untagged(a)
        .push_untagged(b)
        .push_untagged(c)
        .build()
        .unwrap();

    assert!(list.is_pairlist());

    let items: Vec<_> = PairlistIter::new(list).collect();
    assert_eq!(items.len(), 3);
}

#[test]
fn test_rng_thread_safety() {
    use crate::rng::{set_seed, unif_rand};
    use std::sync::{Arc, Barrier};
    use std::thread;

    // Set different seeds in different threads
    let barrier = Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let barrier = barrier.clone();
            thread::spawn(move || {
                set_seed(i as u32 + 1, 1234);
                barrier.wait();
                let mut values = Vec::new();
                for _ in 0..100 {
                    values.push(unif_rand());
                }
                values
            })
        })
        .collect();

    let results: Vec<Vec<f64>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    for i in 0..results.len() {
        for j in (i + 1)..results.len() {
            assert_ne!(
                results[i], results[j],
                "Threads {} and {} produced identical random sequences",
                i, j
            );
        }
    }
}

#[test]
fn test_error_handling() {
    let result = catch_r(|| {
        panic!("test error");
    });
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("test error"));
}

#[test]
fn test_arena_free_list_reuse() {
    use crate::sexp::gengc::minor_gc;

    let mut arena = RArena::new();
    let initial_nodes = arena.node_count();

    let _obj = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
    minor_gc();

    let _obj2 = arena.alloc_vector(SEXPTYPE::INTSXP, 3);

    assert!(arena.node_count() <= initial_nodes + 1);
}
