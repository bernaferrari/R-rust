#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/Rdynload.c -- Dynamic Loading Support.
//!
//! Original source: src/main/Rdynload.c (~1770 lines)
//!
//! This module provides support for run-time loading of shared objects
//! and access to symbols within such objects via .C, .Fortran, .Call, .External.
//! On Unix this uses dlopen/dlsym/dlclose.
//!
//! Key functions:
//!   - R_registerRoutines: register native routines for a DllInfo
//!   - R_FindSymbol: look up a symbol across all loaded DLLs
//!   - R_dlsym: look up a symbol in a specific DLL
//!   - R_RegisterCCallable / R_GetCCallable: cross-package callable registration
//!   - do_dynload / do_dynunload: R-level .Internal entry points
//!
//! NOTE: This is a port of the C code. Many functions reference types and
//! functions defined elsewhere (DllInfo, SEXP, etc.). Where the real
//! implementation depends on full R internals, stubs are provided.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::SEXPTYPE;

// ---------------------------------------------------------------------------
// Forward declarations / imports for symbols defined in other modules
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn R_EnsureFDLimit(desired: c_int) -> c_int;
    fn R_init_base(dll: *mut DllInfo);
    fn InitFunctionHashing();
    fn R_reinit_altrep_classes(dll: *mut DllInfo);
    fn R_Suicide(msg: *const c_char);
    fn R_PreserveObject(s: SEXP);
    fn R_MakeWeakRef(key: SEXP, val: SEXP, fin: SEXP, terminal: c_int) -> SEXP;
    fn R_WeakRefKey(w: SEXP) -> SEXP;
    fn R_WeakRefValue(w: SEXP) -> SEXP;
    fn R_ClearExternalPtr(s: SEXP);
    fn R_RegisterCFinalizer(s: SEXP, fun: Option<unsafe extern "C" fn(SEXP)>);
    fn R_MakeExternalPtrFn(p: DL_FUNC, tag: SEXP, prot: SEXP) -> SEXP;
    fn R_ExternalPtrAddrFn(s: SEXP) -> DL_FUNC;
    fn R_NewHashedEnv(enclos: SEXP, size: c_int) -> SEXP;
    fn R_findVarInFrame(env: SEXP, sym: SEXP) -> SEXP;
    fn R_UnboundValue() -> SEXP;
    fn R_alloc(size: usize, nelem: usize) -> *mut c_void;
    fn vmaxget() -> *mut c_void;
    fn vmaxset(vmax: *mut c_void);
    fn Rstrdup(s: *const c_char) -> *mut c_char;
    fn install(name: *const c_char) -> SEXP;
    fn defineVar(symbol: SEXP, value: SEXP, rho: SEXP);
    fn mkString(s: *const c_char) -> SEXP;
    fn mkChar(s: *const c_char) -> SEXP;
    fn checkArity(op: SEXP, args: SEXP);
    fn translateCharFP(x: SEXP) -> *const c_char;
    fn asBool2(x: SEXP, call: SEXP) -> c_int;
    fn ngettext(singular: *const c_char, plural: *const c_char, n: libc::c_ulong) -> *const c_char;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of DLLs that can be loaded.
static mut MaxNumDLLs: c_int = 0;

/// Current number of loaded DLLs.
static mut CountDLL: c_int = 0;

/// R_MAX_NUM_DLLS env var
const R_MAX_NUM_DLLS_ENV: &[u8] = b"R_MAX_NUM_DLLS\0";

// DLL error buffer
#[cfg(not(target_os = "windows"))]
const DLLERR_BUFSIZE: usize = 1000;
#[cfg(target_os = "windows")]
const DLLERR_BUFSIZE: usize = 4000;

static mut DLLerror: [c_char; DLLERR_BUFSIZE] = [0; DLLERR_BUFSIZE];

// RDYN_SHLIB_EXT and RDYN_FILESEP for Unix (prefixed to avoid conflicts)
#[cfg(all(unix, not(target_os = "macos")))]
const RDYN_SHLIB_EXT: &[u8] = b".so\0";
#[cfg(unix)]
const RDYN_FILESEP: &[u8] = b"/\0";

#[cfg(target_os = "macos")]
const RDYN_SHLIB_EXT: &[u8] = b".dylib\0";

const R_PATH_MAX: usize = 4096;

// MAXCOUNT for cleanup
const MAXCOUNT: c_int = 10;

// CACHE_DLL_SYM (Windows-only, kept as stub)
// const CACHE_DLL_SYM: bool = false;

// ---------------------------------------------------------------------------
// Type aliases and structs
// ---------------------------------------------------------------------------

/// Generic function pointer type (matching R's DL_FUNC).
pub type DL_FUNC = Option<unsafe extern "C" fn()>;

/// Native primitive argument type (matches R_NativePrimitiveArgType).
pub type R_NativePrimitiveArgType = c_int;

/// Boolean type for registration.
type Rboolean = c_int;
pub const TRUE: Rboolean = 1;
pub const FALSE: Rboolean = 0;

/// NativeSymbolType enum.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeSymbolType {
    R_ANY_SYM = 0,
    R_C_SYM = 1,
    R_CALL_SYM = 2,
    R_FORTRAN_SYM = 3,
    R_EXTERNAL_SYM = 4,
}

/// Registered native symbol structure.
#[repr(C)]
pub struct R_RegisteredNativeSymbol {
    pub type_: NativeSymbolType,
    pub symbol: R_RegisteredNativeSymbol_u,
    pub dll: *mut DllInfo,
}

#[repr(C)]
pub union R_RegisteredNativeSymbol_u {
    pub c: *mut Rf_DotCSymbol,
    pub call: *mut Rf_DotCallSymbol,
    pub fortran: *mut Rf_DotFortranSymbol,
    pub external: *mut Rf_DotExternalSymbol,
    _bindgen_union_align: u64,
}

