#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/builtin.c -- built-in functions and primitives.
//!
//! Implements:
//!   - asVecSize         -- convert SEXP to vector size (R_xlen_t)
//!   - do_delayed        -- delayedAssign()
//!   - do_makelazy       -- .Internal(makeLazy(...))
//!   - do_onexit         -- on.exit() (SPECIALSXP)
//!   - do_args           -- args()
//!   - do_formals        -- formals()
//!   - do_body           -- body()
//!   - do_bodyCode       -- bodyCode()
//!   - do_envir          -- environment()
//!   - do_envirgets      -- environment<- ()
//!   - do_newenv         -- .Internal(new.env(hash, parent, size))
//!   - do_parentenv      -- parent.env()
//!   - do_parentenvgets  -- parent.env<- ()
//!   - do_envirName      -- environmentName()
//!   - do_cat            -- cat()
//!   - do_makelist       -- list()
//!   - do_expression     -- expression()
//!   - do_makevector     -- vector()
//!   - xlengthgets       -- set length of a vector
//!   - lengthgets        -- older version of xlengthgets
//!   - do_lengthgets     -- length<- ()
//!   - do_switch         -- switch() (SPECIALSXP)

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;
use std::ptr::addr_of_mut;

use crate::main::coerce::vector::coerceVector;
use crate::main::duplicate::{duplicate, shallow_duplicate};
use crate::main::errors::*;
use crate::main::match_mod::pmatch;
use crate::main::printutils::Rprintf;
use crate::main::sysutils::translateChar;
use crate::main::util_main::str2type;
use crate::sexp::accessors::*;
use crate::sexp::attrib_core::{getAttrib, setAttrib};
use crate::sexp::constructors::*;
use crate::sexp::context::{R_GlobalContext, RCNTXT, ctxt_flags};
use crate::sexp::envir::{R_EnvironmentIsLocked, R_NewEnv, R_findVarInFrame, defineVar, findFun};
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::memory_ext::*;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;
use std::cell::Cell;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R_XLEN_T_MAX -- maximum value for R_xlen_t.
const R_XLEN_T_MAX: R_xlen_t = i64::MAX;

/// R_LEN_T_MAX -- maximum value for R_len_t.
const R_LEN_T_MAX: R_xlen_t = c_int::MAX as R_xlen_t;

/// Rboolean TRUE/FALSE constants.
const TRUE: c_int = 1;
const FALSE: c_int = 0;

/// Rprt_adj_left -- left adjustment constant for EncodeString.
const Rprt_adj_left: c_int = 0;

// ---------------------------------------------------------------------------
// Local stubs for macros / functions not yet fully ported
// ---------------------------------------------------------------------------

#[inline(always)]
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

#[inline(always)]
unsafe fn isString(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP.0 }
}

#[inline(always)]
unsafe fn isSymbol(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::SYMSXP.0 }
}

#[inline(always)]
unsafe fn isEnvironment(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::ENVSXP.0 }
}

#[inline(always)]
unsafe fn isVectorAtomic(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP.0
            || t == SEXPTYPE::INTSXP.0
            || t == SEXPTYPE::REALSXP.0
            || t == SEXPTYPE::CPLXSXP.0
            || t == SEXPTYPE::STRSXP.0
            || t == SEXPTYPE::RAWSXP.0
    }
}

#[inline(always)]
unsafe fn isVector(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP.0
            || t == SEXPTYPE::INTSXP.0
            || t == SEXPTYPE::REALSXP.0
            || t == SEXPTYPE::CPLXSXP.0
            || t == SEXPTYPE::STRSXP.0
            || t == SEXPTYPE::RAWSXP.0
            || t == SEXPTYPE::VECSXP.0
            || t == SEXPTYPE::EXPRSXP.0
    }
}

#[inline(always)]
unsafe fn isList(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::LISTSXP.0 }
}

#[inline(always)]
unsafe fn isObject(x: SEXP) -> bool {
    unsafe { !x.is_null() && (OBJECT(x) != 0) }
}

#[inline(always)]
unsafe fn isFactor(x: SEXP) -> bool {
    unsafe {
        if isNull(x) || TYPEOF(x) != SEXPTYPE::INTSXP.0 {
            return false;
        }
        let cls = getAttrib(x, R_ClassSymbol());
        !isNull(cls) && LENGTH(cls) > 0
    }
}

#[inline(always)]
unsafe fn isPrimitive(x: SEXP) -> bool {
    unsafe {
        !x.is_null() && (TYPEOF(x) == SEXPTYPE::BUILTINSXP.0 || TYPEOF(x) == SEXPTYPE::SPECIALSXP.0)
    }
}

#[inline(always)]
unsafe fn isNumeric(x: SEXP) -> bool {
    unsafe {
        if isNull(x) {
            return false;
        }
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::REALSXP.0 || t == SEXPTYPE::CPLXSXP.0
    }
}

#[inline(always)]
unsafe fn isLogical(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::LGLSXP.0 }
}

#[inline(always)]
unsafe fn checkArity(op: SEXP, args: SEXP) {
    crate::main::errors::Rf_checkArityCall(op, args, crate::main::errors::getCurrentCall());
}

#[inline(always)]
unsafe fn check1arg(args: SEXP, call: SEXP, formal: *const c_char) {
    unsafe {
        if isNull(args) || isNull(CDR(args)) {
            Rf_errorcall_fmt(call, c"'%s' is missing".as_ptr(), &[CStr::from_ptr(formal)]);
        }
    }
}

#[inline(always)]
unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        match TYPEOF(x) {
            10 => {
                // LGLSXP
                let p = LOGICAL(x);
                if p.is_null() { NA_LOGICAL } else { *p }
            }
            13 => {
                // INTSXP
                let p = INTEGER(x);
                if p.is_null() { NA_INTEGER } else { *p }
            }
            14 => {
                // REALSXP
                let p = REAL(x);
                if p.is_null() {
                    NA_INTEGER
                } else {
                    let v = *p;
                    if v.is_nan() || v > c_int::MAX as c_double || v < c_int::MIN as c_double {
                        NA_INTEGER
                    } else {
                        v as c_int
                    }
                }
            }
            _ => NA_INTEGER,
        }
    }
}

#[inline(always)]
unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_LOGICAL;
        }
        match TYPEOF(x) {
            10 => {
                let p = LOGICAL(x);
                if p.is_null() { NA_LOGICAL } else { *p }
            }
            13 => {
                let p = INTEGER(x);
                if p.is_null() { NA_LOGICAL } else { *p }
            }
            _ => 0,
        }
    }
}

#[inline(always)]
unsafe fn asReal(x: SEXP) -> c_double {
    unsafe {
        if x.is_null() {
            return NA_REAL;
        }
        match TYPEOF(x) {
            10 => {
                let p = LOGICAL(x);
                if p.is_null() {
                    NA_REAL
                } else if *p == NA_LOGICAL {
                    NA_REAL
                } else {
                    *p as c_double
                }
            }
            13 => {
                let p = INTEGER(x);
                if p.is_null() {
                    NA_REAL
                } else if *p == NA_INTEGER {
                    NA_REAL
                } else {
                    *p as c_double
                }
            }
            14 => {
                let p = REAL(x);
                if p.is_null() { NA_REAL } else { *p }
            }
            _ => NA_REAL,
        }
    }
}

#[inline(always)]
#[unsafe(no_mangle)]
unsafe fn ISNAN(x: c_double) -> bool {
    x.is_nan()
}

#[inline(always)]
#[unsafe(no_mangle)]
unsafe fn R_FINITE(x: c_double) -> bool {
    x.is_finite()
}

