//! Result-name construction for c()/unlist(): NewBase, NewName, ItemName, namesCount, NewExtractNames, c_Extract_opt — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_data_class, getAttrib, isObject, setAttrib};
use crate::eval::dispatch::DispatchOrEval;
use crate::eval::dispatch::promiseArgs;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rbyte, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// NewBase -- construct a dotted base.tag name for c() / unlist()
// ---------------------------------------------------------------------------

/// Build a combined name "base.tag" for recursive name extraction.
pub unsafe fn NewBase(base: SEXP, tag: SEXP) -> SEXP {
    unsafe {
        let base = EnsureString(base);
        let tag = EnsureString(tag);

        let base_empty = if base.is_null() || base == R_NilValue() {
            true
        } else {
            *CHAR(base) == 0
        };
        let tag_empty = if tag.is_null() || tag == R_NilValue() {
            true
        } else {
            *CHAR(tag) == 0
        };

        if !base_empty && !tag_empty {
            // Both non-empty: create "base.tag"
            let sb = std::ffi::CStr::from_ptr(CHAR(base)).to_str().unwrap_or("");
            let st = std::ffi::CStr::from_ptr(CHAR(tag)).to_str().unwrap_or("");
            let combined = format!("{}.{}", sb, st);
            let c_str = std::ffi::CString::new(combined).unwrap_or_default();
            Rf_mkChar(c_str.as_ptr())
        } else if !tag_empty {
            tag
        } else if !base_empty {
            base
        } else {
            R_BlankString()
        }
    }
}

// ---------------------------------------------------------------------------
// NewName -- construct a new element name for c() / unlist()
// ---------------------------------------------------------------------------

/// Build an element name from base, tag, sequence number, and count.
pub unsafe fn NewName(base: SEXP, tag: SEXP, seqno: R_xlen_t, count: c_int) -> SEXP {
    unsafe {
        let base = EnsureString(base);
        let tag = EnsureString(tag);

        let base_empty = if base.is_null() || base == R_NilValue() {
            true
        } else {
            *CHAR(base) == 0
        };
        let tag_empty = if tag.is_null() || tag == R_NilValue() {
            true
        } else {
            *CHAR(tag) == 0
        };

        if !base_empty {
            if !tag_empty {
                // base.tag
                let sb = std::ffi::CStr::from_ptr(CHAR(base)).to_str().unwrap_or("");
                let st = std::ffi::CStr::from_ptr(CHAR(tag)).to_str().unwrap_or("");
                let combined = format!("{}.{}", sb, st);
                let c_str = std::ffi::CString::new(combined).unwrap_or_default();
                Rf_mkChar(c_str.as_ptr())
            } else if count == 1 {
                base
            } else {
                // base<seqno>
                let sb = std::ffi::CStr::from_ptr(CHAR(base)).to_str().unwrap_or("");
                let combined = format!("{}{}", sb, seqno);
                let c_str = std::ffi::CString::new(combined).unwrap_or_default();
                Rf_mkChar(c_str.as_ptr())
            }
        } else if !tag_empty {
            tag
        } else {
            R_BlankString()
        }
    }
}

// ---------------------------------------------------------------------------
// ItemName -- return names[i] if it is a non-empty string, else NULL
// ---------------------------------------------------------------------------

/// Look up `names[i]`; return the CHARSXP if it is non-empty, otherwise
/// `R_NilValue`.  Also used in coerce.c.
pub unsafe fn ItemName(names: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        if names.is_null() || names == R_NilValue() {
            return R_NilValue();
        }
        let elt = STRING_ELT(names, i);
        if elt.is_null() || elt == R_NilValue() {
            return R_NilValue();
        }
        if *CHAR(elt) == 0 {
            // empty string
            return R_NilValue();
        }
        elt
    }
}

// ---------------------------------------------------------------------------
// namesCount -- count names in a (possibly recursive) SEXP
// ---------------------------------------------------------------------------