impl Default for R_RegisteredNativeSymbol_u {
    fn default() -> Self {
        R_RegisteredNativeSymbol_u {
            _bindgen_union_align: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// C method definition structs (matching R_ext/Rdynload.h)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct R_CMethodDef {
    pub name: *const c_char,
    pub fun: DL_FUNC,
    pub numArgs: c_int,
    pub types: *mut R_NativePrimitiveArgType,
}

#[repr(C)]
pub struct R_CallMethodDef {
    pub name: *const c_char,
    pub fun: DL_FUNC,
    pub numArgs: c_int,
    pub types: *mut R_NativePrimitiveArgType,
}

#[repr(C)]
pub struct R_FortranMethodDef {
    pub name: *const c_char,
    pub fun: DL_FUNC,
    pub numArgs: c_int,
    pub types: *mut R_NativePrimitiveArgType,
}

#[repr(C)]
pub struct R_ExternalMethodDef {
    pub name: *const c_char,
    pub fun: DL_FUNC,
    pub numArgs: c_int,
    pub types: *mut R_NativePrimitiveArgType,
}

// ---------------------------------------------------------------------------
// Symbol structs for registered routines
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct Rf_DotCSymbol {
    pub name: *mut c_char,
    pub fun: DL_FUNC,
    pub numArgs: c_int,
    pub types: *mut R_NativePrimitiveArgType,
}

#[repr(C)]
pub struct Rf_DotCallSymbol {
    pub name: *mut c_char,
    pub fun: DL_FUNC,
    pub numArgs: c_int,
}

#[repr(C)]
pub struct Rf_DotFortranSymbol {
    pub name: *mut c_char,
    pub fun: DL_FUNC,
    pub numArgs: c_int,
    pub types: *mut R_NativePrimitiveArgType,
}

#[repr(C)]
pub struct Rf_DotExternalSymbol {
    pub name: *mut c_char,
    pub fun: DL_FUNC,
    pub numArgs: c_int,
}

// ---------------------------------------------------------------------------
// DllInfo structure
// ---------------------------------------------------------------------------

/// DllInfo: information about a loaded shared library / DLL.
/// This is the full definition matching R's internal struct.
#[repr(C)]
pub struct DllInfo {
    pub path: *mut c_char,
    pub name: *mut c_char,
    pub handle: *mut c_void,
    pub useDynamicLookup: Rboolean,
    pub forceSymbols: Rboolean,
    pub numCSymbols: c_int,
    pub numCallSymbols: c_int,
    pub numFortranSymbols: c_int,
    pub numExternalSymbols: c_int,
    pub CSymbols: *mut Rf_DotCSymbol,
    pub CallSymbols: *mut Rf_DotCallSymbol,
    pub FortranSymbols: *mut Rf_DotFortranSymbol,
    pub ExternalSymbols: *mut Rf_DotExternalSymbol,
}

// ---------------------------------------------------------------------------
// OSDynSymbol: OS-specific dynamic symbol operations vtable
// ---------------------------------------------------------------------------

/// OS-specific dynamic symbol operations.
#[repr(C)]
pub struct OSDynSymbol {
    pub loadLibrary:
        Option<unsafe extern "C" fn(*const c_char, c_int, c_int, *const c_char) -> *mut c_void>,
    pub dlsym_fn: Option<unsafe extern "C" fn(*mut DllInfo, *const c_char) -> DL_FUNC>,
    pub closeLibrary: Option<unsafe extern "C" fn(*mut c_void)>,
    pub getError: Option<unsafe extern "C" fn(*mut c_char, c_int)>,
    pub fixPath: Option<unsafe extern "C" fn(*mut c_char)>,
    pub getFullDLLPath:
        Option<unsafe extern "C" fn(SEXP, *mut c_char, usize, *const c_char) -> usize>,
    pub lookupCachedSymbol: Option<
        unsafe extern "C" fn(*const c_char, *const c_char, c_int, *mut *mut DllInfo) -> DL_FUNC,
    >,
    pub deleteCachedSymbols: Option<unsafe extern "C" fn(*mut DllInfo)>,
}

/// Global OS dynamic symbol vtable.
static mut Rf_osDynSymbol: OSDynSymbol = OSDynSymbol::new();

impl OSDynSymbol {
    const fn new() -> Self {
        OSDynSymbol {
            loadLibrary: None,
            dlsym_fn: None,
            closeLibrary: None,
            getError: None,
            fixPath: None,
            getFullDLLPath: None,
            lookupCachedSymbol: None,
            deleteCachedSymbols: None,
        }
    }
}

/// Pointer to the global OS dynamic symbol vtable (for C compat).
/// TODO: This is initialized by InitFunctionHashing in unix/dynload.rs.
pub static mut R_osDynSymbol_ptr: *mut OSDynSymbol = ptr::null_mut();

// ---------------------------------------------------------------------------
// Global DLL table
// ---------------------------------------------------------------------------

/// Array of currently loaded DllInfo pointers.
static mut LoadedDLL: *mut *mut DllInfo = ptr::null_mut();

/// Cache of external pointers to DllInfo objects (VECSXP).
static mut DLLInfoEptrs: SEXP = ptr::null_mut();

/// Weak list of symbol external pointers.
static mut SymbolEptrs: SEXP = ptr::null_mut();

/// CEntryTable for R_RegisterCCallable / R_GetCCallable.
static mut CEntryTable: SEXP = ptr::null_mut();

// ---------------------------------------------------------------------------
// initLoadedDLL / InitDynload
// ---------------------------------------------------------------------------

/// Initialize the dynamic loading subsystem.
pub unsafe fn InitDynload() {
    unsafe {
        initLoadedDLL();
        let which = addDLL(
            Rstrdup(b"base\0".as_ptr() as *const c_char),
            b"base\0".as_ptr() as *mut c_char,
            ptr::null_mut(),
        );
        let dll = *LoadedDLL.add(which as usize);
        R_init_base(dll);
        InitFunctionHashing();
    }
}

/// Allocate the DLL table and associated R objects.
unsafe fn initLoadedDLL() {
    unsafe {
        if CountDLL != 0 || !LoadedDLL.is_null() {
            R_Suicide(b"DLL table corruption detected\0".as_ptr() as *const c_char);
        }

        let req = libc::getenv(R_MAX_NUM_DLLS_ENV.as_ptr() as *const c_char);
        if !req.is_null() {
            let reqlimit = atoi(req);
            if reqlimit < 100 {
                let mut msg = [0i8; 128];
                snprintf_buf(
                    &mut msg,
                    b"R_MAX_NUM_DLLS must be at least %d\0".as_ptr() as *const c_char,
                    100,
                );
                R_Suicide(msg.as_ptr());
            }
            if reqlimit > 1000 {
                let mut msg = [0i8; 128];
                snprintf_buf(
                    &mut msg,
                    b"R_MAX_NUM_DLLS cannot be bigger than %d\0".as_ptr() as *const c_char,
                    1000,
                );
                R_Suicide(msg.as_ptr());
            }
            let needed_fds = ((reqlimit as f64) / 0.6).ceil() as c_int;
            let fdlimit = R_EnsureFDLimit(needed_fds);
            if fdlimit < 0 && reqlimit > 100 {
                let mut msg = [0i8; 128];
                snprintf_buf(
                    &mut msg,
                    b"R_MAX_NUM_DLLS cannot be bigger than %d when fd limit is not known\0".as_ptr()
                        as *const c_char,
                    100,
                );
                R_Suicide(msg.as_ptr());
            } else if fdlimit >= 0 && fdlimit < needed_fds {
                let maxdlllimit = (0.6 * fdlimit as f64) as c_int;
                if maxdlllimit < 100 {
                    R_Suicide(
                        b"the limit on the number of open files is too low\0".as_ptr()
                            as *const c_char,
                    );
                }
                let mut msg = [0i8; 128];
                snprintf_buf(
                    &mut msg,
                    b"R_MAX_NUM_DLLS bigger than %d may exhaust open files limit\0".as_ptr()
                        as *const c_char,
                    maxdlllimit,
                );
                R_Suicide(msg.as_ptr());
            }
            MaxNumDLLs = reqlimit;
        } else {
            let needed_fds: c_int = 1024;
            let fdlimit = R_EnsureFDLimit(needed_fds);
            if fdlimit < 0 {
                MaxNumDLLs = 100;
            } else {
                MaxNumDLLs = (0.6 * fdlimit as f64) as c_int;
                if MaxNumDLLs < 100 {
                    R_Suicide(
                        b"the limit on the number of open files is too low\0".as_ptr()
                            as *const c_char,
                    );
                }
            }
        }

        // Allocate the DLL table
        LoadedDLL = libc::calloc(MaxNumDLLs as usize, std::mem::size_of::<*mut DllInfo>())
            as *mut *mut DllInfo;
        if LoadedDLL.is_null() {
            R_Suicide(b"could not allocate space for DLL table\0".as_ptr() as *const c_char);
        }

        // Create DLLInfoEptrs and SymbolEptrs R objects
        // TODO: These need proper SEXP allocation - using stub for now
        DLLInfoEptrs = ptr::null_mut();
        SymbolEptrs = ptr::null_mut();
    }
}

/// Simple snprintf helper.
unsafe fn snprintf_ptr(buf: *mut c_char, size: usize, fmt: *const c_char, val: c_int) {
    unsafe {
        libc::snprintf(buf, size, fmt, val);
    }
}

/// Helper to pass &[u8] array to snprintf_ptr via as_mut_ptr.
#[inline]
unsafe fn snprintf_buf(buf: &mut [c_char], fmt: *const c_char, val: c_int) {
    unsafe {
        snprintf_ptr(buf.as_mut_ptr(), buf.len(), fmt, val);
    }
}

/// Simple atoi helper.
unsafe fn atoi(s: *const c_char) -> c_int {
    unsafe {
        let cstr = CStr::from_ptr(s);
        match cstr.to_str() {
            Ok(s) => s.trim().parse::<c_int>().unwrap_or(0),
            Err(_) => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// addDLL / DeleteDLL
// ---------------------------------------------------------------------------

/// Add a DLL to the table. Returns the index or 0 on failure.
unsafe fn addDLL(dpath: *mut c_char, DLLname: *mut c_char, handle: *mut c_void) -> c_int {
    unsafe {
        let ans = CountDLL;
        let name = libc::malloc(libc::strlen(DLLname) + 1) as *mut c_char;
        if name.is_null() {
            libc::strcpy(
                std::ptr::addr_of_mut!(DLLerror) as *mut c_char,
                b"could not allocate space for 'name'\0".as_ptr() as *const c_char,
            );
            if !handle.is_null() {
                if let Some(f) = (*R_osDynSymbol_ptr).closeLibrary {
                    f(handle);
                }
            }
            libc::free(dpath as *mut c_void);
            return 0;
        }
        libc::strcpy(name, DLLname);

        let info = libc::malloc(std::mem::size_of::<DllInfo>()) as *mut DllInfo;
        if info.is_null() {
            libc::strcpy(
                std::ptr::addr_of_mut!(DLLerror) as *mut c_char,
                b"could not allocate space for 'DllInfo'\0".as_ptr() as *const c_char,
            );
            if !handle.is_null() {
                if let Some(f) = (*R_osDynSymbol_ptr).closeLibrary {
                    f(handle);
                }
            }
            libc::free(name as *mut c_void);
            libc::free(dpath as *mut c_void);
            return 0;
        }

        libc::memset(info as *mut c_void, 0, std::mem::size_of::<DllInfo>());
        (*info).path = dpath;
        (*info).name = name;
        (*info).handle = handle;
        (*info).numCSymbols = 0;
        (*info).numCallSymbols = 0;
        (*info).numFortranSymbols = 0;
        (*info).numExternalSymbols = 0;
        (*info).CSymbols = ptr::null_mut();
        (*info).CallSymbols = ptr::null_mut();
        (*info).FortranSymbols = ptr::null_mut();
        (*info).ExternalSymbols = ptr::null_mut();

        *LoadedDLL.add(CountDLL as usize) = info;
        CountDLL += 1;

        ans
    }
}

/// Delete a DLL from the table. Returns 1 if found and removed, 0 otherwise.
unsafe fn DeleteDLL(path: *const c_char) -> c_int {
    unsafe {
        let mut loc: c_int = -1;
        for i in 0..CountDLL {
            let dll = *LoadedDLL.add(i as usize);
            if libc::strcmp(path, (*dll).path) == 0 {
                loc = i;
                break;
            }
        }
        if loc < 0 {
            return 0;
        }

        // Delete cached symbols (Windows-only, stub)
        if let Some(f) = (*R_osDynSymbol_ptr).deleteCachedSymbols {
            f(*LoadedDLL.add(loc as usize));
        }

        R_reinit_altrep_classes(*LoadedDLL.add(loc as usize));
        R_callDLLUnload(*LoadedDLL.add(loc as usize));

        if let Some(f) = (*R_osDynSymbol_ptr).closeLibrary {
            f((*(*LoadedDLL.add(loc as usize))).handle);
        }

        Rf_freeDllInfo(*LoadedDLL.add(loc as usize));

        // Compact the table
        for i in (loc + 1)..CountDLL {
            *LoadedDLL.add((i - 1) as usize) = *LoadedDLL.add(i as usize);
        }
        CountDLL -= 1;
        *LoadedDLL.add(CountDLL as usize) = ptr::null_mut();

        1
    }
}

// ---------------------------------------------------------------------------
// R_RegisterDLL / AddDLL (public-facing version)
// ---------------------------------------------------------------------------

/// Register a DLL with the given handle and path.
unsafe fn R_RegisterDLL(handle: *mut c_void, path: *const c_char) -> *mut DllInfo {
    unsafe {
        let dpath = libc::malloc(libc::strlen(path) + 1) as *mut c_char;
        if dpath.is_null() {
            libc::strcpy(
                std::ptr::addr_of_mut!(DLLerror) as *mut c_char,
                b"could not allocate space for 'path'\0".as_ptr() as *const c_char,
            );
            if let Some(f) = (*R_osDynSymbol_ptr).closeLibrary {
                f(handle);
            }
            return ptr::null_mut();
        }
        libc::strcpy(dpath, path);

        if let Some(f) = (*R_osDynSymbol_ptr).fixPath {
            f(dpath);
        }

        // Extract basename from path
        let mut p = libc::strrchr(dpath, RDYN_FILESEP[0] as libc::c_int);
        if p.is_null() {
            p = dpath;
        } else {
            p = p.add(1);
        }

        let mut DLLname = [0i8; R_PATH_MAX];
        if libc::strlen(p) < R_PATH_MAX {
            libc::strcpy(DLLname.as_mut_ptr(), p);
        } else {
            // TODO: error("DLLname '%s' is too long", p);
            return ptr::null_mut();
        }

        // Remove RDYN_SHLIB_EXT if present
        let ext_len = libc::strlen(RDYN_SHLIB_EXT.as_ptr() as *const c_char);
        let name_len = libc::strlen(DLLname.as_ptr() as *const c_char);
        if name_len >= ext_len {
            let pend = DLLname.as_mut_ptr().add(name_len - ext_len);
            if libc::strcmp(pend, RDYN_SHLIB_EXT.as_ptr() as *const c_char) == 0 {
                *pend = 0;
            }
        }

        if addDLL(dpath, DLLname.as_mut_ptr(), handle) != 0 {
            let info = *LoadedDLL.add((CountDLL - 1) as usize);
            (*info).useDynamicLookup = TRUE;
            (*info).forceSymbols = FALSE;
            return info;
        }
        ptr::null_mut()
    }
}

/// AddDLL: Load a shared library and register it.
unsafe fn AddDLL(
    path: *const c_char,
    asLocal: c_int,
    now: c_int,
    DLLsearchpath: *const c_char,
) -> *mut DllInfo {
    unsafe {
        // Check if already loaded
        let mut loc: c_int = -1;
        for i in 0..CountDLL {
            let dll = *LoadedDLL.add(i as usize);
            if libc::strcmp(path, (*dll).path) == 0 {
                loc = i;
                break;
            }
        }

        if loc >= 0 {
            // Already loaded, move to head
            let info = *LoadedDLL.add(loc as usize);
            for i in (loc + 1)..CountDLL {
                *LoadedDLL.add((i - 1) as usize) = *LoadedDLL.add(i as usize);
            }
            *LoadedDLL.add((CountDLL - 1) as usize) = info;
            return info;
        }

        if CountDLL == MaxNumDLLs {
            libc::strcpy(
                std::ptr::addr_of_mut!(DLLerror) as *mut c_char,
                b"maximal number of DLLs reached...\0".as_ptr() as *const c_char,
            );
            return ptr::null_mut();
        }

        let handle = if let Some(f) = (*R_osDynSymbol_ptr).loadLibrary {
            f(path, asLocal, now, DLLsearchpath)
        } else {
            ptr::null_mut()
        };

        if handle.is_null() {
            if let Some(f) = (*R_osDynSymbol_ptr).getError {
                f(
                    std::ptr::addr_of_mut!(DLLerror) as *mut c_char,
                    DLLERR_BUFSIZE as c_int,
                );
            }
            return ptr::null_mut();
        }

        let info = R_RegisterDLL(handle, path);

        if !info.is_null() {
            // Look for R_init_<name> initialization routine
            let nm = (*info).name;
            let len = libc::strlen(nm) + 9;
            let mut tmp = vec![0i8; len];
            // On Unix with ELF, no leading underscore
            libc::snprintf(
                tmp.as_mut_ptr(),
                len,
                b"R_init_%s\0".as_ptr() as *const c_char,
                nm,
            );

            let mut f: Option<unsafe extern "C" fn(*mut DllInfo)> = None;
            if let Some(dlsym_fn) = (*R_osDynSymbol_ptr).dlsym_fn {
                let func = dlsym_fn(info, tmp.as_ptr());
                if let Some(func_raw) = func {
                    f = Some(std::mem::transmute(func_raw));
                }
            }

            // Try with dots replaced by underscores
            if f.is_none() {
                for ch in tmp.iter_mut() {
                    if *ch == b'.' as i8 {
                        *ch = b'_' as i8;
                    }
                }
                if let Some(dlsym_fn) = (*R_osDynSymbol_ptr).dlsym_fn {
                    let func = dlsym_fn(info, tmp.as_ptr());
                    if let Some(func_raw) = func {
                        f = Some(std::mem::transmute(func_raw));
                    }
                }
            }

            if let Some(init_fn) = f {
                init_fn(info);
            }
        }

        info
    }
}

// ---------------------------------------------------------------------------
// R_registerRoutines
// ---------------------------------------------------------------------------

/// Register native routines for a DllInfo object.
// no_mangle removed (duplicate)
pub unsafe extern "C" fn R_registerRoutines(
    info: *mut DllInfo,
    croutines: *const R_CMethodDef,
    callRoutines: *const R_CallMethodDef,
    fortranRoutines: *const R_FortranMethodDef,
    externalRoutines: *const R_ExternalMethodDef,
) -> c_int {
    unsafe {
        if info.is_null() {
            // error("R_RegisterRoutines called with invalid DllInfo object.");
            return 0;
        }

        (*info).useDynamicLookup = if !(*info).handle.is_null() {
            TRUE
        } else {
            FALSE
        };
        (*info).forceSymbols = FALSE;

        if !croutines.is_null() {
            let mut num: c_int = 0;
            let mut p = croutines;
            while !(*p).name.is_null() {
                num += 1;
                p = p.add(1);
            }
            (*info).CSymbols = libc::calloc(num as usize, std::mem::size_of::<Rf_DotCSymbol>())
                as *mut Rf_DotCSymbol;
            if (*info).CSymbols.is_null() {
                return 0;
            }
            (*info).numCSymbols = num;
            for i in 0..num {
                R_addCRoutine(
                    info,
                    croutines.add(i as usize),
                    (*info).CSymbols.add(i as usize),
                );
            }
        }

        if !fortranRoutines.is_null() {
            let mut num: c_int = 0;
            let mut p = fortranRoutines;
            while !(*p).name.is_null() {
                num += 1;
                p = p.add(1);
            }
            (*info).FortranSymbols =
                libc::calloc(num as usize, std::mem::size_of::<Rf_DotFortranSymbol>())
                    as *mut Rf_DotFortranSymbol;
            if (*info).FortranSymbols.is_null() {
                return 0;
            }
            (*info).numFortranSymbols = num;
            for i in 0..num {
                R_addFortranRoutine(
                    info,
                    fortranRoutines.add(i as usize),
                    (*info).FortranSymbols.add(i as usize),
                );
            }
        }

        if !callRoutines.is_null() {
            let mut num: c_int = 0;
            let mut p = callRoutines;
            while !(*p).name.is_null() {
                num += 1;
                p = p.add(1);
            }
            (*info).CallSymbols =
                libc::calloc(num as usize, std::mem::size_of::<Rf_DotCallSymbol>())
                    as *mut Rf_DotCallSymbol;
            if (*info).CallSymbols.is_null() {
                return 0;
            }
            (*info).numCallSymbols = num;
            for i in 0..num {
                R_addCallRoutine(
                    info,
                    callRoutines.add(i as usize),
                    (*info).CallSymbols.add(i as usize),
                );
            }
        }

        if !externalRoutines.is_null() {
            let mut num: c_int = 0;
            let mut p = externalRoutines;
            while !(*p).name.is_null() {
                num += 1;
                p = p.add(1);
            }
            (*info).ExternalSymbols =
                libc::calloc(num as usize, std::mem::size_of::<Rf_DotExternalSymbol>())
                    as *mut Rf_DotExternalSymbol;
            if (*info).ExternalSymbols.is_null() {
                return 0;
            }
            (*info).numExternalSymbols = num;
            for i in 0..num {
                R_addExternalRoutine(
                    info,
                    externalRoutines.add(i as usize),
                    (*info).ExternalSymbols.add(i as usize),
                );
            }
        }

        1
    }
}

// ---------------------------------------------------------------------------
// R_useDynamicSymbols / R_forceSymbols
// ---------------------------------------------------------------------------

/// Set whether dynamic lookup should be used for a DllInfo.
// no_mangle removed (duplicate)
pub unsafe extern "C" fn R_useDynamicSymbols(info: *mut DllInfo, value: Rboolean) -> Rboolean {
    unsafe {
        let old = (*info).useDynamicLookup;
        (*info).useDynamicLookup = value;
        old
    }
}

/// Set whether only registered symbols should be used.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_forceSymbols(info: *mut DllInfo, value: Rboolean) -> Rboolean {
    unsafe {
        let old = (*info).forceSymbols;
        (*info).forceSymbols = value;
        old
    }
}

// ---------------------------------------------------------------------------
// Routine registration helpers
// ---------------------------------------------------------------------------

unsafe fn R_addCRoutine(
    _info: *mut DllInfo,
    croutine: *const R_CMethodDef,
    sym: *mut Rf_DotCSymbol,
) {
    unsafe {
        (*sym).name = Rstrdup((*croutine).name);
        (*sym).fun = (*croutine).fun;
        (*sym).numArgs = if (*croutine).numArgs > -1 {
            (*croutine).numArgs
        } else {
            -1
        };
        if !(*croutine).types.is_null() {
            R_setPrimitiveArgTypes_c(croutine, sym);
        }
    }
}

unsafe fn R_addCallRoutine(
    _info: *mut DllInfo,
    croutine: *const R_CallMethodDef,
    sym: *mut Rf_DotCallSymbol,
) {
    unsafe {
        (*sym).name = Rstrdup((*croutine).name);
        (*sym).fun = (*croutine).fun;
        (*sym).numArgs = if (*croutine).numArgs > -1 {
            (*croutine).numArgs
        } else {
            -1
        };
    }
}

unsafe fn R_addFortranRoutine(
    _info: *mut DllInfo,
    croutine: *const R_FortranMethodDef,
    sym: *mut Rf_DotFortranSymbol,
) {
    unsafe {
        (*sym).name = Rstrdup((*croutine).name);
        (*sym).fun = (*croutine).fun;
        (*sym).numArgs = if (*croutine).numArgs > -1 {
            (*croutine).numArgs
        } else {
            -1
        };
        if !(*croutine).types.is_null() {
            R_setPrimitiveArgTypes_f(croutine, sym);
        }
    }
}

unsafe fn R_addExternalRoutine(
    _info: *mut DllInfo,
    croutine: *const R_ExternalMethodDef,
    sym: *mut Rf_DotExternalSymbol,
) {
    unsafe {
        (*sym).name = Rstrdup((*croutine).name);
        (*sym).fun = (*croutine).fun;
        (*sym).numArgs = if (*croutine).numArgs > -1 {
            (*croutine).numArgs
        } else {
            -1
        };
    }
}

unsafe fn R_setPrimitiveArgTypes_c(croutine: *const R_CMethodDef, sym: *mut Rf_DotCSymbol) {
    unsafe {
        let n = (*croutine).numArgs as usize;
        (*sym).types = libc::malloc(std::mem::size_of::<R_NativePrimitiveArgType>() * n)
            as *mut R_NativePrimitiveArgType;
        if !(*sym).types.is_null() {
            libc::memcpy(
                (*sym).types as *mut c_void,
                (*croutine).types as *const c_void,
                std::mem::size_of::<R_NativePrimitiveArgType>() * n,
            );
        }
    }
}

unsafe fn R_setPrimitiveArgTypes_f(
    croutine: *const R_FortranMethodDef,
    sym: *mut Rf_DotFortranSymbol,
) {
    unsafe {
        let n = (*croutine).numArgs as usize;
        (*sym).types = libc::malloc(std::mem::size_of::<R_NativePrimitiveArgType>() * n)
            as *mut R_NativePrimitiveArgType;
        if !(*sym).types.is_null() {
            libc::memcpy(
                (*sym).types as *mut c_void,
                (*croutine).types as *const c_void,
                std::mem::size_of::<R_NativePrimitiveArgType>() * n,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

unsafe fn Rf_freeCSymbol(sym: *mut Rf_DotCSymbol) {
    unsafe {
        libc::free((*sym).name as *mut c_void);
    }
}

unsafe fn Rf_freeCallSymbol(sym: *mut Rf_DotCallSymbol) {
    unsafe {
        libc::free((*sym).name as *mut c_void);
    }
}

unsafe fn Rf_freeExternalSymbol(sym: *mut Rf_DotExternalSymbol) {
    unsafe {
        libc::free((*sym).name as *mut c_void);
    }
}

unsafe fn Rf_freeFortranSymbol(sym: *mut Rf_DotFortranSymbol) {
    unsafe {
        libc::free((*sym).name as *mut c_void);
    }
}

unsafe fn Rf_freeDllInfo(info: *mut DllInfo) {
    unsafe {
        if info.is_null() {
            return;
        }
        libc::free((*info).name as *mut c_void);
        libc::free((*info).path as *mut c_void);
        if !(*info).CSymbols.is_null() {
            for i in 0..(*info).numCSymbols {
                Rf_freeCSymbol((*info).CSymbols.add(i as usize));
            }
            libc::free((*info).CSymbols as *mut c_void);
        }
        if !(*info).CallSymbols.is_null() {
            for i in 0..(*info).numCallSymbols {
                Rf_freeCallSymbol((*info).CallSymbols.add(i as usize));
            }
            libc::free((*info).CallSymbols as *mut c_void);
        }
        if !(*info).ExternalSymbols.is_null() {
            for i in 0..(*info).numExternalSymbols {
                Rf_freeExternalSymbol((*info).ExternalSymbols.add(i as usize));
            }
            libc::free((*info).ExternalSymbols as *mut c_void);
        }
        if !(*info).FortranSymbols.is_null() {
            for i in 0..(*info).numFortranSymbols {
                Rf_freeFortranSymbol((*info).FortranSymbols.add(i as usize));
            }
            libc::free((*info).FortranSymbols as *mut c_void);
        }
        libc::free(info as *mut c_void);
    }
}

// ---------------------------------------------------------------------------
// R_callDLLUnload
// ---------------------------------------------------------------------------

type DllInfoUnloadCall = Option<unsafe extern "C" fn(*mut DllInfo)>;

/// Call R_unload_<name> if it exists in the DLL.
unsafe fn R_callDLLUnload(dllInfo: *mut DllInfo) {
    unsafe {
        let mut buf = [0i8; 1024];
        libc::snprintf(
            buf.as_mut_ptr(),
            1024,
            b"R_unload_%s\0".as_ptr() as *const c_char,
            (*dllInfo).name,
        );

        let mut symbol = std::mem::zeroed::<R_RegisteredNativeSymbol>();
        symbol.type_ = NativeSymbolType::R_ANY_SYM;

        let f = R_dlsym(dllInfo, buf.as_ptr(), &mut symbol);
        if let Some(unload_fn) = f {
            let typed_fn: DllInfoUnloadCall = std::mem::transmute(unload_fn);
            if let Some(call) = typed_fn {
                call(dllInfo);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Symbol lookup
// ---------------------------------------------------------------------------

/// Look up a registered C symbol.
unsafe fn Rf_lookupRegisteredCSymbol(
    info: *mut DllInfo,
    name: *const c_char,
) -> *mut Rf_DotCSymbol {
    unsafe {
        for i in 0..(*info).numCSymbols {
            if libc::strcmp(name, (*(*info).CSymbols.add(i as usize)).name) == 0 {
                return (*info).CSymbols.add(i as usize);
            }
        }
        ptr::null_mut()
    }
}

/// Look up a registered Fortran symbol.
unsafe fn Rf_lookupRegisteredFortranSymbol(
    info: *mut DllInfo,
    name: *const c_char,
) -> *mut Rf_DotFortranSymbol {
    unsafe {
        for i in 0..(*info).numFortranSymbols {
            if libc::strcmp(name, (*(*info).FortranSymbols.add(i as usize)).name) == 0 {
                return (*info).FortranSymbols.add(i as usize);
            }
        }
        ptr::null_mut()
    }
}

/// Look up a registered Call symbol.
unsafe fn Rf_lookupRegisteredCallSymbol(
    info: *mut DllInfo,
    name: *const c_char,
) -> *mut Rf_DotCallSymbol {
    unsafe {
        for i in 0..(*info).numCallSymbols {
            if libc::strcmp(name, (*(*info).CallSymbols.add(i as usize)).name) == 0 {
                return (*info).CallSymbols.add(i as usize);
            }
        }
        ptr::null_mut()
    }
}

/// Look up a registered External symbol.
unsafe fn Rf_lookupRegisteredExternalSymbol(
    info: *mut DllInfo,
    name: *const c_char,
) -> *mut Rf_DotExternalSymbol {
    unsafe {
        for i in 0..(*info).numExternalSymbols {
            if libc::strcmp(name, (*(*info).ExternalSymbols.add(i as usize)).name) == 0 {
                return (*info).ExternalSymbols.add(i as usize);
            }
        }
        ptr::null_mut()
    }
}

/// Look up a registered symbol in a DllInfo.
unsafe fn R_getDLLRegisteredSymbol(
    info: *mut DllInfo,
    name: *const c_char,
    symbol: *mut R_RegisteredNativeSymbol,
) -> DL_FUNC {
    unsafe {
        let purpose = if symbol.is_null() {
            NativeSymbolType::R_ANY_SYM
        } else {
            (*symbol).type_
        };

        if (purpose == NativeSymbolType::R_ANY_SYM || purpose == NativeSymbolType::R_C_SYM)
            && (*info).numCSymbols > 0
        {
            let sym = Rf_lookupRegisteredCSymbol(info, name);
            if !sym.is_null() {
                if !symbol.is_null() {
                    (*symbol).type_ = NativeSymbolType::R_C_SYM;
                    (*symbol).symbol.c = sym;
                    (*symbol).dll = info;
                }
                return (*sym).fun;
            }
        }

        if (purpose == NativeSymbolType::R_ANY_SYM || purpose == NativeSymbolType::R_CALL_SYM)
            && (*info).numCallSymbols > 0
        {
            let sym = Rf_lookupRegisteredCallSymbol(info, name);
            if !sym.is_null() {
                if !symbol.is_null() {
                    (*symbol).type_ = NativeSymbolType::R_CALL_SYM;
                    (*symbol).symbol.call = sym;
                    (*symbol).dll = info;
                }
                return (*sym).fun;
            }
        }

        if (purpose == NativeSymbolType::R_ANY_SYM || purpose == NativeSymbolType::R_FORTRAN_SYM)
            && (*info).numFortranSymbols > 0
        {
            let sym = Rf_lookupRegisteredFortranSymbol(info, name);
            if !sym.is_null() {
                if !symbol.is_null() {
                    (*symbol).type_ = NativeSymbolType::R_FORTRAN_SYM;
                    (*symbol).symbol.fortran = sym;
                    (*symbol).dll = info;
                }
                return (*sym).fun;
            }
        }

        if (purpose == NativeSymbolType::R_ANY_SYM || purpose == NativeSymbolType::R_EXTERNAL_SYM)
            && (*info).numExternalSymbols > 0
        {
            let sym = Rf_lookupRegisteredExternalSymbol(info, name);
            if !sym.is_null() {
                if !symbol.is_null() {
                    (*symbol).type_ = NativeSymbolType::R_EXTERNAL_SYM;
                    (*symbol).symbol.external = sym;
                    (*symbol).dll = info;
                }
                return (*sym).fun;
            }
        }

        None
    }
}

/// R_dlsym: look up a symbol in a specific DLL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_dlsym(
    info: *mut DllInfo,
    name: *const c_char,
    symbol: *mut R_RegisteredNativeSymbol,
) -> DL_FUNC {
    unsafe {
        let len = libc::strlen(name) + 4;
        let mut buf = vec![0i8; len];

        let f = R_getDLLRegisteredSymbol(info, name, symbol);
        if f.is_some() {
            return f;
        }

        if (*info).useDynamicLookup == FALSE {
            return None;
        }

        // On Unix with ELF, no leading underscore
        libc::snprintf(
            buf.as_mut_ptr(),
            len,
            b"%s\0".as_ptr() as *const c_char,
            name,
        );

        let mut f = None;
        if let Some(dlsym_fn) = (*R_osDynSymbol_ptr).dlsym_fn {
            f = dlsym_fn(info, buf.as_ptr());
        }

        if f.is_some() && !symbol.is_null() {
            (*symbol).dll = info;
        }
        f
    }
}

/// R_FindSymbol: look up a symbol across all loaded DLLs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FindSymbol(
    name: *const c_char,
    pkg: *const c_char,
    symbol: *mut R_RegisteredNativeSymbol,
) -> DL_FUNC {
    unsafe {
        let all = libc::strlen(pkg) == 0;
        let all_int: c_int = if all { 1 } else { 0 };

        // Check cache first
        if let Some(lookup_fn) = (*R_osDynSymbol_ptr).lookupCachedSymbol {
            let mut dll: *mut DllInfo = ptr::null_mut();
            let fcnptr = lookup_fn(name, pkg, all_int, &mut dll);
            if fcnptr.is_some() {
                if !symbol.is_null() && !dll.is_null() {
                    (*symbol).dll = dll;
                }
                return fcnptr;
            }
        }

        let mut i: c_int = CountDLL - 1;
        while i >= 0 {
            let mut doit: c_int = all_int;
            if doit == 0 && libc::strcmp(pkg, (*(*LoadedDLL.add(i as usize))).name) == 0 {
                doit = 2;
            }
            if doit != 0 && (*(*LoadedDLL.add(i as usize))).forceSymbols != 0 {
                doit = 0;
            }
            if doit != 0 {
                let fcnptr = R_dlsym(*LoadedDLL.add(i as usize), name, symbol);
                if fcnptr.is_some() {
                    if !symbol.is_null() {
                        (*symbol).dll = *LoadedDLL.add(i as usize);
                    }
                    return fcnptr;
                }
            }
            if doit > 1 {
                return None;
            }
            i -= 1;
        }

        None
    }
}

// ---------------------------------------------------------------------------
// R_getDllInfo / R_getDllIndex / R_getEmbeddingDllInfo
// ---------------------------------------------------------------------------

/// Look up a DllInfo by path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_getDllInfo(path: *const c_char) -> *mut DllInfo {
    unsafe {
        for i in 0..CountDLL {
            if libc::strcmp(path, (*(*LoadedDLL.add(i as usize))).path) == 0 {
                return *LoadedDLL.add(i as usize);
            }
        }
        ptr::null_mut()
    }
}

/// Get the index of a DllInfo in the loaded DLL table.
unsafe fn R_getDllIndex(info: *mut DllInfo) -> c_int {
    unsafe {
        for i in 0..CountDLL {
            if *LoadedDLL.add(i as usize) == info {
                return i;
            }
        }
        -1
    }
}

/// Get (or create) the embedding DllInfo.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_getEmbeddingDllInfo() -> *mut DllInfo {
    unsafe {
        let dll = R_getDllInfo(b"(embedding)\0".as_ptr() as *const c_char);
        if dll.is_null() {
            let which = addDLL(
                Rstrdup(b"(embedding)\0".as_ptr() as *const c_char),
                b"(embedding)\0".as_ptr() as *mut c_char,
                ptr::null_mut(),
            );
            let dll2 = *LoadedDLL.add(which as usize);
            R_useDynamicSymbols(dll2, FALSE);
            return dll2;
        }
        dll
    }
}

// ---------------------------------------------------------------------------
// R_moduleCdynload / R_cairoCdynload
// ---------------------------------------------------------------------------

/// Load a module DLL from the R modules directory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_moduleCdynload(
    module: *const c_char,
    local: c_int,
    now: c_int,
) -> c_int {
    unsafe {
        let mut dllpath = [0i8; R_PATH_MAX];
        let p = libc::getenv(b"R_HOME\0".as_ptr() as *const c_char);
        if p.is_null() {
            return 0;
        }
        libc::snprintf(
            dllpath.as_mut_ptr(),
            R_PATH_MAX,
            b"%s/modules/%s%s\0".as_ptr() as *const c_char,
            p,
            module,
            RDYN_SHLIB_EXT.as_ptr() as *const c_char,
        );
        let res = AddDLL(
            dllpath.as_ptr(),
            local,
            now,
            b"\0".as_ptr() as *const c_char,
        );
        if res.is_null() {
            // warning("unable to load shared object '%s':\n  %s", dllpath, DLLerror);
        }
        if res.is_null() { 0 } else { 1 }
    }
}

/// Load the Cairo module DLL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_cairoCdynload(local: c_int, now: c_int) -> c_int {
    unsafe {
        let mut dllpath = [0i8; R_PATH_MAX];
        let p = libc::getenv(b"R_HOME\0".as_ptr() as *const c_char);
        if p.is_null() {
            return 0;
        }
        libc::snprintf(
            dllpath.as_mut_ptr(),
            R_PATH_MAX,
            b"%s/library/grDevices/libs/%s%s\0".as_ptr() as *const c_char,
            p,
            b"cairo\0".as_ptr() as *const c_char,
            RDYN_SHLIB_EXT.as_ptr() as *const c_char,
        );
        let res = AddDLL(
            dllpath.as_ptr(),
            local,
            now,
            b"\0".as_ptr() as *const c_char,
        );
        if res.is_null() {
            // warning("unable to load shared object '%s':\n  %s", dllpath, DLLerror);
        }
        if res.is_null() { 0 } else { 1 }
    }
}

// ---------------------------------------------------------------------------
// R_registerSymbolEptr
// ---------------------------------------------------------------------------

/// Register a symbol external pointer in the weak reference list.
unsafe fn R_registerSymbolEptr(_eptr: SEXP, _einfo: SEXP) {
    // TODO: This requires full SEXP manipulation (CONS, SETCDR, etc.)
    // Stub for now since the weakref infrastructure is not yet fully ported
}

// ---------------------------------------------------------------------------
// do_dynload / do_dynunload
// ---------------------------------------------------------------------------

/// .Internal(dynload(...))
pub unsafe fn do_dynload(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let buf = [0i8; 2 * R_PATH_MAX];
        checkArity(op, args);

        // Check that first arg is a single string
        // if (!isString(CAR(args)) || LENGTH(CAR(args)) != 1) error(...);
        // GetFullDLLPath stub
        // TODO: implement full argument checking

        let info = AddDLL(buf.as_ptr(), 0, 0, b"\0".as_ptr() as *const c_char);
        if info.is_null() {
            // error("unable to load shared object '%s':\n  %s", buf, DLLerror);
        }

        ptr::null_mut() // TODO: return Rf_MakeDLLInfo(info)
    }
}

/// .Internal(dynunload(...))
pub unsafe fn do_dynunload(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let buf = [0i8; 2 * R_PATH_MAX];
        checkArity(op, args);

        if DeleteDLL(buf.as_ptr()) == 0 {
            // error("shared object '%s' was not loaded", buf);
        }

        ptr::null_mut() // R_NilValue
    }
}

// ---------------------------------------------------------------------------
// getNativeSymbolInfo / getLoadedDLLs / getRegisteredRoutines
// ---------------------------------------------------------------------------

/// R_getSymbolInfo: resolve a native symbol by name and package.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_getSymbolInfo(
    sname: SEXP,
    spackage: SEXP,
    withRegistrationInfo: SEXP,
) -> SEXP {
    // TODO: full implementation requires SEXP manipulation
    ptr::null_mut()
}

/// R_getDllTable: return the list of all loaded DLLs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_getDllTable() -> SEXP {
    // TODO: full implementation requires SEXP manipulation
    ptr::null_mut()
}

/// R_getRegisteredRoutines: get registered routines for a DLL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_getRegisteredRoutines(dll: SEXP) -> SEXP {
    // TODO: full implementation requires SEXP manipulation
    ptr::null_mut()
}

/// do_getSymbolInfo: .Internal(getNativeSymbolInfo(...))
pub unsafe fn do_getSymbolInfo(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        ptr::null_mut() // TODO: return R_getSymbolInfo(CAR(args), CADR(args), CADDR(args))
    }
}

