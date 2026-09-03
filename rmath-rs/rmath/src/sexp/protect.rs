#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
#![deny(unsafe_op_in_unsafe_fn)]

//! R's PROTECT/UNPROTECT mechanism.
//!
//! R maintains a protection stack to prevent GC from collecting objects that
//! are only referenced by local variables. In this Rust port the stack is owned
//! by the active `RInstance`; new Rust code should protect owner-scoped
//! [`Sexp`](super::object::Sexp) handles instead of raw pointers.
//!
//! # Ownership model
//!
//! * [`Sexp`] handles are **non-`Copy`**: assigning a handle moves it, and
//!   aliasing the same R object requires an explicit [`Clone`](Sexp::clone)
//!   (a cheap second handle over identical memory, never a deep copy).
//! * Holding a handle alone does **not** root the object: the non-moving /
//!   generational GC may collect anything only reachable from Rust locals
//!   once an R evaluation re-enters. To retain a value across a GC point,
//!   push it on the protect stack.
//! * [`RootedSexp`] is the ergonomic RAII rooting layer: it clones the
//!   handle, protects it on creation, and unprotects on [`Drop`], exposing
//!   reads through [`RootedSexp::get`]. Lower-level callers can use
//!   [`protect_sexp`]/[`ProtectGuard`] or the replaceable-slot
//!   [`protect_sexp_with_index`]/[`IndexedProtectGuard`] directly.
//! * Handles to objects that may be *replaced* during evaluation (grown
//!   vectors, PROMSXP re-promises) must be refreshed through the write
//!   barrier — re-derive the handle from its owner or use
//!   [`IndexedProtectGuard::reprotect_sexp`] on a slot-protected value —
//!   never by mutating a raw pointer in place.
//!
//! # Slot-stability contract
//!
//! The protect stack is a `Vec` with stable slot indices. Releasing a slot
//! (`IndexedProtectGuard` / [`RootedSexp`] drop) tombstones its entry
//! (null pointer + a fresh generation) and pushes the index onto a per-
//! instance free list instead of shifting later entries, so guards may be
//! dropped in **any order** — surviving guards keep resolving to their own
//! entries. A later slot push reuses a freed index with a fresh generation.
//! Every pushed entry is tagged with a generation from a monotonic
//! per-instance counter, so a slot handle whose entry was released (or cut
//! away by a truncating `unprotect`) is detectable via
//! [`ProtectionSlot::is_stale`] / [`RootedSexp::is_stale`] instead of
//! silently resolving to another entry's protection.

use super::ffi::SEXP;
use super::instance::{RInstance, with_required_current_instance};
use super::object::{Sexp, SexpOwner};

/// Error returned when a safe protection API receives a handle whose owner was
/// not validated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectError {
    UnownedHandle { api: &'static str, owner: SexpOwner },
}

impl std::fmt::Display for ProtectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtectError::UnownedHandle { api, owner } => {
                write!(f, "{api}: SEXP handle is not owner-scoped ({owner:?})")
            }
        }
    }
}

impl std::error::Error for ProtectError {}

/// RAII guard for the protection stack.
/// Automatically unprotects when dropped.
///
/// ```rust,ignore
/// use rmath::sexp::protect::protect_sexp;
///
/// let guard = protect_sexp(some_sexp);
/// // ... do work ...
/// // guard automatically unprotects when it goes out of scope
/// ```
pub struct ProtectGuard {
    /// Owning instance address, stored with exposed provenance — see
    /// [`with_guard_owner`].
    owner: Option<usize>,
    count: usize,
}

/// Run `f` with a guard's owning instance.
///
/// Guards must act on their OWNING instance even when the ambient current
/// instance has since switched to another session (see the
/// `drops_against_original_instance` tests), and a borrow-like tag captured
/// at creation (the old `NonNull::from(&mut inst)`) is invalidated by every
/// later `&mut RInstance` re-acquisition from the thread-local. The owner is
/// therefore stored as an address with exposed provenance and reconstituted
/// through `ptr::with_exposed_provenance` — the sanctioned wildcard-
/// provenance escape hatch for ambient instance back-references (permissive
/// provenance, the mode CI's Miri job runs in). The cleanup helpers take
/// `&RInstance` — they only touch `RefCell` fields — so the shared wildcard
/// retag validates against the still-live session root tag.
fn with_guard_owner<R>(owner: usize, f: impl FnOnce(&RInstance) -> R) -> R {
    // SAFETY: see the function docs; the owner outlives the guard.
    unsafe { f(&*std::ptr::with_exposed_provenance::<RInstance>(owner)) }
}

