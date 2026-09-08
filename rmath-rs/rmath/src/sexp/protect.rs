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

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;

use super::ffi::SEXP;
use super::instance::{RInstance, with_required_current_instance};
use super::object::{Sexp, SexpOwner};

/// Error returned when a safe protection API receives a handle whose owner was
/// not validated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectError {
    UnownedHandle { api: &'static str, owner: SexpOwner },
    ForeignOwner,
    StaleSlot,
}

impl std::fmt::Display for ProtectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForeignOwner => f.write_str("root replacement belongs to a different owner"),
            Self::StaleSlot => f.write_str("root slot has been released or reused"),
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
        let entries = self.entries.borrow();
        f(&entries)
    }

    /// Rewrite every entry through `update_fn` (non-moving GC sweep).
    pub(crate) fn update_refs(&self, update_fn: &mut impl FnMut(SEXP) -> SEXP) {
        // P2: as with_entries; the sweep's update_fn writes SEXP objects
        // elsewhere, never this Vec's allocation.
        let mut entries = self.entries.borrow_mut();
        for slot in entries.iter_mut() {
            *slot = update_fn(*slot);
        }
    }
}

/// The Rust root table: stable, generational slots for Rust-side handles.
///
/// Slots are claimed by [`RootTable::claim`] (returning a slot id + a fresh
/// generation) and released BY SLOT in any order via [`RootTable::release`]:
/// release tombstones the entry in place (null pointer + bumped generation)
/// and recycles the index through a free list, so surviving slots keep their
/// indices across arbitrary drop orders. A release whose recorded generation
/// no longer matches the live entry (slot already released, recycled, or cut
/// away by a scope unwind) is a no-op — a stale handle can never evict the
/// current owner. NOTHING count-based ever truncates this table: legacy
/// `Rf_unprotect(n)` operates on the [`LegacyProtectionStack`] only.
#[derive(Default)]
pub(crate) struct RootTable {
    entries: RefCell<Vec<SEXP>>,
    /// Generation tag for each entry, kept parallel to `entries`.
    generations: RefCell<Vec<u64>>,
    /// Monotonic source of slot generations.
    next_generation: Cell<u64>,
    /// Vacant entry indices available for reuse.
    free_list: RefCell<Vec<usize>>,
    managed: RefCell<std::collections::HashSet<(usize, u64)>>,
}

