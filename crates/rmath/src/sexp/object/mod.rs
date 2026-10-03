//! Rust object model for R SEXP values.
#![deny(unsafe_op_in_unsafe_fn)]
//!
//! This module is the safe, Rust-facing layer over raw R `SEXP` pointers. It
//! keeps R's object categories recognizable while adding lifetime tracking,
//! checked accessors, copied elements, owned projections, and
//! pairlist iteration. Header reads copy the node's `Copy` fields out once;
//! callers do not hold `&SexprecCore` across a later allocation.
//!
//! # Design
//!
//! [`Sexp<'a>`] wraps a raw `SEXP` pointer with a `PhantomData` marker
//! to track the lifetime of the underlying memory. Safe construction goes
//! through an owner such as [`RArena`](crate::sexp::memory::RArena) or
//! [`RSession`](crate::sexp::session::RSession), so the returned wrapper is
//! tied to the arena or session that owns the object. All element access is
//! bounds-checked. Legacy `Option<T>` accessors are kept for existing ported
//! C-shaped code, while new Rust code should prefer the `try_*` methods so
//! type mistakes and bounds errors stay explicit.
//!
//! # Type Predicates
//!
//! `Sexp` provides methods like [`is_vector`](Sexp::is_vector),
//! [`is_closure`](Sexp::is_closure), and [`is_environment`](Sexp::is_environment)
//! to inspect the type of an R object without unsafe code.
//!
//! # Element Access
//!
//! Use the `*_elt` methods (e.g., [`integer_elt`](Sexp::integer_elt),
//! [`real_elt`](Sexp::real_elt)) for bounds-checked access to individual
//! elements. For bulk access, use copied iterators (e.g.,
//! [`iter_integer`](Sexp::iter_integer)), [`copy_integer_into`](Sexp::copy_integer_into),
//! or [`to_owned_value`](Sexp::to_owned_value). Payload loans are private to this module.

mod error;
mod factory;
mod header;
mod kind;
mod mut_ref;
mod owned;
mod pairlist;
mod primitive;
#[cfg(test)]
mod safety_tests;
mod slots;
mod value;
mod vector;
#[cfg(test)]
mod view;

pub use error::{SexpError, SexpResult};
pub(crate) use factory::{NodeAllocator, NodeDomain, SessionNodeFactory};
pub(crate) use kind::{raw_is_atomic_vector, raw_is_vector};
pub use mut_ref::SexpMut;
pub(crate) use pairlist::PairlistBuilder;
pub use pairlist::PairlistIter;
pub use value::{SexpAttribute, SexpComplex, SexpMetadata, SexpValue};
#[cfg(test)]
use view::SexpView;

use super::ffi::{R_xlen_t, SEXP, SEXPTYPE, SexprecCore};
#[cfg(test)]
use super::globals::R_NilValue;
use super::heap::{HeapIdentity, NodeLink, ReferenceChild, ResolvedLink};
pub(crate) use header::{LeadingScalars, NodeBody, copy_leading_scalars};
use value::sexptype_name;

/// Provenance for a `Sexp` handle.
///
/// R object identity is still the raw pointer, but Rust-facing handles can
/// distinguish values wrapped from a checked owner from values crossing a
/// legacy raw boundary. This is intentionally lightweight: it gives safe APIs
/// a way to reject or audit unknown handles without changing R's pointer model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SexpOwner {
    /// Wrapped from raw translated code without owner validation.
    Unknown,
    /// Immutable process singleton such as `NULL`.
    Static,
    /// Object was validated against a concrete arena owner.
    Arena(usize),
    /// Object was validated against persistent storage owned by a session.
    Session(usize),
}

// ---------------------------------------------------------------------------
// Sexp — safe wrapper around SEXP
// ---------------------------------------------------------------------------

/// A safe, lifetime-tracked wrapper around an R SEXP pointer.
///
/// This type provides bounds-checked access to R objects while maintaining
/// FFI compatibility through [`as_raw`](Sexp::as_raw). Safe construction is
/// owner-scoped via `RArena` allocation methods such as
/// [`alloc_vector_sexp`](crate::sexp::memory::RArena::alloc_vector_sexp) or
/// typed `RSession` APIs. Raw pointer construction is kept crate-local, and
/// internal FFI boundary code that must cross the boundary explicitly uses a
/// crate-private unchecked wrapper.
/// The lifetime parameter `'a` ensures that the `Sexp` cannot outlive the
/// memory it points to.
///
/// # Examples
///
/// ```text
/// use crate::sexp::{Sexp, SEXPTYPE};
/// use crate::sexp::memory::RArena;
///
/// let mut arena = RArena::new();
/// let sexp = arena
///     .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
///     .expect("arena allocation failed");
/// assert!(sexp.is_vector()); // shared reads never move the handle
/// assert_eq!(sexp.len(), 3);
/// ```
///
/// # Pointer Equality
///
/// Checked handles compare allocation identities. A reclaimed slot reused by
/// another object is a different identity, even at the same physical address.
/// Immutable and unchecked views retain their separate pointer identity.
///
/// # Intentionally Not `Copy`
///
/// Handles move by default; cloning explicitly creates another raw alias.
/// Neither moving nor cloning establishes exclusivity over the R object.
/// Checked mutation uses [`SexpMut::try_from_checked`]. Legacy raw acquisition
/// and payload loans are unsafe; their callers must exclude borrowed payload
/// references for the mutation window.
///
/// ```text
/// use crate::sexp::{Sexp, SEXPTYPE};
/// use crate::sexp::memory::RArena;
///
/// let mut arena = RArena::new();
/// let sexp = arena
///     .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
///     .expect("arena allocation failed");
/// let alias = sexp; // moves the handle into `alias`
/// let _ = sexp.len(); // ERROR: use-after-move; clone explicitly instead
/// ```
#[derive(Debug)]
pub struct Sexp<'a> {
    ptr: SEXP,
    owner: SexpOwner,
    node: Option<crate::sexp::heap::CheckedNode>,
    pub(crate) runtime_owner: Option<crate::sexp::owner::WeakOwner>,
    pub(crate) session_owner_ptr: Option<std::ptr::NonNull<crate::sexp::instance::RInstance>>,
    root: Option<std::rc::Rc<crate::sexp::heap::NodeRootLease>>,
    singleton: Option<crate::sexp::globals::SingletonLease>,
    singletons: Option<crate::sexp::globals::SingletonPoolLease>,
    _marker: std::marker::PhantomData<&'a SexprecCore>,
}

/// Duplicate this handle; the clone aliases the same R object, it does not
/// deep-copy it.
///
/// A cloned [`Sexp`] is a second lightweight handle (same raw `SEXP`
/// pointer, same [`SexpOwner`] token) over identical R memory. Cloning is
/// cheap and never deep-copies R's heap. A clone is an alias, not an
/// independent object or a uniqueness proof.
impl Clone for Sexp<'_> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            ptr: self.ptr,
            owner: self.owner,
            node: self.node.clone(),
            runtime_owner: self.runtime_owner.clone(),
            session_owner_ptr: self.session_owner_ptr,
            root: self.root.clone(),
            singleton: self.singleton.clone(),
            singletons: self.singletons.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

/// Shared-borrow handle to an R object.
///
/// Alias for [`Sexp`]: a shared, reborrowable read handle with no mutation
/// surface. Reads (`typeof_`, `len`, predicates, `*_elt` / `try_*_elt`
/// element reads and child-handle accessors) copy values safely. Payload
/// loans stay inside the object implementation; in-place mutation goes through
/// [`SexpMut`](crate::sexp::object::SexpMut) (`try_from_checked` -> `try_set_*` ->
/// `freeze()`).
pub type SexpRef<'a> = Sexp<'a>;

// ---------------------------------------------------------------------------
// Compile-time guard — Sexp must never be Copy
// Stable Rust has no negative bounds, so the guard exploits method
// resolution: `SexpCopyGuardIfNotCopy` applies to every type, while
// `SexpCopyGuardIfCopy` applies only to `Copy` types. Today only the
// first candidate exists and the call below resolves; the moment
// someone adds `impl Copy for Sexp`, both traits become applicable and
// the build fails with an ambiguity error (E0034) instead of silently
// changing handle move semantics.
#[allow(dead_code)]
trait SexpCopyGuardIfNotCopy {
    fn sexp_copy_guard(&self) {}
}

