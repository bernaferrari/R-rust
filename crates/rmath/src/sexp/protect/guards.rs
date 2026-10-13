#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
#![deny(unsafe_op_in_unsafe_fn)]
use super::*;

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
/// Internal translation helper. Safe embedding handles root themselves
/// automatically; embedding callers never manipulate this table.
pub(crate) fn protect_sexp<'a>(value: Sexp<'a>) -> ProtectGuard<'a> {
    try_protect_sexp(value).expect("protect_sexp requires an owner-scoped Sexp")
}

/// Try to protect an owner-scoped SEXP handle.
pub(crate) fn try_protect_sexp<'a>(value: Sexp<'a>) -> Result<ProtectGuard<'a>, ProtectError> {
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
        let value = unsafe { RootValue::from_owner(inst, s) }.expect("invalid protection owner");
        unsafe { (*inst).legacy_protect.push(value, "protect") };
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
    let value = unsafe { RootValue::from_owner(inst, s) }.expect("invalid unprotect owner");
    unsafe { (*inst).legacy_protect.remove_topmost(&value) };
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
pub(crate) struct ProtectionSlot {
    pub(crate) index: Option<usize>,
    pub(crate) generation: u64,
}

impl ProtectionSlot {
    fn inactive() -> Self {
        Self {
            index: None,
            generation: 0,
        }
    }

    pub(crate) fn storage_id(self) -> Option<SlotId> {
        Some(SlotId {
            index: self.index?,
            generation: self.generation,
        })
    }

    pub(crate) fn from_stack_index(index: usize, generation: u64) -> Self {
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
    pub(crate) fn generation(self) -> u64 {
        self.generation
    }

    pub(crate) fn is_active(self) -> bool {
        self.index.is_some()
    }

    /// Whether this handle no longer refers to the root-table entry it was
    /// created for: the entry was released and its index handed out again
    /// (or is gone entirely). Inactive slots are never stale.
    pub(crate) fn is_stale(self) -> bool {
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

pub(crate) fn reprotect_slot(slot: ProtectionSlot, s: SEXP) {
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

pub(crate) fn release_protect_slot(slot: ProtectionSlot) {
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
pub(crate) struct IndexedProtectGuard<'a> {
    owner: Option<GuardOwner>,
    slot: ProtectionSlot,
    value_owner: SexpOwner,
    _confined: Confined<'a>,
}

impl<'a> IndexedProtectGuard<'a> {
    pub(crate) fn slot(&self) -> ProtectionSlot {
        self.slot
    }

    /// Whether the live table entry at the guard's slot index still carries
    /// `expected` as its generation, checked against the guard's OWNING
    /// instance (see [`with_guard_owner`]). Inactive slots (null-SEXP
    /// protections) trivially match — there is no entry to go stale.
    pub(crate) fn slot_generation_is(&self, expected: u64) -> bool {
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

    pub(crate) fn reprotect_sexp(&mut self, value: Sexp<'a>) {
        self.try_reprotect_sexp(value)
            .expect("reprotect_sexp requires an owner-scoped Sexp");
    }

    pub(crate) fn try_reprotect_sexp(&mut self, value: Sexp<'a>) -> Result<(), ProtectError> {
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
pub(crate) struct RootedSexp<'a> {
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
    pub(crate) fn root(sexp: Sexp<'a>) -> Self {
        Self::try_root(sexp).expect("RootedSexp::root requires an owner-scoped Sexp")
    }

    /// Like [`root`](RootedSexp::root), but reports unowned handles as
    /// [`ProtectError::UnownedHandle`] instead of panicking.
    pub(crate) fn try_root(sexp: Sexp<'a>) -> Result<Self, ProtectError> {
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
    /// [`is_stale`](RootedSexp::is_stale)), or its allocation was reclaimed.
    pub(crate) fn get(&self) -> Option<&Sexp<'a>> {
        let stale = self.is_stale();
        if stale { None } else { Some(&self.value) }
    }

    /// Whether the root's protection slot no longer refers to the table
    /// entry created for it — the root was released (or displaced by an
    /// out-of-order drop) and the slot handed out again. Checked reads via
    /// [`get`](RootedSexp::get) report the mismatch as `None`.
    pub(crate) fn is_stale(&self) -> bool {
        !self.value.is_live() || !self.guard.slot_generation_is(self.expected_generation)
    }

    /// The underlying protection slot, for callers that need to reprotect
    /// the rooted value in place (write barrier).
    pub(crate) fn slot(&self) -> ProtectionSlot {
        self.guard.slot()
    }

    /// Replace the rooted value through the write barrier: the table slot
    /// now protects `value` and the guarded handle is updated to alias it.
    ///
    /// # Panics
    /// Panics if `value` is not owner-scoped.
    pub(crate) fn reprotect(&mut self, value: Sexp<'a>) {
        self.try_reprotect(value)
            .expect("RootedSexp::reprotect requires an owner-scoped Sexp");
    }

    /// Non-panicking variant of [`reprotect`](RootedSexp::reprotect).
    pub(crate) fn try_reprotect(&mut self, value: Sexp<'a>) -> Result<(), ProtectError> {
        ensure_owner_scoped(value.clone(), "RootedSexp::reprotect")?;
        self.guard.try_reprotect_sexp(value.clone())?;
        self.value = value;
        Ok(())
    }

    /// Consume the root, returning the guarded handle. The protection is
    /// released; a checked session handle retains its own shared root lease.
    pub(crate) fn unroot(self) -> Sexp<'a> {
        let Self { value, guard, .. } = self;
        drop(guard);
        value
    }
}

/// Protect an owner-scoped SEXP handle in a replaceable root-table slot.
pub(crate) fn protect_sexp_with_index<'a>(value: Sexp<'a>) -> IndexedProtectGuard<'a> {
    try_protect_sexp_with_index(value)
        .expect("protect_sexp_with_index requires an owner-scoped Sexp")
}

/// Try to protect an owner-scoped SEXP handle in a replaceable root-table
/// slot.
pub(crate) fn try_protect_sexp_with_index<'a>(
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

pub(crate) fn try_push_preserve_in(
    inst: *mut RInstance,
    s: SEXP,
) -> Result<Option<SequenceLease>, ProtectError> {
    if s.is_null() {
        return Ok(None);
    }
    let value = unsafe { RootValue::from_owner(inst, s) }?;
    unsafe { (*inst).preserve_stack.try_push(value) }.map(Some)
}

fn push_preserve(s: SEXP) {
    with_required_current_instance(|inst| push_preserve_in(inst, s));
}

pub(crate) fn release_preserved_in(inst: *mut RInstance, s: SEXP) {
    if s.is_null() {
        return;
    }
    let value = unsafe { RootValue::from_owner(inst, s) }.expect("invalid preserve release owner");
    unsafe {
        (*inst).preserve_stack.remove_first(&value);
    }
}

fn release_preserved(s: SEXP) {
    with_required_current_instance(|inst| release_preserved_in(inst, s));
}

/// RAII guard for the preserve stack.
///
/// Dropping the guard releases the preserved object from the owning session.
/// Like every protection guard it is `!Send + !Sync` ([`Confined`]).
pub(crate) struct PreserveGuard<'a> {
    pub(crate) owner: Option<GuardOwner>,
    pub(crate) lease: Option<SequenceLease>,
    pub(crate) _confined: Confined<'a>,
}

impl Drop for PreserveGuard<'_> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.as_ref().filter(|owner| owner.is_live()) {
            // SAFETY: See ProtectGuard::drop.
            if let Some(lease) = self.lease {
                with_guard_owner(owner, |inst| unsafe {
                    (*inst).preserve_stack.remove_lease(lease);
                });
            }
        }
    }
}

