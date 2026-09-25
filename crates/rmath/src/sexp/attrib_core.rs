#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Core attribute operations — ports parts of R's attrib.c.
//!
//! Provides the subset of attrib.c needed for method dispatch:
//! - getAttrib: get an attribute from an object
//! - setAttrib: set an attribute on an object
//! - isObject: check if an object has a "class" attribute
//! - R_classgets: set the class of an object

use std::os::raw::c_int;

use super::accessors::{ATTRIB, CAR, CDR, SET_ATTRIB, SETCAR, SETCDR, STRING_ELT, TAG, TYPEOF, XLENGTH};

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

        // Linear search through attribute pairlist
        let mut current = attrib;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == which {
                let value = CAR(current);
                if which == R_RowNamesSymbol() {
                    return expand_compact_row_names(value);
                }
                return value;
            }
            current = CDR(current);
        }
        if which == R_NamesSymbol() {
            let t = TYPEOF(x);
            if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::DOTSXP {
                let mut n = 0i32;
                let mut scan = x;
                while !scan.is_null() && scan != R_NilValue() {
                    n += 1;
                    scan = CDR(scan);
                }
                if n > 0 {
                    let out = Rf_allocVector(SEXPTYPE::STRSXP, n);
                    let _g = super::protect::protect(out);
                    let mut cell = x;
                    let mut i = 0i64;
                    while !cell.is_null() && cell != R_NilValue() && i < n as i64 {
                        let tag = TAG(cell);
                        let s = if tag.is_null() || tag == R_NilValue() {
                            Rf_mkChar(b"\0".as_ptr() as *const std::os::raw::c_char)
                        } else {
                            super::accessors::PRINTNAME(tag)
                        };
                        super::accessors::SET_STRING_ELT(out, i, s);
                        cell = CDR(cell);
                        i += 1;
                    }
                    return out;
                }
            }
        }


        if which == R_NamesSymbol() {
            return names_from_one_dim(x);
        }

        R_NilValue()
    }
}

/// GNU attrib.c getAttrib0: compact `c(NA, ±n)` expands to `1:n`.
unsafe fn expand_compact_row_names(value: SEXP) -> SEXP {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(value) == SEXPTYPE::INTSXP && XLENGTH(value) == 2 {
            let first = *super::accessors::INTEGER(value);
            let second = *super::accessors::INTEGER(value).add(1);
            if first == super::ffi::NA_INTEGER && second != 0 {
                let n = second.unsigned_abs() as usize;
                let expanded = Rf_allocVector(SEXPTYPE::INTSXP, n as i32);
                let _g = super::protect::protect(expanded);
                let dst = super::accessors::INTEGER(expanded);
                for i in 0..n {
                    *dst.add(i) = (i as i32) + 1;
                }
                return expanded;
            }
        }
        value
    }
}
/// GNU `getAttrib`: a length-1 `dim` exposes `dimnames[[1]]` as `names`.
unsafe fn names_from_one_dim(x: SEXP) -> SEXP {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        if TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) != 1 {
            return R_NilValue();
        }
        let dn = getAttrib(x, R_DimNamesSymbol());
        if TYPEOF(dn) != SEXPTYPE::VECSXP || XLENGTH(dn) < 1 {
            return R_NilValue();
        }
        let first = super::accessors::VECTOR_ELT(dn, 0);
        if first.is_null() || TYPEOF(first) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        first
    }
}

// ---------------------------------------------------------------------------
// setAttrib — set an attribute value
// ---------------------------------------------------------------------------

/// GNU `installAttrib` — store a named attribute without dim/names/class
/// special cases. `R_do_slot_assign` uses this so a slot named `"dim"` can
/// hold a character vector (reg-S4.R reserved slot names).
pub unsafe fn installAttrib(vec: SEXP, name: SEXP, val: SEXP) {
    unsafe {
        if vec.is_null() || name.is_null() {
            return;
        }
        let attrib = ATTRIB(vec);
        let mut previous = R_NilValue();
        let mut current = attrib;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == name {
                SETCAR(current, val);
                return;
            }
            previous = current;
            current = CDR(current);
        }
        let _vec_guard = super::protect::protect(vec);
        let _name_guard = super::protect::protect(name);
        let _val_guard = super::protect::protect(val);
        let new_attr = Rf_cons(val, R_NilValue());
        if new_attr.is_null() {
            return;
        }
        super::accessors::SETTAG(new_attr, name);
        if attrib.is_null() || attrib == R_NilValue() {
            SET_ATTRIB(vec, new_attr);
        } else {
            SETCDR(previous, new_attr);
        }
    }
}


/// Set an attribute on an object.
///
/// This is the equivalent of R's `setAttrib()` from attrib.c.
pub unsafe fn setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        if x.is_null() || which.is_null() {
            return;
        }
        // GNU classgets: empty/NULL class strips; non-string errors.
        // GNU dimgets: coerce dims to INTSXP first.
        let value = if which == R_ClassSymbol() {
            classgets_normalize(x, value)
        } else if which == R_DimSymbol()
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

/// GNU `classgets`: NULL or length-0 STRSXP unclasses; other non-strings error.
pub(crate) unsafe fn classgets_normalize(vec: SEXP, klass: SEXP) -> SEXP {
    unsafe {
        if klass.is_null() || klass == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(klass) != SEXPTYPE::STRSXP {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "attempt to set invalid 'class' attribute".to_string(),
            });
        }
        if XLENGTH(klass) <= 0 {
            return R_NilValue();
        }
        if vec.is_null() || vec == R_NilValue() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "attempt to set an attribute on NULL".to_string(),
            });
        }
        let n = XLENGTH(klass);
        for i in 0..n {
            let elt = STRING_ELT(klass, i);
            if elt.is_null() {
                continue;
            }
            let cs = super::accessors::CHAR(elt);
            if !cs.is_null()
                && std::ffi::CStr::from_ptr(cs).to_bytes() == b"factor"
                && TYPEOF(vec) != SEXPTYPE::INTSXP
            {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "adding class \"factor\" to an invalid object".to_string(),
                });
            }
        }
        klass
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

// Implicit class lives in eval/attrib_core.rs::R_data_class (GNU lang2str /
// type2str). Do not add a second table here.


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
            let class = crate::eval::attrib_core::R_data_class(v);
            assert!(!class.is_null());
        }
    }

}