/// do_getDllTable: .Internal(getLoadedDLLs())
pub unsafe fn do_getDllTable(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        R_getDllTable()
    }
}

/// do_getRegisteredRoutines: .Internal(getRegisteredRoutines(...))
pub unsafe fn do_getRegisteredRoutines(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        checkArity(op, args);
        ptr::null_mut() // TODO: return R_getRegisteredRoutines(CAR(args))
    }
}

// ---------------------------------------------------------------------------
// R_RegisterCCallable / R_GetCCallable
// ---------------------------------------------------------------------------

/// Get or create the per-package CEntry table environment.
unsafe fn get_package_CEntry_table(package: *const c_char) -> SEXP {
    unsafe {
        if CEntryTable.is_null() {
            // CEntryTable = R_NewHashedEnv(R_NilValue, 0);
            // R_PreserveObject(CEntryTable);
            CEntryTable = ptr::null_mut(); // TODO
        }
        let pname = install(package);
        // let penv = R_findVarInFrame(CEntryTable, pname);
        // TODO: full implementation
        ptr::null_mut()
    }
}

/// Register a C-callable function for use from other packages.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RegisterCCallable(
    package: *const c_char,
    name: *const c_char,
    fptr: DL_FUNC,
) {
    unsafe {
        let _penv = get_package_CEntry_table(package);
        // TODO: full implementation requires SEXP manipulation
        // PROTECT(penv);
        // SEXP eptr = R_MakeExternalPtrFn(fptr, R_NilValue, R_NilValue);
        // PROTECT(eptr);
        // defineVar(install(name), eptr, penv);
        // UNPROTECT(2);
    }
}

