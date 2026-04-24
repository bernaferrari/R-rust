//! Safe wrapper types for R SEXP objects.
//!
//! This module provides idiomatic Rust abstractions over the raw FFI
//! `SEXP` pointers. The [`Sexp`] type wraps raw pointers with lifetime
//! tracking and safe accessor methods, while [`PairlistIter`] provides
//! iteration over pairlist chains.
//!
//! # Design
//!
//! [`Sexp<'a>`] wraps a raw `SEXP` pointer with a `PhantomData` marker
//! to track the lifetime of the underlying memory. This ensures that
//! `Sexp` references cannot outlive the arena or session that owns the
//! data. All element access is bounds-checked, returning `Option<T>`
//! rather than panicking on out-of-bounds access.
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

use std::os::raw::{c_double, c_int};

use super::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, SexprecCore};
use super::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Sexp — safe wrapper around SEXP
// ---------------------------------------------------------------------------

/// A safe, lifetime-tracked wrapper around an R SEXP pointer.
///
/// This type provides bounds-checked access to R objects while maintaining
/// FFI compatibility through [`as_raw`](Sexp::as_raw) and
/// [`from_raw`](Sexp::from_raw). The lifetime parameter `'a` ensures
/// that the `Sexp` cannot outlive the memory it points to.
///
/// # Examples
///
/// ```
/// use rmath::sexp::{Sexp, SEXPTYPE};
/// use rmath::sexp::memory::RArena;
///
/// let mut arena = RArena::new();
/// let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
/// let sexp = Sexp::from_raw(ptr).expect("from_raw returned None");
/// assert_eq!(sexp.len(), 3);
/// assert!(sexp.is_vector());
/// ```
///
/// # Pointer Equality
///
/// `Sexp` implements `PartialEq`, `Eq`, and `Hash` based on pointer
/// identity, not structural equality. Two `Sexp` values are equal if
/// and only if they point to the same memory address.
#[derive(Clone, Copy, Debug)]
pub struct Sexp<'a> {
    ptr: SEXP,
    _marker: std::marker::PhantomData<&'a SexprecCore>,
}

impl<'a> Sexp<'a> {
    /// Create a `Sexp` from a raw SEXP pointer.
    ///
    /// Returns `None` if the pointer is null.
    ///
    /// # Examples
    ///
    /// ```
    /// use rmath::sexp::Sexp;
    /// use std::ptr;
    ///
    /// assert!(Sexp::from_raw(ptr::null_mut()).is_none());
    /// ```
    #[inline]
    pub fn from_raw(ptr: SEXP) -> Option<Self> {
        if ptr.is_null() {
            None
        } else if (ptr as usize) % std::mem::align_of::<SexprecCore>() != 0 {
            None
        } else {
            Some(Sexp {
                ptr,
                _marker: std::marker::PhantomData,
            })
        }
    }

