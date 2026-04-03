use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem;
use core::ptr::NonNull;

use alloc::vec::Vec;

use crate::object::Object;
use crate::session::Session;
use crate::sexp::{Header, Sexp};

/// Non-moving GC pointer.
#[repr(transparent)]
#[derive(Debug)]
pub struct Gc<T: Object + ?Sized> {
    ptr: NonNull<T>,
    _marker: PhantomData<&'static T>,
}

/// Rooted GC pointer, prevents collection.
#[repr(transparent)]
#[derive(Debug)]
pub struct Root<T: Object + ?Sized> {
    ptr: NonNull<T>,
    _marker: PhantomData<&'static mut T>,
}

/// Scope for temporary root management.
pub struct Scope<'a> {
    session: &'a mut Session,
    roots: Vec<NonNull<Header>>,
}

/// Write barrier implementation.
pub struct WriteBarrier;

/// Trace trait for GC marking.
pub unsafe trait Trace {
    /// Trace all reachable child objects.
    unsafe fn trace(&mut self);
}

impl<T: Object> Gc<T> {
    #[inline(always)]
    pub(crate) unsafe fn new(ptr: NonNull<T>) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn as_sexp(&self) -> Sexp<T> {
        unsafe { Sexp::new_unchecked(self.ptr) }
    }

    #[inline(always)]
    pub fn root(self) -> Root<T> {
        unsafe { Root::new(self.ptr) }
    }
}

impl<T: Object> Root<T> {
    #[inline(always)]
    pub(crate) unsafe fn new(ptr: NonNull<T>) -> Self {
        let header = ptr.cast::<Header>();
        (*header.as_ptr()).gc_bits |= 0b00000001;
        Session::current().add_root(header);
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn as_sexp(&self) -> Sexp<T> {
        unsafe { Sexp::new_unchecked(self.ptr) }
    }

    #[inline(always)]
    pub fn unroot(self) {
        mem::drop(self);
    }
}

impl<T: Object + ?Sized> Drop for Root<T> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            let header = self.ptr.cast::<Header>();
            (*header.as_ptr()).gc_bits &= !0b00000001;
            Session::current().remove_root(header);
        }
    }
}

impl<'a> Scope<'a> {
    #[inline]
    pub fn new(session: &'a mut Session) -> Self {
        Self {
            session,
            roots: Vec::with_capacity(32),
        }
    }

    #[inline]
    pub fn push<T: Object>(&mut self, obj: Gc<T>) -> Gc<T> {
        unsafe {
            let header = obj.ptr.cast::<Header>();
            (*header.as_ptr()).gc_bits |= 0b00000010;
            self.roots.push(header);
        }
        obj
    }
}

impl<'a> Drop for Scope<'a> {
    #[inline]
    fn drop(&mut self) {
        for header in self.roots.drain(..) {
            unsafe {
                (*header.as_ptr()).gc_bits &= !0b00000010;
            }
        }
    }
}

impl WriteBarrier {
    /// Standard generational write barrier.
    #[inline(always)]
    pub fn store<T: Object>(_dest: Gc<T>, _value: Sexp) {
        #[cfg(not(feature = "no-write-barrier"))]
        unsafe {
            let dest_header = _dest.ptr.cast::<Header>();
            let value_header = _value.as_ptr().cast::<Header>();

            if (*dest_header.as_ptr()).gc_gen > (*value_header.as_ptr()).gc_gen {
                Session::current().remembered_set().push(_value);
            }
        }
    }
}

#[inline(always)]
pub fn store_field<T: Object>(dest: Gc<T>, field: &mut Option<Sexp>, value: Option<Sexp>) {
    *field = value;
    if let Some(val) = value {
        WriteBarrier::store(dest, val);
    }
}
