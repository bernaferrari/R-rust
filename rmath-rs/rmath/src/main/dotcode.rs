#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/dotcode.c -- Foreign function interface (.C, .Fortran, .Call, .External).
//!
//! Implements:
//!   - do_dotCode    -- .C() and .Fortran()
//!   - do_dotcall    -- .Call()
//!   - do_External   -- .External()
//!   - do_dotcallgr  -- .Call.graphics()
//!   - do_Externalgr -- .External.graphics()
//!   - do_isloaded   -- is.loaded()
//!   - R_doDotCall   -- internal dispatcher for .Call
//!   - R_dotCallFn   -- resolve .Call entry point

use std::cell::Cell;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::main::coerce::helpers::R_BlankString;
use crate::main::coerce::vector::asLogical;
use crate::main::errors::{Rf_error, Rf_warning, Rf_warningcall1, errorcall};
use crate::main::memory_main::R_ExternalPtrAddr;
use crate::main::memory_main::R_ExternalPtrTag;
use crate::main::sysutils::translateChar;
use crate::sexp::accessors::*;
use crate::sexp::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::memory_ext::{vmaxget, vmaxset};
use crate::sexp::protect::*;
use crate::sexp::symbol::Rf_install;
use crate::unix::dynload::DL_FUNC;

// ---------------------------------------------------------------------------
// Local inline helpers (defined per-file in the R port)
// ---------------------------------------------------------------------------

/// SINGLESXP constant (not defined as SEXPTYPE variant).
const SINGLESXP: c_int = 14;

#[inline(always)]
unsafe fn isValidString(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() || TYPEOF(s) != SEXPTYPE::STRSXP.0 || LENGTH(s) != 1 {
            return 0;
        }
        let x = STRING_ELT(s, 0);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::CHARSXP.0 {
            return 0;
        }
        let cs = CHAR(x);
        if cs.is_null() || *cs == 0 {
            return 0;
        }
        1
    }
}

#[inline(always)]
unsafe fn PRIMVAL(op: SEXP) -> c_int {
    unsafe { (*op).data.primsxp.offset }
}

#[inline(always)]
unsafe fn length(x: SEXP) -> c_int {
    unsafe { LENGTH(x) }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MaxSymbolBytes: usize = 1024;
const MAX_ARGS: usize = 65;
const FILL: u8 = 0xee;
const NG: usize = 64;
const R_PATH_MAX: usize = 1024;

// DLL reference type enum constants
const NOT_DEFINED: c_int = 0;
const FILENAME: c_int = 1;
const DLL_HANDLE: c_int = 2;
const R_OBJECT_TYPE: c_int = 3;

// Native symbol type constants
const R_C_SYM: c_int = 0;
const R_FORTRAN_SYM: c_int = 1;
const R_CALL_SYM: c_int = 2;
const R_EXTERNAL_SYM: c_int = 3;
const R_ANY_SYM: c_int = 4;

// ---------------------------------------------------------------------------
// DllReference
// ---------------------------------------------------------------------------

#[repr(C)]
struct DllReference {
    DLLname: [c_char; R_PATH_MAX],
    dll: *mut c_void,
    obj: SEXP,
    dll_type: c_int,
}

impl DllReference {
    fn new() -> Self {
        DllReference {
            DLLname: [0; R_PATH_MAX],
            dll: ptr::null_mut(),
            obj: ptr::null_mut(),
            dll_type: NOT_DEFINED,
        }
    }
}

// ---------------------------------------------------------------------------
// R_RegisteredNativeSymbol (simplified stub)
// ---------------------------------------------------------------------------

#[repr(C)]
struct R_RegisteredNativeSymbol {
    sym_type: c_int,
    _symbol: *const c_void,
    _dll: *const c_void,
}

impl R_RegisteredNativeSymbol {
    fn new(sym_type: c_int) -> Self {
        R_RegisteredNativeSymbol {
            sym_type,
            _symbol: ptr::null(),
            _dll: ptr::null(),
        }
    }
}

// ---------------------------------------------------------------------------
// Static symbols (set during first call to do_dotCode)
// ---------------------------------------------------------------------------

thread_local! { static NaokSymbol: Cell<SEXP> = Cell::new(ptr::null_mut()); }
thread_local! { static DupSymbol: Cell<SEXP> = Cell::new(ptr::null_mut()); }
thread_local! { static PkgSymbol: Cell<SEXP> = Cell::new(ptr::null_mut()); }
thread_local! { static EncSymbol: Cell<SEXP> = Cell::new(ptr::null_mut()); }
thread_local! { static CSingSymbol: Cell<SEXP> = Cell::new(ptr::null_mut()); }

// ---------------------------------------------------------------------------
// check1arg2
// ---------------------------------------------------------------------------

unsafe fn check1arg2(arg: SEXP, call: SEXP, _formal: &str) {
    unsafe {
        if TAG(arg) == R_NilValue() {
            return;
        }
        errorcall(
            call,
            b"the first argument should not be named\0".as_ptr() as *const c_char,
        );
    }
}

// ---------------------------------------------------------------------------
// isNativeSymbolInfo
// ---------------------------------------------------------------------------

unsafe fn isNativeSymbolInfo(op: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(op);
        if t != SEXPTYPE::VECSXP.0 {
            return false;
        }
        if LENGTH(op) < 2 {
            return false;
        }
        TYPEOF(VECTOR_ELT(op, 1)) == SEXPTYPE::EXTPTRSXP.0
    }
}

