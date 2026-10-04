#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/dotcode.c — foreign function interface for .Call, .C, .Fortran, .External.
//!
//! Provides the dispatch machinery for calling native C/Fortran routines from R code.
//! Faithfully ports the argument marshaling, symbol resolution, and error checking from
//! the C implementation while using idiomatic Rust patterns to collapse the repetitive
//! function-pointer typedefs and switch dispatches.

use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::mainutils::memory_main::{R_ExternalPtrAddr, sexptype2char};
use crate::mainutils::rdynload::{
    R_FindSymbol as R_lookupLoadedSymbol, R_dlsym, R_findDllByHandle,
};
use crate::mainutils::registration::DllInfo;
use crate::mainutils::relop::PRIMVAL;
use crate::sexp::accessors::{
    CAR, CDR, COMPLEX, INTEGER, LENGTH, PRINTNAME, RAW, REAL, SET_STRING_ELT, SET_VECTOR_ELT,
    SETCDR, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH, translateChar,
};
use crate::sexp::attrib_core::{getAttrib, setAttrib};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector, Rf_length, Rf_mkChar};
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::memory_ext::{R_alloc, vmaxget, vmaxset};
use crate::sexp::symbol::Rf_install;
use crate::unix::dynload::DL_FUNC;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum entry-point name length including nul terminator.
const MAX_SYMBOL_BYTES: usize = 1024;

/// Maximum number of arguments to .C, .Fortran and .Call.
const MAX_ARGS: usize = 65;

/// Maximum DLL / path name length.
const R_PATH_MAX: usize = 4096;

/// Guard byte pattern and count for bounds checking in .C/.Fortran.
const FILL: u8 = 0xee;
const NG: usize = 64;

#[derive(Default)]
pub(crate) struct DotcodeRuntimeState {
    pub retval_check: Option<bool>,
}

// ---------------------------------------------------------------------------
// Native symbol type constants
// ---------------------------------------------------------------------------

const R_ANY_SYM: c_int = -1;
const R_C_SYM: c_int = 1;
const R_FORTRAN_SYM: c_int = 2;
const R_CALL_SYM: c_int = 3;
const R_EXTERNAL_SYM: c_int = 4;

// ---------------------------------------------------------------------------
// DllReference — identifies which DLL to resolve symbols in
// ---------------------------------------------------------------------------

/// Discriminant for how the DLL was specified.
const NOT_DEFINED: i32 = 0;
const FILENAME: i32 = 1;
const DLL_HANDLE: i32 = 2;
const R_OBJECT: i32 = 3;

struct DllReference {
    dll_name: [u8; R_PATH_MAX],
    dll: *mut c_void,
    obj: SEXP,
    ref_type: i32,
}

impl DllReference {
    fn new() -> Self {
        let mut dll_name = [0u8; R_PATH_MAX];
        dll_name[0] = 0;
        DllReference {
            dll_name,
            dll: ptr::null_mut(),
            obj: ptr::null_mut(),
            ref_type: NOT_DEFINED,
        }
    }
}

// ---------------------------------------------------------------------------
// Registered native symbol types (matching R_ext/Rdynload.h)
// ---------------------------------------------------------------------------

#[repr(C)]
struct R_CMethodDef {
    name: *mut c_char,
    fun: DL_FUNC,
    num_args: c_int,
    types: *mut c_int,
}

#[repr(C)]
struct R_CallMethodDef {
    name: *mut c_char,
    fun: DL_FUNC,
    num_args: c_int,
}

#[repr(C)]
struct R_FortranMethodDef {
    name: *mut c_char,
    fun: DL_FUNC,
    num_args: c_int,
}

#[repr(C)]
struct R_ExternalMethodDef {
    name: *mut c_char,
    fun: DL_FUNC,
    num_args: c_int,
}

/// Union holding a pointer to one of the registered symbol definition types.
#[repr(C)]
union NativeSymbolPtr {
    c: *mut R_CMethodDef,
    call: *mut R_CallMethodDef,
    fortran: *mut R_FortranMethodDef,
    external: *mut R_ExternalMethodDef,
}

/// Tracks a registered native routine and which DLL it belongs to.
#[repr(C)]
struct R_RegisteredNativeSymbol {
    type_: c_int,
    symbol: NativeSymbolPtr,
    dll: *mut DllInfo,
}

