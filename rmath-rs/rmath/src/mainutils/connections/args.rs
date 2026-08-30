//! Pairlist argument-extraction helpers shared by all `do_*` connection builtins — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::os::raw::{c_double, c_int};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::ptr;

use bzip2::Compression as BzCompression;
use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use flate2::Compression as GzCompression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::instance::{RInstance, with_current_instance, with_required_current_instance};
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Helper functions for extracting arguments from pairlist
// ---------------------------------------------------------------------------

/// Extract a C string from a CHARSXP.
pub unsafe fn charsxp_to_string(s: SEXP) -> String {
    unsafe {
        if s.is_null() {
            return String::new();
        }
        let ptr = CHAR(s);
        if ptr.is_null() {
            return String::new();
        }
        let cstr = CStr::from_ptr(ptr);
        cstr.to_string_lossy().into_owned()
    }
}

/// Extract a string from the first element of a STRSXP.
pub unsafe fn string_elt(s: SEXP, i: R_xlen_t) -> String {
    unsafe {
        if s.is_null() {
            return String::new();
        }
        let elt = STRING_ELT(s, i);
        charsxp_to_string(elt)
    }
}

/// Check if an SEXP is a string vector and get its first element.
pub unsafe fn check_string_arg(arg: SEXP, name: &str) -> String {
    unsafe {
        if arg.is_null() {
            r_error(&format!("invalid '{}' argument", name));
        }
        let t = TYPEOF(arg);
        if t != SEXPTYPE::STRSXP {
            r_error(&format!("invalid '{}' argument", name));
        }
        let len = LENGTH(arg);
        if len < 1 {
            r_error(&format!("invalid '{}' argument", name));
        }
        string_elt(arg, 0)
    }
}

pub unsafe fn connection_arg_tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return None;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            return None;
        }
        CStr::from_ptr(chars).to_str().ok().map(str::to_string)
    }
}

/// Extract an integer from an SEXP (length-1 INTSXP or similar scalar).
pub unsafe fn as_integer(arg: SEXP) -> c_int {
    unsafe {
        if arg.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::INTSXP {
            let data = INTEGER(arg);
            if data.is_null() {
                return NA_INTEGER;
            }
            *data
        } else if t == SEXPTYPE::REALSXP {
            let data = REAL(arg);
            if data.is_null() {
                return NA_INTEGER;
            }
            *data as c_int
        } else if t == SEXPTYPE::LGLSXP {
            let data = LOGICAL(arg);
            if data.is_null() {
                return NA_INTEGER;
            }
            *data
        } else {
            NA_INTEGER
        }
    }
}

/// Extract a double from an SEXP.
pub unsafe fn as_real(arg: SEXP) -> c_double {
    unsafe {
        if arg.is_null() {
            return NA_REAL;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::REALSXP {
            let data = REAL(arg);
            if data.is_null() {
                return NA_REAL;
            }
            *data
        } else if t == SEXPTYPE::INTSXP {
            let v = as_integer(arg);
            if v == NA_INTEGER {
                NA_REAL
            } else {
                v as c_double
            }
        } else {
            NA_REAL
        }
    }
}

/// Extract a logical value from an SEXP.
pub unsafe fn as_logical(arg: SEXP) -> c_int {
    unsafe {
        if arg.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::LGLSXP {
            let data = LOGICAL(arg);
            if data.is_null() {
                return NA_INTEGER;
            }
            *data
        } else {
            NA_INTEGER
        }
    }
}

/// Check that a logical argument is valid (not NA).
pub unsafe fn check_logical_arg(arg: SEXP, name: &str) -> c_int {
    unsafe {
        let v = as_logical(arg);
        if v == NA_INTEGER {
            r_error(&format!("invalid '{}' argument", name));
        }
        v
    }
}

pub unsafe fn positional_or(args: SEXP, index: usize, default: SEXP) -> SEXP {
    unsafe {
        let mut cell = args;
        for _ in 0..index {
            if cell.is_null() || cell == R_NilValue() {
                return default;
            }
            cell = CDR(cell);
        }
        if cell.is_null() || cell == R_NilValue() {
            return default;
        }
        let value = CAR(cell);
        if value.is_null() || value == R_NilValue() || value == R_MissingArg() {
            default
        } else {
            value
        }
    }
}

pub unsafe fn arg_by_name_or_position(
    args: SEXP,
    position: usize,
    names: &[&str],
    default: SEXP,
) -> SEXP {
    unsafe {
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            if let Some(tag) = connection_arg_tag_name(cell)
                && names.iter().any(|name| tag == *name)
            {
                let value = CAR(cell);
                return if value.is_null() || value == R_NilValue() || value == R_MissingArg() {
                    default
                } else {
                    value
                };
            }
            cell = CDR(cell);
        }

        let mut untagged = 0usize;
        cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            if connection_arg_tag_name(cell).is_none() {
                if untagged == position {
                    let value = CAR(cell);
                    return if value.is_null() || value == R_NilValue() || value == R_MissingArg() {
                        default
                    } else {
                        value
                    };
                }
                untagged += 1;
            }
            cell = CDR(cell);
        }

        default
    }
}