#[inline(always)]
unsafe fn installTrChar(input: SEXP) -> SEXP {
    unsafe {
        let s = CHAR(input);
        if s.is_null() {
            return ptr::null_mut();
        }
        let cstr = CStr::from_ptr(s);
        let bytes = cstr.to_bytes();
        let mut buf: Vec<u8> = vec![0u8; bytes.len() + 1];
        buf[..bytes.len()].copy_from_slice(bytes);
        Rf_install(buf.as_ptr() as *const c_char)
    }
}

#[inline(always)]
unsafe fn install(s: &str) -> SEXP {
    unsafe {
        let c_buf = std::ffi::CString::new(s).unwrap_or_default();
        Rf_install(c_buf.as_ptr())
    }
}

#[inline(always)]
unsafe fn length(x: SEXP) -> c_int {
    unsafe { LENGTH(x) }
}

#[inline(always)]
unsafe fn xlength(x: SEXP) -> R_xlen_t {
    unsafe { XLENGTH(x) }
}

#[inline(always)]
unsafe fn eval(e: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::eval::eval::Rf_eval(e, rho) }
}

/// Memzero: zero-fill memory.
#[inline(always)]
unsafe fn Memzero<T>(dest: *mut T, n: R_xlen_t) {
    unsafe {
        ptr::write_bytes(dest as *mut u8, 0, n as usize * std::mem::size_of::<T>());
    }
}

/// IS_S4_OBJECT: check if an S4 object.
#[inline(always)]
unsafe fn IS_S4_OBJECT(x: SEXP) -> bool {
    unsafe { !x.is_null() && (LEVELS(x) & 0x40000) != 0 }
}

/// R_getS4DataSlot: stub for S4 data slot extraction.
#[inline(always)]
unsafe fn R_getS4DataSlot(_x: SEXP, _type: c_int) -> SEXP {
    ptr::null_mut() // stub
}

/// simple_as_environment: get environment from a subclass if possible; else return NULL.
#[inline(always)]
unsafe fn simple_as_environment(arg: SEXP) -> SEXP {
    unsafe {
        if IS_S4_OBJECT(arg) && TYPEOF(arg) == SEXPTYPE::OBJSXP.0 {
            R_getS4DataSlot(arg, SEXPTYPE::ENVSXP.0)
        } else {
            arg
        }
    }
}

/// R_BaseNamespace -- stub (returns a placeholder).
#[inline(always)]
unsafe fn R_BaseNamespace() -> SEXP {
    ptr::null_mut() // stub
}

/// R_IsNamespaceEnv -- stub.
#[inline(always)]
unsafe fn R_IsNamespaceEnv(_env: SEXP) -> bool {
    false // stub
}

/// R_IsImportsEnv -- check if env is an imports: namespace.
#[inline(always)]
unsafe fn R_IsImportsEnv(env: SEXP) -> bool {
    unsafe {
        if isNull(env) || !isEnvironment(env) {
            return false;
        }
        if ENCLOS(env) != R_BaseNamespace() {
            return false;
        }
        let name = getAttrib(env, R_NameSymbol());
        if !isString(name) || LENGTH(name) != 1 {
            return false;
        }
        let name_string = CHAR(STRING_ELT(name, 0));
        if name_string.is_null() {
            return false;
        }
        let imports_prefix = "imports:";
        let cstr = CStr::from_ptr(name_string);
        let bytes = cstr.to_bytes();
        bytes.starts_with(imports_prefix.as_bytes())
    }
}

/// R_DotEnvSymbol -- the ".Environment" symbol.
#[inline(always)]
unsafe fn R_DotEnvSymbol() -> SEXP {
    unsafe { install(".Environment") }
}

/// R_NameSymbol -- the "name" symbol.
#[inline(always)]
unsafe fn R_NameSymbol() -> SEXP {
    unsafe { install("name") }
}

/// R_NamesSymbol -- the "names" symbol.
#[inline(always)]
unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { install("names") }
}

/// R_ClassSymbol -- the "class" symbol.
#[inline(always)]
unsafe fn R_ClassSymbol() -> SEXP {
    unsafe { install("class") }
}

/// R_BlankString -- the blank string CHARSXP.
#[inline(always)]
unsafe fn R_BlankString() -> SEXP {
    unsafe { Rf_mkChar(c"".as_ptr()) }
}

/// NA_STRING -- the NA string CHARSXP.
#[inline(always)]
unsafe fn NA_STRING() -> SEXP {
    unsafe { Rf_mkChar(c"NA".as_ptr()) }
}

/// mkString -- create a single-element character vector.
#[inline(always)]
unsafe fn mkString(s: &str) -> SEXP {
    unsafe {
        let c_buf = std::ffi::CString::new(s).unwrap_or_default();
        Rf_mkString(c_buf.as_ptr())
    }
}

/// ScalarString -- create a scalar string (single CHARSXP wrapped in STRSXP).
#[inline(always)]
unsafe fn ScalarString(x: SEXP) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::STRSXP.0, 1);
        if !s.is_null() {
            SET_STRING_ELT(s, 0, x);
        }
        s
    }
}

/// listAppend -- append list s to list.
#[inline(always)]
unsafe fn listAppend(list: SEXP, s: SEXP) -> SEXP {
    unsafe {
        let mut tail = list;
        while !CDR(tail).is_null() && CDR(tail) != R_NilValue() {
            tail = CDR(tail);
        }
        SETCDR(tail, s);
        list
    }
}

/// allocFormalsList3 -- create a formals list from 3 symbols.
#[inline(always)]
unsafe fn allocFormalsList3(a: SEXP, b: SEXP, c: SEXP) -> SEXP {
    unsafe {
        let c3 = CONS_NR(c, R_NilValue());
        SETTAG(c3, c);
        let c2 = CONS_NR(b, c3);
        SETTAG(c2, b);
        let c1 = CONS_NR(a, c2);
        SETTAG(c1, a);
        c1
    }
}

/// matchArgs_NR -- match arguments (no reorder).
#[inline(always)]
unsafe fn matchArgs_NR(_formals: SEXP, args: SEXP, _call: SEXP) -> SEXP {
    args // stub: return args as-is
}

/// deparse1line -- deparse to a single line.
#[inline(always)]
unsafe fn deparse1line(_x: SEXP, _addwrap: bool) -> SEXP {
    unsafe { mkString("") }
}

/// CONS -- create a cons cell.
#[inline(always)]
unsafe fn CONS(car: SEXP, cdr: SEXP) -> SEXP {
    unsafe {
        let cell = Rf_allocVector(SEXPTYPE::LISTSXP.0, 2);
        if !cell.is_null() {
            SETCAR(cell, car);
            SETCDR(cell, cdr);
        }
        cell
    }
}

/// RAISE_NAMED -- increase the NAMED level of x to at least v.
#[inline(always)]
unsafe fn RAISE_NAMED(_x: SEXP, _v: c_int) {
    // Stub -- no-op for now
}

/// ENSURE_NAMEDMAX -- ensure NAMED is at max.
#[inline(always)]
unsafe fn ENSURE_NAMEDMAX(_x: SEXP) {
    // Stub -- no-op for now
}

/// MAYBE_REFERENCED -- check if x might be referenced.
#[inline(always)]
unsafe fn MAYBE_REFERENCED(_x: SEXP) -> bool {
    false // stub
}

/// MAYBE_SHARED -- check if x might be shared.
#[inline(always)]
unsafe fn MAYBE_SHARED(_x: SEXP) -> bool {
    false // stub
}

/// IS_ASSIGNMENT_CALL -- check if the call is an assignment.
#[inline(always)]
unsafe fn IS_ASSIGNMENT_CALL(_call: SEXP) -> bool {
    false // stub
}

