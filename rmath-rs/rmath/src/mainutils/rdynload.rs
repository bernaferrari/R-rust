#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/Rdynload.c — dynamic library loading and native symbol registration.
//!
//! Manages the loaded-DLL table, symbol registration for .C/.Call/.Fortran/.External,
//! symbol lookup (registered first, then dynamic), and the R-level interfaces
//! `do_dynload`, `do_dynunload`, `getSymbolInfo`, `getLoadedDLLs`, etc.

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::Ordering;

use crate::mainutils::memory_main::{R_ExternalPtrAddr, R_MakeExternalPtr};
use crate::mainutils::relop::checkArity;
use crate::sexp::accessors::{
    CAR, CDR, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TYPEOF, translateChar,
};
use crate::sexp::attrib_core::{R_ClassSymbol, R_NamesSymbol, setAttrib};
use crate::sexp::constructors::{
    Rf_ScalarLogical, Rf_allocVector, Rf_isString, Rf_length, Rf_mkChar, Rf_mkString,
};
use crate::sexp::envir::{R_NewHashedEnv, R_findVarInFrame, defineVar};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::symbol::Rf_install;
use crate::unix::dynload::{DL_FUNC, InitFunctionHashing};

// ---------------------------------------------------------------------------
// Local error helper
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

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const R_PATH_MAX: usize = 4096;
const MAX_NUM_DLLS_DEFAULT: usize = 614;
const MIN_NUM_DLLS: usize = 100;
const MAX_NUM_DLLS_LIMIT: usize = 1000;

const SHLIB_EXT: &[u8] = b".so\0";
const FILESEP: &[u8] = b"/\0";

// ---------------------------------------------------------------------------
// Registered symbol tables per DLL
// ---------------------------------------------------------------------------

/// A single registered .C symbol entry.
#[repr(C)]
struct DotCSymbol {
    name: *mut c_char,
    fun: DL_FUNC,
    num_args: c_int,
    types: *mut c_int,
}

/// A single registered .Call symbol entry.
#[repr(C)]
struct DotCallSymbol {
    name: *mut c_char,
    fun: DL_FUNC,
    num_args: c_int,
}

/// A single registered .Fortran symbol entry.
#[repr(C)]
struct DotFortranSymbol {
    name: *mut c_char,
    fun: DL_FUNC,
    num_args: c_int,
    types: *mut c_int,
}

/// A single registered .External symbol entry.
#[repr(C)]
struct DotExternalSymbol {
    name: *mut c_char,
    fun: DL_FUNC,
    num_args: c_int,
}

// ---------------------------------------------------------------------------
// DllInfo — tracks a loaded shared library and its registered symbols
// ---------------------------------------------------------------------------

/// Full DllInfo with all fields from the C implementation.
/// This replaces the opaque stub in registration.rs.
#[repr(C)]
pub struct DllInfo {
    pub path: *mut c_char,
    pub name: *mut c_char,
    pub handle: *mut c_void,
    pub use_dynamic_lookup: bool,
    pub force_symbols: bool,

    // Registered symbol tables
    num_c_symbols: c_int,
    c_symbols: *mut DotCSymbol,

    num_call_symbols: c_int,
    call_symbols: *mut DotCallSymbol,

    num_fortran_symbols: c_int,
    fortran_symbols: *mut DotFortranSymbol,

    num_external_symbols: c_int,
    external_symbols: *mut DotExternalSymbol,
}

impl DllInfo {
    fn new(path: *mut c_char, name: *mut c_char, handle: *mut c_void) -> Self {
        DllInfo {
            path,
            name,
            handle,
            use_dynamic_lookup: true,
            force_symbols: false,
            num_c_symbols: 0,
            c_symbols: ptr::null_mut(),
            num_call_symbols: 0,
            call_symbols: ptr::null_mut(),
            num_fortran_symbols: 0,
            fortran_symbols: ptr::null_mut(),
            num_external_symbols: 0,
            external_symbols: ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// Global DLL table
// ---------------------------------------------------------------------------

thread_local! {
    static LOADED_DLL: RefCell<Vec<*mut DllInfo>> = RefCell::new(Vec::new());
    static DLL_INFO_EPTRS: RefCell<SEXP> = RefCell::new(ptr::null_mut());
    static SYMBOL_EPTRS: RefCell<SEXP> = RefCell::new(ptr::null_mut());
    static DLL_ERROR: RefCell<[u8; 4000]> = RefCell::new([0u8; 4000]);
    static MAX_NUM_DLLS: RefCell<usize> = RefCell::new(0);
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
// C/Fortran/Call/External method definitions (input to registration)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct R_CMethodDef {
    pub name: *const c_char,
    pub fun: DL_FUNC,
    pub num_args: c_int,
    pub types: *const c_int,
}

#[repr(C)]
pub struct R_CallMethodDef {
    pub name: *const c_char,
    pub fun: DL_FUNC,
    pub num_args: c_int,
}

#[repr(C)]
pub struct R_FortranMethodDef {
    pub name: *const c_char,
    pub fun: DL_FUNC,
    pub num_args: c_int,
    pub types: *const c_int,
}

#[repr(C)]
pub struct R_ExternalMethodDef {
    pub name: *const c_char,
    pub fun: DL_FUNC,
    pub num_args: c_int,
}

// ---------------------------------------------------------------------------
// init / InitDynload
// ---------------------------------------------------------------------------

/// Initialize the DLL table. Called once at R startup.
fn init_loaded_dll() {
    LOADED_DLL.with(|v| v.borrow_mut().clear());
    DLL_INFO_EPTRS.with(|v| *v.borrow_mut() = ptr::null_mut());
    SYMBOL_EPTRS.with(|v| *v.borrow_mut() = ptr::null_mut());
    MAX_NUM_DLLS.with(|v| *v.borrow_mut() = MAX_NUM_DLLS_DEFAULT);
}

/// Called at R startup to initialize dynamic loading.
pub unsafe fn InitDynload() {
    unsafe {
        init_loaded_dll();

        let base_path = b"base\0".as_ptr() as *const c_char;
        let _idx = add_dll(base_path, base_path, ptr::null_mut());

        LOADED_DLL.with(|v| {
            let dll = v.borrow_mut();
            if !dll.is_empty() {
                crate::mainutils::registration::R_init_base(
                    dll[0] as *mut crate::mainutils::registration::DllInfo,
                );
            }
        });

        InitFunctionHashing();
    }
}

// ---------------------------------------------------------------------------
// addDLL — low-level insertion into the DLL table
// ---------------------------------------------------------------------------

/// Add a DLL entry. Returns its index, or -1 on failure.
unsafe fn add_dll(dpath: *const c_char, dllname: *const c_char, handle: *mut c_void) -> isize {
    unsafe {
        // Duplicate strings
        let path_copy = strdup(dpath);
        if path_copy.is_null() {
            return -1;
        }
        let name_copy = strdup(dllname);
        if name_copy.is_null() {
            libc_free(path_copy as *mut c_void);
            return -1;
        }

        let info = Box::into_raw(Box::new(DllInfo::new(path_copy, name_copy, handle)));

        LOADED_DLL.with(|v| {
            let mut dlls = v.borrow_mut();
            let idx = dlls.len() as isize;
            dlls.push(info);
            idx
        })
    }
}

// ---------------------------------------------------------------------------
// strdup / free helpers
// ---------------------------------------------------------------------------

unsafe fn strdup(s: *const c_char) -> *mut c_char {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }
        let cstr = std::ffi::CStr::from_ptr(s);
        let bytes = cstr.to_bytes_with_nul();
        let len = bytes.len();
        let buf = libc_malloc(len) as *mut c_char;
        if buf.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, len);
        buf
    }
}

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn strlen(s: *const c_char) -> usize;
    fn strcmp(a: *const c_char, b: *const c_char) -> c_int;
    fn strncmp(a: *const c_char, b: *const c_char, n: usize) -> c_int;
    fn strcpy(dst: *mut c_char, src: *const c_char) -> *mut c_char;
    fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
    fn memset(dst: *mut c_void, c: c_int, n: usize) -> *mut c_void;
    fn snprintf(buf: *mut c_char, size: usize, fmt: *const c_char, ...) -> c_int;
}

