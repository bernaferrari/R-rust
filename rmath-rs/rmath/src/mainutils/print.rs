#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/print.c -- print.default() and auto-printing.
//!
//! Provides do_printdefault(), PrintValueRec(), PrintValueEnv(), etc.
//! Faithfully follows R's print.c logic with simplifications for features
//! not yet available (S4 dispatch, source references, Win32 UTF8, etc.).

use std::cell::RefCell;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::eval::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_LevelsSymbol, R_NamesSymbol, R_RowNamesSymbol,
    getAttrib, isObject as isObject_fn,
};
use crate::sexp::accessors::{
    ATTRIB, BODY, CAR, CDR, CHAR, CLOENV, COMPLEX, INTEGER, LENGTH, LOGICAL, PRINTNAME, REAL,
    SET_STRING_ELT, SETCDR, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::{Rf_cons, Rf_mkChar};
use crate::sexp::envir::R_findVarInFrame;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_IsNA, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_NilValue, R_UnboundValue};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const TAGBUFLEN: usize = 256;
const TAGBUFLEN0: usize = TAGBUFLEN + 6;

const Rprt_adj_left: c_int = 0;
const Rprt_adj_right: c_int = 1;

const SIMPLEDEPARSE: c_int = 0;
const DEFAULTDEPARSE: c_int = 1;
const USESOURCE: c_int = 8;

// ---------------------------------------------------------------------------
// Thread-local tagbuf
// ---------------------------------------------------------------------------

thread_local! {
    static TAGBUF: RefCell<[u8; TAGBUFLEN0 * 2]> = RefCell::new([0u8; TAGBUFLEN0 * 2]);
}

unsafe fn tagbuf_strlen() -> usize {
    unsafe { TAGBUF.with(|buf| libc::strlen(buf.borrow().as_ptr() as *const c_char)) }
}

unsafe fn tagbuf_set(idx: usize, val: c_char) {
    TAGBUF.with(|buf| {
        let mut b = buf.borrow_mut();
        if idx < b.len() {
            b[idx] = val as u8;
        }
    });
}

unsafe fn tagbuf_clear() {
    TAGBUF.with(|buf| {
        buf.borrow_mut()[0] = 0;
    });
}

unsafe fn tagbuf_ptr_at(offset: usize) -> *mut c_char {
    TAGBUF.with(|buf| {
        let b = buf.borrow();
        b.as_ptr().wrapping_add(offset) as *mut c_char
    })
}

// ---------------------------------------------------------------------------
// R_PrintData struct
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct R_PrintData {
    pub width: c_int,
    pub gap: c_int,
    pub digits: c_int,
    pub scipen: c_int,
    pub max: c_int,
    pub right: c_int,
    pub quote: c_int,
    pub na_width: c_int,
    pub na_width_noquote: c_int,
    pub na_string: SEXP,
    pub na_string_noquote: SEXP,
    pub useSource: c_int,
    pub cutoff: c_int,
    pub env: SEXP,
    pub callArgs: SEXP,
}

/// Const initializer for static R_PRINT.
const R_PRINT_INIT: R_PrintData = R_PrintData {
    width: 80,
    gap: 1,
    digits: 4,
    scipen: 0,
    max: 99999,
    right: 0,
    quote: 1,
    na_width: 2,
    na_width_noquote: 4,
    na_string: ptr::null_mut(),
    na_string_noquote: ptr::null_mut(),
    useSource: 0,
    cutoff: 60,
    env: ptr::null_mut(),
    callArgs: ptr::null_mut(),
};

thread_local! { static R_PRINT: RefCell<R_PrintData> = RefCell::new(R_PRINT_INIT); }

#[repr(transparent)]
pub struct MutPtr<T>(*mut T);

impl<T> std::ops::Deref for MutPtr<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.0 }
    }
}

impl<T> std::ops::DerefMut for MutPtr<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.0 }
    }
}

pub unsafe fn get_R_print_data() -> MutPtr<R_PrintData> {
    MutPtr(R_PRINT.with(|v| v.as_ptr() as *mut R_PrintData))
}

// ---------------------------------------------------------------------------
// Module-private helpers
// ---------------------------------------------------------------------------

unsafe fn isSymbol(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        if TYPEOF(x) == SEXPTYPE::SYMSXP.0 {
            1
        } else {
            0
        }
    }
}

unsafe fn isString(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP.0 {
            1
        } else {
            0
        }
    }
}

unsafe fn Rf_isNull(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            1
        } else {
            0
        }
    }
}

unsafe fn isFunction(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::CLOSXP.0 || t == SEXPTYPE::BUILTINSXP.0 || t == SEXPTYPE::SPECIALSXP.0 {
            1
        } else {
            0
        }
    }
}

unsafe fn isList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LISTSXP.0 {
            return 1;
        }
        if t == SEXPTYPE::VECSXP.0 && getAttrib(x, R_DimSymbol()) == R_NilValue() {
            return 1;
        }
        0
    }
}

unsafe fn isArray(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let dims = getAttrib(x, R_DimSymbol());
        if dims == R_NilValue() {
            return 0;
        }
        if LENGTH(dims) > 1 { 1 } else { 0 }
    }
}

unsafe fn isDataFrame(_x: SEXP) -> c_int {
    0
}

unsafe fn IS_S4_OBJECT(_x: SEXP) -> c_int {
    0
}

unsafe fn isMethodsDispatchOn() -> c_int {
    0
}

unsafe fn isByteCode(_x: SEXP) -> c_int {
    0
}

/// inherits using a *const c_char class name.
unsafe fn inherits_cstr(x: SEXP, class_name: *const c_char) -> c_int {
    unsafe {
        if x.is_null() || class_name.is_null() {
            return 0;
        }
        let klass = getAttrib(x, R_ClassSymbol());
        if klass == R_NilValue() || TYPEOF(klass) != SEXPTYPE::STRSXP.0 {
            return 0;
        }
        let cn = match CStr::from_ptr(class_name).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let n = LENGTH(klass);
        for i in 0..n {
            let elt = STRING_ELT(klass, i as R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            let s = CStr::from_ptr(CHAR(elt));
            if let Ok(s_str) = s.to_str()
                && s_str == cn
            {
                return 1;
            }
        }
        0
    }
}

unsafe fn translateChar(x: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(x) }
}

/// isValidName stub.
unsafe fn isValidName(s: *const c_char) -> bool {
    unsafe {
        if s.is_null() {
            return false;
        }
        let bytes = CStr::from_ptr(s).to_bytes();
        if bytes.is_empty() {
            return false;
        }
        let first = bytes[0];
        if !(first.is_ascii_alphabetic() || first == b'.') {
            return false;
        }
        bytes
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
    }
}

/// NA_STRING accessor.
unsafe fn NA_STRING_local() -> SEXP {
    unsafe { crate::mainutils::relop::NA_STRING() }
}

/// Get matrix dimnames (delegate to printvector).
unsafe fn GetMatrixDimnames(
    x: SEXP,
    rl: *mut SEXP,
    cl: *mut SEXP,
    rn: *mut *const c_char,
    cn: *mut *const c_char,
) {
    unsafe {
        crate::mainutils::printvector::GetMatrixDimnames(x, rl, cl, rn, cn);
    }
}

/// GetArrayDimnames -- local implementation.
unsafe fn GetArrayDimnames(x: SEXP) -> SEXP {
    unsafe { getAttrib(x, R_DimNamesSymbol()) }
}

/// allocArray (delegate to array.rs).
unsafe fn allocArray(mode: c_int, dims: SEXP) -> SEXP {
    unsafe { crate::mainutils::array::allocArray(mode, dims) }
}

/// asInteger -- local implementation.
unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP.0 {
            if LENGTH(x) >= 1 {
                return *INTEGER(x);
            }
        } else if t == SEXPTYPE::REALSXP.0 {
            if LENGTH(x) >= 1 {
                let v = *REAL(x);
                if ISNAN(v) {
                    return NA_INTEGER;
                }
                return v as c_int;
            }
        } else if t == SEXPTYPE::LGLSXP.0 && LENGTH(x) >= 1 {
            return *LOGICAL(x);
        }
        NA_INTEGER
    }
}

