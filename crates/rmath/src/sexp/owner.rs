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

use crate::eval::jit::JitSettings;

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

    /// A managed owner already holds its heap identity independently of the
    /// mutable instance allocation. Revoked owners cannot supply ambient reads.
    pub(crate) fn live_heap_identity(&self) -> Option<super::heap::HeapIdentity> {
        self.is_live().then(|| self.heap.clone())
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
    /// Clone this pin's availability witness for later liveness checks.
    pub(crate) fn liveness(&self) -> super::instance::InstanceLiveness {
        self.availability.clone()
    }
    pub(crate) fn require_live(&self) -> SexpResult<()> {
        if self.availability.is_live() {
            Ok(())
        } else {
            Err(SexpError::RootUnavailable)
        }
    }

    /// Reborrow this pin's original owner, even if a different runtime is active.
    pub(crate) fn token(&self) -> SexpResult<OwnerToken<'_>> {
        self.require_live()?;
        // SAFETY: the pin retains this allocation and just checked availability.
        Ok(unsafe { OwnerToken::from_raw(self.as_ptr()) })
    }

    /// Lend this owner's output-capture cell for a strictly-local update.
    ///
    /// P1/P2: `f` must not run any ambient-capable call (allocation, protect,
    /// collection, eval) while the lend is live.
    pub(crate) fn with_output_capture<R>(
        &self,
        f: impl FnOnce(&std::cell::RefCell<super::output::OutputCaptureState>) -> R,
    ) -> R {
        // SAFETY: the pin retains the original live allocation; this is a
        // strictly-local single-field lend under the contract above.
        unsafe { f(&(*self.as_ptr()).output_capture) }
    }

    /// Lend this owner's error state for a strictly-local update.
    ///
    /// Same strictly-local lend contract as [`Self::with_output_capture`]. A
    /// nested lend of the same owner's error state panics instead of aliasing.
    pub(crate) fn with_error_state<R>(
        &self,
        f: impl FnOnce(&mut super::instance::ErrorState) -> R,
    ) -> R {
        // SAFETY: the pin retains the original allocation; the checked lend
        // excludes any other `&mut ErrorState` for this owner.
        unsafe { super::instance::with_error_state_in(self.as_ptr(), f) }
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
        let result = operation(self.pin.token()?);
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
        let pointer = value
            .session_owner_ptr
            .and_then(BorrowedOwnerRef::live)
            .ok_or(SexpError::RootUnavailable)?;
        // SAFETY: `live` just validated the owner generation, so the owner
        // has not begun teardown and its generation can be snapshotted.
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

    pub(crate) fn borrowed_pointer(&self) -> Option<BorrowedOwnerRef> {
        match self {
            Self::Managed(_) => None,
            Self::Borrowed { token, .. } => Some(BorrowedOwnerRef {
                pointer: token.pointer,
                generation: token.generation,
            }),
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

    pub(crate) fn full_gc(&self) -> SexpResult<()> {
        self.with_projection(|pointer| {
            // SAFETY: the stored owner projection retains this exact runtime;
            // `OwnerToken::full_gc` validates that it is active and rechecks
            // liveness after finalizers and collection callbacks run.
            unsafe { OwnerToken::from_raw(pointer).full_gc().map(|_| ()) }
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
            let result = unsafe { super::memory::with_arena_in(pointer, f) };
            if super::instance::current_instance_ptr() != Some(pointer) {
                return Err(SexpError::OwnerNotActive);
            }
            Ok(result)
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

/// A borrowed (non-managed) owner address recorded in a value, paired with
/// the owner generation observed when the value was produced. Dereferencing
/// requires [`Self::live`], so a value that outlives its borrowed fixture
/// owner can no longer reach the freed instance.
#[derive(Clone, Copy)]
pub(crate) struct BorrowedOwnerRef {
    pointer: NonNull<RInstance>,
    generation: super::instance::InstanceGeneration,
}

impl std::fmt::Debug for BorrowedOwnerRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BorrowedOwnerRef")
            .field("pointer", &self.pointer)
            .field("live", &self.generation.is_live())
            .finish()
    }
}

impl BorrowedOwnerRef {
    pub(crate) fn new(
        pointer: NonNull<RInstance>,
        generation: super::instance::InstanceGeneration,
    ) -> Self {
        Self {
            pointer,
            generation,
        }
    }

    /// Identity comparison only; never dereference this address directly.
    pub(crate) fn identity(self) -> NonNull<RInstance> {
        self.pointer
    }

    /// The owner address while its original owner has not begun teardown.
    pub(crate) fn live(self) -> Option<NonNull<RInstance>> {
        self.generation.is_live().then_some(self.pointer)
    }
}

/// A copyable, self-validating handle to one interpreter owner.
///
/// Soundness does not rest on `'session`: the token snapshots the owner's
/// [`InstanceGeneration`](super::instance::InstanceGeneration) while the
/// pointer is known live, and every method that dereferences the pointer first
/// checks that generation. The owner revokes its generation before any field
/// is torn down or freed, and owners are thread-confined while tokens are
/// `!Send` (`PhantomData<Rc<()>>`), so nothing can revoke the owner between a
/// successful check and the immediately following field access. A token
/// retained past teardown therefore reports `RootUnavailable` / `None` (or
/// raises an R error for the infallible field lends) instead of reading freed
/// memory, and a different owner later allocated at the same address has a
/// different generation.
#[derive(Clone, Copy)]
pub(crate) struct OwnerToken<'session> {
    pointer: NonNull<RInstance>,
    generation: super::instance::InstanceGeneration,
    _lifetime: PhantomData<(&'session RSession, Rc<()>)>,
}

impl<'session> OwnerToken<'session> {
    /// Bridge a raw owner pointer into the checked token layer.
    ///
    /// # Safety
    /// `pointer` refers to a live `RInstance` allocation at the time of the
    /// call (its generation is read here) whose address stays stable while
    /// it is live. Later token operations validate liveness themselves.
    pub(crate) unsafe fn from_raw(pointer: *mut RInstance) -> Self {
        let pointer = NonNull::new(pointer).expect("live owner pointer");
        Self {
            // SAFETY: the caller guarantees the owner is live right now.
            generation: unsafe { super::instance::instance_generation(pointer.as_ptr()) },
            pointer,
            _lifetime: PhantomData,
        }
    }

    /// Capture this thread's active owner.
    ///
    /// The `'static` lifetime is not a liveness claim: the token re-validates
    /// its owner generation before every dereference, so retaining it past
    /// runtime teardown yields errors rather than undefined behaviour.
    pub(crate) fn current() -> SexpResult<OwnerToken<'static>> {
        let pointer = super::instance::current_instance_ptr().ok_or(SexpError::OwnerNotActive)?;
        // SAFETY: `current_instance_ptr` only reports an installed owner whose
        // liveness witness is still live, and nothing runs between that check
        // and the generation snapshot.
        Ok(unsafe { OwnerToken::from_raw(pointer) })
    }

    /// Whether the token's original owner has not begun teardown.
    pub(crate) fn is_live(self) -> bool {
        self.generation.is_live()
    }

    /// `RootUnavailable` once the original owner has been revoked.
    pub(crate) fn require_live(self) -> SexpResult<()> {
        if self.is_live() {
            Ok(())
        } else {
            Err(SexpError::RootUnavailable)
        }
    }

    /// Infallible field lends raise an R error on a revoked owner, matching
    /// how their call sites already report unavailable owners.
    fn require_live_or_r_error(self) {
        if let Err(error) = self.require_live() {
            super::context::r_error(error.to_string());
        }
    }

    /// Run `operation` with a token for this thread's active runtime.
    ///
    /// Safe counterpart of [`current`](Self::current): the token's lifetime is
    /// higher-ranked, so it cannot escape the callback through its result, and
    /// the owner allocation stays pinned until the callback returns. Token
    /// operations keep their own dynamic checks (liveness, active owner, arena
    /// lend state).
    pub(crate) fn with_current<T>(
        operation: impl for<'operation> FnOnce(OwnerToken<'operation>) -> T,
    ) -> SexpResult<T> {
        let token = Self::current()?;
        let _pin = token.pin()?;
        token.require_active()?;
        Ok(operation(token))
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
        // SAFETY: `require_active` just validated the owner generation, and
        // the checked lend state excludes an overlapping arena loan.
        // Field-local lending ends before deferred callbacks and GC run.
        let result = unsafe { super::memory::with_arena_in(self.as_ptr(), f) };
        if let Some(pin) = &_pin {
            pin.require_live()?;
        }
        self.require_active()?;
        Ok(result)
    }

    pub(crate) fn as_ptr(self) -> *mut RInstance {
        self.pointer.as_ptr()
    }

    /// The owner's managed capability; `None` for a borrowed fixture owner or
    /// once the original owner has been revoked.
    pub(crate) fn weak_owner(self) -> Option<WeakOwner> {
        if !self.is_live() {
            return None;
        }
        // SAFETY: the generation check above proves the owner has not begun
        // teardown, and this short field clone runs no callback.
        unsafe { (*self.as_ptr()).runtime_owner.clone() }
    }

    /// Pin a managed owner (`None` for a live borrowed fixture owner).
    pub(crate) fn pin(self) -> SexpResult<Option<OwnerPin>> {
        self.require_live()?;
        self.weak_owner().map(|owner| owner.pin()).transpose()
    }

    /// Capture immutable allocation-domain and availability capabilities before
    /// lending the arena. Later node wrapping needs no instance field access.
    ///
    /// Raises an R error once the original owner has been revoked; see
    /// [`Self::try_node_factory`] for the fallible form.
    pub(crate) fn node_factory(self) -> super::object::SessionNodeFactory<'session> {
        self.try_node_factory()
            .unwrap_or_else(|error| super::context::r_error(error.to_string()))
    }

    /// Fallible [`Self::node_factory`]: `RootUnavailable` on a revoked owner.
    pub(crate) fn try_node_factory(
        self,
    ) -> SexpResult<super::object::SessionNodeFactory<'session>> {
        self.require_live()?;
        if let Some(owner) = self.weak_owner() {
            return owner.node_factory();
        }
        // SAFETY: the generation check above proves the owner is live. This
        // short field access ends before the factory can be used inside an
        // arena lend.
        let (heap, availability) = unsafe {
            (
                (*self.as_ptr()).heap_identity.clone(),
                super::instance::instance_liveness(self.as_ptr()),
            )
        };
        Ok(super::object::SessionNodeFactory::from_snapshot(
            self,
            heap,
            availability,
        ))
    }

    /// Validate and root a pointer in this owner, even if another is active.
    pub(crate) fn sexp(self, pointer: SEXP) -> SexpResult<Sexp<'session>> {
        Sexp::from_owner_raw(pointer, self)
    }

    /// Lend the token owner's output-capture cell for a strictly-local update.
    ///
    /// P1/P2: `f` must not run any ambient-capable call (allocation, protect,
    /// collection, eval) while the lend is live.
    pub(crate) fn with_output_capture<R>(
        self,
        f: impl FnOnce(&std::cell::RefCell<super::output::OutputCaptureState>) -> R,
    ) -> R {
        self.require_live_or_r_error();
        // SAFETY: the owner generation was just validated; this is a
        // strictly-local shared lend of a `RefCell` (interior mutability is
        // dynamically checked), so nested lends cannot alias mutably.
        unsafe { f(&(*self.as_ptr()).output_capture) }
    }

    /// Clone the owner's session-local connection-table handle. The `Rc` keeps
    /// borrows valid even when this owner is later closed.
    pub(crate) fn connection_table(
        self,
    ) -> Rc<std::cell::RefCell<crate::mainutils::connections::ConnectionTable>> {
        self.require_live_or_r_error();
        // SAFETY: the generation is live and we only clone the
        // connection-table handle.
        unsafe { (*self.as_ptr()).connection_table.clone() }
    }

    /// Copy the owner's session-local compiler settings. Liveness, not active
    /// status, is required so metadata restoration can target its original
    /// owner while another session is active.
    pub(crate) fn jit_settings(self) -> SexpResult<JitSettings> {
        self.require_live()?;
        // SAFETY: the owner is live and Cell::get only copies scalar settings.
        Ok(unsafe { (*self.as_ptr()).jit_settings.get() })
    }

    /// Replace the owner's session-local compiler settings.
    pub(crate) fn set_jit_settings(self, settings: JitSettings) -> SexpResult<()> {
        self.require_live()?;
        // SAFETY: the owner is live and Cell::set holds no evaluator borrow.
        unsafe { (*self.as_ptr()).jit_settings.set(settings) };
        Ok(())
    }

    /// Clone the owner's session-local sink-state handle. The `Rc` keeps
    /// cleanup valid even when this owner is later closed.
    pub(crate) fn sink_state(
        self,
    ) -> Rc<std::cell::RefCell<crate::mainutils::connections::SinkState>> {
        self.require_live_or_r_error();
        // SAFETY: the generation is live and we only clone the sink handle.
        unsafe { (*self.as_ptr()).sink_state.clone() }
    }

    pub(crate) fn browser_file_state(
        self,
    ) -> SexpResult<Rc<crate::mainutils::browser_files::BrowserFileState>> {
        self.require_live()?;
        // SAFETY: the owner generation is live and this only clones its store handle.
        Ok(unsafe { (*self.as_ptr()).browser_files.clone() })
    }

    pub(crate) fn reserve_file_bytes(
        self,
        bytes: usize,
    ) -> Option<super::memory::TransientReservation> {
        self.require_active().ok()?;
        if super::memory::is_arena_lent(self.as_ptr()) {
            return None;
        }
        // SAFETY: the owner is active and its arena is not lent. Admission only
        // updates accounting and cannot collect or reenter the interpreter.
        unsafe { super::memory::reserve_transient_in(self.as_ptr(), bytes) }
    }

    /// Collection callbacks use ambient R dispatch, so require this owner active.
    pub(crate) fn require_active(self) -> SexpResult<()> {
        self.require_live()?;
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
        // SAFETY: `require_active` validated the owner generation; Rust
        // payload loans are excluded by the collector's own lend checks.
        let result = unsafe { super::gengc::minor_gc_in(self.as_ptr()) };
        if let Some(pin) = &_pin {
            pin.require_live()?;
        }
        self.require_active()?;
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
        self.require_active()?;
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
    fn checked_owner_operations_reject_resource_destructor_runtime_replacement() {
        struct ReplaceOnDrop(Rc<std::cell::RefCell<Option<RSession>>>);
        impl Drop for ReplaceOnDrop {
            fn drop(&mut self) {
                *self.0.borrow_mut() = Some(RSession::new_for_gc_tests());
            }
        }
        for operation in 0..4 {
            let session = RSession::new_for_gc_tests();
            let owner = session.owner_token().unwrap();
            let stored = StoredOwner::from_token(owner);
            let replacement = Rc::new(std::cell::RefCell::new(None));
            let pointer = owner
                .with_arena(|arena| {
                    let pointer = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
                    let node = arena.node_token(pointer).unwrap();
                    arena
                        .heap_identity()
                        .attach_resource(&node, Rc::new(ReplaceOnDrop(replacement.clone())))
                        .unwrap();
                    pointer
                })
                .unwrap();
            let result = match operation {
                0 => owner.with_arena(|arena| unsafe {
                    arena.free_node(pointer);
                }),
                1 => stored.with_arena(|arena| unsafe {
                    arena.free_node(pointer);
                }),
                2 => owner.minor_gc().map(|_| ()),
                _ => owner.full_gc().map(|_| ()),
            };
            assert!(
                matches!(result, Err(SexpError::OwnerNotActive)),
                "operation {operation} published success after owner replacement: {result:?}"
            );
            let other = replacement
                .borrow_mut()
                .take()
                .expect("resource destructor ran");
            assert!(other.is_active());
            assert!(session.is_active());
            drop(other);
            drop(session);
        }
    }

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

    #[test]
    fn live_token_paths_are_unchanged() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let token = OwnerToken::current().unwrap();
            assert!(token.is_live());
            assert!(token.require_active().is_ok());
            assert!(token.weak_owner().is_some());
            assert!(token.pin().unwrap().is_some());
            assert!(token.try_node_factory().is_ok());
            assert!(!token.with_output_capture(|capture| capture.borrow().is_capturing()));
            assert_eq!(token.sink_state().borrow().sink_number, 0);
            let pointer = token
                .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
                .unwrap();
            assert!(!pointer.is_null());
            assert!(token.minor_gc().is_ok());
        });
    }

    #[test]
    fn token_retained_past_runtime_teardown_reports_unavailable() {
        let session = RSession::new_for_gc_tests();
        // `current()` is safe and yields a `'static` token that safe code can
        // keep after the runtime is gone.
        let token = session.with_active(|| OwnerToken::current().unwrap());
        assert!(token.is_live());
        drop(session);
        // A later runtime may reuse the freed address; the generation differs.
        let replacement = RSession::new_for_gc_tests();
        replacement.with_active(|| {
            assert!(!token.is_live());
            assert!(token.weak_owner().is_none());
            assert!(matches!(
                token.require_live(),
                Err(SexpError::RootUnavailable)
            ));
            assert!(matches!(token.pin(), Err(SexpError::RootUnavailable)));
            assert!(matches!(
                token.require_active(),
                Err(SexpError::RootUnavailable)
            ));
            assert!(matches!(
                token.try_node_factory(),
                Err(SexpError::RootUnavailable)
            ));
            assert!(matches!(
                token.with_arena(|_| ()),
                Err(SexpError::RootUnavailable)
            ));
            assert!(matches!(token.minor_gc(), Err(SexpError::RootUnavailable)));
            assert!(matches!(token.full_gc(), Err(SexpError::RootUnavailable)));
            // Infallible lends raise an R error rather than dereferencing.
            let lend = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                token.with_output_capture(|_| ())
            }));
            assert!(lend.is_err());
            // The replacement runtime's own token is unaffected.
            assert!(OwnerToken::current().unwrap().require_active().is_ok());
        });
    }

    #[test]
    fn token_rejects_explicitly_closed_runtime() {
        let mut session = RSession::new_for_gc_tests();
        let token = session.with_active(|| OwnerToken::current().unwrap());
        session.close();
        assert!(!token.is_live());
        assert!(token.weak_owner().is_none());
        assert!(matches!(token.pin(), Err(SexpError::RootUnavailable)));
        drop(session);
        assert!(!token.is_live());
    }

    #[test]
    fn pinned_token_preserves_original_owner_and_rejects_revocation() {
        let mut first = RSession::new_for_gc_tests();
        let second = RSession::new_for_gc_tests();
        let pin = first.with_active(|| OwnerToken::current().unwrap().pin().unwrap().unwrap());
        second.with_active(|| {
            let token = pin.token().unwrap();
            assert_eq!(token.as_ptr(), pin.as_ptr());
            assert_ne!(token.as_ptr(), OwnerToken::current().unwrap().as_ptr());
            assert!(matches!(token.require_active(), Err(SexpError::OwnerNotActive)));
        });
        first.close();
        assert!(matches!(pin.token(), Err(SexpError::RootUnavailable)));
    }

    #[test]
    fn nested_error_state_lend_is_rejected() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let pin = OwnerToken::current().unwrap().pin().unwrap().unwrap();
            let nested = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                pin.with_error_state(|_outer| pin.with_error_state(|_inner| ()))
            }));
            assert!(nested.is_err(), "a second &mut ErrorState must not be lent");
            assert!(!super::super::instance::is_error_state_lent(pin.as_ptr()));
            // The unwind released the outer lend; normal lends still work.
            pin.with_error_state(|state| state.toplevel_expr_no = 3);
            assert_eq!(pin.with_error_state(|state| state.toplevel_expr_no), 3);
            pin.with_error_state(|state| state.toplevel_expr_no = 0);
        });
    }

    #[test]
    fn borrowed_owner_value_rejects_revoked_fixture_owner() {
        let mut instance = Box::new(super::super::instance::RInstance::new());
        let pointer: *mut RInstance = &mut *instance;
        // SAFETY: the boxed fixture is live and pinned for this test.
        let token = unsafe { OwnerToken::from_raw(pointer) };
        assert!(token.weak_owner().is_none(), "standalone fixture owner");
        // SAFETY: the fixture stays live across the activation.
        let value = unsafe {
            crate::sexp::session::with_instance_active(pointer, || {
                let raw = token
                    .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
                    .unwrap();
                token.sexp(raw).unwrap()
            })
        };
        assert!(
            value
                .session_owner_ptr
                .is_some_and(|owner| owner.live().is_some())
        );
        assert!(StoredOwner::from_value(&value).is_ok());
        // SAFETY: the fixture is live; nothing borrows its availability field.
        unsafe { super::super::instance::revoke_instance_availability(pointer) };
        assert!(!token.is_live());
        assert!(
            value
                .session_owner_ptr
                .is_some_and(|owner| owner.live().is_none())
        );
        assert!(matches!(
            StoredOwner::from_value(&value),
            Err(SexpError::RootUnavailable) | Err(SexpError::StaleAllocation)
        ));
        assert!(value.node_factory().is_err());
        drop(value);
        drop(instance);
        assert!(!token.is_live());
    }
}
