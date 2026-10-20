//! Checked header/projection accessors.

use super::slab::SLAB_META;
use super::*;

/// Resolve a live legacy projection to owned, generational metadata. The
/// token neither dereferences its header nor borrows its arena.
pub(crate) fn checked_node(pointer: SEXP) -> Option<CheckedNode> {
    let (meta, slot) = find_slab_slot(pointer)?;
    CheckedNode::new(meta.clone(), meta.current_id(slot)?)
}

/// Snapshot automatic handle roots from the one registered production heap.
/// Both arena and permanent pages use this directory and heap identity. The
/// directory borrow ends before any root is projected or passed to tracing.
pub(crate) fn automatic_roots(identity: &HeapIdentity) -> Vec<(SEXP, CheckedNode)> {
    let pages = SLAB_META
        .try_with(|map| {
            map.borrow()
                .values()
                .filter(|entry| entry.meta.belongs_to(identity))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    pages
        .iter()
        .flat_map(|entry| {
            entry.meta.rooted_ids().filter_map(move |id| {
                let (pointer, allocation) = entry.projection.resolve_slot(id.slot())?;
                (allocation.id() == &id).then_some((pointer, allocation))
            })
        })
        .collect()
}

/// Use the input address only to select an owned slot. The returned pointer
/// is freshly projected from that slot's canonical Cell, never from the
/// caller's potentially provenance-free or invalidated pointer tag.
#[inline]
pub(crate) fn checked_projection(pointer: SEXP) -> Option<(SEXP, CheckedNode)> {
    let (entry, slot) = find_slab_entry(pointer)?;
    entry.projection.resolve_slot(slot)
}

/// Admit an address once and snapshot the same live generation through its
/// canonical Cell. This operation is callback-free and retains the physical
/// page until the copied header and exact metadata identity are captured.
#[inline]
pub(crate) fn checked_header(pointer: SEXP) -> Option<(SEXP, CheckedNode, SexprecCore)> {
    let (entry, slot) = find_slab_entry(pointer)?;
    entry.projection.copy_slot(slot)
}

/// A copied header and an optional original weak reader for a multi-slot page.
/// Single-slot permanent pages need no collection-local reader cache.
pub(crate) struct CheckedPageSnapshot {
    pub(crate) projection: SEXP,
    pub(crate) header: SexprecCore,
    pub(crate) reader: Option<NodeProjection<SexprecCore>>,
}

pub(crate) fn checked_page_snapshot(
    pointer: SEXP,
    expected: &CheckedNode,
) -> Option<CheckedPageSnapshot> {
    let (entry, slot) = find_slab_entry(pointer)?;
    if expected.id().slot() != slot {
        return None;
    }
    let (projection, header) = entry.projection.copy_live_projected(expected.id())?;
    let reader = (entry.meta.slots() > 1).then_some(entry.projection);
    Some(CheckedPageSnapshot {
        projection,
        header,
        reader,
    })
}

/// Copy the canonical header through its Cell only if the caller's original
/// allocation identity still belongs to this exact directory slot. An
/// address reused by another generation never refreshes the expected ID.
#[inline]
pub(crate) fn checked_snapshot(pointer: SEXP, expected: &CheckedNode) -> Option<SexprecCore> {
    let (entry, slot) = find_slab_entry(pointer)?;
    if expected.id().slot() != slot {
        return None;
    }
    entry.projection.copy_live(expected.id())
}
