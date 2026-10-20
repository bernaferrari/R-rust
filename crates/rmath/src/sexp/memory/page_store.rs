#![forbid(unsafe_code)]
//! Descriptor buffers are prepared without moving old pages, then committed
//! after page construction. Both buffers remain funded across replacement.
use super::{AllocationCharge, SlabPage, WorkspaceReservation};
use std::{alloc::Layout, ops::Deref};

const INITIAL_CAPACITY: usize = 4;

pub(super) struct PageStore {
    pages: Vec<SlabPage>,
    // Release the actual descriptor buffer (and its pages) before its ticket.
    charge: Option<AllocationCharge>,
}
pub(super) struct PreparedPages {
    pages: Vec<SlabPage>,
    admission: WorkspaceReservation,
}
impl PageStore {
    pub(super) fn new() -> Self {
        Self {
            pages: Vec::new(),
            charge: None,
        }
    }
    fn requested_bytes(capacity: usize) -> Option<usize> {
        Some(Layout::array::<SlabPage>(capacity).ok()?.size())
    }
    pub(super) fn minimum_bytes() -> usize {
        Self::requested_bytes(INITIAL_CAPACITY).expect("initial page descriptor layout")
    }
    /// The whole replacement buffer is temporary headroom, not just the delta.
    pub(super) fn growth(&self) -> Option<(usize, usize)> {
        if self.pages.len() < self.pages.capacity() {
            return Some((self.pages.capacity(), 0));
        }
        let capacity = self.pages.capacity().checked_mul(2)?.max(INITIAL_CAPACITY);
        Some((capacity, Self::requested_bytes(capacity)?))
    }
    pub(super) fn prepare_growth(
        capacity: usize,
        mut admission: WorkspaceReservation,
    ) -> Option<PreparedPages> {
        if admission.bytes() != Self::requested_bytes(capacity)? {
            return None;
        }
        let mut pages = Vec::new();
        pages.try_reserve_exact(capacity).ok()?;
        admission.try_resize(Self::requested_bytes(pages.capacity())?)?;
        Some(PreparedPages { pages, admission })
    }
    pub(super) fn commit_page(
        &mut self,
        page: SlabPage,
        prepared: Option<PreparedPages>,
    ) -> Option<()> {
        if page.meta.page() != self.pages.len()
            || self
                .pages
                .last()
                .is_some_and(|previous| previous.meta.cookie() >= page.meta.cookie())
        {
            return None;
        }
        if let Some(mut prepared) = prepared {
            if prepared.pages.capacity() <= self.pages.len() {
                return None;
            }
            let charge = prepared.admission.commit_allocation()?;
            let mut old = std::mem::take(&mut self.pages);
            // Moving the descriptors never moves their actual pinned headers.
            // Explicit pushes cannot steal the old allocation from IntoIter.
            for page in old.drain(..) {
                prepared.pages.push(page);
            }
            drop(old);
            self.pages = prepared.pages;
            self.charge = Some(charge);
        }
        if self.pages.len() >= self.pages.capacity() {
            return None;
        }
        self.pages.push(page);
        Some(())
    }
    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.pages.capacity()
    }
    #[cfg(test)]
    pub(super) fn allocation_size(&self) -> usize {
        Self::requested_bytes(self.pages.capacity()).expect("actual descriptor layout")
    }
}
impl Deref for PageStore {
    type Target = [SlabPage];
    fn deref(&self) -> &Self::Target {
        &self.pages
    }
}
