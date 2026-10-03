#![forbid(unsafe_code)]
//! Physical node domains are independent of short-lived execution authority.

use super::{Sexp, SexpError, SexpOwner, SexpResult};
use crate::sexp::{
    ffi::SEXP,
    heap::HeapIdentity,
    instance::InstanceLiveness,
    owner::{with_runtime, OwnerToken, RuntimeAccess, StoredOwner, WeakOwner},
};

/// Passive provenance and physical publication checks. A domain has no arena,
/// runtime upgrade, evaluation, or collection operation. Value affiliation is
/// copied only into returned handles for their later embedding callbacks.
#[derive(Clone)]
pub(crate) struct NodeDomain<'session> {
    heap: HeapIdentity,
    availability: InstanceLiveness,
    singletons: crate::sexp::globals::SingletonPoolLease,
    owner_identity: usize,
    runtime_owner: Option<WeakOwner>,
    borrowed_pointer: Option<std::ptr::NonNull<crate::sexp::instance::RInstance>>,
    _session: std::marker::PhantomData<&'session crate::sexp::session::RSession>,
}

impl<'session> NodeDomain<'session> {
    fn from_snapshot(
        owner: &StoredOwner<'session>,
        heap: HeapIdentity,
        availability: InstanceLiveness,
        singletons: crate::sexp::globals::SingletonPoolLease,
    ) -> Self {
        Self {
            heap,
            availability,
            singletons,
            owner_identity: owner.identity_ptr().addr(),
            runtime_owner: owner.managed(),
            borrowed_pointer: owner.borrowed_pointer(),
            _session: std::marker::PhantomData,
        }
    }

    pub(crate) fn from_capability(
        heap: HeapIdentity,
        availability: InstanceLiveness,
        singletons: crate::sexp::globals::SingletonPoolLease,
        runtime_owner: WeakOwner,
    ) -> Self {
        Self {
            heap,
            availability,
            singletons,
            owner_identity: runtime_owner.identity_ptr().addr(),
            runtime_owner: Some(runtime_owner),
            borrowed_pointer: None,
            _session: std::marker::PhantomData,
        }
    }

    pub(crate) fn same_domain(&self, other: &NodeDomain<'_>) -> bool {
        self.heap.same_domain(&other.heap)
    }

    pub(crate) fn belongs_to(&self, heap: &HeapIdentity) -> bool {
        self.heap.same_domain(heap)
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

    pub(crate) fn logical(&self, value: bool) -> Sexp<'session> {
        Sexp::from_singleton(self.singletons.logical(value), self.singletons.clone())
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
            owner: SexpOwner::Session(self.owner_identity),
            node: Some(node),
            runtime_owner: self.runtime_owner.clone(),
            session_owner_ptr: self.borrowed_pointer,
            root: Some(root),
            singleton: None,
            singletons: Some(self.singletons.clone()),
            _marker: std::marker::PhantomData,
        })
    }
}

/// Allocation borrows explicit execution authority. It cannot be retained in
/// callback data after the closed execution scope ends.
pub(crate) struct NodeAllocator<'execution, 'session> {
    access: &'execution RuntimeAccess,
    domain: NodeDomain<'session>,
}

impl<'execution, 'session> NodeAllocator<'execution, 'session> {
    pub(crate) fn new(
        access: &'execution RuntimeAccess,
        domain: NodeDomain<'session>,
    ) -> SexpResult<Self> {
        access.require_active()?;
        if !domain.same_domain(&access.domain()) {
            return Err(SexpError::HeapDomainMismatch);
        }
        Ok(Self { access, domain })
    }

    pub(crate) fn allocate(
        &self,
        allocation: impl FnOnce(&mut crate::sexp::memory::RArena) -> Option<SEXP>,
    ) -> SexpResult<Sexp<'session>> {
        NodeProducer::allocate(self, allocation)
    }

    pub(crate) fn promise(
        &self,
        expression: &Sexp<'_>,
        environment: &Sexp<'_>,
    ) -> SexpResult<Sexp<'session>> {
        NodeProducer::promise(self, expression, environment)
    }

    pub(crate) fn pairlist_cell(
        &self,
        value: &Sexp<'_>,
        rest: &Sexp<'_>,
        tag: &Sexp<'_>,
    ) -> SexpResult<Sexp<'session>> {
        NodeProducer::pairlist_cell(self, value, rest, tag)
    }

    pub(crate) fn character(&self, text: &str) -> SexpResult<Sexp<'session>> {
        NodeProducer::character(self, text)
    }

    pub(crate) fn strings(&self, text: &[&str]) -> SexpResult<Sexp<'session>> {
        NodeProducer::strings(self, text)
    }
}

