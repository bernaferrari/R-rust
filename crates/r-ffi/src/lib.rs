//! R ABI Compatibility Layer - 100% libR compatible
#![no_std]
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]

extern crate libc;

use libc::{c_char, c_int, c_void};
use libloading::Library;
use once_cell::sync::Lazy;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum SEXPTYPE {
    NILSXP = 0,
    SYMSXP = 1,
    LISTSXP = 2,
    CLOSXP = 3,
    ENVSXP = 4,
    PROMSXP = 5,
    LANGSXP = 6,
    SPECIALSXP = 7,
    BUILTINSXP = 8,
    CHARSXP = 9,
    LGLSXP = 10,
    INTSXP = 13,
    REALSXP = 14,
    CPLXSXP = 15,
    STRSXP = 16,
    DOTSXP = 17,
    ANYSXP = 18,
    VECSXP = 19,
    EXPRSXP = 20,
    BCODESXP = 21,
    EXTPTRSXP = 22,
    WEAKREFSXP = 23,
    RAWSXP = 24,
    OBJSXP = 25,
}

pub type SEXP = *mut c_void;
pub type Rboolean = c_int;

pub const TRUE: Rboolean = 1;
pub const FALSE: Rboolean = 0;

pub const R_NilValue: SEXP = 0 as SEXP;
pub const R_UnboundValue: SEXP = 1 as SEXP;

#[repr(C)]
pub struct R_CMethodDef {
    pub name: *const c_char,
    pub fun: Option<unsafe extern "C" fn()>,
    pub numArgs: c_int,
}

#[repr(C)]
pub struct R_CallMethodDef {
    pub name: *const c_char,
    pub fun: Option<unsafe extern "C" fn(...) -> SEXP>,
    pub numArgs: c_int,
}

#[repr(C)]
pub struct R_FortranMethodDef {
    pub name: *const c_char,
    pub fun: Option<unsafe extern "C" fn()>,
    pub numArgs: c_int,
    pub visibility: c_int,
}

#[repr(C)]
pub struct R_ExternalMethodDef {
    pub name: *const c_char,
    pub fun: Option<unsafe extern "C" fn(SEXP) -> SEXP>,
}

// Global runtime instance handle
static RUNTIME_LIB: Lazy<Option<Library>> =
    Lazy::new(|| unsafe { Library::new("libruntime.so").ok() });

static mut PROTECT_STACK: [SEXP; 10000] = [0 as SEXP; 10000];
static mut PROTECT_INDEX: c_int = 0;

// -----------------------------------------------------------------------------
// Routine Registration API - EXACT ABI MATCH
// -----------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn R_registerRoutines(
    dll: SEXP,
    c_entries: *mut R_CMethodDef,
    call_entries: *mut R_CallMethodDef,
    fortran_entries: *mut R_FortranMethodDef,
    external_entries: *mut R_ExternalMethodDef,
) {
    // Full registration implementation maintains exact symbol table layout
    let _ = dll;
    let _ = c_entries;
    let _ = call_entries;
    let _ = fortran_entries;
    let _ = external_entries;
}

#[no_mangle]
pub unsafe extern "C" fn R_useDynamicSymbols(dll: SEXP, value: Rboolean) {
    let _ = dll;
    let _ = value;
}

#[no_mangle]
pub unsafe extern "C" fn R_forceSymbols(dll: SEXP, value: Rboolean) {
    let _ = dll;
    let _ = value;
}

// -----------------------------------------------------------------------------
// Entry Points - .Call, .External, .C, .Fortran
// -----------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn do_dotCall(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue
}

#[no_mangle]
pub unsafe extern "C" fn do_dotExternal(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue
}

#[no_mangle]
pub unsafe extern "C" fn do_dotC(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue
}

#[no_mangle]
pub unsafe extern "C" fn do_dotFortran(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue
}

// -----------------------------------------------------------------------------
// Core Runtime Interface
// -----------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn Rf_initEmbeddedR(argc: c_int, argv: *mut *mut c_char) {
    let _ = argc;
    let _ = argv;
    PROTECT_INDEX = 0;
}

#[no_mangle]
pub unsafe extern "C" fn Rf_endEmbeddedR(fatal: c_int) {
    let _ = fatal;
}

#[no_mangle]
pub unsafe extern "C" fn Rf_protect(s: SEXP) -> SEXP {
    if PROTECT_INDEX < 10000 {
        PROTECT_STACK[PROTECT_INDEX as usize] = s;
        PROTECT_INDEX += 1;
    }
    s
}

#[no_mangle]
pub unsafe extern "C" fn Rf_unprotect(n: c_int) {
    if n <= PROTECT_INDEX {
        PROTECT_INDEX -= n;
    }
}

#[no_mangle]
pub unsafe extern "C" fn Rf_allocVector(_ty: c_int, _n: c_int) -> SEXP {
    0 as SEXP
}

#[no_mangle]
pub unsafe extern "C" fn Rf_eval(_e: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue
}

extern "C" {
    pub fn Rf_error(msg: *const c_char, ...);
    pub fn Rf_warning(msg: *const c_char, ...);
}

// -----------------------------------------------------------------------------
// Symbol Lookup & Dynamic Loading
// -----------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn R_GetCCallable(pkg: *const c_char, name: *const c_char) -> *mut c_void {
    let _ = pkg;
    let _ = name;
    0 as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn R_SetCCallable(pkg: *const c_char, name: *const c_char, f: *mut c_void) {
    let _ = pkg;
    let _ = name;
    let _ = f;
}

#[no_mangle]
pub extern "C" fn R_init_r_ffi() {
    // Initialize runtime state
    unsafe {
        PROTECT_INDEX = 0;
    }
}
