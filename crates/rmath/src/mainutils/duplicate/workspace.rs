//! Charge both traversal containers using their allocators' requested layouts.

use std::{alloc::Layout, cell::RefCell, ptr::NonNull};

use allocator_api2::alloc::{AllocError, Allocator, Global};

use crate::sexp::memory::WorkspaceReservation;

#[derive(Clone, Copy)]
pub(super) struct TraversalAllocator<'workspace>(
    pub(super) &'workspace RefCell<WorkspaceReservation>,
);

// SAFETY: Global supplies valid allocations with the requested layout. Every
// clone borrows the same reservation and a container cannot outlive that
// reservation. Default growth allocates before deallocating, funding both
// blocks while moving elements.
unsafe impl Allocator for TraversalAllocator<'_> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let mut charge = self.0.borrow_mut();
        let previous = charge.bytes();
        let total = previous.checked_add(layout.size()).ok_or(AllocError)?;
        charge.try_resize(total).ok_or(AllocError)?;
        let block = Global.allocate(layout);
        if block.is_err() {
            charge
                .try_resize(previous)
                .expect("refund failed allocation");
        }
        block
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        // SAFETY: The caller provides a live allocation from this allocator.
        // Global returns exactly the layout's size, so the refund is exact.
        unsafe { Global.deallocate(ptr, layout) };
        let mut charge = self.0.borrow_mut();
        let remaining = charge.bytes() - layout.size();
        charge
            .try_resize(remaining)
            .expect("refund freed allocation");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::memory::{ArenaBudget, RArena};
    use allocator_api2::vec::Vec;
    use hashbrown::HashSet;

    #[test]
    fn traversal_allocator_admits_replacement_peak_and_refunds_both_containers() {
        let mut arena = RArena::new();
        let workspace = RefCell::new(arena.reserve_workspace(0).unwrap());
        let allocator = TraversalAllocator(&workspace);
        let mut frames = Vec::<u64, _>::new_in(allocator);
        frames.try_reserve_exact(2).unwrap();
        frames.extend([11, 13]);
        let mut active = HashSet::new_in(allocator);
        active.try_reserve(1).unwrap();
        active.insert(23u64);
        let previous = 2 * size_of::<u64>() + active.allocation_size();
        let replacement = 4 * size_of::<u64>();
        assert_eq!(arena.workspace_bytes(), previous);

        let resident = arena.total_bytes_allocated();
        arena.set_budget(ArenaBudget::new(resident + previous + replacement - 1, 0));
        assert!(frames.try_reserve_exact(2).is_err());
        assert_eq!(frames.as_slice(), [11, 13]);
        assert!(active.contains(&23));
        assert_eq!(arena.workspace_bytes(), previous);

        arena.set_budget(ArenaBudget::new(resident + previous + replacement, 0));
        frames.try_reserve_exact(2).unwrap();
        assert_eq!(frames.as_slice(), [11, 13]);
        assert_eq!(
            arena.workspace_bytes(),
            replacement + active.allocation_size()
        );
        let old_table = active.allocation_size();
        let additional = active.capacity() - active.len() + 1;
        let mut probe = HashSet::new();
        probe.try_reserve(1).unwrap();
        probe.insert(23u64);
        probe.try_reserve(additional).unwrap();
        let new_table = probe.allocation_size();
        drop(probe);
        let table_peak = resident + replacement + old_table + new_table;
        arena.set_budget(ArenaBudget::new(table_peak - 1, 0));
        assert!(active.try_reserve(additional).is_err());
        assert!(active.contains(&23));
        assert_eq!(arena.workspace_bytes(), replacement + old_table);
        arena.set_budget(ArenaBudget::new(table_peak, 0));
        active.try_reserve(additional).unwrap();
        assert!(active.contains(&23));
        assert_eq!(arena.workspace_bytes(), replacement + new_table);
        drop(active);
        assert_eq!(arena.workspace_bytes(), replacement);
        drop(frames);
        assert_eq!(arena.workspace_bytes(), 0);
    }
}
