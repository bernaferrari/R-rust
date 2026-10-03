#![forbid(unsafe_code)]
//! Allocation-domain capabilities captured before an exclusive arena lend.

use super::{Sexp, SexpError, SexpOwner, SexpResult};
use crate::sexp::{ffi::SEXP, heap::HeapIdentity, instance::InstanceLiveness, owner::OwnerToken};

/// Wrap newly parsed nodes without reading their exclusively lent arena or
/// instance. The session lifetime and counted root retain every returned node.
#[derive(Clone)]
pub(crate) struct SessionNodeFactory<'session> {
    owner: OwnerToken<'session>,
    heap: HeapIdentity,
    availability: InstanceLiveness,
    singletons: crate::sexp::globals::SingletonPoolLease,
}

impl<'session> SessionNodeFactory<'session> {
    pub(crate) fn new(owner: OwnerToken<'session>) -> Self {
        owner.node_factory()
    }

    pub(crate) fn from_snapshot(
        owner: OwnerToken<'session>,
        heap: HeapIdentity,
        availability: InstanceLiveness,
    ) -> Self {
        Self {
            owner,
            heap,
            availability,
            singletons: crate::sexp::globals::immutable_singleton_pool(),
        }
    }

    pub(crate) fn require_active(&self) -> SexpResult<()> {
        if !self.availability.is_live() {
            return Err(SexpError::RootUnavailable);
        }
        self.owner.require_active()
    }

    pub(crate) fn nil(&self) -> Sexp<'session> {
        Sexp::from_singleton(self.singletons.nil(), self.singletons.clone())
    }

    /// Install the root before the arena lend ends, so deferred collection or
    /// notifications retain the newly allocated value throughout reentry.
    pub(crate) fn allocate(
        &self,
        allocation: impl FnOnce(&mut crate::sexp::memory::RArena) -> Option<SEXP>,
    ) -> SexpResult<Sexp<'session>> {
        if !self.availability.is_live() {
            return Err(SexpError::RootUnavailable);
        }
        self.owner.with_arena(|arena| {
            let pointer =
                allocation(arena).ok_or(SexpError::AllocationFailed { object: "R value" })?;
            self.wrap(pointer)
        })?
    }

    pub(crate) fn wrap(&self, pointer: SEXP) -> SexpResult<Sexp<'session>> {
        if !self.availability.is_live() {
            return Err(SexpError::RootUnavailable);
        }
        if let Some(singleton) = self.singletons.lease(pointer) {
            return Ok(Sexp::from_singleton(singleton, self.singletons.clone()));
        }
        let (pointer, node) = crate::sexp::memory::checked_projection(pointer)
            .filter(|(_, node)| node.belongs_to(&self.heap))
            .ok_or(SexpError::UnownedPointer {
                address: pointer.addr(),
            })?;
        let root = node.root_lease().ok_or(SexpError::RootUnavailable)?;
        Ok(Sexp {
            ptr: pointer,
            owner: SexpOwner::Session(self.owner.as_ptr().addr()),
            node: Some(node),
            session_owner_ptr: std::ptr::NonNull::new(self.owner.as_ptr()),
            root: Some(root),
            singleton: None,
            singletons: Some(self.singletons.clone()),
            _marker: std::marker::PhantomData,
        })
    }
}