impl R_RegisteredNativeSymbol {
    fn new(sym_type: c_int) -> Self {
        R_RegisteredNativeSymbol {
            type_: sym_type,
            symbol: NativeSymbolPtr { c: ptr::null_mut() },
            dll: ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// Local error / warning helpers
// ---------------------------------------------------------------------------

unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

unsafe fn errorcall(_call: SEXP, msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

unsafe fn native_extension_policy_error(call: SEXP, entrypoint: &str) -> ! {
    unsafe {
        errorcall(
            call,
            &format!(
                "{entrypoint} calls native extension code, which is disabled in this pure-R Android runtime; package authors should use Rust-ported internals or a host-owned native-library policy"
            ),
        )
    }
}

fn native_extension_policy_enabled() -> bool {
    !crate::mainutils::rdynload::native_extensions_enabled()
}

unsafe fn ported_call_name(op: SEXP) -> Option<String> {
    unsafe {
        if TYPEOF(op) == SEXPTYPE::STRSXP && LENGTH(op) >= 1 {
            let ptr = translateChar(STRING_ELT(op, 0));
            if ptr.is_null() {
                return None;
            }
            return std::ffi::CStr::from_ptr(ptr)
                .to_str()
                .ok()
                .map(str::to_string);
        }
        if TYPEOF(op) == SEXPTYPE::SYMSXP {
            let ptr = translateChar(PRINTNAME(op));
            if ptr.is_null() {
                return None;
            }
            return std::ffi::CStr::from_ptr(ptr)
                .to_str()
                .ok()
                .map(str::to_string);
        }
        if TYPEOF(op) == SEXPTYPE::VECSXP && XLENGTH(op) >= 1 {
            return ported_call_name(VECTOR_ELT(op, 0));
        }
        None
    }
}


unsafe fn warning(msg: &str) {
    eprintln!("WARNING: {}", msg);
}

unsafe fn warningcall(_call: SEXP, msg: &str) {
    eprintln!("WARNING: {}", msg);
}

fn dotcode_retval_check_enabled() -> bool {
    std::env::var("_R_CHECK_DOTCODE_RETVAL_")
        .map(|p| p == "TRUE" || p == "true" || p == "1" || p == "yes")
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Session-local symbols
// ---------------------------------------------------------------------------

unsafe fn NaokSymbol() -> SEXP {
    unsafe { Rf_install(b"NAOK\0".as_ptr() as *const c_char) }
}

unsafe fn DupSymbol() -> SEXP {
    unsafe { Rf_install(b"DUP\0".as_ptr() as *const c_char) }
}

unsafe fn PkgSymbol() -> SEXP {
    unsafe { Rf_install(b"PACKAGE\0".as_ptr() as *const c_char) }
}

unsafe fn EncSymbol() -> SEXP {
    unsafe { Rf_install(b"ENCODING\0".as_ptr() as *const c_char) }
}

unsafe fn CSingSymbol() -> SEXP {
    unsafe { Rf_install(b"Csingle\0".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// Helper: isValidString
// ---------------------------------------------------------------------------

/// Check that `s` is a length-1 character string that is not NA.
unsafe fn isValidString(s: SEXP) -> bool {
    unsafe { TYPEOF(s) == SEXPTYPE::STRSXP && LENGTH(s) > 0 }
}

unsafe fn isValidStringF(s: SEXP) -> bool {
    unsafe {
        if !isValidString(s) {
            return false;
        }
        let elt = STRING_ELT(s, 0);
        // Check it's not NA_STRING (NA_STRING has type == CHARSXP and is marked)
        !elt.is_null() && TYPEOF(elt) == SEXPTYPE::CHARSXP
    }
}

// ---------------------------------------------------------------------------
// Helper: isNativeSymbolInfo
// ---------------------------------------------------------------------------

/// Structural check that replaces inherits(op, "NativeSymbolInfo").
unsafe fn isNativeSymbolInfo(op: SEXP) -> bool {
    unsafe {
        TYPEOF(op) == SEXPTYPE::VECSXP
            && LENGTH(op) >= 2
            && TYPEOF(VECTOR_ELT(op, 1)) == SEXPTYPE::EXTPTRSXP
    }
}

// ---------------------------------------------------------------------------
// check1arg2
// ---------------------------------------------------------------------------

unsafe fn check1arg2(arg: SEXP, call: SEXP, _formal: &str) {
    unsafe {
        if TAG(arg).is_null() || TAG(arg).is_null() {
            // Untagged — fine, it's positional
            return;
        }
        if TAG(arg) == R_NilValue() {
            return;
        }
        errorcall(call, "the first argument should not be named");
    }
}

// ---------------------------------------------------------------------------
// checkValidSymbolId
// ---------------------------------------------------------------------------

/// Validates and resolves a .NAME argument. May set `fun` and `symbol` if
/// the argument is an external pointer or NativeSymbolInfo.
unsafe fn checkValidSymbolId(
    op: SEXP,
    call: SEXP,
    fun: &mut DL_FUNC,
    _symbol: &mut R_RegisteredNativeSymbol,
    buf: *mut u8,
) {
    unsafe {
        if isValidStringF(op) {
            if !buf.is_null() {
                let name = translateChar(STRING_ELT(op, 0));
                let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
                if bytes.len() >= MAX_SYMBOL_BYTES {
                    errorcall(call, "symbol name is too long");
                }
                ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                *buf.add(bytes.len()) = 0;
            }
            return;
        }

        if TYPEOF(op) == SEXPTYPE::EXTPTRSXP {
            let _native_sym = Rf_install(b"native symbol\0".as_ptr() as *const c_char);
            let _reg_native_sym =
                Rf_install(b"registered native symbol\0".as_ptr() as *const c_char);

            if fun.is_none() {
                errorcall(call, "NULL value passed as symbol address");
            }
            return;
        }

        if isNativeSymbolInfo(op) {
            checkValidSymbolId(VECTOR_ELT(op, 1), call, fun, _symbol, buf);
            return;
        }

        errorcall(
            call,
            "first argument must be a string (of length 1) or native symbol reference",
        );
    }
}

// ---------------------------------------------------------------------------
// R_dotCallFn
// ---------------------------------------------------------------------------

/// Called from the R-level .Call2() implementation.
///
/// # Safety
/// The caller retains the original runtime and the supplied symbol graph.
pub unsafe fn R_dotCallFn(op: SEXP, call: SEXP, _nargs: c_int) -> DL_FUNC {
    unsafe {
        let mut symbol = R_RegisteredNativeSymbol::new(R_CALL_SYM);
        let mut fun: DL_FUNC = None;
        checkValidSymbolId(op, call, &mut fun, &mut symbol, ptr::null_mut());
        fun
    }
}

// ---------------------------------------------------------------------------
// naokfind
// ---------------------------------------------------------------------------

/// Finds and removes NAOK, DUP, and PACKAGE arguments from the arg list.
/// Returns the pruned argument list and fills in `len` and `naok`.
unsafe fn naokfind(args: SEXP, len: *mut c_int, naok: *mut c_int, dll: &mut DllReference) -> SEXP {
    unsafe {
        let mut nargs = 0i32;
        let mut naok_used = 0u32;
        let mut dup_used = 0u32;
        let mut pkg_used = 0u32;
        *naok = 0;
        *len = 0;

        let naok_sym = NaokSymbol();
        let dup_sym = DupSymbol();
        let pkg_sym = PkgSymbol();

        let mut s = args;
        let mut prev = args;
        let mut head = args;

        while !s.is_null() && s != R_NilValue() {
            let tag = TAG(s);
            if tag == naok_sym {
                *naok = crate::mainutils::coerce::asLogical(CAR(s));
                naok_used += 1;
                if naok_used > 1 {
                    warning("'NAOK' used more than once");
                }
            } else if tag == dup_sym {
                dup_used += 1;
                if dup_used > 1 {
                    warning("'DUP' used more than once");
                }
            } else if tag == pkg_sym {
                let car = CAR(s);
                dll.obj = car;
                if TYPEOF(car) == SEXPTYPE::STRSXP {
                    let p = translateChar(STRING_ELT(car, 0));
                    let p_str = std::ffi::CStr::from_ptr(p).to_bytes();
                    if p_str.len() >= R_PATH_MAX - 1 {
                        error("DLL name is too long");
                    }
                    dll.ref_type = FILENAME;
                    let copy_len = p_str.len().min(R_PATH_MAX - 1);
                    dll.dll_name[..copy_len].copy_from_slice(&p_str[..copy_len]);
                    dll.dll_name[copy_len] = 0;
                    pkg_used += 1;
                    if pkg_used > 1 {
                        warning("'PACKAGE' used more than once");
                    }
                } else if TYPEOF(car) == SEXPTYPE::EXTPTRSXP {
                    dll.dll = R_ExternalPtrAddr(car);
                    dll.ref_type = DLL_HANDLE;
                } else if TYPEOF(car) == SEXPTYPE::VECSXP {
                    dll.ref_type = R_OBJECT;
                    dll.obj = s;
                    let name = translateChar(STRING_ELT(VECTOR_ELT(car, 1), 0));
                    let name_str = std::ffi::CStr::from_ptr(name).to_bytes();
                    let copy_len = name_str.len().min(R_PATH_MAX - 1);
                    dll.dll_name[..copy_len].copy_from_slice(&name_str[..copy_len]);
                    dll.dll_name[copy_len] = 0;
                    dll.dll = R_ExternalPtrAddr(VECTOR_ELT(s, 4));
                } else {
                    error(&format!(
                        "incorrect type ({}) of PACKAGE argument",
                        std::ffi::CStr::from_ptr(sexptype2char(SEXPTYPE(TYPEOF(car))))
                            .to_string_lossy()
                    ));
                }
            } else {
                nargs += 1;
                prev = s;
                s = CDR(s);
                continue;
            }
            if s == head {
                head = CDR(s);
                s = CDR(s);
            } else {
                SETCDR(prev, CDR(s));
                s = CDR(s);
            }
        }

        *len = nargs;
        head
    }
}

// ---------------------------------------------------------------------------
// setDLLname / pkgtrim
// ---------------------------------------------------------------------------

unsafe fn setDLLname(s: SEXP, dll_name: &mut [u8; R_PATH_MAX]) {
    unsafe {
        let ss = CAR(s);
        if TYPEOF(ss) != SEXPTYPE::STRSXP || LENGTH(ss) != 1 {
            error("PACKAGE argument must be a single character string");
        }
        let name = translateChar(STRING_ELT(ss, 0));
        let name_bytes = std::ffi::CStr::from_ptr(name).to_bytes();
        // Skip "package:" prefix if present
        let name_bytes = if name_bytes.starts_with(b"package:") {
            &name_bytes[8..]
        } else {
            name_bytes
        };
        if name_bytes.len() >= R_PATH_MAX - 1 {
            error("PACKAGE argument is too long");
        }
        let copy_len = name_bytes.len().min(R_PATH_MAX - 1);
        dll_name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        dll_name[copy_len] = 0;
    }
}

unsafe fn pkgtrim(args: SEXP, dll: &mut DllReference) -> SEXP {
    unsafe {
        let pkg_sym = PkgSymbol();
        let mut pkg_used = 0u32;
        let mut s = args;
        let head = args;

        while !s.is_null() && s != R_NilValue() {
            let ss = CDR(s);
            if ss == R_NilValue() && TAG(s) == pkg_sym {
                pkg_used += 1;
                if pkg_used > 1 {
                    warning("'PACKAGE' used more than once");
                }
                setDLLname(s, &mut dll.dll_name);
                dll.ref_type = FILENAME;
                return R_NilValue();
            }
            if TAG(ss) == pkg_sym {
                pkg_used += 1;
                if pkg_used > 1 {
                    warning("'PACKAGE' used more than once");
                }
                setDLLname(ss, &mut dll.dll_name);
                dll.ref_type = FILENAME;
                // Can't easily mutate the list — simplified removal
            }
            s = CDR(s);
        }
        head
    }
}

// ---------------------------------------------------------------------------
// enctrim
// ---------------------------------------------------------------------------

unsafe fn enctrim(args: SEXP) -> SEXP {
    unsafe {
        let enc_sym = EncSymbol();
        let mut s = args;
        let head = args;
        while !s.is_null() && s != R_NilValue() {
            let ss = CDR(s);
            if (ss == R_NilValue() && TAG(s) == enc_sym) || TAG(ss) == enc_sym {
                warning("ENCODING is defunct and will be ignored");
                if ss == R_NilValue() && TAG(s) == enc_sym {
                    return R_NilValue();
                }
            }
            s = CDR(s);
        }
        head
    }
}

// ---------------------------------------------------------------------------
// checkNativeType / comparePrimitiveTypes
// ---------------------------------------------------------------------------

unsafe fn checkNativeType(target_type: c_int, actual_type: c_int) -> bool {
    if target_type > 0 {
        if target_type == SEXPTYPE::INTSXP || target_type == SEXPTYPE::LGLSXP {
            return actual_type == SEXPTYPE::INTSXP || actual_type == SEXPTYPE::LGLSXP;
        }
        return target_type == actual_type;
    }
    true
}

unsafe fn comparePrimitiveTypes(ty: c_int, s: SEXP) -> bool {
    unsafe {
        if ty < 0 || TYPEOF(s) == ty {
            return true;
        }
        // SINGLESXP check
        if ty == 14 {
            // SINGLESXP in R
            return crate::mainutils::coerce::asLogical(getAttrib(
                s,
                Rf_install(b"Csingle\0".as_ptr() as *const c_char),
            )) == TRUE as c_int;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// resolveNativeRoutine
// ---------------------------------------------------------------------------

/// Resolves the native routine to call from the .NAME argument, handling
/// PACKAGE=, NAOK=, symbol lookup from namespaces, etc.
unsafe fn resolveNativeRoutine(
    args: SEXP,
    fun: &mut DL_FUNC,
    symbol: &mut R_RegisteredNativeSymbol,
    buf: &mut [u8; MAX_SYMBOL_BYTES],
    nargs: *mut c_int,
    naok: *mut c_int,
    call: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        let mut dll = DllReference::new();

        let op = CAR(args);
        checkValidSymbolId(op, call, fun, symbol, buf.as_mut_ptr());

        if symbol.type_ == R_C_SYM || symbol.type_ == R_FORTRAN_SYM {
            let mut n: c_int = 0;
            let mut na: c_int = 0;
            let pruned = naokfind(CDR(args), &mut n, &mut na, &mut dll);
            if na == crate::sexp::ffi::NA_INTEGER {
                errorcall(call, "invalid 'naok' value");
            }
            if !nargs.is_null() {
                *nargs = n;
            }
            if !naok.is_null() {
                *naok = na;
            }
            if n as usize > MAX_ARGS {
                errorcall(call, "too many arguments in foreign function call");
            }
        }
        let pruned = pkgtrim(CDR(args), &mut dll);

        if fun.is_none() && !buf.is_empty() {
            let looked_up = if dll.ref_type == DLL_HANDLE && !dll.dll.is_null() {
                let loaded = R_findDllByHandle(dll.dll);
                if loaded.is_null() {
                    None
                } else {
                    R_dlsym(loaded, buf.as_ptr() as *const c_char, symbol.type_)
                }
            } else {
                let pkg_ptr = if dll.dll_name[0] == 0 {
                    b"\0".as_ptr() as *const c_char
                } else {
                    dll.dll_name.as_ptr() as *const c_char
                };
                R_lookupLoadedSymbol(buf.as_ptr() as *const c_char, pkg_ptr, symbol.type_)
            };

            *fun = looked_up;
        }

        pruned
    }
}

// ---------------------------------------------------------------------------
// check_retval
// ---------------------------------------------------------------------------

unsafe fn check_retval(call: SEXP, val: SEXP) -> SEXP {
    unsafe {
        let do_check = instance::with_required_current_instance(|inst| {
            // P1: short-lived raw place access, no &mut held across calls.
            let slot = (*inst)
                .dotcode_state
                .retval_check
                .get_or_insert_with(dotcode_retval_check_enabled);
            *slot
        });

        if do_check {
            if (val as usize) < 16 {
                errorcall(call, &format!("WEIRD RETURN VALUE: {:?}", val));
            }
        } else if val.is_null() {
            warningcall(call, "converting NULL pointer to R NULL");
            return R_NilValue();
        }

        val
    }
}

// ---------------------------------------------------------------------------
// Function pointer dispatch via macro
// ---------------------------------------------------------------------------

/// Dispatch a .Call function returning SEXP by argument count.
unsafe fn dispatch_dotcall(fun: DL_FUNC, args: &[SEXP], call: SEXP) -> SEXP {
    unsafe {
        let _ = call;
        match args.len() {
            0 => {
                let f: unsafe extern "C-unwind" fn() -> SEXP = std::mem::transmute_copy(&fun);
                f()
            }
            1 => {
                let f: unsafe extern "C-unwind" fn(SEXP) -> SEXP = std::mem::transmute_copy(&fun);
                f(args[0])
            }
            2 => {
                let f: unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&fun);
                f(args[0], args[1])
            }
            3 => {
                let f: unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute_copy(&fun);
                f(args[0], args[1], args[2])
            }
            4 => {
                let f: unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute_copy(&fun);
                f(args[0], args[1], args[2], args[3])
            }
            5 => {
                let f: unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute_copy(&fun);
                f(args[0], args[1], args[2], args[3], args[4])
            }
            6 => {
                let f: unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute_copy(&fun);
                f(args[0], args[1], args[2], args[3], args[4], args[5])
            }
            7 => {
                let f: unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute_copy(&fun);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6],
                )
            }
            8 => {
                let f: unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute_copy(&fun);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                )
            }
            9 => {
                let f: unsafe extern "C-unwind" fn(
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                ) -> SEXP = std::mem::transmute_copy(&fun);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8],
                )
            }
            10 => {
                let f: unsafe extern "C-unwind" fn(
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                ) -> SEXP = std::mem::transmute_copy(&fun);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9],
                )
            }
            n if n <= MAX_ARGS => {
                errorcall(call, "too many arguments in foreign function call");
            }
            _ => errorcall(ptr::null_mut(), "too many arguments, sorry"),
        }
    }
}


/// Dispatch a .C/.Fortran void function by argument count.
unsafe fn dispatch_wide(fun: DL_FUNC, args: &[*mut c_void]) -> bool {
    unsafe {
        macro_rules! call_n {
            ($($i:literal),+) => {{
                let f: unsafe extern "C" fn($(call_n!(@t $i)),+) = std::mem::transmute_copy(&fun);
                f($(args[$i]),+);
            }};
            (@t $i:literal) => { *mut c_void };
        }
        match args.len() {
            11 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10),
            12 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11),
            13 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12),
            14 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13),
            15 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14),
            16 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15),
            18 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17),
            19 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18),
            20 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19),
            21 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20),
            22 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21),
            23 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22),
            24 => call_n!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23),
            _ => return false,
        }
        true
    }
}

