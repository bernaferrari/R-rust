#![forbid(unsafe_code)]
//! Non-moving mark-and-sweep collection over the direct heap.
//!
//! Marking traces the object graph through one exhaustive `match` in
//! [`RObject::trace_edges`] — no pointer re-derivation anywhere. Weak
//! references are resolved in a fixpoint pass after the strong mark; dead
//! keys queue their finalizers for the runtime to invoke after collection.

use super::heap::{Heap, SlotState};
use super::id::ValueId;
use super::object::RObject;

/// What the collector discovered, handed back for the runtime to act on.
#[derive(Debug, Default)]
pub struct GcOutcome {
    pub collected: usize,
    pub surviving: usize,
    /// `(weakref, finalizer)` pairs whose keys died this cycle. The runtime
    /// invokes the finalizer closures after collection completes.
    pub pending_finalizers: Vec<(ValueId, ValueId)>,
}

/// One extra root set the host session injects (e.g. its environment chain
/// heads and evaluation state) alongside the unified root machinery.
pub struct RootSet<'a> {
    pub stack: &'a super::roots::RootStack,
    pub table: &'a super::roots::RootTable,
    pub extra: &'a [ValueId],
}

/// Run a full mark-and-sweep collection.
pub fn collect(heap: &mut Heap, roots: &RootSet<'_>) -> GcOutcome {
    // ---------- strong mark ----------
    let mut worklist: Vec<ValueId> = Vec::new();
    worklist.extend(roots.stack.roots());
    worklist.extend(roots.table.roots());
    worklist.extend(roots.extra.iter().copied());

    let mut scratch: Vec<ValueId> = Vec::new();
    while let Some(id) = worklist.pop() {
        if id.is_sentinel() {
            continue;
        }
        let marked = match heap.get_mut(id) {
            Some(slot) => {
                if slot.gc_mark {
                    continue;
                }
                slot.gc_mark = true;
                true
            }
            None => false,
        };
        if !marked {
            continue;
        }
        // Collect edges without holding the mutable borrow.
        if let Some(slot) = heap.get(id) {
            scratch.clear();
            if slot.attrib != ValueId::NIL {
                scratch.push(slot.attrib);
            }
            slot.body.trace_edges(&mut scratch);
            worklist.extend(scratch.iter().copied());
        }
    }

    // ---------- weak-reference fixpoint ----------
    // A weakref's value becomes strongly reachable when its key is reachable.
    // Iterate because values can themselves hold keys of other weakrefs.
    loop {
        let mut progress = false;
        let weakrefs: Vec<(ValueId, ValueId, ValueId)> = heap
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, state)| {
                if let SlotState::Occupied(slot) = state {
                    if slot.gc_mark {
                        if let RObject::WeakRef { key, value, .. } = &slot.body {
                            return Some((ValueId::new(index as u32, slot.generation), *key, *value));
                        }
                    }
                }
                None
            })
            .collect();
        for (_, key, value) in weakrefs {
            let key_live = key.is_sentinel() || heap.get(key).map(|s| s.gc_mark).unwrap_or(false);
            if key_live {
                if let Some(slot) = heap.get_mut(value) {
                    if !slot.gc_mark {
                        slot.gc_mark = true;
                        progress = true;
                        // Trace the newly-marked value's own edges.
                        if let Some(slot) = heap.get(value) {
                            scratch.clear();
                            if slot.attrib != ValueId::NIL {
                                scratch.push(slot.attrib);
                            }
                            slot.body.trace_edges(&mut scratch);
                            worklist.extend(scratch.iter().copied());
                            while let Some(id) = worklist.pop() {
                                mark_deep(heap, id, &mut worklist, &mut scratch);
                            }
                        }
                    }
                }
            }
        }
        if !progress {
            break;
        }
    }

    // ---------- finalizer discovery ----------
    let mut pending_finalizers = Vec::new();
    for (index, state) in heap.slots.iter().enumerate() {
        if let SlotState::Occupied(slot) = state {
            if slot.gc_mark {
                if let RObject::WeakRef {
                    key,
                    finalizer,
                    on_exit,
                    ..
                } = &slot.body
                {
                    let key_live = key.is_sentinel() || heap.get(*key).map(|s| s.gc_mark).unwrap_or(false);
                    if !key_live && *finalizer != ValueId::NIL && !*on_exit {
                        pending_finalizers.push((
                            ValueId::new(index as u32, slot.generation),
                            *finalizer,
                        ));
                    }
                }
            }
        }
    }

    // ---------- sweep ----------
    let mut collected = 0usize;
    let mut surviving = 0usize;
    for index in 0..heap.slots.len() {
        let state = &mut heap.slots[index];
        if let SlotState::Occupied(slot) = state {
            if slot.gc_mark {
                slot.gc_mark = false;
                surviving += 1;
            } else {
                let generation = slot.generation;
                *state = SlotState::Free {
                    generation,
                    next_free: heap.free_head,
                };
                heap.free_head = Some(index as u32);
                heap.dec_live();
                collected += 1;
            }
        }
    }
    heap.note_collection(surviving);

    GcOutcome {
        collected,
        surviving,
        pending_finalizers,
    }
}

