//! gengc — extracted verbatim from gengc.rs.

use super::*;

// ---------------------------------------------------------------------------
// Remembered Set
// ---------------------------------------------------------------------------

/// Remembered set tracking old objects with references to young objects.
///
/// Membership lives in owned state (`members`), never in the SEXP mark bit.
/// The mark bit belongs to the collector's mark phase: setting it at barrier
/// time made the remembered-set scans in `do_minor_gc_in` /
/// `mark_from_all_roots_in` early-return on `mark_reachable`, so the young
/// children of a remembered old object were never traced and could be swept
/// while still reachable (plans/001-separate-remembered-set-membership.md).
#[derive(Default)]
pub struct RememberedSet {
    entries: Vec<(SEXP, super::super::heap::CheckedNode)>,
    members: HashSet<super::super::heap::NodeId>,
    #[cfg(test)]
    fail_next_reservation: bool,
}

impl RememberedSet {
    #[inline]
    pub fn add(&mut self, obj: SEXP) {
        if !self.try_add(obj) {
            // Legacy raw setters may already have published the pointer.
            // Continuing after a missing barrier could leave dangling edges;
            // these infallible callers follow Rust's fatal OOM contract.
            std::alloc::handle_alloc_error(std::alloc::Layout::new::<SEXP>());
        }
    }

    /// Record an edge before publishing it. On failure, membership and
    /// entries remain unchanged so the caller can reject the write.
    #[inline]
    pub(crate) fn try_add(&mut self, obj: SEXP) -> bool {
        let Some((projection, allocation)) = super::super::memory::checked_projection(obj) else {
            return true;
        };
        let Some(header) = super::super::memory::checked_snapshot(projection, &allocation) else {
            return true;
        };
        if header.sxpinfo.gcgen() == Generation::Young as u8 {
            return true;
        }
        if self.members.contains(allocation.id()) {
            return true;
        }
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_reservation) {
            return false;
        }
        // Reserve both collections before mutating either, so an allocation
        // failure cannot leave entries and membership out of sync.
        if self.entries.try_reserve(1).is_err() || self.members.try_reserve(1).is_err() {
            return false;
        }
        self.members.insert(allocation.id().clone());
        self.entries.push((projection, allocation));
        true
    }

    #[cfg(test)]
    pub(crate) fn fail_next_reservation_for_test(&mut self) {
        self.fail_next_reservation = true;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.members.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.entries
            .iter()
            .filter_map(|(projection, allocation)| allocation.is_live().then_some(*projection))
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Remap entries through a relocation map (non-moving sweep redirects
    /// freed addresses to `R_NilValue`) and rebuild membership so a later
    /// barrier cannot deduplicate against a stale address.
    pub fn remap(&mut self, old_to_new: &HashMap<usize, SEXP>) {
        self.entries.retain_mut(|(projection, allocation)| {
            if !allocation.is_live() {
                return false;
            }
            if let Some(&replacement) = old_to_new.get(&(*projection as usize)) {
                let Some((canonical, current)) =
                    super::super::memory::checked_projection(replacement)
                else {
                    return false;
                };
                if !allocation.same_heap(&current) {
                    return false;
                }
                *projection = canonical;
                *allocation = current;
            }
            true
        });
        self.rebuild_membership();
    }

    pub(crate) fn retain_live(&mut self, freed: &HashSet<usize>) {
        self.entries.retain(|(projection, allocation)| {
            allocation.is_live() && !freed.contains(&(*projection as usize))
        });
        self.rebuild_membership();
    }

    fn rebuild_membership(&mut self) {
        self.members.clear();
        self.members.extend(
            self.entries
                .iter()
                .map(|(_, allocation)| allocation.id().clone()),
        );
    }

    pub(crate) fn checked_roots(&self) -> Vec<RootValue> {
        self.entries
            .iter()
            .filter(|(_, allocation)| allocation.is_live())
            .map(|(projection, allocation)| RootValue::Checked {
                projection: *projection,
                allocation: allocation.clone(),
            })
            .collect()
    }
}