/// BODY_EXPR -- get body expression (handling bytecode).
#[inline(always)]
unsafe fn BODY_EXPR(x: SEXP) -> SEXP {
    unsafe {
        BODY(x) // stub: doesn't handle BCODESXP
    }
}

/// R_ClosureExpr -- get the expression of a closure (handling bytecode).
#[inline(always)]
unsafe fn R_ClosureExpr(_x: SEXP) -> SEXP {
    ptr::null_mut() // stub
}

#[unsafe(no_mangle)]
/// PrintDefaults -- set default printing options.
#[inline(always)]
unsafe fn PrintDefaults() {
    // Stub -- no-op
}

#[unsafe(no_mangle)]
/// Rstrlen -- get the display length of a string element.
#[inline(always)]
unsafe fn Rstrlen(x: SEXP, _quote: c_int) -> usize {
    unsafe {
        let p = CHAR(x);
        if p.is_null() {
            0
        } else {
            CStr::from_ptr(p).to_bytes().len()
        }
    }
}

/// EncodeString -- encode a string for display.

#[inline(always)]
unsafe fn EncodeString(x: SEXP, _w: c_int, _quote: c_int, _adj: c_int) -> *const c_char {
    unsafe {
        let p = CHAR(x);
        if p.is_null() { c"".as_ptr() } else { p }
    }
}

/// EncodeElement0 -- encode a vector element for display.
#[unsafe(no_mangle)]
#[inline(always)]
unsafe fn EncodeElement0(
    _x: SEXP,
    _indx: c_int,
    _quote: c_int,
    _w: *const c_char,
) -> *const c_char {
    c"".as_ptr() // stub
}

/// OutDec -- the decimal separator character.
#[inline(always)]
unsafe fn OutDec() -> *const c_char {
    c".".as_ptr()
}

/// PRIMNAME -- get the name of a primitive function.
#[inline(always)]
unsafe fn PRIMNAME(_op: SEXP) -> *const c_char {
    c"<primitive>".as_ptr() // stub
}

/// install_from_cstr -- install a symbol from a C string pointer.
#[inline(always)]
unsafe fn install_from_cstr(s: *const c_char) -> SEXP {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }
        Rf_install(s)
    }
}

/// R_ToplevelContext -- get the top-level context.
#[inline(always)]
unsafe fn R_ToplevelContext() -> *mut RCNTXT {
    ptr::null_mut() // stub: return null (bottom of context stack)
}

/// nthcdr -- get the n-th cdr of a list.
#[inline(always)]
#[unsafe(no_mangle)]
unsafe fn nthcdr(x: SEXP, n: c_int) -> SEXP {
    unsafe {
        let mut r = x;
        for _ in 0..n {
            if r.is_null() || r == R_NilValue() {
                return ptr::null_mut();
            }
            r = CDR(r);
        }
        r
    }
}

/// R_typeToChar -- get the type name string.
#[inline(always)]
unsafe fn R_typeToChar(_s: SEXP) -> *const c_char {
    c"unknown".as_ptr() // stub
}

/// UNIMPLEMENTED_TYPE -- signal an unimplemented type error.
#[inline(always)]
unsafe fn UNIMPLEMENTED_TYPE(_routine: *const c_char, _s: SEXP) {
    Rf_error(c"unimplemented type".as_ptr());
}

/// DispatchOrEval -- dispatch or evaluate an internal generic.
#[inline(always)]
unsafe fn DispatchOrEval(
    _call: SEXP,
    _op: SEXP,
    _generic: *const c_char,
    _args: SEXP,
    _rho: SEXP,
    _ans: *mut SEXP,
    _dispatch: c_int,
    _eval: c_int,
) -> c_int {
    0 // stub: no dispatch
}

/// R_IsPackageEnv -- check if environment is a package environment.
#[inline(always)]
unsafe fn R_IsPackageEnv(_rho: SEXP) -> bool {
    false // stub
}

/// R_PackageEnvName -- get package environment name.
#[inline(always)]
unsafe fn R_PackageEnvName(_rho: SEXP) -> SEXP {
    unsafe {
        R_NilValue() // stub
    }
}

/// R_NamespaceEnvSpec -- get namespace spec.
#[inline(always)]
unsafe fn R_NamespaceEnvSpec(_rho: SEXP) -> SEXP {
    unsafe {
        R_NilValue() // stub
    }
}

/// R_findVar -- find a variable in an environment.
#[inline(always)]
unsafe fn R_findVar(symbol: SEXP, rho: SEXP) -> SEXP {
    unsafe { R_findVarInFrame(rho, symbol) }
}

/// getConnection -- get a connection by index.
#[inline(always)]
#[unsafe(no_mangle)]
unsafe fn getConnection(_ifile: c_int) -> *mut c_void {
    ptr::null_mut() // stub
}

/// switch_stdout -- redirect stdout to a connection.
#[inline(always)]
unsafe fn switch_stdout(_ifile: c_int, _flush: c_int) -> c_int {
    0 // stub: no-op
}

// ---------------------------------------------------------------------------
// cat_info struct and helpers
// ---------------------------------------------------------------------------

/// Connection information for cat() cleanup.
#[repr(C)]
struct cat_info {
    wasopen: bool,
    changedcon: c_int,
    con: *mut c_void,
}

/// trChar -- translate a CHARSXP for cat() output.
unsafe fn trChar(x: SEXP) -> *const c_char {
    unsafe { translateChar(x) }
}

/// cat_newline -- print a newline and optional label.
unsafe fn cat_newline(labels: SEXP, width: *mut usize, lablen: c_int, ntot: c_int) {
    unsafe {
        Rprintf(c"\n".as_ptr(), ptr::null_mut());
        *width = 0;
        if !isNull(labels) {
            let lab = STRING_ELT(labels, (ntot % lablen) as R_xlen_t);
            let lab_char = CHAR(lab);
            if !lab_char.is_null() {
                Rprintf(c"%s ".as_ptr(), ptr::null_mut());
                *width += CStr::from_ptr(lab_char).to_bytes().len() + 1;
            }
        }
    }
}

/// cat_sepwidth -- get separator width.
unsafe fn cat_sepwidth(sep: SEXP, width: *mut c_int, ntot: c_int) {
    unsafe {
        if isNull(sep) || LENGTH(sep) == 0 {
            *width = 0;
        } else {
            let s = STRING_ELT(sep, (ntot % LENGTH(sep)) as R_xlen_t);
            let p = CHAR(s);
            if p.is_null() {
                *width = 0;
            } else {
                *width = CStr::from_ptr(p).to_bytes().len() as c_int;
            }
        }
    }
}

/// cat_printsep -- print separator.
unsafe fn cat_printsep(sep: SEXP, ntot: c_int) {
    unsafe {
        if isNull(sep) || LENGTH(sep) == 0 {
            return;
        }
        let _sepchar = trChar(STRING_ELT(sep, (ntot % LENGTH(sep)) as R_xlen_t));
        Rprintf(c"%s".as_ptr(), ptr::null_mut());
    }
}

// ===========================================================================
// Public API
// ===========================================================================

// ---------------------------------------------------------------------------
// asVecSize
// ---------------------------------------------------------------------------

