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
        }
    }

    pub(crate) fn wrap(&self, pointer: SEXP) -> SexpResult<Sexp<'session>> {
        if !self.availability.is_live() {
            return Err(SexpError::RootUnavailable);
        }
        if let Some(pointer) = crate::sexp::session::immutable_singleton_projection(pointer) {
            return Ok(Sexp {
                ptr: pointer,
                owner: SexpOwner::Static,
                node: None,
                session_owner_ptr: None,
                root: None,
                _marker: std::marker::PhantomData,
            });
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
            _marker: std::marker::PhantomData,
        })
    }
}
