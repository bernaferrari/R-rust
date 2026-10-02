#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
#![deny(unsafe_op_in_unsafe_fn)]

//! R's PROTECT/UNPROTECT mechanism, split into two storages.
//!
//! R maintains a protection stack to prevent GC from collecting objects that
//! are only referenced by local variables. This port keeps TWO storages with
//! deliberately different disciplines:
//!
//! # Two storages, two disciplines
//!
//! * **[`LegacyProtectionStack`]** — the C-port stack: a strict LIFO
//!   `Vec<SEXP>` for translated `Rf_protect` / `Rf_unprotect` /
//!   `UNPROTECT_PTR` / `R_ProtectCount` code ([`protect_raw_pointer`],
//!   [`unprotect_count`], [`unprotect_ptr`], [`protect_n`]). Release is
//!   COUNT-BASED: `Rf_unprotect(n)` truncates the top `n` entries and later
//!   entries shift down, exactly like C R. [`R_ProtectCount`] reflects ONLY
//!   this stack's depth. RAII wrappers over it ([`protect_n`]) must unwind
//!   in LIFO order — the same discipline as the C code they translate; a
//!   violation is a caller bug, not a detected error (as in C).
//! * **[`RootTable`]** — the Rust root table: stable, generational slots for
//!   Rust-side handles ([`ProtectGuard`] from [`protect_sexp`] / [`protect`],
//!   [`IndexedProtectGuard`] / [`protect_sexp_with_index`], [`RootedSexp`],
//!   and the `R_ProtectWithIndex` shim). Slots are released BY SLOT ID +
//!   GENERATION in ANY order; a released slot is tombstoned in place (null
//!   pointer + a fresh generation) and its index recycled via a free list,
//!   so surviving guards keep resolving to their own entries and a stale
//!   handle is detectable ([`ProtectionSlot::is_stale`]) instead of silently
//!   aliasing another entry's protection.
//!
//! The two storages never alias: count-based legacy ops cannot truncate root
//! slots and root release never shifts legacy entries. The collector (gengc)
//! explicitly scans BOTH storages at mark and update time.
//!
//! # Ownership model
//!
//! * [`Sexp`](super::object::Sexp) handles are **non-`Copy`**: assigning a
//!   handle moves it, and aliasing the same R object requires an explicit
//!   [`Clone`](Sexp::clone) (a cheap second handle over identical memory,
//!   never a deep copy).
//! * Holding a handle alone does **not** root the object: the non-moving /
//!   generational GC may collect anything only reachable from Rust locals
//!   once an R evaluation re-enters. To retain a value across a GC point,
//!   push it on a protection storage.
//! * [`RootedSexp`] is the ergonomic RAII rooting layer: it clones the
//!   handle, roots it on creation, and releases the root on [`Drop`],
//!   exposing reads through [`RootedSexp::get`]. Lower-level callers can use
//!   [`protect_sexp`]/[`ProtectGuard`] or the replaceable-slot
//!   [`protect_sexp_with_index`]/[`IndexedProtectGuard`] directly.
//! * Handles to objects that may be *replaced* during evaluation (grown
//!   vectors, PROMSXP re-promises) must be refreshed through the write
//!   barrier — re-derive the handle from its owner or use
//!   [`IndexedProtectGuard::reprotect_sexp`] on a slot-protected value —
//!   never by mutating a raw pointer in place.
//!
//! # Slot-stability contract (root table)
//!
//! The root table is a `Vec` with stable slot indices. Releasing a slot
//! (`IndexedProtectGuard` / [`RootedSexp`] / [`ProtectGuard`] drop)
//! tombstones its entry (null pointer + a fresh generation) and pushes the
//! index onto a per-instance free list instead of shifting later entries, so
//! guards may be dropped in **any order** — surviving guards keep resolving
//! to their own entries. A later slot push reuses a freed index with a fresh
//! generation. Every pushed entry is tagged with a generation from a
//! monotonic per-instance counter, so a slot handle whose entry was released
//! is detectable via [`ProtectionSlot::is_stale`] /
//! [`RootedSexp::is_stale`] instead of silently resolving to another entry's
//! protection. Freed TAIL slots collapse off the table (popped together with
//! their generation tags) so the physical Vec stays contiguous; interior
//! frees stay tombstoned beneath live entries.

use std::cell::RefCell;
use std::marker::PhantomData;

use super::ffi::SEXP;
use super::instance::{RInstance, with_required_current_instance};
use super::object::{Sexp, SexpOwner};

#[path = "root_storage.rs"]
mod root_storage;
use root_storage::{RootStorage, SlotId, StorageError};

/// Error returned when a safe protection API receives a handle whose owner was
/// not validated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectError {
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
#[derive(Default)]
pub(crate) struct LegacyProtectionStack {
    entries: RefCell<Vec<SEXP>>,
}

impl LegacyProtectionStack {
    pub(crate) fn new() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
        }
    }

    /// Number of entries currently on the stack (`R_ProtectCount`).
    pub(crate) fn len(&self) -> usize {
        // P2: strictly-local RefCell read; no ambient write intervenes.
        self.entries.borrow().len()
    }

    /// Push `s` on the top of the stack.
    pub(crate) fn push(&self, s: SEXP, api: &str) {
        // P2: strictly-local RefCell access. `try_reserve` may allocate and
        // panic, but neither reenters the interpreter nor touches the
        // instance through another raw path.
        let mut entries = self.entries.borrow_mut();
        reserve_slot_or_fail(&mut entries, api);
        entries.push(s);
    }

    /// Truncate the top `n` entries (`Rf_unprotect(n)`). Popping at least
    /// the whole stack clears it.
    pub(crate) fn pop_count(&self, n: usize) {
        if n == 0 {
            return;
        }
        // P2: strictly-local RefCell access; no ambient write intervenes.
        let mut entries = self.entries.borrow_mut();
        let keep = entries.len().saturating_sub(n);
        entries.truncate(keep);
    }

    /// Remove the topmost entry equal to `s` (`UNPROTECT_PTR`).
    pub(crate) fn remove_topmost(&self, s: SEXP) {
        // P2: strictly-local RefCell access; no ambient write intervenes.
        let mut entries = self.entries.borrow_mut();
        if let Some(pos) = entries.iter().rposition(|&x| x == s) {
            entries.remove(pos);
        }
    }

    /// Unwind to a recorded depth (session `ProtectScope` teardown).
    pub(crate) fn truncate(&self, depth: usize) {
        // P2: strictly-local RefCell access; no ambient write intervenes.
        self.entries.borrow_mut().truncate(depth);
    }

    /// Bulk reset (instance/test harness teardown).
    pub(crate) fn clear(&self) {
        // P2: strictly-local RefCell access; no ambient write intervenes.
        self.entries.borrow_mut().clear();
    }

    /// Run `f` over the live entries in push order.
    pub(crate) fn with_entries<R>(&self, f: impl FnOnce(&[SEXP]) -> R) -> R {
        // P2: the RefCell read borrow (and the &[SEXP] handed to f) covers
        // the legacy stack buffer only; callers mark/update SEXP objects
        // elsewhere, never this Vec's allocation.
        let entries = self.entries.borrow().clone();
        f(&entries)
    }

    fn entries_snapshot(&self) -> Vec<SEXP> {
        self.entries.borrow().clone()
    }
    fn replace_if_current(&self, index: usize, expected: SEXP, replacement: SEXP) {
        let mut entries = self.entries.borrow_mut();
        if entries.get(index) == Some(&expected) {
            entries[index] = replacement;
        }
    }
}

/// A root retains the exact allocation generation, independently of its native
/// address. Shared process singletons have a separate immutable policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RootValue {
    Checked {
        projection: SEXP,
        allocation: super::heap::CheckedNode,
    },
    Static {
        projection: SEXP,
    },
}

impl RootValue {
    fn is_live(&self) -> bool {
        match self {
            Self::Checked { allocation, .. } => allocation.is_live(),
            Self::Static { .. } => true,
        }
    }