/// Convert a scalar SEXP to a vector size (R_xlen_t).
/// Returns -999 on error (caller should check).
pub unsafe fn asVecSize(x: SEXP) -> R_xlen_t {
    unsafe {
        if isVectorAtomic(x) && LENGTH(x) >= 1 {
            let t = TYPEOF(x);
            if t == SEXPTYPE::INTSXP.0 {
                let res = *INTEGER(x).add(0);
                if res == NA_INTEGER {
                    Rf_error(c"vector size cannot be NA".as_ptr());
                }
                return res as R_xlen_t;
            } else if t == SEXPTYPE::REALSXP.0 {
                let d = *REAL(x).add(0);
                if ISNAN(d) {
                    Rf_error(c"vector size cannot be NA/NaN".as_ptr());
                }
                if !R_FINITE(d) {
                    Rf_error(c"vector size cannot be infinite".as_ptr());
                }
                if d > R_XLEN_T_MAX as c_double {
                    Rf_error(c"vector size specified is too large".as_ptr());
                }
                return d as R_xlen_t;
            } else if t == SEXPTYPE::STRSXP.0 {
                let d = asReal(x);
                if ISNAN(d) {
                    Rf_error(c"vector size cannot be NA/NaN".as_ptr());
                }
                if !R_FINITE(d) {
                    Rf_error(c"vector size cannot be infinite".as_ptr());
                }
                if d > R_XLEN_T_MAX as c_double {
                    Rf_error(c"vector size specified is too large".as_ptr());
                }
                return d as R_xlen_t;
            }
        }
        -999 // which gives error in the caller
    }
}

// ---------------------------------------------------------------------------
// do_delayed -- delayedAssign()
// ---------------------------------------------------------------------------

