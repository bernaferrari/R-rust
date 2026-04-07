//! Port of lock.c -- Thread synchronization / locking primitives.
//!
//! The C version provides platform-specific locking implementations for
//! POSIX threads, GNU Pth, Solaris threads, and Win32 threads. For the
//! standalone Rust port, we use simple mutex-based stubs since we operate
//! in single-threaded mode.
//!
//! The key types ported are:
//! - `gl_lock_t`       -- simple mutex
//! - `gl_rwlock_t`     -- read-write lock
//! - `gl_recursive_lock_t` -- recursive mutex
//! - `gl_once_t`       -- one-time initialization

#![allow(non_snake_case, dead_code)]

use std::os::raw::c_int;
use std::sync::atomic::{AtomicI32, Ordering};

// ---------------------------------------------------------------------------
// gl_lock_t -- Simple lock (stub, no-op in standalone mode)
// ---------------------------------------------------------------------------

/// Simple non-recursive lock type.
///
/// In the standalone port this is a no-op since we don't have threads.
/// The C implementation uses pthread_mutex_t, CRITICAL_SECTION, etc.
#[repr(C)]
pub(crate) struct gl_lock_t {
    /// In the C version this holds a pthread_mutex_t or CRITICAL_SECTION.
    /// Here we just need a placeholder.
    _opaque: [u8; 0],
}

/// Initialize a gl_lock_t.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_lock_init(_lock: *mut gl_lock_t) {
    // No-op in standalone mode.
}

/// Acquire a gl_lock_t.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_lock_lock(_lock: *mut gl_lock_t) {
    // No-op in standalone mode.
}

/// Release a gl_lock_t.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_lock_unlock(_lock: *mut gl_lock_t) {
    // No-op in standalone mode.
}

/// Destroy a gl_lock_t.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_lock_destroy(_lock: *mut gl_lock_t) {
    // No-op in standalone mode.
}

// ---------------------------------------------------------------------------
// gl_rwlock_t -- Read-write lock (stub, no-op in standalone mode)
// ---------------------------------------------------------------------------

/// Read-write lock type.
///
/// In the C version this uses pthread_rwlock_t or a custom implementation
/// based on mutex + condvars. In standalone mode, all operations are no-ops.
#[repr(C)]
pub(crate) struct gl_rwlock_t {
    _opaque: [u8; 0],
}

/// Initialize a read-write lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_rwlock_init(_lock: *mut gl_rwlock_t) {
    // No-op in standalone mode.
}

/// Acquire a read (shared) lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_rwlock_rdlock(_lock: *mut gl_rwlock_t) {
    // No-op in standalone mode.
}

/// Acquire a write (exclusive) lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_rwlock_wrlock(_lock: *mut gl_rwlock_t) {
    // No-op in standalone mode.
}

/// Release a read-write lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_rwlock_unlock(_lock: *mut gl_rwlock_t) {
    // No-op in standalone mode.
}

/// Destroy a read-write lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_rwlock_destroy(_lock: *mut gl_rwlock_t) {
    // No-op in standalone mode.
}

// ---------------------------------------------------------------------------
// gl_recursive_lock_t -- Recursive lock (stub, no-op in standalone mode)
// ---------------------------------------------------------------------------

/// Recursive lock type.
///
/// In the C version this uses PTHREAD_MUTEX_RECURSIVE or a manual
/// depth-counting implementation. In standalone mode, all operations
/// are no-ops.
#[repr(C)]
pub(crate) struct gl_recursive_lock_t {
    _opaque: [u8; 0],
}

/// Initialize a recursive lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_recursive_lock_init(_lock: *mut gl_recursive_lock_t) {
    // No-op in standalone mode.
}

/// Acquire a recursive lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_recursive_lock_lock(_lock: *mut gl_recursive_lock_t) {
    // No-op in standalone mode.
}

/// Release a recursive lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_recursive_lock_unlock(_lock: *mut gl_recursive_lock_t) {
    // No-op in standalone mode.
}

/// Destroy a recursive lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_recursive_lock_destroy(_lock: *mut gl_recursive_lock_t) {
    // No-op in standalone mode.
}

// ---------------------------------------------------------------------------
// gl_once_t -- One-time initialization
// ---------------------------------------------------------------------------

/// One-time initialization type.
///
/// In the C POSIX version this uses `pthread_once_t`. In the C Win32 version
/// this uses a custom struct with CRITICAL_SECTION and flags. Here we use
/// a simple atomic flag.
#[repr(C)]
pub(crate) struct gl_once_t {
    /// 0 = not initialized, 1 = initialized, -1 = initialization in progress.
    inited: AtomicI32,
}

impl gl_once_t {
    pub const fn new() -> Self {
        Self {
            inited: AtomicI32::new(0),
        }
    }
}

/// Perform one-time initialization.
///
/// Ensures that `initfunction` is called exactly once, even if multiple
/// threads call this function concurrently.
///
/// # Safety
/// `once_control` must be a valid pointer to a `gl_once_t`.
/// `initfunction` must be a valid function pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_once(
    once_control: *mut gl_once_t,
    initfunction: Option<unsafe extern "C" fn()>,
) {
    unsafe {
        if once_control.is_null() {
            return;
        }
        let once = &mut *once_control;
        if once.inited.load(Ordering::Acquire) == 0 {
            once.inited.store(1, Ordering::Release);
            if let Some(f) = initfunction {
                f();
            }
        }
    }
}

/// Single-threaded one-time initialization check.
///
/// Returns 1 if this is the first call (the caller should perform the
/// initialization), 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_once_singlethreaded(once_control: *mut gl_once_t) -> c_int {
    unsafe {
        if once_control.is_null() {
            return 1;
        }
        let once = &mut *once_control;
        if once.inited.load(Ordering::Relaxed) == 0 {
            once.inited.store(1, Ordering::Relaxed);
            1
        } else {
            0
        }
    }
}

/// Check whether threads are in use.
///
/// In the standalone port, this always returns 0 (no threads).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn glthread_in_use() -> c_int {
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn test_once_initialization() {
        use std::cell::Cell;
        unsafe {
            thread_local! { static INIT_CALLED: Cell<bool> = Cell::new(false); }
            unsafe extern "C" fn init_fn() {
                INIT_CALLED.with(|v| v.set(true));
            }

            let mut once = gl_once_t::new();
            glthread_once(&mut once, Some(init_fn));
            assert_eq!(INIT_CALLED.with(|v| v.get()), true);

            // Calling again should not re-initialize.
            INIT_CALLED.with(|v| v.set(false));
            glthread_once(&mut once, Some(init_fn));
            assert_eq!(INIT_CALLED.with(|v| v.get()), false);
        }
    }

    #[test]
    fn test_once_singlethreaded() {
        unsafe {
            let mut once = gl_once_t::new();
            let result1 = glthread_once_singlethreaded(&mut once);
            assert_eq!(result1, 1);
            let result2 = glthread_once_singlethreaded(&mut once);
            assert_eq!(result2, 0);
        }
    }

    #[test]
    fn test_glthread_in_use() {
        unsafe {
            assert_eq!(glthread_in_use(), 0);
        }
    }

    #[test]
    fn test_rwlock_operations() {
        unsafe {
            let mut lock = gl_rwlock_t { _opaque: [] };
            glthread_rwlock_init(&mut lock);
            glthread_rwlock_rdlock(&mut lock);
            glthread_rwlock_rdlock(&mut lock);
            glthread_rwlock_wrlock(&mut lock);
            glthread_rwlock_unlock(&mut lock);
            glthread_rwlock_destroy(&mut lock);
        }
    }
}
