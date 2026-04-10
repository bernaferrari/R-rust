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
//! For simple cases, use the top-level convenience functions:
//! [`int_vec`], [`real_vec`], [`logical_vec`], [`raw_vec`], [`string_vec`],
//! and [`seq`].
//!
//! # Examples
//!
//! ```
//! use rmath::sexp::builder::{IntVector, RealVector, seq};
//!
//! // Using builders
//! let ints = IntVector::new(&[1, 2, 3]).build();
//! let reals = RealVector::seq(0.0, 1.0, 0.25).build();
//!
//! // Using convenience functions
//! let s = seq(0.0, 2.0, 1.0);
//! ```

use std::os::raw::{c_double, c_int};
use std::ptr;

use super::ffi::{R_xlen_t, Rbyte, SEXP, SEXPTYPE};
use super::globals::R_NilValue;
use super::memory::{RArena, with_arena};
use super::safe::Sexp;

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
/// ```
/// use rmath::sexp::builder::IntVector;
///
/// let vec = IntVector::new(&[1, 2, 3, 4, 5]).build();
/// let zeros = IntVector::zeros(10).build();
/// let nas = IntVector::with_na(3).build();
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

    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| self.build_in(arena))
    }

    pub fn build_in(self, arena: &mut RArena) -> Option<Sexp<'static>> {
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
        Sexp::from_raw(ptr)
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
/// ```
/// use rmath::sexp::builder::RealVector;
///
/// let vec = RealVector::new(&[1.5, 2.5, 3.5]).build();
/// let seq = RealVector::seq(0.0, 1.0, 0.1).build();
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

    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| self.build_in(arena))
    }

    pub fn build_in(self, arena: &mut RArena) -> Option<Sexp<'static>> {
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
        Sexp::from_raw(ptr)
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
/// ```
/// use rmath::sexp::builder::LogicalVector;
///
/// let vec = LogicalVector::new(&[true, false, true]).build();
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

    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| self.build_in(arena))
    }

    pub fn build_in(self, arena: &mut RArena) -> Option<Sexp<'static>> {
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
        Sexp::from_raw(ptr)
    }
}

// ---------------------------------------------------------------------------
// Builder for raw vectors
// ---------------------------------------------------------------------------

/// Builder for raw byte vectors (RAWSXP).
///
/// # Examples
///
/// ```
/// use rmath::sexp::builder::RawVector;
///
/// let vec = RawVector::new(&[0xDE, 0xAD, 0xBE, 0xEF]).build();
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

    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| self.build_in(arena))
    }

    pub fn build_in(self, arena: &mut RArena) -> Option<Sexp<'static>> {
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
        Sexp::from_raw(ptr)
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
/// ```
/// use rmath::sexp::builder::StringVector;
///
/// let vec = StringVector::new(&["hello", "world"]).build();
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

    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| self.build_in(arena))
    }

    pub fn build_in(self, arena: &mut RArena) -> Option<Sexp<'static>> {
        let len = self.values.len() as R_xlen_t;
        let ptr = arena.alloc_vector(SEXPTYPE::STRSXP, len);
        if ptr.is_null() {
            return None;
        }
        let data = unsafe { (*ptr).gengc_next_node as *mut SEXP };
        if data.is_null() {
            return None;
        }
        for (i, s) in self.values.iter().enumerate() {
            let charsxp = arena.alloc_charsxp(s.as_bytes());
            unsafe {
                *data.add(i) = charsxp;
            }
        }
        Sexp::from_raw(ptr)
    }
}

// ---------------------------------------------------------------------------
// Builder for generic vectors (VECSXP)
// ---------------------------------------------------------------------------

/// Builder for generic vectors (VECSXP) containing arbitrary SEXP elements.
///
/// Use [`GenericVector::with_length`] to create a vector of a given size,
/// then chain [`GenericVector::set`] calls to populate elements.
///
/// # Examples
///
/// ```
/// use rmath::sexp::builder::{GenericVector, IntVector};
///
/// let int_v = IntVector::new(&[1, 2]).build().unwrap_or_else(|| panic!("failed to build IntVector"));
/// let vec = GenericVector::with_length(2)
///     .set(0, int_v.as_raw())
///     .build();
/// ```
pub struct GenericVector {
    elements: Vec<SEXP>,
}