    fn projection(&self) -> Option<SEXP> {
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
                let (projection, current) = super::memory::checked_projection(projection)
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
                let projection = super::session::immutable_singleton_projection(projection)
                    .ok_or(ProtectError::ForeignOwner)?;
                Ok(Self::Static { projection })
            }
        }
    }

    /// The raw owner is a live scoped native boundary. No input header is read.
    unsafe fn from_owner(inst: *mut RInstance, input: SEXP) -> Result<Self, ProtectError> {
        if let Some(projection) = super::session::immutable_singleton_projection(input) {
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
    fn try_claim(&self, value: RootValue, managed: bool) -> Result<ProtectionSlot, ProtectError> {
        let value = value.canonical()?;
        let id = self
            .storage
            .borrow_mut()
            .try_claim(value, managed)
            .map_err(|error| match error {
                StorageError::Allocation => ProtectError::Allocation,
                StorageError::GenerationExhausted => ProtectError::GenerationExhausted,
            })?;
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
    fn reprotect(&self, slot: ProtectionSlot, value: RootValue) -> Result<(), ProtectError> {
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
            .filter_map(|(_, value)| value.is_live().then(|| value.clone()))
            .collect()
    }
    pub(crate) fn with_checked_entries<R>(&self, f: impl FnOnce(&[RootValue]) -> R) -> R {
        let entries = self.checked_entries_snapshot();
        f(&entries)
    }
    fn entries_snapshot(&self) -> Vec<SEXP> {
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
    fn updates_snapshot(&self) -> Vec<(SlotId, RootValue)> {
        self.storage
            .borrow()
            .entries()
            .map(|(slot, value)| (slot, value.clone()))
            .collect()
    }
    fn replace_if_current(&self, slot: SlotId, expected: &RootValue, value: RootValue) {
        let mut storage = self.storage.borrow_mut();
        if storage.get(slot) == Some(expected) {
            storage.replace(slot, value);
        }
    }
}

fn reserve_slot_or_fail(stack: &mut Vec<SEXP>, api: &str) {
    if stack.try_reserve(1).is_err() {
        panic!("{api}: protection stack allocation failed");
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
type Confined<'a> = PhantomData<(&'a (), *mut ())>;

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
/// Soundness relies on the owner instance outliving every guard created
/// against it — the session APIs keep the instance alive across the scoped
/// interpreter call that owns the guard — and on the [`Confined`] marker
/// keeping guards on the owning thread. Release-time staleness is guarded
/// separately: root-slot releases check the slot's generation
/// ([`RootTable::release`]) so a guard whose entry was already recycled or
/// unwound is a no-op instead of evicting the live owner.
#[derive(Clone)]
struct GuardOwner {
    pointer: std::ptr::NonNull<RInstance>,
    liveness: super::instance::InstanceLiveness,
}
impl GuardOwner {
    /// Capture a weak lifetime witness while this native owner is live.
    unsafe fn new(pointer: *mut RInstance) -> Self {
        Self {
            pointer: std::ptr::NonNull::new(pointer).expect("live guard owner"),
            liveness: unsafe { super::instance::instance_liveness(pointer) },
        }
    }
    fn is_live(&self) -> bool {
        self.liveness.is_live()
    }
}

fn with_guard_owner<R>(owner: &GuardOwner, f: impl FnOnce(*mut RInstance) -> R) -> R {
    assert!(owner.is_live(), "root owner has been destroyed");
    f(owner.pointer.as_ptr())
}

/// How a [`ProtectGuard`] releases its protection at drop.
enum GuardRelease {
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
pub struct ProtectGuard<'a> {
    /// Provenance-preserving owning instance handle — see
    /// [`with_guard_owner`].
    owner: Option<GuardOwner>,
    release: GuardRelease,
    _confined: Confined<'a>,
}

impl Drop for ProtectGuard<'_> {
    fn drop(&mut self) {
        let Some(owner) = self.owner.as_ref().filter(|owner| owner.is_live()) else {
            return;
        };
        match self.release {
            GuardRelease::LegacyCount(n) if n > 0 => {
                // SAFETY: See with_guard_owner; the owning session keeps the
                // instance alive across the scoped interpreter call that owns
                // the guard.
                with_guard_owner(owner, |inst| unprotect_count_in(inst, n));
            }
            GuardRelease::RootSlot(slot) if slot.is_active() => {
                // SAFETY: See with_guard_owner.
                with_guard_owner(owner, |inst| release_protect_slot_in(inst, slot));
            }
            _ => {}
        }
    }
}

/// Protect an owner-scoped SEXP handle and return an RAII guard.
///
/// This is the Rust API exposed to embedders. Raw pointer protection remains
/// crate-local translation scaffolding for ported interpreter modules.
pub fn protect_sexp<'a>(value: Sexp<'a>) -> ProtectGuard<'a> {
    try_protect_sexp(value).expect("protect_sexp requires an owner-scoped Sexp")
}

/// Try to protect an owner-scoped SEXP handle.
pub fn try_protect_sexp<'a>(value: Sexp<'a>) -> Result<ProtectGuard<'a>, ProtectError> {
    ensure_owner_scoped(value.clone(), "protect_sexp")?;
    let owner = session_owner_handle(&value);
    let slot = try_claim_value(&value, owner.as_ref())?;
    Ok(ProtectGuard {
        owner,
        release: GuardRelease::RootSlot(slot),
        _confined: PhantomData,
    })
}

fn try_claim_value(
    value: &Sexp<'_>,
    owner: Option<&GuardOwner>,
) -> Result<ProtectionSlot, ProtectError> {
    let Some(owner) = owner else {
        return Ok(ProtectionSlot::inactive());
    };
    if !owner.is_live() {
        return Err(ProtectError::OwnerUnavailable);
    }
    let raw = value.clone().as_raw();
    with_guard_owner(owner, |inst| try_protect_raw_with_slot_in(inst, raw, true))
}

/// Protect a raw SEXP and return an RAII guard.
///
/// Legacy compatibility helper for translated code. Prefer
/// [`protect_sexp`] when the caller has an owner-scoped value. The guard
/// holds a generational root-table slot and may be dropped in any order.
pub(crate) unsafe fn protect(s: SEXP) -> ProtectGuard<'static> {
    protect_raw(s)
}

fn protect_raw(s: SEXP) -> ProtectGuard<'static> {
    if s.is_null() {
        return ProtectGuard {
            owner: None,
            release: GuardRelease::LegacyCount(0),
            _confined: PhantomData,
        };
    }

    with_required_current_instance(|inst| {
        // SAFETY: guard creation requires an active instance; `inst` is it.
        let slot = unsafe { protect_raw_with_slot_in(inst, s, "protect") };
        ProtectGuard {
            owner: Some(unsafe { GuardOwner::new(inst) }),
            release: GuardRelease::RootSlot(slot),
            _confined: PhantomData,
        }
    })
}

/// Create a guard that will pop `n` LEGACY-stack entries on drop.
///
/// Callers must already have pushed `n` entries onto the legacy protection
/// stack ([`protect_raw_pointer`] et al.) and want RAII-style unwinding
/// safety around a manual protect batch. LIFO drop discipline is required —
/// see [`LegacyProtectionStack`]. The guard never touches the [`RootTable`].
pub(crate) fn protect_n(n: usize) -> ProtectGuard<'static> {
    ProtectGuard {
        owner: if n == 0 {
            None
        } else {
            Some(with_required_current_instance(|inst| unsafe {
                GuardOwner::new(inst)
            }))
        },
        release: GuardRelease::LegacyCount(n),
        _confined: PhantomData,
    }
}

// ---------------------------------------------------------------------------
// Legacy stack entry points (translated Rf_protect / Rf_unprotect family)
// ---------------------------------------------------------------------------

/// Push `s` onto the owning instance's LEGACY protection stack.
///
/// This is the push half of the translated C discipline: pair it with
/// [`unprotect_count_in`] / [`protect_n`] and unwind LIFO. It never touches
/// the [`RootTable`].
pub(crate) fn push_protect_in(inst: *mut RInstance, s: SEXP) {
    if !s.is_null() {
        // SAFETY: `inst` is a live instance pointer from the caller; the
        // RefCell access is strictly local.
        unsafe { (*inst).legacy_protect.push(s, "protect") };
    }
}

fn push_protect(s: SEXP) {
    with_required_current_instance(|inst| push_protect_in(inst, s));
}

/// Push a raw SEXP onto the LEGACY protection stack and return it.
///
/// The equivalent of R's `Rf_protect()`.
pub(crate) unsafe fn protect_raw_pointer(s: SEXP) -> SEXP {
    push_protect(s);
    s
}

/// Pop the top `n` entries from the LEGACY protection stack.
///
/// The equivalent of R's `Rf_unprotect(n)`.
pub(crate) fn unprotect_count_in(inst: *mut RInstance, n: usize) {
    // SAFETY: `inst` is a live instance pointer from the caller; the RefCell
    // access is strictly local.
    unsafe { (*inst).legacy_protect.pop_count(n) };
}

/// Pop the top `n` entries from the protection stack.
pub(crate) fn unprotect_count(n: usize) {
    with_required_current_instance(|inst| unprotect_count_in(inst, n));
}

/// Unprotect the topmost legacy entry holding `s`.
///
/// This is the equivalent of R's `UNPROTECT_PTR()` macro.
pub(crate) fn unprotect_ptr(s: SEXP) {
    with_required_current_instance(|inst| unprotect_ptr_in(inst, s));
}

pub(crate) fn unprotect_ptr_in(inst: *mut RInstance, s: SEXP) {
    if s.is_null() {
        return;
    }
    // SAFETY: `inst` is a live instance pointer from the caller; the RefCell
    // access is strictly local.
    unsafe { (*inst).legacy_protect.remove_topmost(s) };
}

/// Get the current LEGACY protection stack depth.
///
/// `R_ProtectCount` reflects ONLY the [`LegacyProtectionStack`] depth —
/// generational [`RootTable`] slots held by Rust guards are deliberately
/// invisible to translated count-based code. Used by the context system to
/// track protect depth.
pub(crate) fn R_ProtectCount() -> usize {
    with_required_current_instance(R_ProtectCount_in)
}

