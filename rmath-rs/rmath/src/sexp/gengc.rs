//! Generational garbage collector with card marking write barriers.

use std::alloc::{alloc, dealloc, Layout};
use std::ptr::{self, NonNull};

use super::ffi::{SexprecCore, SxpInfo, SEXP, SEXPTYPE};

/// Card size in bytes for the card marking table.
pub const CARD_SIZE: usize = 512;

/// Card table entry states.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardState {
    Clean = 0,
    Dirty = 1,
    Marked = 2,
}

/// Generations for object aging.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Generation {
    Young = 0,
    Old = 1,
}

// ---------------------------------------------------------------------------
// Card Marking Table
// ---------------------------------------------------------------------------

/// Card marking table for old generation.
pub struct CardTable {
    base: *mut u8,
    size: usize,
    heap_base: *mut u8,
    heap_end: *mut u8,
}

impl CardTable {
    /// Create a new card table covering the given heap range.
    pub unsafe fn new(heap_base: *mut u8, heap_size: usize) -> Self {
        unsafe {
            let card_count = (heap_size + CARD_SIZE - 1) / CARD_SIZE;
            let layout = Layout::from_size_align(card_count, 64).unwrap();
            let base = alloc(layout);
            ptr::write_bytes(base, 0, card_count);

            CardTable {
                base,
                size: card_count,
                heap_base,
                heap_end: heap_base.add(heap_size),
            }
        }
    }

    /// Get the card index for a given object pointer.
    #[inline]
    pub fn card_index(&self, obj: SEXP) -> usize {
        let offset = (obj as *mut u8 as usize) - (self.heap_base as usize);
        offset / CARD_SIZE
    }

    /// Mark a card as dirty containing an old -> young reference.
    #[inline]
    pub fn mark_dirty(&self, obj: SEXP) {
        let idx = self.card_index(obj);
        debug_assert!(idx < self.size);
        unsafe { *self.base.add(idx) = CardState::Dirty as u8 }
    }

    /// Clear all dirty cards after minor GC.
    pub fn clear_dirty(&mut self) {
        unsafe {
            ptr::write_bytes(self.base, 0, self.size);
        }
    }

    /// Iterate over all dirty cards.
    pub fn dirty_cards(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.size).filter(move |&i| unsafe { *self.base.add(i) == CardState::Dirty as u8 })
    }
}

impl Drop for CardTable {
    fn drop(&mut self) {
        unsafe {
            let layout = Layout::from_size_align(self.size, 64).unwrap();
            dealloc(self.base, layout);
        }
    }
}

// ---------------------------------------------------------------------------
// Remembered Set
// ---------------------------------------------------------------------------

/// Remembered set tracking old objects with references to young objects.
#[derive(Default)]
pub struct RememberedSet {
    entries: Vec<SEXP>,
}

impl RememberedSet {
    /// Add an old generation object to the remembered set.
    #[inline]
    pub fn add(&mut self, obj: SEXP) {
        // Already in remembered set?
        unsafe {
            if (*obj).sxpinfo.gcgen() == 0 {
                return;
            }

            if !(*obj).sxpinfo.mark() {
                (*obj).sxpinfo.set_mark(true);
                self.entries.push(obj);
            }
        }
    }

    /// Clear and reset all mark bits.
    pub fn clear(&mut self) {
        for &obj in &self.entries {
            unsafe {
                (*obj).sxpinfo.set_mark(false);
            }
        }
        self.entries.clear();
    }

