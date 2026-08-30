//! Rust object model for R SEXP values.
//!
//! This module is the safe, Rust-facing layer over raw R `SEXP` pointers. It
//! keeps R's object categories recognizable while adding lifetime tracking,
//! checked accessors, type-directed borrowed views, owned projections, and
//! pairlist iteration.
//!
//! # Design
//!
//! [`Sexp<'a>`] wraps a raw `SEXP` pointer with a `PhantomData` marker
//! to track the lifetime of the underlying memory. Safe construction goes
//! through an owner such as [`RArena`](crate::sexp::memory::RArena) or
//! [`RSession`](crate::sexp::session::RSession), so the returned wrapper is
//! tied to the arena or session that owns the object. All element access is
//! bounds-checked. Legacy `Option<T>` accessors are kept for existing ported
//! C-shaped code, while new Rust code should prefer the `try_*` methods and
//! [`SexpView`] so type mistakes and bounds errors stay explicit.
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
//! elements. For bulk access, use the slice methods
//! (e.g., [`as_integer_slice`](Sexp::as_integer_slice)) or iterators
//! (e.g., [`iter_integer`](Sexp::iter_integer)).

mod error;
mod kind;
mod owned;
mod pairlist;
mod primitive;
mod slots;
mod value;
mod vector;
mod view;

pub use error::{SexpError, SexpResult};
pub(crate) use kind::{raw_is_atomic_vector, raw_is_vector};
pub(crate) use pairlist::PairlistBuilder;
pub use pairlist::PairlistIter;
pub use value::{SexpAttribute, SexpComplex, SexpMetadata, SexpValue};
pub use view::SexpView;

use super::ffi::{R_xlen_t, SEXP, SEXPTYPE, SexprecCore};
use super::globals::R_NilValue;
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
/// ```
/// use rmath::sexp::{Sexp, SEXPTYPE};
/// use rmath::sexp::memory::RArena;
///
/// let mut arena = RArena::new();
/// let sexp = arena
///     .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
///     .expect("arena allocation failed");
/// assert!(sexp.clone().is_vector()); // accessors consume the handle: clone to keep it
/// assert_eq!(sexp.len(), 3);
/// ```
///
/// # Pointer Equality
///
/// `Sexp` implements `PartialEq`, `Eq`, and `Hash` based on pointer
/// identity, not structural equality. Two `Sexp` values are equal if
/// and only if they point to the same memory address.
///
/// # Intentionally Not `Copy`
///
/// `Sexp` deliberately does not implement `Copy`. A `Copy` handle would let
/// a stale alias legally survive an in-place mutation of the same R object
/// reached through another handle (`SET_*` element writes, attribute
/// assignment, environment rebinding) — precisely the aliasing-undefined-
/// behavior class this crate forbids. Handles therefore move by default;
/// clone explicitly (see the [`Clone`] impl) when a second handle is
/// actually intended:
///
/// ```compile_fail
/// use rmath::sexp::{Sexp, SEXPTYPE};
/// use rmath::sexp::memory::RArena;
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
    _marker: std::marker::PhantomData<&'a SexprecCore>,
}

