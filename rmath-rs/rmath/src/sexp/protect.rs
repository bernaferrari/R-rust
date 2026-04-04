#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R's PROTECT/UNPROTECT mechanism.
//!
//! In C, R maintains a protection stack to prevent GC from collecting objects
//! that are only referenced by C local variables. In this port we maintain the
//! same stack, even though we don't have GC yet — this ensures correctness
//! and compatibility when GC is eventually implemented.

use std::cell::RefCell;
use std::os::raw::c_int;
use std::ptr;

use super::ffi::SEXP;

/// RAII guard for the protection stack.
/// Automatically unprotects when dropped.
///
/// ```
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
        unsafe {
            Rf_unprotect(self.count as c_int);
        }
    }
}

/// Protect a SEXP and return an RAII guard.
///
/// This is the idiomatic Rust way to protect SEXP values.
/// The guard automatically unprotects when it goes out of scope.
pub fn protect(s: SEXP) -> ProtectGuard {
    unsafe {
        Rf_protect(s);
    }
    ProtectGuard { count: 1 }
}

/// Protect n SEXP values and return an RAII guard.
///
/// Use this when you need to protect multiple values at once.
pub fn protect_n(n: usize) -> ProtectGuard {
    ProtectGuard { count: n }
}

// ---------------------------------------------------------------------------
// Thread-local protection stack
// ---------------------------------------------------------------------------

thread_local! {
    /// The thread-local protection stack.
    static PROTECT_STACK: RefCell<ProtectStack> = RefCell::new(ProtectStack::new());
}

/// The protection stack state.
struct ProtectStack {
    /// Stack of protected SEXP pointers.
    stack: Vec<SEXP>,
}

impl ProtectStack {
    fn new() -> Self {
        ProtectStack { stack: Vec::new() }
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
pub unsafe extern "C" fn Rf_protect(s: SEXP) -> SEXP {
    if !s.is_null() {
        PROTECT_STACK.with(|ps| {
            ps.borrow_mut().stack.push(s);
        });
    }
    s
}

/// Unprotect the top n entries from the protection stack.
///
/// This is the equivalent of R's `UNPROTECT(n)` macro.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_unprotect(n: c_int) {
    if n <= 0 {
        return;
    }
    PROTECT_STACK.with(|ps| {
        let mut stack = ps.borrow_mut();
        let len = stack.stack.len();
        if (n as usize) > len {
            panic!(
                "Rf_unprotect: trying to unprotect {} items but only {} on stack",
                n, len
            );
        }
        let new_len = len - (n as usize);
        stack.stack.truncate(new_len);
    });
}

/// Unprotect the top entry from the protection stack.
///
/// This is the equivalent of R's `UNPROTECT_PTR()` macro.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_unprotect_ptr(s: SEXP) {
    PROTECT_STACK.with(|ps| {
        let mut stack = ps.borrow_mut();
        // Remove the last occurrence of s from the stack
        if let Some(pos) = stack.stack.iter().rposition(|&x| x == s) {
            stack.stack.remove(pos);
        }
    });
}

/// Get the current number of entries on the protection stack.
///
/// Used by the context system to track protect depth.
pub fn R_ProtectCount() -> usize {
    PROTECT_STACK.with(|ps| ps.borrow().stack.len())
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ProtectWithIndex(s: SEXP) -> *mut ProtectIndex {
    let index = PROTECT_STACK.with(|ps| {
        let mut stack = ps.borrow_mut();
        stack.stack.push(s);
        stack.stack.len() - 1
    });
    // Return index as an opaque pointer - no allocation, no leak
    index as *mut ProtectIndex
}

/// Free a ProtectIndex returned by R_ProtectWithIndex.
///
/// This is a no-op - the index was just a number, not an allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FreeProtectIndex(_pi: *mut ProtectIndex) {
    // No-op - the index was just a number, not an allocation
}

/// Unprotect the entry at the given index and replace it with a new value.
///
/// This is the equivalent of R's `R_Reprotect()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Reprotect(s: SEXP, index: *mut ProtectIndex) {
    if index.is_null() {
        return;
    }
    let idx = index as usize;
    PROTECT_STACK.with(|ps| {
        let mut stack = ps.borrow_mut();
        if idx < stack.stack.len() {
            stack.stack[idx] = s;
        }
    });
}