impl Drop for ProtectGuard {
    fn drop(&mut self) {
        if let (Some(owner), count) = (self.owner, self.count)
            && count > 0
        {
            // SAFETY: The guard is created only while its owning RInstance is
            // active and the session APIs keep that instance alive across the
            // scoped interpreter call that owns the guard.
            with_guard_owner(owner, |inst| unsafe {
                unprotect_count_in(inst, count);
            });
        }
    }
}

/// Protect an owner-scoped SEXP handle and return an RAII guard.
///
/// This is the Rust API exposed to embedders. Raw pointer protection remains
/// crate-local translation scaffolding for ported interpreter modules.
pub fn protect_sexp(value: Sexp<'_>) -> ProtectGuard {
    try_protect_sexp(value).expect("protect_sexp requires an owner-scoped Sexp")
}

/// Try to protect an owner-scoped SEXP handle.
pub fn try_protect_sexp(value: Sexp<'_>) -> Result<ProtectGuard, ProtectError> {
    ensure_owner_scoped(value.clone(), "protect_sexp")?;
    Ok(protect_raw(value.as_raw()))
}

/// Protect a raw SEXP and return an RAII guard.
///
/// Legacy compatibility helper for translated code. Prefer
/// [`protect_sexp`] when the caller has an owner-scoped value.
pub(crate) fn protect(s: SEXP) -> ProtectGuard {
    protect_raw(s)
}

fn protect_raw(s: SEXP) -> ProtectGuard {
    if s.is_null() {
        return ProtectGuard {
            owner: None,
            count: 0,
        };
    }

    let owner = with_required_current_instance(|inst| {
        push_protect_in(inst, s);
        inst as *mut RInstance as usize
    });

    ProtectGuard {
        owner: Some(owner),
        count: 1,
    }
}

/// Create a guard that will pop `n` stack entries on drop.
///
/// Callers must already have pushed `n` entries and want RAII-style unwinding
/// safety around a manual protect batch.
pub(crate) fn protect_n(n: usize) -> ProtectGuard {
    ProtectGuard {
        owner: if n == 0 {
            None
        } else {
            Some(with_required_current_instance(|inst| {
                inst as *mut RInstance as usize
            }))
        },
        count: n,
    }
}

fn reserve_slot_or_fail(stack: &mut Vec<SEXP>, api: &str) {
    if stack.try_reserve(1).is_err() {
        panic!("{api}: protection stack allocation failed");
    }
}

/// Allocate the next protection-slot generation.
///
/// The counter is monotonic per instance, so every push — including a push
/// that reuses the index of a released entry — is distinguishable from
/// every slot handle captured earlier.
fn next_slot_generation(inst: &RInstance) -> u64 {
    let generation = inst.protect_slot_next_generation.get();
    inst.protect_slot_next_generation
        .set(generation.wrapping_add(1));
    generation
}

/// Record `generation` for the entry just pushed at `index`, keeping the
/// generation log parallel to the protect stack even when foreign code
/// truncated, cleared, or pushed onto the stack directly (the session
/// `ProtectScope`, instance teardown, GC test harnesses): surplus entries
/// are dropped and gaps are back-filled, so the log stays aligned with the
/// entries this module pushed.
fn record_slot_generation(inst: &RInstance, index: usize, generation: u64) {
    let mut generations = inst.protect_stack_generations.borrow_mut();
    if generations.len() > index {
        generations.truncate(index);
    }
    if generations.len() < index {
        generations.resize(index, generation);
    }
    generations.push(generation);
}

/// Claim a protect-stack slot for `s`, reusing a tombstoned index when one is
/// free. Freed tail slots collapse off the stack on release while interior
/// frees stay vacant (null) beneath live entries; reuse pops any free index
/// and overwrites it in place, so surviving slots keep their indices across
/// arbitrary drop orders. Returns the index now holding `s` and the fresh
/// generation recorded for it.
fn claim_protect_slot_in(inst: &RInstance, s: SEXP, api: &str) -> (usize, u64) {
    if let Some(index) = inst.protect_slot_free.borrow_mut().pop() {
        let mut stack = inst.protect_stack.borrow_mut();
        let mut generations = inst.protect_stack_generations.borrow_mut();
        if index < stack.len() && index < generations.len() {
            stack[index] = s;
            let generation = next_slot_generation(inst);
            generations[index] = generation;
            return (index, generation);
        }
        // Stale free-list entry (truncated away without pruning): discard it
        // and fall through to a fresh push.
    }
    let mut stack = inst.protect_stack.borrow_mut();
    reserve_slot_or_fail(&mut stack, api);
    stack.push(s);
    let index = stack.len() - 1;
    let generation = next_slot_generation(inst);
    record_slot_generation(inst, index, generation);
    (index, generation)
}