/// Count the number of names in `v`, recursing if `recurse` is true.
/// Stops early once `nameData->count` exceeds 1.
pub unsafe fn namesCount(v: SEXP, recurse: c_int, nameData: *mut NameData) {
    unsafe {
        if v.is_null() || v == R_NilValue() {
            return;
        }

        if crate::mainutils::objects::isS4(v) != 0 {
            (*nameData).count += 1;
            return;
        }

        let n = xlength(v);
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();
        let names = getAttrib(v, names_sym);

        let t = TYPEOF(v);

        match t {
            NILSXP_I => {
                // nothing
            }
            LISTSXP_I => {
                if recurse != 0 {
                    let mut current = v;
                    for _i in 0..n {
                        if (*nameData).count > 1 {
                            break;
                        }
                        let namei = ItemName(names, _i);
                        let _name_guard = protect(namei);
                        if namei == R_NilValue() {
                            namesCount(CAR(current), recurse, nameData);
                        }
                        current = CDR(current);
                    }
                } else {
                    // fall through to vector case
                    for i in 0..n {
                        if (*nameData).count > 1 {
                            break;
                        }
                        (*nameData).count += 1;
                    }
                }
            }
            VECSXP_I | EXPRSXP_I => {
                if recurse != 0 {
                    for i in 0..n {
                        if (*nameData).count > 1 {
                            break;
                        }
                        let namei = ItemName(names, i);
                        if namei == R_NilValue() {
                            namesCount(VECTOR_ELT(v, i), recurse, nameData);
                        }
                    }
                } else {
                    for i in 0..n {
                        if (*nameData).count > 1 {
                            break;
                        }
                        (*nameData).count += 1;
                    }
                }
            }
            LGLSXP_I | INTSXP_I | REALSXP_I | CPLXSXP_I | STRSXP_I | RAWSXP_I => {
                for i in 0..n {
                    if (*nameData).count > 1 {
                        break;
                    }
                    (*nameData).count += 1;
                }
            }
            _ => {
                (*nameData).count += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// NewExtractNames -- build names attribute for c() / unlist() result
// ---------------------------------------------------------------------------

/// Recursively extract and construct names for the result vector.
pub unsafe fn NewExtractNames(
    v: SEXP,
    base: SEXP,
    tag: SEXP,
    recurse: c_int,
    data: *mut BindData,
    nameData: *mut NameData,
) {
    unsafe {
        if v.is_null() || v == R_NilValue() {
            return;
        }

        let mut savecount: c_int = 0;
        let mut saveseqno: R_xlen_t = 0;
        let mut base = base;
        let mut _base_guard = None;

        // If we have a new tag, reset the index sequence and create the new basename
        if !tag.is_null() && tag != R_NilValue() {
            base = NewBase(base, tag);
            _base_guard = Some(protect(base));
            saveseqno = (*nameData).seqno;
            savecount = (*nameData).count;
            (*nameData).count = 0;
            namesCount(v, recurse, nameData);
            (*nameData).seqno = 0;
        } else {
            saveseqno = 0;
        }

        if crate::mainutils::objects::isS4(v) != 0 {
            let new_name = NewName(base, R_NilValue(), (*nameData).seqno + 1, (*nameData).count);
            (*nameData).seqno += 1;
            SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
            (*data).ans_nnames += 1;
            if !tag.is_null() && tag != R_NilValue() {
                (*nameData).count = savecount;
            }
            (*nameData).seqno += saveseqno;
            return;
        }

        let n = xlength(v);
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();
        let _names = getAttrib(v, names_sym);

        let t = TYPEOF(v);

        match t {
            NILSXP_I => {
                // nothing
            }
            LISTSXP_I => {
                let mut current = v;
                for _i in 0..n {
                    let namei = ItemName(_names, _i);
                    let _name_guard = protect(namei);
                    if recurse != 0 {
                        NewExtractNames(CAR(current), base, namei, recurse, data, nameData);
                    } else {
                        let new_name =
                            NewName(base, namei, (*nameData).seqno + 1, (*nameData).count);
                        (*nameData).seqno += 1;
                        SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
                        (*data).ans_nnames += 1;
                    }
                    current = CDR(current);
                }
            }
            VECSXP_I | EXPRSXP_I => {
                for i in 0..n {
                    let namei = ItemName(_names, i);
                    if recurse != 0 {
                        NewExtractNames(VECTOR_ELT(v, i), base, namei, recurse, data, nameData);
                    } else {
                        let new_name =
                            NewName(base, namei, (*nameData).seqno + 1, (*nameData).count);
                        (*nameData).seqno += 1;
                        SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
                        (*data).ans_nnames += 1;
                    }
                }
            }
            LGLSXP_I | INTSXP_I | REALSXP_I | CPLXSXP_I | STRSXP_I | RAWSXP_I => {
                for i in 0..n {
                    let namei = ItemName(_names, i);
                    let new_name = NewName(base, namei, (*nameData).seqno + 1, (*nameData).count);
                    (*nameData).seqno += 1;
                    SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
                    (*data).ans_nnames += 1;
                }
            }
            _ => {
                let new_name =
                    NewName(base, R_NilValue(), (*nameData).seqno + 1, (*nameData).count);
                (*nameData).seqno += 1;
                SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
                (*data).ans_nnames += 1;
            }
        }

        if !tag.is_null() && tag != R_NilValue() {
            (*nameData).count = savecount;
        }

        (*nameData).seqno += saveseqno;
    }
}

// ---------------------------------------------------------------------------
// c_Extract_opt -- extract recursive= and use.names= from c() arguments
// ---------------------------------------------------------------------------

/// Remove optional named arguments (recursive, use.names) from the `c()`
/// argument list, returning the cleaned list.
pub unsafe fn c_Extract_opt(
    ans: SEXP,
    recurse: *mut bool,
    usenames: *mut bool,
    call: SEXP,
) -> SEXP {
    unsafe {
        let mut ans = ans;
        let mut last: SEXP = ptr::null_mut();
        let mut next: SEXP;
        let mut n_recurse: c_int = 0;
        let mut n_usenames: c_int = 0;

        let mut a = ans;
        while !a.is_null() && a != R_NilValue() {
            let n = TAG(a);
            next = CDR(a);

            // Check for "recursive" argument
            if !n.is_null() && n != R_NilValue() && !Rf_isNull(n) != 0 && TYPEOF(n) == SYMSXP_I {
                let name = CHAR(PRINTNAME(n));
                if !name.is_null() {
                    let name_str = std::ffi::CStr::from_ptr(name).to_str().unwrap_or("");
                    if name_str.starts_with("recurs") {
                        n_recurse += 1;
                        if n_recurse > 1 {
                            std::panic::panic_any(crate::sexp::context::RError {
                                message: "repeated formal argument 'recursive'".to_string(),
                            });
                        }
                        // Check if CAR(a) is a logical
                        let val = CAR(a);
                        if !val.is_null() && val != R_NilValue() && TYPEOF(val) == LGLSXP_I {
                            let v = *LOGICAL(val);
                            if v != NA_LOGICAL {
                                *recurse = v != 0;
                            }
                        }
                        if last.is_null() {
                            ans = next;
                        } else {
                            SETCDR(last, next);
                        }
                        a = next;
                        continue;
                    }
                    if name_str.starts_with("use.name") {
                        n_usenames += 1;
                        if n_usenames > 1 {
                            std::panic::panic_any(crate::sexp::context::RError {
                                message: "repeated formal argument 'use.names'".to_string(),
                            });
                        }
                        let val = CAR(a);
                        if !val.is_null() && val != R_NilValue() && TYPEOF(val) == LGLSXP_I {
                            let v = *LOGICAL(val);
                            if v != NA_LOGICAL {
                                *usenames = v != 0;
                            }
                        }
                        if last.is_null() {
                            ans = next;
                        } else {
                            SETCDR(last, next);
                        }
                        a = next;
                        continue;
                    }
                }
            }

            last = a;
            a = next;
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// Determine the result mode from ans_flags
// ---------------------------------------------------------------------------

/// Given ans_flags bitmask, determine the result SEXPTYPE.
/// Returns NILSXP if no type flags are set.
pub unsafe fn ans_flags_to_mode(flags: c_int) -> SEXPTYPE {
    if flags & 512 != 0 {
        SEXPTYPE::EXPRSXP
    } else if flags & 256 != 0 {
        SEXPTYPE::VECSXP
    } else if flags & 128 != 0 {
        SEXPTYPE::STRSXP
    } else if flags & 64 != 0 {
        SEXPTYPE::CPLXSXP
    } else if flags & 32 != 0 {
        SEXPTYPE::REALSXP
    } else if flags & 16 != 0 {
        SEXPTYPE::INTSXP
    } else if flags & 2 != 0 {
        SEXPTYPE::LGLSXP
    } else if flags & 1 != 0 {
        SEXPTYPE::RAWSXP
    } else {
        SEXPTYPE::NILSXP
    }
}
