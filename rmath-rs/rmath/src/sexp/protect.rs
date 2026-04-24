#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R's PROTECT/UNPROTECT mechanism.
//!
//! In C, R maintains a protection stack to prevent GC from collecting objects
//! that are only referenced by C local variables. In this port we maintain the
//! same stack, even though we don't have GC yet — this ensures correctness
//! and compatibility when GC is eventually implemented.

use std::os::raw::c_int;

use super::ffi::SEXP;

/// RAII guard for the protection stack.
/// Automatically unprotects when dropped.
///
/// ```rust,ignore
/// use rmath::sexp::protect::{protect, Rf_protect, Rf_unprotect};
///
/// let guard = protect(some_sexp);
/// // ... do work ...
/// // guard automatically unprotects when it goes out of scope
/// ```
pub struct ProtectGuard {
    count: usize,
}

impl Drop for ProtectGuard {
    fn drop(&mut self) {
        if self.count > 0 {
            unsafe {
                Rf_unprotect(self.count as c_int);
            }
        }
    }
}

/// Protect a SEXP and return an RAII guard.
///
/// This is the idiomatic Rust way to protect SEXP values.
/// The guard automatically unprotects when it goes out of scope.
pub fn protect(s: SEXP) -> ProtectGuard {
    if !s.is_null() {
        unsafe {
            Rf_protect(s);
        }
    }
    ProtectGuard {
        count: if s.is_null() { 0 } else { 1 },
    }
}

/// Create a guard that will unprotect `n` stack entries on drop.
///
/// This does not call `Rf_protect`; callers must already have pushed `n`
/// entries and want RAII-style unwinding safety around a manual protect batch.
pub fn protect_n(n: usize) -> ProtectGuard {
    ProtectGuard { count: n }
}

fn reserve_slot_or_fail(stack: &mut Vec<SEXP>, api: &str) {
    if stack.try_reserve(1).is_err() {
        panic!("{api}: protection stack allocation failed");
    }
}

// ---------------------------------------------------------------------------
// Core protect/unprotect functions
// ---------------------------------------------------------------------------

/// Protect an SEXP from garbage collection.
///
/// Pushes the pointer onto the protection stack. Returns the same pointer.
/// This is the equivalent of R's `PROTECT()` macro.
#[unsafe(no_mangle)]
pub unsafe fn Rf_protect(s: SEXP) -> SEXP {
    if !s.is_null() {
        super::instance::with_required_current_instance(|inst| {
            reserve_slot_or_fail(&mut inst.protect_stack, "Rf_protect");
            inst.protect_stack.push(s);
        });
    }
    s
}

/// Unprotect the top n entries from the protection stack.
///
/// This is the equivalent of R's `UNPROTECT(n)` macro.
///
/// # Safety
///
/// This function will not panic. If n exceeds the stack depth,
/// it unprotects all entries and returns gracefully.
#[unsafe(no_mangle)]
pub unsafe fn Rf_unprotect(n: c_int) {
    if n <= 0 {
        return;
    }
    let to_remove = n as usize;
    super::instance::with_required_current_instance(|inst| {
        let len = inst.protect_stack.len();
        if to_remove >= len {
            inst.protect_stack.clear();
        } else {
            inst.protect_stack.truncate(len - to_remove);
        }
    });
}

