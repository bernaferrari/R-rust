//! Lifetime-bound access to a live interpreter owner.
//!
//! This token borrows the session lifetime, not the whole RInstance. Field
//! access can therefore obey P1/P2 without holding a Rust instance reference
//! across callbacks. It never lends R payload references.

use std::{
    cell::UnsafeCell,
    marker::PhantomData,
    ptr::NonNull,
    rc::{Rc, Weak},
};

use super::{
    ffi::SEXP,
    instance::RInstance,
    object::{Sexp, SexpError, SexpResult},
    session::RSession,
};

/// Revocable authority to the original interpreter allocation. Values retain
/// this weak capability, never a strong edge back to their runtime.
#[derive(Clone)]
pub(crate) struct WeakOwner {
    allocation: Weak<UnsafeCell<RInstance>>,
    heap: super::heap::HeapIdentity,
    availability: super::instance::InstanceLiveness,
    singletons: super::globals::SingletonPoolLease,
    identity: *mut RInstance,
}

impl std::fmt::Debug for WeakOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeakOwner")
            .field("live", &self.is_live())
            .finish_non_exhaustive()
    }
}

impl WeakOwner {
    pub(crate) fn from_rc(allocation: &Rc<UnsafeCell<RInstance>>) -> Self {
        let identity = allocation.get();
        // The original Rc is retained by the caller. Both field snapshots end
        // before any interpreter entry or callback.
        let (heap, availability) = unsafe {
            (
                (*identity).heap_identity.clone(),
                super::instance::instance_liveness(identity),
            )
        };
        Self {
            allocation: Rc::downgrade(allocation),
            heap,
            availability,
            singletons: super::globals::immutable_singleton_pool(),
            identity,
        }
    }

    #[cfg(test)]
    pub(crate) fn allocation_strong_count(&self) -> usize {
        self.allocation.strong_count()
    }

    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.allocation, &other.allocation)
    }

    pub(crate) fn is_live(&self) -> bool {
        self.availability.is_live() && self.allocation.strong_count() != 0
    }

    /// Identity comparison only; dereferencing requires an operation pin.
    pub(crate) fn identity_ptr(&self) -> *mut RInstance {
        self.identity
    }

    pub(crate) fn pin(&self) -> SexpResult<OwnerPin> {
        if !self.availability.is_live() {
            return Err(SexpError::RootUnavailable);
        }
        let allocation = self
            .allocation
            .upgrade()
            .ok_or(SexpError::RootUnavailable)?;
        let pin = OwnerPin {
            allocation,
            availability: self.availability.clone(),
        };
        pin.require_live()?;
        Ok(pin)
    }

    pub(crate) fn node_factory<'s>(&self) -> SexpResult<super::object::SessionNodeFactory<'s>> {
        self.pin()?;
        Ok(super::object::SessionNodeFactory::from_capability(
            self.heap.clone(),
            self.availability.clone(),
            self.singletons.clone(),
            self.clone(),
        ))
    }
}

/// An operation-local strong lease. Cleanup can still access the original
/// physical allocation after revocation; successful work must recheck liveness.
pub(crate) struct OwnerPin {
    allocation: Rc<UnsafeCell<RInstance>>,
    availability: super::instance::InstanceLiveness,
}
impl OwnerPin {
    pub(crate) fn as_ptr(&self) -> *mut RInstance {
        self.allocation.get()
    }
    pub(crate) fn require_live(&self) -> SexpResult<()> {
        if self.availability.is_live() {
            Ok(())
        } else {
            Err(SexpError::RootUnavailable)
        }
    }
}

/// Execution authority exists only inside `with_runtime`. It cannot be cloned
/// or retained by provider data. Allocated values retain physical leases only.
pub(crate) struct RuntimeAccess {
    pin: OwnerPin,
    domain: super::object::NodeDomain<'static>,
}

/// Keep the original runtime alive for a closed execution scope. The higher
/// ranked callback cannot return a borrow of this authority or its allocator.
pub(crate) fn with_runtime<T>(
    owner: &WeakOwner,
    operation: impl for<'execution> FnOnce(&'execution RuntimeAccess) -> T,
) -> SexpResult<T> {
    let access = RuntimeAccess {
        pin: owner.pin()?,
        domain: super::object::NodeDomain::from_capability(
            owner.heap.clone(),
            owner.availability.clone(),
            owner.singletons.clone(),
            owner.clone(),
        ),
    };
    access.require_active()?;
    let result = operation(&access);
    access.require_active()?;
    Ok(result)
}

impl RuntimeAccess {
    pub(crate) fn require_live(&self) -> SexpResult<()> {
        self.pin.require_live()
    }