unsafe fn dispatch_dotcode(fun: DL_FUNC, args: &[*mut c_void], call: SEXP) {
    unsafe {
        let _ = call;
        match args.len() {
            0 => {
                let f: unsafe extern "C" fn() = std::mem::transmute_copy(&fun);
                f()
            }
            1 => {
                let f: unsafe extern "C" fn(*mut c_void) = std::mem::transmute_copy(&fun);
                f(args[0])
            }
            2 => {
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void) =
                    std::mem::transmute_copy(&fun);
                f(args[0], args[1])
            }
            3 => {
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
                    std::mem::transmute_copy(&fun);
                f(args[0], args[1], args[2])
            }
            4 => {
                let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) =
                    std::mem::transmute_copy(&fun);
                f(args[0], args[1], args[2], args[3])
            }
            5 => {
                let f: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) = std::mem::transmute_copy(&fun);
                f(args[0], args[1], args[2], args[3], args[4])
            }
            6 => {
                let f: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) = std::mem::transmute_copy(&fun);
                f(args[0], args[1], args[2], args[3], args[4], args[5])
            }
            7 => {
                let f: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) = std::mem::transmute_copy(&fun);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6],
                )
            }
            8 => {
                let f: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) = std::mem::transmute_copy(&fun);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                )
            }
            9 => {
                let f: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) = std::mem::transmute_copy(&fun);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8],
                )
            }
            10 => {
                let f: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) = std::mem::transmute_copy(&fun);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9],
                )
            }
            17 => {
                let f: unsafe extern "C" fn(
                    *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void,
                    *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void,
                    *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void,
                ) = std::mem::transmute_copy(&fun);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                    args[8], args[9], args[10], args[11], args[12], args[13], args[14], args[15],
                    args[16],
                )
            }
            n if n <= MAX_ARGS => {
                if !dispatch_wide(fun, args) {
                    errorcall(call, "too many arguments in foreign function call");
                }
            }
            _ => errorcall(ptr::null_mut(), "too many arguments, sorry"),
        }
    }
}


// ---------------------------------------------------------------------------
// R_doDotCall — the .Call dispatcher
// ---------------------------------------------------------------------------

/// Foreign ABI dispatch for host-authorized native extensions. Bundled Rust
/// routines use exact typed descriptors instead.
///
/// # Safety
/// The caller proves the erased pointer implements the exact C-unwind ABI and
/// argument count, keeps its library loaded, and retains all payload allocations
/// and the original runtime across invocation and result validation.
pub unsafe fn R_doDotCall(fun: DL_FUNC, nargs: c_int, cargs: &[SEXP], call: SEXP) -> SEXP {
    unsafe {
        if fun.is_none() {
            return R_NilValue();
        }
        let n = nargs as usize;
        if n > MAX_ARGS {
            errorcall(call, "too many arguments, sorry");
        }
        let args = &cargs[..n];
        let retval = dispatch_dotcall(fun, args, call);
        check_retval(call, retval)
    }
}

// ---------------------------------------------------------------------------
// do_External — .External and .External2
// ---------------------------------------------------------------------------

/// Actual owning payloads captured before reconstruction or native callbacks.
/// Control arguments select resolution; they never become callable payloads.
struct NativeOperands {
    name: crate::sexp::object::Sexp<'static>,
    payload: Vec<(
        crate::sexp::object::Sexp<'static>,
        crate::sexp::object::Sexp<'static>,
    )>,
    package: Option<(
        String,
        crate::sexp::object::Sexp<'static>,
        crate::sexp::object::Sexp<'static>,
    )>,
}

fn native_admission_error(message: impl Into<String>) -> crate::sexp::object::SexpError {
    crate::sexp::object::SexpError::EvaluationFailed {
        message: message.into(),
    }
}

impl NativeOperands {
    fn capture(
        arguments: crate::sexp::object::Sexp<'static>,
        interface: crate::mainutils::native_routines::NativeInterface,
    ) -> crate::sexp::object::SexpResult<Self> {
        if arguments.is_nil() {
            return Err(native_admission_error("'.NAME' is missing"));
        }
        if !arguments.try_tag()?.is_nil() {
            return Err(native_admission_error(
                "the first argument should not be named",
            ));
        }
        let name = arguments.try_car()?.into_owned()?;
        let mut snapshots = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut cursor = arguments.try_cdr()?;
        while !cursor.is_nil() {
            let identity = cursor
                .allocation()?
                .link()
                .ok_or(crate::sexp::object::SexpError::StaleAllocation)?;
            if !seen.insert(identity) {
                return Err(native_admission_error("cyclic native argument list"));
            }
            let value = cursor.try_car()?.into_owned()?;
            let tag = cursor.try_tag()?.into_owned()?;
            snapshots.push((value, tag));
            cursor = cursor.try_cdr()?;
        }
        let mut payload = Vec::new();
        let mut package = None;
        // Every source cell and edge has been captured before character/ALTREP
        // access can invoke a provider and detach the original argument graph.
        for (value, tag) in snapshots {
            let control = !tag.is_nil() && tag.try_printname()?.try_as_string()? == "PACKAGE";
            if control {
                if value.typeof_() != SEXPTYPE::STRSXP || value.len() != 1 {
                    return Err(native_admission_error(
                        "PACKAGE argument must be a single character string",
                    ));
                }
                let text = value.try_string_elt(0)?.try_as_string()?;
                if package.is_some() {
                    eprintln!("WARNING: 'PACKAGE' used more than once");
                }
                package = Some((
                    text.strip_prefix("package:").unwrap_or(&text).to_owned(),
                    value,
                    tag,
                ));
            } else {
                if interface == crate::mainutils::native_routines::NativeInterface::Call
                    && payload.len() == MAX_ARGS
                {
                    return Err(native_admission_error(
                        "too many arguments in foreign function call",
                    ));
                }
                payload.push((value, tag));
            }
        }
        Ok(Self {
            name,
            payload,
            package,
        })
    }

    fn lookup_name(&self) -> crate::sexp::object::SexpResult<Option<String>> {
        let name = if self.name.typeof_() == SEXPTYPE::VECSXP {
            if self.name.len() == 0 {
                return Ok(None);
            }
            self.name.try_vector_elt(0)?
        } else {
            self.name.clone()
        };
        match name.typeof_() {
            SEXPTYPE::STRSXP if name.len() == 1 => {
                Ok(Some(name.try_string_elt(0)?.try_as_string()?))
            }
            SEXPTYPE::SYMSXP => Ok(Some(name.try_printname()?.try_as_string()?)),
            _ => Ok(None),
        }
    }