    /// Create a `Sexp` from a raw pointer without null checking.
    ///
    /// # Safety
    ///
    /// The pointer must be non-null and point to a valid `SexprecCore`
    /// that lives at least as long as `'a`.
    #[inline]
    pub const unsafe fn from_raw_unchecked(ptr: SEXP) -> Self {
        Sexp {
            ptr,
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

    #[inline]
    fn typed_data<T>(self, expected: SEXPTYPE) -> Option<*const T> {
        if self.typeof_() != expected {
            return None;
        }
        let data = unsafe { (*self.ptr).gengc_next_node as *const T };
        if data.is_null() { None } else { Some(data) }
    }

    #[inline]
    fn typed_data_mut<T>(self, expected: SEXPTYPE) -> Option<*mut T> {
        if self.typeof_() != expected {
            return None;
        }
        let data = unsafe { (*self.ptr).gengc_next_node as *mut T };
        if data.is_null() { None } else { Some(data) }
    }

    #[inline]
    fn typed_slice<T>(self, expected: SEXPTYPE) -> Option<&'a [T]> {
        if self.typeof_() != expected {
            return None;
        }
        let len = self.len() as usize;
        if len == 0 {
            return Some(&[]);
        }
        let data = self.typed_data::<T>(expected)?;
        Some(unsafe { std::slice::from_raw_parts(data, len) })
    }

    #[inline]
    fn vector_sexp_data(self) -> Option<*const SEXP> {
        if !matches!(self.typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP) {
            return None;
        }
        let data = unsafe { (*self.ptr).gengc_next_node as *const SEXP };
        if data.is_null() { None } else { Some(data) }
    }

    #[inline]
    fn vector_sexp_data_mut(self) -> Option<*mut SEXP> {
        if !matches!(self.typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP) {
            return None;
        }
        let data = unsafe { (*self.ptr).gengc_next_node as *mut SEXP };
        if data.is_null() { None } else { Some(data) }
    }

    #[inline]
    fn valid_index(self, i: R_xlen_t) -> bool {
        i >= 0 && i < self.len()
    }

    /// Get the type of this SEXP.
    #[inline]
    pub fn typeof_(self) -> SEXPTYPE {
        unsafe { (*self.ptr).sxpinfo.type_of() }
    }

    /// Get the length of a vector SEXP.
    ///
    /// Returns 0 for non-vector types.
    #[inline]
    pub fn len(self) -> R_xlen_t {
        if self.typeof_().is_vector_type() {
            unsafe { (*self.ptr).vecsxp_length() }
        } else {
            0
        }
    }

    /// Check if this SEXP is empty (length 0 or nil).
    #[inline]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Check if this is R_NilValue.
    #[inline]
    pub fn is_nil(self) -> bool {
        self.ptr == unsafe { R_NilValue() }
    }

    /// Convert to a boolean value.
    ///
    /// Returns true for non-NULL values, or the actual boolean/logical value
    /// for LGLSXP/INTSXP types.
    pub fn to_bool(self) -> bool {
        if self.is_nil() {
            return false;
        }
        match self.typeof_() {
            SEXPTYPE::LGLSXP => self.logical_elt(0).unwrap_or(0) != 0,
            SEXPTYPE::INTSXP => self.integer_elt(0).unwrap_or(0) != 0,
            SEXPTYPE::REALSXP => self.real_elt(0).unwrap_or(0.0) != 0.0,
            _ => true,
        }
    }

    /// Convert to an f64 value.
    ///
    /// Returns 0.0 for non-numeric types.
    pub fn as_f64(self) -> f64 {
        match self.typeof_() {
            SEXPTYPE::REALSXP => self.real_elt(0).unwrap_or(0.0),
            SEXPTYPE::INTSXP => self.integer_elt(0).unwrap_or(0) as f64,
            SEXPTYPE::LGLSXP => self.logical_elt(0).unwrap_or(0) as f64,
            _ => 0.0,
        }
    }

    /// Check if this is a null value (R_NilValue).
    #[inline]
    pub fn is_null_value(self) -> bool {
        self.is_nil()
    }

    /// Check if this is a symbol (SYMSXP).
    #[inline]
    pub fn is_symbol(self) -> bool {
        self.typeof_() == SEXPTYPE::SYMSXP
    }

    /// Check if this is a closure (CLOSXP, i.e., a user-defined function).
    #[inline]
    pub fn is_closure(self) -> bool {
        self.typeof_() == SEXPTYPE::CLOSXP
    }

    /// Check if this is an environment (ENVSXP).
    #[inline]
    pub fn is_environment(self) -> bool {
        self.typeof_() == SEXPTYPE::ENVSXP
    }

    /// Check if this is a pairlist (LISTSXP or LANGSXP).
    #[inline]
    pub fn is_pairlist(self) -> bool {
        let t = self.typeof_();
        t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP
    }

    /// Check if this is an atomic vector.
    ///
    /// Atomic vectors hold primitive data directly (LGLSXP, INTSXP,
    /// REALSXP, CPLXSXP, STRSXP, RAWSXP).
    #[inline]
    pub fn is_atomic(self) -> bool {
        self.typeof_().is_atomic_type()
    }

    /// Check if this is a vector type.
    ///
    /// Includes all atomic vectors plus VECSXP, EXPRSXP, and RAWSXP.
    #[inline]
    pub fn is_vector(self) -> bool {
        self.typeof_().is_vector_type()
    }

    // --- Vector element access with bounds checking ---

    /// Get the i-th logical value with bounds checking.
    ///
    /// Returns `None` if this is not a logical vector, the index is out of
    /// bounds, or the data pointer is null.
    #[inline]
    pub fn logical_elt(self, i: R_xlen_t) -> Option<c_int> {
        if !self.valid_index(i) {
            return None;
        }
        let data = self.typed_data::<c_int>(SEXPTYPE::LGLSXP)?;
        Some(unsafe { *data.add(i as usize) })
    }

    /// Get the i-th integer value with bounds checking.
    ///
    /// Returns `None` if this is not an integer vector, the index is out of
    /// bounds, or the data pointer is null.
    #[inline]
    pub fn integer_elt(self, i: R_xlen_t) -> Option<c_int> {
        if !self.valid_index(i) {
            return None;
        }
        let data = self.typed_data::<c_int>(SEXPTYPE::INTSXP)?;
        Some(unsafe { *data.add(i as usize) })
    }

    /// Get the i-th real (double) value with bounds checking.
    ///
    /// Returns `None` if this is not a real vector, the index is out of bounds,
    /// or the data pointer is null.
    #[inline]
    pub fn real_elt(self, i: R_xlen_t) -> Option<c_double> {
        if !self.valid_index(i) {
            return None;
        }
        let data = self.typed_data::<c_double>(SEXPTYPE::REALSXP)?;
        Some(unsafe { *data.add(i as usize) })
    }

    /// Get the i-th raw byte with bounds checking.
    ///
    /// Returns `None` if this is not a raw vector, the index is out of bounds,
    /// or the data pointer is null.
    #[inline]
    pub fn raw_elt(self, i: R_xlen_t) -> Option<Rbyte> {
        if !self.valid_index(i) {
            return None;
        }
        let data = self.typed_data::<Rbyte>(SEXPTYPE::RAWSXP)?;
        Some(unsafe { *data.add(i as usize) })
    }

    /// Get the i-th complex value with bounds checking.
    ///
    /// Returns `None` if this is not a complex vector, the index is out of
    /// bounds, or the data pointer is null.
    #[inline]
    pub fn complex_elt(self, i: R_xlen_t) -> Option<Rcomplex> {
        if !self.valid_index(i) {
            return None;
        }
        let data = self.typed_data::<Rcomplex>(SEXPTYPE::CPLXSXP)?;
        Some(unsafe { *data.add(i as usize) })
    }

    /// Get the i-th string element (CHARSXP) with bounds checking.
    ///
    /// Returns `None` if the index is out of bounds, the data pointer is null,
    /// or the element itself is null.
    #[inline]
    pub fn string_elt(self, i: R_xlen_t) -> Option<Sexp<'a>> {
        if self.typeof_() != SEXPTYPE::STRSXP || !self.valid_index(i) {
            return None;
        }
        let data = self.typed_data::<SEXP>(SEXPTYPE::STRSXP)?;
        Self::from_raw(unsafe { *data.add(i as usize) })
    }