// ---------------------------------------------------------------------------
// checkValidSymbolId
// ---------------------------------------------------------------------------

unsafe fn checkValidSymbolId(
    op: SEXP,
    call: SEXP,
    fun: &mut DL_FUNC,
    symbol: &mut R_RegisteredNativeSymbol,
    _buf: *mut c_char,
) {
    unsafe {
        if isValidString(op) != 0 {
            return;
        }

        if TYPEOF(op) == SEXPTYPE::EXTPTRSXP.0 {
            thread_local! { static native_symbol: Cell<SEXP> = Cell::new(ptr::null_mut()); }
            thread_local! { static registered_native_symbol: Cell<SEXP> = Cell::new(ptr::null_mut()); }
            if native_symbol.with(|v| v.get()).is_null() {
                native_symbol
                    .with(|v| v.set(Rf_install(b"native symbol\0".as_ptr() as *const c_char)));
                registered_native_symbol.with(|v| {
                    v.set(Rf_install(
                        b"registered native symbol\0".as_ptr() as *const c_char
                    ))
                });
            }
            if R_ExternalPtrTag(op) == native_symbol.with(|v| v.get()) {
                let addr = R_ExternalPtrAddr(op);
                if !addr.is_null() {
                    // SAFETY: addr is a function pointer obtained from R_ExternalPtrAddr,
                    // stored as *mut c_void by R's external pointer API. Converting back to
                    // a function pointer via transmute_copy is the standard pattern for .C/.Call.
                    *fun = Some(std::mem::transmute_copy(&addr));
                }
            } else if R_ExternalPtrTag(op) == registered_native_symbol.with(|v| v.get()) {
                let tmp = R_ExternalPtrAddr(op) as *const R_RegisteredNativeSymbol;
                if !tmp.is_null() {
                    if symbol.sym_type != R_ANY_SYM && symbol.sym_type != (*tmp).sym_type {
                        errorcall(
                            call,
                            b"NULL value passed as symbol address\0".as_ptr() as *const c_char,
                        );
                    }
                }
            }
            if fun.is_none() {
                errorcall(
                    call,
                    b"NULL value passed as symbol address\0".as_ptr() as *const c_char,
                );
            }
            return;
        } else if isNativeSymbolInfo(op) {
            checkValidSymbolId(VECTOR_ELT(op, 1), call, fun, symbol, _buf);
            return;
        }

        errorcall(
            call,
            b"first argument must be a string (of length 1) or native symbol reference\0".as_ptr()
                as *const c_char,
        );
    }
}

