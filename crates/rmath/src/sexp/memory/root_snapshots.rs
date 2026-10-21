//! Exact copied roots with original-heap snapshot admission and streaming GC views.
#![forbid(unsafe_code)]

use super::{
    CheckedNode, HeapIdentity, SEXP,
    slab::{SLAB_META, SlabMetadata},
};
use crate::sexp::protect::{RootSnapshot, StorageError};
use std::ops::Bound::{Excluded, Unbounded};

pub(crate) type CheckedRootSnapshot = RootSnapshot<(SEXP, CheckedNode)>;

pub(super) fn snapshot_error(_: StorageError) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: "R allocation failed: memory or node budget exceeded".to_owned(),
    })
}

/// Cloning directory capabilities does not allocate a page-entry buffer.
/// GC tracing receives one exact identity after each directory loan ends.
pub(crate) fn automatic_roots_for_marking(
    identity: &HeapIdentity,
) -> impl Iterator<Item = (SEXP, CheckedNode)> + use<> {
    AutomaticRoots {
        identity: identity.clone(),
        after: None,
        page: None,
        slot: 0,
        done: false,
    }
}

struct AutomaticRoots {
    identity: HeapIdentity,
    after: Option<usize>,
    page: Option<SlabMetadata>,
    slot: usize,
    done: bool,
}

impl Iterator for AutomaticRoots {
    type Item = (SEXP, CheckedNode);
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(page) = &self.page {
                while let Some(id) = page.meta.rooted_ids_from(self.slot).next() {
                    self.slot = id.slot() + 1;
                    if let Some((pointer, allocation)) = page.projection.resolve_slot(id.slot()) {
                        if allocation.id() == &id {
                            return Some((pointer, allocation));
                        }
                    }
                }
            }
            if self.done {
                return None;
            }
            self.page = None;
            let next = SLAB_META
                .try_with(|directory| {
                    directory
                        .borrow()
                        .range((self.after.map_or(Unbounded, Excluded), Unbounded))
                        .find_map(|(&key, page)| {
                            page.meta
                                .belongs_to(&self.identity)
                                .then(|| (key, page.clone()))
                        })
                })
                .ok()
                .flatten();
            if let Some((key, page)) = next {
                self.after = Some(key);
                self.page = Some(page);
                self.slot = 0;
            } else {
                self.done = true;
            }
        }
    }
}

pub(super) fn automatic_snapshot(identity: &HeapIdentity) -> CheckedRootSnapshot {
    // Both traversals are callback-free. The collector uses the streaming view
    // directly so collection needs no new root buffer under a lowered limit.
    let count = automatic_roots_for_marking(identity).count();
    RootSnapshot::try_collect(
        &identity.byte_ledger(),
        count,
        automatic_roots_for_marking(identity),
    )
    .unwrap_or_else(|error| snapshot_error(error))
}
