//! Safe builder API for creating R objects.
//!
//! This module provides ergonomic constructors for R objects that avoid
//! the need to call raw FFI functions directly. Each builder type follows
//! a fluent API pattern, allowing chained construction.
//!
//! # Builders
//!
//! - [`IntVector`] — Build integer vectors (INTSXP) from slices, with
//!   support for NA-filled and zero-filled vectors.
//! - [`RealVector`] — Build real/double vectors (REALSXP) from slices,
//!   with support for sequences via [`RealVector::seq`].
//! - [`LogicalVector`] — Build logical vectors (LGLSXP) from boolean slices.
//! - [`RawVector`] — Build raw byte vectors (RAWSXP).
//! - [`StringVector`] — Build character vectors (STRSXP) from string slices.
//! - [`GenericVector`] — Build generic vectors (VECSXP) containing arbitrary
//!   SEXP elements.
//! - [`PairlistBuilder`] — Build pairlist chains (LISTSXP) with optional tags.
//!
//! # Convenience Functions
//!
//! For simple cases, use the top-level `_in` convenience functions with an
//! explicit arena borrow: [`int_vec_in`], [`real_vec_in`], [`logical_vec_in`],
//! [`raw_vec_in`], [`string_vec_in`], and [`seq_in`].
//!
//! # Examples
//!
//! ```text
//! use crate::sexp::builder::{IntVector, RealVector, seq_in};
//! use crate::sexp::memory::RArena;
//!
//! let mut arena = RArena::new();
//!
//! // Using builders
//! let ints = IntVector::new(&[1, 2, 3]).build_in(&mut arena);
//! let reals = RealVector::seq(0.0, 1.0, 0.25).build_in(&mut arena);
//!
//! // Using convenience functions
//! let s = seq_in(&mut arena, 0.0, 2.0, 1.0);
//! ```

use std::os::raw::{c_double, c_int};
use std::ptr;

use super::ffi::{R_xlen_t, Rbyte, SEXP, SEXPTYPE};
use super::globals::R_NilValue;
use super::memory::{self, RArena};
use super::object::{Sexp, SexpError, SexpResult};

// ---------------------------------------------------------------------------
// Builder for integer vectors
// ---------------------------------------------------------------------------

/// Builder for integer vectors (INTSXP).
///
/// Provides a fluent API for constructing integer vectors from Rust data.
/// Supports initialization from slices, NA-filled vectors, and zero-filled vectors.
///
/// # Examples
///
/// ```text
/// use crate::sexp::builder::IntVector;
///
/// let mut arena = rmath::sexp::memory::RArena::new();
/// let vec = IntVector::new(&[1, 2, 3, 4, 5]).build_in(&mut arena);
/// let zeros = IntVector::zeros(10).build_in(&mut arena);
/// let nas = IntVector::with_na(3).build_in(&mut arena);
/// ```
pub struct IntVector {
    values: Vec<c_int>,
}

impl IntVector {
    /// Create a new builder from a slice of integers.
    pub fn new(values: &[c_int]) -> Self {
        IntVector {
            values: values.to_vec(),
        }
    }

    /// Create a new builder with n NA values.
    pub fn with_na(n: usize) -> Self {
        IntVector {
            values: vec![super::ffi::NA_INTEGER; n],
        }
    }

    /// Create a new builder with n zero values.
    pub fn zeros(n: usize) -> Self {
        IntVector { values: vec![0; n] }
    }

    /// Create an integer sequence from `start` to `end`, inclusive.
    ///
    /// Returns `None` if the sequence length cannot be represented by R's
    /// integer vector length contract or if reserving the backing storage fails.
    pub fn sequence(start: c_int, end: c_int) -> Option<Self> {
        let len = (i64::from(end) - i64::from(start)).abs() + 1;
        if len > i64::from(c_int::MAX) {
            return None;
        }
        let len = usize::try_from(len).ok()?;
        let mut values = Vec::new();
        if values.try_reserve_exact(len).is_err() {
            return None;
        }

        let step: c_int = if start <= end { 1 } else { -1 };
        for i in 0..len {
            values.push(start + (i as c_int * step));
        }

        Some(IntVector { values })
    }