/// Look up a C-callable function registered by another package.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetCCallable(package: *const c_char, name: *const c_char) -> DL_FUNC {
    unsafe {
        let _penv = get_package_CEntry_table(package);
        // TODO: full implementation requires SEXP manipulation
        None
    }
}

// ---------------------------------------------------------------------------
// Rf_registerRoutines (SEXP-based version)
// ---------------------------------------------------------------------------

/// Register routines from R-level symbol objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_registerRoutines(sSymbolList: SEXP) -> SEXP {
    // TODO: full implementation requires extensive SEXP manipulation
    // This is the R-level registration path, less critical for the Rust port
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Rf_lookupCachedSymbol / Rf_deleteCachedSymbols
// ---------------------------------------------------------------------------

/// Look up a cached symbol (Windows-only).
// no_mangle removed (duplicate)
pub unsafe extern "C" fn Rf_lookupCachedSymbol(
    name: *const c_char,
    pkg: *const c_char,
    _all: c_int,
    dll: *mut *mut DllInfo,
) -> DL_FUNC {
    // CACHE_DLL_SYM is Windows-only
    None
}

/// Delete cached symbols for a DLL (Windows-only).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_deleteCachedSymbols(_dll: *mut DllInfo) {
    // CACHE_DLL_SYM is Windows-only
}