impl GenericVector {
    /// Create a new builder with n null elements.
    pub fn with_length(n: usize) -> Self {
        GenericVector {
            elements: vec![ptr::null_mut(); n],
        }
    }

    /// Set the element at the given index.
    ///
    /// Silently ignores indices that are out of bounds.
    pub fn set(mut self, index: usize, value: SEXP) -> Self {
        if index < self.elements.len() {
            self.elements[index] = value;
        }
        self
    }

    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| self.build_in(arena))
    }

    pub fn build_in(self, arena: &mut RArena) -> Option<Sexp<'static>> {
        let len = self.elements.len() as R_xlen_t;
        let ptr = arena.alloc_vector(SEXPTYPE::VECSXP, len);
        if ptr.is_null() {
            return None;
        }
        let data = unsafe { (*ptr).gengc_next_node as *mut SEXP };
        if data.is_null() {
            return None;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(self.elements.as_ptr(), data, self.elements.len());
        }
        Sexp::from_raw(ptr)
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
/// ```
/// use rmath::sexp::builder::PairlistBuilder;
/// use rmath::sexp::memory::RArena;
/// use rmath::sexp::SEXPTYPE;
///
/// let mut arena = RArena::new();
/// let a = arena.alloc_node(SEXPTYPE::INTSXP);
/// let list = PairlistBuilder::new()
///     .push_untagged(a)
///     .build();
/// ```
pub struct PairlistBuilder {
    elements: Vec<(SEXP, SEXP)>, // (car, tag) pairs
}

impl PairlistBuilder {
    /// Create a new empty pairlist builder.
    pub fn new() -> Self {
        PairlistBuilder {
            elements: Vec::new(),
        }
    }

    /// Add an element with an optional tag.
    pub fn push(mut self, car: SEXP, tag: SEXP) -> Self {
        self.elements.push((car, tag));
        self
    }

    /// Add an untagged element.
    pub fn push_untagged(self, car: SEXP) -> Self {
        self.push(car, ptr::null_mut())
    }

    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| self.build_in(arena))
    }

    pub fn build_in(self, arena: &mut RArena) -> Option<Sexp<'static>> {
        let mut result: SEXP = ptr::null_mut();
        for (car, tag) in self.elements.into_iter().rev() {
            result = arena.cons(car, result, tag);
        }
        Sexp::from_raw(result)
    }
}

impl Default for PairlistBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Convenience functions
// ---------------------------------------------------------------------------

pub fn int_vec(values: &[c_int]) -> Option<Sexp<'static>> {
    IntVector::new(values).build()
}

pub fn int_vec_in(arena: &mut RArena, values: &[c_int]) -> Option<Sexp<'static>> {
    IntVector::new(values).build_in(arena)
}

pub fn real_vec(values: &[c_double]) -> Option<Sexp<'static>> {
    RealVector::new(values).build()
}

pub fn real_vec_in(arena: &mut RArena, values: &[c_double]) -> Option<Sexp<'static>> {
    RealVector::new(values).build_in(arena)
}

pub fn logical_vec(values: &[bool]) -> Option<Sexp<'static>> {
    LogicalVector::new(values).build()
}

pub fn logical_vec_in(arena: &mut RArena, values: &[bool]) -> Option<Sexp<'static>> {
    LogicalVector::new(values).build_in(arena)
}

pub fn raw_vec(values: &[Rbyte]) -> Option<Sexp<'static>> {
    RawVector::new(values).build()
}

pub fn raw_vec_in(arena: &mut RArena, values: &[Rbyte]) -> Option<Sexp<'static>> {
    RawVector::new(values).build_in(arena)
}

pub fn string_vec(values: &[&str]) -> Option<Sexp<'static>> {
    StringVector::new(values).build()
}

pub fn string_vec_in(arena: &mut RArena, values: &[&str]) -> Option<Sexp<'static>> {
    StringVector::new(values).build_in(arena)
}

pub fn seq(start: f64, end: f64, step: f64) -> Option<Sexp<'static>> {
    RealVector::seq(start, end, step).build()
}

pub fn seq_in(arena: &mut RArena, start: f64, end: f64, step: f64) -> Option<Sexp<'static>> {
    RealVector::seq(start, end, step).build_in(arena)
}