// ---------------------------------------------------------------------------
// checkNativeType
// ---------------------------------------------------------------------------

unsafe fn checkNativeType(target_type: c_int, actual_type: c_int) -> bool {
    if target_type > 0 {
        if target_type == SEXPTYPE::INTSXP.0 || target_type == SEXPTYPE::LGLSXP.0 {
            return actual_type == SEXPTYPE::INTSXP.0 || actual_type == SEXPTYPE::LGLSXP.0;
        }
        return target_type == actual_type;
    }
    true
}

// ---------------------------------------------------------------------------
// comparePrimitiveTypes
// ---------------------------------------------------------------------------

unsafe fn comparePrimitiveTypes(type_: c_int, s: SEXP) -> bool {
    unsafe {
        if type_ == SEXPTYPE::ANYSXP.0 || TYPEOF(s) == type_ {
            return true;
        }
        if type_ == SINGLESXP {
            return asLogical(getAttrib(s, CSingSymbol.with(|v| v.get()))) == 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// naokfind
// ---------------------------------------------------------------------------

unsafe fn naokfind(
    mut args: SEXP,
    len: &mut c_int,
    naok: &mut c_int,
    dll: &mut DllReference,
) -> SEXP {
    unsafe {
        let mut nargs: c_int = 0;
        let mut _naokused: c_int = 0;
        let mut _dupused: c_int = 0;
        let mut _pkgused: c_int = 0;

        *naok = 0;
        *len = 0;

        let mut s = args;
        let mut prev = args;

        while s != R_NilValue() {
            if TAG(s) == NaokSymbol.with(|v| v.get()) {
                *naok = asLogical(CAR(s));
                if _naokused == 1 {
                    Rf_warning(b"'%s' used more than once\0".as_ptr() as *const c_char);
                }
                _naokused += 1;
            } else if TAG(s) == DupSymbol.with(|v| v.get()) {
                if _dupused == 1 {
                    Rf_warning(b"'%s' used more than once\0".as_ptr() as *const c_char);
                }
                _dupused += 1;
            } else if TAG(s) == PkgSymbol.with(|v| v.get()) {
                dll.obj = CAR(s);
                if TYPEOF(CAR(s)) == SEXPTYPE::STRSXP.0 {
                    let p = translateChar(STRING_ELT(CAR(s), 0));
                    let p_len = libc::strlen(p);
                    if p_len > R_PATH_MAX - 1 {
                        Rf_error(b"DLL name is too long\0".as_ptr() as *const c_char);
                    }
                    dll.dll_type = FILENAME;
                    ptr::copy_nonoverlapping(p, dll.DLLname.as_mut_ptr(), p_len);
                    dll.DLLname[p_len] = 0;
                    if _pkgused > 1 {
                        Rf_warning(b"'%s' used more than once\0".as_ptr() as *const c_char);
                    }
                    _pkgused += 1;
                } else {
                    if TYPEOF(CAR(s)) == SEXPTYPE::EXTPTRSXP.0 {
                        dll.dll = R_ExternalPtrAddr(CAR(s));
                        dll.dll_type = DLL_HANDLE;
                    } else if TYPEOF(CAR(s)) == SEXPTYPE::VECSXP.0 {
                        dll.dll_type = R_OBJECT_TYPE;
                        dll.obj = s;
                        let name_sexp = STRING_ELT(VECTOR_ELT(CAR(s), 1), 0);
                        let name = translateChar(name_sexp);
                        let name_len = libc::strlen(name);
                        ptr::copy_nonoverlapping(name, dll.DLLname.as_mut_ptr(), name_len);
                        dll.DLLname[name_len] = 0;
                        dll.dll = R_ExternalPtrAddr(VECTOR_ELT(s, 4));
                    } else {
                        Rf_error(b"incorrect type of PACKAGE argument\0".as_ptr() as *const c_char);
                    }
                }
            } else {
                nargs += 1;
                prev = s;
                s = CDR(s);
                continue;
            }
            if s == args {
                args = s;
                s = CDR(s);
            } else {
                SETCDR(prev, s);
                s = CDR(s);
            }
        }
        *len = nargs;
        args
    }
}

// ---------------------------------------------------------------------------
// setDLLname
// ---------------------------------------------------------------------------

unsafe fn setDLLname(s: SEXP, DLLname: &mut [c_char]) {
    unsafe {
        let ss = CAR(s);
        if TYPEOF(ss) != SEXPTYPE::STRSXP.0 || LENGTH(ss) != 1 {
            Rf_error(
                b"PACKAGE argument must be a single character string\0".as_ptr() as *const c_char,
            );
        }
        let name = translateChar(STRING_ELT(ss, 0));
        let name = if libc::strncmp(name, b"package:\0".as_ptr() as *const c_char, 8) == 0 {
            name.add(8)
        } else {
            name
        };
        let name_len = libc::strlen(name);
        if name_len > R_PATH_MAX - 1 {
            Rf_error(b"PACKAGE argument is too long\0".as_ptr() as *const c_char);
        }
        ptr::copy_nonoverlapping(name, DLLname.as_mut_ptr(), name_len);
        DLLname[name_len] = 0;
    }
}

// ---------------------------------------------------------------------------
// pkgtrim
// ---------------------------------------------------------------------------

unsafe fn pkgtrim(args: SEXP, dll: &mut DllReference) -> SEXP {
    unsafe {
        let mut pkgused: c_int = 0;
        if PkgSymbol.with(|v| v.get()).is_null() {
            PkgSymbol.with(|v| v.set(Rf_install(b"PACKAGE\0".as_ptr() as *const c_char)));
        }

        let mut s = args;
        while s != R_NilValue() {
            let ss = CDR(s);
            if ss == R_NilValue() && TAG(s) == PkgSymbol.with(|v| v.get()) {
                if pkgused == 1 {
                    Rf_warning(b"'%s' used more than once\0".as_ptr() as *const c_char);
                }
                setDLLname(s, &mut dll.DLLname);
                dll.dll_type = FILENAME;
                pkgused += 1;
                return R_NilValue();
            }
            if !ss.is_null() && TAG(ss) == PkgSymbol.with(|v| v.get()) {
                if pkgused == 1 {
                    Rf_warning(b"'%s' used more than once\0".as_ptr() as *const c_char);
                }
                setDLLname(ss, &mut dll.DLLname);
                dll.dll_type = FILENAME;
                pkgused += 1;
                SETCDR(s, CDR(ss));
            }
            s = CDR(s);
        }
        args
    }
}

// ---------------------------------------------------------------------------
// enctrim
// ---------------------------------------------------------------------------

unsafe fn enctrim(args: SEXP) -> SEXP {
    unsafe {
        let mut s = args;
        while s != R_NilValue() {
            let ss = CDR(s);
            if ss == R_NilValue() && TAG(s) == EncSymbol.with(|v| v.get()) {
                Rf_warning(b"ENCODING is defunct and will be ignored\0".as_ptr() as *const c_char);
                return R_NilValue();
            }
            if !ss.is_null() && TAG(ss) == EncSymbol.with(|v| v.get()) {
                Rf_warning(b"ENCODING is defunct and will be ignored\0".as_ptr() as *const c_char);
                SETCDR(s, CDR(ss));
            }
            s = CDR(s);
        }
        args
    }
}

// ---------------------------------------------------------------------------
// check_retval
// ---------------------------------------------------------------------------

unsafe fn check_retval(call: SEXP, mut val: SEXP) -> SEXP {
    unsafe {
        thread_local! { static inited: Cell<bool> = Cell::new(false); }
        thread_local! { static do_check: Cell<bool> = Cell::new(false); }

        if !inited.with(|v| v.get()) {
            inited.with(|v| v.set(true));
            let env_ptr = libc::getenv(b"_R_CHECK_DOTCODE_RETVAL_\0".as_ptr() as *const c_char);
            if !env_ptr.is_null() {
                let p = std::ffi::CStr::from_ptr(env_ptr);
                if !p.to_bytes().is_empty() {
                    let first = p.to_bytes()[0];
                    if first == b'T'
                        || first == b't'
                        || first == b'Y'
                        || first == b'y'
                        || first == b'1'
                    {
                        do_check.with(|v| v.set(true));
                    }
                }
            }
        }

        if do_check.with(|v| v.get()) {
            let val_addr = val as usize;
            if val_addr < 16 {
                errorcall(call, b"WEIRD RETURN VALUE\0".as_ptr() as *const c_char);
            }
        } else if val.is_null() {
            Rf_warningcall1(
                call,
                b"converting NULL pointer to R NULL\0".as_ptr() as *const c_char,
            );
            val = R_NilValue();
        }

        val
    }
}

// ---------------------------------------------------------------------------
// resolveNativeRoutine (stub)
// ---------------------------------------------------------------------------

unsafe fn resolveNativeRoutine(
    mut args: SEXP,
    fun: &mut DL_FUNC,
    symbol: &mut R_RegisteredNativeSymbol,
    _buf: *mut c_char,
    nargs_ptr: *mut c_int,
    naok_ptr: *mut c_int,
    call: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        let op = CAR(args);

        checkValidSymbolId(op, call, fun, symbol, _buf);

        if symbol.sym_type == R_C_SYM || symbol.sym_type == R_FORTRAN_SYM {
            let mut nargs: c_int = 0;
            let mut naok: c_int = 0;
            let mut dll = DllReference::new();
            args = naokfind(CDR(args), &mut nargs, &mut naok, &mut dll);
            if naok == NA_INTEGER {
                errorcall(call, b"invalid 'naok' value\0".as_ptr() as *const c_char);
            }
            if nargs > MAX_ARGS as c_int {
                errorcall(
                    call,
                    b"too many arguments in foreign function call\0".as_ptr() as *const c_char,
                );
            }
            if !nargs_ptr.is_null() {
                *nargs_ptr = nargs;
            }
            if !naok_ptr.is_null() {
                *naok_ptr = naok;
            }
        } else {
            let mut dll = DllReference::new();
            args = pkgtrim(args, &mut dll);
        }

        if fun.is_some() {
            return args;
        }

        // TODO: full implementation with namespace lookup, R_FindSymbol, etc.
        errorcall(
            call,
            b"C/Fortran symbol name not in load table\0".as_ptr() as *const c_char,
        );
        unreachable!()
    }
}

// ===========================================================================
// do_isloaded
// ===========================================================================

// no_mangle removed (duplicate)
pub unsafe fn do_isloaded(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let nargs = length(args);
        if nargs < 1 {
            Rf_error(b"no arguments supplied\0".as_ptr() as *const c_char);
        }
        if nargs > 3 {
            Rf_error(b"too many arguments\0".as_ptr() as *const c_char);
        }

        if isValidString(CAR(args)) == 0 {
            Rf_error(b"invalid 'symbol' argument\0".as_ptr() as *const c_char);
        }

        // Simplified: always return FALSE since we can't actually check DLL symbols.
        Rf_ScalarLogical(0)
    }
}

// ===========================================================================
// R_dotCallFn
// ===========================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_dotCallFn(op: SEXP, call: SEXP, _nargs: c_int) -> DL_FUNC {
    unsafe {
        let mut symbol = R_RegisteredNativeSymbol::new(R_CALL_SYM);
        let mut fun: DL_FUNC = None;
        checkValidSymbolId(op, call, &mut fun, &mut symbol, ptr::null_mut());
        fun
    }
}