pub(crate) fn R_ProtectCount_in(inst: *mut RInstance) -> usize {
    // P2: strictly-local RefCell read; no ambient write intervenes.
    // SAFETY: `inst` is a live instance pointer from the caller.
    unsafe { (*inst).legacy_protect.len() }
}

/// Run `f` while temporarily protecting extra roots on the LEGACY stack.
///
/// The `extra` callback pushes entries (via [`push_protect_in`] or
/// [`protect_raw_pointer`]); on exit exactly the number of entries it added
/// is popped, LIFO.
pub(crate) fn with_temporary_extra_protects<F, R>(extra: impl FnOnce(*mut RInstance), f: F) -> R
where
    F: FnOnce() -> R,
{
    with_required_current_instance(|inst| {
        // P2: strictly-local RefCell reads around the caller's extra-root
        // push; f() runs with no instance borrow held.
        // SAFETY: `inst` is the required ambient instance.
        let start = unsafe { (*inst).legacy_protect.len() };
        extra(inst);
        // SAFETY: as above.
        let added = unsafe { (*inst).legacy_protect.len().saturating_sub(start) };
        let result = f();
        unprotect_count_in(inst, added);
        result
    })
}

// ---------------------------------------------------------------------------
// Root-table entry points (Rust guards)
// ---------------------------------------------------------------------------

/// Stable handle for a protected root-table slot that may be replaced with
/// another value before it is unprotected.
///
/// The handle records the generation assigned to the table entry when it was
/// claimed. Releasing a slot tombstones its entry in place (null pointer +
/// fresh generation) without disturbing later slots; later claims reuse the
/// freed index with newer generations, so [`is_stale`](ProtectionSlot::is_stale)
/// detects a handle whose slot was released and handed out again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtectionSlot {
    index: Option<usize>,
    generation: u64,
}

impl ProtectionSlot {
    fn inactive() -> Self {
        Self {
            index: None,
            generation: 0,
        }
    }

    fn storage_id(self) -> Option<SlotId> {
        Some(SlotId {
            index: self.index?,
            generation: self.generation,
        })
    }

    fn from_stack_index(index: usize, generation: u64) -> Self {
        Self {
            index: Some(index),
            generation,
        }
    }

    fn from_legacy_ptr(index: *mut ProtectIndex) -> Self {
        let raw = index as usize;
        if raw == 0 {
            Self::inactive()
        } else {
            // Legacy encoded indices carry no generation; they are transient
            // values passed straight back to `R_Reprotect`, never held long
            // enough to be checked for staleness.
            Self::from_stack_index(raw - 1, 0)
        }
    }

    fn into_legacy_ptr(self) -> *mut ProtectIndex {
        self.index
            .map(|index| (index + 1) as *mut ProtectIndex)
            .unwrap_or(std::ptr::null_mut())
    }

    /// The generation assigned to the table entry when this slot was
    /// created. A released-then-reused slot always reports a different
    /// generation than handles captured before the release.
    pub fn generation(self) -> u64 {
        self.generation
    }

    pub fn is_active(self) -> bool {
        self.index.is_some()
    }

    /// Whether this handle no longer refers to the root-table entry it was
    /// created for: the entry was released and its index handed out again
    /// (or is gone entirely). Inactive slots are never stale.
    pub fn is_stale(self) -> bool {
        with_required_current_instance(|inst| protect_slot_is_stale_in(inst, self))
    }
}

fn protect_raw_with_slot(s: SEXP, api: &str) -> ProtectionSlot {
    with_required_current_instance(|inst| protect_raw_with_slot_in(inst, s, api))
}

/// Claim a generational root-table slot for `s` on `inst`.
fn protect_raw_with_slot_in(inst: *mut RInstance, s: SEXP, api: &str) -> ProtectionSlot {
    try_protect_raw_with_slot_in(inst, s, false).unwrap_or_else(|error| panic!("{api}: {error}"))
}

fn try_protect_raw_with_slot_in(
    inst: *mut RInstance,
    s: SEXP,
    managed: bool,
) -> Result<ProtectionSlot, ProtectError> {
    if s.is_null() {
        return Ok(ProtectionSlot::inactive());
    }
    // SAFETY: all callers retain the live native owner for this local access.
    let value = unsafe { RootValue::from_owner(inst, s) }?;
    unsafe { (*inst).root_table.try_claim(value, managed) }
}

fn reprotect_slot(slot: ProtectionSlot, s: SEXP) {
    with_required_current_instance(|inst| reprotect_slot_in(inst, slot, s));
}

fn reprotect_slot_in(inst: *mut RInstance, slot: ProtectionSlot, s: SEXP) {
    // P2: strictly-local RefCell write; no ambient write intervenes.
    // SAFETY: `inst` is a live instance pointer from the caller.
    if !slot.is_active() {
        return;
    }
    let value = unsafe { RootValue::from_owner(inst, s) }.expect("invalid root replacement");
    let _ = unsafe { (*inst).root_table.reprotect(slot, value) };
}

fn release_protect_slot(slot: ProtectionSlot) {
    with_required_current_instance(|inst| release_protect_slot_in(inst, slot));
}

fn release_protect_slot_in(inst: *mut RInstance, slot: ProtectionSlot) {
    // P2: strictly-local RefCell access; no ambient write intervenes.
    // SAFETY: `inst` is a live instance pointer from the caller.
    unsafe { (*inst).root_table.release(slot) };
}

/// The generation currently recorded for `slot`'s table index, or `None`
/// when the entry is gone.
fn protect_slot_generation_in(inst: *mut RInstance, slot: ProtectionSlot) -> Option<u64> {
    // P2: strictly-local RefCell read; no ambient write intervenes.
    // SAFETY: `inst` is a live instance pointer from the caller.
    unsafe { (*inst).root_table.generation_at(slot) }
}

/// Whether `slot` no longer refers to the table entry it was created for.
fn protect_slot_is_stale_in(inst: *mut RInstance, slot: ProtectionSlot) -> bool {
    if !slot.is_active() {
        return false;
    }
    protect_slot_generation_in(inst, slot) != Some(slot.generation)
}

/// RAII guard for a replaceable root-table slot.
pub struct IndexedProtectGuard<'a> {
    owner: Option<GuardOwner>,
    slot: ProtectionSlot,
    value_owner: SexpOwner,
    _confined: Confined<'a>,
}

impl<'a> IndexedProtectGuard<'a> {
    pub fn slot(&self) -> ProtectionSlot {
        self.slot
    }

    /// Whether the live table entry at the guard's slot index still carries
    /// `expected` as its generation, checked against the guard's OWNING
    /// instance (see [`with_guard_owner`]). Inactive slots (null-SEXP
    /// protections) trivially match — there is no entry to go stale.
    fn slot_generation_is(&self, expected: u64) -> bool {
        match self.owner.as_ref() {
            Some(owner) if self.slot.is_active() => {
                owner.is_live()
                    && with_guard_owner(owner, |inst| {
                        protect_slot_generation_in(inst, self.slot) == Some(expected)
                    })
            }
            _ => true,
        }
    }

    pub(crate) unsafe fn reprotect_raw(&mut self, value: SEXP) {
        if let Some(owner) = self.owner.as_ref().filter(|owner| owner.is_live()) {
            // SAFETY: See ProtectGuard::drop.
            with_guard_owner(owner, |inst| reprotect_slot_in(inst, self.slot, value));
        }
    }

    pub fn reprotect_sexp(&mut self, value: Sexp<'a>) {
        self.try_reprotect_sexp(value)
            .expect("reprotect_sexp requires an owner-scoped Sexp");
    }

    pub fn try_reprotect_sexp(&mut self, value: Sexp<'a>) -> Result<(), ProtectError> {
        ensure_owner_scoped(value.clone(), "reprotect_sexp")?;
        if value.owner() != self.value_owner {
            return Err(ProtectError::ForeignOwner);
        }
        if !self.slot_generation_is(self.slot.generation) {
            return Err(ProtectError::StaleSlot);
        }
        // SAFETY: checked value has the same live owner as this managed slot.
        unsafe { self.reprotect_raw(value.as_raw()) };
        Ok(())
    }
}

impl Drop for IndexedProtectGuard<'_> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.as_ref().filter(|owner| owner.is_live()) {
            // SAFETY: See ProtectGuard::drop.
            with_guard_owner(owner, |inst| release_protect_slot_in(inst, self.slot));
        }
    }
}

/// An owner-scoped [`Sexp`] handle kept alive in the root table.
///
/// `RootedSexp` is the ergonomic rooting layer over the [`RootTable`]:
/// [`RootedSexp::root`] clones the (non-`Copy`) handle, claims a
/// generational slot for it on creation, and releases that slot when the
/// root is dropped, so callers never juggle `protect_sexp`/`unprotect`
/// bookkeeping by hand. Reads go through [`RootedSexp::get`].
///
/// Slots have stable indices: tombstoned entries are reused with a fresh
/// generation, so roots may be dropped in any order and surviving roots keep
/// resolving to their own protections. See the module docs for the
/// slot-stability contract.
///
/// Every root records the generation of the table entry it created; reads
/// through [`RootedSexp::get`] verify that the entry is still the root's
/// own, and [`RootedSexp::is_stale`] reports a released-then-reused slot.
///
/// ```rust,ignore
/// use rmath::sexp::protect::RootedSexp;
///
/// let root = RootedSexp::root(some_sexp.clone());
/// run_gc_point(); // the rooted value survives collection
/// let n = root.get().expect("root is live").length(); // checked read
/// drop(root); // table slot released
/// ```
pub struct RootedSexp<'a> {
    value: Sexp<'a>,
    guard: IndexedProtectGuard<'a>,
    /// Generation of the table entry captured at root creation; verified
    /// against the live entry on every checked read.
    expected_generation: u64,
}