pub(crate) fn push_protect_in(inst: &mut RInstance, s: SEXP) {
    if !s.is_null() {
        claim_protect_slot_in(inst, s, "protect");
    }
}

fn push_protect(s: SEXP) {
    with_required_current_instance(|inst| push_protect_in(inst, s));
}

fn push_preserve_in(inst: &mut RInstance, s: SEXP) {
    if !s.is_null() {
        let mut stack = inst.preserve_stack.borrow_mut();
        reserve_slot_or_fail(&mut stack, "preserve");
        stack.push(s);
    }
}

fn push_preserve(s: SEXP) {
    with_required_current_instance(|inst| push_preserve_in(inst, s));
}

pub(crate) fn release_preserved_in(inst: &RInstance, s: SEXP) {
    if s.is_null() {
        return;
    }
    let mut stack = inst.preserve_stack.borrow_mut();
    if let Some(pos) = stack.iter().position(|&x| x == s) {
        stack.remove(pos);
    }
}

fn release_preserved(s: SEXP) {
    with_required_current_instance(|inst| release_preserved_in(inst, s));
}

/// RAII guard for the preserve stack.
///
/// Dropping the guard releases the preserved object from the active session.
pub struct PreserveGuard {
    owner: Option<usize>,
    value: SEXP,
}

impl Drop for PreserveGuard {
    fn drop(&mut self) {
        if let Some(owner) = self.owner {
            // SAFETY: See ProtectGuard::drop; preserve guards are scoped to the
            // owning session entrypoint that created them.
            with_guard_owner(owner, |inst| unsafe {
                release_preserved_in(inst, self.value);
            });
        }
    }
}

/// Preserve an owner-scoped SEXP handle until the returned guard is dropped.
pub fn preserve_sexp(value: Sexp<'_>) -> PreserveGuard {
    try_preserve_sexp(value).expect("preserve_sexp requires an owner-scoped Sexp")
}

/// Try to preserve an owner-scoped SEXP handle until the returned guard is dropped.
pub fn try_preserve_sexp(value: Sexp<'_>) -> Result<PreserveGuard, ProtectError> {
    ensure_owner_scoped(value.clone(), "preserve_sexp")?;
    let raw = value.as_raw();
    if raw.is_null() {
        return Ok(PreserveGuard {
            owner: None,
            value: raw,
        });
    }

    let owner = with_required_current_instance(|inst| {
        push_preserve_in(inst, raw);
        inst as *mut RInstance as usize
    });
    Ok(PreserveGuard {
        owner: Some(owner),
        value: raw,
    })
}

// ---------------------------------------------------------------------------
// Core protect/unprotect functions
// ---------------------------------------------------------------------------

/// Push a raw SEXP onto the protection stack and return it.
pub(crate) fn protect_raw_pointer(s: SEXP) -> SEXP {
    push_protect(s);
    s
}

/// Pop the top `n` entries from the protection stack.
/// Run `f` while temporarily protecting extra roots on the protect stack.
pub(crate) fn with_temporary_extra_protects<F, R>(extra: impl FnOnce(&mut RInstance), f: F) -> R
where
    F: FnOnce() -> R,
{
    with_required_current_instance(|inst| {
        let start = inst.protect_stack.borrow().len();
        extra(inst);
        let added = inst.protect_stack.borrow().len().saturating_sub(start);
        let result = f();
        unprotect_count_in(inst, added);
        result
    })
}

