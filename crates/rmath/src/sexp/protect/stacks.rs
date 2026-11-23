#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
#![deny(unsafe_op_in_unsafe_fn)]
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use super::snapshot::RootSnapshot;
use super::*;

/// Error returned when a safe protection API receives a handle whose owner was
/// not validated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProtectError {
    UnownedHandle { api: &'static str, owner: SexpOwner },
    ForeignOwner,
    StaleSlot,
    StaleAllocation,
    Allocation,
    GenerationExhausted,
    OwnerUnavailable,
}

impl std::fmt::Display for ProtectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForeignOwner => f.write_str("root replacement belongs to a different owner"),
            Self::StaleSlot => f.write_str("root slot has been released or reused"),
            Self::StaleAllocation => f.write_str("root allocation has been reclaimed or reused"),
            Self::Allocation => f.write_str("protection storage allocation failed"),
            Self::GenerationExhausted => f.write_str("root generation exhausted"),
            Self::OwnerUnavailable => f.write_str("root owner has been destroyed"),
            ProtectError::UnownedHandle { api, owner } => {
                write!(f, "{api}: SEXP handle is not owner-scoped ({owner:?})")
            }
        }
    }
}

impl std::error::Error for ProtectError {}

// ---------------------------------------------------------------------------
// The two storages
// ---------------------------------------------------------------------------

/// The C-port protection stack: strict LIFO, count-based release.
///
/// This is the storage behind the translated `Rf_protect`/`Rf_unprotect`/
/// `UNPROTECT_PTR`/`R_ProtectCount` entry points ([`protect_raw_pointer`],
/// [`unprotect_count_in`], [`unprotect_ptr_in`], [`R_ProtectCount_in`]) and
/// the count-shaped RAII wrapper [`protect_n`]. It reproduces C R semantics:
///
/// * pushes append at the top;
/// * `Rf_unprotect(n)` TRUNCATES the top `n` entries — later entries shift
///   down, so any handle into the middle of the stack is invalidated exactly
///   as in C;
/// * `R_ProtectCount` is this stack's length and nothing else.
///
/// LIFO discipline is the CALLER's responsibility (as in C): popping more
/// than was pushed since the last marker, or dropping a [`protect_n`] guard
/// out of order, releases the wrong entries and is not detected. Rust-side
/// handles that need arbitrary drop order must use the [`RootTable`] APIs
/// instead — the two storages never alias, so a legacy `unprotect` can never
/// truncate a root slot and a root release can never shift legacy entries.
/// The common canonical occupants of ordered native root channels.
pub(crate) struct NativeRoots {
    pub(crate) entries: RefCell<RootSequence<RootValue>>,
}
impl NativeRoots {
    fn new(ledger: crate::sexp::memory::MemoryLedger) -> Self {
        Self {
            entries: RefCell::new(RootSequence::new(ledger)),
        }
    }
    fn len(&self) -> usize {
        self.entries.borrow().len()
    }
    pub(crate) fn try_push(&self, value: RootValue) -> Result<SequenceLease, ProtectError> {
        let value = value.canonical()?;
        self.entries
            .borrow_mut()
            .try_push(value)
            .map_err(storage_error)
    }
    fn clear(&self) {
        self.entries.borrow_mut().clear();
    }
    // Marking only enqueues these copied roots. It finishes this iterator
    // before tracing may execute any R/provider/notification callbacks.
    fn checked_roots_for_marking(&self) -> impl Iterator<Item = RootValue> + '_ {
        (0..self.len()).filter_map(move |index| {
            let entries = self.entries.borrow();
            let value = entries.at(index)?;
            value.is_live().then(|| value.clone())
        })
    }
    fn copy_native_entries(&self, output: &mut [SEXP]) -> Option<usize> {
        let entries = self.entries.borrow();
        let len = entries.len();
        let output = output.get_mut(..len)?;
        for ((_, value), target) in entries.entries().zip(output) {
            *target = value.projection().unwrap_or(std::ptr::null_mut());
        }
        Some(len)
    }
    fn checked_entries_snapshot(&self) -> RootSnapshot<RootValue> {
        snapshot_or_error({
            let entries = self.entries.borrow();
            let count = entries
                .entries()
                .filter(|(_, value)| value.is_live())
                .count();
            RootSnapshot::try_collect(
                entries.ledger(),
                count,
                entries
                    .entries()
                    .filter(|(_, value)| value.is_live())
                    .map(|(_, value)| value.clone()),
            )
        })
    }
    pub(crate) fn entries_snapshot(&self) -> RootSnapshot<SEXP> {
        snapshot_or_error({
            let entries = self.entries.borrow();
            RootSnapshot::try_collect(
                entries.ledger(),
                entries.len(),
                entries
                    .entries()
                    .map(|(_, value)| value.projection().unwrap_or(std::ptr::null_mut())),
            )
        })
    }
    pub(crate) fn updates_snapshot(&self) -> RootSnapshot<(SequenceLease, RootValue)> {
        snapshot_or_error({
            let entries = self.entries.borrow();
            RootSnapshot::try_collect(
                entries.ledger(),
                entries.len(),
                entries
                    .entries()
                    .map(|(lease, value)| (lease, value.clone())),
            )
        })
    }
    pub(crate) fn replace_if_current(
        &self,
        lease: SequenceLease,
        expected: &RootValue,
        value: RootValue,
    ) {
        self.entries
            .borrow_mut()
            .replace_if_current(lease, expected, value);
    }
}
// Sweep uses a fixed map, not a callback. Keep each existing lease in place
// and reject stale/foreign projections without borrowing the active arena.
fn remap_root_values<'a>(
    roots: impl Iterator<Item = &'a mut RootValue>,
    heap: &crate::sexp::heap::HeapIdentity,
    old_to_new: &HashMap<usize, SEXP>,
) -> Result<(), ProtectError> {
    for root in roots {
        let Some(old) = root.projection() else { continue; };
        let Some(&new) = old_to_new.get(&old.addr()) else { continue; };
        if old != new {
            *root = RootValue::from_heap(heap, new)?;
        }
    }
    Ok(())
}