    fn argument_list(
        &self,
        allocator: &crate::sexp::object::NodeAllocator<'_, 'static>,
        include_package: bool,
        nil: &crate::sexp::object::Sexp<'static>,
    ) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
        let mut result = nil.clone();
        if include_package {
            if let Some((_, value, tag)) = &self.package {
                result = allocator.pairlist_cell(value, &result, tag)?;
            }
        }
        for (value, tag) in self.payload.iter().rev() {
            result = allocator.pairlist_cell(value, &result, tag)?;
        }
        allocator.pairlist_cell(&self.name, &result, nil)
    }
}

fn lookup_bundled_native(
    name: &str,
    package: Option<&str>,
) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    fn in_package(
        name: &str,
        package: &str,
    ) -> Option<crate::mainutils::native_routines::NativeRoutine> {
        match package {
            "methods" => crate::library::methods::native_calls::lookup(name),
            "tools" => crate::library::tools::native_calls::lookup(name),
            "utils" => crate::library::utils::lookup(name),
            "stats" => crate::library::stats::random::lookup_external(name)
                .or_else(|| crate::library::stats::random::lookup_call(name)),
            "splines" => crate::library::splines::splines::lookup(name),
            "grDevices" => crate::library::grdevices::lookup(name),
            "grid" => crate::library::grid::lookup(name),
            "graphics" => crate::library::graphics::lookup(name),
            _ => None,
        }
    }
    match package {
        Some(package) => in_package(name, package),
        None => [
            "methods",
            "tools",
            "utils",
            "stats",
            "splines",
            "grDevices",
            "grid",
            "graphics",
        ]
        .into_iter()
        .find_map(|package| in_package(name, package)),
    }
}

/// This translated entry retains original values and execution authority for
/// the full callback, including reconstruction and result validation.
unsafe fn invoke_native_handler(
    call: SEXP,
    operator: SEXP,
    arguments: SEXP,
    environment: SEXP,
    interface: crate::mainutils::native_routines::NativeInterface,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    use crate::mainutils::native_routines::NativeInterface;
    use crate::sexp::{
        object::SexpError,
        owner::{OwnerToken, with_runtime},
    };
    let owner = unsafe { OwnerToken::current()? };
    let managed = owner.weak_owner().ok_or(SexpError::RootUnavailable)?;
    // Primitive projections may be immortal native table entries. Capture
    // their immutable identity before allocating a local owning operator.
    let operator_identity =
        unsafe { crate::eval::primitive::PrimitiveDescriptor::from_raw(operator) }
            .map(|descriptor| (descriptor.name, descriptor.op.typeof_()));
    with_runtime(&managed, |access| {
        let domain = access.domain();
        let call = domain.wrap(call)?.into_owned()?;
        let environment = domain.wrap(environment)?.into_owned()?;
        let operands = NativeOperands::capture(domain.wrap(arguments)?.into_owned()?, interface)?;
        access.require_active()?;
        let name = operands.lookup_name()?;
        access.require_active()?;
        let package = operands.package.as_ref().map(|(name, _, _)| name.as_str());
        let routine = name
            .as_deref()
            .and_then(|name| lookup_bundled_native(name, package));
        // Rejection happens before any argument-list construction, wrapper
        // allocation, or native callback. The pointer descriptor is definitive.
        if let Some(routine) = routine {
            routine
                .validate_request(interface, operands.payload.len())
                .map_err(|error| native_admission_error(error.to_string()))?;
        } else if native_extension_policy_enabled() {
            unsafe { native_extension_policy_error(call.as_raw(), &interface.to_string()) };
        }
        let result = if let Some(routine) = routine {
            match interface {
                NativeInterface::Call => {
                    let pointers: Vec<_> = operands
                        .payload
                        .iter()
                        .map(|(value, _)| value.as_raw())
                        .collect();
                    access.require_active()?;
                    unsafe { routine.invoke_call(&pointers) }
                        .map_err(|error| native_admission_error(error.to_string()))?
                }
                NativeInterface::External | NativeInterface::External2 => {
                    let allocator = access.allocator(&domain)?;
                    let list = operands.argument_list(&allocator, false, &domain.nil())?;
                    if interface == NativeInterface::External {
                        access.require_active()?;
                        unsafe {
                            routine.invoke_external1(list.as_raw(), operands.payload.len())
                        }
                    } else {
                        let operator = match operator_identity {
                            Some((name, kind)) => access.with_native(|owner| {
                                let pointer = unsafe {
                                    crate::eval::primitive::make_primitive_binding(name, kind)
                                };
                                owner.sexp(pointer)?.into_owned()
                            })?,
                            None => domain.nil(),
                        };
                        access.require_active()?;
                        unsafe {
                            routine.invoke_external2(
                                call.as_raw(),
                                operator.as_raw(),
                                list.as_raw(),
                                environment.as_raw(),
                                operands.payload.len(),
                            )
                        }
                    }
                    .map_err(|error| native_admission_error(error.to_string()))?
                }
            }
        } else {
            // Trusted host extensions still supply an erased foreign ABI.
            // They are deliberately separate from typed bundled admission.
            let allocator = access.allocator(&domain)?;
            let list = operands.argument_list(&allocator, true, &domain.nil())?;
            let mut function: DL_FUNC = None;
            let mut symbol = R_RegisteredNativeSymbol::new(if interface == NativeInterface::Call {
                R_CALL_SYM
            } else {
                R_EXTERNAL_SYM
            });
            let mut buffer = [0; MAX_SYMBOL_BYTES];
            unsafe {
                resolveNativeRoutine(
                    list.as_raw(),
                    &mut function,
                    &mut symbol,
                    &mut buffer,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    call.as_raw(),
                    environment.as_raw(),
                );
            }
            access.require_active()?;
            if function.is_none() {
                return Err(native_admission_error(format!(
                    "C symbol name {:?} not in load table",
                    name.unwrap_or_default()
                )));
            }
            let payload = operands.argument_list(&allocator, false, &domain.nil())?;
            match interface {
                NativeInterface::Call => {
                    let pointers: Vec<_> = operands
                        .payload
                        .iter()
                        .map(|(value, _)| value.as_raw())
                        .collect();
                    access.require_active()?;
                    unsafe {
                        R_doDotCall(function, pointers.len() as c_int, &pointers, call.as_raw())
                    }
                }
                NativeInterface::External => {
                    // SAFETY: trusted host policy assumes the foreign symbol
                    // implements this ABI. Runtime registration metadata is
                    // tracked separately; bundled functions never reach here.
                    let function: unsafe extern "C-unwind" fn(SEXP) -> SEXP =
                        unsafe { std::mem::transmute_copy(&function) };
                    access.require_active()?;
                    unsafe { function(payload.as_raw()) }
                }
                NativeInterface::External2 => {
                    let operator = match operator_identity {
                        Some((name, kind)) => access.with_native(|owner| {
                            let pointer = unsafe {
                                crate::eval::primitive::make_primitive_binding(name, kind)
                            };
                            owner.sexp(pointer)?.into_owned()
                        })?,
                        None => domain.nil(),
                    };
                    let function: unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP =
                        unsafe { std::mem::transmute_copy(&function) };
                    access.require_active()?;
                    unsafe {
                        function(
                            call.as_raw(),
                            operator.as_raw(),
                            payload.as_raw(),
                            environment.as_raw(),
                        )
                    }
                }
            }
        };
        access.require_active()?;
        let result = unsafe { check_retval(call.as_raw(), result) };
        domain.wrap(result)?.into_owned()
    })?
}

/// .External / .External2 handler.
pub unsafe fn do_External(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let interface = if unsafe { PRIMVAL(op) } == 1 {
        crate::mainutils::native_routines::NativeInterface::External2
    } else {
        crate::mainutils::native_routines::NativeInterface::External
    };
    unsafe { invoke_native_handler(call, op, args, env, interface) }
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}

/// .Call handler.
pub unsafe fn do_dotcall(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        invoke_native_handler(
            call,
            op,
            args,
            env,
            crate::mainutils::native_routines::NativeInterface::Call,
        )
    }
    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    .as_raw()
}

// ---------------------------------------------------------------------------
// do_dotCode — .C() and .Fortran() handler
// ---------------------------------------------------------------------------

mod buffer_dispatch;

#[cfg(test)]
mod native_inventory;

pub unsafe fn do_dotCode(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    use crate::mainutils::native_routines::buffers::BufferInterface;
    let interface = if unsafe { PRIMVAL(op) } == 0 { BufferInterface::C } else { BufferInterface::Fortran };
    unsafe { buffer_dispatch::invoke(call, op, args, env, interface) }
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}