unsafe fn libc_malloc(size: usize) -> *mut c_void {
    unsafe { malloc(size) }
}

unsafe fn libc_free(ptr: *mut c_void) {
    unsafe { free(ptr) }
}

unsafe fn libc_calloc(n: usize, size: usize) -> *mut c_void {
    unsafe { calloc(n, size) }
}

// ---------------------------------------------------------------------------
// R_getDllInfo / R_getDllIndex
// ---------------------------------------------------------------------------

/// Find a DllInfo by its path.
pub unsafe fn R_getDllInfo(path: *const c_char) -> *mut DllInfo {
    unsafe {
        LOADED_DLL.with(|v| {
            let dlls = v.borrow();
            for &dll in dlls.iter() {
                if !dll.is_null() && strcmp((*dll).path, path) == 0 {
                    return dll;
                }
            }
            ptr::null_mut()
        })
    }
}

/// Find the index of a DllInfo in the table.
unsafe fn R_getDllIndex(info: *mut DllInfo) -> isize {
    LOADED_DLL.with(|v| {
        let dlls = v.borrow();
        for (i, &dll) in dlls.iter().enumerate() {
            if dll == info {
                return i as isize;
            }
        }
        -1
    })
}

// ---------------------------------------------------------------------------
// R_getEmbeddingDllInfo
// ---------------------------------------------------------------------------

pub unsafe fn R_getEmbeddingDllInfo() -> *mut DllInfo {
    unsafe {
        let embed_path = b"(embedding)\0".as_ptr() as *const c_char;
        let mut dll = R_getDllInfo(embed_path);
        if dll.is_null() {
            let _idx = add_dll(embed_path, embed_path, ptr::null_mut());
            dll = R_getDllInfo(embed_path);
            if !dll.is_null() {
                (*dll).use_dynamic_lookup = false;
            }
        }
        dll
    }
}

// ---------------------------------------------------------------------------
// R_useDynamicSymbols / R_forceSymbols
// ---------------------------------------------------------------------------

pub unsafe fn R_useDynamicSymbols(info: *mut DllInfo, value: bool) -> bool {
    unsafe {
        if info.is_null() {
            return false;
        }
        let old = (*info).use_dynamic_lookup;
        (*info).use_dynamic_lookup = value;
        old
    }
}

pub unsafe fn R_forceSymbols(info: *mut DllInfo, value: bool) -> bool {
    unsafe {
        if info.is_null() {
            return false;
        }
        let old = (*info).force_symbols;
        (*info).force_symbols = value;
        old
    }
}

// ---------------------------------------------------------------------------
// Symbol registration helpers
// ---------------------------------------------------------------------------

unsafe fn set_primitive_arg_types(croutine: &R_FortranMethodDef, sym: *mut DotFortranSymbol) {
    unsafe {
        let n = croutine.num_args;
        if n <= 0 {
            return;
        }
        let n_usize = n as usize;
        let types = libc_malloc(n_usize * std::mem::size_of::<c_int>()) as *mut c_int;
        if types.is_null() {
            error("allocation failure in set_primitive_arg_types");
        }
        if !croutine.types.is_null() {
            memcpy(
                types as *mut c_void,
                croutine.types as *const c_void,
                n_usize * std::mem::size_of::<c_int>(),
            );
        }
        (*sym).types = types;
    }
}

unsafe fn add_c_routine(info: *mut DllInfo, croutine: &R_CMethodDef, sym: *mut DotCSymbol) {
    unsafe {
        (*sym).name = strdup(croutine.name);
        (*sym).fun = croutine.fun;
        (*sym).num_args = if croutine.num_args > -1 {
            croutine.num_args
        } else {
            -1
        };
        if !croutine.types.is_null() {
            // Need to create a FortranMethodDef-like wrapper for the type copy.
            // For .C routines, types are the same as for Fortran.
            let n = croutine.num_args;
            if n > 0 {
                let n_usize = n as usize;
                let types = libc_malloc(n_usize * std::mem::size_of::<c_int>()) as *mut c_int;
                if types.is_null() {
                    error("allocation failure in add_c_routine");
                }
                memcpy(
                    types as *mut c_void,
                    croutine.types as *const c_void,
                    n_usize * std::mem::size_of::<c_int>(),
                );
                (*sym).types = types;
            }
        }
    }
}