// Evaluate the preparation block first: all table loans end before error hooks.
fn snapshot_or_error<T>(result: Result<RootSnapshot<T>, StorageError>) -> RootSnapshot<T> {
    result
        .unwrap_or_else(|_| crate::sexp::context::r_error("protection snapshot allocation failed"))
}

fn storage_error(error: StorageError) -> ProtectError {
    match error {
        StorageError::Allocation => ProtectError::Allocation,
        StorageError::GenerationExhausted => ProtectError::GenerationExhausted,
    }
}

pub(crate) struct LegacyProtectionStack {
    roots: NativeRoots,
}
impl LegacyProtectionStack {
    pub(crate) fn copy_native_entries(&self, output: &mut [SEXP]) -> Option<usize> {
        self.roots.copy_native_entries(output)
    }
    #[cfg(test)]
    pub(crate) fn roots_for_test(&self) -> &NativeRoots {
        &self.roots
    }
    pub(crate) fn new(ledger: crate::sexp::memory::MemoryLedger) -> Self {
        Self {
            roots: NativeRoots::new(ledger),
        }
    }
    pub(crate) fn len(&self) -> usize {
        self.roots.len()
    }
    pub(crate) fn push(&self, value: RootValue, api: &str) {
        self.roots.try_push(value).unwrap_or_else(|error| {
            if error == ProtectError::Allocation {
                crate::sexp::context::r_error(format!("{api}: {error}"));
            }
            panic!("{api}: {error}")
        });
    }
    pub(crate) fn pop_count(&self, count: usize) {
        self.roots.entries.borrow_mut().pop_count(count);
    }
    pub(crate) fn truncate(&self, depth: usize) {
        self.roots.entries.borrow_mut().truncate(depth);
    }
    pub(crate) fn remove_topmost(&self, value: &RootValue) {
        self.roots.entries.borrow_mut().remove_last(value);
    }
    pub(crate) fn clear(&self) {
        self.roots.clear();
    }
    pub(crate) fn checked_roots_for_marking(&self) -> impl Iterator<Item = RootValue> + '_ {
        self.roots.checked_roots_for_marking()
    }
    pub(crate) fn checked_entries_snapshot(&self) -> RootSnapshot<RootValue> {
        self.roots.checked_entries_snapshot()
    }
    pub(crate) fn entries_snapshot(&self) -> RootSnapshot<SEXP> {
        self.roots.entries_snapshot()
    }
    pub(crate) fn updates_snapshot(&self) -> RootSnapshot<(SequenceLease, RootValue)> {
        self.roots.updates_snapshot()
    }
    pub(crate) fn replace_if_current(
        &self,
        lease: SequenceLease,
        expected: &RootValue,
        replacement: RootValue,
    ) {
        self.roots.replace_if_current(lease, expected, replacement);
    }
    pub(crate) fn with_entries<R>(&self, f: impl FnOnce(&[SEXP]) -> R) -> R {
        let entries = self.entries_snapshot();
        f(&entries)
    }
}