// ---------------------------------------------------------------------------
// Safe scalar constructors
// ---------------------------------------------------------------------------

pub fn scalar_integer(x: c_int) -> Option<Sexp<'static>> {
    with_arena(|arena| scalar_integer_in(arena, x))
}

pub fn scalar_integer_in(arena: &mut RArena, x: c_int) -> Option<Sexp<'static>> {
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
    Sexp::from_raw(ptr)
}

pub fn scalar_real(x: c_double) -> Option<Sexp<'static>> {
    with_arena(|arena| scalar_real_in(arena, x))
}

pub fn scalar_real_in(arena: &mut RArena, x: c_double) -> Option<Sexp<'static>> {
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
    Sexp::from_raw(ptr)
}

pub fn scalar_logical(x: c_int) -> Option<Sexp<'static>> {
    with_arena(|arena| scalar_logical_in(arena, x))
}

pub fn scalar_logical_in(arena: &mut RArena, x: c_int) -> Option<Sexp<'static>> {
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
    Sexp::from_raw(ptr)
}

pub fn scalar_raw(x: Rbyte) -> Option<Sexp<'static>> {
    with_arena(|arena| scalar_raw_in(arena, x))
}

pub fn scalar_raw_in(arena: &mut RArena, x: Rbyte) -> Option<Sexp<'static>> {
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
    Sexp::from_raw(ptr)
}

pub fn scalar_string(s: &str) -> Option<Sexp<'static>> {
    with_arena(|arena| scalar_string_in(arena, s))
}

pub fn scalar_string_in(arena: &mut RArena, s: &str) -> Option<Sexp<'static>> {
    let ptr = arena.alloc_vector(SEXPTYPE::STRSXP, 1);
    if ptr.is_null() {
        return None;
    }
    let data = unsafe { (*ptr).gengc_next_node as *mut SEXP };
    if data.is_null() {
        return None;
    }
    let charsxp = arena.alloc_charsxp(s.as_bytes());
    unsafe {
        *data = charsxp;
    }
    Sexp::from_raw(ptr)
}

pub fn scalar_complex(r: c_double, i: c_double) -> Option<Sexp<'static>> {
    with_arena(|arena| scalar_complex_in(arena, r, i))
}

pub fn scalar_complex_in(arena: &mut RArena, r: c_double, i: c_double) -> Option<Sexp<'static>> {
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
    Sexp::from_raw(ptr)
}

pub fn mk_char(s: &[u8]) -> Option<Sexp<'static>> {
    with_arena(|arena| mk_char_in(arena, s))
}

pub fn mk_char_in(arena: &mut RArena, s: &[u8]) -> Option<Sexp<'static>> {
    Sexp::from_raw(arena.alloc_charsxp(s))
}

// ---------------------------------------------------------------------------
// Safe language/pairlist constructors
// ---------------------------------------------------------------------------

pub fn cons(car: Sexp<'_>, cdr: Sexp<'_>, tag: Option<Sexp<'_>>) -> Option<Sexp<'static>> {
    with_arena(|arena| cons_in(arena, car, cdr, tag))
}

pub fn cons_in(
    arena: &mut RArena,
    car: Sexp<'_>,
    cdr: Sexp<'_>,
    tag: Option<Sexp<'_>>,
) -> Option<Sexp<'static>> {
    let tag_raw = tag.map(|t| t.as_raw()).unwrap_or(ptr::null_mut());
    Sexp::from_raw(arena.cons(car.as_raw(), cdr.as_raw(), tag_raw))
}

pub fn lang2(car: Sexp<'_>, arg: Sexp<'_>) -> Option<Sexp<'static>> {
    with_arena(|arena| lang2_in(arena, car, arg))
}

pub fn lang2_in(arena: &mut RArena, car: Sexp<'_>, arg: Sexp<'_>) -> Option<Sexp<'static>> {
    let cdr = arena.alloc_node(SEXPTYPE::LANGSXP);
    if cdr.is_null() {
        return None;
    }
    unsafe {
        (*cdr).data.listsxp.carval = arg.as_raw();
        (*cdr).data.listsxp.cdrval = R_NilValue();
        (*cdr).data.listsxp.tagval = ptr::null_mut();
    }
    let head = arena.alloc_node(SEXPTYPE::LANGSXP);
    if head.is_null() {
        return None;
    }
    unsafe {
        (*head).data.listsxp.carval = car.as_raw();
        (*head).data.listsxp.cdrval = cdr;
        (*head).data.listsxp.tagval = ptr::null_mut();
    }
    Sexp::from_raw(head)
}