impl<'session> NodeProducer<'session> for NodeAllocator<'_, 'session> {
    fn domain(&self) -> &NodeDomain<'session> {
        &self.domain
    }
    fn require_active(&self) -> SexpResult<()> {
        self.access.require_active()
    }
    fn allocate(
        &self,
        allocation: impl FnOnce(&mut crate::sexp::memory::RArena) -> Option<SEXP>,
    ) -> SexpResult<Sexp<'session>> {
        self.access
            .with_arena(|arena| root_allocation(&self.domain, arena, allocation))?
    }
}

/// Compatibility entry point for existing translated callers. Managed
/// allocation delegates to a closed execution scope and an explicit allocator.
#[derive(Clone)]
pub(crate) struct SessionNodeFactory<'session> {
    owner: StoredOwner<'session>,
    domain: NodeDomain<'session>,
}

impl<'session> From<SessionNodeFactory<'session>> for NodeDomain<'session> {
    fn from(factory: SessionNodeFactory<'session>) -> Self {
        factory.domain
    }
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
        let owner = StoredOwner::from_token(owner);
        let domain = NodeDomain::from_snapshot(
            &owner,
            heap,
            availability,
            crate::sexp::globals::immutable_singleton_pool(),
        );
        Self { owner, domain }
    }

    pub(crate) fn from_capability(
        heap: HeapIdentity,
        availability: InstanceLiveness,
        singletons: crate::sexp::globals::SingletonPoolLease,
        runtime_owner: WeakOwner,
    ) -> Self {
        let domain =
            NodeDomain::from_capability(heap, availability, singletons, runtime_owner.clone());
        Self {
            owner: StoredOwner::Managed(runtime_owner),
            domain,
        }
    }

    pub(crate) fn domain(&self) -> NodeDomain<'session> {
        self.domain.clone()
    }
    pub(crate) fn require_active(&self) -> SexpResult<()> {
        self.owner.require_active()
    }
    pub(crate) fn link(&self, value: &Sexp<'_>) -> SexpResult<crate::sexp::heap::NodeLink> {
        self.domain.link(value)
    }
    pub(crate) fn nil(&self) -> Sexp<'session> {
        self.domain.nil()
    }
    pub(crate) fn missing(&self) -> Sexp<'session> {
        self.domain.missing()
    }
    pub(crate) fn unbound(&self) -> Sexp<'session> {
        self.domain.unbound()
    }
    pub(crate) fn wrap(&self, pointer: SEXP) -> SexpResult<Sexp<'session>> {
        self.domain.wrap(pointer)
    }

    pub(crate) fn allocate(
        &self,
        allocation: impl FnOnce(&mut crate::sexp::memory::RArena) -> Option<SEXP>,
    ) -> SexpResult<Sexp<'session>> {
        NodeProducer::allocate(self, allocation)
    }

    pub(crate) fn promise(
        &self,
        expression: &Sexp<'_>,
        environment: &Sexp<'_>,
    ) -> SexpResult<Sexp<'session>> {
        NodeProducer::promise(self, expression, environment)
    }

    pub(crate) fn pairlist_cell(
        &self,
        value: &Sexp<'_>,
        rest: &Sexp<'_>,
        tag: &Sexp<'_>,
    ) -> SexpResult<Sexp<'session>> {
        NodeProducer::pairlist_cell(self, value, rest, tag)
    }

    pub(crate) fn character(&self, text: &str) -> SexpResult<Sexp<'session>> {
        NodeProducer::character(self, text)
    }
    pub(crate) fn strings(&self, text: &[&str]) -> SexpResult<Sexp<'session>> {
        NodeProducer::strings(self, text)
    }
}

impl<'session> NodeProducer<'session> for SessionNodeFactory<'session> {
    fn domain(&self) -> &NodeDomain<'session> {
        &self.domain
    }
    fn require_active(&self) -> SexpResult<()> {
        self.owner.require_active()
    }
    fn allocate(
        &self,
        allocation: impl FnOnce(&mut crate::sexp::memory::RArena) -> Option<SEXP>,
    ) -> SexpResult<Sexp<'session>> {
        match &self.owner {
            StoredOwner::Managed(owner) => with_runtime(owner, |access| {
                access.allocator(&self.domain)?.allocate(allocation)
            })?,
            StoredOwner::Borrowed { .. } => self
                .owner
                .with_arena(|arena| root_allocation(&self.domain, arena, allocation))?,
        }
    }
}

