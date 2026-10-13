//! gengc — extracted verbatim from gengc.rs.

use super::*;

/// Newly ready finalizers become roots during this collection, after the
/// initial mark pass. Trace their complete graph before deciding what to free.
/// Keys alone are insufficient: a child can have been queued for sweeping too.
pub(crate) fn retain_newly_ready_finalizer_graph(
    instance: *mut instance::RInstance,
    to_free: &mut Vec<SEXP>,
    promote_young: bool,
) -> usize {
    if to_free.is_empty() {
        return 0;
    }
    unsafe {
        let unreachable: HashSet<usize> = to_free.iter().map(|&obj| obj as usize).collect();
        let keys = crate::mainutils::memory_main::mark_finalizers_ready_for_unreachable_in(
            &mut (*instance).memory_state,
            &unreachable,
        );
        if keys.is_empty() {
            return 0;
        }
        // Copy original pointers, preserving provenance and ending the memory
        // state borrow before recursive traversal touches the owning graph.
        let ready_roots: Vec<SEXP> = (*instance)
            .memory_state
            .pending_finalizers
            .iter()
            .filter(|finalizer| finalizer.is_ready())
            .map(|finalizer| finalizer.obj())
            .collect();
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