    pub fn build_in<'arena>(self, arena: &'arena mut RArena) -> Option<Sexp<'arena>> {
        let len = self.values.len() as R_xlen_t;
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, len);
        if ptr.is_null() {
            return None;
        }
        let data = unsafe { (*ptr).gengc_next_node as *mut c_int };
        if data.is_null() {
            return None;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(self.values.as_ptr(), data, self.values.len());
        }
        arena.sexp(ptr)
    }
}

// ---------------------------------------------------------------------------
// Builder for real vectors
// ---------------------------------------------------------------------------

/// Builder for real (double) vectors (REALSXP).
///
/// Provides a fluent API for constructing numeric vectors from Rust data.
/// Supports initialization from slices, NA-filled vectors, zero-filled vectors,
/// and arithmetic sequences.
///
/// # Examples
///
/// ```text
/// use crate::sexp::builder::RealVector;
///
/// let mut arena = rmath::sexp::memory::RArena::new();
/// let vec = RealVector::new(&[1.5, 2.5, 3.5]).build_in(&mut arena);
/// let seq = RealVector::seq(0.0, 1.0, 0.1).build_in(&mut arena);
/// ```
pub struct RealVector {
    values: Vec<c_double>,
}

impl RealVector {
    /// Create a new builder from a slice of doubles.
    pub fn new(values: &[c_double]) -> Self {
        RealVector {
            values: values.to_vec(),
        }
    }

    /// Create a new builder with n NA values.
    pub fn with_na(n: usize) -> Self {
        RealVector {
            values: vec![super::ffi::NA_REAL; n],
        }
    }

    /// Create a new builder with n zero values.
    pub fn zeros(n: usize) -> Self {
        RealVector {
            values: vec![0.0; n],
        }
    }

    /// Create a sequence from start to end (inclusive) with given step.
    ///
    /// Returns an empty builder if step is zero. For positive steps,
    /// values are generated while `v <= end`. For negative steps,
    /// values are generated while `v >= end`.
    pub fn seq(start: c_double, end: c_double, step: c_double) -> Self {
        if step == 0.0 {
            return RealVector { values: vec![] };
        }
        let mut values = Vec::new();
        let mut v = start;
        if step > 0.0 {
            while v <= end {
                values.push(v);
                v += step;
            }
        } else {
            while v >= end {
                values.push(v);
                v += step;
            }
        }
        RealVector { values }
    }

    pub fn build_in<'arena>(self, arena: &'arena mut RArena) -> Option<Sexp<'arena>> {
        let len = self.values.len() as R_xlen_t;
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, len);
        if ptr.is_null() {
            return None;
        }
        let data = unsafe { (*ptr).gengc_next_node as *mut c_double };
        if data.is_null() {
            return None;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(self.values.as_ptr(), data, self.values.len());
        }
        arena.sexp(ptr)
    }
}

// ---------------------------------------------------------------------------
// Builder for logical vectors
// ---------------------------------------------------------------------------

/// Builder for logical (boolean) vectors (LGLSXP).
///
/// Converts Rust `bool` values to R's logical representation
/// (`1` for `true`, `0` for `false`).
///
/// # Examples
///
/// ```text
/// use crate::sexp::builder::LogicalVector;
///
/// let mut arena = rmath::sexp::memory::RArena::new();
/// let vec = LogicalVector::new(&[true, false, true]).build_in(&mut arena);
/// ```
pub struct LogicalVector {
    values: Vec<c_int>,
}

impl LogicalVector {
    /// Create a new builder from a slice of booleans.
    pub fn new(values: &[bool]) -> Self {
        LogicalVector {
            values: values.iter().map(|&b| if b { 1 } else { 0 }).collect(),
        }
    }

    /// Create a new builder with n NA values.
    pub fn with_na(n: usize) -> Self {
        LogicalVector {
            values: vec![super::ffi::NA_INTEGER; n],
        }
    }

    pub fn build_in<'arena>(self, arena: &'arena mut RArena) -> Option<Sexp<'arena>> {
        let len = self.values.len() as R_xlen_t;
        let ptr = arena.alloc_vector(SEXPTYPE::LGLSXP, len);
        if ptr.is_null() {
            return None;
        }
        let data = unsafe { (*ptr).gengc_next_node as *mut c_int };
        if data.is_null() {
            return None;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(self.values.as_ptr(), data, self.values.len());
        }
        arena.sexp(ptr)
    }
}

// ---------------------------------------------------------------------------
// Builder for raw vectors
// ---------------------------------------------------------------------------

