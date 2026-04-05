//! ALTREP (Alternative Representations) support.
//!
//! ALTREP allows R vectors to compute elements on demand rather than
//! storing all elements in memory. This is used for sequences like
//! `1:1000000` which would be expensive to materialize.
//!
//! # Design
//!
//! An ALTREP object is a VECSXP with:
//! - The ALT bit set (sxpinfo.alt)
//! - data1: pointer to the ALTREP class/methods
//! - data2: pointer to the ALTREP instance data
//!
//! # Example
//!
//! ```rust
//! use rmath::sexp::altrep::AltrepBuilder;
//!
//! // Create a lazy sequence 1..1000000
//! let seq = AltrepBuilder::new()
//!     .class("sequence")
//!     .data2(AltrepData::Sequence { start: 1.0, end: 1000000.0, by: 1.0 })
//!     .build();
//! ```

use std::os::raw::c_double;
use std::os::raw::c_int;

use super::ffi::{R_xlen_t, SexprecCore, SexprecData, SEXP, SEXPTYPE};
use super::memory::with_arena;

/// ALTREP data payload types.
#[derive(Debug)]
pub enum AltrepData {
    /// Lazy sequence: start, end, step
    Sequence { start: f64, end: f64, by: f64 },
    /// Repeated value
    Repeat { value: SEXP, length: i64 },
    /// Deferred computation
    Deferred { expr: SEXP, env: SEXP },
    /// External data source
    External { ptr: *mut std::os::raw::c_void },
}

/// ALTREP class definition.
///
/// Defines the methods that an ALTREP vector must implement.
#[derive(Debug)]
pub struct AltrepClass {
    /// Class name (for debugging)
    pub name: &'static str,
    /// Get element at index (0-based)
    pub get_elt: fn(&AltrepData, i64) -> SEXP,
    /// Get pointer to data (for bulk access)
    pub get_dataptr: fn(&AltrepData) -> *mut std::os::raw::c_void,
    /// Get length of the vector
    pub get_length: fn(&AltrepData) -> i64,
    /// Materialize the full vector (compute all elements)
    pub materialize: fn(&AltrepData) -> SEXP,
}

/// Builder for ALTREP objects.
pub struct AltrepBuilder {
    class: Option<&'static AltrepClass>,
    data: Option<AltrepData>,
    length: i64,
}

impl AltrepBuilder {
    /// Create a new ALTREP builder.
    pub fn new() -> Self {
        AltrepBuilder {
            class: None,
            data: None,
            length: 0,
        }
    }

    /// Set the ALTREP class.
    pub fn class(mut self, class: &'static AltrepClass) -> Self {
        self.class = Some(class);
        self
    }

    /// Set the ALTREP data payload.
    pub fn data2(mut self, data: AltrepData) -> Self {
        if let Some(ref cls) = self.class {
            self.length = (cls.get_length)(&data);
        }
        self.data = Some(data);
        self
    }

    /// Build the ALTREP object.
    ///
    /// Returns None if class or data is not set.
    pub fn build(self) -> Option<SEXP> {
        let class = self.class?;
        let data = self.data?;

        with_arena(|arena| {
            // Create a VECSXP to hold the ALTREP metadata
            let vec = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
            if vec.is_null() {
                return None;
            }

            unsafe {
                // Set the ALT bit
                (*vec).sxpinfo.set_alt(true);

                // Store class pointer in data1
                let data_ptr = (*vec).gengc_next_node as *mut SEXP;
                if data_ptr.is_null() {
                    return None;
                }

                // Store class as external pointer (simplified)
                // In a full implementation, this would be a proper EXTPTRSXP
                *data_ptr = class as *const AltrepClass as SEXP;

                // Store data in second slot
                // For now, we box the data and store the pointer
                let boxed_data = Box::new(data);
                let data_ptr2 = data_ptr.add(1);
                *data_ptr2 = Box::into_raw(boxed_data) as SEXP;
            }

            Some(vec)
        })
    }
}

impl Default for AltrepBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Built-in ALTREP classes
// ---------------------------------------------------------------------------

/// ALTREP class for lazy sequences (e.g., 1:1000000).
pub static SEQUENCE_CLASS: AltrepClass = AltrepClass {
    name: "sequence",
    get_elt: |data, i| {
        let AltrepData::Sequence { start, end: _, by } = data else {
            return std::ptr::null_mut();
        };
        with_arena(|arena| {
            let vec = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            if vec.is_null() {
                return std::ptr::null_mut();
            }
            let data_ptr = unsafe { (*vec).gengc_next_node as *mut c_double };
            if data_ptr.is_null() {
                return std::ptr::null_mut();
            }
            unsafe {
                *data_ptr = *start + (i as f64) * *by;
            }
            vec
        })
    },
    get_dataptr: |_data| std::ptr::null_mut(),
    get_length: |data| match data {
        AltrepData::Sequence { start, end, by } => {
            if *by == 0.0 {
                0
            } else {
                ((*end - *start) / *by).abs() as i64 + 1
            }
        }
        _ => 0,
    },
    materialize: |data| {
        let AltrepData::Sequence { start, end, by } = data else {
            return std::ptr::null_mut();
        };

        let len = if *by == 0.0 {
            0
        } else {
            ((*end - *start) / *by).abs() as i64 + 1
        };

        if len <= 0 {
            return std::ptr::null_mut();
        }

        with_arena(|arena| {
            let vec = arena.alloc_vector(SEXPTYPE::REALSXP, len as R_xlen_t);
            if vec.is_null() {
                return std::ptr::null_mut();
            }
            let data_ptr = unsafe { (*vec).gengc_next_node as *mut c_double };
            if data_ptr.is_null() {
                return std::ptr::null_mut();
            }
            unsafe {
                for i in 0..len {
                    *data_ptr.add(i as usize) = *start + (i as f64) * *by;
                }
            }
            vec
        })
    },
};