// ===========================================================================
// R_doDotCall -- dispatch .Call with 0..65 arguments
// ===========================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_doDotCall(
    fun: DL_FUNC,
    nargs: c_int,
    cargs: *mut SEXP,
    call: SEXP,
) -> SEXP {
    unsafe {
        let fun = match fun {
            Some(f) => f,
            None => return check_retval(call, R_NilValue()),
        };
        if cargs.is_null() {
            return check_retval(call, R_NilValue());
        }

        // SAFETY: `fun` is a function pointer obtained from R_ExternalPtrAddr (native symbol)
        // or from .Call/.External registration. Transmuting to the correct arity signature
        // is safe because the caller (R's .Call/.External interface) guarantees the arity
        // matches nargs. All signatures are `unsafe extern "C" fn(...) -> SEXP`.
        let retval: SEXP = match nargs {
            0 => {
                let f: unsafe extern "C" fn() -> SEXP = std::mem::transmute(fun);
                f()
            }
            1 => {
                let f: unsafe extern "C" fn(SEXP) -> SEXP = std::mem::transmute(fun);
                f(*cargs.add(0))
            }
            2 => {
                let f: unsafe extern "C" fn(SEXP, SEXP) -> SEXP = std::mem::transmute(fun);
                f(*cargs.add(0), *cargs.add(1))
            }
            3 => {
                let f: unsafe extern "C" fn(SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute(fun);
                f(*cargs.add(0), *cargs.add(1), *cargs.add(2))
            }
            4 => {
                let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute(fun);
                f(*cargs.add(0), *cargs.add(1), *cargs.add(2), *cargs.add(3))
            }
            _ => call_dotcall_generic(fun, nargs, cargs),
        };

        check_retval(call, retval)
    }
}

/// Generic dispatcher for 5..65 arguments.
unsafe fn call_dotcall_generic(
    fun: unsafe extern "C" fn(),
    nargs: c_int,
    cargs: *mut SEXP,
) -> SEXP {
    unsafe {
        match nargs {
            5 => {
                let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                )
            }
            6 => {
                let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                )
            }
            7 => {
                let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP, SEXP, SEXP, SEXP) -> SEXP =
                    std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                    *cargs.add(6),
                )
            }
            8 => {
                let f: unsafe extern "C" fn(
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                ) -> SEXP = std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                    *cargs.add(6),
                    *cargs.add(7),
                )
            }
            9 => {
                let f: unsafe extern "C" fn(
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                ) -> SEXP = std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                    *cargs.add(6),
                    *cargs.add(7),
                    *cargs.add(8),
                )
            }
            10 => {
                let f: unsafe extern "C" fn(
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
                ) -> SEXP = std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                    *cargs.add(6),
                    *cargs.add(7),
                    *cargs.add(8),
                    *cargs.add(9),
                )
            }
            11 => {
                let f: unsafe extern "C" fn(
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
                    SEXP,
                ) -> SEXP = std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                    *cargs.add(6),
                    *cargs.add(7),
                    *cargs.add(8),
                    *cargs.add(9),
                    *cargs.add(10),
                )
            }
            12 => {
                let f: unsafe extern "C" fn(
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
                    SEXP,
                    SEXP,
                ) -> SEXP = std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                    *cargs.add(6),
                    *cargs.add(7),
                    *cargs.add(8),
                    *cargs.add(9),
                    *cargs.add(10),
                    *cargs.add(11),
                )
            }
            13 => {
                let f: unsafe extern "C" fn(
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
                    SEXP,
                    SEXP,
                    SEXP,
                ) -> SEXP = std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                    *cargs.add(6),
                    *cargs.add(7),
                    *cargs.add(8),
                    *cargs.add(9),
                    *cargs.add(10),
                    *cargs.add(11),
                    *cargs.add(12),
                )
            }
            14 => {
                let f: unsafe extern "C" fn(
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
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                ) -> SEXP = std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                    *cargs.add(6),
                    *cargs.add(7),
                    *cargs.add(8),
                    *cargs.add(9),
                    *cargs.add(10),
                    *cargs.add(11),
                    *cargs.add(12),
                    *cargs.add(13),
                )
            }
            15 => {
                let f: unsafe extern "C" fn(
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
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                    SEXP,
                ) -> SEXP = std::mem::transmute(fun);
                f(
                    *cargs.add(0),
                    *cargs.add(1),
                    *cargs.add(2),
                    *cargs.add(3),
                    *cargs.add(4),
                    *cargs.add(5),
                    *cargs.add(6),
                    *cargs.add(7),
                    *cargs.add(8),
                    *cargs.add(9),
                    *cargs.add(10),
                    *cargs.add(11),
                    *cargs.add(12),
                    *cargs.add(13),
                    *cargs.add(14),
                )
            }
            _ => {
                if nargs < 16 || nargs > MAX_ARGS as c_int {
                    Rf_error(b"too many arguments, sorry\0".as_ptr() as *const c_char);
                    unreachable!()
                }
                type VarCallFn = unsafe extern "C" fn(*mut SEXP) -> SEXP;
                let f: VarCallFn = std::mem::transmute(fun);
                f(cargs)
            }
        }
    }
}