pub unsafe fn do_delayed(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut name = R_NilValue();
        let expr: SEXP;
        let eenv: SEXP;
        let aenv: SEXP;

        checkArity(op, args);

        if !isString(CAR(args)) || LENGTH(CAR(args)) == 0 {
            Rf_error(c"invalid first argument".as_ptr());
        } else {
            name = installTrChar(STRING_ELT(CAR(args), 0));
        }
        let mut args_rest = CDR(args);
        expr = CAR(args_rest);

        args_rest = CDR(args_rest);
        eenv = CAR(args_rest);
        if isNull(eenv) {
            Rf_error(c"use of NULL environment is defunct".as_ptr());
        } else if !isEnvironment(eenv) {
            Rf_error(c"invalid '%s' argument".as_ptr());
        }

        args_rest = CDR(args_rest);
        aenv = CAR(args_rest);
        if isNull(aenv) {
            Rf_error(c"use of NULL environment is defunct".as_ptr());
        } else if !isEnvironment(aenv) {
            Rf_error(c"invalid '%s' argument".as_ptr());
        }

        defineVar(name, mkPROMISE(expr, eenv), aenv);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_makelazy -- .Internal(makeLazy(...))
// ---------------------------------------------------------------------------

pub unsafe fn do_makelazy(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let names: SEXP;
        let values: SEXP;
        let val: SEXP;
        let expr: SEXP;
        let eenv: SEXP;
        let aenv: SEXP;
        let expr0: SEXP;

        checkArity(op, args);
        names = CAR(args);
        let mut args_rest = CDR(args);
        if !isString(names) {
            Rf_error(c"invalid first argument".as_ptr());
        }
        values = CAR(args_rest);
        args_rest = CDR(args_rest);
        expr = CAR(args_rest);
        args_rest = CDR(args_rest);
        eenv = CAR(args_rest);
        args_rest = CDR(args_rest);
        if !isEnvironment(eenv) {
            Rf_error(c"invalid '%s' argument".as_ptr());
        }
        aenv = CAR(args_rest);
        if !isEnvironment(aenv) {
            Rf_error(c"invalid '%s' argument".as_ptr());
        }

        let n = XLENGTH(names);
        for i in 0..n {
            let name = installTrChar(STRING_ELT(names, i));
            let val = Rf_protect(eval(VECTOR_ELT(values, i), eenv));
            let expr0 = Rf_protect(duplicate(expr));
            SETCAR(CDR(expr0), val);
            defineVar(name, mkPROMISE(expr0, eenv), aenv);
            Rf_unprotect(2);
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_onexit -- on.exit() (SPECIALSXP)
// ---------------------------------------------------------------------------

thread_local! { static do_onexit_formals: Cell<SEXP> = Cell::new(ptr::null_mut()); }

pub unsafe fn do_onexit(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let code: SEXP;
        let mut addit: c_int = FALSE;
        let mut after: c_int = TRUE;

        checkArity(op, args);
        if do_onexit_formals.with(|v| v.get()).is_null() {
            do_onexit_formals.with(|v| {
                v.set(allocFormalsList3(
                    install("expr"),
                    install("add"),
                    install("after"),
                ))
            });
        }

        let argList = matchArgs_NR(do_onexit_formals.with(|v| v.get()), args, call);
        Rf_protect(argList);
        if CAR(argList) == R_MissingArg() {
            code = R_NilValue();
        } else {
            code = CAR(argList);
        }

        if CADR(argList) != R_MissingArg() {
            let addit_val = Rf_protect(eval(CADR(argList), rho));
            addit = asLogical(addit_val);
            Rf_unprotect(1);
            if addit == NA_INTEGER {
                Rf_errorcall1(call, c"invalid '%s' argument".as_ptr(), c"add".as_ptr());
            }
        }
        if CADDR(argList) != R_MissingArg() {
            let after_val = Rf_protect(eval(CADDR(argList), rho));
            after = asLogical(after_val);
            Rf_unprotect(1);
            if after == NA_INTEGER {
                Rf_errorcall1(call, c"invalid '%s' argument".as_ptr(), c"lifo".as_ptr());
            }
        }

        let mut ctxt = R_GlobalContext();
        // Search for the context to which the on.exit action is to be attached.
        while !ctxt.is_null()
            && ctxt != R_ToplevelContext()
            && !((*ctxt).callflag & ctxt_flags::CTXT_FUNCTION != 0 && (*ctxt).cloenv == rho)
        {
            ctxt = (*ctxt).nextcontext;
        }
        if !ctxt.is_null() && ((*ctxt).callflag & ctxt_flags::CTXT_FUNCTION != 0) {
            if code == R_NilValue() && addit == FALSE {
                (*ctxt).conexit = R_NilValue();
            } else {
                let oldcode = (*ctxt).conexit;
                if oldcode == R_NilValue() || addit == FALSE {
                    (*ctxt).conexit = CONS(code, R_NilValue());
                } else {
                    if after != 0 {
                        let codelist = Rf_protect(CONS(code, R_NilValue()));
                        (*ctxt).conexit = listAppend(shallow_duplicate(oldcode), codelist);
                        Rf_unprotect(1);
                    } else {
                        (*ctxt).conexit = CONS(code, oldcode);
                    }
                }
            }
        }
        Rf_unprotect(1);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_args -- args()
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_args(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let s: SEXP;

        checkArity(op, args);
        if TYPEOF(CAR(args)) == SEXPTYPE::STRSXP.0 && LENGTH(CAR(args)) == 1 {
            let s_installed = Rf_protect(installTrChar(STRING_ELT(CAR(args), 0)));
            SETCAR(args, findFun(s_installed, rho));
            Rf_unprotect(1);
        }

        if TYPEOF(CAR(args)) == SEXPTYPE::CLOSXP.0 {
            s = allocSExp(SEXPTYPE::CLOSXP);
            SET_FORMALS(s, FORMALS(CAR(args)));
            SET_BODY(s, R_NilValue());
            SET_CLOENV(s, R_GlobalEnv());
            return s;
        }

        if TYPEOF(CAR(args)) == SEXPTYPE::BUILTINSXP.0
            || TYPEOF(CAR(args)) == SEXPTYPE::SPECIALSXP.0
        {
            let nm = PRIMNAME(CAR(args));
            let env_sym = install(".ArgsEnv");

            let mut env = R_findVarInFrame(R_BaseEnv(), env_sym);
            Rf_protect(env);
            // If it's a promise, evaluate it
            if TYPEOF(env) == SEXPTYPE::PROMSXP.0 {
                env = eval(env, R_BaseEnv());
            }
            let s2 = R_findVarInFrame(env, install_from_cstr(nm));
            Rf_protect(s2);
            if s2 != R_UnboundValue() {
                s = duplicate(s2);
                SET_BODY(s, R_NilValue());
                SET_CLOENV(s, R_GlobalEnv());
                Rf_unprotect(2);
                return s;
            }
            Rf_unprotect(2);

            // Try .GenericArgsEnv
            let generic_env_sym = install(".GenericArgsEnv");
            env = R_findVarInFrame(R_BaseEnv(), generic_env_sym);
            Rf_protect(env);
            if TYPEOF(env) == SEXPTYPE::PROMSXP.0 {
                env = eval(env, R_BaseEnv());
            }
            let s3 = R_findVarInFrame(env, install_from_cstr(nm));
            Rf_protect(s3);
            if s3 != R_UnboundValue() {
                s = allocSExp(SEXPTYPE::CLOSXP);
                SET_FORMALS(s, FORMALS(s3));
                SET_BODY(s, R_NilValue());
                SET_CLOENV(s, R_GlobalEnv());
                Rf_unprotect(2);
                return s;
            }
            Rf_unprotect(2);
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_formals -- formals()
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_formals(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) == SEXPTYPE::CLOSXP.0 {
            let f = FORMALS(CAR(args));
            RAISE_NAMED(f, NAMED(CAR(args)));
            return f;
        } else {
            if !(TYPEOF(CAR(args)) == SEXPTYPE::BUILTINSXP.0
                || TYPEOF(CAR(args)) == SEXPTYPE::SPECIALSXP.0)
            {
                warningcall(call, c"argument is not a function".as_ptr());
            }
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// do_body -- body()
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_body(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) == SEXPTYPE::CLOSXP.0 {
            let b = BODY_EXPR(CAR(args));
            RAISE_NAMED(b, NAMED(CAR(args)));
            return b;
        } else {
            if !(TYPEOF(CAR(args)) == SEXPTYPE::BUILTINSXP.0
                || TYPEOF(CAR(args)) == SEXPTYPE::SPECIALSXP.0)
            {
                warningcall(call, c"argument is not a function".as_ptr());
            }
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// do_bodyCode -- bodyCode()
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_bodyCode(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) == SEXPTYPE::CLOSXP.0 {
            let bc = BODY(CAR(args));
            RAISE_NAMED(bc, NAMED(CAR(args)));
            return bc;
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// do_envir -- environment()
// ---------------------------------------------------------------------------

pub unsafe fn do_envir(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) == SEXPTYPE::CLOSXP.0 {
            CLOENV(CAR(args))
        } else if CAR(args) == R_NilValue() {
            let ctx = R_GlobalContext();
            if ctx.is_null() {
                R_NilValue()
            } else {
                // sysparent is actually an SEXP in the full impl; here use a stub
                R_GlobalEnv()
            }
        } else {
            getAttrib(CAR(args), R_DotEnvSymbol())
        }
    }
}

// ---------------------------------------------------------------------------
// do_envirgets -- environment<- ()
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_envirgets(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let mut s = CAR(args);
        let mut env = CADR(args);

        if TYPEOF(s) == SEXPTYPE::CLOSXP.0
            && (isEnvironment(env)
                || {
                    env = simple_as_environment(env);
                    isEnvironment(env)
                }
                || isNull(env))
        {
            if isNull(env) {
                Rf_error(c"use of NULL environment is defunct".as_ptr());
            }
            if MAYBE_SHARED(s) || ((!IS_ASSIGNMENT_CALL(call)) && MAYBE_REFERENCED(s)) {
                s = duplicate(s);
            }
            if TYPEOF(BODY(s)) == SEXPTYPE::BCODESXP.0 {
                SET_BODY(s, R_ClosureExpr(s));
            }
            SET_CLOENV(s, env);
        } else if isNull(env) || isEnvironment(env) || {
            env = simple_as_environment(env);
            isEnvironment(env)
        } {
            if !isNull(env) && isPrimitive(s) {
                warningcall(call, c"setting environment(<primitive function>) is not possible and trying it is deprecated".as_ptr());
            } else {
                setAttrib(s, R_DotEnvSymbol(), env);
            }
        } else {
            Rf_error(c"replacement object is not an environment".as_ptr());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// do_newenv -- .Internal(new.env(hash, parent, size))
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_newenv(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut enclos: SEXP;
        let hash: c_int;
        let mut size: c_int = 0;

        checkArity(op, args);

        hash = asInteger(CAR(args));
        let args_rest = CDR(args);
        enclos = CAR(args_rest);
        if isNull(enclos) {
            Rf_error(c"use of NULL environment is defunct".as_ptr());
        }

        if !isEnvironment(enclos) && {
            enclos = simple_as_environment(enclos);
            !isEnvironment(enclos)
        } {
            Rf_error(c"'enclos' must be an environment".as_ptr());
        }

        if hash != 0 {
            size = asInteger(CADR(args_rest));
            if size == NA_INTEGER {
                size = 0; // use internal default
            }
        } else {
            size = 0;
        }
        R_NewEnv(enclos, hash, size)
    }
}

// ---------------------------------------------------------------------------
// do_parentenv -- parent.env()
// ---------------------------------------------------------------------------

pub unsafe fn do_parentenv(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let mut arg = CAR(args);

        if !isEnvironment(arg) && {
            arg = simple_as_environment(arg);
            !isEnvironment(arg)
        } {
            Rf_error(c"argument is not an environment".as_ptr());
        }
        if arg == R_EmptyEnv() {
            Rf_error(c"the empty environment has no parent".as_ptr());
        }
        ENCLOS(arg)
    }
}

// ---------------------------------------------------------------------------
// do_parentenvgets -- parent.env<- ()
// ---------------------------------------------------------------------------

pub unsafe fn do_parentenvgets(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let mut env = CAR(args);
        if isNull(env) {
            Rf_error(c"use of NULL environment is defunct".as_ptr());
        } else if !isEnvironment(env) && {
            env = simple_as_environment(env);
            !isEnvironment(env)
        } {
            Rf_error(c"argument is not an environment".as_ptr());
        }
        if env == R_EmptyEnv() {
            Rf_error(c"can not set parent of the empty environment".as_ptr());
        }
        if R_EnvironmentIsLocked(env) != 0 && R_IsNamespaceEnv(env) {
            Rf_error(c"can not set the parent environment of a namespace".as_ptr());
        }
        if R_EnvironmentIsLocked(env) != 0 && R_IsImportsEnv(env) {
            Rf_error(c"can not set the parent environment of package imports".as_ptr());
        }
        let mut parent = CADR(args);
        if isNull(parent) {
            Rf_error(c"use of NULL environment is defunct".as_ptr());
        } else if !isEnvironment(parent) && {
            parent = simple_as_environment(parent);
            !isEnvironment(parent)
        } {
            Rf_error(c"'parent' is not an environment".as_ptr());
        }

        SET_ENCLOS(env, parent);

        CAR(args)
    }
}

// ---------------------------------------------------------------------------
// do_envirName -- environmentName()
// ---------------------------------------------------------------------------

pub unsafe fn do_envirName(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut env = CAR(args);
        let mut ans = Rf_protect(mkString(""));

        checkArity(op, args);
        if TYPEOF(env) == SEXPTYPE::ENVSXP.0 || {
            env = simple_as_environment(env);
            TYPEOF(env) == SEXPTYPE::ENVSXP.0
        } {
            if env == R_GlobalEnv() {
                ans = mkString("R_GlobalEnv");
            } else if env == R_BaseEnv() {
                ans = mkString("base");
            } else if env == R_EmptyEnv() {
                ans = mkString("R_EmptyEnv");
            } else if R_IsPackageEnv(env) {
                ans = ScalarString(STRING_ELT(R_PackageEnvName(env), 0));
            } else if R_IsNamespaceEnv(env) {
                ans = ScalarString(STRING_ELT(R_NamespaceEnvSpec(env), 0));
            } else {
                let res = getAttrib(env, R_NameSymbol());
                if !isNull(res) {
                    ans = res;
                }
            }
        }
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_cat -- cat()
// ---------------------------------------------------------------------------

pub unsafe fn do_cat(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ci: cat_info = std::mem::zeroed();
        let objs: SEXP;
        let file: SEXP;
        let fill: SEXP;
        let sepr: SEXP;
        let labs: SEXP;
        let mut s: SEXP;
        let ifile: c_int;
        let append: c_int;
        let mut i: c_int;
        let mut iobj: c_int;
        let mut n: c_int;
        let nobjs: c_int;
        let mut sepw: c_int = 0;
        let lablen: c_int;
        let mut ntot: c_int = 0;
        let mut nlsep: c_int = 0;
        let mut nlines: c_int = 0;
        let mut width: usize = 0;
        let pwidth: usize;
        let mut buf: [u8; 512] = [0; 512];
        let mut p: *const c_char = c"".as_ptr();

        checkArity(op, args);

        // Use standard printing defaults
        PrintDefaults();

        objs = CAR(args);
        let mut args_rest = CDR(args);

        file = CAR(args_rest);
        ifile = asInteger(file);
        args_rest = CDR(args_rest);

        sepr = CAR(args_rest);
        if !isString(sepr) {
            Rf_error(c"invalid '%s' specification".as_ptr());
        }
        nlsep = 0;
        let sep_len = LENGTH(sepr);
        for i in 0..sep_len {
            let sep_elt = STRING_ELT(sepr, i as R_xlen_t);
            let sep_str = CHAR(sep_elt);
            if !sep_str.is_null() {
                let cstr = CStr::from_ptr(sep_str);
                let bytes = cstr.to_bytes();
                // Check for '\n' in the string (ASCII)
                for &b in bytes {
                    if b == b'\n' {
                        nlsep = 1;
                        break;
                    }
                }
            }
            if nlsep != 0 {
                break;
            }
        }
        args_rest = CDR(args_rest);

        fill = CAR(args_rest);
        if (!isNumeric(fill) && !isLogical(fill)) || LENGTH(fill) != 1 {
            Rf_error(c"invalid '%s' argument".as_ptr());
        }
        if isLogical(fill) {
            if asLogical(fill) == 1 {
                pwidth = 80; // R_print.width default
            } else {
                pwidth = usize::MAX;
            }
        } else {
            let ipwidth = asInteger(fill);
            if ipwidth <= 0 {
                warningcall(
                    call,
                    c"non-positive 'fill' argument will be ignored".as_ptr(),
                );
                pwidth = usize::MAX;
            } else {
                pwidth = ipwidth as usize;
            }
        }
        args_rest = CDR(args_rest);

        labs = CAR(args_rest);
        if !isString(labs) && labs != R_NilValue() {
            Rf_error(c"invalid '%s' argument".as_ptr());
        }
        lablen = length(labs);
        args_rest = CDR(args_rest);

        append = asLogical(CAR(args_rest));
        if append == NA_LOGICAL {
            Rf_error(c"invalid '%s' specification".as_ptr());
        }

        ci.wasopen = false; // stub
        ci.changedcon = 0; // stub
        ci.con = ptr::null_mut();

        nobjs = length(objs);
        width = 0;
        ntot = 0;
        nlines = 0;

        for iobj in 0..nobjs {
            s = VECTOR_ELT(objs, iobj as R_xlen_t);
            if iobj != 0 && !isNull(s) {
                cat_printsep(sepr, ntot);
                ntot += 1;
            }
            n = length(s);
            // 0-length objects are ignored
            if n > 0 {
                if !isNull(labs) && (iobj == 0) && (asInteger(fill) > 0) {
                    // Print label
                    let lab = STRING_ELT(labs, (nlines % lablen) as R_xlen_t);
                    let lab_char = CHAR(lab);
                    if !lab_char.is_null() {
                        Rprintf(c"%s ".as_ptr(), ptr::null_mut());
                        width += CStr::from_ptr(lab_char).to_bytes().len() + 1;
                    }
                    nlines += 1;
                }
                if isString(s) {
                    p = trChar(STRING_ELT(s, 0));
                } else if isSymbol(s) {
                    p = CHAR(PRINTNAME(s));
                } else if isVectorAtomic(s) {
                    p = EncodeElement0(s, 0, 0, OutDec());
                    let p_cstr = CStr::from_ptr(p);
                    let p_bytes = p_cstr.to_bytes();
                    let copy_len = std::cmp::min(p_bytes.len(), 511);
                    buf[..copy_len].copy_from_slice(&p_bytes[..copy_len]);
                    buf[copy_len] = 0;
                    p = buf.as_ptr() as *const c_char;
                } else {
                    Rf_error(c"argument cannot be handled by 'cat'".as_ptr());
                }

                let w = if p.is_null() {
                    0
                } else {
                    CStr::from_ptr(p).to_bytes().len()
                };
                cat_sepwidth(sepr, &mut sepw, ntot);
                if (iobj > 0) && (width + w + sepw as usize > pwidth) {
                    cat_newline(labs, &mut width, lablen, nlines);
                    nlines += 1;
                }
                for i in 0..n {
                    Rprintf(c"%s".as_ptr(), ptr::null_mut());
                    width += w + sepw as usize;
                    if i < (n - 1) {
                        cat_printsep(sepr, ntot);
                        if isString(s) {
                            p = trChar(STRING_ELT(s, (i + 1) as R_xlen_t));
                        } else {
                            p = EncodeElement0(s, i + 1, 0, OutDec());
                            let p_cstr = CStr::from_ptr(p);
                            let p_bytes = p_cstr.to_bytes();
                            let copy_len = std::cmp::min(p_bytes.len(), 511);
                            buf[..copy_len].copy_from_slice(&p_bytes[..copy_len]);
                            buf[copy_len] = 0;
                            p = buf.as_ptr() as *const c_char;
                        }
                        let new_w = if p.is_null() {
                            0
                        } else {
                            CStr::from_ptr(p).to_bytes().len()
                        };
                        cat_sepwidth(sepr, &mut sepw, ntot);
                        if width + new_w + sepw as usize > pwidth {
                            cat_newline(labs, &mut width, lablen, nlines);
                            nlines += 1;
                        }
                    } else {
                        ntot -= 1; // don't advance after last element
                    }
                    ntot += 1;
                }
            }
        }

        if (pwidth != usize::MAX) || nlsep != 0 {
            Rprintf(c"\n".as_ptr(), ptr::null_mut());
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_makelist -- list()
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_makelist(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut n: c_int = 0;
        let mut havenames: c_int = FALSE;

        // compute number of args and check for names
        let mut next = args;
        while !next.is_null() && next != R_NilValue() {
            if TAG(next) != R_NilValue() {
                havenames = TRUE;
            }
            n += 1;
            next = CDR(next);
        }

        let list = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, n));
        let names = if havenames != 0 {
            let nm = Rf_allocVector(SEXPTYPE::STRSXP.0, n);
            Rf_protect(nm);
            nm
        } else {
            R_NilValue()
        };

        let mut cur = args;
        for i in 0..n {
            if havenames != 0 {
                if TAG(cur) != R_NilValue() {
                    SET_STRING_ELT(names, i as R_xlen_t, PRINTNAME(TAG(cur)));
                } else {
                    SET_STRING_ELT(names, i as R_xlen_t, R_BlankString());
                }
            }
            if NAMED(CAR(cur)) != 0 {
                ENSURE_NAMEDMAX(CAR(cur));
            }
            SET_VECTOR_ELT(list, i as R_xlen_t, CAR(cur));
            cur = CDR(cur);
        }
        if havenames != 0 {
            setAttrib(list, R_NamesSymbol(), names);
            Rf_unprotect(1); // names
        }
        Rf_unprotect(1); // list
        list
    }
}

// ---------------------------------------------------------------------------
// do_expression -- expression() (SPECIALSXP)
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_expression(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut named: c_int = 0;
        let n = length(args);
        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::EXPRSXP.0, n));
        let mut a = args;
        for i in 0..n {
            if MAYBE_REFERENCED(CAR(a)) {
                SET_VECTOR_ELT(ans, i as R_xlen_t, duplicate(CAR(a)));
            } else {
                SET_VECTOR_ELT(ans, i as R_xlen_t, CAR(a));
            }
            if TAG(a) != R_NilValue() {
                named = 1;
            }
            a = CDR(a);
        }
        if named != 0 {
            let nms = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, n));
            let mut a2 = args;
            for i in 0..n {
                if TAG(a2) != R_NilValue() {
                    SET_STRING_ELT(nms, i as R_xlen_t, PRINTNAME(TAG(a2)));
                } else {
                    SET_STRING_ELT(nms, i as R_xlen_t, R_BlankString());
                }
                a2 = CDR(a2);
            }
            setAttrib(ans, R_NamesSymbol(), nms);
            Rf_unprotect(1); // nms
        }
        Rf_unprotect(1); // ans
        ans
    }
}

// ---------------------------------------------------------------------------
// do_makevector -- vector(mode="logical", length=0)
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_makevector(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let len: R_xlen_t;
        let mut s: SEXP;
        let mut mode: c_int;

        checkArity(op, args);
        if length(CADR(args)) != 1 {
            Rf_error(c"invalid '%s' argument".as_ptr());
        }
        len = asVecSize(CADR(args));
        if len < 0 {
            Rf_error(c"invalid '%s' argument".as_ptr());
        }
        s = coerceVector(CAR(args), SEXPTYPE::STRSXP.0);
        if length(s) != 1 {
            Rf_error(c"invalid '%s' argument".as_ptr());
        }
        mode = str2type(CHAR(STRING_ELT(s, 0)));
        if mode == -1 {
            // Check for "double" alias
            let mode_str = CHAR(STRING_ELT(s, 0));
            if !mode_str.is_null() {
                let cstr = CStr::from_ptr(mode_str);
                if cstr.to_bytes() == b"double" {
                    mode = SEXPTYPE::REALSXP.0;
                }
            }
        }
        match mode {
            x if x == SEXPTYPE::LGLSXP.0
                || x == SEXPTYPE::INTSXP.0
                || x == SEXPTYPE::REALSXP.0
                || x == SEXPTYPE::CPLXSXP.0
                || x == SEXPTYPE::STRSXP.0
                || x == SEXPTYPE::EXPRSXP.0
                || x == SEXPTYPE::VECSXP.0
                || x == SEXPTYPE::RAWSXP.0 =>
            {
                s = Rf_allocVector3(mode, len);
            }
            x if x == SEXPTYPE::LISTSXP.0 => {
                if len > c_int::MAX as R_xlen_t {
                    Rf_error(c"too long for a pairlist".as_ptr());
                }
                s = allocList(len as c_int);
            }
            _ => {
                Rf_error(c"vector: cannot make a vector of mode '%s'.".as_ptr());
                s = R_NilValue(); // unreachable
            }
        }

        // Zero-fill appropriate types
        if mode == SEXPTYPE::INTSXP.0 || mode == SEXPTYPE::LGLSXP.0 {
            Memzero(INTEGER(s), len);
        } else if mode == SEXPTYPE::REALSXP.0 {
            Memzero(REAL(s), len);
        } else if mode == SEXPTYPE::CPLXSXP.0 {
            Memzero(COMPLEX(s), len);
        } else if mode == SEXPTYPE::RAWSXP.0 {
            Memzero(RAW(s), len);
        }
        // other cases: list/expression have "NULL", ok

        s
    }
}

