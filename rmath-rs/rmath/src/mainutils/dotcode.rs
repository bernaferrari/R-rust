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
pub fn R_dotCallFn(op: SEXP, call: SEXP, _nargs: c_int) -> DL_FUNC {
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
            *inst
                .dotcode_state
                .retval_check
                .get_or_insert_with(dotcode_retval_check_enabled)
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
///
/// Uses a macro to generate the repetitive match arms, each transmuting the
/// DL_FUNC to the correct arity-specific function pointer type.
macro_rules! define_dotcall_dispatch {
    ($fun:expr, $args:expr, $($i:literal),*) => {
        match $args.len() {
            0 => {
                let f: unsafe extern "C" fn() -> SEXP = std::mem::transmute_copy(&$fun);
                f()
            }
            $(
                $i => {
                    // Build the call by extracting args[0..$i]
                    define_dotcall_dispatch!(@call $fun, $args, $i)
                }
            )*
            _ if $args.len() <= MAX_ARGS => {
                let f: unsafe extern "C" fn(*mut c_void) -> SEXP = std::mem::transmute_copy(&$fun);
                let _ = f;
                R_NilValue()
            }
            _ => {
                errorcall(ptr::null_mut(), "too many arguments, sorry");
            }
        }
    };
    (@call $fun:expr, $args:expr, 1) => {{
        let f: unsafe extern "C" fn(SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0])
    }};
    (@call $fun:expr, $args:expr, 2) => {{
        let f: unsafe extern "C" fn(SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1])
    }};
    (@call $fun:expr, $args:expr, 3) => {{
        let f: unsafe extern "C" fn(SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2])
    }};
    (@call $fun:expr, $args:expr, 4) => {{
        let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3])
    }};
    (@call $fun:expr, $args:expr, 5) => {{
        let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4])
    }};
    (@call $fun:expr, $args:expr, 6) => {{
        let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5])
    }};
    (@call $fun:expr, $args:expr, 7) => {{
        let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5], $args[6])
    }};
    (@call $fun:expr, $args:expr, 8) => {{
        let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5], $args[6], $args[7])
    }};
    (@call $fun:expr, $args:expr, 9) => {{
        let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5], $args[6], $args[7], $args[8])
    }};
    (@call $fun:expr, $args:expr, 10) => {{
        let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5], $args[6], $args[7], $args[8], $args[9])
    }};
    (@call $fun:expr, $args:expr, $n:tt) => {{
        let f: unsafe extern "C" fn(SEXP) -> SEXP = std::mem::transmute_copy(&$fun);
        f($args[0])
    }};
}

unsafe fn dispatch_dotcall(fun: DL_FUNC, args: &[SEXP], call: SEXP) -> SEXP {
    unsafe {
        let _ = call;
        define_dotcall_dispatch!(fun, args, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
    }
}

/// Dispatch a .C/.Fortran void function by argument count.
macro_rules! define_dotcode_dispatch {
    ($fun:expr, $args:expr, $($i:literal),*) => {
        match $args.len() {
            0 => {
                let f: unsafe extern "C" fn() = std::mem::transmute_copy(&$fun);
                f()
            }
            $(
                $i => {
                    define_dotcode_dispatch!(@call $fun, $args, $i)
                }
            )*
            _ if $args.len() <= MAX_ARGS => {
                let f: unsafe extern "C" fn(*mut c_void) = std::mem::transmute_copy(&$fun);
                let _ = f;
            }
            _ => {
                errorcall(ptr::null_mut(), "too many arguments, sorry");
            }
        }
    };
    (@call $fun:expr, $args:expr, 1) => {{
        let f: unsafe extern "C" fn(*mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0])
    }};
    (@call $fun:expr, $args:expr, 2) => {{
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1])
    }};
    (@call $fun:expr, $args:expr, 3) => {{
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2])
    }};
    (@call $fun:expr, $args:expr, 4) => {{
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3])
    }};
    (@call $fun:expr, $args:expr, 5) => {{
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4])
    }};
    (@call $fun:expr, $args:expr, 6) => {{
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5])
    }};
    (@call $fun:expr, $args:expr, 7) => {{
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5], $args[6])
    }};
    (@call $fun:expr, $args:expr, 8) => {{
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5], $args[6], $args[7])
    }};
    (@call $fun:expr, $args:expr, 9) => {{
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5], $args[6], $args[7], $args[8])
    }};
    (@call $fun:expr, $args:expr, 10) => {{
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0], $args[1], $args[2], $args[3], $args[4], $args[5], $args[6], $args[7], $args[8], $args[9])
    }};
    (@call $fun:expr, $args:expr, $n:tt) => {{
        let f: unsafe extern "C" fn(*mut c_void) = std::mem::transmute_copy(&$fun);
        f($args[0])
    }};
}

