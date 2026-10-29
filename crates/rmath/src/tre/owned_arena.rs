//! Safely owned, admitted slabs behind the legacy TRE temporary-arena facade.
#![forbid(unsafe_code)]

use crate::mainutils::grep::{RegexWorkspace, workspace::WorkspaceBuffer};
use std::{cell::Cell, ffi::c_void};

#[repr(C, align(16))]
#[derive(Clone)]
struct Unit(Cell<u128>);

const _: () = assert!(std::mem::size_of::<Unit>() == 16);
type Slab = WorkspaceBuffer<Unit>;

pub(super) struct OwnedArena {
    // Slabs own their storage; moving directory entries never moves cell storage.
    blocks: WorkspaceBuffer<Slab>,
    workspace: RegexWorkspace,
    next: usize,
}

impl OwnedArena {
    pub(super) fn new(workspace: RegexWorkspace) -> Result<Self, ()> {
        Ok(Self {
            blocks: workspace.try_buffer(0)?,
            workspace,
            next: 0,
        })
    }

    pub(super) fn allocate(&mut self, bytes: usize) -> Result<*mut c_void, ()> {
        self.allocate_with_policy(bytes, 8, 64)
    }

    /// Retained tables have known final sizes and need no amortized slab slack.
    pub(super) fn allocate_exact(&mut self, bytes: usize) -> Result<*mut c_void, ()> {
        self.allocate_with_policy(bytes, 1, 1)
    }

    fn allocate_with_policy(
        &mut self,
        bytes: usize,
        growth: usize,
        minimum: usize,
    ) -> Result<*mut c_void, ()> {
        if !self.workspace.is_live() {
            return Err(());
        }
        let units = bytes.max(1).checked_add(15).ok_or(())? / 16;
        if self
            .blocks
            .last()
            .is_none_or(|block| block.len() - self.next < units)
        {
            let capacity = units.checked_mul(growth).ok_or(())?.max(minimum);
            let mut slab = self.workspace.try_buffer(capacity)?;
            slab.try_resize(capacity, Unit(Cell::new(0)))?;
            self.blocks.try_push(slab)?;
            self.next = 0;
        }
        let pointer = self
            .blocks
            .last()
            .ok_or(())?
            .as_ptr()
            .wrapping_add(self.next)
            .cast_mut()
            .cast();
        self.next = self.next.checked_add(units).ok_or(())?;
        Ok(pointer)
    }

    pub(super) fn clear(&mut self) {
        self.blocks.clear();
        self.next = 0;
    }

    #[cfg(test)]
    pub(super) fn contains(&self, pointer: *const c_void) -> bool {
        self.blocks.iter().any(|block| {
            let start = block.as_ptr().addr();
            let bytes = block.len() * std::mem::size_of::<Unit>();
            pointer.addr() >= start && pointer.addr() - start < bytes
        })
    }
}