/// Duplicate this handle; the clone aliases the same R object, it does not
/// deep-copy it.
///
/// A cloned [`Sexp`] is a second lightweight handle (same raw `SEXP`
/// pointer, same [`SexpOwner`] token) over identical R memory. Cloning is
/// cheap and never touches R's heap, but every clone is an alias and the
/// aliasing discipline that forbids `Copy` applies to it: never retain a
/// clone across an in-place mutation of the object reached through another
/// handle. Where a clone exists merely to keep an earlier handle alive,
/// prefer re-deriving the handle from its owner instead.
impl Clone for Sexp<'_> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            ptr: self.ptr,
            owner: self.owner,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a> Sexp<'a> {
    /// Return R's immutable `NULL` singleton as an owner-independent handle.
    #[inline]
    pub fn nil() -> Sexp<'static> {
        unsafe { Sexp::from_static_raw_unchecked(R_NilValue()) }
    }

    /// Create a `Sexp` from a raw SEXP pointer for internal boundary code.
    ///
    /// Returns `None` if the pointer is null or visibly invalid. Public safe
    /// code should use owner-scoped wrapping through `RArena::sexp` or
    /// `RSession::sexp` instead.
    #[inline]
    pub(crate) fn from_raw(ptr: SEXP) -> Option<Self> {
        Self::try_from_raw(ptr).ok()
    }

    /// Create a `Sexp` from a raw SEXP pointer for internal boundary code.
    ///
    /// Unlike [`from_raw`](Self::from_raw), this reports why wrapping failed.
    #[inline]
    pub(crate) fn try_from_raw(ptr: SEXP) -> SexpResult<Self> {
        if ptr.is_null() {
            Err(SexpError::NullPointer)
        } else if (ptr as usize) % std::mem::align_of::<SexprecCore>() != 0 {
            Err(SexpError::MisalignedPointer {
                address: ptr as usize,
            })
        } else {
            Ok(Sexp {
                ptr,
                owner: SexpOwner::Unknown,
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
        let mut sexp = Sexp::try_from_raw(ptr)?;
        sexp.owner = SexpOwner::Arena(Self::arena_owner_token(arena));
        Ok(sexp)
    }

    /// Wrap a pointer that has already been validated against session-owned
    /// persistent storage.
    #[inline]
    pub(crate) fn from_session_raw<'session>(
        ptr: SEXP,
        instance: &'session crate::sexp::instance::RInstance,
    ) -> SexpResult<Sexp<'session>> {
        let mut sexp = Sexp::try_from_raw(ptr)?;
        sexp.owner = SexpOwner::Session(Self::session_owner_token(instance));
        Ok(sexp)
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
            _marker: std::marker::PhantomData,
        }
    }

    /// Create a `Sexp` from a known immutable singleton.
    #[inline]
    pub(crate) const unsafe fn from_static_raw_unchecked(ptr: SEXP) -> Self {
        Sexp {
            ptr,
            owner: SexpOwner::Static,
            _marker: std::marker::PhantomData,
        }
    }

    /// Get the underlying raw SEXP pointer.
    ///
    /// This is useful for passing the `Sexp` to FFI functions that
    /// expect a raw `SEXP`.
    #[inline]
    pub fn as_raw(self) -> SEXP {
        self.ptr
    }

    /// Return the owner provenance attached to this handle.
    #[inline]
    pub fn owner(self) -> SexpOwner {
        self.owner
    }

    /// Return true when this handle was created through a checked owner.
    #[inline]
    pub fn is_owner_scoped(self) -> bool {
        !matches!(self.owner, SexpOwner::Unknown)
    }

    /// Return true when this handle is scoped to `arena` and the arena still
    /// contains the raw pointer.
    #[inline]
    pub fn belongs_to_arena(self, arena: &crate::sexp::memory::RArena) -> bool {
        self.owner == SexpOwner::Arena(Self::arena_owner_token(arena)) && arena.contains(self.ptr)
    }

    /// Return true when this handle is scoped to persistent storage owned by
    /// `instance` and the instance still owns the raw pointer.
    #[inline]
    pub fn belongs_to_session(self, instance: &crate::sexp::instance::RInstance) -> bool {
        self.owner == SexpOwner::Session(Self::session_owner_token(instance))
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
    fn typed_data<T>(self, expected: SEXPTYPE) -> Option<*const T> {
        self.try_typed_data::<T>(expected, sexptype_name(expected))
            .ok()
    }

    #[inline]
    fn try_typed_data<T>(
        self,
        expected: SEXPTYPE,
        expected_name: &'static str,
    ) -> SexpResult<*const T> {
        self.clone().expect_type(expected, expected_name).clone()?;
        let data = unsafe { (*self.ptr).gengc_next_node as *const T };
        if data.is_null() {
            Err(SexpError::MissingData { sexptype: expected })
        } else {
            Ok(data)
        }
    }

    #[inline]
    fn expect_type(self, expected: SEXPTYPE, expected_name: &'static str) -> SexpResult<()> {
        if self.clone().typeof_() != expected {
            Err(SexpError::TypeMismatch {
                expected: expected_name,
                actual: self.typeof_(),
            })
        } else {
            Ok(())
        }
    }

    #[inline]
    fn expect_any_type(self, expected_name: &'static str, expected: &[SEXPTYPE]) -> SexpResult<()> {
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
    fn typed_data_mut<T>(self, expected: SEXPTYPE) -> Option<*mut T> {
        self.try_typed_data_mut::<T>(expected, sexptype_name(expected))
            .ok()
    }

    #[inline]
    fn try_typed_data_mut<T>(
        self,
        expected: SEXPTYPE,
        expected_name: &'static str,
    ) -> SexpResult<*mut T> {
        self.clone().expect_type(expected, expected_name).clone()?;
        let data = unsafe { (*self.ptr).gengc_next_node as *mut T };
        if data.is_null() {
            Err(SexpError::MissingData { sexptype: expected })
        } else {
            Ok(data)
        }
    }

    #[inline]
    fn typed_slice<T>(self, expected: SEXPTYPE) -> Option<&'a [T]> {
        self.try_typed_slice::<T>(expected, sexptype_name(expected))
            .ok()
    }

    #[inline]
    fn try_typed_slice<T>(
        self,
        expected: SEXPTYPE,
        expected_name: &'static str,
    ) -> SexpResult<&'a [T]> {
        self.clone().expect_type(expected, expected_name)?;
        let len = self.clone().len() as usize;
        if len == 0 {
            return Ok(&[]);
        }
        let data = self.try_typed_data::<T>(expected, expected_name)?;
        Ok(unsafe { std::slice::from_raw_parts(data, len) })
    }

    #[inline]
    fn try_index(self, i: R_xlen_t) -> SexpResult<usize> {
        let len = self.len();
        if i >= 0 && i < len {
            Ok(i as usize)
        } else {
            Err(SexpError::OutOfBounds { index: i, len })
        }
    }

    #[inline]
    fn vector_sexp_data(self) -> Option<*const SEXP> {
        self.try_vector_sexp_data().ok()
    }

    #[inline]
    fn try_vector_sexp_data(self) -> SexpResult<*const SEXP> {
        if !matches!(self.clone().typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP) {
            return Err(SexpError::TypeMismatch {
                expected: "generic or expression vector",
                actual: self.typeof_(),
            });
        }
        let data = unsafe { (*self.ptr).gengc_next_node as *const SEXP };
        if data.is_null() {
            Err(SexpError::MissingData {
                sexptype: self.typeof_(),
            })
        } else {
            Ok(data)
        }
    }

    #[inline]
    fn vector_sexp_data_mut(self) -> Option<*mut SEXP> {
        self.try_vector_sexp_data_mut().ok()
    }

    #[inline]
    fn try_vector_sexp_data_mut(self) -> SexpResult<*mut SEXP> {
        if !matches!(self.clone().typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP) {
            return Err(SexpError::TypeMismatch {
                expected: "generic or expression vector",
                actual: self.typeof_(),
            });
        }
        let data = unsafe { (*self.ptr).gengc_next_node as *mut SEXP };
        if data.is_null() {
            Err(SexpError::MissingData {
                sexptype: self.typeof_(),
            })
        } else {
            Ok(data)
        }
    }

    #[inline]
    fn valid_index(self, i: R_xlen_t) -> bool {
        self.try_index(i).is_ok()
    }

    #[inline]
    fn checked_child(ptr: SEXP) -> SexpResult<Sexp<'a>> {
        if ptr.is_null() {
            Ok(unsafe { Sexp::from_raw_unchecked(R_NilValue()) })
        } else {
            Sexp::try_from_raw(ptr)
        }
    }

    #[inline]
    fn try_pairlist(self) -> SexpResult<()> {
        if self.clone().is_pairlist() {
            Ok(())
        } else {
            Err(SexpError::TypeMismatch {
                expected: "pairlist or language object",
                actual: self.typeof_(),
            })
        }
    }

    /// Convert to a boolean value.
    ///
    /// Returns true for non-NULL values, or the actual boolean/logical value
    /// for LGLSXP/INTSXP types.
    pub fn to_bool(self) -> bool {
        self.try_to_bool().unwrap_or(true)
    }

    /// Convert to a boolean value with typed error reporting.
    ///
    /// `NULL` is false. Numeric and logical vectors use their first element;
    /// empty vectors report [`SexpError::OutOfBounds`]. Other values are true,
    /// matching R's broad truthiness at this wrapper layer.
    pub fn try_to_bool(self) -> SexpResult<bool> {
        if self.clone().is_nil() {
            return Ok(false);
        }
        match self.clone().typeof_() {
            SEXPTYPE::LGLSXP => self.try_logical_elt(0).map(|value| value != 0),
            SEXPTYPE::INTSXP => self.try_integer_elt(0).map(|value| value != 0),
            SEXPTYPE::REALSXP => self.try_real_elt(0).map(|value| value != 0.0),
            _ => Ok(true),
        }
    }

    /// Convert to an f64 value.
    ///
    /// Returns 0.0 for non-numeric types.
    pub fn as_f64(self) -> f64 {
        self.try_as_f64().unwrap_or(0.0)
    }

    /// Convert the first logical/integer/real element to `f64`.
    pub fn try_as_f64(self) -> SexpResult<f64> {
        match self.clone().typeof_() {
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
    pub fn car(self) -> Option<Sexp<'a>> {
        if self.clone().is_pairlist() {
            Sexp::from_raw(unsafe { (*self.ptr).data.listsxp.carval })
        } else {
            None
        }
    }

    /// Get the CAR with typed error reporting.
    #[inline]
    pub fn try_car(self) -> SexpResult<Sexp<'a>> {
        self.clone().try_pairlist().clone()?;
        Self::checked_child(unsafe { (*self.ptr).data.listsxp.carval })
    }

    /// Get the CDR (next cell) of a pairlist element.
    ///
    /// Returns `None` if this is not a pairlist or the CDR is null.
    #[inline]
    pub fn cdr(self) -> Option<Sexp<'a>> {
        if self.clone().is_pairlist() {
            Sexp::from_raw(unsafe { (*self.ptr).data.listsxp.cdrval })
        } else {
            None
        }
    }

    /// Get the CDR with typed error reporting.
    #[inline]
    pub fn try_cdr(self) -> SexpResult<Sexp<'a>> {
        self.clone().try_pairlist().clone()?;
        Self::checked_child(unsafe { (*self.ptr).data.listsxp.cdrval })
    }

    /// Get the TAG (name) of a pairlist element.
    ///
    /// Returns `None` if this is not a pairlist or the TAG is null.
    #[inline]
    pub fn tag(self) -> Option<Sexp<'a>> {
        if self.clone().is_pairlist() {
            Sexp::from_raw(unsafe { (*self.ptr).data.listsxp.tagval })
        } else {
            None
        }
    }

    /// Get the TAG with typed error reporting.
    #[inline]
    pub fn try_tag(self) -> SexpResult<Sexp<'a>> {
        self.clone().try_pairlist().clone()?;
        Self::checked_child(unsafe { (*self.ptr).data.listsxp.tagval })
    }

    /// Return the next pairlist cell, or `None` at the end of the chain.
    #[inline]
    pub(crate) fn try_next_pairlist_cell(self) -> SexpResult<Option<Sexp<'a>>> {
        let next = self.try_cdr()?;
        if next.clone().is_nil() {
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
    pub(crate) fn try_tag_name_eq(self, name: &[u8]) -> SexpResult<bool> {
        let tag = self.try_tag()?;
        if tag.clone().is_nil() || tag.clone().typeof_() != SEXPTYPE::SYMSXP {
            return Ok(false);
        }

        Ok(tag.try_printname()?.try_as_bytes()? == name)
    }
}

