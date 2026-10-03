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

    /// Capture the explicitly active owner at a translated raw boundary.
    ///
    /// # Safety
    /// The caller retains this writable owner for `'session` and excludes
    /// overlapping instance, arena and payload loans across token operations.
    pub(crate) unsafe fn current() -> SexpResult<Self> {
        let pointer = super::instance::current_instance_ptr().ok_or(SexpError::OwnerNotActive)?;
        // SAFETY: the raw entry boundary supplies the original owner lifetime.
        Ok(unsafe { Self::from_raw(pointer) })
    }

    /// Lend the active owner's arena; callbacks and collection finish after
    /// this exclusive lend ends. The result cannot borrow from the arena.
    pub(crate) fn with_arena<T>(
        self,
        f: impl FnOnce(&mut super::memory::RArena) -> T,
    ) -> SexpResult<T> {
        self.require_active()?;
        if super::memory::is_arena_lent(self.as_ptr()) {
            return Err(SexpError::OwnerNotActive);
        }
        // SAFETY: the lifetime-bound token retains the owner, and the checked
        // lend state excludes an overlapping arena loan. Field-local lending
        // ends before deferred callbacks and GC run.
        Ok(unsafe { super::memory::with_arena_in(self.as_ptr(), f) })
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
    pub(crate) fn require_active(self) -> SexpResult<()> {
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
