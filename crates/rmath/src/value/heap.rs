#![forbid(unsafe_code)]
//! Direct O(1) generational heap.

use super::id::ValueId;
use super::intern::SymbolInterner;
use super::object::{RObject, Slot};

/// Slot occupancy: either a live object or a free-list link. Free slots
/// retain their generation counter so reuse bumps it and stale handles fail
/// the generation check.
#[derive(Debug)]
pub(crate) enum SlotState {
    Occupied(Slot),
    Free { generation: u32, next_free: Option<u32> },
}

/// The object heap: a flat slot vector with a free list. All access goes
/// through generation-checked `ValueId` handles — O(1) array indexing with
/// no reverse lookups, pinning, or unsafe projection.
#[derive(Debug, Default)]
pub struct Heap {
    pub(crate) slots: Vec<SlotState>,
    pub(crate) free_head: Option<u32>,
    live_count: usize,
    /// Bytes of vector payload data, used as a GC pressure heuristic.
    payload_bytes: usize,
    /// Allocation count at which the next collection triggers; grows
    // geometrically after each collection.
    pub(crate) next_gc_threshold: usize,
    allocations_since_gc: usize,
    pub interner: SymbolInterner,
}

impl Heap {
    pub fn new() -> Heap {
        Heap {
            next_gc_threshold: 1024,
            ..Heap::default()
        }
    }

    /// Allocate a new object and return its handle.
    pub fn alloc(&mut self, body: RObject) -> ValueId {
        self.allocations_since_gc += 1;
        self.payload_bytes += payload_estimate(&body);
        match self.free_head {
            Some(free_index) => {
                let slot_entry = &mut self.slots[free_index as usize];
                let (generation, next_free) = match slot_entry {
                    SlotState::Free {
                        generation,
                        next_free,
                    } => (*generation, *next_free),
                    SlotState::Occupied(_) => unreachable!("free list points at occupied slot"),
                };
                // Reuse with a bumped generation so prior handles to this
                // slot are invalidated.
                let generation = generation.wrapping_add(1).max(1);
                *slot_entry = SlotState::Occupied(Slot::new(generation, body));
                self.free_head = next_free;
                self.live_count += 1;
                ValueId::new(free_index, generation)
            }
            None => {
                debug_assert!(self.slots.len() < u32::MAX as usize);
                let slot_index = self.slots.len() as u32;
                let generation = 1;
                self.slots
                    .push(SlotState::Occupied(Slot::new(generation, body)));
                self.live_count += 1;
                ValueId::new(slot_index, generation)
            }
        }
    }

    /// Generation-checked immutable access. Returns `None` for stale or
    /// sentinel handles.
    pub fn get(&self, id: ValueId) -> Option<&Slot> {
        let index = id.slot_index()?;
        match self.slots.get(index)? {
            SlotState::Occupied(slot) if slot.generation == id.generation() => Some(slot),
            _ => None,
        }
    }

    /// Generation-checked mutable access.
    pub fn get_mut(&mut self, id: ValueId) -> Option<&mut Slot> {
        let index = id.slot_index()?;
        match self.slots.get_mut(index)? {
            SlotState::Occupied(slot) if slot.generation == id.generation() => Some(slot),
            _ => None,
        }
    }

    /// Immutable access to just the payload.
    pub fn object(&self, id: ValueId) -> Option<&RObject> {
        self.get(id).map(|slot| &slot.body)
    }

    /// Mutable access to just the payload.
    pub fn object_mut(&mut self, id: ValueId) -> Option<&mut RObject> {
        self.get_mut(id).map(|slot| &mut slot.body)
    }

    /// True when the handle currently addresses a live object.
    pub fn is_live(&self, id: ValueId) -> bool {
        if id.is_sentinel() {
            return true;
        }
        self.get(id).is_some()
    }

    pub fn live_count(&self) -> usize {
        self.live_count
    }

    pub fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    /// Whether allocation pressure suggests a collection. The collector
    /// itself decides; this is the cheap admission probe.
    pub fn gc_pressure(&self) -> bool {
        self.allocations_since_gc >= self.next_gc_threshold
    }

    pub(crate) fn note_collection(&mut self, surviving: usize) {
        self.allocations_since_gc = 0;
        // Grow the threshold geometrically relative to the surviving set so
        // collection frequency stays proportional to live data.
        self.next_gc_threshold = (surviving.max(256) * 2).max(1024);
    }

    pub(crate) fn dec_live(&mut self) {
        self.live_count = self.live_count.saturating_sub(1);
    }
}

fn payload_estimate(body: &RObject) -> usize {
    match body {
        RObject::Logical(v) | RObject::Integer(v) => v.len() * 4,
        RObject::Real(v) => v.len() * 8,
        RObject::Complex(v) => v.len() * 16,
        RObject::Raw(v) => v.len(),
        RObject::String(v) => v.len() * 8,
        RObject::List(v) | RObject::Expression(v) => v.len() * 8,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_get_roundtrip() {
        let mut heap = Heap::new();
        let id = heap.alloc(RObject::Real(vec![1.0, 2.0, 3.0]));
        assert!(heap.is_live(id));
        match heap.object(id) {
            Some(RObject::Real(v)) => assert_eq!(v, &[1.0, 2.0, 3.0]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn sentinel_handles_are_not_heap_objects() {
        let heap = Heap::new();
        assert!(heap.object(ValueId::NIL).is_none());
        assert!(heap.is_live(ValueId::NIL));
    }
}