    pub(crate) fn require_active(&self) -> SexpResult<()> {
        self.require_live()?;
        if super::instance::current_instance_ptr() == Some(self.pin.as_ptr()) {
            Ok(())
        } else {
            Err(SexpError::OwnerNotActive)
        }
    }

    pub(crate) fn domain(&self) -> super::object::NodeDomain<'static> {
        self.domain.clone()
    }

    pub(crate) fn allocator<'execution, 'session>(
        &'execution self,
        domain: &super::object::NodeDomain<'session>,
    ) -> SexpResult<super::object::NodeAllocator<'execution, 'session>> {
        super::object::NodeAllocator::new(self, domain.clone())
    }

    /// The exclusive arena loan ends before warnings, GC, and destructors can
    /// reenter the runtime. The pin retains cleanup storage after revocation.
    pub(crate) fn with_arena<T>(
        &self,
        operation: impl FnOnce(&mut super::memory::RArena) -> T,
    ) -> SexpResult<T> {
        self.require_active()?;
        if super::memory::is_arena_lent(self.pin.as_ptr()) {
            return Err(SexpError::OwnerNotActive);
        }
        let result = unsafe { super::memory::with_arena_in(self.pin.as_ptr(), operation) };
        self.require_active()?;
        Ok(result)
    }

    /// Native adapters receive a token for this callback only. Its lifetime
    /// cannot escape directly through the result type. This remains an audited
    /// translated boundary: adapters can explicitly derive native owning pins.
    pub(crate) fn with_native<T>(
        &self,
        operation: impl for<'operation> FnOnce(OwnerToken<'operation>) -> SexpResult<T>,
    ) -> SexpResult<T> {
        self.require_active()?;
        let result = operation(unsafe { OwnerToken::from_raw(self.pin.as_ptr()) });
        self.require_active()?;
        result
    }
}

/// Stored authority has a distinct representation for managed runtimes and
/// explicitly borrowed native fixtures. Managed storage contains no raw token.
#[derive(Clone)]
pub(crate) enum StoredOwner<'session> {
    Managed(WeakOwner),
    Borrowed {
        token: OwnerToken<'session>,
        availability: super::instance::InstanceLiveness,
    },
}

impl<'session> StoredOwner<'session> {
    pub(crate) fn from_token(token: OwnerToken<'session>) -> Self {
        match token.weak_owner() {
            Some(owner) => Self::Managed(owner),
            None => Self::Borrowed {
                token,
                availability: unsafe { super::instance::instance_liveness(token.as_ptr()) },
            },
        }
    }

    pub(crate) fn from_value(value: &Sexp<'session>) -> SexpResult<Self> {
        if !value.is_live() {
            return Err(SexpError::StaleAllocation);
        }
        if let Some(owner) = &value.runtime_owner {
            owner.pin()?;
            return Ok(Self::Managed(owner.clone()));
        }
        let pointer = value.session_owner_ptr.ok_or(SexpError::RootUnavailable)?;
        // The borrowed view retains its explicitly proven fixture lifetime.
        Ok(Self::from_token(unsafe {
            OwnerToken::from_raw(pointer.as_ptr())
        }))
    }

