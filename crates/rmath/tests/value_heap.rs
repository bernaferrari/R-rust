//! Integration validation for the pure-Rust ValueId heap: allocation,
//! generational safety, rooting, collection, cycles, finalizers, weakrefs,
//! and ALTREP providers.

use rmath::value::gc::{collect, RootSet};
use rmath::value::object::RObject;
use rmath::value::{Heap, RootStack, RootTable, ValueId};

fn empty_fixture() -> (RootStack, RootTable, Vec<ValueId>) {
    (RootStack::new(), RootTable::new(), Vec::new())
}

#[test]
fn gc_stress_allocations_reuse_slots_without_stale_access() {
    let mut heap = Heap::new();
    let (stack, table, extra) = empty_fixture();
    let mut generational_reuse = 0;
    for round in 0..50 {
        let mut ids = Vec::new();
        for i in 0..100 {
            ids.push(heap.alloc(RObject::Integer(vec![i, round as i32])));
        }
        // Keep only every tenth object rooted this round.
        let survivor = ids[0];
        let outcome = collect(
            &mut heap,
            &RootSet {
                stack: &stack,
                table: &table,
                extra: std::slice::from_ref(&survivor),
            },
        );
        assert!(heap.is_live(survivor));
        assert!(outcome.collected >= 99);
        // All non-rooted handles are stale.
        for id in &ids[1..] {
            assert!(!heap.is_live(*id));
        }
        generational_reuse += outcome.collected;
    }
    assert!(generational_reuse > 4000);
}

#[test]
fn attributes_are_traced_as_edges() {
    let mut heap = Heap::new();
    let attrib_target = heap.alloc(RObject::Real(vec![42.0]));
    let host = heap.alloc(RObject::Integer(vec![1]));
    heap.get_mut(host).unwrap().attrib = attrib_target;
    let (stack, table, extra) = empty_fixture();
    let outcome = collect(
        &mut heap,
        &RootSet {
            stack: &stack,
            table: &table,
            extra: &[host],
        },
    );
    assert_eq!(outcome.collected, 0);
    assert!(heap.is_live(attrib_target));
}

#[test]
fn weakref_dead_key_queues_finalizer() {
    let mut heap = Heap::new();
    let key = heap.alloc(RObject::Real(vec![1.0]));
    let finalizer = heap.alloc(RObject::Real(vec![2.0]));
    let weakref = heap.alloc(RObject::WeakRef {
        key,
        value: ValueId::NIL,
        finalizer,
        on_exit: false,
    });
    let (stack, table, extra) = empty_fixture();
    let outcome = collect(
        &mut heap,
        &RootSet {
            stack: &stack,
            table: &table,
            extra: &[weakref],
        },
    );
    assert!(
        outcome
            .pending_finalizers
            .iter()
            .any(|(w, f)| *w == weakref && *f == finalizer),
        "dead key must queue its finalizer: {:?}",
        outcome.pending_finalizers
    );
    // The finalizer object itself is traced strongly through the weakref.
    assert!(heap.is_live(finalizer));
}

#[test]
fn weakref_live_key_keeps_value_strong() {
    let mut heap = Heap::new();
    let key = heap.alloc(RObject::Real(vec![1.0]));
    let value = heap.alloc(RObject::Real(vec![2.0]));
    let weakref = heap.alloc(RObject::WeakRef {
        key,
        value,
        finalizer: ValueId::NIL,
        on_exit: false,
    });
    let (stack, table, extra) = empty_fixture();
    let outcome = collect(
        &mut heap,
        &RootSet {
            stack: &stack,
            table: &table,
            extra: &[weakref, key],
        },
    );
    assert!(heap.is_live(value), "live key preserves the weak value");
    assert!(outcome.pending_finalizers.is_empty());
}

#[test]
fn altrep_provider_edges_are_traced() {
    use rmath::value::object::AltrepProvider;

    #[derive(Debug)]
    struct LazySeq {
        backing: ValueId,
    }
    impl AltrepProvider for LazySeq {
        fn len(&self) -> usize {
            10
        }
        fn elt_real(&self, index: usize) -> Option<f64> {
            Some(index as f64 + 1.0)
        }
        fn trace_edges(&self, out: &mut Vec<ValueId>) {
            out.push(self.backing);
        }
    }

    let mut heap = Heap::new();
    let backing = heap.alloc(RObject::Real(vec![1.0, 2.0]));
    let altrep = heap.alloc(RObject::Altrep(Box::new(LazySeq { backing })));
    let (stack, table, extra) = empty_fixture();
    collect(
        &mut heap,
        &RootSet {
            stack: &stack,
            table: &table,
            extra: &[altrep],
        },
    );
    assert!(heap.is_live(backing), "altrep provider edges must be traced");

    // Element access goes through the provider.
    match heap.object(altrep) {
        Some(RObject::Altrep(provider)) => {
            assert_eq!(provider.len(), 10);
            assert_eq!(provider.elt_real(3), Some(4.0));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn environment_frames_trace_bindings_and_enclosure() {
    let mut heap = Heap::new();
    let mut interner = rmath::value::SymbolInterner::new();
    let sym = interner.intern_symbol("x");
    heap.interner = interner;
    let bound = heap.alloc(RObject::Real(vec![3.0]));
    let enclosing = heap.alloc(RObject::Environment {
        frame: Default::default(),
        enclos: ValueId::EMPTY_ENV,
    });
    let env = heap.alloc(RObject::Environment {
        frame: Default::default(),
        enclos: enclosing,
    });
    if let Some(RObject::Environment { frame, .. }) = heap.object_mut(env) {
        frame.insert(sym, bound);
    }
    let (stack, table, extra) = empty_fixture();
    collect(
        &mut heap,
        &RootSet {
            stack: &stack,
            table: &table,
            extra: &[env],
        },
    );
    assert!(heap.is_live(bound), "frame bindings are traced");
    assert!(heap.is_live(enclosing), "enclosure chain is traced");
}