pub fn lang3(car: Sexp<'_>, arg1: Sexp<'_>, arg2: Sexp<'_>) -> Option<Sexp<'static>> {
    with_arena(|arena| lang3_in(arena, car, arg1, arg2))
}

pub fn lang3_in(
    arena: &mut RArena,
    car: Sexp<'_>,
    arg1: Sexp<'_>,
    arg2: Sexp<'_>,
) -> Option<Sexp<'static>> {
    let c2 = arena.alloc_node(SEXPTYPE::LANGSXP);
    if c2.is_null() {
        return None;
    }
    unsafe {
        (*c2).data.listsxp.carval = arg2.as_raw();
        (*c2).data.listsxp.cdrval = R_NilValue();
        (*c2).data.listsxp.tagval = ptr::null_mut();
    }
    let c1 = arena.alloc_node(SEXPTYPE::LANGSXP);
    if c1.is_null() {
        return None;
    }
    unsafe {
        (*c1).data.listsxp.carval = arg1.as_raw();
        (*c1).data.listsxp.cdrval = c2;
        (*c1).data.listsxp.tagval = ptr::null_mut();
    }
    let head = arena.alloc_node(SEXPTYPE::LANGSXP);
    if head.is_null() {
        return None;
    }
    unsafe {
        (*head).data.listsxp.carval = car.as_raw();
        (*head).data.listsxp.cdrval = c1;
        (*head).data.listsxp.tagval = ptr::null_mut();
    }
    Sexp::from_raw(head)
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
        let vec = some(IntVector::new(&[1, 2, 3]).build());
        assert!(vec.is_vector());
        assert_eq!(vec.len(), 3);
        assert_eq!(vec.integer_elt(0), Some(1));
        assert_eq!(vec.integer_elt(1), Some(2));
        assert_eq!(vec.integer_elt(2), Some(3));
    }

    #[test]
    fn test_int_vector_zeros() {
        let vec = some(IntVector::zeros(5).build());
        assert_eq!(vec.len(), 5);
        for i in 0..5 {
            assert_eq!(vec.integer_elt(i as R_xlen_t), Some(0));
        }
    }

    #[test]
    fn test_int_vector_na() {
        let vec = some(IntVector::with_na(3).build());
        assert_eq!(vec.len(), 3);
        for i in 0..3 {
            assert_eq!(
                vec.integer_elt(i as R_xlen_t),
                Some(super::super::ffi::NA_INTEGER)
            );
        }
    }

    #[test]
    fn test_real_vector_builder() {
        let vec = some(RealVector::new(&[1.5, 2.5, 3.5]).build());
        assert_eq!(vec.len(), 3);
        assert!((some(vec.real_elt(0)) - 1.5).abs() < f64::EPSILON);
        assert!((some(vec.real_elt(1)) - 2.5).abs() < f64::EPSILON);
        assert!((some(vec.real_elt(2)) - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_real_vector_seq() {
        let vec = some(RealVector::seq(0.0, 1.0, 0.25).build());
        assert_eq!(vec.len(), 5);
        assert!((some(vec.real_elt(0)) - 0.0).abs() < f64::EPSILON);
        assert!((some(vec.real_elt(4)) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_logical_vector_builder() {
        let vec = some(LogicalVector::new(&[true, false, true]).build());
        assert_eq!(vec.len(), 3);
        assert_eq!(vec.logical_elt(0), Some(1));
        assert_eq!(vec.logical_elt(1), Some(0));
        assert_eq!(vec.logical_elt(2), Some(1));
    }

    #[test]
    fn test_raw_vector_builder() {
        let vec = some(RawVector::new(&[0xDE, 0xAD, 0xBE, 0xEF]).build());
        assert_eq!(vec.len(), 4);
        assert_eq!(vec.raw_elt(0), Some(0xDE));
        assert_eq!(vec.raw_elt(1), Some(0xAD));
        assert_eq!(vec.raw_elt(2), Some(0xBE));
        assert_eq!(vec.raw_elt(3), Some(0xEF));
    }

    #[test]
    fn test_string_vector_builder() {
        let vec = some(StringVector::new(&["hello", "world"]).build());
        assert_eq!(vec.len(), 2);
        assert!(vec.string_elt(0).is_some());
        assert!(vec.string_elt(1).is_some());
    }

    #[test]
    fn test_generic_vector_builder() {
        let int_v = some(IntVector::new(&[1, 2]).build());
        let real_v = some(RealVector::new(&[3.0, 4.0]).build());
        let vec = some(
            GenericVector::with_length(2)
                .set(0, int_v.as_raw())
                .set(1, real_v.as_raw())
                .build(),
        );
        assert_eq!(vec.len(), 2);
        assert!(vec.vector_elt(0).is_some());
        assert!(vec.vector_elt(1).is_some());
    }

    #[test]
    fn test_pairlist_builder() {
        let mut arena = crate::sexp::memory::RArena::new();
        let a = arena.alloc_node(SEXPTYPE::INTSXP);
        let b = arena.alloc_node(SEXPTYPE::REALSXP);
        let list = some(
            PairlistBuilder::new()
                .push_untagged(a)
                .push_untagged(b)
                .build(),
        );
        assert!(list.is_pairlist());
        assert!(list.car().is_some());
        assert!(list.cdr().is_some());
    }

    #[test]
    fn test_convenience_functions() {
        let v1 = some(int_vec(&[10, 20, 30]));
        assert_eq!(v1.integer_elt(0), Some(10));

        let v2 = some(real_vec(&[1.0, 2.0]));
        assert!((some(v2.real_elt(0)) - 1.0).abs() < f64::EPSILON);

        let v3 = some(logical_vec(&[true, false]));
        assert_eq!(v3.logical_elt(0), Some(1));

        let v4 = some(raw_vec(&[0xFF]));
        assert_eq!(v4.raw_elt(0), Some(0xFF));

        let v5 = some(string_vec(&["test"]));
        assert_eq!(v5.len(), 1);

        let v6 = some(seq(0.0, 2.0, 1.0));
        assert_eq!(v6.len(), 3);
    }

    #[test]
    fn test_scalar_constructors() {
        let si = some(scalar_integer(42));
        assert_eq!(si.integer_elt(0), Some(42));
        assert_eq!(si.len(), 1);

        let sr = some(scalar_real(3.14));
        assert!((some(sr.real_elt(0)) - 3.14).abs() < f64::EPSILON);

        let sl = some(scalar_logical(1));
        assert_eq!(sl.logical_elt(0), Some(1));

        let sraw = some(scalar_raw(0xAB));
        assert_eq!(sraw.raw_elt(0), Some(0xAB));

        let ss = some(scalar_string("hello"));
        assert_eq!(ss.len(), 1);
        assert!(ss.string_elt(0).is_some());

        let sc = some(scalar_complex(1.0, 2.0));
        let c = some(sc.complex_elt(0));
        assert!((c.r - 1.0).abs() < f64::EPSILON);
        assert!((c.i - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_mk_char() {
        let c = some(mk_char(b"hello"));
        assert!(c.is_charsxp());
        assert_eq!(c.as_str(), Some("hello"));
        assert_eq!(c.as_bytes(), Some(&b"hello"[..]));
    }

    #[test]
    fn test_cons_constructor() {
        let car = some(scalar_integer(1));
        let cdr = some(scalar_real(2.0));
        let cell = some(cons(car, cdr, None));
        assert!(cell.is_pairlist());
        assert!(some(cell.car()).is_vector());
    }

    #[test]
    fn test_lang_constructors() {
        let mut arena = crate::sexp::memory::RArena::new();
        let sym = arena.alloc_node(SEXPTYPE::SYMSXP);
        let fun = some(Sexp::from_raw(sym));
        let arg = some(scalar_integer(1));

        let call = some(lang2(fun, arg));
        assert!(call.is_pairlist());

        let arg2 = some(scalar_real(2.0));
        let call3 = some(lang3(fun, arg, arg2));
        assert!(call3.is_pairlist());
        assert!(call3.car().is_some());
    }
}