// ===========================================================================
// do_dotcall -- .Call(name, ...)
// ===========================================================================

pub unsafe fn do_dotcall(call: SEXP, _op: SEXP, mut args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ofun: DL_FUNC = None;
        let mut symbol = R_RegisteredNativeSymbol::new(R_CALL_SYM);

        let mut cargs: [SEXP; MAX_ARGS] = [ptr::null_mut(); MAX_ARGS];
        let mut nargs: c_int = 0;
        let vmax = vmaxget();
        let mut buf: [c_char; MaxSymbolBytes] = [0; MaxSymbolBytes];

        if length(args) < 1 {
            errorcall(call, b"'.NAME' is missing\0".as_ptr() as *const c_char);
            unreachable!()
        }
        check1arg2(args, call, ".NAME");

        args = resolveNativeRoutine(
            args,
            &mut ofun,
            &mut symbol,
            buf.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            call,
            env,
        );
        args = CDR(args);

        let mut pargs = args;
        while pargs != R_NilValue() {
            if nargs == MAX_ARGS as c_int {
                errorcall(
                    call,
                    b"too many arguments in foreign function call\0".as_ptr() as *const c_char,
                );
                unreachable!()
            }
            cargs[nargs as usize] = CAR(pargs);
            nargs += 1;
            pargs = CDR(pargs);
        }

        let ofun = match ofun {
            Some(f) => f,
            None => {
                vmaxset(vmax);
                return R_NilValue();
            }
        };

        let retval = R_doDotCall(Some(ofun), nargs, cargs.as_mut_ptr(), call);
        vmaxset(vmax);
        retval
    }
}

