#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
#![deny(unsafe_op_in_unsafe_fn)]

//! R's PROTECT/UNPROTECT mechanism.
//!
//! R maintains a protection stack to prevent GC from collecting objects that
//! are only referenced by local variables. In this Rust port the stack is owned
//! by the active `RInstance`; new Rust code should protect owner-scoped
//! [`Sexp`](super::object::Sexp) handles instead of raw pointers.

use super::ffi::SEXP;
use super::object::Sexp;

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
    count: usize,
}

impl Drop for ProtectGuard {
    fn drop(&mut self) {
        if self.count > 0 {
            unprotect_count(self.count);
        }
    }
}

/// Protect an owner-scoped SEXP handle and return an RAII guard.
///
/// This is the Rust API exposed to embedders. Raw pointer protection remains
/// crate-local translation scaffolding for ported interpreter modules.
pub fn protect_sexp(value: Sexp<'_>) -> ProtectGuard {
    protect_raw(value.as_raw())
}

/// Protect a raw SEXP and return an RAII guard.
///
/// Legacy compatibility helper for translated code. Prefer
/// [`protect_sexp`] when the caller has an owner-scoped value.
pub(crate) fn protect(s: SEXP) -> ProtectGuard {
    protect_raw(s)
}

fn protect_raw(s: SEXP) -> ProtectGuard {
    push_protect(s);
    ProtectGuard {
        count: if s.is_null() { 0 } else { 1 },
    }
}

/// Create a guard that will pop `n` stack entries on drop.
///
/// Callers must already have pushed `n` entries and want RAII-style unwinding
/// safety around a manual protect batch.
pub(crate) fn protect_n(n: usize) -> ProtectGuard {
    ProtectGuard { count: n }
}

fn reserve_slot_or_fail(stack: &mut Vec<SEXP>, api: &str) {
    if stack.try_reserve(1).is_err() {
        panic!("{api}: protection stack allocation failed");
    }
}

fn push_protect(s: SEXP) {
    if !s.is_null() {
        super::instance::with_required_current_instance(|inst| {
            reserve_slot_or_fail(&mut inst.protect_stack, "protect");
            inst.protect_stack.push(s);
        });
    }
}

fn push_preserve(s: SEXP) {
    if !s.is_null() {
        super::instance::with_required_current_instance(|inst| {
            reserve_slot_or_fail(&mut inst.preserve_stack, "preserve");
            inst.preserve_stack.push(s);
        });
    }
}

fn release_preserved(s: SEXP) {
    if s.is_null() {
        return;
    }
    super::instance::with_required_current_instance(|inst| {
        if let Some(pos) = inst.preserve_stack.iter().position(|&x| x == s) {
            inst.preserve_stack.remove(pos);
        }
    });
}

/// RAII guard for the preserve stack.
///
/// Dropping the guard releases the preserved object from the active session.
pub struct PreserveGuard {
    value: SEXP,
}

impl Drop for PreserveGuard {
    fn drop(&mut self) {
        release_preserved(self.value);
    }
}

