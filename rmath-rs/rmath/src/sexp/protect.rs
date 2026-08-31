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
//!   reads through [`Deref`]. Lower-level callers can use
//!   [`protect_sexp`]/[`ProtectGuard`] or the replaceable-slot
//!   [`protect_sexp_with_index`]/[`IndexedProtectGuard`] directly.
//! * Handles to objects that may be *replaced* during evaluation (grown
//!   vectors, PROMSXP re-promises) must be refreshed through the write
//!   barrier — re-derive the handle from its owner or use
//!   [`IndexedProtectGuard::reprotect_sexp`] on a slot-protected value —
//!   never by mutating a raw pointer in place.
//!
//! # Drop-order contract
//!
//! The protect stack is a plain `Vec` and slot release (`Vec::remove`)
//! shifts later indices, so guards — including [`RootedSexp`] — must be
//! released in **reverse creation order** (the natural order for RAII
//! scopes). Roadmap: a generation-based handle table would pin slots
//! permanently and remove this LIFO constraint; until then the stack
//! semantics stay as upstream R's.

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

pub(crate) fn push_protect_in(inst: &mut RInstance, s: SEXP) {
    if !s.is_null() {
        let mut stack = inst.protect_stack.borrow_mut();
        reserve_slot_or_fail(&mut stack, "protect");
        stack.push(s);
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
    } else {
        stack.truncate(len - n);
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
    }
}

/// Get the current number of entries on the protection stack.
///
/// Used by the context system to track protect depth.
pub(crate) fn R_ProtectCount() -> usize {
    with_required_current_instance(R_ProtectCount_in)
}

pub(crate) fn R_ProtectCount_in(inst: &mut RInstance) -> usize {
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtectionSlot {
    index: Option<usize>,
}

impl ProtectionSlot {
    fn inactive() -> Self {
        Self { index: None }
    }

    fn from_stack_index(index: usize) -> Self {
        Self { index: Some(index) }
    }

    fn from_legacy_ptr(index: *mut ProtectIndex) -> Self {
        let raw = index as usize;
        if raw == 0 {
            Self::inactive()
        } else {
            Self::from_stack_index(raw - 1)
        }
    }

    fn into_legacy_ptr(self) -> *mut ProtectIndex {
        self.index
            .map(|index| (index + 1) as *mut ProtectIndex)
            .unwrap_or(std::ptr::null_mut())
    }

    pub fn is_active(self) -> bool {
        self.index.is_some()
    }
}

fn protect_raw_with_slot(s: SEXP, api: &str) -> ProtectionSlot {
    with_required_current_instance(|inst| protect_raw_with_slot_in(inst, s, api))
}

fn protect_raw_with_slot_in(inst: &mut RInstance, s: SEXP, api: &str) -> ProtectionSlot {
    if s.is_null() {
        return ProtectionSlot::inactive();
    }
    let mut stack = inst.protect_stack.borrow_mut();
    reserve_slot_or_fail(&mut stack, api);
    stack.push(s);
    ProtectionSlot::from_stack_index(stack.len() - 1)
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
    let mut stack = inst.protect_stack.borrow_mut();
    if index < stack.len() {
        stack.remove(index);
    }
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
/// through [`Deref`] to the guarded handle.
///
/// Roots use a slot-protected stack entry (see [`protect_sexp_with_index`])
/// so an individual root's stack slot is stable across evaluations that push
/// and pop other protections. Like every guard over this `Vec`-backed stack,
/// roots should still be dropped in reverse creation order; see the module
/// docs for the ownership model and the planned generation-based handle
/// table that would remove that constraint.
///
/// ```rust,ignore
/// use rmath::sexp::protect::RootedSexp;
///
/// let root = RootedSexp::root(some_sexp.clone());
/// run_gc_point(); // the rooted value survives collection
/// let n = root.length(); // deref read through the rooted handle
/// drop(root); // stack slot released
/// ```
pub struct RootedSexp<'a> {
    value: Sexp<'a>,
    guard: IndexedProtectGuard,
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
        Ok(Self { value: sexp, guard })
    }

    /// Read the rooted handle.
    pub fn get(&self) -> &Sexp<'a> {
        &self.value
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
        let Self { value, guard } = self;
        drop(guard);
        value
    }
}

impl<'a> std::ops::Deref for RootedSexp<'a> {
    type Target = Sexp<'a>;

    fn deref(&self) -> &Sexp<'a> {
        &self.value
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
            // Deref exposes the guarded handle.
            let readback = root.get().clone();
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
            // LIFO release of the inner root left exactly one entry.
            assert_eq!(R_ProtectCount(), depth_before + 1);
            outer.reprotect(second.clone());
            with_protected_objects(|objects| assert_eq!(objects, &[second.clone().as_raw()]));
            assert_eq!(outer.get().clone(), second);
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
        let value = unsafe { (*left_ptr).arena.sexp(raw) }
            .expect("left arena object should wrap");

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
}
