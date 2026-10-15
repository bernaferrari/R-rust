#![forbid(unsafe_code)]
//! Deparse owns its input and partial output across one pressure retry.

use crate::sexp::{
    ffi::SEXP,
    memory::RArena,
    object::{NodeAllocator, NodeDomain, Sexp, SexpError, SexpResult},
    owner::RuntimeAccess,
};

pub(super) struct OutputAllocator<'execution, 'session> {
    access: &'execution RuntimeAccess,
    allocator: NodeAllocator<'execution, 'session>,
}

impl<'execution, 'session> OutputAllocator<'execution, 'session> {
    pub(super) fn new(
        access: &'execution RuntimeAccess,
        domain: &NodeDomain<'session>,
    ) -> SexpResult<Self> {
        Ok(Self {
            access,
            allocator: access.allocator(domain)?,
        })
    }

    pub(super) fn character(&self, value: &str) -> SexpResult<Sexp<'session>> {
        self.allocate(|arena| Some(arena.alloc_charsxp(value.as_bytes())))
    }

    /// Callers retain the input tree and partially published output, and lend
    /// no R payload across this call. Only a genuine failed allocation gets
    /// one full collection and retry; owner and identity failures propagate.
    pub(super) fn allocate(
        &self,
        mut produce: impl FnMut(&mut RArena) -> Option<SEXP>,
    ) -> SexpResult<Sexp<'session>> {
        let mut allocate = |arena: &mut RArena| {
            let pointer = produce(arena)?;
            (!pointer.is_null()).then_some(pointer)
        };
        match self.allocator.allocate(&mut allocate) {
            Err(SexpError::AllocationFailed { .. }) => {
                self.access
                    .with_native(|owner| owner.full_gc().map(|_| ()))?;
                self.allocator.allocate(&mut allocate)
            }
            result => result,
        }
    }
}