/// Preserved objects keep native first-match release semantics; Rust guards
/// release the precise lease they installed, independently of duplicate values.
pub(crate) struct PreservedRoots {
    pub(crate) roots: NativeRoots,
}
impl PreservedRoots {
    pub(crate) fn new(ledger: crate::sexp::memory::MemoryLedger) -> Self {
        Self {
            roots: NativeRoots::new(ledger),
        }
    }
    pub(crate) fn try_push(&self, value: RootValue) -> Result<SequenceLease, ProtectError> {
        self.roots.try_push(value)
    }
    pub(crate) fn remove_first(&self, value: &RootValue) {
        self.roots.entries.borrow_mut().remove_first(value);
    }
    pub(crate) fn remove_lease(&self, lease: SequenceLease) {
        self.roots.entries.borrow_mut().remove_lease(lease);
    }
    pub(crate) fn clear(&self) {
        self.roots.clear();
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.roots.entries.borrow().is_empty()
    }
    pub(crate) fn checked_roots_for_marking(&self) -> impl Iterator<Item = RootValue> + '_ {
        self.roots.checked_roots_for_marking()
    }
    pub(crate) fn checked_entries_snapshot(&self) -> RootSnapshot<RootValue> {
        self.roots.checked_entries_snapshot()
    }
    pub(crate) fn entries_snapshot(&self) -> RootSnapshot<SEXP> {
        self.roots.entries_snapshot()
    }
    pub(crate) fn updates_snapshot(&self) -> RootSnapshot<(SequenceLease, RootValue)> {
        self.roots.updates_snapshot()
    }
    pub(crate) fn replace_if_current(
        &self,
        lease: SequenceLease,
        expected: &RootValue,
        replacement: RootValue,
    ) {
        self.roots.replace_if_current(lease, expected, replacement);
    }
    #[cfg(test)]
    pub(crate) fn push_for_test(&self, value: RootValue) {
        self.try_push(value).unwrap();
    }
}

/// A root retains the exact allocation generation, independently of its native
/// address. Shared process singletons have a separate immutable policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RootValue {
    Checked {
        projection: SEXP,
        allocation: crate::sexp::heap::CheckedNode,
    },
    Static {
        projection: SEXP,
    },
}

impl RootValue {
    pub(crate) fn is_live(&self) -> bool {
        match self {
            Self::Checked { allocation, .. } => allocation.is_live(),
            Self::Static { .. } => true,
        }
    }

    pub(crate) fn projection(&self) -> Option<SEXP> {
        if !self.is_live() {
            return None;
        }
        Some(match self {
            Self::Checked { projection, .. } | Self::Static { projection } => *projection,
        })
    }

    fn canonical(self) -> Result<Self, ProtectError> {
        match self {
            Self::Checked {
                projection,
                allocation,
            } => {
                if !allocation.is_live() {
                    return Err(ProtectError::StaleAllocation);
                }
                let (projection, current) = crate::sexp::memory::checked_projection(projection)
                    .ok_or(ProtectError::StaleAllocation)?;
                if current != allocation {
                    return Err(ProtectError::StaleAllocation);
                }
                Ok(Self::Checked {
                    projection,
                    allocation,
                })
            }
            Self::Static { projection } => {
                let projection = crate::sexp::session::immutable_singleton_projection(projection)
                    .ok_or(ProtectError::ForeignOwner)?;
                Ok(Self::Static { projection })
            }
        }
    }

    /// The raw owner is a live scoped native boundary. No input header is read.
    pub(crate) unsafe fn from_owner(
        inst: *mut RInstance,
        input: SEXP,
    ) -> Result<Self, ProtectError> {
        // SAFETY: `inst` is a live instance pointer supplied by the caller;
        // only its heap identity field is read, without lending it onward.
        let heap = unsafe { &(*inst).heap_identity };
        Self::from_heap(heap, input)
    }