/// asLogical -- local implementation.
unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return NA_LOGICAL;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP.0 {
            if LENGTH(x) >= 1 {
                return *LOGICAL(x);
            }
        } else if t == SEXPTYPE::INTSXP.0 && LENGTH(x) >= 1 {
            return *INTEGER(x);
        }
        NA_LOGICAL
    }
}

/// FixupDigits -- local implementation.
unsafe fn FixupDigits(digits: SEXP, _warn: c_int) -> c_int {
    unsafe {
        let d = asInteger(digits);
        if d == NA_INTEGER {
            return 0;
        }
        if d < 1 {
            return 1;
        }
        if d > 22 {
            return 22;
        }
        d
    }
}

/// FixupWidth -- local implementation.
unsafe fn FixupWidth(width: SEXP, _warn: c_int) -> c_int {
    unsafe {
        let w = asInteger(width);
        if w == NA_INTEGER {
            return 80;
        }
        if w < 10 {
            return 10;
        }
        if w > 10000 {
            return 10000;
        }
        w
    }
}

pub(crate) unsafe fn CHAR_OR_NULL(x: SEXP) -> *const c_char {
    unsafe {
        if x.is_null() {
            return ptr::null();
        }
        CHAR(x)
    }
}

// ---------------------------------------------------------------------------
// PrintInit
// ---------------------------------------------------------------------------

pub unsafe fn PrintInit(data: *mut std::ffi::c_void, env: SEXP) {
    unsafe {
        let d = data as *mut R_PrintData;
        if d.is_null() {
            return;
        }

        (*d).na_string = NA_STRING_local();
        (*d).na_string_noquote = Rf_mkChar(b"<NA>\0".as_ptr() as *const c_char);
        (*d).na_width = crate::mainutils::printutils::Rstrlen((*d).na_string, 0);
        (*d).na_width_noquote = crate::mainutils::printutils::Rstrlen((*d).na_string_noquote, 0);
        (*d).quote = 1;
        (*d).right = Rprt_adj_left;
        (*d).digits = crate::mainutils::options::GetOptionDigits();

        let scipen_sym = Rf_install(b"scipen\0".as_ptr() as *const c_char);
        (*d).scipen = asInteger(crate::mainutils::options::GetOption1(scipen_sym));
        if (*d).scipen == NA_INTEGER {
            (*d).scipen = 0;
        }

        let maxprint_sym = Rf_install(b"max.print\0".as_ptr() as *const c_char);
        (*d).max = asInteger(crate::mainutils::options::GetOption1(maxprint_sym));
        if (*d).max == NA_INTEGER || (*d).max < 0 {
            (*d).max = 99999;
        } else if (*d).max == c_int::MAX {
            (*d).max -= 1;
        }
        (*d).gap = 1;
        (*d).width = crate::mainutils::options::GetOptionWidth();
        (*d).useSource = USESOURCE;
        (*d).cutoff = crate::mainutils::options::GetOptionCutoff();
        (*d).env = env;
        (*d).callArgs = R_NilValue();
    }
}

// ---------------------------------------------------------------------------
// PrintDefaults
// ---------------------------------------------------------------------------

pub unsafe fn PrintDefaults() {
    unsafe {
        let env = R_GlobalEnv();
        R_PRINT.with(|v| {
            PrintInit(v.as_ptr() as *mut std::ffi::c_void, env);
        });
    }
}

// ---------------------------------------------------------------------------
// Internal: advancePrintArgs
// ---------------------------------------------------------------------------

unsafe fn advancePrintArgs(
    args: *mut SEXP,
    prev: *mut SEXP,
    missingArg: *mut *mut c_int,
    allMissing: *mut c_int,
) {
    unsafe {
        *args = CDR(*args);
        if !(*missingArg).is_null() && **missingArg != 0 {
            SETCDR(*prev, *args);
        } else {
            *allMissing = 0;
            *prev = CDR(*prev);
        }
        *missingArg = (*missingArg).add(1);
    }
}

// ---------------------------------------------------------------------------
// Internal: save/restore tagbuf
// ---------------------------------------------------------------------------

unsafe fn save_tagbuf(save: &mut [u8; TAGBUFLEN0 * 2]) {
    unsafe {
        TAGBUF.with(|buf| {
            let b = buf.borrow();
            let len = libc::strlen(b.as_ptr() as *const c_char);
            if len < save.len() {
                ptr::copy_nonoverlapping(b.as_ptr(), save.as_mut_ptr(), len + 1);
            } else {
                save[0] = 0;
            }
        });
    }
}