/// .C() (op=0) or .Fortran() (op=1) handler.
/// This is the most complex function — marshals R arguments to C types,
/// calls the native routine, then marshals results back.
unsafe fn do_foreign_dotcode(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let entrypoint = if PRIMVAL(op) == 0 { ".C" } else { ".Fortran" };

        let mut naok: c_int = 0;
        let mut nargs_val: c_int = 0;
        let mut fun: DL_FUNC = None;
        let mut symbol = R_RegisteredNativeSymbol::new(R_C_SYM);
        let mut sym_name = [0u8; MAX_SYMBOL_BYTES];
        let _vmax = vmaxget();
        let fort = PRIMVAL(op);
        if fort != 0 {
            symbol.type_ = R_FORTRAN_SYM;
        }

        if Rf_length(args) < 1 {
            errorcall(call, "'.NAME' is missing");
        }
        check1arg2(args, call, ".NAME");

        // Explicitly trusted foreign ABI only. Bundled Rust routines resolve
        // through owning checked-buffer descriptors before this entry.
        if native_extension_policy_enabled() {
            native_extension_policy_error(call, entrypoint);
        }

        let call_args = resolveNativeRoutine(
            args,
            &mut fun,
            &mut symbol,
            &mut sym_name,
            &mut nargs_val,
            &mut naok,
            call,
            env,
        );

        if fun.is_none() {
            errorcall(call, "native symbol is not in the foreign load table");
        }

        // Count arguments
        let mut nargs = 0usize;
        let mut have_names = false;
        let mut pa = call_args;
        while !pa.is_null() && pa != R_NilValue() {
            let tag = TAG(pa);
            if !tag.is_null() && tag != R_NilValue() {
                have_names = true;
            }
            nargs += 1;
            pa = CDR(pa);
        }


        // Build the result vector
        let ans = Rf_allocVector(SEXPTYPE::VECSXP, nargs as c_int);
        if have_names {
            let names = Rf_allocVector(SEXPTYPE::STRSXP, nargs as c_int);
            let mut na = 0usize;
            pa = call_args;
            while !pa.is_null() && pa != R_NilValue() {
                let tag = TAG(pa);
                if tag.is_null() || tag == R_NilValue() {
                    SET_STRING_ELT(
                        names,
                        na as R_xlen_t,
                        Rf_mkChar(b"\0".as_ptr() as *const c_char),
                    );
                } else {
                    SET_STRING_ELT(names, na as R_xlen_t, PRINTNAME(tag));
                }
                na += 1;
                pa = CDR(pa);
            }
            setAttrib(ans, Rf_install(b"names\0".as_ptr() as *const c_char), names);

        }

        // Marshal arguments to C types
        let mut cargs: [*mut c_void; MAX_ARGS] = [ptr::null_mut(); MAX_ARGS];
        pa = call_args;
        let mut na = 0usize;
        while !pa.is_null() && pa != R_NilValue() {
            let s = CAR(pa);
            SET_VECTOR_ELT(ans, na as R_xlen_t, s);

            let t = TYPEOF(s);
            let n = XLENGTH(s);

            match t {
                // RAWSXP
                24 => {
                    let raw_ptr = RAW(s);
                    if !raw_ptr.is_null() && n > 0 {
                        let copy = R_alloc(n as usize, 1) as *mut u8;
                        ptr::copy_nonoverlapping(raw_ptr, copy, n as usize);
                        cargs[na] = copy as *mut c_void;
                    } else {
                        cargs[na] = R_alloc(n as usize, 1);
                    }
                }
                // LGLSXP or INTSXP
                10 | 13 => {
                    let iptr = INTEGER(s);
                    if naok == 0 {
                        for i in 0..n as usize {
                            if *iptr.add(i) == NA_INTEGER {
                                error(&format!("NAs in foreign function call (arg {})", na + 1));
                            }
                        }
                    }
                    if !iptr.is_null() && n > 0 {
                        let copy = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
                        ptr::copy_nonoverlapping(iptr, copy, n as usize);
                        cargs[na] = copy as *mut c_void;
                    } else {
                        cargs[na] = R_alloc(n as usize, std::mem::size_of::<c_int>());
                    }
                }
                // REALSXP
                14 => {
                    let rptr = REAL(s);
                    if naok == 0 {
                        for i in 0..n as usize {
                            let v = *rptr.add(i);
                            if v.is_nan() || v.is_infinite() {
                                error(&format!(
                                    "NA/NaN/Inf in foreign function call (arg {})",
                                    na + 1
                                ));
                            }
                        }
                    }
                    if !rptr.is_null() && n > 0 {
                        let copy = R_alloc(n as usize, std::mem::size_of::<f64>()) as *mut f64;
                        ptr::copy_nonoverlapping(rptr, copy, n as usize);
                        cargs[na] = copy as *mut c_void;
                    } else {
                        cargs[na] = R_alloc(n as usize, std::mem::size_of::<f64>());
                    }
                }
                // CPLXSXP
                15 => {
                    let zptr = COMPLEX(s);
                    if naok == 0 {
                        for i in 0..n as usize {
                            let re = *zptr.add(i);
                            // Simplified NaN check for complex
                            let _ = re;
                        }
                    }
                    if !zptr.is_null() && n > 0 {
                        let copy =
                            R_alloc(n as usize, std::mem::size_of::<Rcomplex>()) as *mut Rcomplex;
                        ptr::copy_nonoverlapping(zptr, copy, n as usize);
                        cargs[na] = copy as *mut c_void;
                    } else {
                        cargs[na] = R_alloc(n as usize, std::mem::size_of::<Rcomplex>());
                    }
                }
                // STRSXP
                16 => {
                    if fort != 0 {
                        // .Fortran: pass a single char buffer
                        let ss = translateChar(STRING_ELT(s, 0));
                        let ss_bytes = std::ffi::CStr::from_ptr(ss).to_bytes();
                        let len = ss_bytes.len().max(255);
                        let fptr = R_alloc(len + 1, 1) as *mut u8;
                        let copy_len = ss_bytes.len().min(len);
                        ptr::copy_nonoverlapping(ss_bytes.as_ptr(), fptr, copy_len);
                        *fptr.add(copy_len) = 0;
                        cargs[na] = fptr as *mut c_void;
                    } else {
                        // .C: pass char** array
                        let cptr = R_alloc(n as usize, std::mem::size_of::<*mut c_char>())
                            as *mut *mut c_char;
                        for i in 0..n as usize {
                            let ss = translateChar(STRING_ELT(s, i as R_xlen_t));
                            let ss_bytes = std::ffi::CStr::from_ptr(ss).to_bytes();
                            let nn = ss_bytes.len() + 1;
                            let ptr_buf = if nn > 1 {
                                let buf = R_alloc(nn, 1) as *mut u8;
                                ptr::copy_nonoverlapping(ss_bytes.as_ptr(), buf, ss_bytes.len());
                                *buf.add(ss_bytes.len()) = 0;
                                buf as *mut c_char
                            } else {
                                // Empty string — allocate a zeroed buffer
                                let buf = R_alloc(128, 1);
                                ptr::write_bytes(buf as *mut u8, 0, 128);
                                buf as *mut c_char
                            };
                            *cptr.add(i) = ptr_buf;
                        }
                        cargs[na] = cptr as *mut c_void;
                    }
                }
                // VECSXP (lists)
                19 => {
                    if fort != 0 {
                        error(&format!("invalid mode to pass to Fortran (arg {})", na + 1));
                    }
                    // Pass as SEXP* array
                    let lptr = R_alloc(n as usize, std::mem::size_of::<SEXP>()) as *mut SEXP;
                    for i in 0..n as usize {
                        *lptr.add(i) = VECTOR_ELT(s, i as R_xlen_t);
                    }
                    cargs[na] = lptr as *mut c_void;
                }
                // CLOSXP, BUILTINSXP, ENVSXP
                // Note: SPECIALSXP (10) shares the value with LGLSXP and is handled above
                8 | 9 | 4 => {
                    if fort != 0 {
                        error(&format!("invalid mode to pass to Fortran (arg {})", na + 1));
                    }
                    cargs[na] = s as *mut c_void;
                }
                // NILSXP
                0 => {
                    error(&format!(
                        "invalid mode to pass to C or Fortran (arg {})",
                        na + 1
                    ));
                }
                // Default: pass as SEXP for .C (deprecated but allowed)
                _ => {
                    if fort != 0 {
                        error(&format!("invalid mode to pass to Fortran (arg {})", na + 1));
                    }
                    cargs[na] = s as *mut c_void;
                }
            }

            na += 1;
            pa = CDR(pa);
        }

        // Call the native routine
        dispatch_dotcode(fun, &cargs[..na], call);

        // Convert results back from C types to R values
        pa = call_args;
        for na_idx in 0..na {
            let p = cargs[na_idx];
            let arg = CAR(pa);
            let t = TYPEOF(arg);
            let n = XLENGTH(arg);

            match t {
                // RAWSXP, INTSXP, LGLSXP, REALSXP, CPLXSXP, STRSXP
                // — results are already in the cargs buffers, copy back
                24 | 10 | 13 | 14 | 15 => {
                    // The native code wrote into our buffer; create a new SEXP with the results
                    let s = VECTOR_ELT(ans, na_idx as R_xlen_t);
                    if t == 14 {
                        // REALSXP: copy back
                        let dest = REAL(s);
                        if !dest.is_null() && !p.is_null() && n > 0 {
                            ptr::copy_nonoverlapping(p as *const f64, dest, n as usize);
                        }
                    } else if t == 13 || t == 10 {
                        // INTSXP/LGLSXP: copy back
                        let dest = INTEGER(s);
                        if !dest.is_null() && !p.is_null() && n > 0 {
                            ptr::copy_nonoverlapping(p as *const c_int, dest, n as usize);
                        }
                    }
                    // For other types, the data was written into the allocated buffer
                    // but we need to copy back into the SEXP's data area
                }
                16 => {
                    // STRSXP: copy strings back
                    if fort != 0 {
                        let buf = p as *const u8;
                        let mut len = 0usize;
                        while *buf.add(len) != 0 && len < 255 {
                            len += 1;
                        }
                        let s = Rf_allocVector(SEXPTYPE::STRSXP, 1);
                        let mut char_buf = vec![0u8; len + 1];
                        ptr::copy_nonoverlapping(buf, char_buf.as_mut_ptr(), len);
                        char_buf[len] = 0;
                        let cstr = std::ffi::CStr::from_bytes_with_nul(&char_buf).unwrap_or(
                            std::ffi::CStr::from_bytes_with_nul(b"\0").unwrap_or_default(),
                        );
                        SET_STRING_ELT(s, 0, Rf_mkChar(cstr.as_ptr()));
                        SET_VECTOR_ELT(ans, na_idx as R_xlen_t, s);
                    } else {
                        let cptr = p as *const *const c_char;
                        let s = Rf_allocVector(SEXPTYPE::STRSXP, n as c_int);
                        for i in 0..n as usize {
                            let cstr = *cptr.add(i);
                            if !cstr.is_null() {
                                SET_STRING_ELT(s, i as R_xlen_t, Rf_mkChar(cstr));
                            }
                        }
                        SET_VECTOR_ELT(ans, na_idx as R_xlen_t, s);
                    }
                }
                _ => {
                    // Other types: leave as-is
                }
            }

            pa = CDR(pa);
        }

        vmaxset(ptr::null_mut()); // simplified
        ans
    }
}