// ---------------------------------------------------------------------------
// R_PreserveObject / R_ReleaseObject — permanent protection
// ---------------------------------------------------------------------------

thread_local! {
    /// Stack of permanently preserved objects.
    static PRESERVE_STACK: RefCell<Vec<SEXP>> = RefCell::new(Vec::new());
}

/// Permanently protect an SEXP from garbage collection.
///
/// Unlike Rf_protect, this protection persists until explicitly released.
/// This is the equivalent of R's `R_PreserveObject()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_PreserveObject(s: SEXP) {
    if !s.is_null() {
        PRESERVE_STACK.with(|ps| {
            ps.borrow_mut().push(s);
        });
    }
}

/// Release a previously preserved object.
///
/// This is the equivalent of R's `R_ReleaseObject()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ReleaseObject(s: SEXP) {
    PRESERVE_STACK.with(|ps| {
        let mut stack = ps.borrow_mut();
        if let Some(pos) = stack.iter().position(|&x| x == s) {
            stack.remove(pos);
        }
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to reset protection stack between tests
    fn reset_protect_stack() {
        PROTECT_STACK.with(|ps| {
            ps.borrow_mut().stack.clear();
        });
        PRESERVE_STACK.with(|ps| {
            ps.borrow_mut().clear();
        });
    }

    #[test]
    fn test_protect_unprotect() {
        reset_protect_stack();
        unsafe {
            let fake = 0x1 as SEXP;
            let result = Rf_protect(fake);
            assert_eq!(result, fake);
            assert_eq!(R_ProtectCount(), 1);
            Rf_unprotect(1);
            assert_eq!(R_ProtectCount(), 0);
        }
    }

    #[test]
    fn test_protect_null() {
        reset_protect_stack();
        unsafe {
            Rf_protect(ptr::null_mut());
            assert_eq!(R_ProtectCount(), 0);
        }
    }

    #[test]
    fn test_protect_multiple() {
        reset_protect_stack();
        unsafe {
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
        }
    }

    #[test]
    fn test_unprotect_ptr() {
        reset_protect_stack();
        unsafe {
            let a = 0x1 as SEXP;
            let b = 0x2 as SEXP;
            Rf_protect(a);
            Rf_protect(b);
            assert_eq!(R_ProtectCount(), 2);
            Rf_unprotect_ptr(a);
            assert_eq!(R_ProtectCount(), 1);
            Rf_unprotect_ptr(b);
            assert_eq!(R_ProtectCount(), 0);
        }
    }

    #[test]
    fn test_unprotect_negative() {
        reset_protect_stack();
        unsafe {
            Rf_unprotect(-1); // Should be a no-op
            assert_eq!(R_ProtectCount(), 0);
        }
    }

    #[test]
    fn test_preserve_release() {
        reset_protect_stack();
        unsafe {
            let fake = 0x1 as SEXP;
            R_PreserveObject(fake);
            PRESERVE_STACK.with(|ps| {
                assert_eq!(ps.borrow().len(), 1);
            });
            R_ReleaseObject(fake);
            PRESERVE_STACK.with(|ps| {
                assert_eq!(ps.borrow().len(), 0);
            });
        }
    }

    #[test]
    fn test_protect_guard() {
        reset_protect_stack();
        let depth_before = R_ProtectCount();
        {
            let a = 0x1 as SEXP;
            let _guard = protect(a);
            assert_eq!(R_ProtectCount(), depth_before + 1);
        }
        // Guard dropped, should be back to original depth
        assert_eq!(R_ProtectCount(), depth_before);
    }

    #[test]
    fn test_protect_n_guard() {
        reset_protect_stack();
        let depth_before = R_ProtectCount();
        {
            unsafe {
                Rf_protect(0x1 as SEXP);
                Rf_protect(0x2 as SEXP);
                Rf_protect(0x3 as SEXP);
            }
            let _guard = protect_n(3);
            assert_eq!(R_ProtectCount(), depth_before + 3);
        }
        assert_eq!(R_ProtectCount(), depth_before);
    }

    #[test]
    #[should_panic(expected = "trying to unprotect")]
    fn test_over_unprotect_panics() {
        reset_protect_stack();
        unsafe {
            Rf_protect(0x1 as SEXP);
            Rf_unprotect(5); // Should panic
        }
    }
}