impl std::fmt::Debug for RootedSexp<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootedSexp").finish_non_exhaustive()
    }
}

impl<'a> RootedSexp<'a> {
    /// Protect a clone of `sexp` until the returned root is dropped.
    ///
    /// # Panics
    /// Panics if `sexp` is not owner-scoped (see [`try_root`]).
    ///
    /// [`try_root`]: RootedSexp::try_root
    pub fn root(sexp: Sexp<'a>) -> Self {
        Self::try_root(sexp).expect("RootedSexp::root requires an owner-scoped Sexp")
    }

    /// Like [`root`](RootedSexp::root), but reports unowned handles as
    /// [`ProtectError::UnownedHandle`] instead of panicking.
    pub fn try_root(sexp: Sexp<'a>) -> Result<Self, ProtectError> {
        ensure_owner_scoped(sexp.clone(), "RootedSexp::root")?;
        let guard = try_protect_sexp_with_index(sexp.clone())?;
        let expected_generation = guard.slot().generation();
        Ok(Self {
            value: sexp,
            guard,
            expected_generation,
        })
    }

    /// Read the rooted handle, verifying that the root's table slot still
    /// refers to the protection created for it. This is the only read path:
    /// there is deliberately no `Deref` impl, so stale roots cannot silently
    /// resolve to another entry's protection.
    ///
    /// Returns `None` when the slot was released and handed out again (see
    /// [`is_stale`](RootedSexp::is_stale)); debug builds assert on the
    /// mismatch, release builds degrade to `None`.
    pub fn get(&self) -> Option<&Sexp<'a>> {
        let stale = self.is_stale();
        debug_assert!(
            !stale,
            "RootedSexp slot was released and reused; the root is stale"
        );
        if stale { None } else { Some(&self.value) }
    }

    /// Whether the root's protection slot no longer refers to the table
    /// entry created for it — the root was released (or displaced by an
    /// out-of-order drop) and the slot handed out again. Checked reads via
    /// [`get`](RootedSexp::get) report the mismatch as `None`.
    pub fn is_stale(&self) -> bool {
        !self.value.is_live() || !self.guard.slot_generation_is(self.expected_generation)
    }

    /// The underlying protection slot, for callers that need to reprotect
    /// the rooted value in place (write barrier).
    pub fn slot(&self) -> ProtectionSlot {
        self.guard.slot()
    }

    /// Replace the rooted value through the write barrier: the table slot
    /// now protects `value` and the guarded handle is updated to alias it.
    ///
    /// # Panics
    /// Panics if `value` is not owner-scoped.
    pub fn reprotect(&mut self, value: Sexp<'a>) {
        self.try_reprotect(value)
            .expect("RootedSexp::reprotect requires an owner-scoped Sexp");
    }

    /// Non-panicking variant of [`reprotect`](RootedSexp::reprotect).
    pub fn try_reprotect(&mut self, value: Sexp<'a>) -> Result<(), ProtectError> {
        ensure_owner_scoped(value.clone(), "RootedSexp::reprotect")?;
        self.guard.try_reprotect_sexp(value.clone())?;
        self.value = value;
        Ok(())
    }

    /// Consume the root, returning the guarded handle. The protection is
    /// released; a checked session handle retains its own shared root lease.
    pub fn unroot(self) -> Sexp<'a> {
        let Self { value, guard, .. } = self;
        drop(guard);
        value
    }
}

/// Protect an owner-scoped SEXP handle in a replaceable root-table slot.
pub fn protect_sexp_with_index<'a>(value: Sexp<'a>) -> IndexedProtectGuard<'a> {
    try_protect_sexp_with_index(value)
        .expect("protect_sexp_with_index requires an owner-scoped Sexp")
}

/// Try to protect an owner-scoped SEXP handle in a replaceable root-table
/// slot.
pub fn try_protect_sexp_with_index<'a>(
    value: Sexp<'a>,
) -> Result<IndexedProtectGuard<'a>, ProtectError> {
    ensure_owner_scoped(value.clone(), "protect_sexp_with_index")?;
    let owner = session_owner_handle(&value);
    let slot = try_claim_value(&value, owner.as_ref())?;
    Ok(IndexedProtectGuard {
        owner,
        slot,
        value_owner: value.owner(),
        _confined: PhantomData,
    })
}

/// Protect a raw SEXP in a replaceable root-table slot.
///
/// Legacy compatibility helper for translated Rust modules. Prefer
/// [`protect_sexp_with_index`] when the caller has an owner-scoped value.
pub(crate) unsafe fn protect_with_index_raw(s: SEXP, api: &str) -> IndexedProtectGuard<'static> {
    if s.is_null() {
        return IndexedProtectGuard {
            owner: None,
            slot: ProtectionSlot::inactive(),
            value_owner: SexpOwner::Unknown,
            _confined: PhantomData,
        };
    }

    with_required_current_instance(|inst| IndexedProtectGuard {
        owner: Some(unsafe { GuardOwner::new(inst) }),
        slot: protect_raw_with_slot_in(inst, s, api),
        value_owner: SexpOwner::Unknown,
        _confined: PhantomData,
    })
}

// ---------------------------------------------------------------------------
// R_ProtectWithIndex / R_Reprotect — legacy C shim over the root table
// ---------------------------------------------------------------------------

/// Opaque legacy marker used by the `R_ProtectWithIndex` compatibility shim.
pub(crate) struct ProtectIndex {
    _private: (),
}

/// Protect an SEXP and return a legacy encoded index for later replacement.
///
/// This is the equivalent of R's `R_ProtectWithIndex()`. The claimed entry
/// lives in the generational [`RootTable`]: it is NOT covered by legacy
/// count-based `Rf_unprotect` — release happens when the surrounding session
/// scope unwinds (the `ProtectScope` teardown truncates the root table), so
/// translated code that PROTECTs-with-index and later UNPROTECTs by count
/// would leak the slot until scope exit. No translated module in this port
/// calls this shim; it exists for C-shape compatibility only.
pub(crate) unsafe fn R_ProtectWithIndex(s: SEXP) -> *mut ProtectIndex {
    protect_raw_with_slot(s, "R_ProtectWithIndex").into_legacy_ptr()
}

/// Free a ProtectIndex returned by R_ProtectWithIndex.
///
/// This is a no-op - the index was just a number, not an allocation.
pub(crate) unsafe fn R_FreeProtectIndex(_pi: *mut ProtectIndex) {}

/// Unprotect the entry at the given index and replace it with a new value.
///
/// This is the equivalent of R's `R_Reprotect()`.
pub(crate) unsafe fn R_Reprotect(s: SEXP, index: *mut ProtectIndex) {
    with_required_current_instance(|inst| {
        let mut slot = ProtectionSlot::from_legacy_ptr(index);
        if let Some(generation) = protect_slot_generation_in(inst, slot) {
            slot.generation = generation;
            reprotect_slot_in(inst, slot, s);
        }
    });
}

// ---------------------------------------------------------------------------
// Preserve stack (R_PreserveObject / R_ReleaseObject)
// ---------------------------------------------------------------------------

fn push_preserve_in(inst: *mut RInstance, s: SEXP) {
    try_push_preserve_in(inst, s).unwrap_or_else(|error| panic!("preserve: {error}"));
}

fn try_push_preserve_in(inst: *mut RInstance, s: SEXP) -> Result<(), ProtectError> {
    if !s.is_null() {
        // P2: strictly-local RefCell access; see
        // LegacyProtectionStack::push on try_reserve.
        // SAFETY: `inst` is a live instance pointer from the caller.
        let mut stack = unsafe { (*inst).preserve_stack.borrow_mut() };
        stack.try_reserve(1).map_err(|_| ProtectError::Allocation)?;
        stack.push(s);
    }
    Ok(())
}

fn push_preserve(s: SEXP) {
    with_required_current_instance(|inst| push_preserve_in(inst, s));
}

pub(crate) fn release_preserved_in(inst: *mut RInstance, s: SEXP) {
    if s.is_null() {
        return;
    }
    // P2: strictly-local RefCell access; no ambient write intervenes.
    // SAFETY: `inst` is a live instance pointer from the caller.
    let mut stack = unsafe { (*inst).preserve_stack.borrow_mut() };
    if let Some(pos) = stack.iter().position(|&x| x == s) {
        stack.remove(pos);
    }
}

fn release_preserved(s: SEXP) {
    with_required_current_instance(|inst| release_preserved_in(inst, s));
}