/// Preserve an owner-scoped SEXP handle until the returned guard is dropped.
pub fn preserve_sexp(value: Sexp<'_>) -> PreserveGuard {
    let raw = value.as_raw();
    push_preserve(raw);
    PreserveGuard { value: raw }
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
pub(crate) fn unprotect_count(n: usize) {
    if n == 0 {
        return;
    }
    super::instance::with_required_current_instance(|inst| {
        let len = inst.protect_stack.len();
        if n >= len {
            inst.protect_stack.clear();
        } else {
            inst.protect_stack.truncate(len - n);
        }
    });
}

/// Unprotect the top entry from the protection stack.
///
/// This is the equivalent of R's `UNPROTECT_PTR()` macro.
pub(crate) fn unprotect_ptr(s: SEXP) {
    if s.is_null() {
        return;
    }
    super::instance::with_required_current_instance(|inst| {
        if let Some(pos) = inst.protect_stack.iter().rposition(|&x| x == s) {
            inst.protect_stack.remove(pos);
        }
    });
}

/// Get the current number of entries on the protection stack.
///
/// Used by the context system to track protect depth.
pub(crate) fn R_ProtectCount() -> usize {
    super::instance::with_required_current_instance(|inst| inst.protect_stack.len())
}

/// Iterate over all protected SEXP values on the stack.
/// Used by the GC to mark protected objects.
pub(crate) fn with_protected_objects<F, R>(f: F) -> R
where
    F: FnOnce(&[SEXP]) -> R,
{
    super::instance::with_required_current_instance(|inst| f(&inst.protect_stack))
}

/// Update all protect stack references using the given mapping function.
/// Used by the GC compaction phase to update moved object pointers.
pub(crate) fn update_protect_stack_refs<F>(mut update_fn: F)
where
    F: FnMut(SEXP) -> SEXP,
{
    super::instance::with_required_current_instance(|inst| {
        for slot in inst.protect_stack.iter_mut() {
            *slot = update_fn(*slot);
        }
    });
}

/// Update all preserve stack references using the given mapping function.
/// Used by the GC compaction phase to update moved object pointers.
pub(crate) fn update_preserve_stack_refs<F>(mut update_fn: F)
where
    F: FnMut(SEXP) -> SEXP,
{
    super::instance::with_required_current_instance(|inst| {
        for slot in inst.preserve_stack.iter_mut() {
            *slot = update_fn(*slot);
        }
    });
}

/// Iterate over all preserved SEXP values.
/// Used by the GC to mark preserved objects.
pub(crate) fn with_preserved_objects<F, R>(f: F) -> R
where
    F: FnOnce(&[SEXP]) -> R,
{
    super::instance::with_required_current_instance(|inst| f(&inst.preserve_stack))
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
    if s.is_null() {
        return ProtectionSlot::inactive();
    }
    super::instance::with_required_current_instance(|inst| {
        reserve_slot_or_fail(&mut inst.protect_stack, api);
        inst.protect_stack.push(s);
        ProtectionSlot::from_stack_index(inst.protect_stack.len() - 1)
    })
}

fn reprotect_slot(slot: ProtectionSlot, s: SEXP) {
    let Some(index) = slot.index else {
        return;
    };
    super::instance::with_required_current_instance(|inst| {
        if index < inst.protect_stack.len() {
            inst.protect_stack[index] = s;
        }
    });
}

fn release_protect_slot(slot: ProtectionSlot) {
    let Some(index) = slot.index else {
        return;
    };
    super::instance::with_required_current_instance(|inst| {
        if index < inst.protect_stack.len() {
            inst.protect_stack.remove(index);
        }
    });
}

/// RAII guard for a replaceable protection stack slot.
pub struct IndexedProtectGuard {
    slot: ProtectionSlot,
}

impl IndexedProtectGuard {
    pub fn slot(&self) -> ProtectionSlot {
        self.slot
    }

    pub(crate) fn reprotect_raw(&mut self, value: SEXP) {
        reprotect_slot(self.slot, value);
    }

    pub fn reprotect_sexp(&mut self, value: Sexp<'_>) {
        self.reprotect_raw(value.as_raw());
    }
}

impl Drop for IndexedProtectGuard {
    fn drop(&mut self) {
        release_protect_slot(self.slot);
    }
}

/// Protect an owner-scoped SEXP handle in a replaceable stack slot.
pub fn protect_sexp_with_index(value: Sexp<'_>) -> IndexedProtectGuard {
    protect_with_index_raw(value.as_raw(), "protect_sexp_with_index")
}

/// Protect a raw SEXP in a replaceable stack slot.
///
/// Legacy compatibility helper for translated Rust modules. Prefer
/// [`protect_sexp_with_index`] when the caller has an owner-scoped value.
pub(crate) fn protect_with_index_raw(s: SEXP, api: &str) -> IndexedProtectGuard {
    IndexedProtectGuard {
        slot: protect_raw_with_slot(s, api),
    }
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use crate::sexp::ffi::SEXPTYPE;
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
            let guard = protect_sexp(value);
            assert_eq!(R_ProtectCount(), depth_before + 1);
            with_protected_objects(|objects| assert_eq!(objects, &[value.as_raw()]));
            drop(guard);
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
    fn test_preserve_sexp_guard() {
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let value = session.sexp(value).expect("value belongs to session");

        session.with_protected(|| {
            let guard = preserve_sexp(value);
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
            guard.reprotect_sexp(second);
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
