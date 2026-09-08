#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Core attribute operations — ports parts of R's attrib.c.
//!
//! Provides the subset of attrib.c needed for method dispatch:
//! - getAttrib: get an attribute from an object
//! - setAttrib: set an attribute on an object
//! - isObject: check if an object has a "class" attribute
//! - R_classgets: set the class of an object

use std::os::raw::c_int;

use super::accessors::{ATTRIB, CAR, CDR, SET_ATTRIB, SETCAR, SETCDR, TAG, TYPEOF};
use super::constructors::*;
use super::ffi::{SEXP, SEXPTYPE};
use super::globals::R_NilValue;
use super::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Pre-interned attribute name symbols
// ---------------------------------------------------------------------------

/// Get the "class" symbol.
pub unsafe fn R_ClassSymbol() -> SEXP {
    unsafe { Rf_install(c"class".as_ptr()) }
}

/// Get the "names" symbol.
pub unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { Rf_install(c"names".as_ptr()) }
}

/// Get the "dim" symbol.
pub unsafe fn R_DimSymbol() -> SEXP {
    unsafe { Rf_install(c"dim".as_ptr()) }
}

/// Get the "dimnames" symbol.
pub unsafe fn R_DimNamesSymbol() -> SEXP {
    unsafe { Rf_install(c"dimnames".as_ptr()) }
}

/// Get the "levels" symbol.
pub unsafe fn R_LevelsSymbol() -> SEXP {
    unsafe { Rf_install(c"levels".as_ptr()) }
}

/// Get the "tsp" symbol.
pub unsafe fn R_TspSymbol() -> SEXP {
    unsafe { Rf_install(c"tsp".as_ptr()) }
}

/// Get the "srcref" symbol.
pub unsafe fn R_SrcRefSymbol() -> SEXP {
    unsafe { Rf_install(c"srcref".as_ptr()) }
}

/// Get the "srcfile" symbol.
pub unsafe fn R_SrcFileSymbol() -> SEXP {
    unsafe { Rf_install(c"srcfile".as_ptr()) }
}

/// Get the "row.names" symbol.
pub unsafe fn R_RowNamesSymbol() -> SEXP {
    unsafe { Rf_install(c"row.names".as_ptr()) }
}

/// Get the ".Environment" symbol.
pub unsafe fn R_EnvironmentSymbol() -> SEXP {
    unsafe { Rf_install(c".Environment".as_ptr()) }
}

// ---------------------------------------------------------------------------
// getAttrib — get an attribute value
// ---------------------------------------------------------------------------

/// Get the value of an attribute from an object.
///
/// This is the equivalent of R's `getAttrib()` from attrib.c.
/// Searches the attribute pairlist for a matching symbol.
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

        // setAttrib(x, R_DimSymbol, val) routes through dimgets(), whose
        // first step stores the dims as integer (coerceVector(val, INTSXP)).
        let value = if which == R_DimSymbol()
            && !value.is_null()
            && value != R_NilValue()
            && TYPEOF(value) != SEXPTYPE::INTSXP
        {
            crate::mainutils::coerce::coerceVector(value, SEXPTYPE::INTSXP.into())
        } else {
            value
        };

        let attrib = ATTRIB(x);

        // Search for existing attribute
        let mut previous = R_NilValue();
        let mut current = attrib;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == which {
                if value.is_null() || value == R_NilValue() {
                    let next = CDR(current);
                    if previous.is_null() || previous == R_NilValue() {
                        SET_ATTRIB(x, next);
                    } else {
                        SETCDR(previous, next);
                    }
                    if which == R_ClassSymbol() {
                        super::accessors::SET_OBJECT(x, 0);
                    }
                    return;
                }
                // Found — replace value
                SETCAR(current, value);
                // Update OBJECT flag for "class" attribute
                if which == R_ClassSymbol() {
                    if value.is_null() || value == R_NilValue() {
                        super::accessors::SET_OBJECT(x, 0);
                    } else {
                        super::accessors::SET_OBJECT(x, 1);
                    }
                }
                return;
            }
            previous = current;
            current = CDR(current);
        }

        if value.is_null() || value == R_NilValue() {
            return;
        }

        // Not found — append the new attribute. GNU R preserves attribute
        // assignment order: replacing an existing value leaves it in place,
        // while a new name is linked after the current tail.
        let _x_guard = super::protect::protect(x);
        let _which_guard = super::protect::protect(which);
        let _value_guard = super::protect::protect(value);
        let new_attr = Rf_cons(value, R_NilValue());
        if !new_attr.is_null() {
            super::accessors::SETTAG(new_attr, which);
            if attrib.is_null() || attrib == R_NilValue() {
                SET_ATTRIB(x, new_attr);
            } else {
                SETCDR(previous, new_attr);
            }
        }

        // Set OBJECT flag if setting "class" to non-nil
        if which == R_ClassSymbol() && !value.is_null() && value != R_NilValue() {
            super::accessors::SET_OBJECT(x, 1);
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
        super::accessors::OBJECT(x)
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
                10 => c"logical",
                13 => c"integer",
                14 => c"numeric",
                15 => c"complex",
                16 => c"character",
                24 => c"raw",
                19 => c"list",
                _ => c"unknown",
            };
            return Rf_mkString(name.as_ptr());
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
        let len_sym = Rf_install(c"length".as_ptr());
        let val = getAttrib(x, len_sym);
        if !val.is_null() && TYPEOF(val) == SEXPTYPE::INTSXP {
            let data = super::accessors::INTEGER(val);
            if !data.is_null() {
                return *data;
            }
        }
        // Default: use the actual length
        super::constructors::Rf_length(x)
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
    use super::*;
    use std::ptr;

    #[test]
    fn test_get_attrib_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(getAttrib(ptr::null_mut(), ptr::null_mut()), R_NilValue());
        }
    }

    #[test]
    fn test_is_object_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(isObject(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_data_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let class = R_data_class(v);
            // Should return "integer" or the CHARSXP for it
            assert!(!class.is_null());
        }
    }
}