#[allow(dead_code)]
trait SexpCopyGuardIfCopy {
    fn sexp_copy_guard(&self);
}

impl<T> SexpCopyGuardIfNotCopy for T {}

impl<T: Copy> SexpCopyGuardIfCopy for T {
    fn sexp_copy_guard(&self) {}
}

const _: () = {
    #[allow(dead_code)]
    fn sexp_is_not_copy(sexp: &Sexp<'_>) {
        sexp.sexp_copy_guard();
    }
};

impl<'a> Sexp<'a> {
    /// Return R's immutable `NULL` singleton as an owner-independent handle.
    #[inline]
    pub fn nil() -> Sexp<'static> {
        let pool = crate::sexp::globals::immutable_singleton_pool();
        Sexp::from_singleton(pool.nil(), pool)
    }

    /// Retain NULL from this value's original singleton bank, including after
    /// the runtime closes or the ambient bank rotates.
    pub(crate) fn original_singleton_nil(&self) -> Option<Sexp<'static>> {
        let pool = self.singletons.as_ref()?.clone();
        Some(Sexp::from_singleton(pool.nil(), pool))
    }

    /// Create a `Sexp` from a raw SEXP pointer for internal boundary code.
    ///
    /// Returns `None` if the pointer is null or visibly invalid. Public safe
    /// code should use owner-scoped wrapping through `RArena::sexp` or
    /// `RSession::sexp` instead.
    ///
    /// # Safety
    /// Non-null aligned pointers must name live, initialized R objects whose
    /// entire reachable graph outlives `'a`. The caller must retain GC roots
    /// across every allocating call; alignment checks do not establish liveness.
    #[inline]
    pub(crate) unsafe fn from_raw(ptr: SEXP) -> Option<Self> {
        unsafe { Self::try_from_raw(ptr) }.ok()
    }

    /// Create a `Sexp` from a raw SEXP pointer for internal boundary code.
    ///
    /// Unlike [`from_raw`](Self::from_raw), this reports why wrapping failed.
    ///
    /// # Safety
    /// The liveness, initialization, graph and rooting requirements of
    /// [`from_raw`](Self::from_raw) apply.
    #[inline]
    pub(crate) unsafe fn try_from_raw(ptr: SEXP) -> SexpResult<Self> {
        if ptr.is_null() {
            Err(SexpError::NullPointer)
        } else if (ptr as usize) % std::mem::align_of::<SexprecCore>() != 0 {
            Err(SexpError::MisalignedPointer {
                address: ptr as usize,
            })
        } else if let Some(singleton) = crate::sexp::globals::immutable_singleton_lease(ptr) {
            Ok(Self::from_singleton_lease(singleton, None))
        } else {
            let (ptr, node) =
                crate::sexp::memory::checked_projection(ptr).ok_or(SexpError::UnownedPointer {
                    address: ptr.addr(),
                })?;
            let heap = node.heap_identity();
            let core = heap
                .node_snapshot(&node)
                .ok_or(SexpError::StaleAllocation)?;
            if !core.has_valid_shape() {
                return Err(SexpError::UnownedPointer {
                    address: ptr.addr(),
                });
            }
            if let NodeBody::Vector(vector) = core.data {
                let length =
                    usize::try_from(vector.length).map_err(|_| SexpError::MissingData {
                        sexptype: core.sxpinfo.type_of(),
                    })?;
                if !core.payload.is_empty() && heap.payload_lease(&node).is_none()
                    || core.payload.is_empty() && length > 0 && !core.sxpinfo.alt()
                {
                    return Err(SexpError::MissingData {
                        sexptype: core.sxpinfo.type_of(),
                    });
                }
            }
            Ok(Sexp {
                ptr,
                owner: SexpOwner::Unknown,
                node: Some(node),
                runtime_owner: None,
                session_owner_ptr: None,
                root: None,
                singleton: None,
                singletons: None,
                _marker: std::marker::PhantomData,
            })
        }
    }

    /// Wrap a pointer that has already been validated against `arena`.
    #[inline]
    pub(crate) fn from_arena_raw<'arena>(
        ptr: SEXP,
        arena: &'arena crate::sexp::memory::RArena,
    ) -> SexpResult<Sexp<'arena>> {
        if !arena.contains(ptr) {
            return Err(SexpError::UnownedPointer {
                address: ptr as usize,
            });
        }
        let token = arena.node_token(ptr).ok_or(SexpError::UnownedPointer {
            address: ptr as usize,
        })?;
        let ptr = arena
            .resolve_node(token.id())
            .ok_or(SexpError::UnownedPointer {
                address: ptr as usize,
            })?;
        let mut sexp = unsafe { Sexp::try_from_raw(ptr) }?;
        sexp.root = Some(token.root_lease().ok_or(SexpError::RootUnavailable)?);
        sexp.singletons = Some(crate::sexp::globals::immutable_singleton_pool());
        sexp.node = Some(token);
        sexp.owner = SexpOwner::Arena(Self::arena_owner_token(arena));
        Ok(sexp)
    }

    /// Validate and root a pointer against a lifetime-bound owner capability.
    #[inline]
    pub(crate) fn from_owner_raw<'session>(
        ptr: SEXP,
        owner: crate::sexp::owner::OwnerToken<'session>,
    ) -> SexpResult<Sexp<'session>> {
        SessionNodeFactory::new(owner).wrap(ptr)
    }

    /// Create a `Sexp` from a raw pointer without null checking.
    ///
    /// # Safety
    ///
    /// The pointer must be non-null and point to a valid `SexprecCore`
    /// that lives at least as long as `'a`.
    #[inline]
    pub(crate) const unsafe fn from_raw_unchecked(ptr: SEXP) -> Self {
        Sexp {
            ptr,
            owner: SexpOwner::Unknown,
            node: None,
            runtime_owner: None,
            session_owner_ptr: None,
            root: None,
            singleton: None,
            singletons: None,
            _marker: std::marker::PhantomData,
        }
    }

    /// Create a `Sexp` from a known immutable singleton.
    #[inline]
    pub(crate) unsafe fn from_static_raw_unchecked(ptr: SEXP) -> Self {
        let pool = crate::sexp::globals::immutable_singleton_pool();
        let singleton = pool.lease(ptr).expect("known immutable singleton");
        Self::from_singleton(singleton, pool)
    }

    fn from_singleton(
        singleton: crate::sexp::globals::SingletonLease,
        pool: crate::sexp::globals::SingletonPoolLease,
    ) -> Self {
        Self::from_singleton_lease(singleton, Some(pool))
    }

    fn from_singleton_lease(
        singleton: crate::sexp::globals::SingletonLease,
        pool: Option<crate::sexp::globals::SingletonPoolLease>,
    ) -> Self {
        Sexp {
            ptr: singleton.projection(),
            owner: SexpOwner::Static,
            node: None,
            runtime_owner: None,
            session_owner_ptr: None,
            root: None,
            singleton: Some(singleton),
            singletons: pool,
            _marker: std::marker::PhantomData,
        }
    }

    pub(crate) fn singleton_snapshot(&self, ptr: SEXP) -> Option<SexprecCore> {
        if let Some(owner) = &self.singleton {
            if owner.projection() == ptr {
                return Some(owner.snapshot());
            }
        }
        self.singletons.as_ref()?.snapshot(ptr)
    }

    pub(crate) fn singleton_projection(&self, ptr: SEXP) -> Option<SEXP> {
        self.singletons.as_ref()?.canonical_projection(ptr)
    }

    /// Whether this value is the retained `NA_character_` sentinel.
    #[inline]
    pub fn is_na_string(&self) -> bool {
        if let Some(singleton) = &self.singleton {
            return singleton.is_na_string();
        }
        if let Some(pool) = &self.singletons {
            return self.ptr == pool.na_string_projection();
        }
        // Unsafe raw fixtures retain their ambient bank by caller contract.
        // Inspection never initializes or recreates a bank during teardown.
        self.owner == SexpOwner::Unknown
            && crate::sexp::globals::immutable_na_string_projection() == Some(self.ptr)
    }

    /// Borrow the raw projection while this handle retains the allocation.
    #[inline]
    pub fn as_raw(&self) -> SEXP {
        self.ensure_live()
            .expect("SEXP allocation has been reclaimed");
        self.ptr
    }

    /// Move the original physical leases into a lifetime-independent value.
    /// Interpreter work retains only revocable weak authority, avoiding a
    /// value-to-runtime strong cycle. Unchecked and borrowed arena views cannot
    /// manufacture ownership by dropping their lifetime marker.
    pub fn into_owned(self) -> SexpResult<Sexp<'static>> {
        self.ensure_live()?;
        match self.owner {
            SexpOwner::Static if self.singleton.is_some() => {}
            SexpOwner::Session(_) if self.root.is_some() && self.runtime_owner.is_some() => {}
            _ => return Err(SexpError::RootUnavailable),
        }
        Ok(Sexp {
            ptr: self.ptr,
            owner: self.owner,
            node: self.node,
            runtime_owner: self.runtime_owner,
            session_owner_ptr: None,
            root: self.root,
            singleton: self.singleton,
            singletons: self.singletons,
            _marker: std::marker::PhantomData,
        })
    }

    pub(crate) fn pin_runtime(&self) -> SexpResult<Option<crate::sexp::owner::OwnerPin>> {
        self.runtime_owner
            .as_ref()
            .map(|owner| owner.pin())
            .transpose()
    }

    /// Capture this value's original domain, never an ambient replacement.
    pub(crate) fn node_factory(&self) -> SexpResult<SessionNodeFactory<'a>> {
        self.ensure_live()?;
        if let Some(owner) = &self.runtime_owner {
            return owner.node_factory();
        }
        let owner = self.session_owner_ptr.ok_or(SexpError::RootUnavailable)?;
        if self.node.is_none() || self.owner != SexpOwner::Session(owner.as_ptr().addr()) {
            return Err(SexpError::RootUnavailable);
        }
        // Borrow-bound standalone translated owner; never accepted by into_owned.
        let owner = unsafe { crate::sexp::owner::OwnerToken::from_raw(owner.as_ptr()) };
        let factory = SessionNodeFactory::new(owner);
        factory.wrap(self.ptr)?;
        Ok(factory)
    }

    /// Check the allocation generation without dereferencing interpreter memory.
    /// A reclaimed allocation never becomes valid when its address is reused.
    pub fn is_live(&self) -> bool {
        self.node.as_ref().is_none_or(|node| node.is_live())
    }

    fn ensure_live(&self) -> SexpResult<()> {
        if self.is_live() {
            Ok(())
        } else {
            Err(SexpError::StaleAllocation)
        }
    }

    /// Borrow the original checked allocation without recapturing its address.
    /// Immutable sentinels and unchecked native views have no allocation token.
    pub(crate) fn allocation(&self) -> SexpResult<&crate::sexp::heap::CheckedNode> {
        self.ensure_live()?;
        self.node.as_ref().ok_or(SexpError::UnownedPointer {
            address: self.ptr.addr(),
        })
    }

    /// Return the owner provenance attached to this handle.
    #[inline]
    pub fn owner(&self) -> SexpOwner {
        self.owner
    }

    /// Return true when this handle was created through a checked owner.
    #[inline]
    pub fn is_owner_scoped(&self) -> bool {
        !matches!(self.owner, SexpOwner::Unknown)
    }

    /// Return true when this handle is scoped to `arena` and the arena still
    /// contains the raw pointer.
    #[inline]
    pub fn belongs_to_arena(&self, arena: &crate::sexp::memory::RArena) -> bool {
        self.owner == SexpOwner::Arena(Self::arena_owner_token(arena))
            && self.is_live()
            && arena.contains(self.ptr)
    }

    /// Return true when this handle is scoped to persistent storage owned by
    /// `instance` and the instance still owns the raw pointer.
    #[inline]
    pub fn belongs_to_session(&self, instance: &crate::sexp::instance::RInstance) -> bool {
        self.owner == SexpOwner::Session(Self::session_owner_token(instance))
            && self.is_live()
            && instance.owns_sexp(self.ptr)
    }

    #[inline]
    fn arena_owner_token(arena: &crate::sexp::memory::RArena) -> usize {
        arena as *const crate::sexp::memory::RArena as usize
    }

    #[inline]
    fn session_owner_token(instance: &crate::sexp::instance::RInstance) -> usize {
        instance as *const crate::sexp::instance::RInstance as usize
    }

    #[inline]
    fn typed_data<T>(&self, expected: SEXPTYPE) -> Option<*const T> {
        self.try_typed_data::<T>(expected, sexptype_name(expected))
            .ok()
    }

    #[inline]
    /// Expand a compact sequence before a pointer or element write.
    ///
    /// Single-element readers use the formula instead and never call this.
    fn materialize_compact_payload(&self) -> SexpResult<()> {
        self.ensure_live()?;
        let header = self.header();
        if header.sxpinfo.alt() && header.payload.is_empty() {
            let pin = self.pin_runtime()?;
            let pointer = pin
                .as_ref()
                .map(|owner| owner.as_ptr())
                .or_else(|| self.session_owner_ptr.map(|owner| owner.as_ptr()));
            if let Some(owner) = pointer {
                // The owner is retained by this handle. Each callback runs after
                // header copying, with no instance or R payload borrow alive.
                unsafe {
                    crate::sexp::session::with_instance_active(owner, || {
                        if super::altrep::materialize_raw(self.ptr)? {
                            return Ok(());
                        }
                        super::altseq::materialize(self.ptr);
                        Ok(())
                    })
                }?;
                if let Some(pin) = &pin {
                    pin.require_live()?;
                }
            } else {
                unsafe {
                    if !super::altrep::materialize_raw(self.ptr)? {
                        super::altseq::materialize(self.ptr);
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn materialize_compact_payload_for_raw(&self) -> SexpResult<()> {
        self.materialize_compact_payload()
    }

    /// Resolve the actual current typed allocation after any provider callback.
    fn try_payload_lease(
        &self,
        expected: SEXPTYPE,
        expected_name: &'static str,
    ) -> SexpResult<crate::sexp::payload::PayloadLease> {
        self.expect_type(expected, expected_name)?;
        self.materialize_compact_payload()?;
        self.expect_type(expected, expected_name)?;
        self.header()
            .payload_lease()
            .cloned()
            .ok_or(SexpError::MissingData { sexptype: expected })
    }

    fn try_typed_data<T>(
        &self,
        expected: SEXPTYPE,
        expected_name: &'static str,
    ) -> SexpResult<*const T> {
        let lease = self.try_payload_lease(expected, expected_name)?;
        Ok(lease.native_projection().cast::<T>())
    }

    #[inline]
    fn expect_type(&self, expected: SEXPTYPE, expected_name: &'static str) -> SexpResult<()> {
        self.ensure_live()?;
        if self.typeof_() != expected {
            Err(SexpError::TypeMismatch {
                expected: expected_name,
                actual: self.typeof_(),
            })
        } else {
            Ok(())
        }
    }

    #[inline]
    fn expect_any_type(
        &self,
        expected_name: &'static str,
        expected: &[SEXPTYPE],
    ) -> SexpResult<()> {
        self.ensure_live()?;
        let actual = self.typeof_();
        if expected.contains(&actual) {
            Ok(())
        } else {
            Err(SexpError::TypeMismatch {
                expected: expected_name,
                actual,
            })
        }
    }

    #[inline]
    unsafe fn typed_slice<T>(&self, expected: SEXPTYPE) -> Option<&'_ [T]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_typed_slice::<T>(expected, sexptype_name(expected))
        }
        .ok()
    }

    #[inline]
    unsafe fn try_typed_slice<T>(
        &self,
        expected: SEXPTYPE,
        expected_name: &'static str,
    ) -> SexpResult<&'_ [T]> {
        self.expect_type(expected, expected_name)?;
        if self.len() == 0 {
            return Ok(&[]);
        }
        let lease = self.try_payload_lease(expected, expected_name)?;
        let len = usize::try_from(self.len())
            .map_err(|_| SexpError::MissingData { sexptype: expected })?;
        if len > lease.capacity() {
            return Err(SexpError::MissingData { sexptype: expected });
        }
        // The original parent's attachment retains this projection. The
        // caller excludes all replacement/mutation for the returned borrow.
        Ok(unsafe { std::slice::from_raw_parts(lease.native_projection().cast::<T>(), len) })
    }

    #[inline]
    fn try_index(&self, i: R_xlen_t) -> SexpResult<usize> {
        self.ensure_live()?;
        let len = self.len();
        if i >= 0 && i < len {
            Ok(i as usize)
        } else {
            Err(SexpError::OutOfBounds { index: i, len })
        }
    }

    fn reference_node(&self) -> SexpResult<crate::sexp::heap::CheckedNode> {
        self.ensure_live()?;
        if let Some(node) = &self.node {
            return Ok(node.clone());
        }
        crate::sexp::memory::checked_projection(self.ptr)
            .map(|(_, node)| node)
            .ok_or(SexpError::UnownedPointer {
                address: self.ptr.addr(),
            })
    }

    fn reference_elt(&self, index: usize) -> SexpResult<NodeLink> {
        let node = self.reference_node()?;
        node.heap_identity()
            .reference_link_elt(&node, index)
            .ok_or(SexpError::MissingData {
                sexptype: self.typeof_(),
            })
    }

    fn set_reference_elt(&self, index: usize, child: &Sexp<'_>) -> SexpResult<()> {
        use crate::sexp::heap::ReferenceChild;
        let node = self.reference_node()?;
        let heap = node.heap_identity();
        let singleton;
        let child_node;
        let value = if let Some(owner) = &child.singleton {
            ReferenceChild::Singleton(owner)
        } else if let Some(owner) = &child.node {
            ReferenceChild::Node(owner)
        } else if let Some(owner) = self
            .singletons
            .as_ref()
            .and_then(|pool| pool.lease(child.ptr))
            .or_else(|| heap.retained_singleton(child.ptr))
            .or_else(|| {
                (self.owner == SexpOwner::Unknown)
                    .then(|| crate::sexp::globals::immutable_singleton_lease(child.ptr))
                    .flatten()
            })
        {
            singleton = owner;
            ReferenceChild::Singleton(&singleton)
        } else {
            child_node = child.reference_node()?;
            ReferenceChild::Node(&child_node)
        };
        heap.set_reference_elt(&node, index, value)
            .ok_or(SexpError::MissingData {
                sexptype: self.typeof_(),
            })
    }

    #[inline]
    fn valid_index(&self, i: R_xlen_t) -> bool {
        self.try_index(i).is_ok()
    }

    fn check_child_owner(&self, child: &Sexp<'_>) -> SexpResult<()> {
        self.ensure_live()?;
        child.ensure_live()?;
        let ptr = child.clone().as_raw();
        let valid = if child.singleton.is_some() && self.node.is_some() {
            // The typed edge setter retains this actual lease, including a
            // bank that closed before the parent was allocated.
            true
        } else if self.singleton_projection(ptr).is_some() {
            true
        } else if self.owner == SexpOwner::Unknown {
            // Unsafe raw factories establish graph ownership in their contract.
            true
        } else if let (Some(parent), Some(child)) = (&self.node, &child.node) {
            parent.same_heap(child)
        } else {
            false
        };
        if valid {
            Ok(())
        } else {
            Err(SexpError::UnownedPointer {
                address: ptr as usize,
            })
        }
    }

    /// Retain a new graph edge in the checked handle's original session.
    /// Standalone arenas have no generational collector or ambient owner.
    fn remember_child(&self, child: &Sexp<'_>) -> SexpResult<()> {
        let pin = self.pin_runtime()?;
        let pointer = pin
            .as_ref()
            .map(|owner| owner.as_ptr())
            .or_else(|| self.session_owner_ptr.map(|owner| owner.as_ptr()));
        if let Some(owner) = pointer {
            // SAFETY: the operation pins the owner and both live nodes;
            // check_child_owner precedes this strictly-local state update.
            if !unsafe { crate::sexp::gengc::write_barrier_in(owner, self.ptr, child.ptr) } {
                return Err(SexpError::AllocationFailed {
                    object: "GC write barrier",
                });
            }
        } else if self.owner == SexpOwner::Unknown {
            // Legacy raw mutation requires the caller's active owner/rooting
            // contract. No Rust payload reference survives this local call.
            crate::sexp::gengc::write_barrier(self.ptr, child.ptr);
        }
        Ok(())
    }

    /// Capture this handle's original capability as an edge in `heap`.
    /// Singleton links retain the actual lease, including an earlier bank.
    pub(crate) fn link_in(&self, heap: &HeapIdentity) -> SexpResult<NodeLink> {
        self.ensure_live()?;
        let node;
        let singleton;
        let child = if let Some(value) = &self.singleton {
            ReferenceChild::Singleton(value)
        } else if let Some(value) = &self.node {
            ReferenceChild::Node(value)
        } else if let Some(value) = self
            .singletons
            .as_ref()
            .and_then(|pool| pool.lease(self.ptr))
            .or_else(|| heap.retained_singleton(self.ptr))
            .or_else(|| crate::sexp::globals::immutable_singleton_lease(self.ptr))
        {
            singleton = value;
            ReferenceChild::Singleton(&singleton)
        } else {
            node = self.reference_node()?;
            ReferenceChild::Node(&node)
        };
        heap.capture_child(child).ok_or(SexpError::UnownedPointer {
            address: self.ptr.addr(),
        })
    }

    fn optional_child(&self, link: NodeLink) -> Option<Sexp<'a>> {
        if link.is_null() {
            None
        } else {
            self.checked_child(link).ok()
        }
    }

    #[inline]
    pub(crate) fn checked_child(&self, link: NodeLink) -> SexpResult<Sexp<'a>> {
        self.ensure_live()?;
        if link.is_null() {
            return Ok(if let Some(pool) = &self.singletons {
                Self::from_singleton(pool.nil(), pool.clone())
            } else {
                Sexp::nil()
            });
        }
        let parent = self.reference_node()?;
        match parent
            .heap_identity()
            .resolve_link(link)
            .ok_or(SexpError::StaleAllocation)?
        {
            ResolvedLink::Null => unreachable!("non-null link resolved as null"),
            ResolvedLink::Singleton(singleton) => Ok(Self::from_singleton_lease(
                singleton,
                self.singletons.clone(),
            )),
            ResolvedLink::Node {
                projection,
                allocation,
            } => {
                let root = allocation.root_lease().ok_or(SexpError::RootUnavailable)?;
                Ok(Self {
                    ptr: projection,
                    owner: self.owner,
                    node: Some(allocation),
                    runtime_owner: self.runtime_owner.clone(),
                    session_owner_ptr: self.session_owner_ptr,
                    root: Some(root),
                    singleton: None,
                    singletons: self.singletons.clone(),
                    _marker: std::marker::PhantomData,
                })
            }
        }
    }

    /// Convert to a boolean value.
    ///
    /// Returns true for non-NULL values, or the actual boolean/logical value
    /// for LGLSXP/INTSXP types.
    pub fn to_bool(&self) -> bool {
        self.try_to_bool().unwrap_or(true)
    }

    /// Convert to a boolean value with typed error reporting.
    ///
    /// `NULL` is false. Numeric and logical vectors use their first element;
    /// empty vectors report [`SexpError::OutOfBounds`]. Other values are true,
    /// matching R's broad truthiness at this wrapper layer.
    pub fn try_to_bool(&self) -> SexpResult<bool> {
        if self.is_nil() {
            return Ok(false);
        }
        match self.typeof_() {
            SEXPTYPE::LGLSXP => self.try_logical_elt(0).map(|value| value != 0),
            SEXPTYPE::INTSXP => self.try_integer_elt(0).map(|value| value != 0),
            SEXPTYPE::REALSXP => self.try_real_elt(0).map(|value| value != 0.0),
            _ => Ok(true),
        }
    }

    /// Convert to an f64 value.
    ///
    /// Returns 0.0 for non-numeric types.
    pub fn as_f64(&self) -> f64 {
        self.try_as_f64().unwrap_or(0.0)
    }

    /// Convert the first logical/integer/real element to `f64`.
    pub fn try_as_f64(&self) -> SexpResult<f64> {
        match self.typeof_() {
            SEXPTYPE::REALSXP => self.try_real_elt(0),
            SEXPTYPE::INTSXP => self.try_integer_elt(0).map(|value| value as f64),
            SEXPTYPE::LGLSXP => self.try_logical_elt(0).map(|value| value as f64),
            _ => Err(SexpError::TypeMismatch {
                expected: "logical, integer, or real vector",
                actual: self.typeof_(),
            }),
        }
    }

    // --- Pairlist iteration ---

    /// Get the CAR (value) of a pairlist element.
    ///
    /// Returns `None` if this is not a pairlist or the CAR is null.
    #[inline]
    pub fn car(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::List(cell) => self.optional_child(cell.carval),
            _ => None,
        }
    }

    /// Get the CAR with typed error reporting.
    #[inline]
    pub fn try_car(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::List(cell) => self.checked_child(cell.carval),
            _ => self.pairlist_mismatch(),
        }
    }

    /// Get the CDR (next cell) of a pairlist element.
    ///
    /// Returns `None` if this is not a pairlist or the CDR is null.
    #[inline]
    pub fn cdr(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::List(cell) => self.optional_child(cell.cdrval),
            _ => None,
        }
    }

    /// Get the CDR with typed error reporting.
    #[inline]
    pub fn try_cdr(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::List(cell) => self.checked_child(cell.cdrval),
            _ => self.pairlist_mismatch(),
        }
    }

    /// Get the TAG (name) of a pairlist element.
    ///
    /// Returns `None` if this is not a pairlist or the TAG is null.
    #[inline]
    pub fn tag(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::List(cell) => self.optional_child(cell.tagval),
            _ => None,
        }
    }

    /// Get the TAG with typed error reporting.
    #[inline]
    pub fn try_tag(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::List(cell) => self.checked_child(cell.tagval),
            _ => self.pairlist_mismatch(),
        }
    }

    fn pairlist_mismatch(&self) -> SexpResult<Sexp<'a>> {
        Err(SexpError::TypeMismatch {
            expected: "pairlist or language object",
            actual: self.typeof_(),
        })
    }

    /// Return the next pairlist cell, or `None` at the end of the chain.
    #[inline]
    pub(crate) fn try_next_pairlist_cell(&self) -> SexpResult<Option<Sexp<'a>>> {
        let next = self.try_cdr()?;
        if next.is_nil() {
            Ok(None)
        } else {
            Ok(Some(next))
        }
    }

    /// Return the value at the `index`th pairlist cell.
    pub(crate) fn try_pairlist_arg(mut self, index: usize) -> SexpResult<Sexp<'a>> {
        for _ in 0..index {
            if self.clone().is_nil() {
                return Err(SexpError::MissingArgument { index });
            }
            self = self.try_cdr()?;
        }

        if self.clone().is_nil() {
            Err(SexpError::MissingArgument { index })
        } else {
            self.try_car()
        }
    }

    /// Return the value at the `index`th pairlist cell, or `None` when absent.
    pub(crate) fn try_optional_pairlist_arg(
        mut self,
        index: usize,
    ) -> SexpResult<Option<Sexp<'a>>> {
        for _ in 0..index {
            if self.clone().is_nil() {
                return Ok(None);
            }
            self = self.try_cdr()?;
        }

        if self.clone().is_nil() {
            Ok(None)
        } else {
            self.try_car().map(Some)
        }
    }

    /// Compare a pairlist cell's symbol tag to a byte name.
    ///
    /// Untagged cells and non-symbol tags are valid R list cells; they simply
    /// do not match.
    pub(crate) fn try_tag_name_eq(&self, name: &[u8]) -> SexpResult<bool> {
        let tag = self.try_tag()?;
        if tag.is_nil() || tag.typeof_() != SEXPTYPE::SYMSXP {
            return Ok(false);
        }

        let printname = tag.try_printname()?;
        printname.try_char_eq(name)
    }
}

