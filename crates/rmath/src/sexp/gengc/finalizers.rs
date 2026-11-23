//! gengc — extracted verbatim from gengc.rs.

use super::*;

/// Newly ready finalizers become roots during this collection, after the
/// initial mark pass. Trace their complete graph before deciding what to free.
/// Keys alone are insufficient: a child can have been queued for sweeping too.
///
/// # Safety
///
/// `inst`/`instance` must point to a live `RInstance` (as supplied by the instance accessors) with
/// no conflicting outstanding borrow of the fields touched here.
pub(crate) unsafe fn retain_newly_ready_finalizer_graph(
    instance: *mut instance::RInstance,
    to_free: &mut Vec<SEXP>,
    promote_young: bool,
) -> usize {
    if to_free.is_empty() {
        return 0;
    }
    unsafe {
        let unreachable: HashSet<usize> = to_free.iter().map(|&obj| obj as usize).collect();
        // Snapshot original pointers, preserving provenance and ending the
        // queue borrow before recursive traversal touches the owning graph.
        let ready_roots =
            crate::mainutils::memory_main::ready_finalizer_roots_in(instance, &unreachable);
        if ready_roots.is_empty() {
            return 0;
        }
        for root in ready_roots {
            mark_reachable(root);
        }
        let mut promoted = 0;
        to_free.retain(|obj| {
            if !super::super::memory::arena_node_marked(*obj) {
                return true;
            }
            if promote_young && (**obj).sxpinfo.gcgen() == Generation::Young as u8 {
                (**obj).sxpinfo.set_gcgen(Generation::Old as u8);
                promoted += 1;
            }
            (**obj).sxpinfo.set_mark(false);
            false
        });
        promoted
    }
}
