//! Lifetime-bound access to a live interpreter owner.
//!
//! This token borrows the session lifetime, not the whole RInstance. Field
//! access can therefore obey P1/P2 without holding a Rust instance reference
//! across callbacks. It never lends R payload references.

use std::{marker::PhantomData, ptr::NonNull, rc::Rc};

use super::{
    ffi::SEXP,
    instance::RInstance,
    object::{Sexp, SexpError, SexpResult},
    session::RSession,
};

#[derive(Clone, Copy)]
pub(crate) struct OwnerToken<'session> {
    pointer: NonNull<RInstance>,
    _lifetime: PhantomData<(&'session RSession, Rc<()>)>,
}

impl<'session> OwnerToken<'session> {
    /// Bridge an already-proven raw owner into the checked lifetime layer.
    ///
    /// # Safety
    /// The original writable owner allocation outlives `'session`. No whole
    /// instance or arena borrow may overlap token operations. Rust payload
    /// loans must exclude mutation and R execution through this token.
    pub(crate) unsafe fn from_raw(pointer: *mut RInstance) -> Self {
        Self {
            pointer: NonNull::new(pointer).expect("live owner pointer"),
            _lifetime: PhantomData,
        }
    }

    pub(crate) fn as_ptr(self) -> *mut RInstance {
        self.pointer.as_ptr()
    }

    /// Capture immutable allocation-domain and availability capabilities before
    /// lending the arena. Later node wrapping needs no instance field access.
    pub(crate) fn node_factory(self) -> super::object::SessionNodeFactory<'session> {
        // SAFETY: the token's lifetime retains the physical owner. This short
        // field access ends before the factory can be used inside an arena lend.
        let (heap, availability) = unsafe {
            (
                (*self.as_ptr()).heap_identity.clone(),
                super::instance::instance_liveness(self.as_ptr()),
            )
        };
        super::object::SessionNodeFactory::from_snapshot(self, heap, availability)
    }

    /// Validate and root a pointer in this owner, even if another is active.
    pub(crate) fn sexp(self, pointer: SEXP) -> SexpResult<Sexp<'session>> {
        Sexp::from_owner_raw(pointer, self)
    }

    /// Collection callbacks use ambient R dispatch, so require this owner active.
    fn require_active(self) -> SexpResult<()> {
        if super::instance::current_instance_ptr() == Some(self.as_ptr()) {
            Ok(())
        } else {
            Err(SexpError::OwnerNotActive)
        }
    }

    pub(crate) fn minor_gc(self) -> SexpResult<(usize, usize)> {
        self.require_active()?;
        // SAFETY: the token retains its live owner and excludes Rust payload loans.
        Ok(unsafe { super::gengc::minor_gc_in(self.as_ptr()) })
    }

    pub(crate) fn full_gc(self) -> SexpResult<(usize, usize)> {
        self.require_active()?;
        // SAFETY: same owner/loan contract as minor_gc; arena lends defer collection.
        Ok(unsafe { super::gengc::full_gc_in(self.as_ptr()) })
    }
}