// ---------------------------------------------------------------------------
// xlengthgets -- set length of a vector or list
// ---------------------------------------------------------------------------

pub unsafe fn xlengthgets(x: SEXP, len: R_xlen_t) -> SEXP {
    unsafe {
        let lenx: R_xlen_t;
        let rval: SEXP;
        let xnames: SEXP;
        let names: SEXP;
        let mut i: R_xlen_t;

        if !isVector(x) && !isList(x) {
            Rf_error(c"cannot set length of non-(vector or list)".as_ptr());
        }
        if len < 0 {
            Rf_error(c"invalid value".as_ptr()); // e.g. -999 from asVecSize()
        }
        if isNull(x) && len > 0 {
            warningcall(
                ptr::null_mut(),
                c"length of NULL cannot be changed".as_ptr(),
            );
        }
        lenx = xlength(x);
        if lenx == len {
            return x;
        }
        let rval = Rf_protect(Rf_allocVector3(TYPEOF(x), len));
        let xnames = Rf_protect(getAttrib(x, R_NamesSymbol()));
        if !isNull(xnames) {
            names = Rf_allocVector(SEXPTYPE::STRSXP.0, len as c_int);
        } else {
            names = R_NilValue();
        }

        let t = TYPEOF(x);
        if t == SEXPTYPE::NILSXP.0 {
            // nothing to copy
        } else if t == SEXPTYPE::LGLSXP.0 || t == SEXPTYPE::INTSXP.0 {
            for i in 0..len {
                if i < lenx {
                    *INTEGER(rval).add(i as usize) = *INTEGER(x).add(i as usize);
                    if !isNull(xnames) {
                        SET_STRING_ELT(names, i, STRING_ELT(xnames, i));
                    }
                } else {
                    *INTEGER(rval).add(i as usize) = NA_INTEGER;
                }
            }
        } else if t == SEXPTYPE::REALSXP.0 {
            for i in 0..len {
                if i < lenx {
                    *REAL(rval).add(i as usize) = *REAL(x).add(i as usize);
                    if !isNull(xnames) {
                        SET_STRING_ELT(names, i, STRING_ELT(xnames, i));
                    }
                } else {
                    *REAL(rval).add(i as usize) = NA_REAL;
                }
            }
        } else if t == SEXPTYPE::CPLXSXP.0 {
            for i in 0..len {
                if i < lenx {
                    *COMPLEX(rval).add(i as usize) = *COMPLEX(x).add(i as usize);
                    if !isNull(xnames) {
                        SET_STRING_ELT(names, i, STRING_ELT(xnames, i));
                    }
                } else {
                    (*COMPLEX(rval).add(i as usize)).r = NA_REAL;
                    (*COMPLEX(rval).add(i as usize)).i = NA_REAL;
                }
            }
        } else if t == SEXPTYPE::STRSXP.0 {
            for i in 0..len {
                if i < lenx {
                    SET_STRING_ELT(rval, i, STRING_ELT(x, i));
                    if !isNull(xnames) {
                        SET_STRING_ELT(names, i, STRING_ELT(xnames, i));
                    }
                } else {
                    SET_STRING_ELT(rval, i, NA_STRING());
                }
            }
        } else if t == SEXPTYPE::LISTSXP.0 {
            let mut t_list = rval;
            let mut x_list = x;
            while !t_list.is_null() && t_list != R_NilValue() {
                SETCAR(t_list, CAR(x_list));
                SETTAG(t_list, TAG(x_list));
                if CDR(t_list).is_null() || CDR(t_list) == R_NilValue() {
                    break;
                }
                t_list = CDR(t_list);
                if CDR(x_list).is_null() || CDR(x_list) == R_NilValue() {
                    break;
                }
                x_list = CDR(x_list);
            }
        } else if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0 {
            for i in 0..len {
                if i < lenx {
                    SET_VECTOR_ELT(rval, i, VECTOR_ELT(x, i));
                    if !isNull(xnames) {
                        SET_STRING_ELT(names, i, STRING_ELT(xnames, i));
                    }
                }
            }
        } else if t == SEXPTYPE::RAWSXP.0 {
            for i in 0..len {
                if i < lenx {
                    *RAW(rval).add(i as usize) = *RAW(x).add(i as usize);
                    if !isNull(xnames) {
                        SET_STRING_ELT(names, i, STRING_ELT(xnames, i));
                    }
                } else {
                    *RAW(rval).add(i as usize) = 0;
                }
            }
        } else {
            UNIMPLEMENTED_TYPE(c"length<-".as_ptr(), x);
        }

        if isVector(x) && !isNull(xnames) {
            setAttrib(rval, R_NamesSymbol(), names);
        }
        // *not* keeping "class": in line with x[1:k]
        Rf_unprotect(2);
        rval
    }
}