    /// Build a root value against an already-captured heap identity.
    ///
    /// This is the checked spelling of [`RootValue::from_owner`]: callers
    /// holding a `GuardOwner` (or any live `HeapIdentity`) authenticate the
    /// input without reconstructing an instance reference.
    pub(crate) fn from_heap(
        heap: &crate::sexp::heap::HeapIdentity,
        input: SEXP,
    ) -> Result<Self, ProtectError> {
        if let Some(projection) = crate::sexp::session::immutable_singleton_projection(input) {
            return Ok(Self::Static { projection });
        }
        // The registered arena may already be mutably lent. Recover the
        // canonical Cell projection from shared directory metadata, then lend
        // only the independent original heap identity to authenticate it.
        let (projection, allocation) =
            crate::sexp::memory::checked_projection(input).ok_or(ProtectError::ForeignOwner)?;
        if !allocation.belongs_to(heap) {
            return Err(ProtectError::ForeignOwner);
        }
        Self::Checked {
            projection,
            allocation,
        }
        .canonical()
    }
}

/// The single canonical Rust root store. Lease identity and allocation identity
/// are distinct: recycling either a root slot or a heap address cannot revive an
/// older lease. Native projections are copied only from live checked entries.
pub(crate) struct RootTable {
    storage: RefCell<RootStorage<RootValue>>,
}

impl RootTable {
    #[cfg(test)]
    pub(crate) fn physical_storage_for_test(&self) -> (usize, usize) {
        self.storage.borrow().physical_storage_for_test()
    }
    pub(crate) fn new(ledger: super::super::memory::MemoryLedger) -> Self {
        Self {
            storage: RefCell::new(RootStorage::new(ledger)),
        }
    }
    pub(crate) fn checkpoint(&self) -> u64 {
        self.storage.borrow().checkpoint()
    }
    fn retain_managed(&self, slot: ProtectionSlot) {
        if let Some(slot) = slot.storage_id() {
            self.storage.borrow_mut().retain_managed(slot);
        }
    }
    pub(crate) fn restore(&self, checkpoint: u64) {
        self.storage.borrow_mut().restore(checkpoint);
    }
    #[cfg(any(test, kani))]
    pub(crate) fn set_next_generation_for_test(&self, value: u64) {
        self.storage
            .borrow_mut()
            .set_next_generation_for_test(value);
    }
    pub(crate) fn try_claim(
        &self,
        value: RootValue,
        managed: bool,
    ) -> Result<ProtectionSlot, ProtectError> {
        let value = value.canonical()?;
        let id = self
            .storage
            .borrow_mut()
            .try_claim(value, managed)
            .map_err(storage_error)?;
        Ok(ProtectionSlot::from_stack_index(id.index, id.generation))
    }
    pub(crate) fn claim(&self, value: RootValue, api: &str) -> (usize, u64) {
        let slot = self
            .try_claim(value, false)
            .unwrap_or_else(|error| panic!("{api}: {error}"));
        (slot.index.expect("claimed root slot"), slot.generation)
    }
    pub(crate) fn release(&self, slot: ProtectionSlot) {
        if let Some(id) = slot.storage_id() {
            self.storage.borrow_mut().release(id);
        }
    }
    pub(crate) fn reprotect(
        &self,
        slot: ProtectionSlot,
        value: RootValue,
    ) -> Result<(), ProtectError> {
        let value = value.canonical()?;
        let id = slot.storage_id().ok_or(ProtectError::StaleSlot)?;
        let mut storage = self.storage.borrow_mut();
        if !storage.get(id).is_some_and(RootValue::is_live) {
            return Err(ProtectError::StaleSlot);
        }
        storage.replace(id, value);
        Ok(())
    }
    pub(crate) fn generation_at(&self, slot: ProtectionSlot) -> Option<u64> {
        let index = slot.index?;
        let storage = self.storage.borrow();
        let (id, value) = storage.at(index)?;
        value.is_live().then_some(id.generation)
    }
    pub(crate) fn len(&self) -> usize {
        self.storage.borrow().len()
    }
    pub(crate) fn truncate(&self, depth: usize) {
        self.storage.borrow_mut().truncate(depth);
    }
    pub(crate) fn clear(&self) {
        self.storage.borrow_mut().clear();
    }
    pub(crate) fn checked_roots_for_marking(&self) -> impl Iterator<Item = RootValue> + '_ {
        (0..self.len()).filter_map(move |index| {
            let storage = self.storage.borrow();
            let (_, value) = storage.at(index)?;
            value.is_live().then(|| value.clone())
        })
    }
    pub(crate) fn copy_native_entries(&self, output: &mut [SEXP]) -> Option<usize> {
        let storage = self.storage.borrow();
        let len = storage.len();
        let output = output.get_mut(..len)?;
        for (index, target) in output.iter_mut().enumerate() {
            *target = storage
                .at(index)
                .and_then(|(_, value)| value.projection())
                .unwrap_or(std::ptr::null_mut());
        }
        Some(len)
    }
    pub(crate) fn checked_entries_snapshot(&self) -> RootSnapshot<RootValue> {
        snapshot_or_error({
            let storage = self.storage.borrow();
            let count = storage
                .entries()
                .filter(|(_, value)| value.is_live())
                .count();
            RootSnapshot::try_collect(
                storage.ledger(),
                count,
                storage
                    .entries()
                    .filter(|(_, value)| value.is_live())
                    .map(|(_, value)| value.clone()),
            )
        })
    }
    pub(crate) fn entries_snapshot(&self) -> RootSnapshot<SEXP> {
        snapshot_or_error({
            let storage = self.storage.borrow();
            RootSnapshot::try_collect(
                storage.ledger(),
                storage.len(),
                (0..storage.len()).map(|index| {
                    storage
                        .at(index)
                        .and_then(|(_, value)| value.projection())
                        .unwrap_or(std::ptr::null_mut())
                }),
            )
        })
    }
    pub(crate) fn with_entries<R>(&self, f: impl FnOnce(&[SEXP]) -> R) -> R {
        let entries = self.entries_snapshot();
        f(&entries)
    }
    pub(crate) fn updates_snapshot(&self) -> RootSnapshot<(SlotId, RootValue)> {
        snapshot_or_error({
            let storage = self.storage.borrow();
            RootSnapshot::try_collect(
                storage.ledger(),
                storage.entries().count(),
                storage.entries().map(|(slot, value)| (slot, value.clone())),
            )
        })
    }
    pub(crate) fn replace_if_current(&self, slot: SlotId, expected: &RootValue, value: RootValue) {
        let mut storage = self.storage.borrow_mut();
        if storage.get(slot) == Some(expected) {
            storage.replace(slot, value);
        }
    }
}

