#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Core attribute operations — ports parts of R's attrib.c.
//!
//! Provides the subset of attrib.c needed for method dispatch:
//! - getAttrib: get an attribute from an object
//! - setAttrib: set an attribute on an object
//! - isObject: check if an object has a "class" attribute
//! - R_classgets: set the class of an object

use std::os::raw::c_int;

use crate::sexp::accessors::{ATTRIB, CAR, CDR, SET_ATTRIB, SETCAR, TAG, TYPEOF};
use crate::sexp::constructors::*;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Pre-interned attribute name symbols
// ---------------------------------------------------------------------------

/// Get the "class" symbol.
pub unsafe fn R_ClassSymbol() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("class").unwrap_or_default().as_ptr()) }
}

/// Get the "names" symbol.
pub unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("names").unwrap_or_default().as_ptr()) }
}

/// Get the "dim" symbol.
pub unsafe fn R_DimSymbol() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("dim").unwrap_or_default().as_ptr()) }
}

/// Get the "dimnames" symbol.
pub unsafe fn R_DimNamesSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("dimnames")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Get the "levels" symbol.
pub unsafe fn R_LevelsSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("levels")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Get the "tsp" symbol.
pub unsafe fn R_TspSymbol() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("tsp").unwrap_or_default().as_ptr()) }
}

/// Get the "srcref" symbol.
pub unsafe fn R_SrcRefSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("srcref")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Get the "srcfile" symbol.
pub unsafe fn R_SrcFileSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("srcfile")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Get the "row.names" symbol.
pub unsafe fn R_RowNamesSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("row.names")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Get the ".Environment" symbol.
pub unsafe fn R_EnvironmentSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new(".Environment")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

// ---------------------------------------------------------------------------
// getAttrib — get an attribute value
// ---------------------------------------------------------------------------

/// Get the value of an attribute from an object.
///
/// This is the equivalent of R's `getAttrib()` from attrib.c.
/// Searches the attribute pairlist for a matching symbol.
#[unsafe(no_mangle)]
pub unsafe fn getAttrib(x: SEXP, which: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || which.is_null() {
            return R_NilValue();
        }

        let attrib = ATTRIB(x);
        if attrib.is_null() || attrib == R_NilValue() {
            return R_NilValue();
        }

        // Linear search through attribute pairlist
        let mut current = attrib;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == which {
                return CAR(current);
            }
            current = CDR(current);
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// setAttrib — set an attribute value
// ---------------------------------------------------------------------------

/// Set an attribute on an object.
///
/// This is the equivalent of R's `setAttrib()` from attrib.c.
pub unsafe fn setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        if x.is_null() || which.is_null() {
            return;
        }

        let attrib = ATTRIB(x);

        // Search for existing attribute
        let mut current = attrib;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == which {
                // Found — replace value
                SETCAR(current, value);
                // Update OBJECT flag for "class" attribute
                let name = crate::sexp::accessors::PRINTNAME(which);
                if !name.is_null() {
                    let s = crate::sexp::accessors::CHAR(name);
                    if !s.is_null() {
                        let name_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                        if name_str == "class" {
                            if value.is_null() || value == R_NilValue() {
                                crate::sexp::accessors::SET_OBJECT(x, 0);
                            } else {
                                crate::sexp::accessors::SET_OBJECT(x, 1);
                            }
                        }
                    }
                }
                return;
            }
            current = CDR(current);
        }

        // Not found — prepend new attribute
        let new_attr = Rf_cons(value, attrib);
        if !new_attr.is_null() {
            crate::sexp::accessors::SETTAG(new_attr, which);
            SET_ATTRIB(x, new_attr);
        }

        // Set OBJECT flag if setting "class" to non-nil
        if !value.is_null() && value != R_NilValue() {
            let name = crate::sexp::accessors::PRINTNAME(which);
            if !name.is_null() {
                let s = crate::sexp::accessors::CHAR(name);
                if !s.is_null() {
                    let name_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                    if name_str == "class" {
                        crate::sexp::accessors::SET_OBJECT(x, 1);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// isObject — check if an object has a class attribute
// ---------------------------------------------------------------------------

/// Check if an object is an S4 object (has a "class" attribute).
///
/// This is the equivalent of R's `isObject()` macro.
pub unsafe fn isObject(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        crate::sexp::accessors::OBJECT(x)
    }
}

// ---------------------------------------------------------------------------
// R_classgets — set the class of an object
// ---------------------------------------------------------------------------

/// Set the class attribute of an object.
///
/// This is the equivalent of R's `R_classgets()`.
pub unsafe fn R_classgets(x: SEXP, klass: SEXP) -> SEXP {
    unsafe {
        if klass.is_null() || klass == R_NilValue() {
            return x;
        }

        let class_sym = R_ClassSymbol();
        setAttrib(x, class_sym, klass);
        x
    }
}

// ---------------------------------------------------------------------------
// R_data_class — get the data class of an object
// ---------------------------------------------------------------------------

/// Get the class of an object (returns the first class element).
///
/// This is the equivalent of R's `R_data_class()`.
pub unsafe fn R_data_class(x: SEXP) -> SEXP {
    unsafe {
        let class_val = getAttrib(x, R_ClassSymbol());
        if class_val.is_null() || class_val == R_NilValue() {
            // Return the default class based on type
            let t = TYPEOF(x);
            let name = match t {
                10 => "logical",
                13 => "integer",
                14 => "numeric",
                15 => "complex",
                16 => "character",
                24 => "raw",
                19 => "list",
                _ => "unknown",
            };
            return Rf_mkString(std::ffi::CString::new(name).unwrap_or_default().as_ptr());
        }
        class_val
    }
}

// ---------------------------------------------------------------------------
// R_length_gets — get the length attribute
// ---------------------------------------------------------------------------

/// Get the length of an object via the "length" attribute.
pub unsafe fn R_length_gets(x: SEXP) -> c_int {
    unsafe {
        let len_sym = Rf_install(
            std::ffi::CString::new("length")
                .unwrap_or_default()
                .as_ptr(),
        );
        let val = getAttrib(x, len_sym);
        if !val.is_null() && TYPEOF(val) == SEXPTYPE::INTSXP {
            let data = crate::sexp::accessors::INTEGER(val);
            if !data.is_null() {
                return *data;
            }
        }
        // Default: use the actual length
        crate::sexp::constructors::Rf_length(x)
    }
}

// ---------------------------------------------------------------------------
// Rf_getAttrib — FFI-compatible getAttrib
// ---------------------------------------------------------------------------

/// FFI-compatible version of getAttrib.
pub unsafe fn Rf_getAttrib(x: SEXP, which: SEXP) -> SEXP {
    unsafe { getAttrib(x, which) }
}

/// FFI-compatible version of setAttrib.
pub unsafe fn Rf_setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        setAttrib(x, which, value);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    #[test]
    fn test_get_attrib_null() {
        unsafe {
            assert_eq!(getAttrib(ptr::null_mut(), ptr::null_mut()), R_NilValue());
        }
    }

    #[test]
    fn test_is_object_null() {
        unsafe {
            assert_eq!(isObject(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_data_class() {
        unsafe {
            let v = Rf_ScalarInteger(42);
            let class = R_data_class(v);
            // Should return "integer" or the CHARSXP for it
            assert!(!class.is_null());
        }
    }
}