/// Builder for raw byte vectors (RAWSXP).
///
/// # Examples
///
/// ```text
/// use crate::sexp::builder::RawVector;
///
/// let mut arena = rmath::sexp::memory::RArena::new();
/// let vec = RawVector::new(&[0xDE, 0xAD, 0xBE, 0xEF]).build_in(&mut arena);
/// ```
pub struct RawVector {
    values: Vec<Rbyte>,
}

impl RawVector {
    /// Create a new builder from a slice of bytes.
    pub fn new(values: &[Rbyte]) -> Self {
        RawVector {
            values: values.to_vec(),
        }
    }

    /// Create a new builder with n zero bytes.
    pub fn zeros(n: usize) -> Self {
        RawVector { values: vec![0; n] }
    }

    pub fn build_in<'arena>(self, arena: &'arena mut RArena) -> Option<Sexp<'arena>> {
        let len = self.values.len() as R_xlen_t;
        let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, len);
        if ptr.is_null() {
            return None;
        }
        let data = unsafe { (*ptr).gengc_next_node as *mut Rbyte };
        if data.is_null() {
            return None;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(self.values.as_ptr(), data, self.values.len());
        }
        arena.sexp(ptr)
    }
}

// ---------------------------------------------------------------------------
// Builder for character vectors (strings)
// ---------------------------------------------------------------------------

/// Builder for character vectors (STRSXP).
///
/// Each element in the resulting vector is a CHARSXP object.
///
/// # Examples
///
/// ```text
/// use crate::sexp::builder::StringVector;
///
/// let mut arena = rmath::sexp::memory::RArena::new();
/// let vec = StringVector::new(&["hello", "world"]).build_in(&mut arena);
/// ```
pub struct StringVector {
    values: Vec<String>,
}

impl StringVector {
    /// Create a new builder from a slice of string slices.
    pub fn new(values: &[&str]) -> Self {
        StringVector {
            values: values.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn build_in<'arena>(self, arena: &'arena mut RArena) -> Option<Sexp<'arena>> {
        let len = self.values.len() as R_xlen_t;
        let ptr = arena.alloc_vector(SEXPTYPE::STRSXP, len);
        if ptr.is_null() {
            return None;
        }
        for (i, s) in self.values.iter().enumerate() {
            let charsxp = arena.alloc_charsxp(s.as_bytes());
            arena.set_reference_element(ptr, i, charsxp)?;
        }
        arena.sexp(ptr)
    }
}

// ---------------------------------------------------------------------------
// Builder for generic vectors (VECSXP)
// ---------------------------------------------------------------------------

/// Builder for generic vectors (VECSXP) containing arbitrary SEXP elements.
///
/// Use [`GenericVector::with_length`] to create a vector of a given size,
/// then chain [`GenericVector::set_value`] calls to populate elements from
/// owner-scoped [`Sexp`] handles.
///
/// # Examples
///
/// ```text
/// use crate::sexp::builder::GenericVector;
/// use crate::sexp::Sexp;
///
/// let mut arena = rmath::sexp::memory::RArena::new();
/// let nil = Sexp::nil();
/// let vec = GenericVector::with_length(2)
///     .set_value(0, nil)
///     .build_in(&mut arena);
/// ```
pub struct GenericVector<'a> {
    elements: Vec<Sexp<'a>>,
}

impl<'a> GenericVector<'a> {
    pub fn with_length(n: usize) -> Self {
        Self {
            elements: vec![Sexp::nil(); n],
        }
    }

    pub fn from_values(values: impl IntoIterator<Item = Sexp<'a>>) -> Self {
        Self {
            elements: values.into_iter().collect(),
        }
    }

    pub fn set_value(mut self, index: usize, value: Sexp<'a>) -> Self {
        if let Some(slot) = self.elements.get_mut(index) {
            *slot = value;
        }
        self
    }

    pub fn try_set_value(mut self, index: usize, value: Sexp<'a>) -> SexpResult<Self> {
        let len = self.elements.len() as R_xlen_t;
        let slot = self.elements.get_mut(index).ok_or(SexpError::OutOfBounds {
                index: index as R_xlen_t,
                len,
        })?;
        *slot = value;
        Ok(self)
    }