// ---------------------------------------------------------------------------
// Guards — owner discipline and thread confinement
// ---------------------------------------------------------------------------

/// Thread-confinement marker for protection guards.
///
/// Guards act on their OWNING instance through the weak store handles in
/// [`GuardOwner`] and release into per-instance storages guarded only
/// by `RefCell` — none of that is thread-safe, and the ambient
/// current-instance machinery is thread-local. `PhantomData<*mut ()>` opts
/// guard types out of `Send` and `Sync` at compile time, so a guard can
/// never be moved to (or borrowed from) another thread and dropped against
/// the wrong session.
pub(crate) type Confined<'a> = PhantomData<(&'a (), *mut ())>;

/// Weak handles to the original owner's protection stores. Guards keep neither
/// the runtime nor the stores alive, and skip cleanup after revocation.
///
/// # Owner discipline (the type-level contract)
///
/// Guards must act on their owning instance even when the ambient current
/// instance has switched to another session. Each store is captured as a
/// [`Weak`] into the `Rc` the owning [`RInstance`] holds, so a guard upgrades
/// to a shared borrow of that exact store — never a stale address or a
/// reused session's store.
///
/// Creation requires a live owner. A weak allocation-lifetime witness skips
/// cleanup when that exact owner is destroyed, without keeping its storage alive
/// or mistaking a reused address for the original instance. [`Confined`] keeps
/// guards on the owning thread. Release-time staleness is guarded
/// separately: root-slot releases check the slot's generation
/// ([`RootTable::release`]) so a guard whose entry was already recycled or
/// unwound is a no-op instead of evicting the live owner.
#[derive(Clone)]
pub(crate) struct GuardOwner {
    liveness: crate::sexp::instance::InstanceLiveness,
    heap: crate::sexp::heap::HeapIdentity,
    legacy_protect: Weak<LegacyProtectionStack>,
    root_table: Weak<RootTable>,
    preserve_stack: Weak<PreservedRoots>,
}
impl GuardOwner {
    /// # Safety
    /// `pointer` is the live owner projection for the duration of this capture.
    pub(crate) unsafe fn new(pointer: *mut RInstance) -> Self {
        // SAFETY: capture only shared field projections from the live owner.
        unsafe {
            Self {
                liveness: crate::sexp::instance::instance_liveness(pointer),
                heap: (*pointer).heap_identity.clone(),
                legacy_protect: Rc::downgrade(&(*pointer).legacy_protect),
                root_table: Rc::downgrade(&(*pointer).root_table),
                preserve_stack: Rc::downgrade(&(*pointer).preserve_stack),
            }
        }
    }