unsafe fn add_call_routine(
    info: *mut DllInfo,
    croutine: &R_CallMethodDef,
    sym: *mut DotCallSymbol,
) {
    unsafe {
        let _ = info;
        (*sym).name = strdup(croutine.name);
        (*sym).fun = croutine.fun;
        (*sym).num_args = if croutine.num_args > -1 {
            croutine.num_args
        } else {
            -1
        };
    }
}

unsafe fn add_fortran_routine(
    info: *mut DllInfo,
    croutine: &R_FortranMethodDef,
    sym: *mut DotFortranSymbol,
) {
    unsafe {
        let _ = info;
        (*sym).name = strdup(croutine.name);
        (*sym).fun = croutine.fun;
        (*sym).num_args = if croutine.num_args > -1 {
            croutine.num_args
        } else {
            -1
        };
        if !croutine.types.is_null() {
            set_primitive_arg_types(croutine, sym);
        }
    }
}

unsafe fn add_external_routine(
    info: *mut DllInfo,
    croutine: &R_ExternalMethodDef,
    sym: *mut DotExternalSymbol,
) {
    unsafe {
        let _ = info;
        (*sym).name = strdup(croutine.name);
        (*sym).fun = croutine.fun;
        (*sym).num_args = if croutine.num_args > -1 {
            croutine.num_args
        } else {
            -1
        };
    }
}

// ---------------------------------------------------------------------------
// R_registerRoutines — the main registration entry point
// ---------------------------------------------------------------------------

pub unsafe fn R_registerRoutines(
    info: *mut DllInfo,
    croutines: *const R_CMethodDef,
    call_routines: *const R_CallMethodDef,
    fortran_routines: *const R_FortranMethodDef,
    external_routines: *const R_ExternalMethodDef,
) -> c_int {
    unsafe {
        if info.is_null() {
            error("R_registerRoutines called with invalid DllInfo object.");
        }

        // Default: look in registered, then dynamic (unless no handle like base/embedded)
        (*info).use_dynamic_lookup = !(*info).handle.is_null();
        (*info).force_symbols = false;

        // Register .C routines
        if !croutines.is_null() {
            let num = count_null_terminated_routines(
                croutines as *const c_void,
                std::mem::size_of::<R_CMethodDef>(),
            ) as c_int;
            let sym_table =
                libc_calloc(num as usize, std::mem::size_of::<DotCSymbol>()) as *mut DotCSymbol;
            if sym_table.is_null() && num > 0 {
                error("allocation failure in R_registerRoutines");
            }
            (*info).num_c_symbols = num;
            (*info).c_symbols = sym_table;
            for i in 0..num {
                let cr = &*croutines.add(i as usize);
                add_c_routine(info, cr, sym_table.add(i as usize));
            }
        }

        // Register .Fortran routines
        if !fortran_routines.is_null() {
            let num = count_null_terminated_routines(
                fortran_routines as *const c_void,
                std::mem::size_of::<R_FortranMethodDef>(),
            ) as c_int;
            let sym_table = libc_calloc(num as usize, std::mem::size_of::<DotFortranSymbol>())
                as *mut DotFortranSymbol;
            if sym_table.is_null() && num > 0 {
                // Free previously allocated C symbols
                if !(*info).c_symbols.is_null() {
                    libc_free((*info).c_symbols as *mut c_void);
                    (*info).c_symbols = ptr::null_mut();
                    (*info).num_c_symbols = 0;
                }
                error("allocation failure in R_registerRoutines");
            }
            (*info).num_fortran_symbols = num;
            (*info).fortran_symbols = sym_table;
            for i in 0..num {
                let fr = &*fortran_routines.add(i as usize);
                add_fortran_routine(info, fr, sym_table.add(i as usize));
            }
        }

        // Register .Call routines
        if !call_routines.is_null() {
            let num = count_null_terminated_routines(
                call_routines as *const c_void,
                std::mem::size_of::<R_CallMethodDef>(),
            ) as c_int;
            let sym_table = libc_calloc(num as usize, std::mem::size_of::<DotCallSymbol>())
                as *mut DotCallSymbol;
            if sym_table.is_null() && num > 0 {
                if !(*info).c_symbols.is_null() {
                    libc_free((*info).c_symbols as *mut c_void);
                }
                if !(*info).fortran_symbols.is_null() {
                    libc_free((*info).fortran_symbols as *mut c_void);
                }
                error("allocation failure in R_registerRoutines");
            }
            (*info).num_call_symbols = num;
            (*info).call_symbols = sym_table;
            for i in 0..num {
                let cr = &*call_routines.add(i as usize);
                add_call_routine(info, cr, sym_table.add(i as usize));
            }
        }

        // Register .External routines
        if !external_routines.is_null() {
            let num = count_null_terminated_routines(
                external_routines as *const c_void,
                std::mem::size_of::<R_ExternalMethodDef>(),
            ) as c_int;
            let sym_table = libc_calloc(num as usize, std::mem::size_of::<DotExternalSymbol>())
                as *mut DotExternalSymbol;
            if sym_table.is_null() && num > 0 {
                if !(*info).c_symbols.is_null() {
                    libc_free((*info).c_symbols as *mut c_void);
                }
                if !(*info).fortran_symbols.is_null() {
                    libc_free((*info).fortran_symbols as *mut c_void);
                }
                if !(*info).call_symbols.is_null() {
                    libc_free((*info).call_symbols as *mut c_void);
                }
                error("allocation failure in R_registerRoutines");
            }
            (*info).num_external_symbols = num;
            (*info).external_symbols = sym_table;
            for i in 0..num {
                let er = &*external_routines.add(i as usize);
                add_external_routine(info, er, sym_table.add(i as usize));
            }
        }

        1
    }
}