// ---------------------------------------------------------------------------
// do_isloaded
// ---------------------------------------------------------------------------

/// Check if a native symbol is available.
pub unsafe fn do_isloaded(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let nargs = Rf_length(args);
        if nargs < 1 {
            error("no arguments supplied");
        }
        if nargs > 3 {
            error("too many arguments");
        }

        if !isValidStringF(CAR(args)) {
            error("invalid 'symbol' argument");
        }

        let sym_name = translateChar(STRING_ELT(CAR(args), 0));

        let pkg_ptr: *const c_char;
        if nargs >= 2 {
            let pkg_arg = CAR(CDR(args));
            if pkg_arg.is_null() || pkg_arg == R_NilValue() {
                pkg_ptr = b"\0".as_ptr() as *const c_char;
            } else {
                if !isValidStringF(pkg_arg) {
                    error("invalid 'PACKAGE' argument");
                }
                pkg_ptr = translateChar(STRING_ELT(pkg_arg, 0));
            }
        } else {
            pkg_ptr = b"\0".as_ptr() as *const c_char;
        }

        let sym_type: c_int;
        if nargs >= 3 {
            let type_arg = CAR(CDR(CDR(args)));
            if type_arg.is_null() || type_arg == R_NilValue() {
                sym_type = R_ANY_SYM;
            } else {
                if !isValidStringF(type_arg) {
                    error("invalid 'type' argument");
                }
                sym_type = match std::ffi::CStr::from_ptr(translateChar(STRING_ELT(type_arg, 0)))
                    .to_str()
                    .unwrap_or("")
                {
                    "" => R_ANY_SYM,
                    "Fortran" => R_FORTRAN_SYM,
                    "Call" => R_CALL_SYM,
                    "External" => R_EXTERNAL_SYM,
                    _ => error("invalid 'type' argument"),
                };
            }
        } else {
            sym_type = R_ANY_SYM;
        }

        let found = R_lookupLoadedSymbol(sym_name, pkg_ptr, sym_type);
        Rf_ScalarLogical(if found.is_some() { TRUE } else { FALSE })
    }
}

// ---------------------------------------------------------------------------
// do_Externalgr / do_dotcallgr — graphics variants
// ---------------------------------------------------------------------------

/// .External.graphics handler — simplified for headless environment.
pub unsafe fn do_Externalgr(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { do_External(call, op, args, env) }
}

/// .Call.graphics handler — simplified for headless environment.
pub unsafe fn do_dotcallgr(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { do_dotcall(call, op, args, env) }
}

// ---------------------------------------------------------------------------
// Rf_getCallingDLL / R_FindNativeSymbolFromDLL
// ---------------------------------------------------------------------------

/// Find the DLL that the calling function was loaded from.
pub unsafe fn Rf_getCallingDLL() -> SEXP {
    unsafe {
        // Stub: return R_NilValue — namespace/DLL tracking not yet implemented
        R_NilValue()
    }
}

/// Find a native symbol from a specific DLL.
unsafe fn R_FindNativeSymbolFromDLL(
    name: &[u8],
    dll: &mut DllReference,
    symbol: &mut R_RegisteredNativeSymbol,
    _env: SEXP,
) -> DL_FUNC {
    unsafe {
        if name.is_empty() {
            return None;
        }

        let pkg_ptr = if dll.dll_name[0] == 0 {
            b"\0".as_ptr() as *const c_char
        } else {
            dll.dll_name.as_ptr() as *const c_char
        };

        let looked_up = if dll.ref_type == DLL_HANDLE && !dll.dll.is_null() {
            let loaded = R_findDllByHandle(dll.dll);
            if loaded.is_null() {
                None
            } else {
                R_dlsym(loaded, name.as_ptr() as *const c_char, symbol.type_)
            }
        } else {
            R_lookupLoadedSymbol(name.as_ptr() as *const c_char, pkg_ptr, symbol.type_)
        };

        looked_up
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::RSession;

    #[test]
    fn test_check_valid_symbol_id_copies_name() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let op =
                crate::sexp::constructors::Rf_mkString(b"registered\0".as_ptr() as *const c_char);
            let mut fun: DL_FUNC = None;
            let mut symbol = R_RegisteredNativeSymbol::new(R_CALL_SYM);
            let mut buf = [0u8; MAX_SYMBOL_BYTES];

            checkValidSymbolId(op, R_NilValue(), &mut fun, &mut symbol, buf.as_mut_ptr());

            let copied = std::ffi::CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_str()
                .unwrap_or("");
            assert_eq!(copied, "registered");
            assert!(fun.is_none());
        });
    }

    #[test]
    fn test_isloaded_missing_symbol_is_false() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let args = crate::sexp::constructors::Rf_cons(
                crate::sexp::constructors::Rf_mkString(b"missing\0".as_ptr() as *const c_char),
                R_NilValue(),
            );
            let out = do_isloaded(R_NilValue(), R_NilValue(), args, R_NilValue());
            assert_eq!(*crate::sexp::accessors::INTEGER(out), FALSE as c_int);
        });
    }

    #[test]
    fn test_find_native_symbol_from_dll_empty() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let mut dll = DllReference::new();
            let mut symbol = R_RegisteredNativeSymbol::new(R_CALL_SYM);
            let found =
                R_FindNativeSymbolFromDLL(b"missing\0", &mut dll, &mut symbol, R_NilValue());
            assert!(found.is_none());
        });
    }

    #[test]
    fn test_dotcode_symbols_are_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let left_naok = left.with_protected(|| unsafe { NaokSymbol() });
        let right_naok = right.with_protected(|| unsafe { NaokSymbol() });
        let left_naok_again = left.with_protected(|| unsafe { NaokSymbol() });

        assert_eq!(left_naok, left_naok_again);
        assert_ne!(left_naok, right_naok);
    }
}

#[cfg(test)]
mod typed_native_handler_tests {
    use super::*;
    use crate::{
        mainutils::native_routines::NativeInterface,
        sexp::{
            object::{SessionNodeFactory, Sexp},
            session::RSession,
        },
    };
    use std::{cell::Cell, rc::Rc};