    /// Iterate over remembered objects.
    pub fn iter(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.entries.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// Write Barriers
// ---------------------------------------------------------------------------

/// Write barrier for when an old generation object stores a reference to a young object.
///
/// This is the core generational GC invariant:
/// No young object is referenced only from old objects.
///
/// This function MUST be called BEFORE every pointer write to heap objects.
#[inline(always)]
pub fn write_barrier(parent: SEXP, child: SEXP) {
    if parent.is_null() || child.is_null() {
        return;
    }

    unsafe {
        let parent_gen = (*parent).sxpinfo.gcgen();
        let child_gen = (*child).sxpinfo.gcgen();

        // Old -> Young reference detected
        if parent_gen == Generation::Old as u8 && child_gen == Generation::Young as u8 {
            // Add to remembered set and mark card dirty
            REMBERED_SET.with(|rs| rs.borrow_mut().add(parent));
            CARD_TABLE.with(|ct| ct.borrow().mark_dirty(parent));
        }
    }
}

/// Raw write barrier for vector element assignment.
#[inline(always)]
pub fn vector_write_barrier(vec: SEXP, index: usize, value: SEXP) {
    write_barrier(vec, value);
}

/// Raw write barrier for list field assignment.
#[inline(always)]
pub fn list_write_barrier(list: SEXP, field: u8, value: SEXP) {
    write_barrier(list, value);
}

/// Raw write barrier for attribute assignment.
#[inline(always)]
pub fn attrib_write_barrier(obj: SEXP, value: SEXP) {
    write_barrier(obj, value);
}

// ---------------------------------------------------------------------------
// Generation Promotion
// ---------------------------------------------------------------------------

/// Promote an object from young to old generation.
#[inline]
pub unsafe fn promote_to_old(obj: SEXP) {
    unsafe {
        debug_assert!((*obj).sxpinfo.gcgen() == Generation::Young as u8);
        (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
    }
}

// ---------------------------------------------------------------------------
// Thread Local GC State
// ---------------------------------------------------------------------------

thread_local! {
    static CARD_TABLE: std::cell::RefCell<CardTable> = std::cell::RefCell::new(unsafe {
        // Heap base will be initialized properly when GC is started
        CardTable::new(0x100000000 as *mut u8, 1 << 30)
    });

    static REMBERED_SET: std::cell::RefCell<RememberedSet> = std::cell::RefCell::new(RememberedSet::default());
}

/// Initialize the GC card table for the given heap range.
pub unsafe fn init_gc_heap(heap_base: *mut u8, heap_size: usize) {
    unsafe {
        CARD_TABLE.with(|ct| {
            let mut ct = ct.borrow_mut();
            *ct = CardTable::new(heap_base, heap_size);
        });
    }
}

/// Run minor garbage collection on young generation.
pub fn minor_gc() {
    // 1. Collect roots from stack, registers, globals
    // 2. Add all remembered set objects as extra roots
    // 3. Trace reachable young objects
    // 4. Promote surviving objects to old generation
    // 5. Reset young generation
    // 6. Clear dirty cards and remembered set
    REMBERED_SET.with(|rs| rs.borrow_mut().clear());
    CARD_TABLE.with(|ct| ct.borrow_mut().clear_dirty());
}

// ---------------------------------------------------------------------------
// Barrier Enforcement Wrappers
// ---------------------------------------------------------------------------

/// Guarded vector slot reference that automatically runs write barrier on assignment.
pub struct VectorSlot<'a> {
    vec: SEXP,
    slot: &'a mut SEXP,
}

impl<'a> VectorSlot<'a> {
    #[inline]
    pub fn new(vec: SEXP, slot: &'a mut SEXP) -> Self {
        VectorSlot { vec, slot }
    }

    #[inline]
    pub fn set(&mut self, value: SEXP) {
        vector_write_barrier(self.vec, 0, value);
        *self.slot = value;
    }

    #[inline]
    pub fn get(&self) -> SEXP {
        *self.slot
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::memory::with_arena;
    use super::*;

    #[test]
    fn test_write_barrier_detects_old_to_young() {
        with_arena(|arena| {
            let old_obj = arena.alloc_node(SEXPTYPE::LISTSXP);
            let young_obj = arena.alloc_node(SEXPTYPE::INTSXP);

            unsafe {
                (*old_obj).sxpinfo.set_gcgen(Generation::Old as u8);
                (*young_obj).sxpinfo.set_gcgen(Generation::Young as u8);
            }

            write_barrier(old_obj, young_obj);

            REMBERED_SET.with(|rs| {
                assert_eq!(rs.borrow().len(), 1);
            });
        });
    }

    #[test]
    fn test_card_table_marking() {
        unsafe {
            let heap = alloc(Layout::from_size_align(4096, 4096).unwrap());
            let ct = CardTable::new(heap, 4096);

            let obj = heap.add(1024) as SEXP;
            ct.mark_dirty(obj);

            let dirty: Vec<usize> = ct.dirty_cards().collect();
            assert_eq!(dirty, vec![2]);

            dealloc(heap, Layout::from_size_align(4096, 4096).unwrap());
        }
    }
}