/// Count entries in a null-terminated array of routine definitions.
/// Each entry is `entry_size` bytes; the terminator has name == NULL.
unsafe fn count_null_terminated_routines(base: *const c_void, entry_size: usize) -> usize {
    unsafe {
        let mut count = 0usize;
        let mut ptr = base;
        loop {
            // The first field of each struct is name: *const c_char
            let name_ptr = *(ptr as *const *const c_char);
            if name_ptr.is_null() {
                break;
            }
            count += 1;
            ptr = ptr.add(entry_size);
        }
        count
    }
}

// ---------------------------------------------------------------------------
// Symbol lookup — registered symbols
// ---------------------------------------------------------------------------

unsafe fn lookup_registered_c_symbol(
    info: *const DllInfo,
    name: *const c_char,
) -> *const DotCSymbol {
    unsafe {
        let n = (*info).num_c_symbols;
        if n <= 0 || (*info).c_symbols.is_null() {
            return ptr::null();
        }
        for i in 0..n as usize {
            let sym = (*info).c_symbols.add(i);
            if !(*sym).name.is_null() && strcmp((*sym).name, name) == 0 {
                return sym;
            }
        }
        ptr::null()
    }
}

unsafe fn lookup_registered_call_symbol(
    info: *const DllInfo,
    name: *const c_char,
) -> *const DotCallSymbol {
    unsafe {
        let n = (*info).num_call_symbols;
        if n <= 0 || (*info).call_symbols.is_null() {
            return ptr::null();
        }
        for i in 0..n as usize {
            let sym = (*info).call_symbols.add(i);
            if !(*sym).name.is_null() && strcmp((*sym).name, name) == 0 {
                return sym;
            }
        }
        ptr::null()
    }
}

unsafe fn lookup_registered_fortran_symbol(
    info: *const DllInfo,
    name: *const c_char,
) -> *const DotFortranSymbol {
    unsafe {
        let n = (*info).num_fortran_symbols;
        if n <= 0 || (*info).fortran_symbols.is_null() {
            return ptr::null();
        }
        for i in 0..n as usize {
            let sym = (*info).fortran_symbols.add(i);
            if !(*sym).name.is_null() && strcmp((*sym).name, name) == 0 {
                return sym;
            }
        }
        ptr::null()
    }
}

unsafe fn lookup_registered_external_symbol(
    info: *const DllInfo,
    name: *const c_char,
) -> *const DotExternalSymbol {
    unsafe {
        let n = (*info).num_external_symbols;
        if n <= 0 || (*info).external_symbols.is_null() {
            return ptr::null();
        }
        for i in 0..n as usize {
            let sym = (*info).external_symbols.add(i);
            if !(*sym).name.is_null() && strcmp((*sym).name, name) == 0 {
                return sym;
            }
        }
        ptr::null()
    }
}

// ---------------------------------------------------------------------------
// R_getDLLRegisteredSymbol — look up a symbol in registered tables
// ---------------------------------------------------------------------------