/// ALTREP class for repeated values.
pub static REPEAT_CLASS: AltrepClass = AltrepClass {
    name: "repeat",
    get_elt: |data, _i| {
        let AltrepData::Repeat { value, .. } = data else {
            return std::ptr::null_mut();
        };
        *value
    },
    get_dataptr: |_data| std::ptr::null_mut(),
    get_length: |data| match data {
        AltrepData::Repeat { length, .. } => *length,
        _ => 0,
    },
    materialize: |data| {
        let AltrepData::Repeat { value, length } = data else {
            return std::ptr::null_mut();
        };

        if *length <= 0 || value.is_null() {
            return std::ptr::null_mut();
        }

        with_arena(|arena| {
            let vec = arena.alloc_vector(SEXPTYPE::VECSXP, *length as R_xlen_t);
            if vec.is_null() {
                return std::ptr::null_mut();
            }
            let data_ptr = unsafe { (*vec).gengc_next_node as *mut SEXP };
            if data_ptr.is_null() {
                return std::ptr::null_mut();
            }
            unsafe {
                for i in 0..*length {
                    *data_ptr.add(i as usize) = *value;
                }
            }
            vec
        })
    },
};

// ---------------------------------------------------------------------------
// ALTREP query functions
// ---------------------------------------------------------------------------

/// Check if a SEXP is an ALTREP object.
pub fn is_altrep(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    unsafe { (*x).sxpinfo.alt() }
}

/// Get the ALTREP class of an object.
pub fn altrep_class(x: SEXP) -> Option<&'static AltrepClass> {
    if !is_altrep(x) {
        return None;
    }
    unsafe {
        let data = (*x).gengc_next_node as *mut SEXP;
        if data.is_null() {
            return None;
        }
        let class_ptr = *data as *const AltrepClass;
        if class_ptr.is_null() {
            return None;
        }
        Some(&*class_ptr)
    }
}

/// Get the length of an ALTREP object.
pub fn altrep_length(x: SEXP) -> Option<i64> {
    let class = altrep_class(x)?;
    // Would need to access data2 — simplified for now
    None
}

/// Get a single element from an ALTREP object.
pub fn altrep_elt(x: SEXP, i: i64) -> Option<SEXP> {
    let class = altrep_class(x)?;
    // Would need to access data2 — simplified for now
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_altrep_builder_sequence() {
        let builder = AltrepBuilder::new()
            .class(&SEQUENCE_CLASS)
            .data2(AltrepData::Sequence {
                start: 1.0,
                end: 100.0,
                by: 1.0,
            });

        assert_eq!(builder.length, 100);
    }

    #[test]
    fn test_altrep_builder_repeat() {
        let builder = AltrepBuilder::new()
            .class(&REPEAT_CLASS)
            .data2(AltrepData::Repeat {
                value: std::ptr::null_mut(),
                length: 1000,
            });

        assert_eq!(builder.length, 1000);
    }

    #[test]
    fn test_altrep_builder_missing_class() {
        let builder = AltrepBuilder::new().data2(AltrepData::Sequence {
            start: 0.0,
            end: 10.0,
            by: 1.0,
        });

        assert!(builder.build().is_none());
    }

    #[test]
    fn test_is_altrep_null() {
        assert!(!is_altrep(std::ptr::null_mut()));
    }

    #[test]
    fn test_altrep_class_null() {
        assert!(altrep_class(std::ptr::null_mut()).is_none());
    }

    #[test]
    fn test_sequence_length() {
        let data = AltrepData::Sequence {
            start: 1.0,
            end: 100.0,
            by: 1.0,
        };
        assert_eq!(SEQUENCE_CLASS.get_length(&data), 100);
    }

    #[test]
    fn test_sequence_length_negative_step() {
        let data = AltrepData::Sequence {
            start: 100.0,
            end: 1.0,
            by: -1.0,
        };
        assert_eq!(SEQUENCE_CLASS.get_length(&data), 100);
    }

    #[test]
    fn test_sequence_length_zero_step() {
        let data = AltrepData::Sequence {
            start: 1.0,
            end: 100.0,
            by: 0.0,
        };
        assert_eq!(SEQUENCE_CLASS.get_length(&data), 0);
    }
}