// ---------------------------------------------------------------------------
// lengthgets -- older version using R_len_t
// ---------------------------------------------------------------------------

pub unsafe fn lengthgets(x: SEXP, len: c_int) -> SEXP {
    unsafe { xlengthgets(x, len as R_xlen_t) }
}

// ---------------------------------------------------------------------------
// do_lengthgets -- length<- ()
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_lengthgets(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        check1arg(args, call, c"x".as_ptr());

        let x = CAR(args);

        // DispatchOrEval internal generic: length<-
        let mut ans: SEXP = R_NilValue();
        if isObject(x)
            && DispatchOrEval(call, op, c"length<-".as_ptr(), args, rho, &mut ans, 0, 1) != 0
        {
            return ans;
        }
        if length(CADR(args)) != 1 {
            Rf_error(c"wrong length for '%s' argument".as_ptr());
        }
        let len = asVecSize(CADR(args));
        xlengthgets(x, len)
    }
}

// ---------------------------------------------------------------------------
// expandDots -- expand ... in args without evaluating
// ---------------------------------------------------------------------------

unsafe fn expandDots(el: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        Rf_protect(el);
        let head = CONS_NR(R_NilValue(), R_NilValue());
        Rf_protect(head);

        let mut cur = el;
        let mut tail = head;
        while !cur.is_null() && cur != R_NilValue() {
            if CAR(cur) == R_DotsSymbol_fn() {
                let h = R_findVar(CAR(cur), rho);
                Rf_protect(h);
                if TYPEOF(h) == SEXPTYPE::DOTSXP.0 || h == R_NilValue() {
                    let mut h_cur = h;
                    while !h_cur.is_null() && h_cur != R_NilValue() {
                        let new_cell = CONS_NR(CAR(h_cur), R_NilValue());
                        SETCDR(tail, new_cell);
                        tail = CDR(tail);
                        if TAG(h_cur) != R_NilValue() {
                            SETTAG(tail, TAG(h_cur));
                        }
                        h_cur = CDR(h_cur);
                    }
                } else if h != R_MissingArg() {
                    Rf_error(c"'...' used in an incorrect context".as_ptr());
                }
                Rf_unprotect(1); // h
            } else {
                let new_cell = CONS_NR(CAR(cur), R_NilValue());
                SETCDR(tail, new_cell);
                tail = CDR(tail);
                if TAG(cur) != R_NilValue() {
                    SETTAG(tail, TAG(cur));
                }
            }
            cur = CDR(cur);
        }
        Rf_unprotect(2);
        CDR(head)
    }
}

