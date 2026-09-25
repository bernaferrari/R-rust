#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Core attribute operations — ports parts of R's attrib.c.
//!
//! Provides the subset of attrib.c needed for method dispatch:
//! - getAttrib: get an attribute from an object
//! - setAttrib: set an attribute on an object
//! - isObject: check if an object has a "class" attribute
//! - R_classgets: set the class of an object

use std::os::raw::c_int;

use crate::sexp::accessors::{
    ATTRIB, CAR, CDR, CHAR, PRINTNAME, SET_ATTRIB, SET_NAMED, SET_STRING_ELT, SETCAR, SETCDR,
    SETTAG, STRING_ELT, TAG, TYPEOF, XLENGTH,
};

use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NaString, R_NilValue};

use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

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

/// Get the "use.names" symbol.
pub unsafe fn R_UseNamesSymbol() -> SEXP {
    unsafe { Rf_install(c"use.names".as_ptr()) }
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
    unsafe { crate::sexp::attrib_core::getAttrib(x, which) }
}

// ---------------------------------------------------------------------------
// setAttrib — set an attribute value
// ---------------------------------------------------------------------------

/// `names(x) <- pairlist("a","b","c")` stores `c("a","b","c")`.
pub(crate) unsafe fn pairlist_to_names(value: SEXP) -> SEXP {
    unsafe {
        let mut n = 0i32;
        let mut cell = value;
        while !cell.is_null() && cell != R_NilValue() {
            n += 1;
            cell = CDR(cell);
        }
        let out = Rf_allocVector(SEXPTYPE::STRSXP, n);
        let _g = crate::sexp::protect::protect(out);
        cell = value;
        let mut i = 0i32;
        while !cell.is_null() && cell != R_NilValue() && i < n {
            let car = CAR(cell);
            let ch = if TYPEOF(car) == SEXPTYPE::STRSXP && XLENGTH(car) > 0 {
                STRING_ELT(car, 0)
            } else if TYPEOF(car) == SEXPTYPE::CHARSXP {
                car
            } else if TYPEOF(car) == SEXPTYPE::SYMSXP {
                PRINTNAME(car)
            } else {
                crate::sexp::globals::R_NaString()
            };
            SET_STRING_ELT(out, i as i64, ch);
            i += 1;
            cell = CDR(cell);
        }
        out
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
        let value = if which == R_ClassSymbol() {
            crate::sexp::attrib_core::classgets_normalize(x, value)
        } else if which == R_NamesSymbol() && TYPEOF(value) == SEXPTYPE::LISTSXP {
            pairlist_to_names(value)
        } else {
            value
        };

        // GNU namesgets: pairlist/language names are cell tags, not a
        // stored `names` attribute. `quote(f(x=1))[-1]` copies VECSXP
        // names back through setAttrib; without tags, `names()` is NULL
        // and callGeneric's `lapply(names(call[-1]), as.name)` panics.
        // GNU removeAttrib(names) on a pairlist/language also walks
        // cells and SET_TAG(t, R_NilValue) (`names(e) <- NULL` → `f(1)`).
        let xtype = TYPEOF(x);
        if which == R_NamesSymbol() && (xtype == SEXPTYPE::LISTSXP || xtype == SEXPTYPE::LANGSXP) {
            namesgets_pairlist(x, value);
            return;
        }

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
                    update_class_object_flag(x, which, R_NilValue());
                    return;
                }
                // Found — replace value
                SETCAR(current, value);
                // Update OBJECT flag for "class" attribute
                update_class_object_flag(x, which, value);
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
        let _x_guard = crate::sexp::protect::protect(x);
        let _which_guard = crate::sexp::protect::protect(which);
        let _value_guard = crate::sexp::protect::protect(value);
        let new_attr = Rf_cons(value, R_NilValue());
        if !new_attr.is_null() {
            crate::sexp::accessors::SETTAG(new_attr, which);
            if attrib.is_null() || attrib == R_NilValue() {
                SET_ATTRIB(x, new_attr);
            } else {
                SETCDR(previous, new_attr);
            }
        }

        // Set OBJECT flag if setting "class" to non-nil
        update_class_object_flag(x, which, value);
    }
}

/// GNU `namesgets` / `removeAttrib(names)` for LISTSXP/LANGSXP.
/// Non-NULL values install tags (coerced to STRSXP). NULL clears tags.
unsafe fn namesgets_pairlist(list: SEXP, value: SEXP) {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            let mut cell = list;
            while !cell.is_null() && cell != R_NilValue() {
                SETTAG(cell, R_NilValue());
                cell = CDR(cell);
            }
            return;
        }

        let value = if TYPEOF(value) == SEXPTYPE::STRSXP {
            value
        } else {
            crate::mainutils::coerce::coerceVector(value, SEXPTYPE::STRSXP.0)
        };
        let _value_guard = protect(value);
        if value.is_null() || value == R_NilValue() || TYPEOF(value) != SEXPTYPE::STRSXP {
            return;
        }

        let n = XLENGTH(value);
        let mut cell = list;
        let mut i: R_xlen_t = 0;
        while !cell.is_null() && cell != R_NilValue() {
            if i >= n {
                SETTAG(cell, R_NilValue());
            } else {
                let elt = STRING_ELT(value, i);
                if elt.is_null() || elt == R_NilValue() || elt == R_NaString() {
                    SETTAG(cell, R_NilValue());
                } else {
                    let chars = CHAR(elt);
                    if chars.is_null() || *chars == 0 {
                        SETTAG(cell, R_NilValue());
                    } else {
                        SETTAG(cell, Rf_install(chars));
                    }
                }
            }
            i += 1;
            cell = CDR(cell);
        }
    }
}