/// Look up a registered native routine in a DLL.
/// If found and `symbol` is non-null, fills in the symbol info.
/// Returns the function pointer or None.
pub unsafe fn R_getDLLRegisteredSymbol(
    info: *const DllInfo,
    name: *const c_char,
    sym_type: c_int,
) -> DL_FUNC {
    unsafe {
        // .C
        if (sym_type == R_ANY_SYM || sym_type == R_C_SYM) && (*info).num_c_symbols > 0 {
            let sym = lookup_registered_c_symbol(info, name);
            if !sym.is_null() {
                return (*sym).fun;
            }
        }

        // .Call
        if (sym_type == R_ANY_SYM || sym_type == R_CALL_SYM) && (*info).num_call_symbols > 0 {
            let sym = lookup_registered_call_symbol(info, name);
            if !sym.is_null() {
                return (*sym).fun;
            }
        }

        // .Fortran
        if (sym_type == R_ANY_SYM || sym_type == R_FORTRAN_SYM) && (*info).num_fortran_symbols > 0 {
            let sym = lookup_registered_fortran_symbol(info, name);
            if !sym.is_null() {
                return (*sym).fun;
            }
        }

        // .External
        if (sym_type == R_ANY_SYM || sym_type == R_EXTERNAL_SYM) && (*info).num_external_symbols > 0
        {
            let sym = lookup_registered_external_symbol(info, name);
            if !sym.is_null() {
                return (*sym).fun;
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// R_dlsym — look up a symbol (registered, then dynamic)
// ---------------------------------------------------------------------------

/// Look up a native symbol by name in a DLL.
/// Checks registered symbols first, then falls back to dynamic lookup.
pub unsafe fn R_dlsym(info: *mut DllInfo, name: *const c_char, sym_type: c_int) -> DL_FUNC {
    unsafe {
        // Try registered symbols first
        let f = R_getDLLRegisteredSymbol(info, name, sym_type);
        if f.is_some() {
            return f;
        }

        // If dynamic lookup is disabled, stop
        if !(*info).use_dynamic_lookup {
            return None;
        }

        // Try dynamic lookup via dlsym
        if (*info).handle.is_null() {
            return None;
        }

        // On modern ELF systems, symbols don't have leading underscore
        let f = crate::unix::dynload::Rf_lookupCachedSymbol(name, 0);
        if f.is_some() {
            return f;
        }

        None
    }
}

// ---------------------------------------------------------------------------
// R_FindSymbol — search all loaded DLLs for a symbol
// ---------------------------------------------------------------------------

pub unsafe fn R_FindSymbol(name: *const c_char, pkg: *const c_char, sym_type: c_int) -> DL_FUNC {
    unsafe {
        let pkg_cstr = if pkg.is_null() {
            ""
        } else {
            std::ffi::CStr::from_ptr(pkg).to_str().unwrap_or("")
        };
        let all = pkg_cstr.is_empty();

        LOADED_DLL.with(|v| {
            let dlls = v.borrow();
            // Search in reverse order (most recently loaded first)
            let mut i = dlls.len();
            while i > 0 {
                i -= 1;
                let dll = dlls[i];
                if dll.is_null() {
                    continue;
                }

                let mut doit = all;
                if !doit {
                    let dll_name = std::ffi::CStr::from_ptr((*dll).name);
                    let dll_str = dll_name.to_str().unwrap_or("");
                    if dll_str == pkg_cstr {
                        doit = true;
                    }
                }
                if doit && (*dll).force_symbols {
                    doit = false;
                }

                if doit {
                    let f = R_dlsym(dll, name, sym_type);
                    if f.is_some() {
                        return f;
                    }
                }

                // If we found the right DLL but the symbol wasn't there, stop.
                if doit && !all {
                    return None;
                }
            }
            None
        })
    }
}

// ---------------------------------------------------------------------------
// Free DllInfo
// ---------------------------------------------------------------------------

unsafe fn free_dll_info(info: *mut DllInfo) {
    unsafe {
        if info.is_null() {
            return;
        }
        let info = &mut *info;

        // Free C symbols
        if !info.c_symbols.is_null() {
            for i in 0..info.num_c_symbols as usize {
                let sym = info.c_symbols.add(i);
                libc_free((*sym).name as *mut c_void);
                if !(*sym).types.is_null() {
                    libc_free((*sym).types as *mut c_void);
                }
            }
            libc_free(info.c_symbols as *mut c_void);
        }

        // Free Call symbols
        if !info.call_symbols.is_null() {
            for i in 0..info.num_call_symbols as usize {
                let sym = info.call_symbols.add(i);
                libc_free((*sym).name as *mut c_void);
            }
            libc_free(info.call_symbols as *mut c_void);
        }

        // Free Fortran symbols
        if !info.fortran_symbols.is_null() {
            for i in 0..info.num_fortran_symbols as usize {
                let sym = info.fortran_symbols.add(i);
                libc_free((*sym).name as *mut c_void);
                if !(*sym).types.is_null() {
                    libc_free((*sym).types as *mut c_void);
                }
            }
            libc_free(info.fortran_symbols as *mut c_void);
        }

        // Free External symbols
        if !info.external_symbols.is_null() {
            for i in 0..info.num_external_symbols as usize {
                let sym = info.external_symbols.add(i);
                libc_free((*sym).name as *mut c_void);
            }
            libc_free(info.external_symbols as *mut c_void);
        }

        libc_free(info.path as *mut c_void);
        libc_free(info.name as *mut c_void);
        // Drop the Box
        drop(Box::from_raw(info as *mut DllInfo));
    }
}

// ---------------------------------------------------------------------------
// DeleteDLL — remove a DLL from the table
// ---------------------------------------------------------------------------

unsafe fn call_dll_unload(dll_info: *mut DllInfo) {
    unsafe {
        let name = CStr::from_ptr((*dll_info).name);
        let name_bytes = name.to_bytes();
        let buf = format!("R_unload_{}\0", std::str::from_utf8_unchecked(name_bytes));
        unsafe extern "C" {
            fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        }
        let sym = dlsym((*dll_info).handle, buf.as_ptr() as *const c_char);
        if !sym.is_null() {
            let unload_fn: Option<unsafe extern "C" fn(*mut DllInfo)> = std::mem::transmute(sym);
            if let Some(f) = unload_fn {
                f(dll_info);
            }
        }
    }
}

unsafe fn delete_dll(path: *const c_char) -> bool {
    unsafe {
        LOADED_DLL.with(|v| {
            let mut dlls = v.borrow_mut();
            let loc = {
                let mut found = None;
                for (i, &dll) in dlls.iter().enumerate() {
                    if !dll.is_null() && strcmp((*dll).path, path) == 0 {
                        found = Some(i);
                        break;
                    }
                }
                found
            };

            match loc {
                Some(idx) => {
                    let dll = dlls[idx];
                    if !dll.is_null() {
                        call_dll_unload(dll);
                        if !(*dll).handle.is_null() {
                            unsafe extern "C" {
                                fn dlclose(handle: *mut c_void) -> c_int;
                            }
                            dlclose((*dll).handle);
                        }
                        free_dll_info(dll);
                    }
                    dlls.remove(idx);
                    true
                }
                None => false,
            }
        })
    }
}

// ---------------------------------------------------------------------------
// AddDLL — high-level DLL loading (with dlopen)
// ---------------------------------------------------------------------------

/// Load a shared library and register it. Returns the DllInfo, or null on failure.
unsafe fn AddDLL(
    path: *const c_char,
    _as_local: c_int,
    _now: c_int,
    _search_path: *const c_char,
) -> *mut DllInfo {
    unsafe {
        // Check if already loaded — if so, move to end of list (most recent)
        let already_loaded = LOADED_DLL.with(|v| {
            let mut dlls = v.borrow_mut();
            for i in 0..dlls.len() {
                let dll = dlls[i];
                if !dll.is_null() && strcmp((*dll).path, path) == 0 {
                    let entry = dlls.remove(i);
                    dlls.push(entry);
                    return Some(entry);
                }
            }
            None
        });
        if let Some(dll) = already_loaded {
            return dll;
        }

        let at_max =
            MAX_NUM_DLLS.with(|max| LOADED_DLL.with(|v| v.borrow().len() >= *max.borrow()));
        if at_max {
            return ptr::null_mut();
        }

        let handle = {
            let flag = if _now != 0 { 0x2 } else { 0x1 };
            let flag = if _as_local != 0 {
                flag | 0x4
            } else {
                flag | 0x8
            };
            unsafe extern "C" {
                fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
            }
            dlopen(path, flag)
        };

        let dpath = strdup(path);
        if dpath.is_null() {
            return ptr::null_mut();
        }

        let dllname = extract_dll_name(dpath);
        let info = Box::into_raw(Box::new(DllInfo::new(dpath, dllname, handle)));
        (*info).use_dynamic_lookup = !handle.is_null();
        (*info).force_symbols = false;

        LOADED_DLL.with(|v| {
            v.borrow_mut().push(info);
        });

        call_init_routine(info);

        info
    }
}

/// Extract the DLL name from a full path: basename minus SHLIB_EXT.
unsafe fn extract_dll_name(path: *mut c_char) -> *mut c_char {
    unsafe {
        let path_str = std::ffi::CStr::from_ptr(path).to_bytes();
        let path_str = std::str::from_utf8(path_str).unwrap_or("");

        // Find last '/'
        let basename = path_str.rsplit('/').next().unwrap_or(path_str);

        // Remove .so extension
        let name = basename.strip_suffix(".so").unwrap_or(basename);

        strdup(CString::new(name).unwrap_or_default().as_ptr())
    }
}

/// Call the R_init_<name> routine if it exists in the DLL.
unsafe fn call_init_routine(info: *mut DllInfo) {
    unsafe {
        if info.is_null() || (*info).handle.is_null() {
            return;
        }
        let name = std::ffi::CStr::from_ptr((*info).name).to_bytes();
        let name_str = std::str::from_utf8(name).unwrap_or("");

        // R_init_<name>
        let init_name = format!("R_init_{}\0", name_str);
        let init_cstr = init_name.as_ptr() as *const c_char;

        unsafe extern "C" {
            fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        }

        let sym = dlsym((*info).handle, init_cstr);
        if !sym.is_null() {
            let f: unsafe extern "C" fn(*mut DllInfo) = std::mem::transmute(sym);
            f(info);
        }
    }
}

// ---------------------------------------------------------------------------
// R_getSymbolInfo — resolve a symbol and return its info as an SEXP
// ---------------------------------------------------------------------------

pub unsafe fn R_getSymbolInfo(sname: SEXP, spackage: SEXP, _with_registration: SEXP) -> SEXP {
    unsafe {
        let name = if Rf_isString(sname) == 0 || Rf_length(sname) != 1 {
            error("invalid 'name' argument");
        } else {
            translateChar(STRING_ELT(sname, 0))
        };

        let pkg_str = if Rf_length(spackage) == 0 {
            None
        } else if TYPEOF(spackage) == SEXPTYPE::STRSXP {
            Some(
                std::ffi::CStr::from_ptr(translateChar(STRING_ELT(spackage, 0)))
                    .to_bytes_with_nul()
                    .to_vec(),
            )
        } else {
            None
        };

        // Search for the symbol
        let f = if let Some(ref pkg_bytes) = pkg_str {
            let pkg_ptr = pkg_bytes.as_ptr() as *const c_char;
            R_FindSymbol(name, pkg_ptr, R_ANY_SYM)
        } else {
            R_FindSymbol(name, b"\0".as_ptr() as *const c_char, R_ANY_SYM)
        };

        if f.is_none() {
            return R_NilValue();
        }

        // Create a simple NativeSymbolInfo object
        let sym = Rf_allocVector(SEXPTYPE::VECSXP, 3);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, 3);

        SET_VECTOR_ELT(sym, 0, sname);
        SET_STRING_ELT(names, 0, Rf_mkChar(b"name\0".as_ptr() as *const c_char));

        // Address as external pointer (simplified)
        SET_VECTOR_ELT(
            sym,
            1,
            Rf_mkString(b"<native symbol>\0".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(names, 1, Rf_mkChar(b"address\0".as_ptr() as *const c_char));

        SET_VECTOR_ELT(
            sym,
            2,
            Rf_mkString(b"<unknown>\0".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(names, 2, Rf_mkChar(b"dll\0".as_ptr() as *const c_char));

        setAttrib(sym, R_NamesSymbol(), names);
        setAttrib(
            sym,
            R_ClassSymbol(),
            Rf_mkString(b"NativeSymbolInfo\0".as_ptr() as *const c_char),
        );

        sym
    }
}

// ---------------------------------------------------------------------------
// R_getDllTable — return info on all loaded DLLs
// ---------------------------------------------------------------------------

pub unsafe fn R_getDllTable() -> SEXP {
    unsafe {
        LOADED_DLL.with(|v| {
            let dlls = v.borrow();
            let count = dlls.len();
            let ans = Rf_allocVector(SEXPTYPE::VECSXP, count as c_int);
            for i in 0..count {
                let info = dlls[i];
                if !info.is_null() {
                    SET_VECTOR_ELT(ans, i as R_xlen_t, make_dll_info_sexp(info));
                }
            }
            setAttrib(
                ans,
                R_ClassSymbol(),
                Rf_mkString(b"DLLInfoList\0".as_ptr() as *const c_char),
            );
            ans
        })
    }
}

use crate::sexp::ffi::R_xlen_t;

/// Create an SEXP representing a DllInfo.
unsafe fn make_dll_info_sexp(info: *const DllInfo) -> SEXP {
    unsafe {
        let ref_vec = Rf_allocVector(SEXPTYPE::VECSXP, 6);
        let el_names = Rf_allocVector(SEXPTYPE::STRSXP, 6);

        // name
        let tmp = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        if !(*info).name.is_null() {
            SET_STRING_ELT(tmp, 0, Rf_mkChar((*info).name));
        }
        SET_VECTOR_ELT(ref_vec, 0, tmp);
        SET_STRING_ELT(el_names, 0, Rf_mkChar(b"name\0".as_ptr() as *const c_char));

        // path
        let tmp = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        if !(*info).path.is_null() {
            SET_STRING_ELT(tmp, 0, Rf_mkChar((*info).path));
        }
        SET_VECTOR_ELT(ref_vec, 1, tmp);
        SET_STRING_ELT(el_names, 1, Rf_mkChar(b"path\0".as_ptr() as *const c_char));

        // dynamicLookup
        SET_VECTOR_ELT(
            ref_vec,
            2,
            Rf_ScalarLogical(if (*info).use_dynamic_lookup {
                TRUE
            } else {
                FALSE
            }),
        );
        SET_STRING_ELT(
            el_names,
            2,
            Rf_mkChar(b"dynamicLookup\0".as_ptr() as *const c_char),
        );

        // handle (simplified as nil)
        SET_VECTOR_ELT(ref_vec, 3, R_NilValue());
        SET_STRING_ELT(
            el_names,
            3,
            Rf_mkChar(b"handle\0".as_ptr() as *const c_char),
        );

        // info (simplified as nil)
        SET_VECTOR_ELT(ref_vec, 4, R_NilValue());
        SET_STRING_ELT(el_names, 4, Rf_mkChar(b"info\0".as_ptr() as *const c_char));

        // forceSymbols
        SET_VECTOR_ELT(
            ref_vec,
            5,
            Rf_ScalarLogical(if (*info).force_symbols { TRUE } else { FALSE }),
        );
        SET_STRING_ELT(
            el_names,
            5,
            Rf_mkChar(b"forceSymbols\0".as_ptr() as *const c_char),
        );

        setAttrib(ref_vec, R_NamesSymbol(), el_names);
        setAttrib(
            ref_vec,
            R_ClassSymbol(),
            Rf_mkString(b"DLLInfo\0".as_ptr() as *const c_char),
        );

        ref_vec
    }
}

// ---------------------------------------------------------------------------
// R_getRegisteredRoutines — return registered routines for a DLL
// ---------------------------------------------------------------------------

pub unsafe fn R_getRegisteredRoutines(dll: SEXP) -> SEXP {
    unsafe {
        // In a full implementation, we'd extract the DllInfo* from the external pointer.
        // For now, return empty lists since we don't have a real external pointer to dereference.
        let ans = Rf_allocVector(SEXPTYPE::VECSXP, 4);
        let snames = Rf_allocVector(SEXPTYPE::STRSXP, 4);

        let type_names: [&[u8]; 4] = [b".C\0", b".Call\0", b".Fortran\0", b".External\0"];
        for i in 0..4usize {
            SET_VECTOR_ELT(ans, i as R_xlen_t, R_NilValue());
            SET_STRING_ELT(
                snames,
                i as R_xlen_t,
                Rf_mkChar(type_names[i].as_ptr() as *const c_char),
            );
        }

        setAttrib(ans, R_NamesSymbol(), snames);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_dynload / do_dynunload — R-level interfaces
// ---------------------------------------------------------------------------

/// `.Internal(dyn.load(...))` handler.
pub unsafe fn do_dynload(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if Rf_isString(CAR(args)) == 0 || Rf_length(CAR(args)) != 1 {
            error("character argument expected");
        }
        let path = translateChar(STRING_ELT(CAR(args), 0));
        // asLocal, now, searchPath from remaining args (simplified)
        let as_local = if !CAR(CDR(args)).is_null() {
            let l = crate::mainutils::coerce::asLogical(CAR(CDR(args)));
            if l != 0 { 1 } else { 0 }
        } else {
            1
        };
        let now = if !CAR(CDR(CDR(args))).is_null() {
            let l = crate::mainutils::coerce::asLogical(CAR(CDR(CDR(args))));
            if l != 0 { 1 } else { 0 }
        } else {
            1
        };

        let info = AddDLL(path, as_local, now, b"\0".as_ptr() as *const c_char);
        if info.is_null() {
            DLL_ERROR.with(|e| {
                let err_bytes = &*e.borrow();
                let err_str = std::ffi::CStr::from_bytes_until_nul(err_bytes).unwrap_or(
                    std::ffi::CStr::from_bytes_with_nul(b"unknown error\0").unwrap_or_default(),
                );
                error(&format!(
                    "unable to load shared object '{}':\n  {}",
                    std::ffi::CStr::from_ptr(path).to_str().unwrap_or("?"),
                    err_str.to_str().unwrap_or("?")
                ));
            });
        }
        make_dll_info_sexp(info)
    }
}

/// `.Internal(dyn.unload(...))` handler.
pub unsafe fn do_dynunload(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if Rf_isString(CAR(args)) == 0 || Rf_length(CAR(args)) != 1 {
            error("character argument expected");
        }
        let path = translateChar(STRING_ELT(CAR(args), 0));
        if !delete_dll(path) {
            error(&format!(
                "shared object '{}' was not loaded",
                std::ffi::CStr::from_ptr(path).to_str().unwrap_or("?")
            ));
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_getSymbolInfo / do_getDllTable / do_getRegisteredRoutines
// ---------------------------------------------------------------------------

pub unsafe fn do_getSymbolInfo(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        R_getSymbolInfo(CAR(args), CAR(CDR(args)), CAR(CDR(CDR(args))))
    }
}

pub unsafe fn do_getDllTable(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        R_getDllTable()
    }
}

pub unsafe fn do_getRegisteredRoutines(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        R_getRegisteredRoutines(CAR(args))
    }
}

// ---------------------------------------------------------------------------
// R_moduleCdynload / R_cairoCdynload
// ---------------------------------------------------------------------------

pub unsafe fn R_moduleCdynload(module: *const c_char, local: c_int, now: c_int) -> c_int {
    unsafe {
        let r_home = std::env::var("R_HOME").unwrap_or_default();
        if r_home.is_empty() {
            return 0;
        }
        let module_str = std::ffi::CStr::from_ptr(module).to_str().unwrap_or("");
        let dllpath = format!("{}/modules/{}{}\0", r_home, module_str, ".so");
        let info = AddDLL(
            dllpath.as_ptr() as *const c_char,
            local,
            now,
            b"\0".as_ptr() as *const c_char,
        );
        if info.is_null() { 0 } else { 1 }
    }
}

pub unsafe fn R_cairoCdynload(local: c_int, now: c_int) -> c_int {
    unsafe {
        let r_home = std::env::var("R_HOME").unwrap_or_default();
        if r_home.is_empty() {
            return 0;
        }
        let dllpath = format!("{}/library/grDevices/libs/cairo{}\0", r_home, ".so");
        let info = AddDLL(
            dllpath.as_ptr() as *const c_char,
            local,
            now,
            b"\0".as_ptr() as *const c_char,
        );
        if info.is_null() { 0 } else { 1 }
    }
}

// ---------------------------------------------------------------------------
// R_RegisterCCallable / R_GetCCallable — inter-package C function sharing
// ---------------------------------------------------------------------------

use std::sync::atomic::AtomicPtr;

static C_ENTRY_TABLE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

unsafe fn get_package_centry_table(package: *const c_char) -> SEXP {
    unsafe {
        let table = C_ENTRY_TABLE.load(Ordering::Acquire);
        if table.is_null() {
            let env = R_NewHashedEnv(R_NilValue(), 0);
            // Preserve from GC
            // In a full implementation we'd call R_PreserveObject here
            C_ENTRY_TABLE.store(env as *mut c_void, Ordering::Release);
        }
        let table = C_ENTRY_TABLE.load(Ordering::Acquire) as SEXP;
        let pname = Rf_install(package);
        let penv = R_findVarInFrame(table, pname);
        if penv == R_UnboundValue() {
            let new_env = R_NewHashedEnv(R_NilValue(), 0);
            defineVar(pname, new_env, table);
            new_env
        } else {
            penv
        }
    }
}

/// Register a C-callable function for inter-package use.
pub unsafe fn R_RegisterCCallable(package: *const c_char, name: *const c_char, fptr: DL_FUNC) {
    unsafe {
        let penv = get_package_centry_table(package);
        let sym_name = Rf_install(name);
        if let Some(fp) = fptr {
            let fptr_raw = fp as *mut c_void;
            let eptr = R_MakeExternalPtr(fptr_raw, R_NilValue(), R_NilValue());
            defineVar(sym_name, eptr, penv);
        }
    }
}

/// Retrieve a C-callable function registered by another package.
pub unsafe fn R_GetCCallable(package: *const c_char, name: *const c_char) -> DL_FUNC {
    unsafe {
        let penv = get_package_centry_table(package);
        let sym_name = Rf_install(name);
        let val = R_findVarInFrame(penv, sym_name);
        if val == R_UnboundValue() {
            error(&format!(
                "function '{}' not provided by package '{}'",
                std::ffi::CStr::from_ptr(name).to_str().unwrap_or("?"),
                std::ffi::CStr::from_ptr(package).to_str().unwrap_or("?")
            ));
        }
        if TYPEOF(val) != SEXPTYPE::EXTPTRSXP {
            error("table entry must be an external pointer");
        }
        let addr = R_ExternalPtrAddr(val);
        std::mem::transmute::<*mut c_void, Option<unsafe extern "C" fn()>>(addr)
    }
}

// ---------------------------------------------------------------------------
// Rf_lookupCachedSymbol — compatibility stub
// ---------------------------------------------------------------------------

pub unsafe fn Rf_lookupCachedSymbol(
    _name: *const c_char,
    _pkg: *const c_char,
    _all: c_int,
) -> DL_FUNC {
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_dynload() {
        unsafe {
            init_loaded_dll();
            LOADED_DLL.with(|v| {
                assert!(v.borrow().is_empty());
            });
        }
    }

    #[test]
    fn test_add_dll() {
        unsafe {
            init_loaded_dll();
            let idx = add_dll(
                b"test_path\0".as_ptr() as *const c_char,
                b"test_name\0".as_ptr() as *const c_char,
                ptr::null_mut(),
            );
            assert_eq!(idx, 0);
            LOADED_DLL.with(|v| {
                assert_eq!(v.borrow().len(), 1);
            });
        }
    }

    #[test]
    fn test_get_dll_info_not_found() {
        unsafe {
            init_loaded_dll();
            let result = R_getDllInfo(b"nonexistent\0".as_ptr() as *const c_char);
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_get_dll_info_found() {
        unsafe {
            init_loaded_dll();
            let _ = add_dll(
                b"test_path\0".as_ptr() as *const c_char,
                b"test_name\0".as_ptr() as *const c_char,
                ptr::null_mut(),
            );
            let result = R_getDllInfo(b"test_path\0".as_ptr() as *const c_char);
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_delete_dll() {
        unsafe {
            init_loaded_dll();
            let _ = add_dll(
                b"to_delete\0".as_ptr() as *const c_char,
                b"del_name\0".as_ptr() as *const c_char,
                ptr::null_mut(),
            );
            assert!(delete_dll(b"to_delete\0".as_ptr() as *const c_char));
            LOADED_DLL.with(|v| {
                assert!(v.borrow().is_empty());
            });
        }
    }

    #[test]
    fn test_delete_dll_not_found() {
        unsafe {
            init_loaded_dll();
            assert!(!delete_dll(b"nonexistent\0".as_ptr() as *const c_char));
        }
    }

    #[test]
    fn test_use_dynamic_symbols() {
        unsafe {
            init_loaded_dll();
            let _ = add_dll(
                b"path\0".as_ptr() as *const c_char,
                b"name\0".as_ptr() as *const c_char,
                ptr::null_mut(),
            );
            let dll = R_getDllInfo(b"path\0".as_ptr() as *const c_char);
            assert!(!dll.is_null());
            let old = R_useDynamicSymbols(dll, false);
            assert!(old); // default is true
            assert!(!(*dll).use_dynamic_lookup);
        }
    }

    #[test]
    fn test_force_symbols() {
        unsafe {
            init_loaded_dll();
            let _ = add_dll(
                b"path2\0".as_ptr() as *const c_char,
                b"name2\0".as_ptr() as *const c_char,
                ptr::null_mut(),
            );
            let dll = R_getDllInfo(b"path2\0".as_ptr() as *const c_char);
            assert!(!dll.is_null());
            let old = R_forceSymbols(dll, true);
            assert!(!old); // default is false
            assert!((*dll).force_symbols);
        }
    }

    #[test]
    fn test_extract_dll_name() {
        unsafe {
            let path = strdup(b"/usr/lib/libfoo.so\0".as_ptr() as *const c_char);
            let name = extract_dll_name(path);
            let name_str = std::ffi::CStr::from_ptr(name).to_str().unwrap_or("");
            assert_eq!(name_str, "libfoo");
            libc_free(path as *mut c_void);
            libc_free(name as *mut c_void);
        }
    }

    #[test]
    fn test_find_symbol_empty() {
        unsafe {
            init_loaded_dll();
            let f = R_FindSymbol(
                b"nonexistent\0".as_ptr() as *const c_char,
                b"\0".as_ptr() as *const c_char,
                R_ANY_SYM,
            );
            assert!(f.is_none());
        }
    }
}