unsafe fn restore_tagbuf(save: &[u8; TAGBUFLEN0 * 2]) {
    unsafe {
        TAGBUF.with(|buf| {
            let mut b = buf.borrow_mut();
            let len = libc::strlen(save.as_ptr() as *const c_char);
            if len < TAGBUFLEN0 * 2 {
                ptr::copy_nonoverlapping(save.as_ptr(), b.as_mut_ptr(), len + 1);
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintLanguage
// ---------------------------------------------------------------------------

unsafe fn PrintLanguage(s: SEXP, data: &R_PrintData) {
    unsafe {
        let t = crate::mainutils::deparse::deparse1w(s, false, data.useSource | DEFAULTDEPARSE);
        Rf_protect(t);
        R_PRINT.with(|v| *v.borrow_mut() = data.clone());

        let n = LENGTH(t);
        for i in 0..n {
            let elt = STRING_ELT(t, i as R_xlen_t);
            if !elt.is_null() && elt != R_NilValue() {
                println!("{}", CStr::from_ptr(CHAR(elt)).to_str().unwrap_or("?"));
            }
        }
        Rf_unprotect(1);
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintClosure
// ---------------------------------------------------------------------------

unsafe fn PrintClosure(s: SEXP, data: &R_PrintData) {
    unsafe {
        PrintLanguage(s, data);

        if isByteCode(BODY(s)) != 0 {
            println!("<bytecode: {:?}>", BODY(s));
        }
        let t = CLOENV(s);
        if t != R_GlobalEnv() {
            let env_str = crate::mainutils::printutils::EncodeEnvironment(t);
            if !env_str.is_null() {
                println!(
                    "{}",
                    CStr::from_ptr(env_str).to_str().unwrap_or("<environment>")
                );
            } else {
                println!("<environment>");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintSpecial
// ---------------------------------------------------------------------------

unsafe fn PrintSpecial(s: SEXP, data: &R_PrintData) {
    unsafe {
        let nm = crate::eval::builtin::PRIMNAME(s);
        let nm_cstr = std::ffi::CString::new(nm).unwrap_or_default();
        let nm_ptr = nm_cstr.as_ptr();

        let env = R_findVarInFrame(
            R_BaseEnv(),
            Rf_install(b".ArgsEnv\0".as_ptr() as *const c_char),
        );

        let mut s2 = R_findVarInFrame(env, Rf_install(nm_ptr));
        if s2 == R_UnboundValue() {
            let env2 = R_findVarInFrame(
                R_BaseEnv(),
                Rf_install(b".GenericArgsEnv\0".as_ptr() as *const c_char),
            );
            s2 = R_findVarInFrame(env2, Rf_install(nm_ptr));
        }

        if s2 != R_UnboundValue() {
            let t = crate::mainutils::deparse::deparse1m(s2, false, DEFAULTDEPARSE);
            Rf_protect(t);
            R_PRINT.with(|v| *v.borrow_mut() = data.clone());

            let line = STRING_ELT(t, 0);
            if !line.is_null() && line != R_NilValue() {
                print!("{} ", CStr::from_ptr(CHAR(line)).to_str().unwrap_or(""));
            }
            println!(".Primitive(\"{}\")", nm);
            Rf_unprotect(1);
        } else {
            println!(".Primitive(\"{}\")", nm);
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintExpression
// ---------------------------------------------------------------------------

unsafe fn PrintExpression(s: SEXP, data: &R_PrintData) {
    unsafe {
        let u = crate::mainutils::deparse::deparse1w(s, false, data.useSource | DEFAULTDEPARSE);
        Rf_protect(u);
        R_PRINT.with(|v| *v.borrow_mut() = data.clone());

        let n = LENGTH(u);
        for i in 0..n {
            let elt = STRING_ELT(u, i as R_xlen_t);
            if !elt.is_null() && elt != R_NilValue() {
                println!("{}", CStr::from_ptr(CHAR(elt)).to_str().unwrap_or("?"));
            }
        }
        Rf_unprotect(1);
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintDispatch
// ---------------------------------------------------------------------------

unsafe fn PrintDispatch(s: SEXP, data: &R_PrintData) {
    unsafe {
        if isObject_fn(s) != 0 {
            PrintObject(s, data);
        } else {
            PrintValueRec_inner(s, data);
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintObjectS3
// ---------------------------------------------------------------------------

unsafe fn PrintObjectS3(s: SEXP, data: &R_PrintData) {
    // Simplified: just print a message. Full S3 dispatch requires eval.
    println!("<S3 object>");
    let _ = (s, data);
}

// ---------------------------------------------------------------------------
// Internal: PrintObject
// ---------------------------------------------------------------------------

unsafe fn PrintObject(s: SEXP, data: &R_PrintData) {
    unsafe {
        let mut save = [0u8; TAGBUFLEN0 * 2];
        save_tagbuf(&mut save);

        PrintObjectS3(s, data);

        R_PRINT.with(|v| *v.borrow_mut() = data.clone());
        restore_tagbuf(&save);
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintGenericVector
// ---------------------------------------------------------------------------

unsafe fn PrintGenericVector(s: SEXP, data: &R_PrintData) {
    unsafe {
        let ns = XLENGTH(s);
        let dims = getAttrib(s, R_DimSymbol());

        if dims != R_NilValue() && LENGTH(dims) > 1 {
            // Array-like list
            Rf_protect(dims);
            let t = Rf_protect(allocArray(SEXPTYPE::STRSXP.0, dims));

            let limit = if ns <= data.max as i64 + 1 {
                ns
            } else {
                data.max as i64
            };
            for i in 0..limit {
                let s_i = VECTOR_ELT(s, i);
                let mut pbuf = [0u8; 115];

                if isObject_fn(s_i) != 0 {
                    let snip = match TYPEOF(s_i) {
                        x if x == SEXPTYPE::NILSXP.0 => "NULL",
                        x if x == SEXPTYPE::LGLSXP.0 => "logical",
                        x if x == SEXPTYPE::INTSXP.0 => {
                            if inherits_cstr(s_i, b"factor\0".as_ptr() as *const c_char) != 0 {
                                "factor"
                            } else {
                                "integer"
                            }
                        }
                        x if x == SEXPTYPE::REALSXP.0 => "numeric",
                        x if x == SEXPTYPE::CPLXSXP.0 => "complex",
                        x if x == SEXPTYPE::STRSXP.0 => "character",
                        x if x == SEXPTYPE::RAWSXP.0 => "raw",
                        x if x == SEXPTYPE::LISTSXP.0 || x == SEXPTYPE::VECSXP.0 => "list",
                        x if x == SEXPTYPE::LANGSXP.0 => "expression",
                        _ => "?",
                    };
                    let full = format!("{},{}", snip, LENGTH(s_i));
                    let bytes = full.as_bytes();
                    let copy_len = bytes.len().min(114);
                    pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    pbuf[copy_len] = 0;
                } else {
                    match TYPEOF(s_i) {
                        _t if _t == SEXPTYPE::NILSXP.0 => {
                            let src = b"NULL\0";
                            pbuf[..src.len()].copy_from_slice(src);
                        }
                        _t if _t == SEXPTYPE::LGLSXP.0 => {
                            if LENGTH(s_i) == 1 {
                                let x = LOGICAL(s_i);
                                if !x.is_null() {
                                    let mut w: c_int = 0;
                                    crate::mainutils::format::formatLogical(x, 1, &mut w);
                                    let enc = crate::mainutils::printutils::EncodeLogical(*x, w);
                                    if !enc.is_null() {
                                        let cs = CStr::from_ptr(enc);
                                        let bytes = cs.to_bytes();
                                        let copy_len = bytes.len().min(114);
                                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                        pbuf[copy_len] = 0;
                                    }
                                }
                            } else {
                                let full = format!("logical,{}", LENGTH(s_i));
                                let bytes = full.as_bytes();
                                let copy_len = bytes.len().min(114);
                                pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                pbuf[copy_len] = 0;
                            }
                        }
                        _t if _t == SEXPTYPE::INTSXP.0 => {
                            if inherits_cstr(s_i, b"factor\0".as_ptr() as *const c_char) != 0 {
                                let full = format!("factor,{}", LENGTH(s_i));
                                let bytes = full.as_bytes();
                                let copy_len = bytes.len().min(114);
                                pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                pbuf[copy_len] = 0;
                            } else if LENGTH(s_i) == 1 {
                                let x = INTEGER(s_i);
                                if !x.is_null() {
                                    let mut w: c_int = 0;
                                    crate::mainutils::format::formatInteger(x, 1, &mut w);
                                    let enc = crate::mainutils::printutils::EncodeInteger(*x, w);
                                    if !enc.is_null() {
                                        let cs = CStr::from_ptr(enc);
                                        let bytes = cs.to_bytes();
                                        let copy_len = bytes.len().min(114);
                                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                        pbuf[copy_len] = 0;
                                    }
                                }
                            } else {
                                let full = format!("integer,{}", LENGTH(s_i));
                                let bytes = full.as_bytes();
                                let copy_len = bytes.len().min(114);
                                pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                pbuf[copy_len] = 0;
                            }
                        }
                        _t if _t == SEXPTYPE::REALSXP.0 => {
                            if LENGTH(s_i) == 1 {
                                let x = REAL(s_i);
                                if !x.is_null() {
                                    let mut w: c_int = 0;
                                    let mut d: c_int = 0;
                                    let mut e: c_int = 0;
                                    crate::mainutils::format::formatReal(
                                        x, 1, &mut w, &mut d, &mut e, 0,
                                    );
                                    let outdec: *const c_char = b".\0".as_ptr() as *const c_char;
                                    let enc = crate::mainutils::printutils::EncodeReal0(
                                        *x, w, d, e, outdec,
                                    );
                                    if !enc.is_null() {
                                        let cs = CStr::from_ptr(enc);
                                        let bytes = cs.to_bytes();
                                        let copy_len = bytes.len().min(114);
                                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                        pbuf[copy_len] = 0;
                                    }
                                }
                            } else {
                                let full = format!("numeric,{}", LENGTH(s_i));
                                let bytes = full.as_bytes();
                                let copy_len = bytes.len().min(114);
                                pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                pbuf[copy_len] = 0;
                            }
                        }
                        _t if _t == SEXPTYPE::CPLXSXP.0 => {
                            if LENGTH(s_i) == 1 {
                                let x = COMPLEX(s_i);
                                if !x.is_null() {
                                    let cx = *x;
                                    if R_IsNA(cx.r) || R_IsNA(cx.i) {
                                        let outdec: *const c_char =
                                            b".\0".as_ptr() as *const c_char;
                                        let enc = crate::mainutils::printutils::EncodeReal0(
                                            NA_REAL,
                                            data.na_width,
                                            0,
                                            0,
                                            outdec,
                                        );
                                        if !enc.is_null() {
                                            let cs = CStr::from_ptr(enc);
                                            let bytes = cs.to_bytes();
                                            let copy_len = bytes.len().min(114);
                                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                            pbuf[copy_len] = 0;
                                        }
                                    } else {
                                        let mut wr: c_int = 0;
                                        let mut dr: c_int = 0;
                                        let mut er: c_int = 0;
                                        let mut wi: c_int = 0;
                                        let mut di: c_int = 0;
                                        let mut ei: c_int = 0;
                                        crate::mainutils::format::formatComplex(
                                            x, 1, &mut wr, &mut dr, &mut er, &mut wi, &mut di,
                                            &mut ei, 0,
                                        );
                                        let outdec: *const c_char =
                                            b".\0".as_ptr() as *const c_char;
                                        let enc = crate::mainutils::printutils::EncodeComplex(
                                            cx, wr, dr, er, wi, di, ei, outdec,
                                        );
                                        if !enc.is_null() {
                                            let cs = CStr::from_ptr(enc);
                                            let bytes = cs.to_bytes();
                                            let copy_len = bytes.len().min(114);
                                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                            pbuf[copy_len] = 0;
                                        }
                                    }
                                }
                            } else {
                                let full = format!("complex,{}", LENGTH(s_i));
                                let bytes = full.as_bytes();
                                let copy_len = bytes.len().min(114);
                                pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                pbuf[copy_len] = 0;
                            }
                        }
                        _t if _t == SEXPTYPE::STRSXP.0 => {
                            if LENGTH(s_i) == 1 {
                                let ctmp = translateChar(STRING_ELT(s_i, 0));
                                if !ctmp.is_null() {
                                    let s_str = CStr::from_ptr(ctmp).to_str().unwrap_or("");
                                    let quoted = if s_str.len() < 100 {
                                        format!("\"{}\"", s_str)
                                    } else {
                                        format!("\"{}\" [truncated]", &s_str[..99])
                                    };
                                    let bytes = quoted.as_bytes();
                                    let copy_len = bytes.len().min(114);
                                    pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                    pbuf[copy_len] = 0;
                                }
                            } else {
                                let full = format!("character,{}", LENGTH(s_i));
                                let bytes = full.as_bytes();
                                let copy_len = bytes.len().min(114);
                                pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                pbuf[copy_len] = 0;
                            }
                        }
                        _t if _t == SEXPTYPE::RAWSXP.0 => {
                            let full = format!("raw,{}", LENGTH(s_i));
                            let bytes = full.as_bytes();
                            let copy_len = bytes.len().min(114);
                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                            pbuf[copy_len] = 0;
                        }
                        _t if _t == SEXPTYPE::LISTSXP.0 || _t == SEXPTYPE::VECSXP.0 => {
                            let full = format!("list,{}", LENGTH(s_i));
                            let bytes = full.as_bytes();
                            let copy_len = bytes.len().min(114);
                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                            pbuf[copy_len] = 0;
                        }
                        _t if _t == SEXPTYPE::LANGSXP.0 => {
                            let src = b"expression\0";
                            pbuf[..src.len()].copy_from_slice(src);
                        }
                        _ => {
                            let src = b"?\0";
                            pbuf[..src.len()].copy_from_slice(src);
                        }
                    }
                }
                pbuf[114] = 0;
                SET_STRING_ELT(t, i, Rf_mkChar(pbuf.as_ptr() as *const c_char));
            }

            if LENGTH(dims) == 2 {
                let mut rl: SEXP = R_NilValue();
                let mut cl: SEXP = R_NilValue();
                let mut rn: *const c_char = ptr::null();
                let mut cn: *const c_char = ptr::null();
                GetMatrixDimnames(s, &mut rl, &mut cl, &mut rn, &mut cn);
                crate::mainutils::printarray::printMatrix(
                    t, 0, dims, 0, data.right, rl, cl, rn, cn,
                );
            } else {
                let names = Rf_protect(GetArrayDimnames(s));
                crate::mainutils::printarray::printArray(t, dims, 0, Rprt_adj_left, names);
                Rf_unprotect(1);
            }
            Rf_unprotect(2); // dims, t
        } else {
            // No dim
            let names = Rf_protect(getAttrib(s, R_NamesSymbol()));

            let taglen = tagbuf_strlen();
            let ptag = tagbuf_ptr_at(taglen);
            let sz = TAGBUFLEN0 * 2 - taglen;

            if ns > 0 {
                let n_pr = if ns <= data.max as i64 + 1 {
                    ns
                } else {
                    data.max as i64
                };
                for i in 0..n_pr {
                    if i > 0 {
                        println!();
                    }

                    if names != R_NilValue() && i < XLENGTH(names) {
                        let name_elt = STRING_ELT(names, i);
                        if !name_elt.is_null() && name_elt != R_NilValue() {
                            let name_chars = CHAR(name_elt);
                            if !name_chars.is_null() && *name_chars != 0 {
                                let name_len = libc::strlen(name_chars);
                                if taglen + name_len > TAGBUFLEN {
                                    if taglen <= TAGBUFLEN {
                                        libc::snprintf(
                                            ptag,
                                            sz,
                                            b"$...\0".as_ptr() as *const c_char,
                                        );
                                    }
                                } else {
                                    let na_str = NA_STRING_local();
                                    if name_elt == na_str {
                                        libc::snprintf(
                                            ptag,
                                            sz,
                                            b"$<NA>\0".as_ptr() as *const c_char,
                                        );
                                    } else if isValidName(name_chars) {
                                        libc::snprintf(
                                            ptag,
                                            sz,
                                            b"$%s\0".as_ptr() as *const c_char,
                                            name_chars,
                                        );
                                    } else {
                                        let enc =
                                            crate::mainutils::printutils::EncodeChar(name_elt);
                                        if !enc.is_null() && isValidName(enc) {
                                            libc::snprintf(
                                                ptag,
                                                sz,
                                                b"$%s\0".as_ptr() as *const c_char,
                                                enc,
                                            );
                                        } else if !enc.is_null() {
                                            libc::snprintf(
                                                ptag,
                                                sz,
                                                b"$`%s`\0".as_ptr() as *const c_char,
                                                enc,
                                            );
                                        } else {
                                            libc::snprintf(
                                                ptag,
                                                sz,
                                                b"$...\0".as_ptr() as *const c_char,
                                            );
                                        }
                                    }
                                }
                            } else {
                                let iw = crate::mainutils::printutils::IndexWidth_xlen(i);
                                if taglen + iw as usize > TAGBUFLEN {
                                    if taglen <= TAGBUFLEN {
                                        libc::snprintf(
                                            ptag,
                                            sz,
                                            b"$...\0".as_ptr() as *const c_char,
                                        );
                                    }
                                } else {
                                    libc::snprintf(
                                        ptag,
                                        sz,
                                        b"[[%lld]]\0".as_ptr() as *const c_char,
                                        i + 1,
                                    );
                                }
                            }
                        } else {
                            let iw = crate::mainutils::printutils::IndexWidth_xlen(i);
                            if taglen + iw as usize > TAGBUFLEN {
                                if taglen <= TAGBUFLEN {
                                    libc::snprintf(ptag, sz, b"$...\0".as_ptr() as *const c_char);
                                }
                            } else {
                                libc::snprintf(
                                    ptag,
                                    sz,
                                    b"[[%lld]]\0".as_ptr() as *const c_char,
                                    i + 1,
                                );
                            }
                        }
                    } else {
                        let iw = crate::mainutils::printutils::IndexWidth_xlen(i);
                        if taglen + iw as usize > TAGBUFLEN {
                            if taglen <= TAGBUFLEN {
                                libc::snprintf(ptag, sz, b"$...\0".as_ptr() as *const c_char);
                            }
                        } else {
                            libc::snprintf(
                                ptag,
                                sz,
                                b"[[%lld]]\0".as_ptr() as *const c_char,
                                i + 1,
                            );
                        }
                    }

                    // Print tag line
                    TAGBUF.with(|buf| {
                        let b = buf.borrow();
                        let s = CStr::from_ptr(b.as_ptr() as *const c_char)
                            .to_str()
                            .unwrap_or("");
                        println!("{}", s);
                    });

                    PrintDispatch(VECTOR_ELT(s, i), data);

                    tagbuf_set(taglen, 0);
                }
                println!();
                if (n_pr as i64) < ns {
                    println!(
                        " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]",
                        ns - n_pr as i64
                    );
                }
            } else {
                // Empty list
                if names != R_NilValue() {
                    print!("named ");
                }
                println!("list()");
            }
            Rf_unprotect(1); // names
        }
        printAttributes(s, data, false);
    }
}

// ---------------------------------------------------------------------------
// Internal: printList
// ---------------------------------------------------------------------------

unsafe fn printList(s: SEXP, data: &R_PrintData) {
    unsafe {
        let dims = getAttrib(s, R_DimSymbol());

        if dims != R_NilValue() && LENGTH(dims) > 1 {
            Rf_protect(dims);
            let t = Rf_protect(allocArray(SEXPTYPE::STRSXP.0, dims));
            let mut i: i64 = 0;
            let mut cur = s;

            while cur != R_NilValue() && TYPEOF(cur) == SEXPTYPE::LISTSXP.0 {
                let mut pbuf = [0u8; 101];
                match TYPEOF(CAR(cur)) {
                    _t if _t == SEXPTYPE::NILSXP.0 => {
                        let src = b"NULL\0";
                        pbuf[..src.len()].copy_from_slice(src);
                    }
                    _t if _t == SEXPTYPE::LGLSXP.0 => {
                        let full = format!("logical,{}", LENGTH(CAR(cur)));
                        let bytes = full.as_bytes();
                        let copy_len = bytes.len().min(100);
                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        pbuf[copy_len] = 0;
                    }
                    _t if _t == SEXPTYPE::INTSXP.0 || _t == SEXPTYPE::REALSXP.0 => {
                        let full = format!("numeric,{}", LENGTH(CAR(cur)));
                        let bytes = full.as_bytes();
                        let copy_len = bytes.len().min(100);
                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        pbuf[copy_len] = 0;
                    }
                    _t if _t == SEXPTYPE::CPLXSXP.0 => {
                        let full = format!("complex,{}", LENGTH(CAR(cur)));
                        let bytes = full.as_bytes();
                        let copy_len = bytes.len().min(100);
                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        pbuf[copy_len] = 0;
                    }
                    _t if _t == SEXPTYPE::STRSXP.0 => {
                        let full = format!("character,{}", LENGTH(CAR(cur)));
                        let bytes = full.as_bytes();
                        let copy_len = bytes.len().min(100);
                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        pbuf[copy_len] = 0;
                    }
                    _t if _t == SEXPTYPE::RAWSXP.0 => {
                        let full = format!("raw,{}", LENGTH(CAR(cur)));
                        let bytes = full.as_bytes();
                        let copy_len = bytes.len().min(100);
                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        pbuf[copy_len] = 0;
                    }
                    _t if _t == SEXPTYPE::LISTSXP.0 => {
                        let full = format!("list,{}", LENGTH(CAR(cur)));
                        let bytes = full.as_bytes();
                        let copy_len = bytes.len().min(100);
                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        pbuf[copy_len] = 0;
                    }
                    _t if _t == SEXPTYPE::LANGSXP.0 => {
                        let src = b"expression\0";
                        pbuf[..src.len()].copy_from_slice(src);
                    }
                    _ => {
                        let src = b"?\0";
                        pbuf[..src.len()].copy_from_slice(src);
                    }
                }
                pbuf[100] = 0;
                SET_STRING_ELT(t, i, Rf_mkChar(pbuf.as_ptr() as *const c_char));
                cur = CDR(cur);
                i += 1;
            }

            if LENGTH(dims) == 2 {
                let mut rl: SEXP = R_NilValue();
                let mut cl: SEXP = R_NilValue();
                let mut rn: *const c_char = ptr::null();
                let mut cn: *const c_char = ptr::null();
                GetMatrixDimnames(s, &mut rl, &mut cl, &mut rn, &mut cn);
                crate::mainutils::printarray::printMatrix(
                    t, 0, dims, data.quote, data.right, rl, cl, rn, cn,
                );
            } else {
                let dimnames = Rf_protect(getAttrib(s, R_DimNamesSymbol()));
                crate::mainutils::printarray::printArray(t, dims, 0, Rprt_adj_left, dimnames);
                Rf_unprotect(1);
            }
            Rf_unprotect(2); // dims, t
        } else {
            let mut i: c_int = 1;
            let mut cur = s;

            let taglen = tagbuf_strlen();
            let ptag = tagbuf_ptr_at(taglen);
            let sz = TAGBUFLEN0 * 2 - taglen;

            while TYPEOF(cur) == SEXPTYPE::LISTSXP.0 {
                if i > 1 {
                    println!();
                }

                let tag = TAG(cur);
                if tag != R_NilValue() && isSymbol(tag) != 0 {
                    let pname = PRINTNAME(tag);
                    let name_chars = CHAR(pname);
                    let name_len = libc::strlen(name_chars);
                    if taglen + name_len > TAGBUFLEN {
                        if taglen <= TAGBUFLEN {
                            libc::snprintf(ptag, sz, b"$...\0".as_ptr() as *const c_char);
                        }
                    } else {
                        let na_str = NA_STRING_local();
                        if pname == na_str {
                            libc::snprintf(ptag, sz, b"$<NA>\0".as_ptr() as *const c_char);
                        } else if isValidName(name_chars) {
                            libc::snprintf(
                                ptag,
                                sz,
                                b"$%s\0".as_ptr() as *const c_char,
                                name_chars,
                            );
                        } else {
                            let enc = crate::mainutils::printutils::EncodeChar(pname);
                            if !enc.is_null() && isValidName(enc) {
                                libc::snprintf(ptag, sz, b"$%s\0".as_ptr() as *const c_char, enc);
                            } else if !enc.is_null() {
                                libc::snprintf(ptag, sz, b"$`%s`\0".as_ptr() as *const c_char, enc);
                            } else {
                                libc::snprintf(ptag, sz, b"$...\0".as_ptr() as *const c_char);
                            }
                        }
                    }
                } else {
                    let iw = crate::mainutils::printutils::IndexWidth_xlen(i as i64);
                    if taglen + iw as usize > TAGBUFLEN {
                        if taglen <= TAGBUFLEN {
                            libc::snprintf(ptag, sz, b"$...\0".as_ptr() as *const c_char);
                        }
                    } else {
                        libc::snprintf(ptag, sz, b"[[%d]]\0".as_ptr() as *const c_char, i);
                    }
                }

                TAGBUF.with(|buf| {
                    let b = buf.borrow();
                    let s = CStr::from_ptr(b.as_ptr() as *const c_char)
                        .to_str()
                        .unwrap_or("");
                    println!("{}", s);
                });

                PrintDispatch(CAR(cur), data);

                tagbuf_set(taglen, 0);
                cur = CDR(cur);
                i += 1;
            }

            if cur != R_NilValue() {
                print!("\n. \n\n");
                PrintValueRec_inner(cur, data);
            }
            println!();
        }
        printAttributes(s, data, false);
    }
}

// ---------------------------------------------------------------------------
// Internal: printAttributes
// ---------------------------------------------------------------------------

unsafe fn printAttributes(s: SEXP, data: &R_PrintData, useSlots: bool) {
    unsafe {
        let mut a = ATTRIB(s);
        if a == R_NilValue() {
            return;
        }

        let current_len = tagbuf_strlen();
        if current_len > TAGBUFLEN0 {
            return;
        }

        let mut save = [0u8; TAGBUFLEN0 * 2];
        save_tagbuf(&mut save);

        // Remove list tag if it looks like a list, not an attribute
        if current_len > 0 {
            let last = tagbuf_ptr_at(current_len - 1);
            if !last.is_null() && *last != b')' as c_char {
                tagbuf_clear();
            }
        }

        let comment_sym = Rf_install(b"comment\0".as_ptr() as *const c_char);
        let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
        let whole_srcref_sym = Rf_install(b"wholeSrcref\0".as_ptr() as *const c_char);
        let srcfile_sym = Rf_install(b"srcfile\0".as_ptr() as *const c_char);

        while a != R_NilValue() {
            let tag = TAG(a);

            // Skip certain attributes
            if useSlots && tag == R_ClassSymbol() {
                a = CDR(a);
                continue;
            }
            if isArray(s) != 0 && (tag == R_DimSymbol() || tag == R_DimNamesSymbol()) {
                a = CDR(a);
                continue;
            }
            if inherits_cstr(s, b"factor\0".as_ptr() as *const c_char) != 0
                && (tag == R_LevelsSymbol() || tag == R_ClassSymbol())
            {
                a = CDR(a);
                continue;
            }
            if isDataFrame(s) != 0 && tag == R_RowNamesSymbol() {
                a = CDR(a);
                continue;
            }
            if isArray(s) == 0 && tag == R_NamesSymbol() {
                a = CDR(a);
                continue;
            }
            if tag == comment_sym
                || tag == srcref_sym
                || tag == whole_srcref_sym
                || tag == srcfile_sym
            {
                a = CDR(a);
                continue;
            }

            // Build the attribute tag
            let space = TAGBUFLEN0 - tagbuf_strlen();
            let ptag_start = tagbuf_ptr_at(tagbuf_strlen());

            if !tag.is_null() && isSymbol(tag) != 0 {
                let pname = PRINTNAME(tag);
                let enc = crate::mainutils::printutils::EncodeChar(pname);
                if !enc.is_null() {
                    if useSlots {
                        libc::snprintf(
                            ptag_start,
                            space,
                            b"Slot \"%s\":\0".as_ptr() as *const c_char,
                            enc,
                        );
                    } else {
                        libc::snprintf(
                            ptag_start,
                            space,
                            b"attr(,\"%s\")\0".as_ptr() as *const c_char,
                            enc,
                        );
                    }
                }
            }

            TAGBUF.with(|buf| {
                let b = buf.borrow();
                let s = CStr::from_ptr(b.as_ptr() as *const c_char)
                    .to_str()
                    .unwrap_or("");
                println!("{}", s);
            });

            if tag == R_RowNamesSymbol() {
                let val = Rf_protect(getAttrib(s, R_RowNamesSymbol()));
                PrintValueRec_inner(val, data);
                Rf_unprotect(1);
                tagbuf_set(tagbuf_strlen(), 0);
                a = CDR(a);
                continue;
            }

            PrintDispatch(CAR(a), data);
            tagbuf_set(tagbuf_strlen(), 0);
            a = CDR(a);
        }

        restore_tagbuf(&save);
    }
}

// ---------------------------------------------------------------------------
// PrintValueRec
// ---------------------------------------------------------------------------

pub unsafe fn PrintValueRec(s: SEXP, _data: *mut std::ffi::c_void) {
    unsafe {
        let data = _data as *const R_PrintData;
        if data.is_null() {
            PrintDefaults();
            R_PRINT.with(|v| PrintValueRec_inner(s, &*v.as_ptr()));
        } else {
            PrintValueRec_inner(s, &*data);
        }
    }
}

unsafe fn PrintValueRec_inner(s: SEXP, data: &R_PrintData) {
    unsafe {
        if s.is_null() {
            return;
        }

        if isMethodsDispatchOn() == 0 && (IS_S4_OBJECT(s) != 0 || TYPEOF(s) == SEXPTYPE(25).0) {
            println!("<S4 object>");
            return;
        }

        match TYPEOF(s) {
            t if t == SEXPTYPE::NILSXP.0 => {
                println!("NULL");
            }
            t if t == SEXPTYPE::SYMSXP.0 => {
                let t = crate::mainutils::deparse::deparse1(s, false, SIMPLEDEPARSE);
                Rf_protect(t);
                R_PRINT.with(|v| *v.borrow_mut() = data.clone());
                let line = STRING_ELT(t, 0);
                if !line.is_null() && line != R_NilValue() {
                    println!("{}", CStr::from_ptr(CHAR(line)).to_str().unwrap_or("?"));
                }
                Rf_unprotect(1);
            }
            t if t == SEXPTYPE::SPECIALSXP.0 || t == SEXPTYPE::BUILTINSXP.0 => {
                PrintSpecial(s, data);
            }
            t if t == SEXPTYPE::CHARSXP.0 => {
                print!("<CHARSXP: ");
                let enc = crate::mainutils::printutils::EncodeString(
                    s,
                    0,
                    b'"' as c_int,
                    crate::mainutils::printutils::Rprt_adj::none,
                );
                if !enc.is_null() {
                    print!("{}", CStr::from_ptr(enc).to_str().unwrap_or(""));
                }
                println!(">");
                return; // skip attributes for CHARSXP
            }
            t if t == SEXPTYPE::EXPRSXP.0 => {
                PrintExpression(s, data);
            }
            t if t == SEXPTYPE::LANGSXP.0 => {
                PrintLanguage(s, data);
            }
            t if t == SEXPTYPE::CLOSXP.0 => {
                PrintClosure(s, data);
            }
            t if t == SEXPTYPE::ENVSXP.0 => {
                let env_str = crate::mainutils::printutils::EncodeEnvironment(s);
                if !env_str.is_null() {
                    println!(
                        "{}",
                        CStr::from_ptr(env_str).to_str().unwrap_or("<environment>")
                    );
                } else {
                    println!("<environment>");
                }
            }
            t if t == SEXPTYPE::PROMSXP.0 => {
                println!("<promise: {:?}>", s);
            }
            t if t == SEXPTYPE::DOTSXP.0 => {
                println!("<...>");
            }
            t if t == SEXPTYPE::VECSXP.0 => {
                PrintGenericVector(s, data);
                return; // handles attributes
            }
            t if t == SEXPTYPE::LISTSXP.0 => {
                printList(s, data);
            }
            t if t == SEXPTYPE::LGLSXP.0
                || t == SEXPTYPE::INTSXP.0
                || t == SEXPTYPE::REALSXP.0
                || t == SEXPTYPE::STRSXP.0
                || t == SEXPTYPE::CPLXSXP.0
                || t == SEXPTYPE::RAWSXP.0 =>
            {
                let dim = Rf_protect(getAttrib(s, R_DimSymbol()));
                if TYPEOF(dim) == SEXPTYPE::INTSXP.0 {
                    if LENGTH(dim) == 1 {
                        let dnames = getAttrib(s, R_DimNamesSymbol());
                        if dnames != R_NilValue() && VECTOR_ELT(dnames, 0) != R_NilValue() {
                            let nn = getAttrib(dnames, R_NamesSymbol());
                            let title = if nn != R_NilValue() && LENGTH(nn) > 0 {
                                let title_elt = STRING_ELT(nn, 0);
                                if !title_elt.is_null() && title_elt != R_NilValue() {
                                    CHAR(title_elt)
                                } else {
                                    ptr::null()
                                }
                            } else {
                                ptr::null()
                            };
                            crate::mainutils::printvector::printNamedVector(
                                s,
                                VECTOR_ELT(dnames, 0),
                                data.quote,
                                title,
                            );
                        } else {
                            crate::mainutils::printvector::printVector(s, 1, data.quote);
                        }
                    } else if LENGTH(dim) == 2 {
                        let mut rl: SEXP = R_NilValue();
                        let mut cl: SEXP = R_NilValue();
                        let mut rn: *const c_char = ptr::null();
                        let mut cn: *const c_char = ptr::null();
                        GetMatrixDimnames(s, &mut rl, &mut cl, &mut rn, &mut cn);
                        crate::mainutils::printarray::printMatrix(
                            s, 0, dim, data.quote, data.right, rl, cl, rn, cn,
                        );
                    } else {
                        let dimnames = Rf_protect(GetArrayDimnames(s));
                        crate::mainutils::printarray::printArray(
                            s, dim, data.quote, data.right, dimnames,
                        );
                        Rf_unprotect(1);
                    }
                } else {
                    Rf_unprotect(1); // dim
                    let names = Rf_protect(getAttrib(s, R_NamesSymbol()));
                    if names != R_NilValue() {
                        crate::mainutils::printvector::printNamedVector(
                            s,
                            names,
                            data.quote,
                            ptr::null(),
                        );
                    } else {
                        crate::mainutils::printvector::printVector(s, 1, data.quote);
                    }
                    Rf_unprotect(1);
                    printAttributes(s, data, false);
                    return;
                }
                Rf_unprotect(1); // dim
            }
            t if t == SEXPTYPE::EXTPTRSXP.0 => {
                println!("<pointer: {:?}>", s);
            }
            t if t == SEXPTYPE::BCODESXP.0 => {
                println!("<bytecode: {:?}>", s);
            }
            t if t == SEXPTYPE::WEAKREFSXP.0 => {
                println!("<weak reference>");
            }
            t if t == SEXPTYPE(25).0 => {
                // OBJSXP
                if IS_S4_OBJECT(s) != 0 {
                    println!("<S4 Type Object>");
                } else {
                    println!("<object>");
                }
            }
            _ => {
                println!("<unknown type {}>", TYPEOF(s));
            }
        }

        printAttributes(s, data, false);
    }
}

// ---------------------------------------------------------------------------
// PrintValue
// ---------------------------------------------------------------------------

pub unsafe fn PrintValue(s: SEXP) {
    unsafe {
        PrintValueEnv(s, R_GlobalEnv());
    }
}

// ---------------------------------------------------------------------------
// PrintValueEnv
// ---------------------------------------------------------------------------

pub unsafe fn PrintValueEnv(s: SEXP, env: SEXP) {
    unsafe {
        if s.is_null() {
            return;
        }

        PrintDefaults();
        tagbuf_clear();

        Rf_protect(s);

        let mut data = R_PRINT_INIT.clone();
        PrintInit(&mut data as *mut R_PrintData as *mut std::ffi::c_void, env);

        if isFunction(s) != 0 {
            PrintObject(s, &data);
        } else {
            PrintDispatch(s, &data);
        }

        Rf_unprotect(1);
    }
}

// ---------------------------------------------------------------------------
// R_PV
// ---------------------------------------------------------------------------

pub unsafe fn R_PV(s: SEXP) {
    unsafe {
        if !s.is_null() && isObject_fn(s) != 0 {
            PrintValueEnv(s, R_GlobalEnv());
        }
    }
}

// ---------------------------------------------------------------------------
// CustomPrintValue
// ---------------------------------------------------------------------------

pub unsafe fn CustomPrintValue(s: SEXP, env: SEXP) {
    unsafe {
        tagbuf_clear();

        let mut data = R_PRINT_INIT.clone();
        PrintInit(&mut data as *mut R_PrintData as *mut std::ffi::c_void, env);
        PrintValueRec_inner(s, &data);
    }
}

// ---------------------------------------------------------------------------
// do_printdefault
// ---------------------------------------------------------------------------

pub unsafe fn do_printdefault(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if Rf_isNull(args) != 0 {
            return ptr::null_mut();
        }

        let x = CAR(args);
        let mut args_rest = CDR(args);

        let mut data = R_PRINT_INIT.clone();
        PrintInit(&mut data as *mut R_PrintData as *mut std::ffi::c_void, rho);

        // Simplified: if not enough args, just print directly
        if args_rest == R_NilValue() || CDR(args_rest) == R_NilValue() {
            R_PRINT.with(|v| *v.borrow_mut() = data.clone());
            tagbuf_clear();
            PrintValueRec_inner(x, &data);
            PrintDefaults();
            return x;
        }

        // Full path: .Internal(print.default(x, args, missings))
        let missings_vec = CAR(args_rest);
        args_rest = CDR(args_rest);

        let wrapped_args = CAR(args_rest);
        let mut missing_arg_ptr = LOGICAL(missings_vec);
        let mut all_missing: c_int = 1;

        let orig = Rf_protect(Rf_cons(R_NilValue(), wrapped_args));
        let mut prev = orig;
        let mut cur_args = wrapped_args;

        // digits
        if cur_args != R_NilValue() && Rf_isNull(CAR(cur_args)) == 0 {
            data.digits = FixupDigits(CAR(cur_args), 2);
        }
        if cur_args != R_NilValue() {
            advancePrintArgs(
                &mut cur_args,
                &mut prev,
                &mut missing_arg_ptr,
                &mut all_missing,
            );
        }

        // quote
        if cur_args != R_NilValue() {
            data.quote = asLogical(CAR(cur_args));
            if data.quote == NA_LOGICAL {
                data.quote = 1;
            }
            advancePrintArgs(
                &mut cur_args,
                &mut prev,
                &mut missing_arg_ptr,
                &mut all_missing,
            );
        }

        // na.print
        if cur_args != R_NilValue() {
            let naprint = CAR(cur_args);
            if Rf_isNull(naprint) == 0 && isString(naprint) != 0 && LENGTH(naprint) >= 1 {
                let na_str = STRING_ELT(naprint, 0);
                data.na_string = na_str;
                data.na_string_noquote = na_str;
                data.na_width = crate::mainutils::printutils::Rstrlen(data.na_string, 0);
                data.na_width_noquote = data.na_width;
            }
            advancePrintArgs(
                &mut cur_args,
                &mut prev,
                &mut missing_arg_ptr,
                &mut all_missing,
            );
        }

        // gap
        if cur_args != R_NilValue() {
            let gap = CAR(cur_args);
            if Rf_isNull(gap) == 0 {
                data.gap = asInteger(gap);
                if data.gap == NA_INTEGER || data.gap < 0 {
                    data.gap = 1;
                }
                if data.gap > 1024 {
                    data.gap = 1;
                }
            }
            advancePrintArgs(
                &mut cur_args,
                &mut prev,
                &mut missing_arg_ptr,
                &mut all_missing,
            );
        }

        // right
        if cur_args != R_NilValue() {
            let right_val = asLogical(CAR(cur_args));
            if right_val != NA_LOGICAL {
                data.right = right_val;
            }
            advancePrintArgs(
                &mut cur_args,
                &mut prev,
                &mut missing_arg_ptr,
                &mut all_missing,
            );
        }

        // max
        if cur_args != R_NilValue() {
            let max_val = CAR(cur_args);
            if Rf_isNull(max_val) == 0 {
                data.max = asInteger(max_val);
                if data.max == NA_INTEGER || data.max < 0 {
                    data.max = 99999;
                } else if data.max == c_int::MAX {
                    data.max -= 1;
                }
            }
            advancePrintArgs(
                &mut cur_args,
                &mut prev,
                &mut missing_arg_ptr,
                &mut all_missing,
            );
        }

        // width
        if cur_args != R_NilValue() {
            let width_val = CAR(cur_args);
            if Rf_isNull(width_val) == 0 {
                data.width = FixupWidth(width_val, 2);
            }
            advancePrintArgs(
                &mut cur_args,
                &mut prev,
                &mut missing_arg_ptr,
                &mut all_missing,
            );
        }

        // useSource
        if cur_args != R_NilValue() {
            let use_source = asLogical(CAR(cur_args));
            if use_source != NA_LOGICAL {
                data.useSource = if use_source != 0 { USESOURCE } else { 0 };
            }
            advancePrintArgs(
                &mut cur_args,
                &mut prev,
                &mut missing_arg_ptr,
                &mut all_missing,
            );
        }

        let no_params = if all_missing != 0 && cur_args == R_NilValue() {
            1
        } else {
            0
        };
        data.callArgs = CDR(orig);

        R_PRINT.with(|v| *v.borrow_mut() = data.clone());

        tagbuf_clear();

        if no_params != 0 && IS_S4_OBJECT(x) != 0 && isMethodsDispatchOn() != 0 {
            PrintObject(x, &data);
        } else {
            PrintValueRec_inner(x, &data);
        }

        PrintDefaults();
        Rf_unprotect(1); // orig
        x
    }
}

// ---------------------------------------------------------------------------
// do_prmatrix
// ---------------------------------------------------------------------------

pub unsafe fn do_prmatrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut a = args;
        let x = CAR(a);
        a = CDR(a);
        let rowlab = CAR(a);
        a = CDR(a);
        let collab = CAR(a);
        a = CDR(a);

        let quote = asInteger(CAR(a));
        a = CDR(a);
        R_PRINT.with(|v| v.borrow_mut().right = asInteger(CAR(a)));
        a = CDR(a);
        let naprint = CAR(a);

        if Rf_isNull(naprint) == 0 && isString(naprint) != 0 && LENGTH(naprint) >= 1 {
            let na_str = STRING_ELT(naprint, 0);
            R_PRINT.with(|v| {
                let mut ds = v.borrow_mut();
                ds.na_string = na_str;
                ds.na_string_noquote = na_str;
                ds.na_width = crate::mainutils::printutils::Rstrlen(ds.na_string, 0);
                ds.na_width_noquote = ds.na_width;
            });
        }

        let mut rowlab_use = rowlab;
        let mut collab_use = collab;
        if LENGTH(rowlab) == 0 {
            rowlab_use = R_NilValue();
        }
        if LENGTH(collab) == 0 {
            collab_use = R_NilValue();
        }

        let dim = getAttrib(x, R_DimSymbol());
        let right_val = R_PRINT.with(|v| v.borrow().right);
        crate::mainutils::printarray::printMatrix(
            x,
            0,
            dim,
            quote,
            right_val,
            rowlab_use,
            collab_use,
            ptr::null(),
            ptr::null(),
        );
        PrintDefaults();
        x
    }
}

// ---------------------------------------------------------------------------
// do_unclass
// ---------------------------------------------------------------------------

pub unsafe fn do_unclass(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if !x.is_null() {
            crate::eval::attrib_core::setAttrib(x, R_ClassSymbol(), R_NilValue());
        }
        x
    }
}

// ---------------------------------------------------------------------------
// isEnvironment (public, #[unsafe(no_mangle)])
// ---------------------------------------------------------------------------

pub unsafe fn isEnvironment(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        if TYPEOF(x) == SEXPTYPE::ENVSXP.0 {
            1
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// inherits (public, #[unsafe(no_mangle)])
// ---------------------------------------------------------------------------

pub unsafe fn inherits(x: SEXP, class: SEXP, _pkg: SEXP) -> c_int {
    unsafe {
        if class.is_null() || TYPEOF(class) != SEXPTYPE::STRSXP.0 || LENGTH(class) < 1 {
            return 0;
        }
        let class_name = CHAR(STRING_ELT(class, 0));
        if class_name.is_null() {
            return 0;
        }
        inherits_cstr(x, class_name)
    }
}

// ---------------------------------------------------------------------------
// Module-private helpers
// ---------------------------------------------------------------------------

unsafe fn GetOptionDigits() -> c_int {
    unsafe { crate::mainutils::options::GetOptionDigits() }
}

unsafe fn GetOption1(sym: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::GetOption1(sym) }
}

unsafe fn do_names(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn do_dim(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn do_str(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn test_do_printdefault_null() {
        unsafe {
            let result = do_printdefault(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_get_option_digits() {
        unsafe {
            assert!(GetOptionDigits() > 0);
        }
    }

    #[test]
    fn test_is_symbol_null() {
        unsafe {
            assert_eq!(isSymbol(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_is_environment_null() {
        unsafe {
            assert_eq!(isEnvironment(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_is_string_null() {
        unsafe {
            assert_eq!(isString(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_is_null_helper() {
        unsafe {
            assert_eq!(Rf_isNull(ptr::null_mut()), 1);
            assert_eq!(Rf_isNull(R_NilValue()), 1);
        }
    }

    #[test]
    fn test_print_data_init() {
        assert_eq!(R_PRINT_INIT.width, 80);
        assert_eq!(R_PRINT_INIT.gap, 1);
        assert_eq!(R_PRINT_INIT.digits, 4);
        assert_eq!(R_PRINT_INIT.scipen, 0);
        assert_eq!(R_PRINT_INIT.max, 99999);
        assert_eq!(R_PRINT_INIT.right, 0);
        assert_eq!(R_PRINT_INIT.quote, 1);
    }

    #[test]
    fn test_constants() {
        assert_eq!(Rprt_adj_left, 0);
        assert_eq!(Rprt_adj_right, 1);
        assert_eq!(TAGBUFLEN, 256);
        assert_eq!(TAGBUFLEN0, 262);
    }

    #[test]
    fn test_tagbuf_thread_local() {
        unsafe {
            tagbuf_clear();
            TAGBUF.with(|buf| {
                let mut b = buf.borrow_mut();
                let src = b"hello\0";
                b[..src.len()].copy_from_slice(src);
            });
            TAGBUF.with(|buf| {
                let b = buf.borrow();
                let s = CStr::from_ptr(b.as_ptr() as *const c_char)
                    .to_str()
                    .unwrap_or("");
                assert_eq!(s, "hello");
            });
            tagbuf_clear();
        }
    }

    #[test]
    fn test_print_defaults() {
        unsafe {
            PrintDefaults();
            let pd = get_R_print_data();
            assert!(pd.digits > 0);
            assert!(pd.width > 0);
        }
    }

    #[test]
    fn test_print_value_null() {
        unsafe {
            PrintValue(ptr::null_mut());
        }
    }

    #[test]
    fn test_print_value_rec_null() {
        unsafe {
            let mut data = R_PRINT_INIT.clone();
            PrintValueRec(
                ptr::null_mut(),
                &mut data as *mut R_PrintData as *mut std::ffi::c_void,
            );
        }
    }

    #[test]
    fn test_print_value_env_null() {
        unsafe {
            PrintValueEnv(ptr::null_mut(), R_GlobalEnv());
        }
    }

    #[test]
    fn test_r_pv_null() {
        unsafe {
            R_PV(ptr::null_mut());
        }
    }

    #[test]
    fn test_custom_print_value_null() {
        unsafe {
            CustomPrintValue(ptr::null_mut(), ptr::null_mut());
        }
    }

    #[test]
    fn test_is_valid_name() {
        unsafe {
            assert!(isValidName(b"foo\0".as_ptr() as *const c_char));
            assert!(isValidName(b".foo\0".as_ptr() as *const c_char));
            assert!(isValidName(b"foo_bar\0".as_ptr() as *const c_char));
            assert!(!isValidName(b"123\0".as_ptr() as *const c_char));
            assert!(!isValidName(b"\0".as_ptr() as *const c_char));
            assert!(!isValidName(ptr::null()));
        }
    }

    #[test]
    fn test_inherits_null() {
        unsafe {
            assert_eq!(
                inherits(ptr::null_mut(), ptr::null_mut(), ptr::null_mut()),
                0
            );
        }
    }

    #[test]
    fn test_save_restore_tagbuf() {
        unsafe {
            tagbuf_clear();
            TAGBUF.with(|buf| {
                let mut b = buf.borrow_mut();
                let src = b"test_prefix$\0";
                b[..src.len()].copy_from_slice(src);
            });
            let mut save = [0u8; TAGBUFLEN0 * 2];
            save_tagbuf(&mut save);

            tagbuf_clear();
            TAGBUF.with(|buf| {
                let mut b = buf.borrow_mut();
                let src = b"modified\0";
                b[..src.len()].copy_from_slice(src);
            });

            restore_tagbuf(&save);
            TAGBUF.with(|buf| {
                let b = buf.borrow();
                let s = CStr::from_ptr(b.as_ptr() as *const c_char)
                    .to_str()
                    .unwrap_or("");
                assert_eq!(s, "test_prefix$");
            });
            tagbuf_clear();
        }
    }

    #[test]
    fn test_do_prmatrix_nil() {
        unsafe {
            let result = do_prmatrix(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_do_unclass_null() {
        unsafe {
            let result = do_unclass(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert!(result.is_null());
        }
    }
}