/// RAII guard for the preserve stack.
///
/// Dropping the guard releases the preserved object from the owning session.
/// Like every protection guard it is `!Send + !Sync` ([`Confined`]).
pub struct PreserveGuard<'a> {
    owner: Option<GuardOwner>,
    value: SEXP,
    _confined: Confined<'a>,
}

impl Drop for PreserveGuard<'_> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.as_ref().filter(|owner| owner.is_live()) {
            // SAFETY: See ProtectGuard::drop.
            with_guard_owner(owner, |inst| release_preserved_in(inst, self.value));
        }
    }
}

/// Preserve an owner-scoped SEXP handle until the returned guard is dropped.
pub fn preserve_sexp<'a>(value: Sexp<'a>) -> PreserveGuard<'a> {
    try_preserve_sexp(value).expect("preserve_sexp requires an owner-scoped Sexp")
}

/// Try to preserve an owner-scoped SEXP handle until the returned guard is
/// dropped.
pub fn try_preserve_sexp<'a>(value: Sexp<'a>) -> Result<PreserveGuard<'a>, ProtectError> {
    ensure_owner_scoped(value.clone(), "preserve_sexp")?;
    let raw = value.clone().as_raw();
    if raw.is_null() {
        return Ok(PreserveGuard {
            owner: None,
            value: raw,
            _confined: PhantomData,
        });
    }

    let owner = session_owner_handle(&value);
    if let Some(owner) = owner.as_ref() {
        if !owner.is_live() {
            return Err(ProtectError::OwnerUnavailable);
        }
        with_guard_owner(owner, |inst| try_push_preserve_in(inst, raw))?;
    }
    Ok(PreserveGuard {
        owner,
        value: raw,
        _confined: PhantomData,
    })
}

/// Permanently protect an SEXP from garbage collection.
///
/// Unlike a protection guard, this protection persists until explicitly released.
/// This is the equivalent of R's `R_PreserveObject()`.
pub(crate) unsafe fn R_PreserveObject(s: SEXP) {
    push_preserve(s);
}

/// Release a previously preserved object.
///
/// This is the equivalent of R's `R_ReleaseObject()`.
pub(crate) unsafe fn R_ReleaseObject(s: SEXP) {
    release_preserved(s);
}

// Arena handles borrow their arena, preventing collection through its mutable API.
// Static objects never require roots. Session values are rooted in their own
// instance, independently of the ambient thread-local session.
fn session_owner_handle(value: &Sexp<'_>) -> Option<GuardOwner> {
    match value.owner() {
        SexpOwner::Session(_) => value
            .session_owner_ptr
            .map(|pointer| unsafe { GuardOwner::new(pointer.as_ptr()) }),
        _ => None,
    }
}

fn ensure_owner_scoped(value: Sexp<'_>, api: &'static str) -> Result<(), ProtectError> {
    if !value.is_live() {
        return Err(ProtectError::StaleAllocation);
    }
    if value.clone().is_owner_scoped() {
        Ok(())
    } else {
        Err(ProtectError::UnownedHandle {
            api,
            owner: value.owner(),
        })
    }
}

// ---------------------------------------------------------------------------
// GC integration — the collector scans BOTH storages
// ---------------------------------------------------------------------------

/// Iterate over every protection entry: first the legacy stack (in push
/// order), then the root table (in index order; tombstoned slots hold
/// null). Used by GC-facing tests and diagnostics; the collector's mark and
/// update paths walk the two storages through the same view.
pub(crate) fn with_protected_objects<F, R>(f: F) -> R
where
    F: FnOnce(&[SEXP], &[SEXP]) -> R,
{
    with_required_current_instance(|inst| with_protected_objects_in(inst, f))
}

pub(crate) fn with_protected_objects_in<F, R>(inst: *mut RInstance, f: F) -> R
where
    F: FnOnce(&[SEXP], &[SEXP]) -> R,
{
    // All owner and storage borrows end before the supplied code runs.
    let legacy = unsafe { (*inst).legacy_protect.entries_snapshot() };
    let roots = unsafe { (*inst).root_table.entries_snapshot() };
    f(&legacy, &roots)
}

/// Update all protection entries — legacy stack AND root table — using the
/// given mapping function. Used by the non-moving GC sweep to redirect
/// references to freed objects.
pub(crate) fn update_protect_stack_refs<F>(update_fn: F)
where
    F: FnMut(SEXP) -> SEXP,
{
    with_required_current_instance(|inst| update_protect_stack_refs_in(inst, update_fn));
}

pub(crate) fn update_protect_stack_refs_in<F>(inst: *mut RInstance, mut update_fn: F)
where
    F: FnMut(SEXP) -> SEXP,
{
    // Snapshot native projections and checked identities before callbacks.
    // A callback may remove roots or destroy this owner; neither resurrects an
    // old lease nor permits a subsequent dereference of the destroyed owner.
    let liveness = unsafe { super::instance::instance_liveness(inst) };
    let legacy = unsafe { (*inst).legacy_protect.entries_snapshot() };
    let roots = unsafe { (*inst).root_table.updates_snapshot() };
    for (index, expected) in legacy.into_iter().enumerate() {
        if !liveness.is_live() {
            return;
        }
        let replacement = update_fn(expected);
        if !liveness.is_live() {
            return;
        }
        unsafe {
            (*inst)
                .legacy_protect
                .replace_if_current(index, expected, replacement);
        }
    }
    for (slot, expected) in roots {
        if !liveness.is_live() {
            return;
        }
        let Some(projection) = expected.projection() else {
            continue;
        };
        let replacement = update_fn(projection);
        if !liveness.is_live() {
            return;
        }
        let replacement = unsafe { RootValue::from_owner(inst, replacement) }
            .expect("root update returned an invalid owner projection");
        unsafe {
            (*inst)
                .root_table
                .replace_if_current(slot, &expected, replacement);
        }
    }
}

/// Update all preserve stack references using the given mapping function.
/// Used by non-moving GC sweep to redirect references to freed objects.
pub(crate) fn update_preserve_stack_refs<F>(update_fn: F)
where
    F: FnMut(SEXP) -> SEXP,
{
    with_required_current_instance(|inst| update_preserve_stack_refs_in(inst, update_fn));
}

pub(crate) fn update_preserve_stack_refs_in<F>(inst: *mut RInstance, mut update_fn: F)
where
    F: FnMut(SEXP) -> SEXP,
{
    let liveness = unsafe { super::instance::instance_liveness(inst) };
    let snapshot = unsafe { (*inst).preserve_stack.borrow().clone() };
    for (index, expected) in snapshot.into_iter().enumerate() {
        if !liveness.is_live() {
            return;
        }
        let replacement = update_fn(expected);
        if !liveness.is_live() {
            return;
        }
        let mut stack = unsafe { (*inst).preserve_stack.borrow_mut() };
        if stack.get(index) == Some(&expected) {
            stack[index] = replacement;
        }
    }
}

/// Iterate over all preserved SEXP values.
/// Used by the GC to mark preserved objects.
pub(crate) fn with_preserved_objects<F, R>(f: F) -> R
where
    F: FnOnce(&[SEXP]) -> R,
{
    with_required_current_instance(|inst| with_preserved_objects_in(inst, f))
}