impl RootTable {
    pub(crate) fn new() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
            generations: RefCell::new(Vec::new()),
            next_generation: Cell::new(0),
            free_list: RefCell::new(Vec::new()),
            managed: RefCell::new(Default::default()),
        }
    }

    pub(crate) fn checkpoint(&self) -> u64 {
        self.next_generation.get()
    }

    fn retain_managed(&self, slot: ProtectionSlot) {
        if let Some(index) = slot.index {
            self.managed.borrow_mut().insert((index, slot.generation));
        }
    }

    /// Clean up scope-owned raw roots, including reused interior slots.
    /// Lifetime-bound Rust guards retain their roots until their own Drop.
    pub(crate) fn restore(&self, checkpoint: u64) {
        let slots: Vec<_> = self
            .generations
            .borrow()
            .iter()
            .copied()
            .enumerate()
            .filter(|&(i, g)| g >= checkpoint && !self.managed.borrow().contains(&(i, g)))
            .map(|(i, g)| ProtectionSlot::from_stack_index(i, g))
            .collect();
        for slot in slots {
            self.release(slot);
        }
    }

    /// Allocate the next slot generation. Monotonic per table, so every
    /// claim — including one that reuses a released index — is
    /// distinguishable from every slot handle captured earlier.
    fn next_gen(&self) -> u64 {
        // P2: strictly-local Cell access; no ambient write intervenes.
        let generation = self.next_generation.get();
        self.next_generation.set(
            generation
                .checked_add(1)
                .expect("root generation exhausted"),
        );
        generation
    }

    /// Claim a slot for `s`, reusing a tombstoned index when one is free.
    /// Returns the index now holding `s` and the fresh generation recorded
    /// for it.
    pub(crate) fn claim(&self, s: SEXP, api: &str) -> (usize, u64) {
        // P2: strictly-local RefCell access. `reserve_slot_or_fail` may
        // allocate (try_reserve) and panic, but neither reenters the
        // interpreter nor touches the instance through another raw path.
        if let Some(index) = self.free_list.borrow_mut().pop() {
            let mut entries = self.entries.borrow_mut();
            let mut generations = self.generations.borrow_mut();
            if index < entries.len() && index < generations.len() {
                entries[index] = s;
                let generation = self.next_gen();
                generations[index] = generation;
                return (index, generation);
            }
            // Stale free-list entry (truncated away without pruning):
            // discard it and fall through to a fresh push.
        }
        let mut entries = self.entries.borrow_mut();
        reserve_slot_or_fail(&mut entries, api);
        entries.push(s);
        let index = entries.len() - 1;
        let generation = self.next_gen();
        let mut generations = self.generations.borrow_mut();
        debug_assert_eq!(
            generations.len(),
            index,
            "root generation log must stay parallel to the root table"
        );
        if generations.len() < index {
            generations.resize(index, generation);
        }
        generations.push(generation);
        (index, generation)
    }

    /// Release the slot `slot` refers to, if it still carries the generation
    /// the handle recorded. Tombstones the entry in place and queues the
    /// index for reuse; freed tail slots collapse off the table so the
    /// physical Vec stays contiguous (interior frees stay tombstoned beneath
    /// live entries). A stale release (index already freed, stolen by a
    /// scope unwind, or whose generation moved on) is a no-op — dropping two
    /// guards that disagree on the entry must not evict the live owner.
    pub(crate) fn release(&self, slot: ProtectionSlot) {
        let Some(index) = slot.index else {
            return;
        };
        // P2: strictly-local RefCell access; no ambient write intervenes.
        let mut entries = self.entries.borrow_mut();
        if index >= entries.len() {
            return;
        }
        let mut generations = self.generations.borrow_mut();
        if index >= generations.len() || generations[index] != slot.generation {
            return;
        }
        self.managed.borrow_mut().remove(&(index, slot.generation));
        entries[index] = std::ptr::null_mut();
        generations[index] = self.next_gen();
        let mut free = self.free_list.borrow_mut();
        if !free.contains(&index) {
            free.push(index);
        }
        while entries.len() > 0 && entries[entries.len() - 1].is_null() {
            let tail = entries.len() - 1;
            if let Some(pos) = free.iter().position(|&i| i == tail) {
                free.swap_remove(pos);
                entries.pop();
                generations.pop();
            } else {
                break;
            }
        }
    }

    /// Replace the value held by `slot`'s entry, if the entry still exists.
    /// Used by the write barrier (`R_Reprotect` / `reprotect_sexp`).
    pub(crate) fn reprotect(&self, slot: ProtectionSlot, s: SEXP) {
        let Some(index) = slot.index else {
            return;
        };
        // P2: strictly-local RefCell write; no ambient write intervenes.
        let mut entries = self.entries.borrow_mut();
        if index < entries.len() && self.generations.borrow().get(index) == Some(&slot.generation) {
            entries[index] = s;
        }
    }

    /// The generation currently recorded for `slot`'s index, or `None` when
    /// the entry is gone.
    pub(crate) fn generation_at(&self, slot: ProtectionSlot) -> Option<u64> {
        let index = slot.index?;
        // P2: strictly-local RefCell read; no ambient write intervenes.
        let generations = self.generations.borrow();
        generations.get(index).copied()
    }

    /// Number of entries in the table (including interior tombstones; freed
    /// tails collapse). This is the root-table bookkeeping depth, NOT
    /// `R_ProtectCount`.
    pub(crate) fn len(&self) -> usize {
        // P2: strictly-local RefCell read; no ambient write intervenes.
        self.entries.borrow().len()
    }

    /// Unwind to a recorded depth (session `ProtectScope` teardown): cut
    /// every entry at/above `depth`, drop the parallel generations, and drop
    /// free-list indices that no longer name entries.
    pub(crate) fn truncate(&self, depth: usize) {
        // P2: strictly-local RefCell access; no ambient write intervenes.
        self.entries.borrow_mut().truncate(depth);
        self.generations.borrow_mut().truncate(depth);
        self.free_list.borrow_mut().retain(|&index| index < depth);
        self.managed
            .borrow_mut()
            .retain(|&(index, _)| index < depth);
    }

    /// Bulk reset (instance/test harness teardown).
    pub(crate) fn clear(&self) {
        // P2: strictly-local RefCell access; no ambient write intervenes.
        self.entries.borrow_mut().clear();
        self.generations.borrow_mut().clear();
        self.free_list.borrow_mut().clear();
        self.managed.borrow_mut().clear();
    }

    /// Run `f` over every entry in index order. Tombstoned slots hold null;
    /// callers null-guard before dereferencing.
    pub(crate) fn with_entries<R>(&self, f: impl FnOnce(&[SEXP]) -> R) -> R {
        // P2: as LegacyProtectionStack::with_entries.
        let entries = self.entries.borrow();
        f(&entries)
    }

    /// Rewrite every entry through `update_fn` (non-moving GC sweep).
    pub(crate) fn update_refs(&self, update_fn: &mut impl FnMut(SEXP) -> SEXP) {
        // P2: as LegacyProtectionStack::update_refs.
        let mut entries = self.entries.borrow_mut();
        for slot in entries.iter_mut() {
            *slot = update_fn(*slot);
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
/// Guards must act on their OWNING instance even when the ambient current
/// instance has since switched to another session (see the
/// `drops_against_original_instance` tests), and a borrow-like tag captured
/// at creation (the old `NonNull::from(&mut inst)`) is invalidated by every
/// later `&mut RInstance` re-acquisition from the thread-local. The owner is
/// therefore stored as an address with exposed provenance and reconstituted
/// through `ptr::with_exposed_provenance` — the sanctioned wildcard-
/// provenance escape hatch for ambient instance back-references (permissive
/// provenance, the mode CI's Miri job runs in). The cleanup helpers take a
/// raw `*mut RInstance` and only touch `RefCell` fields through raw place
/// accesses, so no borrow tag is created and nothing can be popped by
/// reentrant ambient writes.
///
/// Soundness relies on the owner instance outliving every guard created
/// against it — the session APIs keep the instance alive across the scoped
/// interpreter call that owns the guard — and on the [`Confined`] marker
/// keeping guards on the owning thread. Release-time staleness is guarded
/// separately: root-slot releases check the slot's generation
/// ([`RootTable::release`]) so a guard whose entry was already recycled or
/// unwound is a no-op instead of evicting the live owner.
fn with_guard_owner<R>(owner: usize, f: impl FnOnce(*mut RInstance) -> R) -> R {
    // SAFETY: see the function docs; the owner outlives the guard.
    unsafe { f(std::ptr::with_exposed_provenance_mut::<RInstance>(owner)) }
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
    /// Owning instance address, stored with exposed provenance — see
    /// [`with_guard_owner`].
    owner: Option<usize>,
    release: GuardRelease,
    _confined: Confined<'a>,
}

impl Drop for ProtectGuard<'_> {
    fn drop(&mut self) {
        let Some(owner) = self.owner else {
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
    let owner = session_owner(&value);
    Ok(ProtectGuard {
        owner,
        release: GuardRelease::RootSlot(owner.map_or_else(ProtectionSlot::inactive, |owner| {
            with_guard_owner(owner, |inst| {
                let slot = protect_raw_with_slot_in(inst, value.as_raw(), "protect_sexp");
                unsafe {
                    (*inst).root_table.retain_managed(slot);
                }
                slot
            })
        })),
        _confined: PhantomData,
    })
}

/// Protect a raw SEXP and return an RAII guard.
///
/// Legacy compatibility helper for translated code. Prefer
/// [`protect_sexp`] when the caller has an owner-scoped value. The guard
/// holds a generational root-table slot and may be dropped in any order.
pub(crate) fn protect(s: SEXP) -> ProtectGuard<'static> {
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
            owner: Some(inst as usize),
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
            Some(with_required_current_instance(|inst| inst as usize))
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
pub(crate) fn protect_raw_pointer(s: SEXP) -> SEXP {
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
    if s.is_null() {
        return ProtectionSlot::inactive();
    }
    // SAFETY: `inst` is a live instance pointer from the caller; the RefCell
    // access is strictly local.
    let (index, generation) = unsafe { (*inst).root_table.claim(s, api) };
    ProtectionSlot::from_stack_index(index, generation)
}

fn reprotect_slot(slot: ProtectionSlot, s: SEXP) {
    with_required_current_instance(|inst| reprotect_slot_in(inst, slot, s));
}

fn reprotect_slot_in(inst: *mut RInstance, slot: ProtectionSlot, s: SEXP) {
    // P2: strictly-local RefCell write; no ambient write intervenes.
    // SAFETY: `inst` is a live instance pointer from the caller.
    unsafe { (*inst).root_table.reprotect(slot, s) };
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
    owner: Option<usize>,
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
        match self.owner {
            Some(owner) if self.slot.is_active() => with_guard_owner(owner, |inst| {
                protect_slot_generation_in(inst, self.slot) == Some(expected)
            }),
            _ => true,
        }
    }

    pub(crate) fn reprotect_raw(&mut self, value: SEXP) {
        if let Some(owner) = self.owner {
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
        self.reprotect_raw(value.as_raw());
        Ok(())
    }
}

impl Drop for IndexedProtectGuard<'_> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner {
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
        !self.guard.slot_generation_is(self.expected_generation)
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
    /// released; the caller owns the returned handle without a table root.
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
    let owner = session_owner(&value);
    Ok(IndexedProtectGuard {
        owner,
        slot: owner.map_or_else(ProtectionSlot::inactive, |owner| {
            with_guard_owner(owner, |inst| {
                let slot = protect_raw_with_slot_in(
                    inst,
                    value.clone().as_raw(),
                    "protect_sexp_with_index",
                );
                unsafe {
                    (*inst).root_table.retain_managed(slot);
                }
                slot
            })
        }),
        value_owner: value.owner(),
        _confined: PhantomData,
    })
}

/// Protect a raw SEXP in a replaceable root-table slot.
///
/// Legacy compatibility helper for translated Rust modules. Prefer
/// [`protect_sexp_with_index`] when the caller has an owner-scoped value.
pub(crate) fn protect_with_index_raw(s: SEXP, api: &str) -> IndexedProtectGuard<'static> {
    if s.is_null() {
        return IndexedProtectGuard {
            owner: None,
            slot: ProtectionSlot::inactive(),
            value_owner: SexpOwner::Unknown,
            _confined: PhantomData,
        };
    }

    with_required_current_instance(|inst| IndexedProtectGuard {
        owner: Some(inst as usize),
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
    if !s.is_null() {
        // P2: strictly-local RefCell access; see
        // LegacyProtectionStack::push on try_reserve.
        // SAFETY: `inst` is a live instance pointer from the caller.
        let mut stack = unsafe { (*inst).preserve_stack.borrow_mut() };
        reserve_slot_or_fail(&mut stack, "preserve");
        stack.push(s);
    }
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
    owner: Option<usize>,
    value: SEXP,
    _confined: Confined<'a>,
}

impl Drop for PreserveGuard<'_> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner {
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

    let owner = session_owner(&value);
    if let Some(owner) = owner {
        with_guard_owner(owner, |inst| push_preserve_in(inst, raw));
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
fn session_owner(value: &Sexp<'_>) -> Option<usize> {
    match value.owner() {
        SexpOwner::Session(owner) => Some(owner),
        _ => None,
    }
}

fn ensure_owner_scoped(value: Sexp<'_>, api: &'static str) -> Result<(), ProtectError> {
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
    // P2: the RefCell read borrows (and the &[SEXP] pairs handed to f) cover
    // the two stack buffers only; GC marking writes SEXP headers elsewhere,
    // never these Vecs' allocations.
    // SAFETY: `inst` is a live instance pointer from the caller.
    unsafe {
        (*inst)
            .legacy_protect
            .with_entries(|legacy| (*inst).root_table.with_entries(|roots| f(legacy, roots)))
    }
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
    // P2: the RefCell write borrows cover the two stack buffers only; the
    // sweep's update_fn writes SEXP objects elsewhere.
    // SAFETY: `inst` is a live instance pointer from the caller.
    unsafe {
        (*inst).legacy_protect.update_refs(&mut update_fn);
        (*inst).root_table.update_refs(&mut update_fn);
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
    // P2: as update_protect_stack_refs_in.
    // SAFETY: `inst` is a live instance pointer from the caller.
    let mut stack = unsafe { (*inst).preserve_stack.borrow_mut() };
    for slot in stack.iter_mut() {
        *slot = update_fn(*slot);
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
    let stack = unsafe { (*inst).preserve_stack.borrow() };
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
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let fake = 0x1 as SEXP;
            let result = protect_raw_pointer(fake);
            assert_eq!(result, fake);
            assert_eq!(R_ProtectCount(), 1);
            unprotect_count(1);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_protect_null() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            protect_raw_pointer(ptr::null_mut());
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_protect_multiple() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let a = 0x1 as SEXP;
            let b = 0x2 as SEXP;
            let c = 0x3 as SEXP;
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
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let a = 0x1 as SEXP;
            let b = 0x2 as SEXP;
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
        let session = RSession::new();
        session.with_protected(|| unsafe {
            unprotect_ptr(ptr::null_mut());
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_zero() {
        let session = RSession::new();
        session.with_protected(|| {
            unprotect_count(0);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_exceeds_stack() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            protect_raw_pointer(0x1 as SEXP);
            unprotect_count(5);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_protect_n_guard() {
        let session = RSession::new();
        session.with_protected(|| {
            let depth_before = R_ProtectCount();
            unsafe {
                protect_raw_pointer(0x1 as SEXP);
                protect_raw_pointer(0x2 as SEXP);
                protect_raw_pointer(0x3 as SEXP);
            }
            let _guard = protect_n(3);
            assert_eq!(R_ProtectCount(), depth_before + 3);
            drop(_guard);
            assert_eq!(R_ProtectCount(), depth_before);
        });
    }

    #[test]
    fn test_protect_n_guard_drops_against_original_instance() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };

        protect_raw_pointer(0x1 as SEXP);
        protect_raw_pointer(0x2 as SEXP);
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
        let session = RSession::new();
        session.with_protected(|| unsafe {
            protect_raw_pointer(0x1 as SEXP);
            protect_raw_pointer(0x2 as SEXP);
            with_protected_objects(|legacy, roots| {
                assert_eq!(legacy.len(), 2);
                assert!(roots.is_empty());
            });
            unprotect_count(2);
        });
    }

    #[test]
    fn test_update_protect_stack_refs() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            protect_raw_pointer(0x1 as SEXP);
            protect_raw_pointer(0x2 as SEXP);
            update_protect_stack_refs(|ptr| {
                if ptr as usize == 0x1 {
                    0x100 as SEXP
                } else {
                    ptr
                }
            });
            with_protected_objects(|legacy, _| {
                assert_eq!(legacy[0] as usize, 0x100);
                assert_eq!(legacy[1] as usize, 0x2);
            });
            unprotect_count(2);
        });
    }

    // -- Root-table guards ---------------------------------------------------

    #[test]
    fn test_protect_sexp_guard() {
        let mut session = RSession::new();
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
                assert_eq!(roots, &[value.as_raw()]);
            });
            let _ = roots_before;
            drop(guard);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
            assert_eq!(R_ProtectCount(), legacy_before);
        });
    }

    #[test]
    fn test_rooted_sexp_roots_and_unroots() {
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let root = RootedSexp::root(value.clone());
            with_protected_objects(|_, roots| assert_eq!(roots, &[value.clone().as_raw()]));
            let readback = root.get().expect("fresh root must resolve").clone();
            assert_eq!(readback, value);
            let sexp = root.unroot();
            assert_eq!(sexp.as_raw(), value.clone().as_raw());
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_rooted_sexp_reprotect_and_nested_lifo_drop() {
        let mut session = RSession::new();
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
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 1));
            outer.reprotect(second.clone());
            with_protected_objects(|_, roots| assert_eq!(roots, &[second.clone().as_raw()]));
            assert_eq!(
                outer.get().expect("outer root must resolve").clone(),
                second
            );
            drop(outer);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_raw_protect_guard_null_legacy() {
        let session = RSession::new();
        session.with_protected(|| {
            let depth_before = R_ProtectCount();
            let guard = protect(ptr::null_mut());
            assert_eq!(R_ProtectCount(), depth_before);
            drop(guard);
            assert_eq!(R_ProtectCount(), depth_before);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_safe_protect_rejects_unknown_owner() {
        let mut session = RSession::new();
        let raw = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = Sexp::from_raw(raw).expect("raw value should wrap as legacy boundary");

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
        let mut session = RSession::new();
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
        let mut session = RSession::new();
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
            let mut guard = protect_sexp_with_index(first);
            assert!(guard.slot().is_active());
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 1));
            guard.reprotect_sexp(second.clone());
            with_protected_objects(|_, roots| assert_eq!(roots, &[second.as_raw()]));
            drop(guard);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_indexed_raw_guard_null_is_inactive() {
        let session = RSession::new();
        session.with_protected(|| {
            let guard = protect_with_index_raw(ptr::null_mut(), "test");
            assert!(!guard.slot().is_active());
            drop(guard);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_protect_guard_drops_against_original_instance() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };

        let left_ptr = current_instance_ptr().expect("left should be installed");
        let guard = protect(0x1 as SEXP);
        // As above: inspect through the installed pointer, never a fresh
        // borrow of the local, while the guard is live.
        with_protected_objects_in(left_ptr, |legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots, &[0x1 as SEXP]);
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
        let mut left = RInstance::new();
        let mut right = RInstance::new();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };
        // Mid-guard inspection goes through the pointer recorded at install
        // time: a fresh `&mut left`/`&left` (or reference derivation from a
        // fresh raw pointer) would retag the allocation and pop the guard
        // owner's exposed tag out from under the wildcard release path
        // (aliasing UB under Stacked Borrows).
        let left_ptr = current_instance_ptr().expect("left should be installed");

        let mut guard = protect_with_index_raw(0x1 as SEXP, "test");
        with_protected_objects_in(left_ptr, |legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots, &[0x1 as SEXP]);
        });

        unsafe {
            replace_current_instance(Some(&mut right));
        }
        guard.reprotect_raw(0x2 as SEXP);
        with_protected_objects_in(left_ptr, |legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots, &[0x2 as SEXP])
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
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let fake = 0x1 as SEXP;
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
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let idx = R_ProtectWithIndex(ptr::null_mut());
            assert!((idx as usize) == 0);
            assert_eq!(R_ProtectCount(), 0);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_reprotect() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let a = 0x1 as SEXP;
            let b = 0x2 as SEXP;
            let idx = R_ProtectWithIndex(a);
            R_Reprotect(b, idx);
            with_protected_objects(|_, roots| assert_eq!(roots[0], b));
        });
    }

    #[test]
    fn test_reprotect_null_index() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            R_Reprotect(0x1 as SEXP, ptr::null_mut());
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
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let fake = 0x1 as SEXP;
            R_PreserveObject(fake);
            with_preserved_objects(|objects| assert_eq!(objects.len(), 1));
            R_ReleaseObject(fake);
            with_preserved_objects(|objects| assert_eq!(objects.len(), 0));
        });
    }

    #[test]
    fn test_preserve_null() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            R_PreserveObject(ptr::null_mut());
            with_preserved_objects(|objects| assert_eq!(objects.len(), 0));
        });
    }

    #[test]
    fn test_release_null() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            R_ReleaseObject(ptr::null_mut());
        });
    }

    #[test]
    fn test_preserve_guard_drops_against_original_instance() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };
        // Direct access to the installed instance goes through the pointer
        // recorded at install time: a fresh `&mut left` would retag the
        // allocation and pop the installed borrow tag out from under the
        // ambient re-acquisition `preserve_sexp` performs (Stacked Borrows).
        let left_ptr = current_instance_ptr().expect("left should be installed");
        let raw = unsafe { (*left_ptr).arena.alloc_node(SEXPTYPE::INTSXP) };
        let value =
            Sexp::from_session_raw(raw, unsafe { &*left_ptr }).expect("left object should wrap");

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
        let session = RSession::new();
        session.with_protected(|| {
            let first = protect_with_index_raw(0x1 as SEXP, "test");
            let first_slot = first.slot();
            assert!(first_slot.is_active());
            drop(first);

            // The same index is handed out again with a fresh generation.
            let second = protect_with_index_raw(0x2 as SEXP, "test");
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
        let mut session = RSession::new();
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
            with_protected_objects(|_, roots| assert_eq!(roots, &[value.clone().as_raw()]));
        });
    }

    #[test]
    fn test_released_slot_reuse_reports_stale_generation() {
        let mut session = RSession::new();
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
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
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
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let first = RootedSexp::root(value.clone());
            let second = RootedSexp::root(value.clone());
            let third = RootedSexp::root(value.clone());
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 3));
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
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 3));

            // The freed index is reusable: a fresh root lands exactly there
            // with a newer generation while the survivors stay healthy.
            let fresh = RootedSexp::root(value.clone());
            assert!(!fresh.is_stale());
            assert!(fresh.get().is_some());
            assert!(!second.is_stale());
            assert!(!third.is_stale());
            with_protected_objects(|_, roots| assert_eq!(roots.len(), 3));

            drop(fresh);
            drop(third);
            drop(second);
            // All slots released: the tail collapse pops every entry, so the
            // live-entry depth is restored exactly.
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_shuffled_roots_survive_vec_drop_order() {
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let mut roots: Vec<RootedSexp<'_>> =
                (0..16).map(|_| RootedSexp::root(value.clone())).collect();
            with_protected_objects(|_, table| assert_eq!(table.len(), 16));
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
            with_protected_objects(|_, table| assert!(table.is_empty()));
        });
    }

    #[test]
    fn test_slot_reuse_rejects_old_token() {
        let session = RSession::new();
        session.with_protected(|| {
            let first = protect_with_index_raw(0x1 as SEXP, "test");
            let stale_token = first.slot();
            assert!(stale_token.is_active());
            assert!(!stale_token.is_stale());
            drop(first);
            assert!(stale_token.is_stale());

            // The freed index is handed out again with a fresh generation.
            let second = protect_with_index_raw(0x2 as SEXP, "test");
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
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let first = RootedSexp::root(value.clone());
            let second = RootedSexp::root(value.clone());

            // Legacy pushes and count-based pops interleave with live roots.
            unsafe {
                protect_raw_pointer(0xA as SEXP);
                protect_raw_pointer(0xB as SEXP);
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
                assert_eq!(roots.len(), 2);
            });

            drop(second);
            assert!(!first.is_stale());
            drop(first);
            with_protected_objects(|_, roots| assert!(roots.is_empty()));
        });
    }

    #[test]
    fn test_root_release_never_shifts_legacy_entries() {
        let session = RSession::new();
        session.with_protected(|| {
            unsafe {
                protect_raw_pointer(0x1 as SEXP);
                protect_raw_pointer(0x2 as SEXP);
                protect_raw_pointer(0x3 as SEXP);
            }
            assert_eq!(R_ProtectCount(), 3);

            // Claim and release roots (in any order) around the live legacy
            // entries: the legacy stack must not shift.
            let a = protect(0xA1 as SEXP);
            let b = protect_with_index_raw(0xA2 as SEXP, "test");
            drop(a);
            with_protected_objects(|legacy, _| {
                assert_eq!(legacy, &[0x1 as SEXP, 0x2 as SEXP, 0x3 as SEXP]);
            });
            drop(b);
            assert_eq!(R_ProtectCount(), 3);
            with_protected_objects(|legacy, roots| {
                assert_eq!(legacy, &[0x1 as SEXP, 0x2 as SEXP, 0x3 as SEXP]);
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
        let left = RSession::new();
        let right = RSession::new();

        let left_root = left.with_active(|| RootedSexp::root(left.global_env().unwrap()));
        let left_guard = left.with_active(|| protect(0x10 as SEXP));
        let right_guard = right.with_active(|| protect(0x20 as SEXP));

        left.with_active(|| {
            with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots.len(), 2); // left_root + left_guard
            })
        });
        right.with_active(|| {
            with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots, &[0x20 as SEXP]); // right_guard only
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
        let session = RSession::new();
        session.with_protected(|| unsafe {
            protect_raw_pointer(0x1 as SEXP);
            let _root = protect_with_index_raw(0x2 as SEXP, "test");
            update_protect_stack_refs(|ptr| {
                if ptr as usize == 0x1 || ptr as usize == 0x2 {
                    0x100 as SEXP
                } else {
                    ptr
                }
            });
            with_protected_objects(|legacy, roots| {
                assert_eq!(legacy[0] as usize, 0x100);
                assert_eq!(roots[0] as usize, 0x100);
            });
            unprotect_count(1);
        });
    }

    #[test]
    fn test_update_preserve_stack_refs() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            R_PreserveObject(0x1 as SEXP);
            update_preserve_stack_refs(|ptr| 0x200 as SEXP);
            with_preserved_objects(|objects| {
                assert_eq!(objects[0] as usize, 0x200);
            });
            R_ReleaseObject(0x200 as SEXP);
        });
    }

    #[test]
    fn test_with_preserved_objects() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            R_PreserveObject(0x1 as SEXP);
            R_PreserveObject(0x2 as SEXP);
            with_preserved_objects(|objects| {
                assert_eq!(objects.len(), 2);
            });
            R_ReleaseObject(0x1 as SEXP);
            R_ReleaseObject(0x2 as SEXP);
        });
    }
    #[test]
    fn safe_roots_use_value_owner_not_ambient_session() {
        let left = RSession::new();
        let right = RSession::new();
        let value = left.global_env().unwrap();
        let mut root = protect_sexp_with_index(value.clone());
        left.with_active(|| {
            with_protected_objects(|_, roots| assert_eq!(roots, &[value.as_raw()]))
        });
        right.with_active(|| with_protected_objects(|_, roots| assert!(roots.is_empty())));
        assert_eq!(
            root.try_reprotect_sexp(right.global_env().unwrap()),
            Err(ProtectError::ForeignOwner)
        );
    }

    #[test]
    fn stale_reprotect_cannot_replace_reused_slot() {
        let session = RSession::new();
        let mut old = protect_sexp_with_index(session.global_env().unwrap());
        release_protect_slot(old.slot());
        let live = protect_sexp_with_index(session.base_env().unwrap());
        assert_eq!(
            old.try_reprotect_sexp(session.global_env().unwrap()),
            Err(ProtectError::StaleSlot)
        );
        reprotect_slot(old.slot(), session.global_env().unwrap().as_raw());
        with_protected_objects(|_, roots| {
            assert_eq!(roots, &[session.base_env().unwrap().as_raw()])
        });
        drop(old);
        assert!(!live.slot().is_stale());
    }

    #[test]
    fn public_scope_does_not_revoke_returned_root() {
        let session = RSession::new();
        let root = session.with_protected(|| RootedSexp::root(session.global_env().unwrap()));
        session.gc();
        assert!(!root.is_stale());
        assert!(root.get().unwrap().is_environment());
    }
}