    pub fn build_in<'arena>(self, arena: &'arena mut RArena) -> Option<Sexp<'arena>> {
        if self.elements.iter().any(|v| !arena.accepts_child(v)) {
            return None;
        }
        let ptr = arena.alloc_vector(SEXPTYPE::VECSXP, self.elements.len() as R_xlen_t);
        if ptr.is_null() {
            return None;
        }
        for (i, value) in self.elements.iter().enumerate() {
            arena.set_reference_element(ptr, i, value.clone().as_raw())?;
        }
        arena.sexp(ptr)
    }
}

// ---------------------------------------------------------------------------
// Builder for pairlists
// ---------------------------------------------------------------------------

/// Builder for pairlists (LISTSXP chains).
///
/// Constructs a linked list of cons cells. Elements are added in order
/// and the chain is terminated with `R_NilValue`.
///
/// # Examples
///
/// ```text
/// use crate::sexp::builder::PairlistBuilder;
/// use crate::sexp::memory::RArena;
/// use crate::sexp::Sexp;
///
/// let mut arena = RArena::new();
/// let nil = Sexp::nil();
/// let list = PairlistBuilder::new()
///     .push_untagged_value(nil)
///     .build_in(&mut arena);
/// ```
pub struct PairlistBuilder<'a> {
    elements: Vec<(Sexp<'a>, Option<Sexp<'a>>)>,
}

impl<'a> PairlistBuilder<'a> {
    pub fn new() -> Self {
        Self {
            elements: Vec::new(),
        }
    }

    pub fn from_untagged_values(values: impl IntoIterator<Item = Sexp<'a>>) -> Self {
        values
            .into_iter()
            .fold(Self::new(), Self::push_untagged_value)
    }

    pub fn push_value(mut self, car: Sexp<'a>, tag: Option<Sexp<'a>>) -> Self {
        self.elements.push((car, tag));
        self
    }

    pub fn push_untagged_value(self, car: Sexp<'a>) -> Self {
        self.push_value(car, None)
    }

    pub fn build_in<'arena>(self, arena: &'arena mut RArena) -> Option<Sexp<'arena>> {
        if self.elements.iter().any(|(car, tag)| {
            !arena.accepts_child(car) || tag.as_ref().is_some_and(|tag| !arena.accepts_child(tag))
        }) {
            return None;
        }
        let mut result = unsafe { R_NilValue() };
        for (car, tag) in self.elements.iter().rev() {
            // SAFETY: every child was checked above; no GC runs during this arena lend.
            result = unsafe {
                arena.cons(car.clone().as_raw(), result, tag.as_ref()
                        .map_or(ptr::null_mut(), |tag| tag.clone().as_raw()),
                )
            };
            if result.is_null() {
                return None;
            }
        }
        arena.sexp(result)
    }
}

impl Default for PairlistBuilder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Convenience functions
// ---------------------------------------------------------------------------

pub fn int_vec_in<'arena>(arena: &'arena mut RArena, values: &[c_int]) -> Option<Sexp<'arena>> {
    IntVector::new(values).build_in(arena)
}

pub(crate) fn int_sequence_current(start: c_int, end: c_int) -> Option<SEXP> {
    unsafe {
        /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
        memory::with_arena(|arena| {
        IntVector::sequence(start, end)
            .and_then(|builder| builder.build_in(arena))
            .map(|value| value.as_raw())
    })
    }
}

pub fn real_vec_in<'arena>(arena: &'arena mut RArena, values: &[c_double]) -> Option<Sexp<'arena>> {
    RealVector::new(values).build_in(arena)
}

pub fn logical_vec_in<'arena>(arena: &'arena mut RArena, values: &[bool]) -> Option<Sexp<'arena>> {
    LogicalVector::new(values).build_in(arena)
}

pub fn raw_vec_in<'arena>(arena: &'arena mut RArena, values: &[Rbyte]) -> Option<Sexp<'arena>> {
    RawVector::new(values).build_in(arena)
}

pub fn string_vec_in<'arena>(arena: &'arena mut RArena, values: &[&str]) -> Option<Sexp<'arena>> {
    StringVector::new(values).build_in(arena)
}

pub fn seq_in<'arena>(
    arena: &'arena mut RArena,
    start: f64,
    end: f64,
    step: f64,
) -> Option<Sexp<'arena>> {
    RealVector::seq(start, end, step).build_in(arena)
}

// ---------------------------------------------------------------------------
// Safe scalar constructors
// ---------------------------------------------------------------------------