    /// Capture the ambient active owner.
    ///
    /// This is the checked spelling of [`GuardOwner::new`] for callers that
    /// act on the current session: the ambient entry supplies the live
    /// pinned instance, so no unsafe contract is left for the caller.
    pub(crate) fn current() -> Self {
        crate::sexp::instance::with_required_current_instance(|inst| unsafe { Self::new(inst) })
    }

    /// Capture a guard owner from an already-managed weak owner.
    ///
    /// No raw-pointer authority is required: the weak owner proves liveness
    /// and supplies its own pinned projection for the capture.
    pub(crate) fn from_weak(owner: &crate::sexp::owner::WeakOwner) -> Option<Self> {
        let pin = owner.pin().ok()?;
        // SAFETY: the pin keeps the original allocation alive through capture.
        Some(unsafe { Self::new(pin.as_ptr()) })
    }

    pub(crate) fn is_live(&self) -> bool {
        self.liveness.is_live()
    }

    /// A clone of the owner's teardown witness for interleaved liveness
    /// checks between callbacks.
    pub(crate) fn liveness(&self) -> crate::sexp::instance::InstanceLiveness {
        self.liveness.clone()
    }

    /// Upgrade a weak store handle when the owning instance is still live.
    ///
    /// Returns `None` after revocation so cleanup skips the destroyed owner
    /// instead of touching recycled memory.
    fn live_store<T>(&self, store: &Weak<T>) -> Option<Rc<T>> {
        if !self.is_live() {
            return None;
        }
        store.upgrade()
    }

    /// Run `f` on this owner's legacy protection stack while it is live.
    pub(in crate::sexp) fn with_legacy_protect<R>(
        &self,
        f: impl FnOnce(&LegacyProtectionStack) -> R,
    ) -> Option<R> {
        self.live_store(&self.legacy_protect).map(|stack| f(&stack))
    }

    /// Run `f` on this owner's generational root table while it is live.
    pub(in crate::sexp) fn with_root_table<R>(&self, f: impl FnOnce(&RootTable) -> R) -> Option<R> {
        self.live_store(&self.root_table).map(|table| f(&table))
    }

    /// Run `f` on this owner's preserved-roots stack while it is live.
    pub(in crate::sexp) fn with_preserve_stack<R>(
        &self,
        f: impl FnOnce(&PreservedRoots) -> R,
    ) -> Option<R> {
        self.live_store(&self.preserve_stack).map(|stack| f(&stack))
    }

    /// Depth checkpoints of both protection stores for scope-style restore.
    pub(crate) fn protection_checkpoint(&self) -> (usize, u64) {
        (
            self.legacy_len(),
            self.with_root_table(RootTable::checkpoint)
                .expect("live protection owner"),
        )
    }

    /// Truncate both protection stores back to a [`Self::protection_checkpoint`].
    pub(crate) fn restore_protection(&self, legacy_depth: usize, root_depth: u64) {
        self.with_legacy_protect(|stack| stack.truncate(legacy_depth));
        self.with_root_table(|table| table.restore(root_depth));
    }

    /// Apply a collector-owned remap without allocating a callback snapshot.
    pub(crate) fn remap_for_gc(&self, old_to_new: &HashMap<usize, SEXP>) -> Result<(), ProtectError> {
        if old_to_new.is_empty() {
            return Ok(());
        }
        self.with_legacy_protect(|stack| {
            remap_root_values(stack.roots.entries.borrow_mut().values_mut(), &self.heap, old_to_new)
        }).ok_or(ProtectError::OwnerUnavailable)??;
        self.with_root_table(|table| {
            remap_root_values(table.storage.borrow_mut().values_mut(), &self.heap, old_to_new)
        }).ok_or(ProtectError::OwnerUnavailable)??;
        self.with_preserve_stack(|stack| {
            remap_root_values(stack.roots.entries.borrow_mut().values_mut(), &self.heap, old_to_new)
        }).ok_or(ProtectError::OwnerUnavailable)?
    }