// ---------------------------------------------------------------------------
// setDflt -- record the default value and detect multiple defaults
// ---------------------------------------------------------------------------

unsafe fn setDflt(arg: SEXP, dflt: SEXP) -> SEXP {
    unsafe {
        if !dflt.is_null() {
            let dflt1 = Rf_protect(deparse1line(dflt, true));
            let dflt2 = Rf_protect(deparse1line(CAR(arg), true));
            Rf_error(c"duplicate 'switch' defaults".as_ptr());
            // unreachable
        }
        CAR(arg)
    }
}

// ---------------------------------------------------------------------------
// do_switch -- switch() (SPECIALSXP)
// ---------------------------------------------------------------------------

// no_mangle removed (duplicate)
pub unsafe fn do_switch(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let nargs = length(args);
        let argval: c_int;
        let x: SEXP;
        let mut y: SEXP;
        let mut z: SEXP;
        let w: SEXP;
        let mut ans: SEXP = R_NilValue();
        let mut dflt: SEXP = ptr::null_mut(); // NULL means no default yet

        if nargs < 1 {
            Rf_errorcall_fmt(call, c"'EXPR' is missing".as_ptr(), &[]);
        }
        check1arg(args, call, c"EXPR".as_ptr());
        let x = Rf_protect(eval(CAR(args), rho));
        if !isVector(x) || LENGTH(x) != 1 {
            Rf_errorcall_fmt(call, c"EXPR must be a length 1 vector".as_ptr(), &[]);
        }
        if isFactor(x) {
            warningcall(call, c"EXPR is a \"factor\", treated as integer.\n Consider using 'switch(as.character( * ), ...)' instead.".as_ptr());
        }

        if nargs > 1 {
            let w = Rf_protect(expandDots(CDR(args), rho));
            if isString(x) {
                y = w;
                while !y.is_null() && y != R_NilValue() {
                    if TAG(y) != R_NilValue() {
                        if pmatch(STRING_ELT(x, 0), TAG(y), 1) != 0 {
                            // Find the next non-missing argument
                            while CAR(y) == R_MissingArg() {
                                y = CDR(y);
                                if y.is_null() || y == R_NilValue() {
                                    break;
                                }
                                if TAG(y) == R_NilValue() {
                                    dflt = setDflt(y, dflt);
                                }
                            }
                            if y.is_null() || y == R_NilValue() {
                                Rf_unprotect(2);
                                set_R_Visible(0);
                                return R_NilValue();
                            }
                            // Check for multiple defaults following y
                            z = CDR(y);
                            while !z.is_null() && z != R_NilValue() {
                                if TAG(z) == R_NilValue() {
                                    dflt = setDflt(z, dflt);
                                }
                                z = CDR(z);
                            }

                            ans = eval(CAR(y), rho);
                            Rf_unprotect(2);
                            return ans;
                        }
                    } else {
                        dflt = setDflt(y, dflt);
                    }
                    y = CDR(y);
                }
                if !dflt.is_null() {
                    ans = eval(dflt, rho);
                    Rf_unprotect(2);
                    return ans;
                }
                // fall through to error
            } else {
                // Treat as numeric
                argval = asInteger(x);
                if argval != NA_INTEGER && argval >= 1 && argval <= length(w) {
                    let alt = CAR(nthcdr(w, argval - 1));
                    if alt == R_MissingArg() {
                        Rf_error(c"empty alternative in numeric switch".as_ptr());
                    }
                    ans = eval(alt, rho);
                    Rf_unprotect(2);
                    return ans;
                }
                // fall through to error
            }
            Rf_unprotect(1); // w
        } else {
            warningcall(call, c"'switch' with no alternatives".as_ptr());
        }
        // an error
        Rf_unprotect(1); // x
        set_R_Visible(0);
        R_NilValue()
    }
}