pub fn scalar_integer_in<'arena>(arena: &'arena mut RArena, x: c_int) -> Option<Sexp<'arena>> {
    let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
    if ptr.is_null() {
        return None;
    }
    let data = unsafe { (*ptr).gengc_next_node as *mut c_int };
    if data.is_null() {
        return None;
    }
    unsafe { *data = x };
    unsafe {
        (*ptr).sxpinfo.set_scalar(true);
    }
    arena.sexp(ptr)
}

pub fn scalar_real_in<'arena>(arena: &'arena mut RArena, x: c_double) -> Option<Sexp<'arena>> {
    let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
    if ptr.is_null() {
        return None;
    }
    let data = unsafe { (*ptr).gengc_next_node as *mut c_double };
    if data.is_null() {
        return None;
    }
    unsafe { *data = x };
    unsafe {
        (*ptr).sxpinfo.set_scalar(true);
    }
    arena.sexp(ptr)
}

pub fn scalar_logical_in<'arena>(arena: &'arena mut RArena, x: c_int) -> Option<Sexp<'arena>> {
    let ptr = arena.alloc_vector(SEXPTYPE::LGLSXP, 1);
    if ptr.is_null() {
        return None;
    }
    let data = unsafe { (*ptr).gengc_next_node as *mut c_int };
    if data.is_null() {
        return None;
    }
    unsafe { *data = x };
    unsafe {
        (*ptr).sxpinfo.set_scalar(true);
    }
    arena.sexp(ptr)
}

pub fn scalar_raw_in<'arena>(arena: &'arena mut RArena, x: Rbyte) -> Option<Sexp<'arena>> {
    let ptr = arena.alloc_vector(SEXPTYPE::RAWSXP, 1);
    if ptr.is_null() {
        return None;
    }
    let data = unsafe { (*ptr).gengc_next_node as *mut Rbyte };
    if data.is_null() {
        return None;
    }
    unsafe { *data = x };
    unsafe {
        (*ptr).sxpinfo.set_scalar(true);
    }
    arena.sexp(ptr)
}

pub fn scalar_string_in<'arena>(arena: &'arena mut RArena, s: &str) -> Option<Sexp<'arena>> {
    let ptr = arena.alloc_vector(SEXPTYPE::STRSXP, 1);
    if ptr.is_null() {
        return None;
    }
    let charsxp = arena.alloc_charsxp(s.as_bytes());
    arena.set_reference_element(ptr, 0, charsxp)?;
    arena.sexp(ptr)
}

pub fn scalar_bytes_in<'arena>(arena: &'arena mut RArena, bytes: &[u8]) -> Option<Sexp<'arena>> {
    let ptr = arena.alloc_vector(SEXPTYPE::STRSXP, 1);
    if ptr.is_null() {
        return None;
    }
    let charsxp = arena.alloc_charsxp(bytes);
    arena.set_reference_element(ptr, 0, charsxp)?;
    arena.sexp(ptr)
}

pub fn scalar_complex_in<'arena>(
    arena: &'arena mut RArena,
    r: c_double,
    i: c_double,
) -> Option<Sexp<'arena>> {
    let ptr = arena.alloc_vector(SEXPTYPE::CPLXSXP, 1);
    if ptr.is_null() {
        return None;
    }
    let data = unsafe { (*ptr).gengc_next_node as *mut super::ffi::Rcomplex };
    if data.is_null() {
        return None;
    }
    unsafe {
        *data = super::ffi::Rcomplex { r, i };
    }
    unsafe {
        (*ptr).sxpinfo.set_scalar(true);
    }
    arena.sexp(ptr)
}

pub fn mk_char_in<'arena>(arena: &'arena mut RArena, s: &[u8]) -> Option<Sexp<'arena>> {
    arena.alloc_charsxp_sexp(s)
}

// ---------------------------------------------------------------------------
// Safe language/pairlist constructors
// ---------------------------------------------------------------------------

pub fn cons_in<'arena>(
    arena: &'arena mut RArena,
    car: Sexp<'_>,
    cdr: Sexp<'_>,
    tag: Option<Sexp<'_>>,
) -> Option<Sexp<'arena>> {
    arena.cons_sexp(car, cdr, tag)
}