/// Unprotect the top entry from the protection stack.
///
/// This is the equivalent of R's `UNPROTECT_PTR()` macro.
pub unsafe fn Rf_unprotect_ptr(s: SEXP) {
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
pub fn R_ProtectCount() -> usize {
    super::instance::with_required_current_instance(|inst| inst.protect_stack.len())
}

/// Iterate over all protected SEXP values on the stack.
/// Used by the GC to mark protected objects.
pub fn with_protected_objects<F, R>(f: F) -> R
where
    F: FnOnce(&[SEXP]) -> R,
{
    super::instance::with_required_current_instance(|inst| f(&inst.protect_stack))
}

/// Update all protect stack references using the given mapping function.
/// Used by the GC compaction phase to update moved object pointers.
pub fn update_protect_stack_refs<F>(mut update_fn: F)
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
pub fn update_preserve_stack_refs<F>(mut update_fn: F)
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
pub fn with_preserved_objects<F, R>(f: F) -> R
where
    F: FnOnce(&[SEXP]) -> R,
{
    super::instance::with_required_current_instance(|inst| f(&inst.preserve_stack))
}

// ---------------------------------------------------------------------------
// R_ProtectWithIndex — protect with an index for later unprotection
// ---------------------------------------------------------------------------

/// Result of R_ProtectWithIndex — holds the index for unprotection.
pub struct ProtectIndex {
    index: usize,
}

/// Protect an SEXP and return an index that can be used to unprotect it later.
///
/// This is the equivalent of R's `R_ProtectWithIndex()`.
pub unsafe fn R_ProtectWithIndex(s: SEXP) -> *mut ProtectIndex {
    let index = super::instance::with_required_current_instance(|inst| {
        if !s.is_null() {
            reserve_slot_or_fail(&mut inst.protect_stack, "R_ProtectWithIndex");
            inst.protect_stack.push(s);
            (inst.protect_stack.len() - 1) + 1
        } else {
            0
        }
    });
    index as *mut ProtectIndex
}

/// Free a ProtectIndex returned by R_ProtectWithIndex.
///
/// This is a no-op - the index was just a number, not an allocation.
pub unsafe fn R_FreeProtectIndex(_pi: *mut ProtectIndex) {}

/// Unprotect the entry at the given index and replace it with a new value.
///
/// This is the equivalent of R's `R_Reprotect()`.
pub unsafe fn R_Reprotect(s: SEXP, index: *mut ProtectIndex) {
    if index.is_null() {
        return;
    }
    let idx = (index as usize).wrapping_sub(1);
    super::instance::with_required_current_instance(|inst| {
        if idx < inst.protect_stack.len() {
            inst.protect_stack[idx] = s;
        }
    });
}

// ---------------------------------------------------------------------------
// R_PreserveObject / R_ReleaseObject — permanent protection
// ---------------------------------------------------------------------------

/// Permanently protect an SEXP from garbage collection.
///
/// Unlike Rf_protect, this protection persists until explicitly released.
/// This is the equivalent of R's `R_PreserveObject()`.
pub unsafe fn R_PreserveObject(s: SEXP) {
    if !s.is_null() {
        super::instance::with_required_current_instance(|inst| {
            reserve_slot_or_fail(&mut inst.preserve_stack, "R_PreserveObject");
            inst.preserve_stack.push(s);
        });
    }
}

/// Release a previously preserved object.
///
/// This is the equivalent of R's `R_ReleaseObject()`.
pub unsafe fn R_ReleaseObject(s: SEXP) {
    if s.is_null() {
        return;
    }
    super::instance::with_required_current_instance(|inst| {
        if let Some(pos) = inst.preserve_stack.iter().position(|&x| x == s) {
            inst.preserve_stack.remove(pos);
        }
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn test_protect_unprotect() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let fake = 0x1 as SEXP;
            let result = Rf_protect(fake);
            assert_eq!(result, fake);
            assert_eq!(R_ProtectCount(), 1);
            Rf_unprotect(1);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_protect_null() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            Rf_protect(ptr::null_mut());
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
            Rf_protect(a);
            Rf_protect(b);
            Rf_protect(c);
            assert_eq!(R_ProtectCount(), 3);
            Rf_unprotect(2);
            assert_eq!(R_ProtectCount(), 1);
            Rf_unprotect(1);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_ptr() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let a = 0x1 as SEXP;
            let b = 0x2 as SEXP;
            Rf_protect(a);
            Rf_protect(b);
            assert_eq!(R_ProtectCount(), 2);
            Rf_unprotect_ptr(a);
            assert_eq!(R_ProtectCount(), 1);
            Rf_unprotect_ptr(b);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_ptr_null() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            Rf_unprotect_ptr(ptr::null_mut());
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_negative() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            Rf_unprotect(-1);
            assert_eq!(R_ProtectCount(), 0);
        });
    }

    #[test]
    fn test_unprotect_exceeds_stack() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            Rf_protect(0x1 as SEXP);
            Rf_unprotect(5);
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
    fn test_protect_guard() {
        let session = RSession::new();
        session.with_protected(|| {
            let depth_before = R_ProtectCount();
            let a = 0x1 as SEXP;
            let _guard = protect(a);
            assert_eq!(R_ProtectCount(), depth_before + 1);
            drop(_guard);
            assert_eq!(R_ProtectCount(), depth_before);
        });
    }

    #[test]
    fn test_protect_guard_null() {
        let session = RSession::new();
        session.with_protected(|| {
            let depth_before = R_ProtectCount();
            let _guard = protect(ptr::null_mut());
            assert_eq!(R_ProtectCount(), depth_before);
            drop(_guard);
            assert_eq!(R_ProtectCount(), depth_before);
        });
    }

    #[test]
    fn test_protect_n_guard() {
        let session = RSession::new();
        session.with_protected(|| {
            let depth_before = R_ProtectCount();
            unsafe {
                Rf_protect(0x1 as SEXP);
                Rf_protect(0x2 as SEXP);
                Rf_protect(0x3 as SEXP);
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
            Rf_unprotect(1);
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
            Rf_unprotect(1);
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
            Rf_protect(0x1 as SEXP);
            Rf_protect(0x2 as SEXP);
            with_protected_objects(|objects| {
                assert_eq!(objects.len(), 2);
            });
            Rf_unprotect(2);
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
            Rf_protect(0x1 as SEXP);
            Rf_protect(0x2 as SEXP);
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
            Rf_unprotect(2);
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