    /// Authenticate `s` against this owner's captured heap identity.
    pub(crate) fn root_value(&self, s: SEXP) -> Result<RootValue, ProtectError> {
        RootValue::from_heap(&self.heap, s)
    }

    /// Push `s` onto this owner's legacy protection stack.
    pub(crate) fn push_legacy(&self, s: SEXP) {
        let value = self.root_value(s).expect("invalid protection owner");
        self.with_legacy_protect(|stack| stack.push(value, "protect"));
    }

    /// Pop the top `n` entries from this owner's legacy stack.
    pub(crate) fn pop_legacy_count(&self, n: usize) {
        self.with_legacy_protect(|stack| stack.pop_count(n));
    }

    /// Remove the topmost legacy entry holding `s` on this owner.
    pub(crate) fn remove_topmost_legacy(&self, s: SEXP) {
        let value = self.root_value(s).expect("invalid unprotect owner");
        self.with_legacy_protect(|stack| stack.remove_topmost(&value));
    }

    /// Current depth of this owner's legacy protection stack.
    pub(crate) fn legacy_len(&self) -> usize {
        self.with_legacy_protect(|stack| stack.len()).unwrap_or(0)
    }

    /// Copy this owner's legacy native projections into `output` when it
    /// fits, returning the entry count.
    pub(crate) fn legacy_copy_native_entries(&self, output: &mut [SEXP]) -> Option<usize> {
        self.with_legacy_protect(|stack| stack.copy_native_entries(output))
            .flatten()
    }

    /// Snapshot this owner's legacy entries as native projections.
    pub(crate) fn legacy_entries_snapshot(&self) -> Option<RootSnapshot<SEXP>> {
        self.with_legacy_protect(|stack| stack.entries_snapshot())
    }

    /// Snapshot this owner's legacy leases for a relocating update pass.
    pub(crate) fn legacy_updates_snapshot(
        &self,
    ) -> Option<RootSnapshot<(SequenceLease, RootValue)>> {
        self.with_legacy_protect(|stack| stack.updates_snapshot())
    }

    /// Replace `expected` with `replacement` on this owner's legacy stack
    /// when the lease still names the entry.
    pub(crate) fn legacy_replace_if_current(
        &self,
        lease: SequenceLease,
        expected: &RootValue,
        replacement: RootValue,
    ) {
        self.with_legacy_protect(|stack| {
            stack.replace_if_current(lease, expected, replacement);
        });
    }

    /// Claim a generational root-table slot for `s` on this owner.
    pub(crate) fn try_protect_value(
        &self,
        s: SEXP,
        managed: bool,
    ) -> Result<ProtectionSlot, ProtectError> {
        let value = self.root_value(s)?;
        self.with_root_table(|table| table.try_claim(value, managed))
            .unwrap_or(Err(ProtectError::OwnerUnavailable))
    }

    /// Replace `slot` through this owner's checked store.
    pub(crate) fn try_reprotect_value(
        &self,
        slot: ProtectionSlot,
        s: SEXP,
    ) -> Result<(), ProtectError> {
        let value = self.root_value(s)?;
        self.with_root_table(|table| table.reprotect(slot, value))
            .unwrap_or(Err(ProtectError::OwnerUnavailable))
    }

    /// Release `slot` from this owner's root table.
    pub(crate) fn release_root(&self, slot: ProtectionSlot) {
        self.with_root_table(|table| table.release(slot));
    }

    /// The generation currently recorded for `slot` on this owner.
    pub(crate) fn root_generation_at(&self, slot: ProtectionSlot) -> Option<u64> {
        self.with_root_table(|table| table.generation_at(slot))
            .flatten()
    }

    /// Copy this owner's root-table native projections into `output` when it
    /// fits, returning the entry count.
    pub(crate) fn root_copy_native_entries(&self, output: &mut [SEXP]) -> Option<usize> {
        self.with_root_table(|table| table.copy_native_entries(output))
            .flatten()
    }

    /// Snapshot this owner's root-table entries as native projections.
    pub(crate) fn root_entries_snapshot(&self) -> Option<RootSnapshot<SEXP>> {
        self.with_root_table(|table| table.entries_snapshot())
    }