// ===========================================================================
// do_External -- .External(name, ...)
// ===========================================================================

pub unsafe fn do_External(call: SEXP, op: SEXP, mut args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ofun: DL_FUNC = None;
        let mut symbol = R_RegisteredNativeSymbol::new(R_EXTERNAL_SYM);
        let vmax = vmaxget();
        let mut buf: [c_char; MaxSymbolBytes] = [0; MaxSymbolBytes];

        if length(args) < 1 {
            errorcall(call, b"'.NAME' is missing\0".as_ptr() as *const c_char);
            unreachable!()
        }
        check1arg2(args, call, ".NAME");

        args = resolveNativeRoutine(
            args,
            &mut ofun,
            &mut symbol,
            buf.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            call,
            env,
        );

        let ofun = match ofun {
            Some(f) => f,
            None => {
                vmaxset(vmax);
                return R_NilValue();
            }
        };

        let retval = if PRIMVAL(op) == 1 {
            // SAFETY: ofun is a registered .External function pointer; PRIMVAL==1 means
            // it expects (call, op, args, env) signature.
            let f: unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP = std::mem::transmute(ofun);
            f(call, op, args, env)
        } else {
            // SAFETY: ofun is a registered .External function pointer; PRIMVAL!=1 means
            // it expects the simple (args) signature.
            let f: unsafe extern "C" fn(SEXP) -> SEXP = std::mem::transmute(ofun);
            f(args)
        };

        vmaxset(vmax);
        check_retval(call, retval)
    }
}