/// Raise an R error via panic.
pub fn r_error(msg: &str) -> ! {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Check if an SEXP inherits from a given class name.
/// Simplified check: looks at the class attribute.
pub unsafe fn inherits_class(x: SEXP, class_name: &str) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        // If it's an integer scalar, we check the class attribute
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP && LENGTH(x) == 1 {
            // Fast path for this port: many call sites pass a plain connection index
            // (INTSXP scalar) without reliable class metadata attached.
            if class_name == "connection" {
                let data = INTEGER(x);
                if !data.is_null() {
                    let idx = *data;
                    if idx >= 0 {
                        init_connections_table();
                        let table = connection_table();
                        let ui = idx as usize;
                        if ui < table.len() && table[ui].is_some() {
                            return true;
                        }
                    }
                }
            }

            let class_attr = ATTRIB(x);
            if !class_attr.is_null() {
                // Walk the class attribute pairlist
                let mut p = class_attr;
                while !p.is_null() && TYPEOF(p) == SEXPTYPE::LISTSXP {
                    let tag = TAG(p);
                    if !tag.is_null() {
                        let pname = PRINTNAME(tag);
                        if !pname.is_null() {
                            let name = charsxp_to_string(pname);
                            if name == "class" {
                                let val = CAR(p);
                                if !val.is_null() && TYPEOF(val) == SEXPTYPE::STRSXP {
                                    let len = LENGTH(val);
                                    for i in 0..len as R_xlen_t {
                                        let s = string_elt(val, i);
                                        if s == class_name {
                                            return true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    p = CDR(p);
                }
            }
        }
        false
    }
}

/// Set the class attribute on an SEXP to a two-element vector.
pub unsafe fn set_connection_class(ans: SEXP, specific_class: &str) {
    unsafe {
        let class_sym = crate::eval::attrib_core::R_ClassSymbol();
        if class_sym.is_null() {
            return;
        }
        let class_vec = Rf_allocVector(SEXPTYPE::STRSXP, 2);
        if class_vec.is_null() {
            return;
        }
        let c1 = CString::new(specific_class).unwrap_or_default();
        let c2 = CString::new("connection").unwrap_or_default();
        let charsxp1 = Rf_mkChar(c1.as_ptr());
        let charsxp2 = Rf_mkChar(c2.as_ptr());
        if !charsxp1.is_null() {
            SET_STRING_ELT(class_vec, 0, charsxp1);
        }
        if !charsxp2.is_null() {
            SET_STRING_ELT(class_vec, 1, charsxp2);
        }
        crate::eval::attrib_core::setAttrib(ans, class_sym, class_vec);
    }
}