/// Preserve an owner-scoped SEXP handle until the returned guard is dropped.
pub(crate) fn preserve_sexp<'a>(value: Sexp<'a>) -> PreserveGuard<'a> {
    try_preserve_sexp(value).expect("preserve_sexp requires an owner-scoped Sexp")
}

/// Try to preserve an owner-scoped SEXP handle until the returned guard is
/// dropped.
pub(crate) fn try_preserve_sexp<'a>(value: Sexp<'a>) -> Result<PreserveGuard<'a>, ProtectError> {
    ensure_owner_scoped(value.clone(), "preserve_sexp")?;
    let raw = value.clone().as_raw();
    let owner = session_owner_handle(&value);
    let lease = if let Some(owner) = owner.as_ref() {
        if !owner.is_live() {
            return Err(ProtectError::OwnerUnavailable);
        }
        with_guard_owner(owner, |inst| try_push_preserve_in(inst, raw))?
    } else {
        None
    };
    Ok(PreserveGuard {
        owner,
        lease,
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
        SexpOwner::Session(_) => {
            if let Some(owner) = &value.runtime_owner {
                let pin = owner.pin().ok()?;
                Some(unsafe { GuardOwner::new(pin.as_ptr()) })
            } else {
                value
                    .session_owner_ptr
                    .map(|pointer| unsafe { GuardOwner::new(pointer.as_ptr()) })
            }
        }
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
/// updates use this compatibility view; marking consumes exact allocation
/// identities through `RootTable::checked_entries_snapshot`.
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
    let liveness = unsafe { crate::sexp::instance::instance_liveness(inst) };
    let legacy = unsafe { (*inst).legacy_protect.updates_snapshot() };
    let roots = unsafe { (*inst).root_table.updates_snapshot() };
    for (lease, expected) in legacy {
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
            .expect("legacy root update returned an invalid owner projection");
        unsafe {
            (*inst)
                .legacy_protect
                .replace_if_current(lease, &expected, replacement);
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
    let liveness = unsafe { crate::sexp::instance::instance_liveness(inst) };
    let snapshot = unsafe { (*inst).preserve_stack.updates_snapshot() };
    for (lease, expected) in snapshot {
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
            .expect("preserved root update returned an invalid owner projection");
        unsafe {
            (*inst)
                .preserve_stack
                .replace_if_current(lease, &expected, replacement);
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
    let stack = unsafe { (*inst).preserve_stack.entries_snapshot() };
    f(&stack)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