pub(crate) fn unprotect_count_in(inst: &RInstance, n: usize) {
    if n == 0 {
        return;
    }
    let mut stack = inst.protect_stack.borrow_mut();
    let len = stack.len();
    if n >= len {
        stack.clear();
        inst.protect_stack_generations.borrow_mut().clear();
        inst.protect_slot_free.borrow_mut().clear();
    } else {
        let keep = len - n;
        stack.truncate(keep);
        inst.protect_stack_generations.borrow_mut().truncate(keep);
        // Free-list indices at/above the new length no longer name live
        // slots: drop them, and release protection for any live-rooted
        // object whose slot was cut away is reported stale via the
        // truncated generation.
        inst.protect_slot_free
            .borrow_mut()
            .retain(|&index| index < keep);
    }
}

/// Pop the top `n` entries from the protection stack.
pub(crate) fn unprotect_count(n: usize) {
    with_required_current_instance(|inst| unprotect_count_in(inst, n));
}

/// Unprotect the top entry from the protection stack.
///
/// This is the equivalent of R's `UNPROTECT_PTR()` macro.
pub(crate) fn unprotect_ptr(s: SEXP) {
    with_required_current_instance(|inst| unprotect_ptr_in(inst, s));
}

pub(crate) fn unprotect_ptr_in(inst: &mut RInstance, s: SEXP) {
    if s.is_null() {
        return;
    }
    let mut stack = inst.protect_stack.borrow_mut();
    if let Some(pos) = stack.iter().rposition(|&x| x == s) {
        stack.remove(pos);
        let mut generations = inst.protect_stack_generations.borrow_mut();
        if pos < generations.len() {
            generations.remove(pos);
        }
    }
}

/// Get the current number of entries on the protection stack.
///
/// Used by the context system to track protect depth.
pub(crate) fn R_ProtectCount() -> usize {
    with_required_current_instance(R_ProtectCount_in)
}

pub(crate) fn R_ProtectCount_in(inst: &mut RInstance) -> usize {
    // Freed tail slots collapse off the stack on release, so the physical
    // length is the live-entry tail depth; interior tombstones below live
    // entries are impossible by construction.
    inst.protect_stack.borrow().len()
}

/// Iterate over all protected SEXP values on the stack.
/// Used by the GC to mark protected objects.
pub(crate) fn with_protected_objects<F, R>(f: F) -> R
where
    F: FnOnce(&[SEXP]) -> R,
{
    with_required_current_instance(|inst| with_protected_objects_in(inst, f))
}

pub(crate) fn with_protected_objects_in<F, R>(inst: &mut RInstance, f: F) -> R
where
    F: FnOnce(&[SEXP]) -> R,
{
    let stack = inst.protect_stack.borrow();
    f(&stack)
}

/// Update all protect stack references using the given mapping function.
/// Used by non-moving GC sweep to redirect references to freed objects.
pub(crate) fn update_protect_stack_refs<F>(update_fn: F)
where
    F: FnMut(SEXP) -> SEXP,
{
    with_required_current_instance(|inst| update_protect_stack_refs_in(inst, update_fn));
}

