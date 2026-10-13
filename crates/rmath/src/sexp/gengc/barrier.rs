//! gengc — extracted verbatim from gengc.rs.

use super::*;

// ---------------------------------------------------------------------------
// Write Barriers
// ---------------------------------------------------------------------------

#[inline(always)]
pub fn write_barrier(parent: SEXP, child: SEXP) {
    if parent.is_null() || child.is_null() {
        return;
    }

    let Some(instance) = instance::current_instance_ptr() else {
        return;
    };
    // Inputs are address lookups only; the checked barrier copies canonical
    // cells after validating both allocation identities and their owner.
    if !unsafe { write_barrier_in(instance, parent, child) } {
        // A valid edge may have been published by an infallible setter.
        // Invalid inputs are rejected before any reservation is attempted.
        if checked_barrier_pair(instance, parent, child).is_some() {
            std::alloc::handle_alloc_error(std::alloc::Layout::new::<SEXP>());
        }
    }
}

pub(crate) fn checked_barrier_pair(
    instance: *mut instance::RInstance,
    parent: SEXP,
    child: SEXP,
) -> Option<(SEXP, u8, u8)> {
    let (parent, parent_id) = super::super::memory::checked_projection(parent)?;
    let (child, child_id) = super::super::memory::checked_projection(child)?;
    // SAFETY: only callers with a live owner invoke this local helper. The
    // immutable identity is independent of an outstanding arena lend.
    let identity = unsafe { &(*instance).heap_identity };
    if !parent_id.belongs_to(identity) || !child_id.belongs_to(identity) {
        return None;
    }
    let parent_header = super::super::memory::checked_snapshot(parent, &parent_id)?;
    let child_header = super::super::memory::checked_snapshot(child, &child_id)?;
    Some((
        parent,
        parent_header.sxpinfo.gcgen(),
        child_header.sxpinfo.gcgen(),
    ))
}

/// Record an old-to-young edge in the original owner's remembered set.
/// Does not activate a session, collect, or call the evaluator.
///
/// # Safety
/// `instance` is live; parent and child are live nodes owned by that instance
/// or immutable storage. No borrow of its GC state overlaps this operation.
#[inline]
pub(crate) unsafe fn write_barrier_in(
    instance: *mut instance::RInstance,
    parent: SEXP,
    child: SEXP,
) -> bool {
    if parent.is_null() || child.is_null() {
        return true;
    }
    // Immutable singleton edges never introduce a young allocation.
    if super::super::globals::immutable_singleton_projection(parent).is_some()
        || super::super::globals::immutable_singleton_projection(child).is_some()
    {
        return true;
    }
    let Some((parent, parent_gen, child_gen)) = checked_barrier_pair(instance, parent, child)
    else {
        return false;
    };
    if parent_gen == Generation::Old as u8 && child_gen == Generation::Young as u8 {
        with_gc_state_in(instance, |state| state.remembered_set.try_add(parent))
    } else {
        true
    }
}

#[inline(always)]
pub fn vector_write_barrier(vec: SEXP, index: usize, value: SEXP) {
    write_barrier(vec, value);
}

#[inline(always)]
pub fn list_write_barrier(list: SEXP, field: u8, value: SEXP) {
    write_barrier(list, value);
}

#[inline(always)]
pub fn attrib_write_barrier(obj: SEXP, value: SEXP) {
    write_barrier(obj, value);
}

// ---------------------------------------------------------------------------
// Generation Promotion
// ---------------------------------------------------------------------------

#[inline]
pub unsafe fn promote_to_old(obj: SEXP) {
    if obj.is_null() || super::super::globals::immutable_singleton_projection(obj).is_some() {
        return;
    }
    unsafe {
        debug_assert!((*obj).sxpinfo.gcgen() == Generation::Young as u8);
        (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
    }
}