pub(crate) fn with_preserved_objects_in<F, R>(inst: *mut RInstance, f: F) -> R
where
    F: FnOnce(&[SEXP]) -> R,
{
    // P2: as with_protected_objects_in.
    // SAFETY: `inst` is a live instance pointer from the caller.
    let stack = unsafe { (*inst).preserve_stack.borrow().clone() };
    f(&stack)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;
    use std::ptr::addr_of_mut;

    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::instance::{RInstance, current_instance_ptr, replace_current_instance};
    use crate::sexp::session::RSession;

    use super::*;

    // -- Legacy stack -------------------------------------------------------

    #[test]
    fn test_protect_unprotect() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            let fake = token(0x1);
            let result = protect_raw_pointer(fake);
            assert_eq!(result, fake);
            assert_eq!(R_ProtectCount(), 1);
            unprotect_count(1);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_protect_null() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            protect_raw_pointer(ptr::null_mut());
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_protect_multiple() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            let a = token(0x1);
            let b = token(0x2);
            let c = token(0x3);
            protect_raw_pointer(a);
            protect_raw_pointer(b);
            protect_raw_pointer(c);
            assert_eq!(R_ProtectCount(), 3);
            unprotect_count(2);
            assert_eq!(R_ProtectCount(), 1);
            unprotect_count(1);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_ptr() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            let a = token(0x1);
            let b = token(0x2);
            protect_raw_pointer(a);
            protect_raw_pointer(b);
            assert_eq!(R_ProtectCount(), 2);
            unprotect_ptr(a);
            assert_eq!(R_ProtectCount(), 1);
            unprotect_ptr(b);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_ptr_null() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            unprotect_ptr(ptr::null_mut());
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_zero() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| {
            unprotect_count(0);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_exceeds_stack() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            protect_raw_pointer(token(0x1));
            unprotect_count(5);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_protect_n_guard() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| {
            let depth_before = R_ProtectCount();
            unsafe {
                protect_raw_pointer(token(0x1));
                protect_raw_pointer(token(0x2));
                protect_raw_pointer(token(0x3));
            }
            let _guard = protect_n(3);
            assert_eq!(R_ProtectCount(), depth_before + 3);
            drop(_guard);
            assert_eq!(R_ProtectCount(), depth_before);
        });
    }

    #[test]
    fn test_protect_n_guard_drops_against_original_instance() {
        let mut left = RInstance::new_for_gc_tests();
        let mut right = RInstance::new_for_gc_tests();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };

        unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            protect_raw_pointer(token(0x1))
        };
        unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            protect_raw_pointer(token(0x2))
        };
        let guard = protect_n(2);
        assert_eq!(R_ProtectCount_in(addr_of_mut!(left)), 2);

        unsafe {
            replace_current_instance(Some(&mut right));
        }
        drop(guard);

        assert_eq!(R_ProtectCount_in(addr_of_mut!(left)), 0);
        assert_eq!(R_ProtectCount_in(addr_of_mut!(right)), 0);
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_with_protected_objects() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            protect_raw_pointer(token(0x1));
            protect_raw_pointer(token(0x2));
            with_protected_objects(|legacy, roots| {
                assert_eq!(legacy.len(), 2);
                assert!(roots.is_empty());
            });
            unprotect_count(2);
        });
    }

    #[test]
    fn test_update_protect_stack_refs() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            protect_raw_pointer(token(0x1));
            protect_raw_pointer(token(0x2));
            update_protect_stack_refs(|ptr| if ptr == token(0x1) { token(0x100) } else { ptr });
            with_protected_objects(|legacy, _| {
                assert_eq!(legacy[0], token(0x100));
                assert_eq!(legacy[1], token(0x2));
            });
            unprotect_count(2);
        });
    }

    // -- Root-table guards ---------------------------------------------------

    #[test]
    fn test_protect_sexp_guard() {
        let mut session = RSession::new_for_gc_tests();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let (roots_before, legacy_before) =
                with_protected_objects(|legacy, roots| (roots.len(), legacy.len()));
            let guard = protect_sexp(value.clone());
            // Root-table guards are invisible to the count-based legacy API.
            assert_eq!(R_ProtectCount(), legacy_before);
            with_protected_objects(|legacy, roots| {
                assert_eq!(legacy.len(), legacy_before);
                assert_eq!(roots, &[value.clone().as_raw(); 2]);
            });
            assert_eq!(roots_before, 1);
            drop(guard);
            with_protected_objects(|_, roots| assert_eq!(roots.len(), roots_before));
            assert_eq!(R_ProtectCount(), legacy_before);
        });
    }

    #[test]
    fn test_rooted_sexp_roots_and_unroots() {
        let mut session = RSession::new_for_gc_tests();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let root = RootedSexp::root(value.clone());
            with_protected_objects(|_, roots| assert_eq!(roots, &[value.clone().as_raw(); 2]));
            let readback = root.get().expect("fresh root must resolve").clone();
            assert_eq!(readback, value);
            let sexp = root.unroot();
            assert_eq!(sexp.as_raw(), value.clone().as_raw());
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 1));
        });
    }

    #[test]
    fn test_rooted_sexp_reprotect_and_nested_lifo_drop() {
        let mut session = RSession::new_for_gc_tests();
        let (raw_first, raw_second) = session
            .with_arena(|arena| {
                (
                    arena.alloc_node(SEXPTYPE::INTSXP),
                    arena.alloc_node(SEXPTYPE::REALSXP),
                )
            })
            .expect("session should be active");
        let first = session.sexp(raw_first).expect("value belongs to session");
        let second = session.sexp(raw_second).expect("value belongs to session");

        session.with_protected(|| {
            let mut outer = RootedSexp::root(first.clone());
            {
                let inner = RootedSexp::root(first.clone());
                assert!(inner.slot().is_active());
            }
            // The tail drop collapses the inner slot off the table, so the
            // outer root is the only entry left.
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 3));
            outer.reprotect(second.clone());
            with_protected_objects(|_, roots| {
                assert_eq!(roots, &[raw_first, raw_second, raw_second])
            });
            assert_eq!(
                outer.get().expect("outer root must resolve").clone(),
                second
            );
            drop(outer);
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 2));
        });
    }

    #[test]
    fn test_raw_protect_guard_null_legacy() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| {
            let depth_before = R_ProtectCount();
            let guard = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(ptr::null_mut()) };
            assert_eq!(R_ProtectCount(), depth_before);
            drop(guard);
            assert_eq!(R_ProtectCount(), depth_before);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_safe_protect_rejects_unknown_owner() {
        let mut session = RSession::new_for_gc_tests();
        let raw = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(raw) }.expect("raw value should wrap as legacy boundary");

        session.with_protected(|| {
            assert!(matches!(
                try_protect_sexp(value.clone()),
                Err(ProtectError::UnownedHandle {
                    api: "protect_sexp",
                    owner: SexpOwner::Unknown,
                })
            ));
            assert!(matches!(
                try_preserve_sexp(value.clone()),
                Err(ProtectError::UnownedHandle {
                    api: "preserve_sexp",
                    owner: SexpOwner::Unknown,
                })
            ));
            assert!(matches!(
                try_protect_sexp_with_index(value),
                Err(ProtectError::UnownedHandle {
                    api: "protect_sexp_with_index",
                    owner: SexpOwner::Unknown,
                })
            ));
        });
    }

    #[test]
    fn test_preserve_sexp_guard() {
        let mut session = RSession::new_for_gc_tests();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let guard = preserve_sexp(value.clone());
            with_preserved_objects(|objects| assert_eq!(objects, &[value.as_raw()]));
            drop(guard);
            with_preserved_objects(|objects| assert!(objects.is_empty()));
        });
    }

    #[test]
    fn test_indexed_protect_guard_reprotects_and_unwinds() {
        let mut session = RSession::new_for_gc_tests();
        let first = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let second = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::REALSXP))
            .expect("session should be active");
        let first = session.sexp(first).expect("first value belongs to session");
        let second = session
            .sexp(second)
            .expect("second value belongs to session");

        session.with_protected(|| {
            let mut guard = protect_sexp_with_index(first.clone());
            assert!(guard.slot().is_active());
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 3));
            guard.reprotect_sexp(second.clone());
            with_protected_objects(|_, roots| {
                assert_eq!(
                    roots,
                    &[
                        first.clone().as_raw(),
                        second.clone().as_raw(),
                        second.clone().as_raw()
                    ]
                )
            });
            drop(guard);
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 2));
        });
    }

    #[test]
    fn test_indexed_raw_guard_null_is_inactive() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| {
            let guard = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(ptr::null_mut(), "test") };
            assert!(!guard.slot().is_active());
            drop(guard);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_protect_guard_drops_against_original_instance() {
        let mut left = RInstance::new_for_gc_tests();
        let mut right = RInstance::new_for_gc_tests();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };

        let left_ptr = current_instance_ptr().expect("left should be installed");
        let guard = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            protect(token(0x1))
        };
        // As above: inspect through the installed pointer, never a fresh
        // borrow of the local, while the guard is live.
        with_protected_objects_in(left_ptr, |legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots, &[token(0x1)]);
        });
        with_protected_objects_in(addr_of_mut!(right), |legacy, roots| {
            assert!(legacy.is_empty());
            assert!(roots.is_empty());
        });

        unsafe {
            replace_current_instance(Some(&mut right));
        }
        drop(guard);

        with_protected_objects_in(left_ptr, |_, roots| {
            // The root either collapsed (tail release) or tombstoned.
            assert!(roots.iter().all(|&p| p.is_null()));
        });
        with_protected_objects_in(addr_of_mut!(right), |_, roots| assert!(roots.is_empty()));
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_indexed_guard_drops_against_original_instance() {
        let mut left = RInstance::new_for_gc_tests();
        let mut right = RInstance::new_for_gc_tests();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };
        // Mid-guard inspection goes through the pointer recorded at install
        // time: a fresh `&mut left`/`&left` (or reference derivation from a
        // fresh raw pointer) would retag the allocation and pop the guard
        // owner's exposed tag out from under the wildcard release path
        // (aliasing UB under Stacked Borrows).
        let left_ptr = current_instance_ptr().expect("left should be installed");

        let mut guard = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            protect_with_index_raw(token(0x1), "test")
        };
        with_protected_objects_in(left_ptr, |legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots, &[token(0x1)]);
        });

        unsafe {
            replace_current_instance(Some(&mut right));
        }
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            guard.reprotect_raw(token(0x2))
        });
        with_protected_objects_in(left_ptr, |legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots, &[token(0x2)])
        });
        with_protected_objects_in(addr_of_mut!(right), |legacy, roots| {
            assert!(legacy.is_empty());
            assert!(roots.is_empty());
        });
        drop(guard);

        with_protected_objects_in(left_ptr, |_, roots| {
            assert!(roots.iter().all(|&p| p.is_null()))
        });
        with_protected_objects_in(addr_of_mut!(right), |_, roots| assert!(roots.is_empty()));
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_protect_with_index() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            let fake = token(0x1);
            let idx = R_ProtectWithIndex(fake);
            assert!(!idx.is_null());
            // The shim claims a generational root-table slot: the legacy
            // count-based view does not see it.
            assert_eq!(R_ProtectCount(), 0);
            with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots, &[fake]);
            });
        });
    }

    #[test]
    fn test_protect_with_index_null() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            let idx = R_ProtectWithIndex(ptr::null_mut());
            assert!((idx as usize) == 0);
            assert_eq!(R_ProtectCount(), 0);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_reprotect() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            let a = token(0x1);
            let b = token(0x2);
            let idx = R_ProtectWithIndex(a);
            R_Reprotect(b, idx);
            with_protected_objects(|_, roots| assert_eq!(roots[0], b));
        });
    }

    #[test]
    fn test_reprotect_null_index() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            R_Reprotect(token(0x1), ptr::null_mut());
        });
    }

    #[test]
    fn test_free_protect_index() {
        unsafe {
            R_FreeProtectIndex(ptr::null_mut());
            R_FreeProtectIndex(0x1 as *mut ProtectIndex);
        }
    }

    // -- Preserve / release --------------------------------------------------

    #[test]
    fn test_preserve_release() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            let fake = token(0x1);
            R_PreserveObject(fake);
            with_preserved_objects(|objects| assert_eq!(objects.len(), 1));
            R_ReleaseObject(fake);
            with_preserved_objects(|objects| assert_eq!(objects.len(), 0));
        });
    }

    #[test]
    fn test_preserve_null() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            R_PreserveObject(ptr::null_mut());
            with_preserved_objects(|objects| assert_eq!(objects.len(), 0));
        });
    }

    #[test]
    fn test_release_null() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            R_ReleaseObject(ptr::null_mut());
        });
    }

    #[test]
    fn test_preserve_guard_drops_against_original_instance() {
        let mut left = RInstance::new_for_gc_tests();
        let mut right = RInstance::new_for_gc_tests();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };
        // Direct access to the installed instance goes through the pointer
        // recorded at install time: a fresh `&mut left` would retag the
        // allocation and pop the installed borrow tag out from under the
        // ambient re-acquisition `preserve_sexp` performs (Stacked Borrows).
        let left_ptr = current_instance_ptr().expect("left should be installed");
        let raw = unsafe { (*left_ptr).arena.alloc_node(SEXPTYPE::INTSXP) };
        let value = unsafe { crate::sexp::owner::OwnerToken::from_raw(left_ptr) }
            .sexp(raw)
            .expect("left object should wrap");

        let guard = preserve_sexp(value);
        with_preserved_objects_in(unsafe { &mut *left_ptr }, |objects| {
            assert_eq!(objects, &[raw])
        });
        with_preserved_objects_in(addr_of_mut!(right), |objects| assert!(objects.is_empty()));

        unsafe {
            replace_current_instance(Some(&mut right));
        }
        drop(guard);

        with_preserved_objects_in(unsafe { &mut *left_ptr }, |objects| {
            assert!(objects.is_empty())
        });
        with_preserved_objects_in(addr_of_mut!(right), |objects| assert!(objects.is_empty()));
        unsafe {
            replace_current_instance(previous);
        }
    }

    // -- Slot generations -----------------------------------------------------

    #[test]
    fn test_slot_generations_differ_across_release_and_reuse() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| {
            let first = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0x1), "test") };
            let first_slot = first.slot();
            assert!(first_slot.is_active());
            drop(first);

            // The same index is handed out again with a fresh generation.
            let second = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0x2), "test") };
            let second_slot = second.slot();
            assert_ne!(first_slot.generation(), second_slot.generation());

            // The live handle still matches its own generation; the released
            // handle resolves to a different generation at its index.
            assert!(!second_slot.is_stale());
            assert!(first_slot.is_stale());
            drop(second);
        });
    }

    #[test]
    fn test_rooted_sexp_generation_survives_full_gc() {
        let mut session = RSession::new_for_gc_tests();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let root = RootedSexp::root(value.clone());
            let generation = root.slot().generation();
            assert!(!root.is_stale());

            // The root pins the value in the root table, so collection
            // must leave the value and the slot's generation intact.
            crate::sexp::gengc::full_gc();

            assert!(!root.is_stale());
            assert_eq!(root.slot().generation(), generation);
            let readback = root.get().expect("rooted value must resolve after gc");
            assert_eq!(readback.clone().as_raw(), value.clone().as_raw());
            with_protected_objects(|_, roots| assert_eq!(roots, &[value.clone().as_raw(); 2]));
        });
    }

    #[test]
    fn test_released_slot_reuse_reports_stale_generation() {
        let mut session = RSession::new_for_gc_tests();
        let raw = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(raw).expect("value belongs to session");

        session.with_protected(|| {
            let root = RootedSexp::root(value.clone());
            let slot = root.slot();
            assert!(slot.is_active());
            assert!(!slot.is_stale());
            let generation = slot.generation();

            // Release the slot, then allocate and churn roots so its index
            // is handed out again.
            let sexp = root.unroot();
            assert_eq!(sexp.as_raw(), value.clone().as_raw());
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 1));
            crate::sexp::instance::with_required_current_instance(|inst| unsafe {
                for _ in 0..1000 {
                    (*inst).arena.alloc_node(SEXPTYPE::INTSXP);
                }
            });

            for _ in 0..1000 {
                let churn = RootedSexp::root(value.clone());
                assert_ne!(churn.slot().generation(), generation);
                drop(churn);
            }

            // The old handle's slot was released and its index handed out
            // again: the entry living there (if any) carries a different
            // generation, so the handle reports stale.
            assert!(slot.is_stale());
        });
    }

    #[test]
    fn test_arbitrary_drop_order_keeps_surviving_roots_live() {
        let mut session = RSession::new_for_gc_tests();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let first = RootedSexp::root(value.clone());
            let second = RootedSexp::root(value.clone());
            let third = RootedSexp::root(value.clone());
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 4));
            assert!(!first.is_stale());
            assert!(!second.is_stale());
            assert!(!third.is_stale());

            // Drop out of order: the interior tombstone leaves the survivors'
            // indices untouched, so they keep resolving to their own entries.
            drop(first);
            assert!(second.get().is_some());
            assert!(third.get().is_some());
            assert!(!second.is_stale());
            assert!(!third.is_stale());
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 4));

            // The freed index is reusable: a fresh root lands exactly there
            // with a newer generation while the survivors stay healthy.
            let fresh = RootedSexp::root(value.clone());
            assert!(!fresh.is_stale());
            assert!(fresh.get().is_some());
            assert!(!second.is_stale());
            assert!(!third.is_stale());
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 4));

            drop(fresh);
            drop(third);
            drop(second);
            // All slots released: the tail collapse pops every entry, so the
            // live-entry depth is restored exactly.
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 1));
        });
    }

    #[test]
    fn test_shuffled_roots_survive_vec_drop_order() {
        let mut session = RSession::new_for_gc_tests();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let mut roots: Vec<RootedSexp<'_>> =
                (0..16).map(|_| RootedSexp::root(value.clone())).collect();
            with_protected_objects(|_, table| assert_eq!(table.len(), 17));
            // Deterministic bit-reversal permutation: exercises a genuinely
            // shuffled drop order without pulling in an RNG dependency.
            let mut permuted: Vec<RootedSexp<'_>> = Vec::with_capacity(roots.len());
            while !roots.is_empty() {
                let mid = roots.len() / 2;
                permuted.push(roots.remove(mid));
            }
            let mut roots = permuted;
            // Drop half in shuffled order: every survivor still reads.
            for _ in 0..8 {
                roots.pop();
            }
            // Drain the rest in the shuffled order too: every release either
            // reuses, tombstones, or collapses, so the depth is restored.
            while roots.pop().is_some() {}
            with_protected_objects(|_, table| assert_eq!(table.len(), 1));
        });
    }

    #[test]
    fn test_slot_reuse_rejects_old_token() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| {
            let first = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0x1), "test") };
            let stale_token = first.slot();
            assert!(stale_token.is_active());
            assert!(!stale_token.is_stale());
            drop(first);
            assert!(stale_token.is_stale());

            // The freed index is handed out again with a fresh generation.
            let second = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0x2), "test") };
            let live_token = second.slot();
            assert!(live_token.is_active());
            assert_ne!(stale_token.generation(), live_token.generation());
            assert!(!live_token.is_stale());

            // A double release against the stale token cannot evict the live
            // owner: generations disagree, so the tombstone is untouched.
            release_protect_slot(stale_token);
            assert!(!live_token.is_stale());
            assert!(!second.slot().is_stale());
            assert_eq!(second.slot().generation(), live_token.generation());
            drop(second);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    // -- Split-storage invariants (the two disciplines never alias) ----------

    #[test]
    fn test_legacy_unprotect_never_truncates_root_slots() {
        let mut session = RSession::new_for_gc_tests();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let first = RootedSexp::root(value.clone());
            let second = RootedSexp::root(value.clone());

            // Legacy pushes and count-based pops interleave with live roots.
            unsafe {
                protect_raw_pointer(token(0xA));
                protect_raw_pointer(token(0xB));
            }
            assert_eq!(R_ProtectCount(), 2);
            unprotect_count(2);
            assert_eq!(R_ProtectCount(), 0);

            // The roots survived the count-based truncation untouched.
            assert!(!first.is_stale());
            assert!(!second.is_stale());
            assert!(first.get().is_some());
            assert!(second.get().is_some());
            with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots.len(), 3);
            });

            drop(second);
            assert!(!first.is_stale());
            drop(first);
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 1));
        });
    }

    #[test]
    fn test_root_release_never_shifts_legacy_entries() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| {
            unsafe {
                protect_raw_pointer(token(0x1));
                protect_raw_pointer(token(0x2));
                protect_raw_pointer(token(0x3));
            }
            assert_eq!(R_ProtectCount(), 3);

            // Claim and release roots (in any order) around the live legacy
            // entries: the legacy stack must not shift.
            let a = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(token(0xA1)) };
            let b = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0xA2), "test") };
            drop(a);
            with_protected_objects(|legacy, _| {
                assert_eq!(legacy, &[token(0x1), token(0x2), token(0x3)]);
            });
            drop(b);
            assert_eq!(R_ProtectCount(), 3);
            with_protected_objects(|legacy, roots| {
                assert_eq!(legacy, &[token(0x1), token(0x2), token(0x3)]);
                assert!(roots.iter().all(|&p| p.is_null()) || roots.is_empty());
            });

            // Legacy LIFO unwinding still works after the root churn.
            unprotect_count(3);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_nested_sessions_with_interleaved_roots() {
        // Two live sessions on one thread, each with roots claimed while it
        // was ambient. Re-activation goes through each session's STABLE
        // instance pointer (never a fresh `&mut`), so guard owner
        // exposures stay valid while the ambient instance switches — the
        // documented owner discipline (see `with_guard_owner`).
        let left = RSession::new_for_gc_tests();
        let right = RSession::new_for_gc_tests();

        let left_root = left.with_active(|| RootedSexp::root(left.global_env().unwrap()));
        let left_guard = left.with_active(|| unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(token(0x10)) });
        let right_guard = right.with_active(|| unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(token(0x20)) });

        left.with_active(|| {
            with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots.len(), 3); // handle lease + explicit root + left_guard
            })
        });
        right.with_active(|| {
            with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots, &[token(0x20)]); // right_guard only
            })
        });

        // Interleaved, out-of-order drops while the OTHER session is
        // ambient: each guard releases against its own instance and slot.
        drop(left_guard);
        left.with_active(|| assert!(!left_root.is_stale()));
        drop(right_guard);
        left.with_active(|| assert!(!left_root.is_stale()));
        drop(left_root);

        left.with_active(|| with_protected_objects(|_, roots| assert!(roots.is_empty())));
        right.with_active(|| with_protected_objects(|_, roots| assert!(roots.is_empty())));
    }
    #[test]
    fn test_update_protect_stack_refs_covers_both_storages() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            protect_raw_pointer(token(0x1));
            let _root = protect_with_index_raw(token(0x2), "test");
            update_protect_stack_refs(|ptr| {
                if ptr == token(0x1) || ptr == token(0x2) {
                    token(0x100)
                } else {
                    ptr
                }
            });
            with_protected_objects(|legacy, roots| {
                assert_eq!(legacy[0], token(0x100));
                assert_eq!(roots[0], token(0x100));
            });
            unprotect_count(1);
        });
    }

    #[test]
    fn test_update_preserve_stack_refs() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            R_PreserveObject(token(0x1));
            update_preserve_stack_refs(|ptr| token(0x200));
            with_preserved_objects(|objects| {
                assert_eq!(objects[0], token(0x200));
            });
            R_ReleaseObject(token(0x200));
        });
    }

    #[test]
    fn test_with_preserved_objects() {
        let session = RSession::new_for_gc_tests();
        session.with_protected(|| unsafe {
            R_PreserveObject(token(0x1));
            R_PreserveObject(token(0x2));
            with_preserved_objects(|objects| {
                assert_eq!(objects.len(), 2);
            });
            R_ReleaseObject(token(0x1));
            R_ReleaseObject(token(0x2));
        });
    }
    #[test]
    fn safe_roots_use_value_owner_not_ambient_session() {
        let left = RSession::new_for_gc_tests();
        let right = RSession::new_for_gc_tests();
        let value = left.global_env().unwrap();
        let mut root = protect_sexp_with_index(value.clone());
        left.with_active(|| {
            with_protected_objects(|_, roots| assert_eq!(roots, &[value.clone().as_raw(); 2]))
        });
        right.with_active(|| with_protected_objects(|_, roots| assert!(roots.is_empty())));
        assert_eq!(
            root.try_reprotect_sexp(right.global_env().unwrap()),
            Err(ProtectError::ForeignOwner)
        );
    }

    #[test]
    fn stale_reprotect_cannot_replace_reused_slot() {
        let session = RSession::new_for_gc_tests();
        let mut old = protect_sexp_with_index(session.global_env().unwrap());
        release_protect_slot(old.slot());
        let live = protect_sexp_with_index(session.base_env().unwrap());
        assert_eq!(
            old.try_reprotect_sexp(session.global_env().unwrap()),
            Err(ProtectError::StaleSlot)
        );
        reprotect_slot(old.slot(), session.global_env().unwrap().as_raw());
        let base = session.base_env().unwrap().as_raw();
        with_protected_objects(|_, roots| {
            assert_eq!(
                roots
                    .iter()
                    .copied()
                    .filter(|p| !p.is_null())
                    .collect::<Vec<_>>(),
                vec![base]
            );
        });
        drop(old);
        assert!(!live.slot().is_stale());
    }

    #[test]
    fn public_scope_does_not_revoke_returned_root() {
        let session = RSession::new_for_gc_tests();
        let root = session.with_protected(|| RootedSexp::root(session.global_env().unwrap()));
        session.gc();
        assert!(!root.is_stale());
        assert!(root.get().unwrap().is_environment());
    }

    // These are actual process-owned native objects, not fabricated addresses.
    fn token(n: usize) -> SEXP {
        unsafe {
            match n % 7 {
                0 => super::super::globals::R_NilValue(),
                1 => super::super::globals::R_UnboundValue(),
                2 => super::super::globals::R_MissingArg(),
                3 => super::super::globals::R_RestartToken(),
                4 => super::super::globals::R_NaString(),
                5 => super::super::globals::R_True(),
                _ => super::super::globals::R_False(),
            }
        }
    }

    #[test]
    fn exhausted_safe_claim_returns_error_without_mutating_existing_roots() {
        let session = RSession::new_for_gc_tests();
        let value = session.global_env().unwrap();
        let owner = value.session_owner_ptr.unwrap().as_ptr();
        let before = unsafe { (*owner).root_table.entries_snapshot() };
        unsafe {
            (*owner).root_table.set_next_generation_for_test(u64::MAX);
        }
        assert!(matches!(
            try_protect_sexp(value.clone()),
            Err(ProtectError::GenerationExhausted)
        ));
        assert!(matches!(
            try_protect_sexp_with_index(value.clone()),
            Err(ProtectError::GenerationExhausted)
        ));
        assert!(matches!(
            RootedSexp::try_root(value.clone()),
            Err(ProtectError::GenerationExhausted)
        ));
        assert_eq!(unsafe { (*owner).root_table.entries_snapshot() }, before);
        drop(value); // Even at generation exhaustion, root cleanup cannot panic.
        assert_eq!(unsafe { (*owner).root_table.len() }, 0);
    }
}