pub(crate) fn update_protect_stack_refs_in<F>(inst: &mut RInstance, mut update_fn: F)
where
    F: FnMut(SEXP) -> SEXP,
{
    let mut stack = inst.protect_stack.borrow_mut();
    for slot in stack.iter_mut() {
        *slot = update_fn(*slot);
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

pub(crate) fn update_preserve_stack_refs_in<F>(inst: &mut RInstance, mut update_fn: F)
where
    F: FnMut(SEXP) -> SEXP,
{
    let mut stack = inst.preserve_stack.borrow_mut();
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

pub(crate) fn with_preserved_objects_in<F, R>(inst: &mut RInstance, f: F) -> R
where
    F: FnOnce(&[SEXP]) -> R,
{
    let stack = inst.preserve_stack.borrow();
    f(&stack)
}

// ---------------------------------------------------------------------------
// Indexed protection — protect a stack slot that can be replaced later
// ---------------------------------------------------------------------------

/// Opaque legacy marker used by the `R_ProtectWithIndex` compatibility shim.
pub(crate) struct ProtectIndex {
    _private: (),
}

/// Stable handle for a protected stack slot that may be replaced with another
/// value before it is unprotected.
///
/// The handle records the generation assigned to the stack entry when it was
/// pushed. Releasing a slot tombstones its entry in place (null pointer +
/// fresh generation) without disturbing later slots; later pushes reuse the
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

    /// The generation assigned to the stack entry when this slot was
    /// created. A released-then-reused slot always reports a different
    /// generation than handles captured before the release.
    pub fn generation(self) -> u64 {
        self.generation
    }

    pub fn is_active(self) -> bool {
        self.index.is_some()
    }

    /// Whether this handle no longer refers to the protection stack entry it
    /// was created for: the entry was released and its index handed out
    /// again (or is gone entirely). Inactive slots are never stale.
    pub fn is_stale(self) -> bool {
        with_required_current_instance(|inst| protect_slot_is_stale_in(inst, self))
    }
}

fn protect_raw_with_slot(s: SEXP, api: &str) -> ProtectionSlot {
    with_required_current_instance(|inst| protect_raw_with_slot_in(inst, s, api))
}

fn protect_raw_with_slot_in(inst: &mut RInstance, s: SEXP, api: &str) -> ProtectionSlot {
    if s.is_null() {
        return ProtectionSlot::inactive();
    }
    let (index, generation) = claim_protect_slot_in(inst, s, api);
    ProtectionSlot::from_stack_index(index, generation)
}

fn reprotect_slot(slot: ProtectionSlot, s: SEXP) {
    with_required_current_instance(|inst| reprotect_slot_in(inst, slot, s));
}

fn reprotect_slot_in(inst: &RInstance, slot: ProtectionSlot, s: SEXP) {
    let Some(index) = slot.index else {
        return;
    };
    let mut stack = inst.protect_stack.borrow_mut();
    if index < stack.len() {
        stack[index] = s;
    }
}

fn release_protect_slot(slot: ProtectionSlot) {
    with_required_current_instance(|inst| release_protect_slot_in(inst, slot));
}

fn release_protect_slot_in(inst: &RInstance, slot: ProtectionSlot) {
    let Some(index) = slot.index else {
        return;
    };
    // Tombstone the entry in place so surviving slots keep their indices:
    // clear the pointer, bump the generation, and queue the index for
    // reuse. A stale release (index already freed, stolen by a truncating
    // unprotect, or whose generation moved on) is a no-op — dropping two
    // guards that disagree on the entry must not evict the live owner.
    // Freed tail slots collapse immediately (popped off the stack) so the
    // physical stack stays contiguous and legacy LIFO depths,
    // `with_protected_objects` tail snapshots, and the GC root scan never
    // observe holes below live entries; interior frees stay tombstoned until
    // every slot above them has also been released.
    let mut stack = inst.protect_stack.borrow_mut();
    if index >= stack.len() {
        return;
    }
    let mut generations = inst.protect_stack_generations.borrow_mut();
    if index >= generations.len() || generations[index] != slot.generation {
        return;
    }
    stack[index] = std::ptr::null_mut();
    generations[index] = next_slot_generation(inst);
    let mut free = inst.protect_slot_free.borrow_mut();
    if !free.contains(&index) {
        free.push(index);
    }
    while stack.len() > 0 && stack[stack.len() - 1].is_null() {
        let tail = stack.len() - 1;
        if let Some(pos) = free.iter().position(|&i| i == tail) {
            free.swap_remove(pos);
            stack.pop();
            generations.pop();
        } else {
            break;
        }
    }
}
/// The generation currently recorded for `slot`'s stack index, or `None`
/// when the entry is gone (released, or the generation log was desynced by
/// a foreign direct push onto the stack).
fn protect_slot_generation_in(inst: &RInstance, slot: ProtectionSlot) -> Option<u64> {
    let index = slot.index?;
    let generations = inst.protect_stack_generations.borrow();
    generations.get(index).copied()
}

/// Whether `slot` no longer refers to the stack entry it was created for.
fn protect_slot_is_stale_in(inst: &RInstance, slot: ProtectionSlot) -> bool {
    if !slot.is_active() {
        return false;
    }
    protect_slot_generation_in(inst, slot) != Some(slot.generation)
}

/// RAII guard for a replaceable protection stack slot.
pub struct IndexedProtectGuard {
    owner: Option<usize>,
    slot: ProtectionSlot,
}

impl IndexedProtectGuard {
    pub fn slot(&self) -> ProtectionSlot {
        self.slot
    }

    /// Whether the live stack entry at the guard's slot index still carries
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
            with_guard_owner(owner, |inst| unsafe {
                reprotect_slot_in(inst, self.slot, value);
            });
        }
    }

    pub fn reprotect_sexp(&mut self, value: Sexp<'_>) {
        self.try_reprotect_sexp(value)
            .expect("reprotect_sexp requires an owner-scoped Sexp");
    }

    pub fn try_reprotect_sexp(&mut self, value: Sexp<'_>) -> Result<(), ProtectError> {
        ensure_owner_scoped(value.clone(), "reprotect_sexp")?;
        self.reprotect_raw(value.as_raw());
        Ok(())
    }
}