    /// Get the i-th vector element with bounds checking.
    ///
    /// Returns `None` if the index is out of bounds, the data pointer is null,
    /// or the element itself is null.
    #[inline]
    pub fn vector_elt(self, i: R_xlen_t) -> Option<Sexp<'a>> {
        if !self.valid_index(i) {
            return None;
        }
        let data = self.vector_sexp_data()?;
        Self::from_raw(unsafe { *data.add(i as usize) })
    }

    // --- Pairlist iteration ---

    /// Get the CAR (value) of a pairlist element.
    ///
    /// Returns `None` if this is not a pairlist or the CAR is null.
    #[inline]
    pub fn car(self) -> Option<Sexp<'a>> {
        if self.is_pairlist() {
            Sexp::from_raw(unsafe { (*self.ptr).data.listsxp.carval })
        } else {
            None
        }
    }

    /// Get the CDR (next cell) of a pairlist element.
    ///
    /// Returns `None` if this is not a pairlist or the CDR is null.
    #[inline]
    pub fn cdr(self) -> Option<Sexp<'a>> {
        if self.is_pairlist() {
            Sexp::from_raw(unsafe { (*self.ptr).data.listsxp.cdrval })
        } else {
            None
        }
    }

    /// Get the TAG (name) of a pairlist element.
    ///
    /// Returns `None` if this is not a pairlist or the TAG is null.
    #[inline]
    pub fn tag(self) -> Option<Sexp<'a>> {
        if self.is_pairlist() {
            Sexp::from_raw(unsafe { (*self.ptr).data.listsxp.tagval })
        } else {
            None
        }
    }

    // --- Closure accessors ---

    /// Get the formal parameters of a closure.
    ///
    /// Returns `None` if this is not a closure or the formals are null.
    #[inline]
    pub fn formals(self) -> Option<Sexp<'a>> {
        if self.is_closure() {
            Sexp::from_raw(unsafe { (*self.ptr).data.closxp.formals })
        } else {
            None
        }
    }

    /// Get the body of a closure.
    ///
    /// Returns `None` if this is not a closure or the body is null.
    #[inline]
    pub fn body(self) -> Option<Sexp<'a>> {
        if self.is_closure() {
            Sexp::from_raw(unsafe { (*self.ptr).data.closxp.body })
        } else {
            None
        }
    }

    /// Get the environment of a closure.
    ///
    /// Returns `None` if this is not a closure or the environment is null.
    #[inline]
    pub fn cloenv(self) -> Option<Sexp<'a>> {
        if self.is_closure() {
            Sexp::from_raw(unsafe { (*self.ptr).data.closxp.env })
        } else {
            None
        }
    }

    // --- Environment accessors ---

    /// Get the frame of an environment.
    ///
    /// Returns `None` if this is not an environment or the frame is null.
    #[inline]
    pub fn frame(self) -> Option<Sexp<'a>> {
        if self.is_environment() {
            Sexp::from_raw(unsafe { (*self.ptr).data.envsxp.frame })
        } else {
            None
        }
    }

    /// Get the enclosing (parent) environment.
    ///
    /// Returns `None` if this is not an environment or the enclosing env is null.
    #[inline]
    pub fn enclos(self) -> Option<Sexp<'a>> {
        if self.is_environment() {
            Sexp::from_raw(unsafe { (*self.ptr).data.envsxp.enclos })
        } else {
            None
        }
    }

    /// Get the hash table of an environment.
    ///
    /// Returns `None` if this is not an environment or the hashtab is null.
    #[inline]
    pub fn hashtab(self) -> Option<Sexp<'a>> {
        if self.is_environment() {
            Sexp::from_raw(unsafe { (*self.ptr).data.envsxp.hashtab })
        } else {
            None
        }
    }

    // --- Promise accessors ---

    /// Get the value of a promise.
    ///
    /// Returns `None` if this is not a promise or the value is null.
    #[inline]
    pub fn prvalue(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::PROMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.promsxp.value })
        } else {
            None
        }
    }

    /// Get the code/expression of a promise.
    ///
    /// Returns `None` if this is not a promise or the code is null.
    #[inline]
    pub fn prcode(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::PROMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.promsxp.expr })
        } else {
            None
        }
    }

    /// Get the environment of a promise.
    ///
    /// Returns `None` if this is not a promise or the environment is null.
    #[inline]
    pub fn prenv(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::PROMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.promsxp.env })
        } else {
            None
        }
    }

    // --- Symbol accessors ---

    /// Get the value of a symbol binding.
    ///
    /// Returns `None` if this is not a symbol or the value is null.
    #[inline]
    pub fn symvalue(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::SYMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.symsxp.internal })
        } else {
            None
        }
    }

    /// Get the print name of a symbol.
    ///
    /// Returns `None` if this is not a symbol or the print name is null.
    #[inline]
    pub fn printname(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::SYMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.symsxp.pname })
        } else {
            None
        }
    }

    // --- Attribute access ---

    /// Get the attributes of this SEXP.
    ///
    /// Returns `None` if there are no attributes.
    #[inline]
    pub fn attrib(self) -> Option<Sexp<'a>> {
        Sexp::from_raw(unsafe { (*self.ptr).attrib })
    }

    /// Check if this object has the OBJECT flag set (has a class attribute).
    ///
    /// S3 and S4 objects have this flag set, triggering method dispatch.
    #[inline]
    pub fn is_object(self) -> bool {
        unsafe { (*self.ptr).sxpinfo.obj() }
    }

    // --- Primitive/Builtin/Special accessors ---

    #[inline]
    pub fn is_special(self) -> bool {
        self.typeof_() == SEXPTYPE::SPECIALSXP
    }

    #[inline]
    pub fn is_builtin(self) -> bool {
        self.typeof_() == SEXPTYPE::BUILTINSXP
    }

    #[inline]
    pub fn is_primitive(self) -> bool {
        let t = self.typeof_();
        t == SEXPTYPE::SPECIALSXP || t == SEXPTYPE::BUILTINSXP
    }

    pub fn primoffset(self) -> Option<c_int> {
        if self.is_primitive() {
            Some(unsafe { (*self.ptr).data.primsxp.offset })
        } else {
            None
        }
    }

    // --- CHARSXP accessors ---

    #[inline]
    pub fn is_charsxp(self) -> bool {
        self.typeof_() == SEXPTYPE::CHARSXP
    }

    pub fn char_len(self) -> Option<R_xlen_t> {
        if self.is_charsxp() {
            Some(unsafe { (*self.ptr).data.charsxp_truelen })
        } else {
            None
        }
    }

    pub fn as_bytes(self) -> Option<&'a [u8]> {
        if self.is_charsxp() {
            let len = unsafe { (*self.ptr).data.charsxp_truelen } as usize;
            let data = unsafe { (*self.ptr).gengc_next_node as *const u8 };
            if len == 0 {
                return Some(&[]);
            }
            if data.is_null() {
                return None;
            }
            Some(unsafe { std::slice::from_raw_parts(data, len) })
        } else {
            None
        }
    }

    pub fn as_str(self) -> Option<&'a str> {
        self.as_bytes().and_then(|b| std::str::from_utf8(b).ok())
    }

    // --- Complex vector accessors ---

    pub fn set_complex_elt(self, i: R_xlen_t, v: Rcomplex) -> bool {
        if !self.valid_index(i) {
            return false;
        }
        let Some(data) = self.typed_data_mut::<Rcomplex>(SEXPTYPE::CPLXSXP) else {
            return false;
        };
        unsafe { *data.add(i as usize) = v };
        true
    }

    pub fn as_complex_slice(self) -> Option<&'a [Rcomplex]> {
        self.typed_slice::<Rcomplex>(SEXPTYPE::CPLXSXP)
    }

    pub fn iter_complex(self) -> impl Iterator<Item = Rcomplex> + 'a {
        self.as_complex_slice().unwrap_or(&[]).iter().copied()
    }

    // --- Dot-dot-dot (DOTSXP) ---

    #[inline]
    pub fn is_dots(self) -> bool {
        self.typeof_() == SEXPTYPE::DOTSXP
    }

    // --- Bytecode (BCODESXP) ---

    #[inline]
    pub fn is_bytecode(self) -> bool {
        self.typeof_() == SEXPTYPE::BCODESXP
    }

    // --- External pointer (EXTPTRSXP) ---

    #[inline]
    pub fn is_extptr(self) -> bool {
        self.typeof_() == SEXPTYPE::EXTPTRSXP
    }

    pub fn extptr_ptr(self) -> Option<*mut std::os::raw::c_void> {
        if self.is_extptr() {
            Some(unsafe { (*self.ptr).data.extptr[0] })
        } else {
            None
        }
    }

    pub fn extptr_tag(self) -> Option<Sexp<'a>> {
        if self.is_extptr() {
            Sexp::from_raw(unsafe { (*self.ptr).data.extptr[1] as SEXP })
        } else {
            None
        }
    }

    pub fn extprot(self) -> Option<Sexp<'a>> {
        if self.is_extptr() {
            Sexp::from_raw(unsafe { (*self.ptr).data.extptr[2] as SEXP })
        } else {
            None
        }
    }

    // --- Weak reference (WEAKREFSXP) ---

    #[inline]
    pub fn is_weakref(self) -> bool {
        self.typeof_() == SEXPTYPE::WEAKREFSXP
    }

    // --- S4 object (OBJSXP) ---

    #[inline]
    pub fn is_s4(self) -> bool {
        self.typeof_() == SEXPTYPE::OBJSXP
    }

    // --- Expression vector (EXPRSXP) ---

    #[inline]
    pub fn is_expression(self) -> bool {
        self.typeof_() == SEXPTYPE::EXPRSXP
    }

    // --- Function (FUNSXP) ---

    #[inline]
    pub fn is_function(self) -> bool {
        let t = self.typeof_();
        t == SEXPTYPE::CLOSXP || t == SEXPTYPE::SPECIALSXP || t == SEXPTYPE::BUILTINSXP
    }

    // --- Data pointer ---

    /// Get the raw data pointer for vector types.
    ///
    /// Returns `None` for non-vector types or if the data pointer is null.
    /// The returned pointer points to the element data buffer (same as
    /// R's `DATAPTR()`).
    #[inline]
    pub fn data_ptr(self) -> Option<*mut std::os::raw::c_void> {
        if self.typeof_().is_vector_type() || self.typeof_() == SEXPTYPE::CHARSXP {
            let ptr = unsafe { (*self.ptr).gengc_next_node as *mut std::os::raw::c_void };
            if ptr.is_null() { None } else { Some(ptr) }
        } else {
            None
        }
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
        let t = self.typeof_();
        write!(f, "Sexp({:?}, len={})", t.0, self.len())
    }
}