/// Seal and root before releasing the arena, including deferred GC callbacks.
fn root_allocation<'session>(
    domain: &NodeDomain<'session>,
    arena: &mut crate::sexp::memory::RArena,
    allocation: impl FnOnce(&mut crate::sexp::memory::RArena) -> Option<SEXP>,
) -> SexpResult<Sexp<'session>> {
    let pointer = allocation(arena).ok_or(SexpError::AllocationFailed { object: "R value" })?;
    domain.wrap(pointer)
}

/// Both explicit and translated producers share initialization rules; this
/// private trait grants no additional execution or physical storage authority.
trait NodeProducer<'session> {
    fn domain(&self) -> &NodeDomain<'session>;
    fn require_active(&self) -> SexpResult<()>;
    fn allocate(
        &self,
        allocation: impl FnOnce(&mut crate::sexp::memory::RArena) -> Option<SEXP>,
    ) -> SexpResult<Sexp<'session>>;

    fn promise(&self, expression: &Sexp<'_>, environment: &Sexp<'_>) -> SexpResult<Sexp<'session>> {
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
        let domain = self.domain();
        let body = NodeBody::Promise(Promsxp {
            value: domain.link(&domain.unbound())?,
            expr: domain.link(&expression)?,
            env: domain.link(&environment)?,
        });
        self.allocate(|arena| {
            let pointer = arena.alloc_node(SEXPTYPE::PROMSXP);
            let node = arena.node_token(pointer)?;
            let mut header = domain.heap.node_snapshot(&node)?;
            header.data = body;
            domain.heap.replace_node(&node, header)?;
            Some(pointer)
        })
    }

    fn pairlist_cell(
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
        let domain = self.domain();
        let body = NodeBody::List(Listsxp {
            carval: domain.link(&value)?,
            cdrval: domain.link(&rest)?,
            tagval: domain.link(&tag)?,
        });
        self.allocate(|arena| {
            let pointer = arena.alloc_node(SEXPTYPE::LISTSXP);
            let node = arena.node_token(pointer)?;
            let mut header = domain.heap.node_snapshot(&node)?;
            header.data = body;
            domain.heap.replace_node(&node, header)?;
            Some(pointer)
        })
    }

    fn character(&self, text: &str) -> SexpResult<Sexp<'session>> {
        self.allocate(|arena| Some(arena.alloc_charsxp(text.as_bytes())))
    }

    fn strings(&self, text: &[&str]) -> SexpResult<Sexp<'session>> {
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
    fn explicit_allocator_rejects_foreign_original_heap() {
        let left = RSession::new_for_gc_tests();
        let left_owner = left.owner_token().unwrap().weak_owner().unwrap();
        let left_domain = with_runtime(&left_owner, |access| access.domain()).unwrap();
        let right = RSession::new_for_gc_tests();
        let right_owner = right.owner_token().unwrap().weak_owner().unwrap();
        with_runtime(&right_owner, |access| {
            assert!(matches!(
                access.allocator(&left_domain),
                Err(SexpError::HeapDomainMismatch)
            ));
            let domain = access.domain();
            let value = access
                .allocator(&domain)
                .unwrap()
                .character("right")
                .unwrap();
            assert!(domain.link(&value).is_ok());
            assert!(left_domain.link(&value).is_err());
        })
        .unwrap();
    }

    #[test]
    fn execution_scope_returns_actual_leases_without_retaining_runtime() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let (domain, character) = with_runtime(&owner, |access| {
            let domain = access.domain();
            let character = access
                .allocator(&domain)
                .unwrap()
                .character("physical lease outlives execution")
                .unwrap();
            (domain, character)
        })
        .unwrap();
        assert_eq!(owner.allocation_strong_count(), 1);
        let nil = domain.nil();
        let logical = domain.logical(true);
        close_immutable_singletons_for_test();
        let replacement = crate::sexp::globals::immutable_singleton_pool();
        assert_ne!(logical.as_raw(), replacement.logical(true).projection());
        assert_eq!(domain.logical(true).as_raw(), logical.as_raw());
        drop(session);
        assert_eq!(owner.allocation_strong_count(), 0);
        assert!(character
            .try_char_eq(b"physical lease outlives execution")
            .unwrap());
        assert_eq!(domain.nil().as_raw(), nil.as_raw());
        assert_eq!(logical.try_logical_elt(0).unwrap(), 1);
        assert!(domain.link(&character).is_ok());
        assert!(matches!(
            domain.wrap(character.as_raw()),
            Err(SexpError::RootUnavailable)
        ));
        assert!(matches!(
            with_runtime(&owner, |_| ()),
            Err(SexpError::RootUnavailable)
        ));
    }

    #[test]
    fn explicit_allocator_rejects_lent_arena_and_allows_reentry_after_release() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let nested = Rc::new(std::cell::RefCell::new(None));
        let nested_callback = nested.clone();
        let callback_owner = owner.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            let character = with_runtime(&callback_owner, |access| {
                let domain = access.domain();
                access
                    .allocator(&domain)
                    .unwrap()
                    .character("after arena release")
            })
            .unwrap()
            .unwrap();
            *nested_callback.borrow_mut() = Some(character);
        }));
        with_runtime(&owner, |access| {
            let domain = access.domain();
            access
                .with_arena(|_| {
                    assert!(matches!(
                        access.allocator(&domain).unwrap().character("nested lend"),
                        Err(SexpError::OwnerNotActive)
                    ));
                })
                .unwrap();
            let published = access
                .allocator(&domain)
                .unwrap()
                .character("before collection")
                .unwrap();
            access.with_native(|token| token.full_gc()).unwrap();
            assert!(published.try_char_eq(b"before collection").unwrap());
        })
        .unwrap();
        assert!(nested
            .borrow()
            .as_ref()
            .unwrap()
            .try_char_eq(b"after arena release")
            .unwrap());
        drop(session);
        assert_eq!(owner.allocation_strong_count(), 0);
    }

    #[test]
    fn execution_scope_denies_publication_after_collecting_callback_close_or_drop() {
        for drop_runtime in [false, true] {
            let session = RSession::new_for_gc_tests();
            let owner = session.owner_token().unwrap().weak_owner().unwrap();
            let sessions = Rc::new(std::cell::RefCell::new(Some(session)));
            let observed = Rc::new(Cell::new(false));
            let observed_callback = observed.clone();
            let callback_sessions = Rc::downgrade(&sessions);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed_callback.set(true);
                let sessions = callback_sessions.upgrade().unwrap();
                if drop_runtime {
                    let removed = sessions.borrow_mut().take();
                    drop(removed);
                } else {
                    sessions.borrow_mut().as_mut().unwrap().close();
                }
            }));
            let saved = std::cell::RefCell::new(None);
            let result = with_runtime(&owner, |access| {
                let domain = access.domain();
                let candidate = access
                    .allocator(&domain)
                    .unwrap()
                    .character("retained before callback")
                    .unwrap();
                *saved.borrow_mut() = Some(candidate.clone());
                let result = access.with_native(|token| {
                    token.full_gc()?;
                    Ok(candidate)
                });
                assert!(matches!(result, Err(SexpError::RootUnavailable)));
                assert!(owner.allocation_strong_count() >= 1);
                assert!(matches!(
                    access.allocator(&domain),
                    Err(SexpError::RootUnavailable)
                ));
                result
            });
            assert!(matches!(result, Err(SexpError::RootUnavailable)));
            assert!(observed.get());
            drop(sessions);
            assert_eq!(owner.allocation_strong_count(), 0);
            assert!(saved
                .borrow()
                .as_ref()
                .unwrap()
                .try_char_eq(b"retained before callback")
                .unwrap());
        }
    }

    #[test]
    fn managed_factories_retain_original_bank_without_raw_token_or_runtime_cycle() {
        let mut session = RSession::new_for_gc_tests();
        let weak = session.owner_token().unwrap().weak_owner().unwrap();
        let factory: SessionNodeFactory<'static> = weak.node_factory().unwrap();
        assert!(matches!(
            &factory.owner,
            crate::sexp::owner::StoredOwner::Managed(_)
        ));
        let nil = factory.nil();
        let character = factory
            .character("original owned factory bytes")
            .unwrap()
            .into_owned()
            .unwrap();
        close_immutable_singletons_for_test();
        let replacement = crate::sexp::globals::immutable_singleton_pool();
        assert_ne!(nil.as_raw(), replacement.nil().projection());
        // A fresh translated boundary still chooses this runtime's original
        // bank, rather than whichever process bank happens to be current.
        let later = session.owner_token().unwrap().node_factory();
        assert_eq!(later.nil().as_raw(), nil.as_raw());
        drop(later);
        session.close();
        assert!(matches!(
            factory.require_active(),
            Err(SexpError::RootUnavailable)
        ));
        assert!(matches!(
            factory.character("closed"),
            Err(SexpError::RootUnavailable)
        ));
        assert!(matches!(
            factory.wrap(character.as_raw()),
            Err(SexpError::RootUnavailable)
        ));
        drop(session);
        assert_eq!(weak.allocation_strong_count(), 0);
        assert_eq!(factory.nil().as_raw(), nil.as_raw());
        assert!(character
            .try_char_eq(b"original owned factory bytes")
            .unwrap());
    }

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
            let heap = factory.domain.heap.clone();
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