// ===========================================================================
// do_Externalgr -- .External.graphics(name, ...)
// ===========================================================================

pub unsafe fn do_Externalgr(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // TODO: Full implementation requires GEcurrentDevice(), recordGraphics, etc.
        do_External(call, op, args, env)
    }
}

// ===========================================================================
// do_dotcallgr -- .Call.graphics(name, ...)
// ===========================================================================

pub unsafe fn do_dotcallgr(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // TODO: Full implementation requires GEcurrentDevice(), recordGraphics, etc.
        do_dotcall(call, op, args, env)
    }
}

// ===========================================================================
// do_dotCode -- .C() and .Fortran()
// ===========================================================================

pub unsafe fn do_dotCode(call: SEXP, op: SEXP, mut args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut fun: DL_FUNC = None;
        let mut naok: c_int = 0;
        let mut nargs: c_int = 0;
        let Fort = PRIMVAL(op);
        let _copy = false; // R_CBoundsCheck -- stub
        let mut symbol = if Fort != 0 {
            R_RegisteredNativeSymbol::new(R_FORTRAN_SYM)
        } else {
            R_RegisteredNativeSymbol::new(R_C_SYM)
        };

        let vmax = vmaxget();
        let mut symName: [c_char; MaxSymbolBytes] = [0; MaxSymbolBytes];

        // Initialize static symbols
        if NaokSymbol.with(|v| v.get()).is_null() {
            NaokSymbol.with(|v| v.set(Rf_install(b"NAOK\0".as_ptr() as *const c_char)));
        }
        if DupSymbol.with(|v| v.get()).is_null() {
            DupSymbol.with(|v| v.set(Rf_install(b"DUP\0".as_ptr() as *const c_char)));
        }
        if PkgSymbol.with(|v| v.get()).is_null() {
            PkgSymbol.with(|v| v.set(Rf_install(b"PACKAGE\0".as_ptr() as *const c_char)));
        }
        if EncSymbol.with(|v| v.get()).is_null() {
            EncSymbol.with(|v| v.set(Rf_install(b"ENCODING\0".as_ptr() as *const c_char)));
        }
        if CSingSymbol.with(|v| v.get()).is_null() {
            CSingSymbol.with(|v| v.set(Rf_install(b"Csingle\0".as_ptr() as *const c_char)));
        }

        if length(args) < 1 {
            errorcall(call, b"'.NAME' is missing\0".as_ptr() as *const c_char);
            unreachable!()
        }
        check1arg2(args, call, ".NAME");

        args = enctrim(args);
        args = resolveNativeRoutine(
            args,
            &mut fun,
            &mut symbol,
            symName.as_mut_ptr(),
            &mut nargs,
            &mut naok,
            call,
            env,
        );

        // Construct the return value
        nargs = 0;
        let mut havenames = false;
        let mut pa = args;
        while pa != R_NilValue() {
            if TAG(pa) != R_NilValue() {
                havenames = true;
            }
            nargs += 1;
            pa = CDR(pa);
        }

        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, nargs));
        if havenames {
            let names = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, nargs));
            let mut na: c_int = 0;
            pa = args;
            while pa != R_NilValue() {
                if TAG(pa) == R_NilValue() {
                    SET_STRING_ELT(names, na as R_xlen_t, R_BlankString());
                } else {
                    SET_STRING_ELT(names, na as R_xlen_t, PRINTNAME(TAG(pa)));
                }
                na += 1;
                pa = CDR(pa);
            }
            setAttrib(ans, R_NamesSymbol(), names);
            Rf_unprotect(1);
        }

        // Convert arguments and call the function
        // Stub: set ans elements to original args
        let mut na: c_int = 0;
        pa = args;
        while pa != R_NilValue() {
            SET_VECTOR_ELT(ans, na as R_xlen_t, CAR(pa));
            na += 1;
            pa = CDR(pa);
        }

        Rf_unprotect(1);
        vmaxset(vmax);
        ans
    }
}