/// Iterative deep-mark helper shared by the strong pass and the weakref
/// fixpoint; bounded by the heap size since marks are idempotent.
fn mark_deep(heap: &mut Heap, id: ValueId, worklist: &mut Vec<ValueId>, scratch: &mut Vec<ValueId>) {
    if id.is_sentinel() {
        return;
    }
    let newly = match heap.get_mut(id) {
        Some(slot) => {
            if slot.gc_mark {
                false
            } else {
                slot.gc_mark = true;
                true
            }
        }
        None => false,
    };
    if !newly {
        return;
    }
    if let Some(slot) = heap.get(id) {
        scratch.clear();
        if slot.attrib != ValueId::NIL {
            scratch.push(slot.attrib);
        }
        slot.body.trace_edges(scratch);
        worklist.extend(scratch.iter().copied());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::roots::{RootStack, RootTable};

    fn fixture(heap: &Heap) -> (RootStack, RootTable, Vec<ValueId>) {
        let _ = heap;
        (RootStack::new(), RootTable::new(), Vec::new())
    }

    #[test]
    fn unrooted_objects_are_collected_rooted_survive() {
        let mut heap = Heap::new();
        let rooted = heap.alloc(RObject::Real(vec![1.0]));
        let garbage = heap.alloc(RObject::Real(vec![2.0]));
        let (stack, table, extra) = fixture(&heap);
        let frame = stack.frame();
        frame.protect(rooted);
        let outcome = collect(&mut heap, &RootSet {
            stack: &stack,
            table: &table,
            extra: &extra,
        });
        assert_eq!(outcome.collected, 1);
        assert!(heap.is_live(rooted));
        assert!(!heap.is_live(garbage));
        drop(frame);
    }

    #[test]
    fn stale_handles_fail_after_slot_reuse() {
        let mut heap = Heap::new();
        let garbage = heap.alloc(RObject::Real(vec![1.0]));
        let (stack, table, extra) = fixture(&heap);
        collect(&mut heap, &RootSet {
            stack: &stack,
            table: &table,
            extra: &extra,
        });
        assert!(!heap.is_live(garbage));
        // Reallocate — the slot is reused with a bumped generation.
        let fresh = heap.alloc(RObject::Real(vec![9.0]));
        assert!(heap.is_live(fresh));
        assert!(!heap.is_live(garbage), "stale handle must stay invalid");
        assert_ne!(garbage, fresh);
    }

    #[test]
    fn cyclic_garbage_is_collected() {
        let mut heap = Heap::new();
        // Two pairlist cells pointing at each other — pure cycle, no roots.
        let a = heap.alloc(RObject::Pairlist {
            car: ValueId::NIL,
            cdr: ValueId::NIL,
            tag: ValueId::NIL,
        });
        let b = heap.alloc(RObject::Pairlist {
            car: ValueId::NIL,
            cdr: ValueId::NIL,
            tag: ValueId::NIL,
        });
        if let Some(RObject::Pairlist { cdr, .. }) = heap.object_mut(a) {
            *cdr = b;
        }
        if let Some(RObject::Pairlist { cdr, .. }) = heap.object_mut(b) {
            *cdr = a;
        }
        let (stack, table, extra) = fixture(&heap);
        let outcome = collect(&mut heap, &RootSet {
            stack: &stack,
            table: &table,
            extra: &extra,
        });
        assert_eq!(outcome.collected, 2, "mark-sweep collects cycles");
    }

    #[test]
    fn rooted_cycles_survive_through_their_edges() {
        let mut heap = Heap::new();
        let a = heap.alloc(RObject::Pairlist {
            car: ValueId::NIL,
            cdr: ValueId::NIL,
            tag: ValueId::NIL,
        });
        let b = heap.alloc(RObject::Pairlist {
            car: ValueId::NIL,
            cdr: ValueId::NIL,
            tag: ValueId::NIL,
        });
        if let Some(RObject::Pairlist { cdr, .. }) = heap.object_mut(a) {
            *cdr = b;
        }
        if let Some(RObject::Pairlist { cdr, .. }) = heap.object_mut(b) {
            *cdr = a;
        }
        let (stack, table, extra) = fixture(&heap);
        let frame = stack.frame();
        frame.protect(a);
        let outcome = collect(&mut heap, &RootSet {
            stack: &stack,
            table: &table,
            extra: &extra,
        });
        assert_eq!(outcome.collected, 0);
        assert!(heap.is_live(a) && heap.is_live(b));
    }
}