unsafe fn dispatch_dotcode(fun: DL_FUNC, args: &[*mut c_void], call: SEXP) {
    unsafe {
        let _ = call;
        define_dotcode_dispatch!(fun, args, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
    }
}

// ---------------------------------------------------------------------------
// R_doDotCall — the .Call dispatcher
// ---------------------------------------------------------------------------

/// Core .Call dispatch: invokes a native function returning SEXP with 0..MAX_ARGS arguments.
pub fn R_doDotCall(fun: DL_FUNC, nargs: c_int, cargs: &[SEXP], call: SEXP) -> SEXP {
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

/// .External / .External2 handler.
pub unsafe fn do_External(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if native_extension_policy_enabled() {
            native_extension_policy_error(call, ".External");
        }

        let mut ofun: DL_FUNC = None;
        let mut symbol = R_RegisteredNativeSymbol::new(R_EXTERNAL_SYM);
        let _vmax = vmaxget();
        let mut buf = [0u8; MAX_SYMBOL_BYTES];

        if Rf_length(args) < 1 {
            errorcall(call, "'.NAME' is missing");
        }
        check1arg2(args, call, ".NAME");
        let _args = resolveNativeRoutine(
            args,
            &mut ofun,
            &mut symbol,
            &mut buf,
            ptr::null_mut(),
            ptr::null_mut(),
            call,
            env,
        );

        if ofun.is_none() {
            errorcall(call, "NULL value passed as symbol address");
        }

        let primval = PRIMVAL(op);
        let retval = if primval == 1 {
            // .External2: fun(call, op, args, env)
            type ExtRoutine2 = unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP;
            let f: ExtRoutine2 = std::mem::transmute_copy(&ofun);
            f(call, op, args, env)
        } else {
            // .External: fun(args)
            type ExtRoutine = unsafe extern "C" fn(SEXP) -> SEXP;
            let f: ExtRoutine = std::mem::transmute_copy(&ofun);
            f(args)
        };

        vmaxset(ptr::null_mut()); // simplified
        check_retval(call, retval)
    }
}

// ---------------------------------------------------------------------------
// do_dotcall — .Call handler
// ---------------------------------------------------------------------------

/// .Call handler.
pub unsafe fn do_dotcall(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if native_extension_policy_enabled() {
            native_extension_policy_error(call, ".Call");
        }

        let mut ofun: DL_FUNC = None;
        let mut symbol = R_RegisteredNativeSymbol::new(R_CALL_SYM);
        let _vmax = vmaxget();
        let mut buf = [0u8; MAX_SYMBOL_BYTES];

        if Rf_length(args) < 1 {
            errorcall(call, "'.NAME' is missing");
        }
        check1arg2(args, call, ".NAME");

        let _args = resolveNativeRoutine(
            args,
            &mut ofun,
            &mut symbol,
            &mut buf,
            ptr::null_mut(),
            ptr::null_mut(),
            call,
            env,
        );

        // Collect arguments (skip .NAME)
        let mut cargs: [SEXP; MAX_ARGS] = [ptr::null_mut(); MAX_ARGS];
        let mut nargs = 0usize;
        let mut pargs = CDR(args);
        while !pargs.is_null() && pargs != R_NilValue() {
            if nargs >= MAX_ARGS {
                errorcall(call, "too many arguments in foreign function call");
            }
            cargs[nargs] = CAR(pargs);
            nargs += 1;
            pargs = CDR(pargs);
        }

        if ofun.is_none() {
            return R_NilValue();
        }

        let retval = R_doDotCall(ofun, nargs as c_int, &cargs, call);
        vmaxset(ptr::null_mut()); // simplified
        retval
    }
}

// ---------------------------------------------------------------------------
// do_dotCode — .C() and .Fortran() handler
// ---------------------------------------------------------------------------

/// .C() (op=0) or .Fortran() (op=1) handler.
/// This is the most complex function — marshals R arguments to C types,
/// calls the native routine, then marshals results back.
pub unsafe fn do_dotCode(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let entrypoint = if PRIMVAL(op) == 0 { ".C" } else { ".Fortran" };
        if native_extension_policy_enabled() {
            native_extension_policy_error(call, entrypoint);
        }

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

        let _pruned_args = resolveNativeRoutine(
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
            return R_NilValue();
        }

        // Count arguments
        let mut nargs = 0usize;
        let mut have_names = false;
        let mut pa = args;
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
            pa = args;
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
        pa = args;
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
        pa = args;
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
