#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
#![deny(unsafe_op_in_unsafe_fn)]
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
#[derive(Default)]
pub(crate) struct NativeRoots {
    pub(crate) entries: RefCell<RootSequence<RootValue>>,
}
impl NativeRoots {
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
    fn checked_entries_snapshot(&self) -> Vec<RootValue> {
        self.entries
            .borrow()
            .entries()
            .filter(|(_, value)| value.is_live())
            .map(|(_, value)| value.clone())
            .collect()
    }
    pub(crate) fn entries_snapshot(&self) -> Vec<SEXP> {
        self.entries
            .borrow()
            .entries()
            .map(|(_, value)| value.projection().unwrap_or(std::ptr::null_mut()))
            .collect()
    }
    pub(crate) fn updates_snapshot(&self) -> Vec<(SequenceLease, RootValue)> {
        self.entries
            .borrow()
            .entries()
            .map(|(lease, value)| (lease, value.clone()))
            .collect()
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
fn storage_error(error: StorageError) -> ProtectError {
    match error {
        StorageError::Allocation => ProtectError::Allocation,
        StorageError::GenerationExhausted => ProtectError::GenerationExhausted,
    }
}

#[derive(Default)]
pub(crate) struct LegacyProtectionStack {
    roots: NativeRoots,
}
impl LegacyProtectionStack {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn len(&self) -> usize {
        self.roots.len()
    }
    pub(crate) fn push(&self, value: RootValue, api: &str) {
        self.roots
            .try_push(value)
            .unwrap_or_else(|error| panic!("{api}: {error}"));
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
    pub(crate) fn checked_entries_snapshot(&self) -> Vec<RootValue> {
        self.roots.checked_entries_snapshot()
    }
    pub(crate) fn entries_snapshot(&self) -> Vec<SEXP> {
        self.roots.entries_snapshot()
    }
    pub(crate) fn updates_snapshot(&self) -> Vec<(SequenceLease, RootValue)> {
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
#[derive(Default)]
pub(crate) struct PreservedRoots {
    pub(crate) roots: NativeRoots,
}
impl PreservedRoots {
    pub(crate) fn new() -> Self {
        Self::default()
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
    pub(crate) fn checked_entries_snapshot(&self) -> Vec<RootValue> {
        self.roots.checked_entries_snapshot()
    }
    pub(crate) fn entries_snapshot(&self) -> Vec<SEXP> {
        self.roots.entries_snapshot()
    }
    pub(crate) fn updates_snapshot(&self) -> Vec<(SequenceLease, RootValue)> {
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
        if let Some(projection) = crate::sexp::session::immutable_singleton_projection(input) {
            return Ok(Self::Static { projection });
        }
        let projection =
            unsafe { (*inst).canonical_projection(input) }.ok_or(ProtectError::ForeignOwner)?;
        let allocation =
            unsafe { (*inst).node_token(projection) }.ok_or(ProtectError::StaleAllocation)?;
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
#[derive(Default)]
pub(crate) struct RootTable {
    storage: RefCell<RootStorage<RootValue>>,
}

impl RootTable {
    pub(crate) fn new() -> Self {
        Self::default()
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
    pub(crate) fn checked_entries_snapshot(&self) -> Vec<RootValue> {
        self.storage
            .borrow()
            .entries()
            .filter(|(_, value)| value.is_live())
            .map(|(_, value)| value.clone())
            .collect()
    }
    pub(crate) fn entries_snapshot(&self) -> Vec<SEXP> {
        let storage = self.storage.borrow();
        (0..storage.len())
            .map(|index| {
                storage
                    .at(index)
                    .and_then(|(_, value)| value.projection())
                    .unwrap_or(std::ptr::null_mut())
            })
            .collect()
    }
    pub(crate) fn with_entries<R>(&self, f: impl FnOnce(&[SEXP]) -> R) -> R {
        let entries = self.entries_snapshot();
        f(&entries)
    }
    pub(crate) fn updates_snapshot(&self) -> Vec<(SlotId, RootValue)> {
        self.storage
            .borrow()
            .entries()
            .map(|(slot, value)| (slot, value.clone()))
            .collect()
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
/// Guards act on their OWNING instance (stored as an address, see
/// [`with_guard_owner`]) and release into per-instance storages guarded only
/// by `RefCell` — none of that is thread-safe, and the ambient
/// current-instance machinery is thread-local. `PhantomData<*mut ()>` opts
/// guard types out of `Send` and `Sync` at compile time, so a guard can
/// never be moved to (or borrowed from) another thread and dropped against
/// the wrong session.
pub(crate) type Confined<'a> = PhantomData<(&'a (), *mut ())>;

/// Run `f` with a guard's owning instance.
///
/// # Owner discipline (the type-level contract)
///
/// Guards must act on their owning instance even when the ambient current
/// instance has switched to another session. The owner retains the original
/// session allocation's pointer provenance in a `NonNull<RInstance>` handle;
/// it is never reconstructed from an integer address. Cleanup uses raw place
/// accesses to the owner's `RefCell` fields without creating a new exclusive
/// borrow of the complete instance.
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
    pointer: std::ptr::NonNull<RInstance>,
    liveness: crate::sexp::instance::InstanceLiveness,
    runtime_owner: Option<crate::sexp::owner::WeakOwner>,
}
impl GuardOwner {
    /// Capture a weak lifetime witness while this native owner is live.
    pub(crate) unsafe fn new(pointer: *mut RInstance) -> Self {
        Self {
            pointer: std::ptr::NonNull::new(pointer).expect("live guard owner"),
            liveness: unsafe { crate::sexp::instance::instance_liveness(pointer) },
            runtime_owner: unsafe { (*pointer).runtime_owner.clone() },
        }
    }
    pub(crate) fn is_live(&self) -> bool {
        self.liveness.is_live()
    }
}

pub(crate) fn with_guard_owner<R>(owner: &GuardOwner, f: impl FnOnce(*mut RInstance) -> R) -> R {
    assert!(owner.is_live(), "root owner has been destroyed");
    let pin = owner
        .runtime_owner
        .as_ref()
        .map(|owner| owner.pin().expect("live guard owner"));
    f(pin
        .as_ref()
        .map_or(owner.pointer.as_ptr(), |pin| pin.as_ptr()))
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
/// The guard carries its owner as a raw address (see [`with_guard_owner`])
/// plus the release token for the entry it owns:
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
    /// Provenance-preserving owning instance handle — see
    /// [`with_guard_owner`].
    pub(crate) owner: Option<GuardOwner>,
    pub(crate) release: GuardRelease,
    pub(crate) _confined: Confined<'a>,
}