// Note: Index<usize> is intentionally NOT implemented for Sexp.
// The Index trait requires returning &Self::Output, but Sexp elements
// are created on-the-fly from raw pointers. Use vector_elt() and
// string_elt() for bounds-checked element access instead.

// ---------------------------------------------------------------------------
// PartialEq/Eq/Hash — allocation identity
// ---------------------------------------------------------------------------

impl PartialEq for Sexp<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (&self.node, &other.node) {
            (Some(first), Some(second)) => first == second,
            (None, None) => self.ptr == other.ptr,
            _ => false,
        }
    }
}

impl Eq for Sexp<'_> {}

impl std::hash::Hash for Sexp<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if let Some(node) = &self.node {
            1_u8.hash(state);
            node.id().hash(state);
        } else {
            0_u8.hash(state);
            (self.ptr as usize).hash(state);
        }
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl std::fmt::Display for Sexp<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let t = self.clone().typeof_();
        write!(f, "Sexp({:?}, len={})", t.0, self.clone().len())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(deprecated)] // translated tests exercise the Sexp compat setters
mod tests {
    use std::ptr;

    use super::*;
    use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, NA_REAL, Rcomplex};
    use crate::sexp::globals::R_NaString;
    use crate::sexp::memory::RArena;

    fn some<T>(opt: Option<T>) -> T {
        opt.unwrap_or_else(|| panic!("unexpected None in test"))
    }

    #[test]
    fn test_sexp_from_raw_null() {
        assert!(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(ptr::null_mut()) }.is_none());
    }

    #[test]
    fn test_sexp_from_raw_misaligned_pointer() {
        assert!(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(std::ptr::without_provenance_mut(0x1)) }.is_none());
    }

    #[test]
    fn test_raw_wrapped_sexp_has_unknown_owner() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert_eq!(sexp.clone().owner(), SexpOwner::Unknown);
        assert!(!sexp.is_owner_scoped());
    }

    #[test]
    fn test_nil_has_static_owner() {
        let nil = Sexp::nil();
        assert_eq!(nil.clone().owner(), SexpOwner::Static);
        assert!(nil.is_owner_scoped());
    }

    #[test]
    fn test_arena_wrapped_sexp_has_arena_owner() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        let sexp = arena.sexp(ptr).expect("arena-owned pointer should wrap");
        assert!(matches!(sexp.clone().owner(), SexpOwner::Arena(_)));
        assert!(sexp.clone().is_owner_scoped());
        assert!(sexp.belongs_to_arena(&arena));
    }

    #[test]
    fn test_sexp_len_non_vector() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::SYMSXP);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert_eq!(sexp.clone().len(), 0);
        assert!(sexp.clone().is_empty());
        assert!(sexp.is_symbol());
    }

    #[test]
    fn test_sexp_len_vector() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 5);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert_eq!(sexp.clone().len(), 5);
        assert!(!sexp.clone().is_empty());
        assert!(sexp.is_vector());
    }

    #[test]
    fn test_sexp_bounds_check() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert!(sexp.clone().integer_elt(5).is_none());
        assert!(sexp.clone().integer_elt(-1).is_none());
        assert!(sexp.clone().integer_elt(0).is_some());
        assert!(sexp.integer_elt(2).is_some());
    }

    #[test]
    fn test_pairlist_iter() {
        let mut arena = RArena::new();
        let list = arena.alloc_list_chain(3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(list)
        });
        let items: Vec<_> = PairlistIter::new(sexp).collect();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_sexp_partial_eq() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp1 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let sexp2 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert_eq!(sexp1, sexp2);
    }

    #[test]
    fn test_sexp_display() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 5);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let s = format!("{}", sexp);
        assert!(s.contains("len=5"));
    }

    #[test]
    fn test_set_integer_elt() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(0, 42)
        });
        assert!(
            unsafe {
                /* SAFETY: fixture has no outstanding payload borrows. */
                sexp.clone().set_integer_elt(5, 99)
            } == false
        );
        assert_eq!(sexp.integer_elt(0), Some(42));
    }

    #[test]
    fn test_set_real_elt() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_real_elt(0, 3.14)
        });
        assert!(
            unsafe {
                /* SAFETY: fixture has no outstanding payload borrows. */
                sexp.clone().set_real_elt(5, 99.0)
            } == false
        );
        assert_eq!(sexp.real_elt(0), Some(3.14));
    }

    #[test]
    fn test_set_raw_elt() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_raw_elt(0, 0xFF)
        });
        assert!(
            unsafe {
                /* SAFETY: fixture has no outstanding payload borrows. */
                sexp.clone().set_raw_elt(5, 0xAA)
            } == false
        );
        assert_eq!(sexp.raw_elt(0), Some(0xFF));
    }

    #[test]
    fn test_as_integer_slice() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let slice = unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            sexp.as_integer_slice()
        };
        assert!(slice.is_some());
        assert_eq!(some(slice).len(), 3);
    }

    #[test]
    fn test_as_real_slice() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 4);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let slice = unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            sexp.as_real_slice()
        };
        assert!(slice.is_some());
        assert_eq!(some(slice).len(), 4);
    }

    #[test]
    fn test_as_raw_slice() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, 5);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let slice = unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            sexp.as_raw_slice()
        };
        assert!(slice.is_some());
        assert_eq!(some(slice).len(), 5);
    }

    #[test]
    fn test_iter_integer() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let items: Vec<_> = sexp.iter_integer().collect();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_iter_real() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 4);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let items: Vec<_> = sexp.iter_real().collect();
        assert_eq!(items.len(), 4);
    }

    #[test]
    fn test_iter_raw() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, 5);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let items: Vec<_> = sexp.iter_raw().collect();
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn test_sexp_equality() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 5);
        let a = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let b = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert_eq!(a, b);
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn test_sexp_hash() {
        use std::collections::HashSet;
        let mut arena = RArena::new();
        let p1 = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let p2 = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let a = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(p1)
        });
        let b = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(p2)
        });
        let mut set = HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&a));
        assert!(!set.contains(&b));
    }

    #[test]
    fn test_sexp_display_len10() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 10);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        let s = format!("{}", sexp);
        assert!(s.contains("len=10"));
    }

    #[test]
    fn test_sexp_mutation() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert_eq!(sexp.clone().integer_elt(0), Some(0));
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(0, 42)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(1, -7)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(2, 99)
        });
        assert_eq!(sexp.clone().integer_elt(0), Some(42));
        assert_eq!(sexp.clone().integer_elt(1), Some(-7));
        assert_eq!(sexp.clone().integer_elt(2), Some(99));
        assert!(!unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(5, 0)
        });
        assert!(sexp.integer_elt(5).is_none());
    }

    #[test]
    fn test_sexp_real_mutation() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_real_elt(0, 1.5)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_real_elt(1, 2.5)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_real_elt(2, 3.5)
        });
        assert_eq!(sexp.clone().real_elt(0), Some(1.5));
        assert_eq!(sexp.clone().real_elt(1), Some(2.5));
        assert_eq!(sexp.real_elt(2), Some(3.5));
    }

    #[test]
    fn test_sexp_slice_views() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 4);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(0, 10)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(1, 20)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(2, 30)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_integer_elt(3, 40)
        });
        let slice = some(unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            sexp.as_integer_slice()
        });
        assert_eq!(slice, &[10, 20, 30, 40]);
    }

    #[test]
    fn test_sexp_real_slice() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_real_elt(0, 1.1)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_real_elt(1, 2.2)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_real_elt(2, 3.3)
        });
        let slice = some(unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            sexp.as_real_slice()
        });
        assert!((slice[0] - 1.1).abs() < f64::EPSILON);
        assert!((slice[1] - 2.2).abs() < f64::EPSILON);
        assert!((slice[2] - 3.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_sexp_iterators() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 5);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        for i in 0..5 {
            unsafe {
                /* SAFETY: fixture has no outstanding payload borrows. */
                sexp.clone().set_integer_elt(i, (i * 10) as i32)
            };
        }
        let values: Vec<_> = sexp.iter_integer().collect();
        assert_eq!(values, vec![0, 10, 20, 30, 40]);
    }

    #[test]
    fn test_sexp_real_iterator() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 4);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        for i in 0..4 {
            unsafe {
                /* SAFETY: fixture has no outstanding payload borrows. */
                sexp.clone().set_real_elt(i, i as f64 * 0.5)
            };
        }
        let values: Vec<_> = sexp.iter_real().collect();
        assert!((values[0] - 0.0).abs() < f64::EPSILON);
        assert!((values[1] - 0.5).abs() < f64::EPSILON);
        assert!((values[2] - 1.0).abs() < f64::EPSILON);
        assert!((values[3] - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_sexp_raw_mutation() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, 4);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_raw_elt(0, 0xDE)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_raw_elt(1, 0xAD)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_raw_elt(2, 0xBE)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_raw_elt(3, 0xEF)
        });
        let slice = some(unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            sexp.as_raw_slice()
        });
        assert_eq!(slice, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_try_accessors_report_type_and_bounds_errors() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 2);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().try_set_integer_elt(0, 10)
        }
        .expect("set integer");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().try_set_integer_elt(1, 20)
        }
        .expect("set integer");

        assert_eq!(sexp.clone().try_integer_elt(1), Ok(20));
        assert!(matches!(
            sexp.clone().try_integer_elt(2),
            Err(SexpError::OutOfBounds { index: 2, len: 2 })
        ));
        assert!(matches!(
            sexp.try_real_elt(0),
            Err(SexpError::TypeMismatch { expected, .. }) if expected == "real vector"
        ));
    }

    #[test]
    fn test_sexp_view_exposes_typed_borrow() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 2);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().try_set_real_elt(0, 1.5)
        }
        .expect("set real");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().try_set_real_elt(1, 2.5)
        }
        .expect("set real");

        match unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            sexp.view()
        }
        .expect("view")
        {
            SexpView::Real(values) => assert_eq!(values, &[1.5, 2.5]),
            other => panic!("unexpected view: {other:?}"),
        }
    }

    #[test]
    fn test_to_owned_value_maps_atomic_na_values() {
        let mut arena = RArena::new();
        let logical = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::LGLSXP, 3))
        });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            logical.clone().try_set_logical_elt(0, 1)
        }
        .expect("set logical");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            logical.clone().try_set_logical_elt(1, 0)
        }
        .expect("set logical");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            logical.clone().try_set_logical_elt(2, NA_LOGICAL)
        }
        .expect("set logical");

        let integer = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::INTSXP, 2))
        });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            integer.clone().try_set_integer_elt(0, 10)
        }
        .expect("set integer");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            integer.clone().try_set_integer_elt(1, NA_INTEGER)
        }
        .expect("set integer");

        let real = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::REALSXP, 1))
        });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            real.clone().try_set_real_elt(0, NA_REAL)
        }
        .expect("set real");

        assert_eq!(
            logical.to_owned_value().expect("logical value"),
            SexpValue::LogicalVector(vec![Some(true), Some(false), None])
        );
        assert_eq!(
            integer.to_owned_value().expect("integer value"),
            SexpValue::IntegerVector(vec![Some(10), None])
        );
        assert_eq!(
            real.to_owned_value().expect("real value"),
            SexpValue::Real(None)
        );
    }

    #[test]
    fn test_to_owned_value_maps_strings_raw_complex_and_lists() {
        let mut arena = RArena::new();
        let strings = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::STRSXP, 2))
        });
        let hello = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_charsxp(b"hello"))
        });
        let na_string = some(unsafe { Sexp::from_raw(R_NaString()) });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            strings.clone().try_set_string_elt(0, hello)
        }
        .expect("set string");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            strings.clone().try_set_string_elt(1, na_string)
        }
        .expect("set string");
        assert_eq!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                strings.try_string_text_elt(0)
            }
            .expect("text"),
            Some("hello")
        );
        assert_eq!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                strings.try_string_text_elt(1)
            }
            .expect("NA text"),
            None
        );

        let raw = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::RAWSXP, 2))
        });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            raw.clone().try_set_raw_elt(0, 0x41)
        }
        .expect("set raw");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            raw.clone().try_set_raw_elt(1, 0x5a)
        }
        .expect("set raw");

        let complex = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::CPLXSXP, 2))
        });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            complex
                .clone()
                .try_set_complex_elt(0, Rcomplex { r: 1.0, i: -2.0 })
        }
        .expect("set complex");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            complex
                .clone()
                .try_set_complex_elt(1, Rcomplex { r: NA_REAL, i: 0.0 })
        }
        .expect("set complex");

        let list = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::VECSXP, 3))
        });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            list.clone().try_set_vector_elt(0, strings)
        }
        .expect("set list");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            list.clone().try_set_vector_elt(1, raw)
        }
        .expect("set list");
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            list.clone().try_set_vector_elt(2, complex)
        }
        .expect("set list");

        assert_eq!(
            list.to_owned_value().expect("list value"),
            SexpValue::List(vec![
                SexpValue::StringVector(vec![Some("hello".to_string()), None]),
                SexpValue::RawVector(vec![0x41, 0x5a]),
                SexpValue::ComplexVector(vec![
                    Some(SexpComplex {
                        real: 1.0,
                        imaginary: -2.0,
                    }),
                    None,
                ]),
            ])
        );
    }

    #[test]
    fn test_to_owned_value_preserves_core_metadata() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let vector = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 2)))
            .unwrap();
        let mut vector = SexpMut::try_from_checked(vector).unwrap();
        vector.try_set_integer_elt(0, 10).unwrap();
        vector.try_set_integer_elt(1, 20).unwrap();
        let vector = vector.freeze();
        let names = factory.strings(&["a", "b"]).unwrap();
        let dim = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 2)))
            .unwrap();
        let mut dim = SexpMut::try_from_checked(dim).unwrap();
        dim.try_set_integer_elt(0, 1).unwrap();
        dim.try_set_integer_elt(1, 2).unwrap();
        let dim = dim.freeze();
        let class = factory.strings(&["matrix"]).unwrap();
        let tags = session.with_active(|| unsafe {
            [c"names", c"dim", c"class"].map(|name| {
                factory
                    .wrap(crate::sexp::symbol::Rf_install(name.as_ptr()))
                    .unwrap()
            })
        });
        let mut attributes = pairlist::PairlistBuilder::from_factory(factory);
        for (value, tag) in [names, dim, class].into_iter().zip(tags) {
            attributes.push(value, Some(tag)).unwrap();
        }
        let attributes = attributes.finish().unwrap();
        session.with_active(|| unsafe {
            crate::sexp::accessors::SET_ATTRIB(vector.as_raw(), attributes.as_raw());
        });

        let value = vector.clone().to_owned_value().expect("owned value");
        let SexpValue::Attributed { value, metadata } = value else {
            panic!("expected attributed value");
        };

        assert_eq!(*value, SexpValue::IntegerVector(vec![Some(10), Some(20)]));
        assert_eq!(
            metadata.names,
            Some(vec![Some("a".to_string()), Some("b".to_string())])
        );
        assert_eq!(metadata.dim, Some(vec![1, 2]));
        assert_eq!(metadata.class, Some(vec![Some("matrix".to_string())]));
        assert_eq!(metadata.attributes.len(), 3);
    }

    #[test]
    fn test_try_accessors_cover_non_vector_slots() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ptr)
        });
        unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().try_set_integer_elt(0, 7)
        }
        .expect("set integer");

        assert_eq!(sexp.clone().try_as_f64(), Ok(7.0));
        assert_eq!(sexp.clone().try_to_bool(), Ok(true));
        assert_eq!(
            sexp.clone().try_attrib().expect("attribute").as_raw(),
            unsafe { R_NilValue() }
        );
        assert!(sexp.clone().try_data_ptr().is_ok());
        assert!(matches!(
            sexp.try_formals(),
            Err(SexpError::TypeMismatch { expected, .. }) if expected == "closure"
        ));

        let symbol = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_node(SEXPTYPE::SYMSXP))
        });
        assert!(matches!(
            symbol.try_data_ptr(),
            Err(SexpError::TypeMismatch { expected, .. }) if expected == "vector or character scalar"
        ));

        let extptr = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_node(SEXPTYPE::EXTPTRSXP))
        });
        assert!(
            extptr
                .clone()
                .try_extptr_ptr()
                .expect("external pointer")
                .is_null()
        );
        assert_eq!(
            extptr
                .clone()
                .try_extptr_tag()
                .expect("external tag")
                .as_raw(),
            unsafe { R_NilValue() }
        );
        assert_eq!(
            extptr.try_extprot().expect("external prot").as_raw(),
            unsafe { R_NilValue() }
        );
    }

    #[test]
    fn test_sexp_type_predicates() {
        let mut arena = RArena::new();
        let sym = arena.alloc_node(SEXPTYPE::SYMSXP);
        let closure = arena.alloc_node(SEXPTYPE::CLOSXP);
        let env = arena.alloc_node(SEXPTYPE::ENVSXP);
        let list = arena.alloc_list_chain(2);
        let vec = arena.alloc_vector(SEXPTYPE::INTSXP, 3);

        assert!(some(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(sym) }).is_symbol());
        assert!(some(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(closure) }).is_closure());
        assert!(some(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(env) }).is_environment());
        assert!(some(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(list) }).is_pairlist());
        assert!(some(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(vec) }).is_vector());
        assert!(some(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(vec) }).is_atomic());
    }

    #[test]
    fn test_pairlist_iter_empty() {
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(nil)
        });
        let items: Vec<_> = PairlistIter::new(sexp).collect();
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_sexp_car_cdr_tag() {
        let mut arena = RArena::new();
        let car_val = arena.alloc_node(SEXPTYPE::INTSXP);
        let tag_val = arena.alloc_node(SEXPTYPE::SYMSXP);
        let cell = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.cons(
                car_val,
                unsafe { crate::sexp::globals::R_NilValue() },
                tag_val,
            )
        };
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(cell)
        });
        assert!(sexp.clone().car().is_some());
        assert!(sexp.clone().cdr().is_some());
        assert!(sexp.clone().tag().is_some());
        assert!(some(sexp.clone().car()).is_symbol() == false);
        assert!(some(sexp.tag()).is_symbol());
    }

    #[test]
    fn test_pairlist_argument_helpers() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let first = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::INTSXP)))
            .unwrap();
        let second = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::REALSXP)))
            .unwrap();
        let first_value = first.as_raw();
        let second_value = second.as_raw();
        let na_rm = session.with_active(|| unsafe {
            factory
                .wrap(crate::sexp::symbol::Rf_install(c"na.rm".as_ptr()))
                .unwrap()
        });
        let mut list = pairlist::PairlistBuilder::from_factory(factory);
        list.push(first, Some(na_rm)).unwrap();
        list.push(second, None).unwrap();
        let first = list.finish().unwrap();
        let second = first.try_next_pairlist_cell().unwrap().unwrap();
        let second_cell = second.as_raw();

        assert_eq!(
            first.clone().try_pairlist_arg(0).unwrap().as_raw(),
            first_value
        );
        assert_eq!(
            first.clone().try_pairlist_arg(1).unwrap().as_raw(),
            second_value
        );
        assert!(matches!(
            first.clone().try_pairlist_arg(2),
            Err(SexpError::MissingArgument { index: 2 })
        ));
        assert!(
            first
                .clone()
                .try_optional_pairlist_arg(2)
                .unwrap()
                .is_none()
        );
        assert!(
            first
                .clone()
                .try_optional_pairlist_arg(10)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            first
                .clone()
                .try_next_pairlist_cell()
                .unwrap()
                .unwrap()
                .as_raw(),
            second_cell
        );
        assert!(second.clone().try_next_pairlist_cell().unwrap().is_none());
        assert!(first.try_tag_name_eq(b"na.rm").unwrap());
        assert!(!second.try_tag_name_eq(b"na.rm").unwrap());
    }

    #[test]
    fn test_sexp_closure_accessors() {
        let mut arena = RArena::new();
        let formals = arena.alloc_list_chain(1);
        let body = arena.alloc_node(SEXPTYPE::NILSXP);
        let env = arena.alloc_node(SEXPTYPE::ENVSXP);
        let closure = arena.alloc_node(SEXPTYPE::CLOSXP);
        unsafe {
            crate::sexp::accessors::SET_FORMALS(closure, formals);
            crate::sexp::accessors::SET_BODY(closure, body);
            crate::sexp::accessors::SET_CLOENV(closure, env);
        }
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(closure)
        });
        assert!(sexp.clone().is_closure());
        assert!(sexp.clone().formals().is_some());
        assert!(sexp.clone().body().is_some());
        assert!(sexp.cloenv().is_some());
    }

    #[test]
    fn test_sexp_environment_accessors() {
        let mut arena = RArena::new();
        let frame = arena.alloc_list_chain(1);
        let enclos = arena.alloc_node(SEXPTYPE::ENVSXP);
        let env = arena.alloc_node(SEXPTYPE::ENVSXP);
        unsafe {
            crate::sexp::accessors::SET_FRAME(env, frame);
            crate::sexp::accessors::SET_ENCLOS(env, enclos);
            crate::sexp::accessors::SET_HASHTAB(env, ptr::null_mut());
        }
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(env)
        });
        assert!(sexp.clone().is_environment());
        assert!(sexp.clone().frame().is_some());
        assert!(sexp.enclos().is_some());
    }

    #[test]
    fn test_sexp_slice_wrong_type() {
        let mut arena = RArena::new();
        let sym = arena.alloc_node(SEXPTYPE::SYMSXP);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(sym)
        });
        assert!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                sexp.as_integer_slice()
            }
            .is_none()
        );
        assert!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                sexp.as_real_slice()
            }
            .is_none()
        );
        assert!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                sexp.as_raw_slice()
            }
            .is_none()
        );
    }

    #[test]
    fn test_atomic_accessors_reject_wrong_vector_type() {
        let mut arena = RArena::new();
        let real = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::REALSXP, 2))
        });
        let int = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::INTSXP, 2))
        });
        let logical = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::LGLSXP, 2))
        });

        assert!(real.clone().integer_elt(0).is_none());
        assert!(
            unsafe {
                /* SAFETY: fixture has no outstanding payload borrows. */
                real.clone().set_integer_elt(0, 1)
            } == false
        );
        assert!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                real.as_integer_slice()
            }
            .is_none()
        );
        assert!(real.iter_integer().next().is_none());

        assert!(int.clone().real_elt(0).is_none());
        assert!(
            unsafe {
                /* SAFETY: fixture has no outstanding payload borrows. */
                int.clone().set_real_elt(0, 1.0)
            } == false
        );
        assert!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                int.as_real_slice()
            }
            .is_none()
        );
        assert!(int.iter_real().next().is_none());

        assert!(logical.clone().integer_elt(0).is_none());
        assert!(
            unsafe {
                /* SAFETY: fixture has no outstanding payload borrows. */
                logical.clone().set_integer_elt(0, 1)
            } == false
        );
        assert!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                logical.as_integer_slice()
            }
            .is_none()
        );
        assert_eq!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                logical.as_logical_slice()
            },
            Some(&[0, 0][..])
        );
    }

    #[test]
    fn test_vector_accessors_reject_string_vectors() {
        let mut arena = RArena::new();
        let strings = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_vector(SEXPTYPE::STRSXP, 1))
        });
        let ch = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(arena.alloc_charsxp(b"x"))
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            strings.clone().set_string_elt(0, ch)
        });
        assert!(strings.clone().string_elt(0).is_some());
        assert!(strings.clone().vector_elt(0).is_none());
        assert!(strings.iter_vector().next().is_none());
    }

    #[test]
    fn test_sexp_primitive_accessors() {
        let mut arena = RArena::new();
        let special = arena.alloc_node(SEXPTYPE::SPECIALSXP);
        unsafe {
            (*special).data.primitive_mut().offset = 42;
        }
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(special)
        });
        assert!(sexp.clone().is_special());
        assert!(sexp.clone().is_primitive());
        assert!(!sexp.clone().is_builtin());
        assert_eq!(sexp.primoffset(), Some(42));

        let builtin = arena.alloc_node(SEXPTYPE::BUILTINSXP);
        unsafe {
            (*builtin).data.primitive_mut().offset = 7;
        }
        let sexp2 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(builtin)
        });
        assert!(sexp2.clone().is_builtin());
        assert!(sexp2.clone().is_primitive());
        assert!(!sexp2.clone().is_special());
        assert_eq!(sexp2.primoffset(), Some(7));

        let other = arena.alloc_node(SEXPTYPE::INTSXP);
        let sexp3 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(other)
        });
        assert!(!sexp3.clone().is_primitive());
        assert_eq!(sexp3.primoffset(), None);
    }

    #[test]
    fn test_sexp_charsxp_accessors() {
        let mut arena = RArena::new();
        let charsxp = arena.alloc_charsxp(b"hello world");
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(charsxp)
        });
        assert!(sexp.clone().is_charsxp());
        assert_eq!(sexp.clone().char_len(), Some(11));
        assert_eq!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                sexp.as_bytes()
            },
            Some(&b"hello world"[..])
        );
        assert_eq!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                sexp.as_str()
            },
            Some("hello world")
        );

        let other = arena.alloc_node(SEXPTYPE::INTSXP);
        let sexp2 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(other)
        });
        assert!(!sexp2.clone().is_charsxp());
        assert!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                sexp2.as_bytes()
            }
            .is_none()
        );
        assert!(
            unsafe {
                /* SAFETY: caller retains the handle and excludes payload mutation. */
                sexp2.as_str()
            }
            .is_none()
        );
    }

    #[test]
    fn test_sexp_complex_accessors() {
        let mut arena = RArena::new();
        let vec = arena.alloc_vector(SEXPTYPE::CPLXSXP, 3);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(vec)
        });

        let c1 = Rcomplex { r: 1.0, i: 2.0 };
        let c2 = Rcomplex { r: 3.0, i: 4.0 };
        let c3 = Rcomplex { r: 5.0, i: 6.0 };

        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_complex_elt(0, c1)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_complex_elt(1, c2)
        });
        assert!(unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_complex_elt(2, c3)
        });
        assert!(!unsafe {
            /* SAFETY: fixture has no outstanding payload borrows. */
            sexp.clone().set_complex_elt(3, c1)
        }); // out of bounds

        assert_eq!(sexp.clone().complex_elt(0), Some(c1));
        assert_eq!(sexp.clone().complex_elt(1), Some(c2));
        assert_eq!(sexp.clone().complex_elt(2), Some(c3));

        let slice = some(unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            sexp.as_complex_slice()
        });
        assert_eq!(slice.len(), 3);
        assert_eq!(slice[0].r, 1.0);
        assert_eq!(slice[2].i, 6.0);

        let vals: Vec<Rcomplex> = sexp.iter_complex().collect();
        assert_eq!(vals.len(), 3);
    }

    #[test]
    fn test_sexp_new_type_predicates() {
        let mut arena = RArena::new();

        let dots = arena.alloc_node(SEXPTYPE::DOTSXP);
        let sexp = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(dots)
        });
        assert!(sexp.clone().is_dots());

        let bc = arena.alloc_node(SEXPTYPE::BCODESXP);
        let sexp2 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(bc)
        });
        assert!(sexp2.is_bytecode());

        let ext = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
        let sexp3 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(ext)
        });
        assert!(sexp3.clone().is_extptr());
        assert!(sexp3.clone().extptr_ptr().is_some());
        assert!(sexp3.extptr_tag().is_none());

        let wr = arena.alloc_node(SEXPTYPE::WEAKREFSXP);
        let sexp4 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(wr)
        });
        assert!(sexp4.is_weakref());

        let s4 = arena.alloc_node(SEXPTYPE::OBJSXP);
        let sexp5 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(s4)
        });
        assert!(sexp5.is_s4());

        let expr = arena.alloc_vector(SEXPTYPE::EXPRSXP, 0);
        let sexp6 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(expr)
        });
        assert!(sexp6.is_expression());

        let clos = arena.alloc_node(SEXPTYPE::CLOSXP);
        let sexp7 = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(clos)
        });
        assert!(sexp7.is_function());
        assert!(sexp.is_function() == false);
    }
}
