#![forbid(unsafe_code)]
//! Allocation-domain capabilities captured before an exclusive arena lend.

use super::{Sexp, SexpError, SexpOwner, SexpResult};
use crate::sexp::{ffi::SEXP, heap::HeapIdentity, instance::InstanceLiveness, owner::OwnerToken};

/// Wrap newly parsed nodes without reading their exclusively lent arena or
/// instance. The session lifetime and counted root retain every returned node.
#[derive(Clone)]
pub(crate) struct SessionNodeFactory<'session> {
    owner: OwnerToken<'session>,
    runtime_owner: Option<crate::sexp::owner::WeakOwner>,
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
            runtime_owner: owner.weak_owner(),
            heap,
            availability,
            singletons: crate::sexp::globals::immutable_singleton_pool(),
        }
    }

    pub(crate) fn from_capability(
        owner: OwnerToken<'session>,
        heap: HeapIdentity,
        availability: InstanceLiveness,
        singletons: crate::sexp::globals::SingletonPoolLease,
        runtime_owner: crate::sexp::owner::WeakOwner,
    ) -> Self {
        Self {
            owner,
            runtime_owner: Some(runtime_owner),
            heap,
            availability,
            singletons,
        }
    }

    fn pin(&self) -> SexpResult<Option<crate::sexp::owner::OwnerPin>> {
        self.runtime_owner
            .as_ref()
            .map(|owner| owner.pin())
            .transpose()
    }

    pub(crate) fn require_active(&self) -> SexpResult<()> {
        let _pin = self.pin()?;
        if !self.availability.is_live() {
            return Err(SexpError::RootUnavailable);
        }
        self.owner.require_active()
    }

    pub(crate) fn link(&self, value: &Sexp<'_>) -> SexpResult<crate::sexp::heap::NodeLink> {
        value.link_in(&self.heap)
    }

    pub(crate) fn nil(&self) -> Sexp<'session> {
        Sexp::from_singleton(self.singletons.nil(), self.singletons.clone())
    }

    pub(crate) fn missing(&self) -> Sexp<'session> {
        Sexp::from_singleton(self.singletons.missing(), self.singletons.clone())
    }

    pub(crate) fn unbound(&self) -> Sexp<'session> {
        Sexp::from_singleton(self.singletons.unbound(), self.singletons.clone())
    }

    /// Publish a fully initialized promise with original checked child links.
    pub(crate) fn promise(
        &self,
        expression: &Sexp<'_>,
        environment: &Sexp<'_>,
    ) -> SexpResult<Sexp<'session>> {
        use crate::sexp::ffi::{NodeBody, Promsxp, SEXPTYPE};

        self.require_active()?;
        let expression = expression.clone();
        let environment = environment.clone();
        environment.ensure_live()?;
        if !matches!(environment.typeof_(), SEXPTYPE::NILSXP | SEXPTYPE::ENVSXP) {
            return Err(SexpError::TypeMismatch {
                expected: "promise environment or NULL",
                actual: environment.typeof_(),
            });
        }
        let body = NodeBody::Promise(Promsxp {
            value: self.link(&self.unbound())?,
            expr: self.link(&expression)?,
            env: self.link(&environment)?,
        });
        self.allocate(|arena| {
            let pointer = arena.alloc_node(SEXPTYPE::PROMSXP);
            let node = arena.node_token(pointer)?;
            let mut header = self.heap.node_snapshot(&node)?;
            header.data = body;
            self.heap.replace_node(&node, header)?;
            Some(pointer)
        })
    }

    /// Initialize every list edge before automatic rooting and callbacks.
    pub(crate) fn pairlist_cell(
        &self,
        value: &Sexp<'_>,
        rest: &Sexp<'_>,
        tag: &Sexp<'_>,
    ) -> SexpResult<Sexp<'session>> {
        use crate::sexp::ffi::{Listsxp, NodeBody, SEXPTYPE};

        self.require_active()?;
        let value = value.clone();
        let rest = rest.clone();
        let tag = tag.clone();
        let body = NodeBody::List(Listsxp {
            carval: self.link(&value)?,
            cdrval: self.link(&rest)?,
            tagval: self.link(&tag)?,
        });
        self.allocate(|arena| {
            let pointer = arena.alloc_node(SEXPTYPE::LISTSXP);
            let node = arena.node_token(pointer)?;
            let mut header = self.heap.node_snapshot(&node)?;
            header.data = body;
            self.heap.replace_node(&node, header)?;
            Some(pointer)
        })
    }

    /// Copy bounded Rust text without requiring a trailing NUL byte.
    pub(crate) fn character(&self, text: &str) -> SexpResult<Sexp<'session>> {
        self.allocate(|arena| Some(arena.alloc_charsxp(text.as_bytes())))
    }

    /// Build an owning character vector, retaining it and each child across GC.
    pub(crate) fn strings(&self, text: &[&str]) -> SexpResult<Sexp<'session>> {
        let length = text
            .len()
            .try_into()
            .map_err(|_| SexpError::AllocationFailed {
                object: "character vector",
            })?;
        let vector = self.allocate(|arena| {
            Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::STRSXP, length))
        })?;
        let mut vector = super::SexpMut::try_from_checked(vector)?;
        for (index, text) in text.iter().enumerate() {
            let character = self.character(text)?;
            vector.try_set_string_elt(index as _, character)?;
        }
        Ok(vector.freeze())
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
        let pin = self.pin()?;
        let result = self.owner.with_arena(|arena| {
            let pointer =
                allocation(arena).ok_or(SexpError::AllocationFailed { object: "R value" })?;
            self.wrap(pointer)
        })??;
        if let Some(pin) = &pin {
            pin.require_live()?;
        }
        Ok(result)
    }

    pub(crate) fn wrap(&self, pointer: SEXP) -> SexpResult<Sexp<'session>> {
        if !self.availability.is_live() {
            return Err(SexpError::RootUnavailable);
        }
        if let Some(singleton) = self.singletons.lease(pointer) {
            return Ok(Sexp::from_singleton(singleton, self.singletons.clone()));
        }
        if let Some(singleton) = self.heap.retained_singleton(pointer) {
            return Ok(Sexp::from_singleton_lease(
                singleton,
                Some(self.singletons.clone()),
            ));
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
            runtime_owner: self.runtime_owner.clone(),
            session_owner_ptr: if self.runtime_owner.is_some() {
                None
            } else {
                std::ptr::NonNull::new(self.owner.as_ptr())
            },
            root: Some(root),
            singleton: None,
            singletons: Some(self.singletons.clone()),
            _marker: std::marker::PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{
        ffi::{NodeBody, SEXPTYPE},
        globals::close_immutable_singletons_for_test,
        session::RSession,
    };
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn checked_producers_publish_initialized_roots_and_keep_original_sentinels() {
        let mut session = RSession::new_for_gc_tests();
        {
            let (result, _, _) = session.eval_code_with_output_capture("gctorture(TRUE)");
            result.unwrap();
        }
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let expression = factory.character("saved promise expression").unwrap();
            let environment = factory.nil();
            assert!(matches!(
                factory.promise(&expression, &expression),
                Err(SexpError::TypeMismatch {
                    expected: "promise environment or NULL",
                    actual: SEXPTYPE::CHARSXP
                })
            ));
            let unbound = factory.unbound();
            let missing = factory.missing();
            let expression_link = factory.link(&expression).unwrap();
            let environment_link = factory.link(&environment).unwrap();
            let unbound_link = factory.link(&unbound).unwrap();
            let missing_link = factory.link(&missing).unwrap();
            close_immutable_singletons_for_test();
            let replacement_pool = crate::sexp::globals::immutable_singleton_pool();
            assert_ne!(unbound.as_raw(), replacement_pool.unbound().projection());
            assert_ne!(missing.as_raw(), replacement_pool.missing().projection());
            assert_eq!(factory.unbound().as_raw(), unbound.as_raw());
            assert_eq!(factory.missing().as_raw(), missing.as_raw());

            let notifications = Rc::new(Cell::new(0));
            let observed = notifications.clone();
            let heap = factory.heap.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                for (_, node) in crate::sexp::memory::fresh_allocation_roots(&heap) {
                    let header = heap.node_snapshot(&node).unwrap();
                    match header.data {
                        NodeBody::Promise(body) => {
                            assert_eq!(body.expr, expression_link);
                            assert_eq!(body.env, environment_link);
                            assert_eq!(body.value, unbound_link);
                        }
                        NodeBody::List(body) => {
                            assert_eq!(body.carval, missing_link);
                            assert_eq!(body.cdrval, environment_link);
                            assert_eq!(body.tagval, environment_link);
                        }
                        _ => continue,
                    }
                    assert!(node.root_count() > 0);
                    observed.set(observed.get() + 1);
                    crate::sexp::gengc::full_gc();
                    assert!(node.is_live());
                }
            }));

            let promise = factory.promise(&expression, &environment).unwrap();
            let cell = factory
                .pairlist_cell(&missing, &environment, &environment)
                .unwrap();
            assert_eq!(notifications.get(), 2);
            drop(expression);
            drop(environment);
            drop(unbound);
            drop(missing);
            owner.full_gc().unwrap();
            assert_eq!(promise.typeof_(), SEXPTYPE::PROMSXP);
            assert_eq!(
                factory.link(&promise.try_prcode().unwrap()).unwrap(),
                expression_link
            );
            assert_eq!(
                factory.link(&promise.try_prenv().unwrap()).unwrap(),
                environment_link
            );
            assert_eq!(
                factory.link(&promise.try_prvalue().unwrap()).unwrap(),
                unbound_link
            );
            assert_eq!(
                factory.link(&cell.try_car().unwrap()).unwrap(),
                missing_link
            );
        });
    }
}