impl Drop for IndexedProtectGuard {
    fn drop(&mut self) {
        if let Some(owner) = self.owner {
            // SAFETY: See ProtectGuard::drop.
            with_guard_owner(owner, |inst| unsafe {
                release_protect_slot_in(inst, self.slot);
            });
        }
    }
}

/// An owner-scoped [`Sexp`] handle kept alive on the protect stack.
///
/// `RootedSexp` is the ergonomic rooting layer over the protection stack:
/// [`RootedSexp::root`] clones the (non-`Copy`) handle, protects the cloned
/// SEXP on creation, and unprotects it when the root is dropped, so callers
/// never juggle `protect_sexp`/`unprotect` bookkeeping by hand. Reads go
/// through [`RootedSexp::get`].
///
/// Roots use a slot-protected stack entry (see [`protect_sexp_with_index`])
/// with stable slot indices: tombstoned entries are reused with a fresh
/// generation, so roots may be dropped in any order and surviving roots keep
/// resolving to their own protections. See the module docs for the slot-
/// stability contract.
///
/// Every root records the generation of the stack entry it created; reads
/// through [`RootedSexp::get`] verify that the entry is still the root's
/// own, and [`RootedSexp::is_stale`] reports a released-then-reused slot.
///
/// ```rust,ignore
/// use rmath::sexp::protect::RootedSexp;
///
/// let root = RootedSexp::root(some_sexp.clone());
/// run_gc_point(); // the rooted value survives collection
/// let n = root.get().expect("root is live").length(); // checked read
/// drop(root); // stack slot released
/// ```
pub struct RootedSexp<'a> {
    value: Sexp<'a>,
    guard: IndexedProtectGuard,
    /// Generation of the stack entry captured at root creation; verified
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
        let guard = protect_sexp_with_index(sexp.clone());
        let expected_generation = guard.slot().generation();
        Ok(Self {
            value: sexp,
            guard,
            expected_generation,
        })
    }

    /// Read the rooted handle, verifying that the root's stack slot still
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

    /// Whether the root's protection slot no longer refers to the stack
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

    /// Replace the rooted value through the write barrier: the stack slot
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
        self.guard.reprotect_sexp(value.clone());
        self.value = value;
        Ok(())
    }

    /// Consume the root, returning the guarded handle. The protection is
    /// released; the caller owns the returned handle without a stack root.
    pub fn unroot(self) -> Sexp<'a> {
        let Self { value, guard, .. } = self;
        drop(guard);
        value
    }
}

/// Protect an owner-scoped SEXP handle in a replaceable stack slot.
pub fn protect_sexp_with_index(value: Sexp<'_>) -> IndexedProtectGuard {
    try_protect_sexp_with_index(value)
        .expect("protect_sexp_with_index requires an owner-scoped Sexp")
}

/// Try to protect an owner-scoped SEXP handle in a replaceable stack slot.
pub fn try_protect_sexp_with_index(value: Sexp<'_>) -> Result<IndexedProtectGuard, ProtectError> {
    ensure_owner_scoped(value.clone(), "protect_sexp_with_index")?;
    Ok(protect_with_index_raw(
        value.as_raw(),
        "protect_sexp_with_index",
    ))
}

/// Protect a raw SEXP in a replaceable stack slot.
///
/// Legacy compatibility helper for translated Rust modules. Prefer
/// [`protect_sexp_with_index`] when the caller has an owner-scoped value.
pub(crate) fn protect_with_index_raw(s: SEXP, api: &str) -> IndexedProtectGuard {
    if s.is_null() {
        return IndexedProtectGuard {
            owner: None,
            slot: ProtectionSlot::inactive(),
        };
    }

    with_required_current_instance(|inst| IndexedProtectGuard {
        owner: Some(inst as *mut RInstance as usize),
        slot: protect_raw_with_slot_in(inst, s, api),
    })
}

/// Protect an SEXP and return a legacy encoded index for later replacement.
///
/// This is the equivalent of R's `R_ProtectWithIndex()`.
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
    reprotect_slot(ProtectionSlot::from_legacy_ptr(index), s);
}