// Note: Index<usize> is intentionally NOT implemented for Sexp.
// The Index trait requires returning &Self::Output, but Sexp elements
// are created on-the-fly from raw pointers. Use vector_elt() and
// string_elt() for bounds-checked element access instead.

// ---------------------------------------------------------------------------
// PartialEq/Eq/Hash — pointer equality
// ---------------------------------------------------------------------------

impl PartialEq for Sexp<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl Eq for Sexp<'_> {}

impl std::hash::Hash for Sexp<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.ptr as usize).hash(state);
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
        assert!(Sexp::from_raw(ptr::null_mut()).is_none());
    }

    #[test]
    fn test_sexp_from_raw_misaligned_pointer() {
        assert!(Sexp::from_raw(0x1 as SEXP).is_none());
    }

    #[test]
    fn test_raw_wrapped_sexp_has_unknown_owner() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        let sexp = some(Sexp::from_raw(ptr));
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
        let sexp = some(Sexp::from_raw(ptr));
        assert_eq!(sexp.clone().len(), 0);
        assert!(sexp.clone().is_empty());
        assert!(sexp.is_symbol());
    }

    #[test]
    fn test_sexp_len_vector() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 5);
        let sexp = some(Sexp::from_raw(ptr));
        assert_eq!(sexp.clone().len(), 5);
        assert!(!sexp.clone().is_empty());
        assert!(sexp.is_vector());
    }

    #[test]
    fn test_sexp_bounds_check() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.clone().integer_elt(5).is_none());
        assert!(sexp.clone().integer_elt(-1).is_none());
        assert!(sexp.clone().integer_elt(0).is_some());
        assert!(sexp.integer_elt(2).is_some());
    }

    #[test]
    fn test_pairlist_iter() {
        let mut arena = RArena::new();
        let list = arena.alloc_list_chain(3);
        let sexp = some(Sexp::from_raw(list));
        let items: Vec<_> = PairlistIter::new(sexp).collect();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_sexp_partial_eq() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp1 = some(Sexp::from_raw(ptr));
        let sexp2 = some(Sexp::from_raw(ptr));
        assert_eq!(sexp1, sexp2);
    }

    #[test]
    fn test_sexp_display() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 5);
        let sexp = some(Sexp::from_raw(ptr));
        let s = format!("{}", sexp);
        assert!(s.contains("len=5"));
    }

    #[test]
    fn test_set_integer_elt() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.clone().set_integer_elt(0, 42));
        assert!(sexp.clone().set_integer_elt(5, 99) == false);
        assert_eq!(sexp.integer_elt(0), Some(42));
    }

    #[test]
    fn test_set_real_elt() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.clone().set_real_elt(0, 3.14));
        assert!(sexp.clone().set_real_elt(5, 99.0) == false);
        assert_eq!(sexp.real_elt(0), Some(3.14));
    }

    #[test]
    fn test_set_raw_elt() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.clone().set_raw_elt(0, 0xFF));
        assert!(sexp.clone().set_raw_elt(5, 0xAA) == false);
        assert_eq!(sexp.raw_elt(0), Some(0xFF));
    }

    #[test]
    fn test_as_integer_slice() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        let slice = sexp.as_integer_slice();
        assert!(slice.is_some());
        assert_eq!(some(slice).len(), 3);
    }

    #[test]
    fn test_as_real_slice() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 4);
        let sexp = some(Sexp::from_raw(ptr));
        let slice = sexp.as_real_slice();
        assert!(slice.is_some());
        assert_eq!(some(slice).len(), 4);
    }

    #[test]
    fn test_as_raw_slice() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, 5);
        let sexp = some(Sexp::from_raw(ptr));
        let slice = sexp.as_raw_slice();
        assert!(slice.is_some());
        assert_eq!(some(slice).len(), 5);
    }

    #[test]
    fn test_iter_integer() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        let items: Vec<_> = sexp.iter_integer().collect();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_iter_real() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 4);
        let sexp = some(Sexp::from_raw(ptr));
        let items: Vec<_> = sexp.iter_real().collect();
        assert_eq!(items.len(), 4);
    }

    #[test]
    fn test_iter_raw() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, 5);
        let sexp = some(Sexp::from_raw(ptr));
        let items: Vec<_> = sexp.iter_raw().collect();
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn test_sexp_equality() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 5);
        let a = some(Sexp::from_raw(ptr));
        let b = some(Sexp::from_raw(ptr));
        assert_eq!(a, b);
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn test_sexp_hash() {
        use std::collections::HashSet;
        let mut arena = RArena::new();
        let p1 = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let p2 = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let a = some(Sexp::from_raw(p1));
        let b = some(Sexp::from_raw(p2));
        let mut set = HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&a));
        assert!(!set.contains(&b));
    }

    #[test]
    fn test_sexp_display_len10() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 10);
        let sexp = some(Sexp::from_raw(ptr));
        let s = format!("{}", sexp);
        assert!(s.contains("len=10"));
    }

    #[test]
    fn test_sexp_mutation() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert_eq!(sexp.clone().integer_elt(0), Some(0));
        assert!(sexp.clone().set_integer_elt(0, 42));
        assert!(sexp.clone().set_integer_elt(1, -7));
        assert!(sexp.clone().set_integer_elt(2, 99));
        assert_eq!(sexp.clone().integer_elt(0), Some(42));
        assert_eq!(sexp.clone().integer_elt(1), Some(-7));
        assert_eq!(sexp.clone().integer_elt(2), Some(99));
        assert!(!sexp.clone().set_integer_elt(5, 0));
        assert!(sexp.integer_elt(5).is_none());
    }

    #[test]
    fn test_sexp_real_mutation() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.clone().set_real_elt(0, 1.5));
        assert!(sexp.clone().set_real_elt(1, 2.5));
        assert!(sexp.clone().set_real_elt(2, 3.5));
        assert_eq!(sexp.clone().real_elt(0), Some(1.5));
        assert_eq!(sexp.clone().real_elt(1), Some(2.5));
        assert_eq!(sexp.real_elt(2), Some(3.5));
    }

    #[test]
    fn test_sexp_slice_views() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 4);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.clone().set_integer_elt(0, 10));
        assert!(sexp.clone().set_integer_elt(1, 20));
        assert!(sexp.clone().set_integer_elt(2, 30));
        assert!(sexp.clone().set_integer_elt(3, 40));
        let slice = some(sexp.as_integer_slice());
        assert_eq!(slice, &[10, 20, 30, 40]);
    }

    #[test]
    fn test_sexp_real_slice() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.clone().set_real_elt(0, 1.1));
        assert!(sexp.clone().set_real_elt(1, 2.2));
        assert!(sexp.clone().set_real_elt(2, 3.3));
        let slice = some(sexp.as_real_slice());
        assert!((slice[0] - 1.1).abs() < f64::EPSILON);
        assert!((slice[1] - 2.2).abs() < f64::EPSILON);
        assert!((slice[2] - 3.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_sexp_iterators() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 5);
        let sexp = some(Sexp::from_raw(ptr));
        for i in 0..5 {
            sexp.clone().set_integer_elt(i, (i * 10) as i32);
        }
        let values: Vec<_> = sexp.iter_integer().collect();
        assert_eq!(values, vec![0, 10, 20, 30, 40]);
    }

    #[test]
    fn test_sexp_real_iterator() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 4);
        let sexp = some(Sexp::from_raw(ptr));
        for i in 0..4 {
            sexp.clone().set_real_elt(i, i as f64 * 0.5);
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
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.clone().set_raw_elt(0, 0xDE));
        assert!(sexp.clone().set_raw_elt(1, 0xAD));
        assert!(sexp.clone().set_raw_elt(2, 0xBE));
        assert!(sexp.clone().set_raw_elt(3, 0xEF));
        let slice = some(sexp.as_raw_slice());
        assert_eq!(slice, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_try_accessors_report_type_and_bounds_errors() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 2);
        let sexp = some(Sexp::from_raw(ptr));
        sexp.clone()
            .try_set_integer_elt(0, 10)
            .expect("set integer");
        sexp.clone()
            .try_set_integer_elt(1, 20)
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
        let sexp = some(Sexp::from_raw(ptr));
        sexp.clone().try_set_real_elt(0, 1.5).expect("set real");
        sexp.clone().try_set_real_elt(1, 2.5).expect("set real");

        match sexp.view().expect("view") {
            SexpView::Real(values) => assert_eq!(values, &[1.5, 2.5]),
            other => panic!("unexpected view: {other:?}"),
        }
    }

    #[test]
    fn test_to_owned_value_maps_atomic_na_values() {
        let mut arena = RArena::new();
        let logical = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::LGLSXP, 3)));
        logical
            .clone()
            .try_set_logical_elt(0, 1)
            .expect("set logical");
        logical
            .clone()
            .try_set_logical_elt(1, 0)
            .expect("set logical");
        logical
            .clone()
            .try_set_logical_elt(2, NA_LOGICAL)
            .expect("set logical");

        let integer = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::INTSXP, 2)));
        integer
            .clone()
            .try_set_integer_elt(0, 10)
            .expect("set integer");
        integer
            .clone()
            .try_set_integer_elt(1, NA_INTEGER)
            .expect("set integer");

        let real = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::REALSXP, 1)));
        real.clone().try_set_real_elt(0, NA_REAL).expect("set real");

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
        let strings = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::STRSXP, 2)));
        let hello = some(Sexp::from_raw(arena.alloc_charsxp(b"hello")));
        let na_string = some(Sexp::from_raw(unsafe { R_NaString() }));
        strings
            .clone()
            .try_set_string_elt(0, hello)
            .expect("set string");
        strings
            .clone()
            .try_set_string_elt(1, na_string)
            .expect("set string");
        assert_eq!(
            strings.clone().try_string_text_elt(0).expect("text"),
            Some("hello")
        );
        assert_eq!(
            strings.clone().try_string_text_elt(1).expect("NA text"),
            None
        );

        let raw = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::RAWSXP, 2)));
        raw.clone().try_set_raw_elt(0, 0x41).expect("set raw");
        raw.clone().try_set_raw_elt(1, 0x5a).expect("set raw");

        let complex = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::CPLXSXP, 2)));
        complex
            .clone()
            .try_set_complex_elt(0, Rcomplex { r: 1.0, i: -2.0 })
            .expect("set complex");
        complex
            .clone()
            .try_set_complex_elt(1, Rcomplex { r: NA_REAL, i: 0.0 })
            .expect("set complex");

        let list = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::VECSXP, 3)));
        list.clone()
            .try_set_vector_elt(0, strings)
            .expect("set list");
        list.clone().try_set_vector_elt(1, raw).expect("set list");
        list.clone()
            .try_set_vector_elt(2, complex)
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
        let _session = crate::sexp::session::RSession::new();
        let mut arena = RArena::new();
        let vector = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::INTSXP, 2)));
        vector
            .clone()
            .try_set_integer_elt(0, 10)
            .expect("set integer");
        vector
            .clone()
            .try_set_integer_elt(1, 20)
            .expect("set integer");

        let names = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::STRSXP, 2)));
        names
            .clone()
            .try_set_string_elt(0, some(Sexp::from_raw(arena.alloc_charsxp(b"a"))))
            .expect("set name");
        names
            .clone()
            .try_set_string_elt(1, some(Sexp::from_raw(arena.alloc_charsxp(b"b"))))
            .expect("set name");

        let dim = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::INTSXP, 2)));
        dim.clone().try_set_integer_elt(0, 1).expect("set dim");
        dim.clone().try_set_integer_elt(1, 2).expect("set dim");

        let class = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::STRSXP, 1)));
        class
            .clone()
            .try_set_string_elt(0, some(Sexp::from_raw(arena.alloc_charsxp(b"matrix"))))
            .expect("set class");

        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        let class_cell = arena.cons(class.as_raw(), nil, unsafe {
            crate::sexp::symbol::Rf_install(c"class".as_ptr())
        });
        let dim_cell = arena.cons(dim.as_raw(), class_cell, unsafe {
            crate::sexp::symbol::Rf_install(c"dim".as_ptr())
        });
        let names_cell = arena.cons(names.as_raw(), dim_cell, unsafe {
            crate::sexp::symbol::Rf_install(c"names".as_ptr())
        });
        unsafe { crate::sexp::accessors::SET_ATTRIB(vector.clone().as_raw(), names_cell) };

        let value = vector.to_owned_value().expect("owned value");
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
        let sexp = some(Sexp::from_raw(ptr));
        sexp.clone().try_set_integer_elt(0, 7).expect("set integer");

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

        let symbol = some(Sexp::from_raw(arena.alloc_node(SEXPTYPE::SYMSXP)));
        assert!(matches!(
            symbol.try_data_ptr(),
            Err(SexpError::TypeMismatch { expected, .. }) if expected == "vector or character scalar"
        ));

        let extptr = some(Sexp::from_raw(arena.alloc_node(SEXPTYPE::EXTPTRSXP)));
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

        assert!(some(Sexp::from_raw(sym)).is_symbol());
        assert!(some(Sexp::from_raw(closure)).is_closure());
        assert!(some(Sexp::from_raw(env)).is_environment());
        assert!(some(Sexp::from_raw(list)).is_pairlist());
        assert!(some(Sexp::from_raw(vec)).is_vector());
        assert!(some(Sexp::from_raw(vec)).is_atomic());
    }

    #[test]
    fn test_pairlist_iter_empty() {
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        let sexp = some(Sexp::from_raw(nil));
        let items: Vec<_> = PairlistIter::new(sexp).collect();
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_sexp_car_cdr_tag() {
        let mut arena = RArena::new();
        let car_val = arena.alloc_node(SEXPTYPE::INTSXP);
        let tag_val = arena.alloc_node(SEXPTYPE::SYMSXP);
        let cell = arena.cons(
            car_val,
            unsafe { crate::sexp::globals::R_NilValue() },
            tag_val,
        );
        let sexp = some(Sexp::from_raw(cell));
        assert!(sexp.clone().car().is_some());
        assert!(sexp.clone().cdr().is_some());
        assert!(sexp.clone().tag().is_some());
        assert!(some(sexp.clone().car()).is_symbol() == false);
        assert!(some(sexp.tag()).is_symbol());
    }

    #[test]
    fn test_pairlist_argument_helpers() {
        let _session = crate::sexp::session::RSession::new();
        let mut arena = RArena::new();
        let first_value = arena.alloc_node(SEXPTYPE::INTSXP);
        let second_value = arena.alloc_node(SEXPTYPE::REALSXP);
        let na_rm = unsafe { crate::sexp::symbol::Rf_install(c"na.rm".as_ptr()) };
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        let second_cell = arena.cons(second_value, nil, nil);
        let first_cell = arena.cons(first_value, second_cell, na_rm);

        let first = some(Sexp::from_raw(first_cell));
        let second = some(Sexp::from_raw(second_cell));

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
            (*closure).data.closxp.formals = formals;
            (*closure).data.closxp.body = body;
            (*closure).data.closxp.env = env;
        }
        let sexp = some(Sexp::from_raw(closure));
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
            (*env).data.envsxp.frame = frame;
            (*env).data.envsxp.enclos = enclos;
            (*env).data.envsxp.hashtab = ptr::null_mut();
        }
        let sexp = some(Sexp::from_raw(env));
        assert!(sexp.clone().is_environment());
        assert!(sexp.clone().frame().is_some());
        assert!(sexp.enclos().is_some());
    }

    #[test]
    fn test_sexp_slice_wrong_type() {
        let mut arena = RArena::new();
        let sym = arena.alloc_node(SEXPTYPE::SYMSXP);
        let sexp = some(Sexp::from_raw(sym));
        assert!(sexp.clone().as_integer_slice().is_none());
        assert!(sexp.clone().as_real_slice().is_none());
        assert!(sexp.as_raw_slice().is_none());
    }

    #[test]
    fn test_atomic_accessors_reject_wrong_vector_type() {
        let mut arena = RArena::new();
        let real = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::REALSXP, 2)));
        let int = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::INTSXP, 2)));
        let logical = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::LGLSXP, 2)));

        assert!(real.clone().integer_elt(0).is_none());
        assert!(real.clone().set_integer_elt(0, 1) == false);
        assert!(real.clone().as_integer_slice().is_none());
        assert!(real.iter_integer().next().is_none());

        assert!(int.clone().real_elt(0).is_none());
        assert!(int.clone().set_real_elt(0, 1.0) == false);
        assert!(int.clone().as_real_slice().is_none());
        assert!(int.iter_real().next().is_none());

        assert!(logical.clone().integer_elt(0).is_none());
        assert!(logical.clone().set_integer_elt(0, 1) == false);
        assert!(logical.clone().as_integer_slice().is_none());
        assert_eq!(logical.as_logical_slice(), Some(&[0, 0][..]));
    }

    #[test]
    fn test_vector_accessors_reject_string_vectors() {
        let mut arena = RArena::new();
        let strings = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::STRSXP, 1)));
        let ch = some(Sexp::from_raw(arena.alloc_charsxp(b"x")));
        assert!(strings.clone().set_string_elt(0, ch));
        assert!(strings.clone().string_elt(0).is_some());
        assert!(strings.clone().vector_elt(0).is_none());
        assert!(strings.iter_vector().next().is_none());
    }

    #[test]
    fn test_sexp_primitive_accessors() {
        let mut arena = RArena::new();
        let special = arena.alloc_node(SEXPTYPE::SPECIALSXP);
        unsafe {
            (*special).data.primsxp.offset = 42;
        }
        let sexp = some(Sexp::from_raw(special));
        assert!(sexp.clone().is_special());
        assert!(sexp.clone().is_primitive());
        assert!(!sexp.clone().is_builtin());
        assert_eq!(sexp.primoffset(), Some(42));

        let builtin = arena.alloc_node(SEXPTYPE::BUILTINSXP);
        unsafe {
            (*builtin).data.primsxp.offset = 7;
        }
        let sexp2 = some(Sexp::from_raw(builtin));
        assert!(sexp2.clone().is_builtin());
        assert!(sexp2.clone().is_primitive());
        assert!(!sexp2.clone().is_special());
        assert_eq!(sexp2.primoffset(), Some(7));

        let other = arena.alloc_node(SEXPTYPE::INTSXP);
        let sexp3 = some(Sexp::from_raw(other));
        assert!(!sexp3.clone().is_primitive());
        assert_eq!(sexp3.primoffset(), None);
    }

    #[test]
    fn test_sexp_charsxp_accessors() {
        let mut arena = RArena::new();
        let charsxp = arena.alloc_charsxp(b"hello world");
        let sexp = some(Sexp::from_raw(charsxp));
        assert!(sexp.clone().is_charsxp());
        assert_eq!(sexp.clone().char_len(), Some(11));
        assert_eq!(sexp.clone().as_bytes(), Some(&b"hello world"[..]));
        assert_eq!(sexp.as_str(), Some("hello world"));

        let other = arena.alloc_node(SEXPTYPE::INTSXP);
        let sexp2 = some(Sexp::from_raw(other));
        assert!(!sexp2.clone().is_charsxp());
        assert!(sexp2.clone().as_bytes().is_none());
        assert!(sexp2.as_str().is_none());
    }

    #[test]
    fn test_sexp_complex_accessors() {
        let mut arena = RArena::new();
        let vec = arena.alloc_vector(SEXPTYPE::CPLXSXP, 3);
        let sexp = some(Sexp::from_raw(vec));

        let c1 = Rcomplex { r: 1.0, i: 2.0 };
        let c2 = Rcomplex { r: 3.0, i: 4.0 };
        let c3 = Rcomplex { r: 5.0, i: 6.0 };

        assert!(sexp.clone().set_complex_elt(0, c1));
        assert!(sexp.clone().set_complex_elt(1, c2));
        assert!(sexp.clone().set_complex_elt(2, c3));
        assert!(!sexp.clone().set_complex_elt(3, c1)); // out of bounds

        assert_eq!(sexp.clone().complex_elt(0), Some(c1));
        assert_eq!(sexp.clone().complex_elt(1), Some(c2));
        assert_eq!(sexp.clone().complex_elt(2), Some(c3));

        let slice = some(sexp.clone().as_complex_slice());
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
        let sexp = some(Sexp::from_raw(dots));
        assert!(sexp.clone().is_dots());

        let bc = arena.alloc_node(SEXPTYPE::BCODESXP);
        let sexp2 = some(Sexp::from_raw(bc));
        assert!(sexp2.is_bytecode());

        let ext = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
        unsafe {
            (*ext).data.extptr = [
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ];
        }
        let sexp3 = some(Sexp::from_raw(ext));
        assert!(sexp3.clone().is_extptr());
        assert!(sexp3.clone().extptr_ptr().is_some());
        assert!(sexp3.extptr_tag().is_none());

        let wr = arena.alloc_node(SEXPTYPE::WEAKREFSXP);
        let sexp4 = some(Sexp::from_raw(wr));
        assert!(sexp4.is_weakref());

        let s4 = arena.alloc_node(SEXPTYPE::OBJSXP);
        let sexp5 = some(Sexp::from_raw(s4));
        assert!(sexp5.is_s4());

        let expr = arena.alloc_vector(SEXPTYPE::EXPRSXP, 0);
        let sexp6 = some(Sexp::from_raw(expr));
        assert!(sexp6.is_expression());

        let clos = arena.alloc_node(SEXPTYPE::CLOSXP);
        let sexp7 = some(Sexp::from_raw(clos));
        assert!(sexp7.is_function());
        assert!(sexp.is_function() == false);
    }
}
