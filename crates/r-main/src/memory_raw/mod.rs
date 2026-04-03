//! Raw memory operations. ALL unsafe code from application modules must be moved here.
//!
//! This module is the only allowed location for raw pointer operations, manual memory
//! management, FFI calls, and other unsafe operations. All other modules must use
//! the safe typed interfaces exported from this module.

use core::marker::PhantomData;
use core::ptr::NonNull;

use r_runtime::gc::{Gc, Root};
use r_runtime::session::Session;
use r_runtime::sexp::{Header, Object, Sexp, TypeTagged, SEXPTYPE};

/// Raw vector access with write barrier enforcement
#[repr(transparent)]
pub struct RawVector<T> {
    ptr: NonNull<[T]>,
    header: NonNull<Header>,
}

impl<T> RawVector<T> {
    /// Create raw vector access from valid GC pointer
    ///
    /// # Safety
    /// Pointer must point to a live GC-allocated vector of the correct type
    #[inline]
    pub unsafe fn new_unchecked(ptr: NonNull<[T]>, header: NonNull<Header>) -> Self {
        Self { ptr, header }
    }

    /// Get immutable slice
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        unsafe { self.ptr.as_ref() }
    }

    /// Get mutable slice with write barrier
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe {
            (*self.header.as_ptr()).gc_bits |= 0b00001000;
            Session::current()
                .remembered_set()
                .push(Sexp::new_unchecked(self.header.cast()));
            self.ptr.as_mut()
        }
    }

    /// Get length
    #[inline]
    pub fn len(&self) -> usize {
        unsafe { self.ptr.as_ref().len() }
    }
}

/// Raw null pointer validation
#[inline]
pub fn null_check<T>(ptr: *const T) -> Option<NonNull<T>> {
    NonNull::new(ptr as *mut T)
}

/// Raw setjmp/longjmp wrapper returning Result
#[inline]
pub unsafe fn with_jmpbuf<F, R, E>(f: F) -> Result<R, E>
where
    F: FnOnce() -> R,
    E: Default,
{
    // Internal implementation will go here
    unimplemented!()
}