// ---------------------------------------------------------------------------
// R_getRoutineSymbols (internal helper)
// ---------------------------------------------------------------------------

unsafe fn R_getRoutineSymbols(_type: NativeSymbolType, _info: *mut DllInfo) -> SEXP {
    // TODO: full implementation
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// freeRegisteredNativeSymbolCopy (internal helper)
// ---------------------------------------------------------------------------

unsafe fn freeRegisteredNativeSymbolCopy(_ref: SEXP) {
    // TODO: full implementation
}

// ---------------------------------------------------------------------------
// createRSymbolObject (internal helper)
// ---------------------------------------------------------------------------

unsafe fn createRSymbolObject(
    _sname: SEXP,
    _f: DL_FUNC,
    _symbol: *mut R_RegisteredNativeSymbol,
    _withRegistrationInfo: bool,
) -> SEXP {
    // TODO: full implementation requires extensive SEXP manipulation
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// getSymbolComponent (internal helper)
// ---------------------------------------------------------------------------

unsafe fn getSymbolComponent(
    _sSym: SEXP,
    _name: *const c_char,
    _type: SEXPTYPE,
    _optional: c_int,
) -> SEXP {
    // TODO: full implementation
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Rf_MakeDLLInfo / Rf_MakeNativeSymbolRef / Rf_MakeRegisteredNativeSymbol
// ---------------------------------------------------------------------------

unsafe fn Rf_MakeNativeSymbolRef(_f: DL_FUNC) -> SEXP {
    // TODO: full implementation
    ptr::null_mut()
}

unsafe fn Rf_MakeRegisteredNativeSymbol(_symbol: *mut R_RegisteredNativeSymbol) -> SEXP {
    // TODO: full implementation
    ptr::null_mut()
}

unsafe fn Rf_MakeDLLInfo(_info: *mut DllInfo) -> SEXP {
    // TODO: full implementation
    ptr::null_mut()
}

unsafe fn Rf_makeDllObject(_inst: *mut c_void) -> SEXP {
    // TODO: full implementation
    ptr::null_mut()
}

unsafe fn Rf_makeDllInfoReference(_info: *mut DllInfo) -> SEXP {
    // TODO: full implementation
    ptr::null_mut()
}
