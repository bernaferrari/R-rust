use core::marker::PhantomData;
use core::ptr::NonNull;

use crate::gc::Gc;
use crate::object::Object;

/// R's exact SEXPTYPE tag values.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SEXPTYPE {
    NILSXP = 0,
    SYMSXP = 1,
    LISTSXP = 2,
    CLOSXP = 3,
    ENVSXP = 4,
    PROMSXP = 5,
    LANGSXP = 6,
    SPECIALSXP = 7,
    BUILTINSXP = 8,
    CHARSXP = 9,
    LGLSXP = 10,
    INTSXP = 13,
    REALSXP = 14,
    CPLXSXP = 15,
    STRSXP = 16,
    DOTSXP = 17,
    ANYSXP = 18,
    VECSXP = 19,
    EXPRSXP = 20,
    BCODESXP = 21,
    EXTPTRSXP = 22,
    WEAKREFSXP = 23,
    RAWSXP = 24,
    S4SXP = 25,
    NEWSXP = 30,
    FREESXP = 31,
}

/// Type tag trait for static type checking.
pub trait TypeTagged: Object {
    const TAG: SEXPTYPE;
}

/// Tagged SEXP pointer.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Sexp<T: Object = dyn Object> {
    ptr: NonNull<T>,
    _marker: PhantomData<fn() -> T>,
}

impl<T: Object> Sexp<T> {
    /// Create a new Sexp pointer from a valid GC-managed object.
    #[inline(always)]
    pub(crate) unsafe fn new_unchecked(ptr: NonNull<T>) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    /// Get the raw pointer.
    #[inline(always)]
    pub fn as_ptr(self) -> *const T {
        self.ptr.as_ptr()
    }

    /// Get mutable raw pointer.
    #[inline(always)]
    pub fn as_mut_ptr(self) -> *mut T {
        self.ptr.as_ptr()
    }

    /// Cast to a known object type.
    #[inline]
    pub fn cast<U: Object + TypeTagged>(self) -> Sexp<U> {
        debug_assert_eq!(unsafe { (*self.as_ptr()).tag() }, U::TAG);
        unsafe { Sexp::new_unchecked(self.ptr.cast()) }
    }

    /// Get the object header.
    #[inline(always)]
    pub fn header(self) -> &'static Header {
        unsafe { &*(self.as_ptr() as *const Header) }
    }
}

/// Common object header present on all GC objects.
#[repr(C)]
#[derive(Debug)]
pub struct Header {
    pub(crate) tag: SEXPTYPE,
    pub(crate) gc_bits: u8,
    pub(crate) gc_gen: u8,
    pub(crate) flags: u8,
    pub(crate) attributes: Option<Sexp>,
    pub(crate) prev: Option<Sexp>,
    pub(crate) next: Option<Sexp>,
}

impl Header {
    #[inline(always)]
    pub const fn new(tag: SEXPTYPE) -> Self {
        Self {
            tag,
            gc_bits: 0,
            gc_gen: 0,
            flags: 0,
            attributes: None,
            prev: None,
            next: None,
        }
    }

    #[inline(always)]
    pub fn tag(&self) -> SEXPTYPE {
        self.tag
    }
}

unsafe impl<T: Object> Send for Sexp<T> {}
unsafe impl<T: Object> Sync for Sexp<T> {}
