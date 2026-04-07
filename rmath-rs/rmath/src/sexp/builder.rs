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
use super::globals::{R_BaseEnv, R_GlobalEnv, R_NilValue};
use super::memory::with_arena;
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

    /// Build the integer vector.
    ///
    /// Returns `None` if allocation fails.
    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| {
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
        })
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

    /// Build the real vector.
    ///
    /// Returns `None` if allocation fails.
    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| {
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
        })
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

    /// Build the logical vector.
    ///
    /// Returns `None` if allocation fails.
    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| {
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
        })
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

    /// Build the raw vector.
    ///
    /// Returns `None` if allocation fails.
    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| {
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
        })
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

    /// Build the string vector.
    ///
    /// Returns `None` if allocation fails.
    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| {
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
        })
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
/// let int_v = IntVector::new(&[1, 2]).build().expect("failed to build IntVector");
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

    /// Build the generic vector.
    ///
    /// Returns `None` if allocation fails.
    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| {
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
        })
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

    /// Build the pairlist chain.
    ///
    /// Returns `None` if the result is null (empty builder).
    pub fn build(self) -> Option<Sexp<'static>> {
        with_arena(|arena| {
            let mut result: SEXP = ptr::null_mut();
            for (car, tag) in self.elements.into_iter().rev() {
                result = arena.cons(car, result, tag);
            }
            Sexp::from_raw(result)
        })
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

/// Create an integer vector from a slice.
///
/// A shorthand for `IntVector::new(values).build()`.
pub fn int_vec(values: &[c_int]) -> Option<Sexp<'static>> {
    IntVector::new(values).build()
}

/// Create a real vector from a slice.
///
/// A shorthand for `RealVector::new(values).build()`.
pub fn real_vec(values: &[c_double]) -> Option<Sexp<'static>> {
    RealVector::new(values).build()
}

/// Create a logical vector from a slice of booleans.
///
/// A shorthand for `LogicalVector::new(values).build()`.
pub fn logical_vec(values: &[bool]) -> Option<Sexp<'static>> {
    LogicalVector::new(values).build()
}

/// Create a raw vector from a slice of bytes.
///
/// A shorthand for `RawVector::new(values).build()`.
pub fn raw_vec(values: &[Rbyte]) -> Option<Sexp<'static>> {
    RawVector::new(values).build()
}

/// Create a string vector from a slice of string slices.
///
/// A shorthand for `StringVector::new(values).build()`.
pub fn string_vec(values: &[&str]) -> Option<Sexp<'static>> {
    StringVector::new(values).build()
}

/// Create a sequence of real numbers.
///
/// A shorthand for `RealVector::seq(start, end, step).build()`.
pub fn seq(start: f64, end: f64, step: f64) -> Option<Sexp<'static>> {
    RealVector::seq(start, end, step).build()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_vector_builder() {
        let vec = IntVector::new(&[1, 2, 3]).build().unwrap();
        assert!(vec.is_vector());
        assert_eq!(vec.len(), 3);
        assert_eq!(vec.integer_elt(0), Some(1));
        assert_eq!(vec.integer_elt(1), Some(2));
        assert_eq!(vec.integer_elt(2), Some(3));
    }

    #[test]
    fn test_int_vector_zeros() {
        let vec = IntVector::zeros(5).build().unwrap();
        assert_eq!(vec.len(), 5);
        for i in 0..5 {
            assert_eq!(vec.integer_elt(i as R_xlen_t), Some(0));
        }
    }

    #[test]
    fn test_int_vector_na() {
        let vec = IntVector::with_na(3).build().unwrap();
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
        let vec = RealVector::new(&[1.5, 2.5, 3.5]).build().unwrap();
        assert_eq!(vec.len(), 3);
        assert!((vec.real_elt(0).unwrap() - 1.5).abs() < f64::EPSILON);
        assert!((vec.real_elt(1).unwrap() - 2.5).abs() < f64::EPSILON);
        assert!((vec.real_elt(2).unwrap() - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_real_vector_seq() {
        let vec = RealVector::seq(0.0, 1.0, 0.25).build().unwrap();
        assert_eq!(vec.len(), 5);
        assert!((vec.real_elt(0).unwrap() - 0.0).abs() < f64::EPSILON);
        assert!((vec.real_elt(4).unwrap() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_logical_vector_builder() {
        let vec = LogicalVector::new(&[true, false, true]).build().unwrap();
        assert_eq!(vec.len(), 3);
        assert_eq!(vec.logical_elt(0), Some(1));
        assert_eq!(vec.logical_elt(1), Some(0));
        assert_eq!(vec.logical_elt(2), Some(1));
    }

    #[test]
    fn test_raw_vector_builder() {
        let vec = RawVector::new(&[0xDE, 0xAD, 0xBE, 0xEF]).build().unwrap();
        assert_eq!(vec.len(), 4);
        assert_eq!(vec.raw_elt(0), Some(0xDE));
        assert_eq!(vec.raw_elt(1), Some(0xAD));
        assert_eq!(vec.raw_elt(2), Some(0xBE));
        assert_eq!(vec.raw_elt(3), Some(0xEF));
    }

    #[test]
    fn test_string_vector_builder() {
        let vec = StringVector::new(&["hello", "world"]).build().unwrap();
        assert_eq!(vec.len(), 2);
        assert!(vec.string_elt(0).is_some());
        assert!(vec.string_elt(1).is_some());
    }

    #[test]
    fn test_generic_vector_builder() {
        let int_v = IntVector::new(&[1, 2]).build().unwrap();
        let real_v = RealVector::new(&[3.0, 4.0]).build().unwrap();
        let vec = GenericVector::with_length(2)
            .set(0, int_v.as_raw())
            .set(1, real_v.as_raw())
            .build()
            .unwrap();
        assert_eq!(vec.len(), 2);
        assert!(vec.vector_elt(0).is_some());
        assert!(vec.vector_elt(1).is_some());
    }

    #[test]
    fn test_pairlist_builder() {
        let mut arena = crate::sexp::memory::RArena::new();
        let a = arena.alloc_node(SEXPTYPE::INTSXP);
        let b = arena.alloc_node(SEXPTYPE::REALSXP);
        let list = PairlistBuilder::new()
            .push_untagged(a)
            .push_untagged(b)
            .build()
            .unwrap();
        assert!(list.is_pairlist());
        assert!(list.car().is_some());
        assert!(list.cdr().is_some());
    }

    #[test]
    fn test_convenience_functions() {
        let v1 = int_vec(&[10, 20, 30]).unwrap();
        assert_eq!(v1.integer_elt(0), Some(10));

        let v2 = real_vec(&[1.0, 2.0]).unwrap();
        assert!((v2.real_elt(0).unwrap() - 1.0).abs() < f64::EPSILON);

        let v3 = logical_vec(&[true, false]).unwrap();
        assert_eq!(v3.logical_elt(0), Some(1));

        let v4 = raw_vec(&[0xFF]).unwrap();
        assert_eq!(v4.raw_elt(0), Some(0xFF));

        let v5 = string_vec(&["test"]).unwrap();
        assert_eq!(v5.len(), 1);

        let v6 = seq(0.0, 2.0, 1.0).unwrap();
        assert_eq!(v6.len(), 3);
    }
}