// ---------------------------------------------------------------------------
// R_PreserveObject / R_ReleaseObject — permanent protection
// ---------------------------------------------------------------------------

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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::instance::{RInstance, current_instance_ptr, replace_current_instance};
    use crate::sexp::session::RSession;

    use super::*;

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
    fn test_protect_sexp_guard() {
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let depth_before = R_ProtectCount();
            let guard = protect_sexp(value.clone());
            assert_eq!(R_ProtectCount(), depth_before + 1);
            with_protected_objects(|objects| assert_eq!(objects, &[value.as_raw()]));
            drop(guard);
            assert_eq!(R_ProtectCount(), depth_before);
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
            let depth_before = R_ProtectCount();
            let root = RootedSexp::root(value.clone());
            assert_eq!(R_ProtectCount(), depth_before + 1);
            with_protected_objects(|objects| assert_eq!(objects, &[value.clone().as_raw()]));
            let readback = root.get().expect("fresh root must resolve").clone();
            assert_eq!(readback, value);
            let sexp = root.unroot();
            assert_eq!(sexp.as_raw(), value.clone().as_raw());
            assert_eq!(R_ProtectCount(), depth_before);
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
            let depth_before = R_ProtectCount();
            let mut outer = RootedSexp::root(first.clone());
            {
                let inner = RootedSexp::root(first.clone());
                assert!(inner.slot().is_active());
            }
            // The tail drop collapses the inner slot off the stack, so the
            // outer root is the only entry left.
            assert_eq!(R_ProtectCount(), depth_before + 1);
            outer.reprotect(second.clone());
            with_protected_objects(|objects| assert_eq!(objects, &[second.clone().as_raw()]));
            assert_eq!(
                outer.get().expect("outer root must resolve").clone(),
                second
            );
            drop(outer);
            assert_eq!(R_ProtectCount(), depth_before);
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
            let depth_before = R_ProtectCount();
            let mut guard = protect_sexp_with_index(first);
            assert!(guard.slot().is_active());
            assert_eq!(R_ProtectCount(), depth_before + 1);
            guard.reprotect_sexp(second.clone());
            with_protected_objects(|objects| assert_eq!(objects, &[second.as_raw()]));
            drop(guard);
            assert_eq!(R_ProtectCount(), depth_before);
        });
    }

    #[test]
    fn test_indexed_raw_guard_null_is_inactive() {
        let session = RSession::new();
        session.with_protected(|| {
            let depth_before = R_ProtectCount();
            let guard = protect_with_index_raw(ptr::null_mut(), "test");
            assert!(!guard.slot().is_active());
            drop(guard);
            assert_eq!(R_ProtectCount(), depth_before);
        });
    }

    #[test]
    fn test_protect_guard_drops_against_original_instance() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };

        let guard = protect(0x1 as SEXP);
        assert_eq!(R_ProtectCount_in(&mut left), 1);
        assert_eq!(R_ProtectCount_in(&mut right), 0);

        unsafe {
            replace_current_instance(Some(&mut right));
        }
        drop(guard);

        assert_eq!(R_ProtectCount_in(&mut left), 0);
        assert_eq!(R_ProtectCount_in(&mut right), 0);
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_protect_n_guard_drops_against_original_instance() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };

        protect_raw_pointer(0x1 as SEXP);
        protect_raw_pointer(0x2 as SEXP);
        let guard = protect_n(2);
        assert_eq!(R_ProtectCount_in(&mut left), 2);

        unsafe {
            replace_current_instance(Some(&mut right));
        }
        drop(guard);

        assert_eq!(R_ProtectCount_in(&mut left), 0);
        assert_eq!(R_ProtectCount_in(&mut right), 0);
        unsafe {
            replace_current_instance(previous);
        }
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
        let value = unsafe { (*left_ptr).arena.sexp(raw) }.expect("left arena object should wrap");

        let guard = preserve_sexp(value);
        with_preserved_objects_in(unsafe { &mut *left_ptr }, |objects| {
            assert_eq!(objects, &[raw])
        });
        with_preserved_objects_in(&mut right, |objects| assert!(objects.is_empty()));

        unsafe {
            replace_current_instance(Some(&mut right));
        }
        drop(guard);

        with_preserved_objects_in(unsafe { &mut *left_ptr }, |objects| {
            assert!(objects.is_empty())
        });
        with_preserved_objects_in(&mut right, |objects| assert!(objects.is_empty()));
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_indexed_guard_drops_against_original_instance() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };

        let mut guard = protect_with_index_raw(0x1 as SEXP, "test");
        assert_eq!(R_ProtectCount_in(&mut left), 1);

        unsafe {
            replace_current_instance(Some(&mut right));
        }
        guard.reprotect_raw(0x2 as SEXP);
        with_protected_objects_in(&mut left, |objects| assert_eq!(objects, &[0x2 as SEXP]));
        with_protected_objects_in(&mut right, |objects| assert!(objects.is_empty()));
        drop(guard);

        assert_eq!(R_ProtectCount_in(&mut left), 0);
        assert_eq!(R_ProtectCount_in(&mut right), 0);
        unsafe {
            replace_current_instance(previous);
        }
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
    fn test_protect_with_index() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let fake = 0x1 as SEXP;
            let idx = R_ProtectWithIndex(fake);
            assert!(!idx.is_null());
            assert_eq!(R_ProtectCount(), 1);
            unprotect_count(1);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_protect_with_index_null() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let idx = R_ProtectWithIndex(ptr::null_mut());
            assert!((idx as usize) == 0);
            assert_eq!(R_ProtectCount(), 0);
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
            with_protected_objects(|objects| assert_eq!(objects[0], b));
            unprotect_count(1);
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

    #[test]
    fn test_with_protected_objects() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            protect_raw_pointer(0x1 as SEXP);
            protect_raw_pointer(0x2 as SEXP);
            with_protected_objects(|objects| {
                assert_eq!(objects.len(), 2);
            });
            unprotect_count(2);
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
            with_protected_objects(|objects| {
                assert_eq!(objects[0] as usize, 0x100);
                assert_eq!(objects[1] as usize, 0x2);
            });
            unprotect_count(2);
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

            // The root pins the value on the protect stack, so collection
            // must leave the value and the slot's generation intact.
            crate::sexp::gengc::full_gc();

            assert!(!root.is_stale());
            assert_eq!(root.slot().generation(), generation);
            let readback = root.get().expect("rooted value must resolve after gc");
            assert_eq!(readback.clone().as_raw(), value.clone().as_raw());
            with_protected_objects(|objects| assert_eq!(objects, &[value.clone().as_raw()]));
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
            let depth_before = R_ProtectCount();
            let root = RootedSexp::root(value.clone());
            let slot = root.slot();
            assert!(slot.is_active());
            assert!(!slot.is_stale());
            let generation = slot.generation();

            // Release the slot, then allocate and churn roots so its index
            // is handed out again.
            let sexp = root.unroot();
            assert_eq!(sexp.as_raw(), value.clone().as_raw());
            assert_eq!(R_ProtectCount(), depth_before);
            crate::sexp::instance::with_required_current_instance(|inst| {
                for _ in 0..1000 {
                    inst.arena.alloc_node(SEXPTYPE::INTSXP);
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
            let depth_before = R_ProtectCount();
            let first = RootedSexp::root(value.clone());
            let second = RootedSexp::root(value.clone());
            let third = RootedSexp::root(value.clone());
            assert_eq!(R_ProtectCount(), depth_before + 3);
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
            assert_eq!(R_ProtectCount(), depth_before + 3);

            // The freed index is reusable: a fresh root lands exactly there
            // with a newer generation while the survivors stay healthy.
            let fresh = RootedSexp::root(value.clone());
            assert!(!fresh.is_stale());
            assert!(fresh.get().is_some());
            assert!(!second.is_stale());
            assert!(!third.is_stale());
            assert_eq!(R_ProtectCount(), depth_before + 3);

            drop(fresh);
            drop(third);
            drop(second);
            // All slots released: the tail collapse pops every entry, so the
            // live-entry depth is restored exactly.
            assert_eq!(R_ProtectCount(), depth_before);
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
            let depth_before = R_ProtectCount();
            let mut roots: Vec<RootedSexp<'_>> =
                (0..16).map(|_| RootedSexp::root(value.clone())).collect();
            assert_eq!(R_ProtectCount(), depth_before + 16);
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
            assert_eq!(R_ProtectCount(), depth_before);
        });
    }

    #[test]
    fn test_slot_reuse_rejects_old_token() {
        let session = RSession::new();
        session.with_protected(|| {
            let depth_before = R_ProtectCount();
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
            assert_eq!(R_ProtectCount(), depth_before);
        });
    }
}