    /// Snapshot this owner's root-table slots for a relocating update pass.
    pub(crate) fn root_updates_snapshot(&self) -> Option<RootSnapshot<(SlotId, RootValue)>> {
        self.with_root_table(|table| table.updates_snapshot())
    }

    /// Replace `expected` with `replacement` in this owner's root table when
    /// the slot still names the entry.
    pub(crate) fn root_replace_if_current(
        &self,
        slot: SlotId,
        expected: &RootValue,
        replacement: RootValue,
    ) {
        self.with_root_table(|table| {
            table.replace_if_current(slot, expected, replacement);
        });
    }

    /// Push `s` onto this owner's preserve stack, returning its lease.
    pub(crate) fn try_preserve(&self, s: SEXP) -> Result<Option<SequenceLease>, ProtectError> {
        let value = self.root_value(s)?;
        self.with_preserve_stack(|stack| stack.try_push(value).map(Some))
            .unwrap_or(Err(ProtectError::OwnerUnavailable))
    }

    /// Remove the first preserve entry holding `s` on this owner.
    pub(crate) fn remove_preserved(&self, s: SEXP) {
        let value = self.root_value(s).expect("invalid preserve release owner");
        self.with_preserve_stack(|stack| {
            stack.remove_first(&value);
        });
    }

    /// Remove exactly `lease` from this owner's preserve stack.
    pub(crate) fn remove_preserve_lease(&self, lease: SequenceLease) {
        self.with_preserve_stack(|stack| {
            stack.remove_lease(lease);
        });
    }

    /// Snapshot this owner's preserve entries as native projections.
    pub(crate) fn preserve_entries_snapshot(&self) -> Option<RootSnapshot<SEXP>> {
        self.with_preserve_stack(|stack| stack.entries_snapshot())
    }

    /// Snapshot this owner's preserve leases for a relocating update pass.
    pub(crate) fn preserve_updates_snapshot(
        &self,
    ) -> Option<RootSnapshot<(SequenceLease, RootValue)>> {
        self.with_preserve_stack(|stack| stack.updates_snapshot())
    }

    /// Replace `expected` with `replacement` on this owner's preserve stack
    /// when the lease still names the entry.
    pub(crate) fn preserve_replace_if_current(
        &self,
        lease: SequenceLease,
        expected: &RootValue,
        replacement: RootValue,
    ) {
        self.with_preserve_stack(|stack| {
            stack.replace_if_current(lease, expected, replacement);
        });
    }
}

/// How a [`ProtectGuard`] releases its protection at drop.
pub(crate) enum GuardRelease {
    /// Pop `n` entries off the owner's [`LegacyProtectionStack`].
    ///
    /// Count-based C semantics: requires LIFO drop order (see
    /// [`LegacyProtectionStack`]); nothing checks it, exactly like the
    /// translated C the wrapper was made for.
    LegacyCount(usize),
    /// Release this exact generational slot in the owner's [`RootTable`].
    /// The release is generation-checked: if the slot was already released,
    /// recycled, or cut away by a scope unwind, drop is a no-op.
    RootSlot(ProtectionSlot),
}

/// RAII guard for a protection entry; automatically unprotects when dropped.
///
/// The guard carries weak handles into its owner's root stores (see
/// [`GuardOwner`]) plus the release token for the entry it owns:
/// * [`protect_sexp`] / [`protect`] / [`try_protect_sexp`] push one
///   generational [`RootTable`] slot — the guard may be dropped in ANY
///   order relative to other root guards;
/// * [`protect_n`] wraps `n` legacy-stack entries — those must unwind LIFO.
///
/// The guard is `!Send + !Sync` ([`Confined`]): it must be dropped on the
/// thread that owns its session.
///
/// ```rust,ignore
/// use rmath::sexp::protect::protect_sexp;
///
/// let guard = protect_sexp(some_sexp);
/// // ... do work ...
/// // guard automatically unprotects when it goes out of scope
/// ```
pub(crate) struct ProtectGuard<'a> {
    /// Weak handles into the owning instance's protection stores — see
    /// [`GuardOwner`].
    pub(crate) owner: Option<GuardOwner>,
    pub(crate) release: GuardRelease,
    pub(crate) _confined: Confined<'a>,
}