    pub(crate) fn into_owned(self) -> SexpResult<StoredOwner<'static>> {
        match self {
            Self::Managed(owner) => Ok(StoredOwner::Managed(owner)),
            Self::Borrowed { .. } => Err(SexpError::RootUnavailable),
        }
    }

    pub(crate) fn identity_ptr(&self) -> *mut RInstance {
        match self {
            Self::Managed(owner) => owner.identity_ptr(),
            Self::Borrowed { token, .. } => token.as_ptr(),
        }
    }

    pub(crate) fn managed(&self) -> Option<WeakOwner> {
        match self {
            Self::Managed(owner) => Some(owner.clone()),
            Self::Borrowed { .. } => None,
        }
    }

    pub(crate) fn borrowed_pointer(&self) -> Option<NonNull<RInstance>> {
        match self {
            Self::Managed(_) => None,
            Self::Borrowed { token, .. } => NonNull::new(token.as_ptr()),
        }
    }

    /// Run one operation while physically retaining the original allocation.
    /// A raw projection grants no safe dereference authority to the closure.
    pub(crate) fn with_projection<T>(
        &self,
        f: impl FnOnce(*mut RInstance) -> SexpResult<T>,
    ) -> SexpResult<T> {
        match self {
            Self::Managed(owner) => {
                let pin = owner.pin()?;
                let result = f(pin.as_ptr());
                pin.require_live()?;
                result
            }
            Self::Borrowed {
                token,
                availability,
            } => {
                if !availability.is_live() {
                    return Err(SexpError::RootUnavailable);
                }
                let result = f(token.as_ptr());
                if !availability.is_live() {
                    return Err(SexpError::RootUnavailable);
                }
                result
            }
        }
    }

    pub(crate) fn require_active(&self) -> SexpResult<()> {
        self.with_projection(|pointer| {
            if super::instance::current_instance_ptr() == Some(pointer) {
                Ok(())
            } else {
                Err(SexpError::OwnerNotActive)
            }
        })
    }

    pub(crate) fn with_arena<T>(
        &self,
        f: impl FnOnce(&mut super::memory::RArena) -> T,
    ) -> SexpResult<T> {
        self.with_projection(|pointer| {
            if super::instance::current_instance_ptr() != Some(pointer)
                || super::memory::is_arena_lent(pointer)
            {
                return Err(SexpError::OwnerNotActive);
            }
            Ok(unsafe { super::memory::with_arena_in(pointer, f) })
        })
    }

    pub(crate) fn node_factory(&self) -> SexpResult<super::object::SessionNodeFactory<'session>> {
        match self {
            Self::Managed(owner) => owner.node_factory(),
            Self::Borrowed {
                token,
                availability,
            } => {
                if !availability.is_live() {
                    return Err(SexpError::RootUnavailable);
                }
                Ok(token.node_factory())
            }
        }
    }

    pub(crate) fn sexp(&self, pointer: SEXP) -> SexpResult<Sexp<'session>> {
        self.node_factory()?.wrap(pointer)
    }
}

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
        let _pin = self.pin()?;
        self.require_active()?;
        if super::memory::is_arena_lent(self.as_ptr()) {
            return Err(SexpError::OwnerNotActive);
        }
        // SAFETY: the lifetime-bound token retains the owner, and the checked
        // lend state excludes an overlapping arena loan. Field-local lending
        // ends before deferred callbacks and GC run.
        let result = unsafe { super::memory::with_arena_in(self.as_ptr(), f) };
        if let Some(pin) = &_pin {
            pin.require_live()?;
        }
        Ok(result)
    }

    pub(crate) fn as_ptr(self) -> *mut RInstance {
        self.pointer.as_ptr()
    }

    pub(crate) fn weak_owner(self) -> Option<WeakOwner> {
        // SAFETY: this token is created only at a lifetime-bound raw entry.
        unsafe { (*self.as_ptr()).runtime_owner.clone() }
    }

    pub(crate) fn pin(self) -> SexpResult<Option<OwnerPin>> {
        self.weak_owner().map(|owner| owner.pin()).transpose()
    }

    /// Capture immutable allocation-domain and availability capabilities before
    /// lending the arena. Later node wrapping needs no instance field access.
    pub(crate) fn node_factory(self) -> super::object::SessionNodeFactory<'session> {
        if let Some(owner) = self.weak_owner() {
            return owner
                .node_factory()
                .expect("live managed node factory owner");
        }
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
        if self.weak_owner().is_some_and(|owner| !owner.is_live()) {
            return Err(SexpError::RootUnavailable);
        }
        if super::instance::current_instance_ptr() == Some(self.as_ptr()) {
            Ok(())
        } else {
            Err(SexpError::OwnerNotActive)
        }
    }

    pub(crate) fn minor_gc(self) -> SexpResult<(usize, usize)> {
        let _pin = self.pin()?;
        self.require_active()?;
        // SAFETY: the token retains its live owner and excludes Rust payload loans.
        let result = unsafe { super::gengc::minor_gc_in(self.as_ptr()) };
        if let Some(pin) = &_pin {
            pin.require_live()?;
        }
        Ok(result)
    }

    pub(crate) fn full_gc(self) -> SexpResult<(usize, usize)> {
        let _pin = self.pin()?;
        self.require_active()?;
        // SAFETY: same owner/loan contract as minor_gc; arena lends defer collection.
        let result = unsafe { super::gengc::full_gc_in(self.as_ptr()) };
        if let Some(pin) = &_pin {
            pin.require_live()?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{
        altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
        ffi::SEXPTYPE,
        object::SexpMut,
    };
    use std::cell::Cell;

    #[test]
    fn owned_value_preserves_original_graph_after_runtime_close_and_drop_without_cycle() {
        let mut session = RSession::new_for_gc_tests();
        let (value, weak, id) = session.with_active(|| {
            let token = session.owner_token().unwrap();
            let factory = token.node_factory();
            let child = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let mut child = SexpMut::try_from_checked(child).unwrap();
            child.try_set_integer_elt(0, 73).unwrap();
            let vector = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 2)))
                .unwrap();
            let mut vector = SexpMut::try_from_checked(vector).unwrap();
            vector.try_set_vector_elt(0, child.freeze()).unwrap();
            vector.try_set_vector_elt(1, factory.nil()).unwrap();
            let value = vector.freeze().into_owned().unwrap();
            let id = value.allocation().unwrap().id().clone();
            (value, token.weak_owner().unwrap(), id)
        });
        session.close();
        assert!(matches!(weak.pin(), Err(SexpError::RootUnavailable)));
        assert!(matches!(
            value.node_factory(),
            Err(SexpError::RootUnavailable)
        ));
        assert_eq!(
            value.try_vector_elt(0).unwrap().try_integer_elt(0).unwrap(),
            73
        );
        drop(session);
        assert_eq!(
            weak.allocation.strong_count(),
            0,
            "values must not own their runtime"
        );
        assert_eq!(value.allocation().unwrap().id(), &id);
        assert_eq!(
            value.try_vector_elt(0).unwrap().try_integer_elt(0).unwrap(),
            73
        );
        assert!(value.try_vector_elt(1).unwrap().is_null_value());
    }

    struct CountReads(Rc<Cell<usize>>);
    impl AltrepClass for CountReads {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::INTSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(2)
        }
        fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            self.0.set(self.0.get() + 1);
            Ok(AltrepElement::Integer(17))
        }
    }
    #[test]
    fn owned_lazy_value_rejects_revoked_provider_before_callback() {
        let session = RSession::new_for_gc_tests();
        let reads = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class("detached-owner-test", CountReads(reads.clone()))
            .unwrap();
        let value = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        drop(session);
        assert_eq!(value.len(), 2);
        assert!(matches!(
            value.try_integer_elt(0),
            Err(SexpError::RootUnavailable)
        ));
        assert_eq!(reads.get(), 0);
        assert!(matches!(
            crate::sexp::altrep::force_materialization(&value),
            Err(SexpError::RootUnavailable)
        ));
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn owned_conversion_rejects_borrowed_and_unchecked_views() {
        let mut arena = crate::sexp::memory::RArena::new();
        let raw = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        assert!(matches!(
            arena.sexp(raw).unwrap().into_owned(),
            Err(SexpError::RootUnavailable)
        ));
        assert!(matches!(
            unsafe { Sexp::try_from_raw(raw) }.unwrap().into_owned(),
            Err(SexpError::RootUnavailable)
        ));
        assert!(Sexp::nil().into_owned().unwrap().is_null_value());
    }
    thread_local! {
        static CLOSE_ON_READ: std::cell::RefCell<Option<RSession>> = const { std::cell::RefCell::new(None) };
    }
    struct CloseOnRead(Rc<Cell<usize>>);
    impl AltrepClass for CloseOnRead {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::INTSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(2)
        }
        fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            self.0.set(self.0.get() + 1);
            let mut session = CLOSE_ON_READ
                .with(|slot| slot.borrow_mut().take())
                .expect("callback owner");
            session.close();
            drop(session);
            Ok(AltrepElement::Integer(17))
        }
    }
    #[test]
    fn owned_lazy_callback_can_close_and_drop_runtime_but_cannot_publish_success() {
        for materialize in [false, true] {
            let session = RSession::new_for_gc_tests();
            let reads = Rc::new(Cell::new(0));
            let class = session
                .register_altrep_class("closing-owner-test", CloseOnRead(reads.clone()))
                .unwrap();
            let value = AltrepBuilder::new(class)
                .build()
                .unwrap()
                .into_owned()
                .unwrap();
            let weak = value.runtime_owner.clone().unwrap();
            CLOSE_ON_READ.with(|slot| *slot.borrow_mut() = Some(session));
            if materialize {
                assert!(matches!(
                    crate::sexp::altrep::force_materialization(&value),
                    Err(SexpError::RootUnavailable)
                ));
                assert!(!crate::sexp::altrep::is_materialized(&value));
            } else {
                assert!(matches!(
                    value.try_integer_elt(0),
                    Err(SexpError::RootUnavailable)
                ));
            }
            assert_eq!(reads.get(), 1);
            assert_eq!(
                weak.allocation.strong_count(),
                0,
                "operation pin must end after rejection"
            );
            assert_eq!(value.len(), 2);
            assert!(matches!(
                value.try_integer_elt(1),
                Err(SexpError::RootUnavailable)
            ));
            assert_eq!(reads.get(), 1);
        }
    }
}