// ---------------------------------------------------------------------------
// Mutation methods
// ---------------------------------------------------------------------------

impl<'a> Sexp<'a> {
    /// Set the i-th logical value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    pub fn set_logical_elt(self, i: R_xlen_t, v: c_int) -> bool {
        if !self.valid_index(i) {
            return false;
        }
        let Some(data) = self.typed_data_mut::<c_int>(SEXPTYPE::LGLSXP) else {
            return false;
        };
        unsafe {
            *data.add(i as usize) = v;
        }
        true
    }

    /// Set the i-th integer value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    pub fn set_integer_elt(self, i: R_xlen_t, v: c_int) -> bool {
        if !self.valid_index(i) {
            return false;
        }
        let Some(data) = self.typed_data_mut::<c_int>(SEXPTYPE::INTSXP) else {
            return false;
        };
        unsafe {
            *data.add(i as usize) = v;
        }
        true
    }

    /// Set the i-th real (double) value.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    pub fn set_real_elt(self, i: R_xlen_t, v: c_double) -> bool {
        if !self.valid_index(i) {
            return false;
        }
        let Some(data) = self.typed_data_mut::<c_double>(SEXPTYPE::REALSXP) else {
            return false;
        };
        unsafe {
            *data.add(i as usize) = v;
        }
        true
    }

    /// Set the i-th raw byte.
    ///
    /// Returns `false` if out of bounds, wrong type, or data pointer is null.
    pub fn set_raw_elt(self, i: R_xlen_t, v: Rbyte) -> bool {
        if !self.valid_index(i) {
            return false;
        }
        let Some(data) = self.typed_data_mut::<Rbyte>(SEXPTYPE::RAWSXP) else {
            return false;
        };
        unsafe {
            *data.add(i as usize) = v;
        }
        true
    }

    /// Set the i-th string element.
    ///
    /// Returns `false` if this is not a string vector, `v` is not CHARSXP,
    /// the index is out of bounds, or data pointer is null.
    pub fn set_string_elt(self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        if !self.valid_index(i) || !v.is_charsxp() {
            return false;
        }
        let Some(data) = self.typed_data_mut::<SEXP>(SEXPTYPE::STRSXP) else {
            return false;
        };
        unsafe {
            *data.add(i as usize) = v.as_raw();
        }
        true
    }

    /// Set the i-th vector element.
    ///
    /// Returns `false` if this is not a generic/expression vector, the index is
    /// out of bounds, or data pointer is null.
    pub fn set_vector_elt(self, i: R_xlen_t, v: Sexp<'a>) -> bool {
        if !self.valid_index(i) {
            return false;
        }
        let Some(data) = self.vector_sexp_data_mut() else {
            return false;
        };
        unsafe {
            *data.add(i as usize) = v.as_raw();
        }
        true
    }

    // --- Slice views ---

    /// Get a slice view of the logical data.
    ///
    /// Returns `None` if this is not a logical vector or the data pointer is null.
    /// The slice is valid for the lifetime `'a` of the `Sexp`.
    pub fn as_logical_slice(self) -> Option<&'a [c_int]> {
        self.typed_slice::<c_int>(SEXPTYPE::LGLSXP)
    }

    /// Get a slice view of the integer data.
    ///
    /// Returns `None` if this is not an integer vector or the data pointer is null.
    pub fn as_integer_slice(self) -> Option<&'a [c_int]> {
        self.typed_slice::<c_int>(SEXPTYPE::INTSXP)
    }

    /// Get a slice view of the real (double) data.
    ///
    /// Returns `None` if this is not a real vector or the data pointer is null.
    pub fn as_real_slice(self) -> Option<&'a [c_double]> {
        self.typed_slice::<c_double>(SEXPTYPE::REALSXP)
    }

    /// Get a slice view of the raw byte data.
    ///
    /// Returns `None` if this is not a raw vector or the data pointer is null.
    pub fn as_raw_slice(self) -> Option<&'a [Rbyte]> {
        self.typed_slice::<Rbyte>(SEXPTYPE::RAWSXP)
    }

    // --- Iterators ---

    /// Iterate over logical elements.
    pub fn iter_logical(self) -> impl Iterator<Item = c_int> + 'a {
        self.as_logical_slice().unwrap_or(&[]).iter().copied()
    }

    /// Iterate over integer elements.
    pub fn iter_integer(self) -> impl Iterator<Item = c_int> + 'a {
        self.as_integer_slice().unwrap_or(&[]).iter().copied()
    }

    /// Iterate over real (double) elements.
    pub fn iter_real(self) -> impl Iterator<Item = c_double> + 'a {
        self.as_real_slice().unwrap_or(&[]).iter().copied()
    }

    /// Iterate over raw byte elements.
    pub fn iter_raw(self) -> impl Iterator<Item = Rbyte> + 'a {
        self.as_raw_slice().unwrap_or(&[]).iter().copied()
    }

    /// Iterate over vector elements (for VECSXP/EXPRSXP).
    ///
    /// Null elements are replaced with `R_NilValue`.
    pub fn iter_vector(self) -> impl Iterator<Item = Sexp<'a>> + 'a {
        let data = self.vector_sexp_data();
        let len = if data.is_some() {
            self.len() as usize
        } else {
            0
        };
        (0..len).map(move |i| {
            let ptr = data.map_or(std::ptr::null_mut(), |data| unsafe { *data.add(i) });
            Sexp::from_raw(ptr).unwrap_or_else(|| unsafe {
                Sexp::from_raw_unchecked(super::globals::R_NilValue())
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Pairlist iterator
// ---------------------------------------------------------------------------

/// An iterator over pairlist (LISTSXP/LANGSXP) elements.
///
/// Yields each cons cell in the chain, stopping at `R_NilValue`.
/// Use [`Sexp::car()`] on each yielded item to access the value,
/// and [`Sexp::tag()`] to access the tag/name.
///
/// # Examples
///
/// ```
/// use rmath::sexp::{Sexp, PairlistIter, SEXPTYPE};
/// use rmath::sexp::memory::RArena;
///
/// let mut arena = RArena::new();
/// let list = arena.alloc_list_chain(3);
/// let sexp = Sexp::from_raw(list).expect("from_raw returned None");
/// let items: Vec<_> = PairlistIter::new(sexp).collect();
/// assert_eq!(items.len(), 3);
/// ```
pub struct PairlistIter<'a> {
    current: Option<Sexp<'a>>,
}

impl<'a> PairlistIter<'a> {
    /// Create a new iterator starting from the given pairlist.
    pub fn new(list: Sexp<'a>) -> Self {
        PairlistIter {
            current: Some(list),
        }
    }
}

impl<'a> Iterator for PairlistIter<'a> {
    type Item = Sexp<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;
        if current.is_nil() {
            self.current = None;
            return None;
        }
        let item = current;
        self.current = item.cdr();
        Some(item)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;
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
    fn test_sexp_len_non_vector() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::SYMSXP);
        let sexp = some(Sexp::from_raw(ptr));
        assert_eq!(sexp.len(), 0);
        assert!(sexp.is_empty());
        assert!(sexp.is_symbol());
    }

    #[test]
    fn test_sexp_len_vector() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 5);
        let sexp = some(Sexp::from_raw(ptr));
        assert_eq!(sexp.len(), 5);
        assert!(!sexp.is_empty());
        assert!(sexp.is_vector());
    }

    #[test]
    fn test_sexp_bounds_check() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.integer_elt(5).is_none());
        assert!(sexp.integer_elt(-1).is_none());
        assert!(sexp.integer_elt(0).is_some());
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
        assert!(sexp.set_integer_elt(0, 42));
        assert!(sexp.set_integer_elt(5, 99) == false);
        assert_eq!(sexp.integer_elt(0), Some(42));
    }

    #[test]
    fn test_set_real_elt() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.set_real_elt(0, 3.14));
        assert!(sexp.set_real_elt(5, 99.0) == false);
        assert_eq!(sexp.real_elt(0), Some(3.14));
    }

    #[test]
    fn test_set_raw_elt() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.set_raw_elt(0, 0xFF));
        assert!(sexp.set_raw_elt(5, 0xAA) == false);
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
        set.insert(a);
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
        assert_eq!(sexp.integer_elt(0), Some(0));
        assert!(sexp.set_integer_elt(0, 42));
        assert!(sexp.set_integer_elt(1, -7));
        assert!(sexp.set_integer_elt(2, 99));
        assert_eq!(sexp.integer_elt(0), Some(42));
        assert_eq!(sexp.integer_elt(1), Some(-7));
        assert_eq!(sexp.integer_elt(2), Some(99));
        assert!(!sexp.set_integer_elt(5, 0));
        assert!(sexp.integer_elt(5).is_none());
    }

    #[test]
    fn test_sexp_real_mutation() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.set_real_elt(0, 1.5));
        assert!(sexp.set_real_elt(1, 2.5));
        assert!(sexp.set_real_elt(2, 3.5));
        assert_eq!(sexp.real_elt(0), Some(1.5));
        assert_eq!(sexp.real_elt(1), Some(2.5));
        assert_eq!(sexp.real_elt(2), Some(3.5));
    }

    #[test]
    fn test_sexp_slice_views() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 4);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.set_integer_elt(0, 10));
        assert!(sexp.set_integer_elt(1, 20));
        assert!(sexp.set_integer_elt(2, 30));
        assert!(sexp.set_integer_elt(3, 40));
        let slice = some(sexp.as_integer_slice());
        assert_eq!(slice, &[10, 20, 30, 40]);
    }

    #[test]
    fn test_sexp_real_slice() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let sexp = some(Sexp::from_raw(ptr));
        assert!(sexp.set_real_elt(0, 1.1));
        assert!(sexp.set_real_elt(1, 2.2));
        assert!(sexp.set_real_elt(2, 3.3));
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
            sexp.set_integer_elt(i, (i * 10) as i32);
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
            sexp.set_real_elt(i, i as f64 * 0.5);
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
        assert!(sexp.set_raw_elt(0, 0xDE));
        assert!(sexp.set_raw_elt(1, 0xAD));
        assert!(sexp.set_raw_elt(2, 0xBE));
        assert!(sexp.set_raw_elt(3, 0xEF));
        let slice = some(sexp.as_raw_slice());
        assert_eq!(slice, &[0xDE, 0xAD, 0xBE, 0xEF]);
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
        assert!(sexp.car().is_some());
        assert!(sexp.cdr().is_some());
        assert!(sexp.tag().is_some());
        assert!(some(sexp.car()).is_symbol() == false);
        assert!(some(sexp.tag()).is_symbol());
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
        assert!(sexp.is_closure());
        assert!(sexp.formals().is_some());
        assert!(sexp.body().is_some());
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
        assert!(sexp.is_environment());
        assert!(sexp.frame().is_some());
        assert!(sexp.enclos().is_some());
    }

    #[test]
    fn test_sexp_slice_wrong_type() {
        let mut arena = RArena::new();
        let sym = arena.alloc_node(SEXPTYPE::SYMSXP);
        let sexp = some(Sexp::from_raw(sym));
        assert!(sexp.as_integer_slice().is_none());
        assert!(sexp.as_real_slice().is_none());
        assert!(sexp.as_raw_slice().is_none());
    }

    #[test]
    fn test_atomic_accessors_reject_wrong_vector_type() {
        let mut arena = RArena::new();
        let real = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::REALSXP, 2)));
        let int = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::INTSXP, 2)));
        let logical = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::LGLSXP, 2)));

        assert!(real.integer_elt(0).is_none());
        assert!(real.set_integer_elt(0, 1) == false);
        assert!(real.as_integer_slice().is_none());
        assert!(real.iter_integer().next().is_none());

        assert!(int.real_elt(0).is_none());
        assert!(int.set_real_elt(0, 1.0) == false);
        assert!(int.as_real_slice().is_none());
        assert!(int.iter_real().next().is_none());

        assert!(logical.integer_elt(0).is_none());
        assert!(logical.set_integer_elt(0, 1) == false);
        assert!(logical.as_integer_slice().is_none());
        assert_eq!(logical.as_logical_slice(), Some(&[0, 0][..]));
    }

    #[test]
    fn test_vector_accessors_reject_string_vectors() {
        let mut arena = RArena::new();
        let strings = some(Sexp::from_raw(arena.alloc_vector(SEXPTYPE::STRSXP, 1)));
        let ch = some(Sexp::from_raw(arena.alloc_charsxp(b"x")));
        assert!(strings.set_string_elt(0, ch));
        assert!(strings.string_elt(0).is_some());
        assert!(strings.vector_elt(0).is_none());
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
        assert!(sexp.is_special());
        assert!(sexp.is_primitive());
        assert!(!sexp.is_builtin());
        assert_eq!(sexp.primoffset(), Some(42));

        let builtin = arena.alloc_node(SEXPTYPE::BUILTINSXP);
        unsafe {
            (*builtin).data.primsxp.offset = 7;
        }
        let sexp2 = some(Sexp::from_raw(builtin));
        assert!(sexp2.is_builtin());
        assert!(sexp2.is_primitive());
        assert!(!sexp2.is_special());
        assert_eq!(sexp2.primoffset(), Some(7));

        let other = arena.alloc_node(SEXPTYPE::INTSXP);
        let sexp3 = some(Sexp::from_raw(other));
        assert!(!sexp3.is_primitive());
        assert_eq!(sexp3.primoffset(), None);
    }

    #[test]
    fn test_sexp_charsxp_accessors() {
        let mut arena = RArena::new();
        let charsxp = arena.alloc_charsxp(b"hello world");
        let sexp = some(Sexp::from_raw(charsxp));
        assert!(sexp.is_charsxp());
        assert_eq!(sexp.char_len(), Some(11));
        assert_eq!(sexp.as_bytes(), Some(&b"hello world"[..]));
        assert_eq!(sexp.as_str(), Some("hello world"));

        let other = arena.alloc_node(SEXPTYPE::INTSXP);
        let sexp2 = some(Sexp::from_raw(other));
        assert!(!sexp2.is_charsxp());
        assert!(sexp2.as_bytes().is_none());
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

        assert!(sexp.set_complex_elt(0, c1));
        assert!(sexp.set_complex_elt(1, c2));
        assert!(sexp.set_complex_elt(2, c3));
        assert!(!sexp.set_complex_elt(3, c1)); // out of bounds

        assert_eq!(sexp.complex_elt(0), Some(c1));
        assert_eq!(sexp.complex_elt(1), Some(c2));
        assert_eq!(sexp.complex_elt(2), Some(c3));

        let slice = some(sexp.as_complex_slice());
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
        assert!(sexp.is_dots());

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
        assert!(sexp3.is_extptr());
        assert!(sexp3.extptr_ptr().is_some());
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