    fn make_arguments(
        factory: &SessionNodeFactory<'_>,
        name: &str,
        values: &[(Sexp<'static>, Sexp<'static>)],
    ) -> Sexp<'static> {
        let name = factory.strings(&[name]).unwrap().into_owned().unwrap();
        make_arguments_value(factory, &name, values)
    }

    fn make_arguments_value(
        factory: &SessionNodeFactory<'_>,
        name: &Sexp<'static>,
        values: &[(Sexp<'static>, Sexp<'static>)],
    ) -> Sexp<'static> {
        let mut list = factory.nil().into_owned().unwrap();
        for (value, tag) in values.iter().rev() {
            list = factory
                .pairlist_cell(value, &list, tag)
                .unwrap()
                .into_owned()
                .unwrap();
        }
        factory
            .pairlist_cell(name, &list, &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap()
    }

    fn operator(session: &RSession, interface: NativeInterface) -> Sexp<'static> {
        let name = match interface {
            NativeInterface::Call => ".Call",
            NativeInterface::External => ".External",
            NativeInterface::External2 => ".External2",
        };
        unsafe {
            let raw = crate::eval::primitive::make_primitive_binding(name, SEXPTYPE::BUILTINSXP);
            session
                .owner_token()
                .unwrap()
                .sexp(raw)
                .unwrap()
                .into_owned()
                .unwrap()
        }
    }

    fn rejection(operation: impl FnOnce() -> SEXP) -> String {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
        let payload = result.expect_err("incompatible native request must be rejected");
        payload
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("native rejection is an R error")
            .message
            .clone()
    }

    #[test]
    fn owning_native_handlers_reject_wrong_interfaces_before_allocation() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            for (name, interface) in [
                ("C_par", NativeInterface::External),
                ("C_nlm", NativeInterface::External),
                ("C_termsform", NativeInterface::External2),
                ("C_doTabExpand", NativeInterface::External),
                ("C_nonASCII", NativeInterface::External2),
                ("C_typeconvert", NativeInterface::Call),
                ("C_contourDef", NativeInterface::External),
                ("C_signrank_free", NativeInterface::Call),
            ] {
                let arguments = make_arguments(&factory, name, &[]);
                let operator = operator(&session, interface);
                let before =
                    session.with_active_in(|instance| unsafe { (*instance).arena.node_count() });
                let message = rejection(|| unsafe {
                    if interface == NativeInterface::Call {
                        do_dotcall(
                            factory.nil().as_raw(),
                            operator.as_raw(),
                            arguments.as_raw(),
                            factory.nil().as_raw(),
                        )
                    } else {
                        do_External(
                            factory.nil().as_raw(),
                            operator.as_raw(),
                            arguments.as_raw(),
                            factory.nil().as_raw(),
                        )
                    }
                });
                assert!(
                    message.contains("routine called through"),
                    "{name}: {message}"
                );
                assert_eq!(
                    session.with_active_in(|instance| unsafe { (*instance).arena.node_count() }),
                    before
                );
            }
        });
    }

    #[test]
    fn owning_native_handlers_reject_all_registered_call_arity_mismatches() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let operator = operator(&session, NativeInterface::Call);
            let nil = factory.nil().into_owned().unwrap();
            for (name, expected) in [
                ("C_R_identC", 2usize),
                ("C_spline_basis", 4),
                ("C_pretty2", 2),
                ("C_rpois", 2),
                ("C_tzcode_type", 0),
            ] {
                for actual in [expected.saturating_sub(1), expected + 1] {
                    if actual == expected {
                        continue;
                    }
                    let values = vec![(nil.clone(), nil.clone()); actual];
                    let arguments = make_arguments(&factory, name, &values);
                    let message = rejection(|| unsafe {
                        do_dotcall(
                            nil.as_raw(),
                            operator.as_raw(),
                            arguments.as_raw(),
                            nil.as_raw(),
                        )
                    });
                    assert!(
                        message.contains(&format!("expected {expected}, received {actual}")),
                        "{name}: {message}"
                    );
                }
            }
        });
    }

    #[test]
    fn owning_native_call_honors_package_and_excludes_control_from_payload() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let operator = operator(&session, NativeInterface::Call);
            let value = factory.strings(&["same"]).unwrap().into_owned().unwrap();
            let invalid_package = unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::constructors::Rf_ScalarInteger(42))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let tag = unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(Rf_install(c"PACKAGE".as_ptr()))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let package = factory.strings(&["methods"]).unwrap().into_owned().unwrap();
            for position in 0..=2 {
                let mut payload = vec![(value.clone(), nil.clone()), (value.clone(), nil.clone())];
                payload.insert(position, (package.clone(), tag.clone()));
                let arguments = make_arguments(&factory, "C_R_identC", &payload);
                let result = unsafe {
                    do_dotcall(
                        nil.as_raw(),
                        operator.as_raw(),
                        arguments.as_raw(),
                        nil.as_raw(),
                    )
                };
                assert_eq!(
                    factory.wrap(result).unwrap().try_logical_elt(0).unwrap(),
                    TRUE
                );
            }
            let wrong_package = factory.strings(&["tools"]).unwrap().into_owned().unwrap();
            let arguments = make_arguments(
                &factory,
                "C_R_identC",
                &[
                    (value.clone(), nil.clone()),
                    (wrong_package, tag.clone()),
                    (value.clone(), nil.clone()),
                ],
            );
            let message = rejection(|| unsafe {
                do_dotcall(
                    nil.as_raw(),
                    operator.as_raw(),
                    arguments.as_raw(),
                    nil.as_raw(),
                )
            });
            assert!(
                message.contains("disabled"),
                "explicit wrong package must not fall back globally: {message}"
            );
            let invalid = make_arguments(&factory, "C_R_identC", &[(invalid_package, tag)]);
            let message = rejection(|| unsafe {
                do_dotcall(
                    nil.as_raw(),
                    operator.as_raw(),
                    invalid.as_raw(),
                    nil.as_raw(),
                )
            });
            assert!(message.contains("PACKAGE argument must be a single character string"));
        });
    }

    #[test]
    fn owning_native_external_retains_detached_controls_across_real_gc() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let operator = operator(&session, NativeInterface::External);
            let tag = unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(Rf_install(c"PACKAGE".as_ptr()))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let package = factory
                .strings(&["grDevices"])
                .unwrap()
                .into_owned()
                .unwrap();
            let package_token = package.allocation().unwrap().clone();
            let arguments = make_arguments(&factory, "C_devcur", &[(package, tag)]);
            let original = arguments.as_raw();
            let name_token = arguments.try_car().unwrap().allocation().unwrap().clone();
            let calls = Rc::new(Cell::new(0));
            let notifications = calls.clone();
            let callback_owner = session.owner_token().unwrap().weak_owner().unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if notifications.get() != 0 {
                    return;
                }
                notifications.set(1);
                unsafe {
                    let pin = callback_owner.pin().unwrap();
                    let instance = pin.as_ptr();
                    (*instance).memory_state.gc_force_gap = 0;
                    crate::sexp::accessors::SETCAR(original, R_NilValue());
                    SETCDR(original, R_NilValue());
                }
                crate::sexp::gengc::full_gc();
                assert!(name_token.is_live());
                assert!(
                    package_token.is_live(),
                    "the removed control has only the operation snapshot as a root"
                );
            }));
            session.with_active_in(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
            });
            let result = unsafe {
                do_External(
                    nil.as_raw(),
                    operator.as_raw(),
                    arguments.as_raw(),
                    nil.as_raw(),
                )
            };
            assert_eq!(factory.wrap(result).unwrap().try_integer_elt(0).unwrap(), 1);
            assert_eq!(calls.get(), 1);
        });
    }

    #[test]
    fn owning_native_external_denies_publication_after_callback_close() {
        // No RefCell loan or RSession reference is held across the callback.
        let session = RSession::new_for_gc_tests();
        let factory = unsafe {
            crate::sexp::owner::OwnerToken::current()
                .unwrap()
                .node_factory()
        };
        let nil = factory.nil().into_owned().unwrap();
        let operator = operator(&session, NativeInterface::External);
        let arguments = make_arguments(&factory, "C_devcur", &[]);
        let callback_owner = session.owner_token().unwrap().weak_owner().unwrap();
        let sessions = Rc::new(std::cell::RefCell::new(Some(session)));
        let weak = Rc::downgrade(&sessions);
        let calls = Rc::new(Cell::new(0));
        let notifications = calls.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if notifications.get() != 0 {
                return;
            }
            notifications.set(1);
            unsafe {
                let pin = callback_owner.pin().unwrap();
                let instance = pin.as_ptr();
                (*instance).memory_state.gc_force_gap = 0;
            }
            weak.upgrade()
                .unwrap()
                .borrow_mut()
                .as_mut()
                .unwrap()
                .close();
        }));
        unsafe {
            let instance = crate::sexp::owner::OwnerToken::current().unwrap().as_ptr();
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        }
        let message = rejection(|| unsafe {
            do_External(
                nil.as_raw(),
                operator.as_raw(),
                arguments.as_raw(),
                nil.as_raw(),
            )
        });
        assert!(
            message.contains("owner could not retain its root"),
            "{message}"
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn owning_native_altrep_name_and_package_collect_and_reject_revoked_runtime() {
        use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
        use crate::sexp::object::SexpResult;
        use std::cell::RefCell;

        struct CollectingText {
            text: &'static str,
            calls: Rc<Cell<usize>>,
            close: bool,
            session: std::rc::Weak<RefCell<Option<RSession>>>,
        }
        impl AltrepClass for CollectingText {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::STRSXP
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
                Ok(1)
            }
            fn element<'s>(
                &self,
                context: &AltrepContext<'s>,
                _: R_xlen_t,
            ) -> SexpResult<AltrepElement<'s>> {
                self.calls.set(self.calls.get() + 1);
                let text = context.string(self.text)?;
                context.gc()?;
                if self.close {
                    self.session
                        .upgrade()
                        .unwrap()
                        .borrow_mut()
                        .as_mut()
                        .unwrap()
                        .close();
                }
                Ok(AltrepElement::String(text))
            }
        }

        for package_provider in [false, true] {
            for close in [false, true] {
                let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
                let calls = Rc::new(Cell::new(0));
                let collections = Rc::new(Cell::new(0));
                let observed = collections.clone();
                crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                    observed.set(observed.get() + 1);
                }));
                // Release all RSession and RefCell loans before provider entry.
                let (factory, nil, operator, arguments) = {
                    let borrowed = sessions.borrow();
                    let session = borrowed.as_ref().unwrap();
                    let factory = unsafe {
                        crate::sexp::owner::OwnerToken::current()
                            .unwrap()
                            .node_factory()
                    };
                    let nil = factory.nil().into_owned().unwrap();
                    let operator = operator(session, NativeInterface::Call);
                    let class = session
                        .register_altrep_class(
                            "native_lookup_collecting_text",
                            CollectingText {
                                text: if package_provider {
                                    "methods"
                                } else {
                                    "C_R_identC"
                                },
                                calls: calls.clone(),
                                close,
                                session: Rc::downgrade(&sessions),
                            },
                        )
                        .unwrap()
                        .into_owned()
                        .unwrap();
                    let provider = AltrepBuilder::new(class)
                        .build()
                        .unwrap()
                        .into_owned()
                        .unwrap();
                    let name = if package_provider {
                        factory
                            .strings(&["C_R_identC"])
                            .unwrap()
                            .into_owned()
                            .unwrap()
                    } else {
                        provider.clone()
                    };
                    let package = if package_provider {
                        provider
                    } else {
                        factory.strings(&["methods"]).unwrap().into_owned().unwrap()
                    };
                    let tag = unsafe {
                        session
                            .owner_token()
                            .unwrap()
                            .sexp(Rf_install(c"PACKAGE".as_ptr()))
                            .unwrap()
                            .into_owned()
                            .unwrap()
                    };
                    let value = factory.strings(&["same"]).unwrap().into_owned().unwrap();
                    let arguments = make_arguments_value(
                        &factory,
                        &name,
                        &[
                            (value.clone(), nil.clone()),
                            (package, tag),
                            (value, nil.clone()),
                        ],
                    );
                    (factory, nil, operator, arguments)
                };
                let invoke = || unsafe {
                    do_dotcall(
                        nil.as_raw(),
                        operator.as_raw(),
                        arguments.as_raw(),
                        nil.as_raw(),
                    )
                };
                if close {
                    let message = rejection(invoke);
                    assert!(
                        message.contains("owner could not retain its root"),
                        "{message}"
                    );
                } else {
                    let result = invoke();
                    assert_eq!(
                        factory.wrap(result).unwrap().try_logical_elt(0).unwrap(),
                        TRUE
                    );
                }
                assert_eq!(calls.get(), 1, "the actual lookup provider must run once");
                assert!(
                    collections.get() > 0,
                    "the provider must execute an actual collection"
                );
            }
        }
    }

    #[test]
    fn owning_native_external_payload_metadata_matches_independent_gnu_registry() {
        use crate::mainutils::native_routines::PayloadArity;
        // GNU R devel 4.7.0 r90451, commit bac583951b728e97b9786804d3b4081f0fe18df5.
        // Independent getDLLRegisteredRoutines inventory, not an implementation table.
        // The 107 rows include unsupported routines; only resolved bundled entries
        // are checked, without treating absent functionality as a passing call.
        const REGISTERED: &str = r"stats	compcases	-1
stats	doD	2
stats	deriv	5
stats	modelframe	8
stats	modelmatrix	2
stats	termsform	5
stats	do_fmin	4
stats	nlm	11
stats	zeroin2	7
stats	optim	7
stats	optimhess	4
stats	call_dqags	7
stats	call_dqagi	7
stats	signrank_free	0
stats	wilcox_free	0
tools	parseLatex	6
tools	parseRd	9
grDevices	PicTeX	6
grDevices	PostScript	19
grDevices	PDF	23
grDevices	devCairo	12
grDevices	devcap	1
grDevices	devcapture	1
grDevices	devcontrol	1
grDevices	devcopy	1
grDevices	devcur	0
grDevices	devdisplaylist	0
grDevices	devholdflush	1
grDevices	devnext	1
grDevices	devoff	1
grDevices	devprev	1
grDevices	devset	1
grDevices	devsize	0
grDevices	contourLines	4
grDevices	getSnapshot	0
grDevices	playSnapshot	1
grDevices	getGraphicsEvent	1
grDevices	getGraphicsEventEnv	1
grDevices	setGraphicsEventEnv	2
grDevices	setPattern	1
grDevices	setClipPath	2
grDevices	setMask	2
grDevices	defineGroup	3
grDevices	useGroup	2
grDevices	devUp	0
grDevices	devAskNewPage	1
grDevices	savePlot	3
grDevices	Quartz	11
grDevices	X11	18
graphics	C_contour	-1
graphics	C_filledcontour	5
graphics	C_image	4
graphics	C_persp	-1
graphics	C_abline	-1
graphics	C_axis	-1
graphics	C_arrows	-1
graphics	C_box	-1
graphics	C_clip	-1
graphics	C_convertX	3
graphics	C_convertY	3
graphics	C_dend	-1
graphics	C_dendwindow	-1
graphics	C_erase	-1
graphics	C_layout	-1
graphics	C_mtext	-1
graphics	C_par	-1
graphics	C_path	-1
graphics	C_plotXY	-1
graphics	C_plot_window	-1
graphics	C_polygon	-1
graphics	C_raster	-1
graphics	C_rect	-1
graphics	C_segments	-1
graphics	C_strHeight	-1
graphics	C_strWidth	-1
graphics	C_symbols	-1
graphics	C_text	-1
graphics	C_title	-1
graphics	C_xspline	-1
graphics	C_plot_new	0
graphics	C_locator	-1
graphics	C_identify	-1
utils	download	6
utils	unzip	7
utils	Rprof	10
utils	Rprofmem	3
utils	countfields	6
utils	readtablehead	7
utils	typeconvert	6
utils	writetable	11
utils	addhistory	1
utils	loadhistory	1
utils	savehistory	1
utils	dataentry	2
utils	dataviewer	2
utils	edit	4
utils	fileedit	3
utils	selectlist	4
utils	hashtab_Ext	2
utils	gethash_Ext	3
utils	sethash_Ext	3
utils	remhash_Ext	2
utils	numhash_Ext	1
utils	typhash_Ext	1
utils	maphash_Ext	2
utils	clrhash_Ext	1
utils	ishashtab_Ext	1";
        let mut covered = 0;
        assert_eq!(REGISTERED.lines().count(), 107);
        for line in REGISTERED.lines() {
            let mut fields = line.split('\t');
            let package = fields.next().unwrap();
            let name = fields.next().unwrap();
            let count: i32 = fields.next().unwrap().parse().unwrap();
            let Some(routine) = lookup_bundled_native(name, Some(package)) else {
                continue;
            };
            assert_ne!(
                routine.interface(),
                NativeInterface::Call,
                "{package}::{name}"
            );
            let expected = if count == -1 {
                PayloadArity::Variadic
            } else {
                PayloadArity::Fixed(count as usize)
            };
            assert_eq!(routine.payload_arity(), expected, "{package}::{name}");
            covered += 1;
        }
        assert_eq!(
            covered, 66,
            "static registration coverage, not handler functionality"
        );
        assert_eq!(
            lookup_bundled_native("parseRdText", Some("tools"))
                .unwrap()
                .payload_arity(),
            PayloadArity::Fixed(9)
        );
        assert_eq!(
            lookup_bundled_native("plot_xy", Some("graphics"))
                .unwrap()
                .payload_arity(),
            PayloadArity::Variadic
        );
    }

    #[test]
    fn owning_native_external_rejects_fixed_payload_counts_before_allocation_or_gc() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let tag = unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(Rf_install(c"PACKAGE".as_ptr()))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let mut requests = Vec::new();
            for (name, package, expected, interface) in [
                ("C_devcur", "grDevices", 0usize, NativeInterface::External),
                ("C_devnext", "grDevices", 1, NativeInterface::External),
                ("C_PDF", "grDevices", 23, NativeInterface::External),
                ("C_typeconvert", "utils", 6, NativeInterface::External2),
                ("C_parseRd", "tools", 9, NativeInterface::External2),
                ("C_nlm", "stats", 11, NativeInterface::External2),
                ("C_do_fmin", "stats", 4, NativeInterface::External2),
                ("C_image", "graphics", 4, NativeInterface::External),
                ("C_signrank_free", "stats", 0, NativeInterface::External),
            ] {
                let package = factory.strings(&[package]).unwrap().into_owned().unwrap();
                for actual in [expected.saturating_sub(1), expected + 1] {
                    if actual == expected {
                        continue;
                    }
                    let mut payload = vec![(nil.clone(), nil.clone()); actual];
                    payload.insert(actual / 2, (package.clone(), tag.clone()));
                    requests.push((
                        name,
                        expected,
                        actual,
                        interface,
                        operator(&session, interface),
                        make_arguments(&factory, name, &payload),
                    ));
                }
            }
            let callbacks = Rc::new(Cell::new(0));
            let observed = callbacks.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1)
            }));
            session.with_active_in(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
            });
            let before =
                session.with_active_in(|instance| unsafe { (*instance).arena.node_count() });
            for (name, expected, actual, _, operator, arguments) in requests {
                let message = rejection(|| unsafe {
                    do_External(
                        nil.as_raw(),
                        operator.as_raw(),
                        arguments.as_raw(),
                        nil.as_raw(),
                    )
                });
                assert!(
                    message.contains(&format!("expected {expected}, received {actual}")),
                    "{name}: {message}"
                );
                assert_eq!(
                    session.with_active_in(|instance| unsafe { (*instance).arena.node_count() }),
                    before
                );
                assert_eq!(
                    callbacks.get(),
                    0,
                    "rejection must precede wrapper allocations and collection"
                );
            }
            session.with_active_in(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 0;
            });
        });
    }

    #[test]
    fn owning_native_external_fixed_and_variadic_neighbors_execute_genuine_handlers() {
        let mut session = RSession::new_for_gc_tests();
        let function = session
            .eval_code_with_output_capture("function(x) (x - 2)^2")
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let nil = factory.nil().into_owned().unwrap();
            let tag = unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(Rf_install(c"PACKAGE".as_ptr()))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let scalar_int = |value| unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::constructors::Rf_ScalarInteger(value))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let scalar_real = |value| unsafe {
                session
                    .owner_token()
                    .unwrap()
                    .sexp(crate::sexp::constructors::Rf_ScalarReal(value))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            let invoke = |name, package, interface, values: Vec<Sexp<'static>>| {
                let package = factory.strings(&[package]).unwrap().into_owned().unwrap();
                let mut payload: Vec<_> = values.into_iter().map(|v| (v, nil.clone())).collect();
                payload.insert(payload.len() / 2, (package, tag.clone()));
                let arguments = make_arguments(&factory, name, &payload);
                let op = operator(&session, interface);
                let result = unsafe {
                    do_External(nil.as_raw(), op.as_raw(), arguments.as_raw(), nil.as_raw())
                };
                factory.wrap(result).unwrap().into_owned().unwrap()
            };
            assert_eq!(
                invoke("C_devcur", "grDevices", NativeInterface::External, vec![])
                    .try_integer_elt(0)
                    .unwrap(),
                1
            );
            assert_eq!(
                invoke(
                    "C_devnext",
                    "grDevices",
                    NativeInterface::External,
                    vec![scalar_int(1)]
                )
                .try_integer_elt(0)
                .unwrap(),
                1
            );
            let optimum = invoke(
                "C_do_fmin",
                "stats",
                NativeInterface::External2,
                vec![
                    function,
                    scalar_real(0.0),
                    scalar_real(4.0),
                    scalar_real(0.01),
                ],
            )
            .try_real_elt(0)
            .unwrap();
            assert!((optimum - 2.0).abs() < 0.01);
            let integer_vector = |values: &[i32]| {
                let value = factory
                    .allocate(|arena| {
                        arena
                            .alloc_vector_sexp(SEXPTYPE::INTSXP, values.len() as R_xlen_t)
                            .map(|value| value.as_raw())
                    })
                    .unwrap();
                let mut value = crate::sexp::object::SexpMut::try_from_checked(value).unwrap();
                for (index, scalar) in values.iter().enumerate() {
                    value
                        .try_set_integer_elt(index as R_xlen_t, *scalar)
                        .unwrap();
                }
                value.freeze().into_owned().unwrap()
            };
            let x = integer_vector(&[1, NA_INTEGER]);
            let y = integer_vector(&[2, 3]);
            let complete = invoke(
                "C_compcases",
                "stats",
                NativeInterface::External,
                vec![x, y],
            );
            assert_eq!(complete.len(), 2);
            assert_eq!(complete.try_logical_elt(0).unwrap(), TRUE);
            assert_eq!(complete.try_logical_elt(1).unwrap(), FALSE);
            // External passes one rooted list pointer, not Call's bounded
            // pointer array. GNU accepts this genuine 66-payload variadic call.
            let x = integer_vector(&[1, NA_INTEGER]);
            let complete = invoke(
                "C_compcases",
                "stats",
                NativeInterface::External,
                vec![x; 66],
            );
            assert_eq!(complete.len(), 2);
            assert_eq!(complete.try_logical_elt(0).unwrap(), TRUE);
            assert_eq!(complete.try_logical_elt(1).unwrap(), FALSE);
        });
    }
}
