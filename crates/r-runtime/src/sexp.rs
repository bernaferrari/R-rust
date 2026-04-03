use core::ptr::NonNull;

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

pub type Tag = SEXPTYPE;

/// Type tag trait for static type checking.
pub trait TypeTagged {
    const TAG: SEXPTYPE;
}

/// Tagged SEXP pointer - stub implementation.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Sexp<T: ?Sized = ()> {
    ptr: NonNull<T>,
}

impl<T: ?Sized> core::fmt::Debug for Sexp<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Sexp").field("ptr", &self.ptr).finish()
    }
}

impl Sexp {
    /// Create a new Sexp pointer - stub
    #[inline(always)]
    pub unsafe fn new_unchecked(ptr: NonNull<()>) -> Self {
        Self { ptr }
    }
}

/// Common object header - stub implementation
#[repr(C)]
#[derive(Debug)]
pub struct Header {
    pub tag: SEXPTYPE,
    pub gc_bits: u8,
}

impl Header {
    #[inline(always)]
    pub const fn new(tag: SEXPTYPE) -> Self {
        Self { tag, gc_bits: 0 }
    }

    #[inline(always)]
    pub fn tag(&self) -> SEXPTYPE {
        self.tag
    }
}

unsafe impl Send for Sexp {}
unsafe impl Sync for Sexp {}