#[cfg(kani)]
mod root_table_kani {
    use super::root_storage::{RootStorage, StorageError};

    #[kani::proof]
    fn root_table_exhaustion_is_atomic() {
        let mut table = RootStorage::default();
        table.set_next_generation_for_test(u64::MAX);
        assert_eq!(
            table.try_claim(1u8, false),
            Err(StorageError::GenerationExhausted)
        );
        assert_eq!(table.checkpoint(), u64::MAX);
        assert_eq!(table.len(), 0);
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn root_table_ops_preserve_invariant() {
        let mut table = RootStorage::default();
        let old = table.try_claim(1u8, false).unwrap();
        table.release(old);
        let live = table.try_claim(2u8, false).unwrap();
        assert_eq!(old.index, live.index);
        assert_ne!(old.generation, live.generation);
        assert!(!table.release(old));
        assert_eq!(table.get(live), Some(&2));
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn root_table_restore_keeps_managed() {
        let mut table = RootStorage::default();
        let checkpoint = table.checkpoint();
        let raw = table.try_claim(1u8, false).unwrap();
        let managed = table.try_claim(2u8, true).unwrap();
        table.restore(checkpoint);
        assert_eq!(table.get(raw), None);
        assert_eq!(table.get(managed), Some(&2));
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn root_table_legacy_stack_is_disjoint() {
        let mut table = RootStorage::default();
        let mut legacy = Vec::new();
        let _ = table.try_claim(1u8, false).unwrap();
        let before = (table.len(), table.checkpoint());
        legacy.push(2u8);
        legacy.pop();
        assert_eq!((table.len(), table.checkpoint()), before);
    }
}
