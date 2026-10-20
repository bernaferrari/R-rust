#![forbid(unsafe_code)]
//! Explicit, bounded mark-phase measurement, separate from R lazy loading.

use super::*;
use crate::sexp::{ffi::EdgeField, heap::ReferenceChild, session::RSession};
use std::time::{Duration, Instant};

#[test]
#[ignore = "manual bounded mark benchmark; no machine-dependent timing assertion"]
fn fixed_live_heap_mark_measurement() {
    for nodes in [16_384, 65_536] {
        let mut session = RSession::new_for_gc_tests();
        let (head, tokens) = session
            .with_arena(|arena| {
                let heap = arena.heap_identity();
                let mut tokens = Vec::with_capacity(nodes);
                let mut head = std::ptr::null_mut();
                for _ in 0..nodes {
                    head = arena.alloc_node(SEXPTYPE::LISTSXP);
                    let token = arena.node_token(head).unwrap();
                    if let Some(previous) = tokens.last() {
                        for field in [EdgeField::ListCar, EdgeField::ListCdr] {
                            heap.set_edge(&token, field, ReferenceChild::Node(previous))
                                .unwrap();
                        }
                    }
                    tokens.push(token);
                }
                (head, tokens)
            })
            .unwrap();
        let root = session.sexp(head).unwrap().into_owned().unwrap();
        full_gc();
        assert!(tokens.iter().all(|token| token.is_live()));
        let collections_before = with_gc_state(|state| state.stats.collections);
        let started = Instant::now();
        let mut rounds = 0;
        let mut mark_ns = 0;
        while started.elapsed() < Duration::from_secs(3) {
            instance::with_required_current_instance(|owner| {
                run_gc_cycle_in(owner, |owner| {
                    let mark_started = Instant::now();
                    mark_from_all_roots_in(owner);
                    mark_ns += mark_started.elapsed().as_nanos();
                    (0, 0)
                });
            });
            rounds += 1;
        }
        let elapsed_ns = started.elapsed().as_nanos();
        let recorded = with_gc_state(|state| state.stats.collections) - collections_before;
        assert_eq!(recorded, rounds);
        assert!(rounds > 0);
        assert!(tokens.iter().all(|token| token.is_live()));
        assert!(root.is_live());
        println!(
            "FIXED_HEAP_MARK {{\"graph_nodes\":{nodes},\"collections\":{rounds},\"mark_ns\":{mark_ns},\"mark_ns_per_collection\":{},\"elapsed_ns\":{elapsed_ns},\"sweep_included\":false}}",
            mark_ns / rounds as u128
        );
    }
}