unsafe fn update_class_object_flag(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        let name = crate::sexp::accessors::PRINTNAME(which);
        if !name.is_null() {
            let s = crate::sexp::accessors::CHAR(name);
            if !s.is_null() {
                let name_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                if name_str == "class" {
                    crate::sexp::accessors::SET_OBJECT(
                        x,
                        if value.is_null() || value == R_NilValue() {
                            0
                        } else {
                            1
                        },
                    );
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

        let dim = getAttrib(x, R_DimSymbol());
        let two_d = !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) == 2;
        if two_d && TYPEOF(klass) == SEXPTYPE::STRSXP {
            let n = XLENGTH(klass);
            let is_matrix = |i: i64| {
                let ch = STRING_ELT(klass, i);
                !ch.is_null()
                    && std::ffi::CStr::from_ptr(CHAR(ch)).to_bytes() == b"matrix"
            };
            let is_array = |i: i64| {
                let ch = STRING_ELT(klass, i);
                !ch.is_null() && std::ffi::CStr::from_ptr(CHAR(ch)).to_bytes() == b"array"
            };
            if (n == 1 && is_matrix(0)) || (n == 2 && is_matrix(0) && is_array(1)) {
                setAttrib(x, class_sym, R_NilValue());
                return x;
            }
        }
        setAttrib(x, class_sym, klass);
        x
    }
}

// ---------------------------------------------------------------------------
// R_data_class — get the data class of an object
// ---------------------------------------------------------------------------

/// GNU `lang2str`: implicit class of a language object is the syntactic
/// head (`if`, `while`, `for`, `=`, `<-`, `(`, `{`) or `"call"`.
pub unsafe fn language_implicit_class_chars(obj: SEXP) -> SEXP {
    unsafe {
        let symb = CAR(obj);
        if TYPEOF(symb) == SEXPTYPE::SYMSXP {
            let pn = PRINTNAME(symb);
            if !pn.is_null() {
                let name = std::ffi::CStr::from_ptr(CHAR(pn)).to_bytes();
                if matches!(name, b"if" | b"while" | b"for" | b"=" | b"<-" | b"(" | b"{") {
                    return pn;
                }
            }
        }
        Rf_mkChar(c"call".as_ptr())
    }
}

/// Get the class of an object (returns the first class element).
///
/// This is the equivalent of R's `R_data_class()`.
pub unsafe fn R_data_class(x: SEXP) -> SEXP {
    unsafe {
        let class_val = getAttrib(x, R_ClassSymbol());
        if class_val.is_null() || class_val == R_NilValue() || TYPEOF(class_val) != SEXPTYPE::STRSXP
        {
            let dim = getAttrib(x, R_DimSymbol());
            if !dim.is_null() && dim != R_NilValue() && TYPEOF(dim) == SEXPTYPE::INTSXP {
                let nd = XLENGTH(dim);
                if nd == 2 {
                    let result = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
                    let _result_guard = protect(result);
                    SET_STRING_ELT(result, 0, Rf_mkChar(c"matrix".as_ptr()));
                    SET_STRING_ELT(result, 1, Rf_mkChar(c"array".as_ptr()));
                    return result;
                }
                if nd > 0 {
                    return Rf_mkString(c"array".as_ptr());
                }
            }

            if TYPEOF(x) == SEXPTYPE::LANGSXP {
                return Rf_ScalarString(language_implicit_class_chars(x));
            }
            if TYPEOF(x) == SEXPTYPE::EXPRSXP {
                return Rf_mkString(c"expression".as_ptr());
            }
            if TYPEOF(x) == SEXPTYPE::SYMSXP {
                return Rf_mkString(c"name".as_ptr());
            }
            if TYPEOF(x) == SEXPTYPE::CLOSXP
                || TYPEOF(x) == SEXPTYPE::SPECIALSXP
                || TYPEOF(x) == SEXPTYPE::BUILTINSXP
            {
                return Rf_mkString(c"function".as_ptr());
            }
            if TYPEOF(x) == SEXPTYPE::LISTSXP {
                return Rf_mkString(c"pairlist".as_ptr());
            }
            if TYPEOF(x) == SEXPTYPE::ENVSXP {
                return Rf_mkString(c"environment".as_ptr());
            }
            if TYPEOF(x) == SEXPTYPE::DOTSXP {
                return Rf_mkString(c"...".as_ptr());
            }

            // GNU type2str fallback for remaining SEXPTYPEs.
            let t = TYPEOF(x);
            let name = match t {
                0 => "NULL",
                10 => "logical",
                13 => "integer",
                14 => "numeric",
                15 => "complex",
                16 => "character",
                19 => "list",
                22 => "externalptr",
                23 => "weakref",
                24 => "raw",
                25 => "S4",
                _ => "unknown",
            };

            return Rf_mkString(std::ffi::CString::new(name).unwrap_or_default().as_ptr());
        }
        // GNU returns the attribute SEXP; callers may `x[] <-` it
        // (.traceClassName). Mark shared so subassign duplicates.
        SET_NAMED(class_val, 2);
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
