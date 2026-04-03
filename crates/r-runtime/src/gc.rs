use crate::object::Object;
use crate::sexp::Header;
use core::ptr::NonNull;

/// Placeholder GC pointer - full implementation pending
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Gc<T: Object> {
    ptr: NonNull<T>,
}

/// Placeholder root pointer - full implementation pending  
#[repr(transparent)]
#[derive(Debug)]
pub struct Root<T: Object> {
    ptr: NonNull<T>,
}

/// Placeholder scope - full implementation pending
pub struct Scope<'a> {
    _marker: core::marker::PhantomData<&'a ()>,
}

/// Placeholder write barrier - full implementation pending
pub struct WriteBarrier;

/// Trace trait for GC marking - stub implementation
pub unsafe trait Trace {
    unsafe fn trace(&mut self);
}

impl<T: Object> Gc<T> {
    #[inline(always)]
    pub(crate) unsafe fn new(ptr: NonNull<T>) -> Self {
        Self { ptr }
    }

    #[inline(always)]
    pub fn as_ptr(&self) -> *const T {
        self.ptr.as_ptr()
    }
}

impl<T: Object> Root<T> {
    #[inline(always)]
    pub(crate) unsafe fn new(ptr: NonNull<T>) -> Self {
        Self { ptr }
    }
}

impl<'a> Scope<'a> {
    #[inline]
    pub fn new() -> Self {
        Self {
            _marker: core::marker::PhantomData,
        }
    }
}

impl Default for Scope<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteBarrier {
    /// Stub write barrier - full implementation pending
    #[inline(always)]
    pub fn store<T: Object>(_dest: Gc<T>, _value: crate::sexp::Sexp) {}
}

/// Stub field storage - full implementation pending
#[inline(always)]
pub fn store_field<T: Object>(
    _dest: Gc<T>,
    field: &mut Option<crate::sexp::Sexp>,
    value: Option<crate::sexp::Sexp>,
) {
    *field = value;
}