pub fn lang2_in<'arena>(
    arena: &'arena mut RArena,
    car: Sexp<'_>,
    arg: Sexp<'_>,
) -> Option<Sexp<'arena>> {
    if !arena.accepts_child(&car) || !arena.accepts_child(&arg) {
        return None;
    }
    let cdr = arena.alloc_node(SEXPTYPE::LANGSXP);
    if cdr.is_null() {
        return None;
    }
    unsafe {
        (*cdr).data.list_mut().carval = arg.clone().as_raw();
        (*cdr).data.list_mut().cdrval = R_NilValue();
        (*cdr).data.list_mut().tagval = ptr::null_mut();
    }
    let head = arena.alloc_node(SEXPTYPE::LANGSXP);
    if head.is_null() {
        return None;
    }
    unsafe {
        (*head).data.list_mut().carval = car.as_raw();
        (*head).data.list_mut().cdrval = cdr;
        (*head).data.list_mut().tagval = ptr::null_mut();
    }
    arena.sexp(head)
}

pub fn lang3_in<'arena>(
    arena: &'arena mut RArena,
    car: Sexp<'_>,
    arg1: Sexp<'_>,
    arg2: Sexp<'_>,
) -> Option<Sexp<'arena>> {
    if !arena.accepts_child(&car) || !arena.accepts_child(&arg1) || !arena.accepts_child(&arg2) {
        return None;
    }
    let c2 = arena.alloc_node(SEXPTYPE::LANGSXP);
    if c2.is_null() {
        return None;
    }
    unsafe {
        (*c2).data.list_mut().carval = arg2.clone().as_raw();
        (*c2).data.list_mut().cdrval = R_NilValue();
        (*c2).data.list_mut().tagval = ptr::null_mut();
    }
    let c1 = arena.alloc_node(SEXPTYPE::LANGSXP);
    if c1.is_null() {
        return None;
    }
    unsafe {
        (*c1).data.list_mut().carval = arg1.clone().as_raw();
        (*c1).data.list_mut().cdrval = c2;
        (*c1).data.list_mut().tagval = ptr::null_mut();
    }
    let head = arena.alloc_node(SEXPTYPE::LANGSXP);
    if head.is_null() {
        return None;
    }
    unsafe {
        (*head).data.list_mut().carval = car.as_raw();
        (*head).data.list_mut().cdrval = c1;
        (*head).data.list_mut().tagval = ptr::null_mut();
    }
    arena.sexp(head)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn some<T>(opt: Option<T>) -> T {
        opt.unwrap_or_else(|| panic!("unexpected None in test"))
    }

    #[test]
    fn test_int_vector_builder() {
        let mut arena = RArena::new();
        let vec = some(IntVector::new(&[1, 2, 3]).build_in(&mut arena));
        assert!(vec.clone().is_vector());
        assert_eq!(vec.clone().len(), 3);
        assert_eq!(vec.clone().integer_elt(0), Some(1));
        assert_eq!(vec.clone().integer_elt(1), Some(2));
        assert_eq!(vec.integer_elt(2), Some(3));
    }

    #[test]
    fn test_int_vector_zeros() {
        let mut arena = RArena::new();
        let vec = some(IntVector::zeros(5).build_in(&mut arena));
        assert_eq!(vec.clone().len(), 5);
        for i in 0..5 {
            assert_eq!(vec.clone().integer_elt(i as R_xlen_t), Some(0));
        }
    }

    #[test]
    fn test_int_vector_na() {
        let mut arena = RArena::new();
        let vec = some(IntVector::with_na(3).build_in(&mut arena));
        assert_eq!(vec.clone().len(), 3);
        for i in 0..3 {
            assert_eq!(
                vec.clone().integer_elt(i as R_xlen_t),
                Some(super::super::ffi::NA_INTEGER)
            );
        }
    }

    #[test]
    fn test_int_vector_sequence() {
        let mut arena = RArena::new();
        let asc = some(some(IntVector::sequence(2, 4)).build_in(&mut arena));
        assert_eq!(asc.clone().len(), 3);
        assert_eq!(asc.clone().integer_elt(0), Some(2));
        assert_eq!(asc.clone().integer_elt(1), Some(3));
        assert_eq!(asc.integer_elt(2), Some(4));
        drop(asc);

        let desc = some(some(IntVector::sequence(1, -1)).build_in(&mut arena));
        assert_eq!(desc.clone().len(), 3);
        assert_eq!(desc.clone().integer_elt(0), Some(1));
        assert_eq!(desc.clone().integer_elt(1), Some(0));
        assert_eq!(desc.integer_elt(2), Some(-1));
    }

    #[test]
    fn test_real_vector_builder() {
        let mut arena = RArena::new();
        let vec = some(RealVector::new(&[1.5, 2.5, 3.5]).build_in(&mut arena));
        assert_eq!(vec.clone().len(), 3);
        assert!((some(vec.clone().real_elt(0)) - 1.5).abs() < f64::EPSILON);
        assert!((some(vec.clone().real_elt(1)) - 2.5).abs() < f64::EPSILON);
        assert!((some(vec.real_elt(2)) - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_real_vector_seq() {
        let mut arena = RArena::new();
        let vec = some(RealVector::seq(0.0, 1.0, 0.25).build_in(&mut arena));
        assert_eq!(vec.clone().len(), 5);
        assert!((some(vec.clone().real_elt(0)) - 0.0).abs() < f64::EPSILON);
        assert!((some(vec.real_elt(4)) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_logical_vector_builder() {
        let mut arena = RArena::new();
        let vec = some(LogicalVector::new(&[true, false, true]).build_in(&mut arena));
        assert_eq!(vec.clone().len(), 3);
        assert_eq!(vec.clone().logical_elt(0), Some(1));
        assert_eq!(vec.clone().logical_elt(1), Some(0));
        assert_eq!(vec.logical_elt(2), Some(1));
    }

    #[test]
    fn test_raw_vector_builder() {
        let mut arena = RArena::new();
        let vec = some(RawVector::new(&[0xDE, 0xAD, 0xBE, 0xEF]).build_in(&mut arena));
        assert_eq!(vec.clone().len(), 4);
        assert_eq!(vec.clone().raw_elt(0), Some(0xDE));
        assert_eq!(vec.clone().raw_elt(1), Some(0xAD));
        assert_eq!(vec.clone().raw_elt(2), Some(0xBE));
        assert_eq!(vec.raw_elt(3), Some(0xEF));
    }

    #[test]
    fn test_string_vector_builder() {
        let mut arena = RArena::new();
        let vec = some(StringVector::new(&["hello", "world"]).build_in(&mut arena));
        assert_eq!(vec.clone().len(), 2);
        assert!(vec.clone().string_elt(0).is_some());
        assert!(vec.string_elt(1).is_some());
    }

    #[test]
    fn test_generic_vector_builder() {
        let mut arena = RArena::new();
        let nil = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(unsafe { R_NilValue() })
        });
        let vec = some(GenericVector::from_values([nil.clone(), nil]).build_in(&mut arena));
        assert_eq!(vec.clone().len(), 2);
        assert!(vec.clone().vector_elt(0).is_some());
        assert!(vec.vector_elt(1).is_some());
    }

    #[test]
    fn test_generic_vector_typed_set() {
        let mut arena = RArena::new();
        let nil = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(unsafe { R_NilValue() })
        });
        let vec = some(
            GenericVector::with_length(1)
                .set_value(0, nil)
                .build_in(&mut arena),
        );
        assert_eq!(vec.clone().len(), 1);
        assert!(vec.vector_elt(0).is_some_and(|sexp| sexp.is_nil()));
    }

    #[test]
    fn test_generic_vector_typed_set_reports_bounds() {
        let nil = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(unsafe { R_NilValue() })
        });
        let builder = GenericVector::with_length(1)
            .try_set_value(0, nil.clone())
            .expect("in-bounds typed set should succeed");

        assert!(matches!(
            builder.try_set_value(2, nil),
            Err(SexpError::OutOfBounds { index: 2, len: 1 })
        ));
    }

    #[test]
    fn test_pairlist_builder() {
        let mut arena = crate::sexp::memory::RArena::new();
        let nil = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(unsafe { R_NilValue() })
        });
        let list =
            some(PairlistBuilder::from_untagged_values([nil.clone(), nil]).build_in(&mut arena));
        assert!(list.clone().is_pairlist());
        assert!(list.clone().car().is_some());
        assert!(list.cdr().is_some());
    }

    #[test]
    fn test_pairlist_builder_accepts_typed_values() {
        let nil = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(unsafe { R_NilValue() })
        });
        let mut arena = crate::sexp::memory::RArena::new();
        let list = some(
            PairlistBuilder::new()
                .push_untagged_value(nil)
                .build_in(&mut arena),
        );

        assert!(list.clone().is_pairlist());
        assert!(list.car().is_some());
    }

    #[test]
    fn test_convenience_functions() {
        let mut arena = RArena::new();
        let v1 = some(int_vec_in(&mut arena, &[10, 20, 30]));
        assert_eq!(v1.integer_elt(0), Some(10));

        let mut arena = RArena::new();
        let v2 = some(real_vec_in(&mut arena, &[1.0, 2.0]));
        assert!((some(v2.real_elt(0)) - 1.0).abs() < f64::EPSILON);

        let mut arena = RArena::new();
        let v3 = some(logical_vec_in(&mut arena, &[true, false]));
        assert_eq!(v3.logical_elt(0), Some(1));

        let mut arena = RArena::new();
        let v4 = some(raw_vec_in(&mut arena, &[0xFF]));
        assert_eq!(v4.raw_elt(0), Some(0xFF));

        let mut arena = RArena::new();
        let v5 = some(string_vec_in(&mut arena, &["test"]));
        assert_eq!(v5.len(), 1);

        let mut arena = RArena::new();
        let v6 = some(seq_in(&mut arena, 0.0, 2.0, 1.0));
        assert_eq!(v6.len(), 3);
    }

    #[test]
    fn test_scalar_constructors() {
        let mut arena = RArena::new();
        let si = some(scalar_integer_in(&mut arena, 42));
        assert_eq!(si.clone().integer_elt(0), Some(42));
        assert_eq!(si.len(), 1);

        let mut arena = RArena::new();
        let sr = some(scalar_real_in(&mut arena, 3.14));
        assert!((some(sr.real_elt(0)) - 3.14).abs() < f64::EPSILON);

        let mut arena = RArena::new();
        let sl = some(scalar_logical_in(&mut arena, 1));
        assert_eq!(sl.logical_elt(0), Some(1));

        let mut arena = RArena::new();
        let sraw = some(scalar_raw_in(&mut arena, 0xAB));
        assert_eq!(sraw.raw_elt(0), Some(0xAB));

        let mut arena = RArena::new();
        let ss = some(scalar_string_in(&mut arena, "hello"));
        assert_eq!(ss.clone().len(), 1);
        assert!(ss.string_elt(0).is_some());

        let mut arena = RArena::new();
        let sc = some(scalar_complex_in(&mut arena, 1.0, 2.0));
        let c = some(sc.complex_elt(0));
        assert!((c.r - 1.0).abs() < f64::EPSILON);
        assert!((c.i - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_mk_char() {
        let mut arena = RArena::new();
        let c = some(mk_char_in(&mut arena, b"hello"));
        assert!(c.clone().is_charsxp());
        assert_eq!(c.as_string().as_deref(), Some("hello"));
        assert_eq!(c.as_string().map(String::into_bytes), Some(b"hello".to_vec()));
    }

    #[test]
    fn test_cons_constructor() {
        let mut arena = RArena::new();
        let car = {
            let car = some(scalar_integer_in(&mut arena, 1));
            car.as_raw()
        };
        let cdr = {
            let cdr = some(scalar_real_in(&mut arena, 2.0));
            cdr.as_raw()
        };
        let cell = some(cons_in(
            &mut arena,
            some(unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                Sexp::from_raw(car)
            }),
            some(unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                Sexp::from_raw(cdr)
            }),
            None,
        ));
        assert!(cell.clone().is_pairlist());
        assert!(some(cell.car()).is_vector());
    }

    #[test]
    fn test_lang_constructors() {
        let mut arena = crate::sexp::memory::RArena::new();
        let sym = arena.alloc_node(SEXPTYPE::SYMSXP);
        let fun = some(unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            Sexp::from_raw(sym)
        });
        let arg = {
            let arg = some(scalar_integer_in(&mut arena, 1));
            some(unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                Sexp::from_raw(arg.clone().as_raw())
            })
        };

        let call = some(lang2_in(&mut arena, fun.clone(), arg.clone()));
        assert!(call.is_pairlist());
        drop(call);

        let arg2 = {
            let arg2 = some(scalar_real_in(&mut arena, 2.0));
            some(unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                Sexp::from_raw(arg2.clone().as_raw())
            })
        };
        let call3 = some(lang3_in(&mut arena, fun, arg, arg2));
        assert!(call3.clone().is_pairlist());
        assert!(call3.car().is_some());
    }
}
