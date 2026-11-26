#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/Rdynload.c — dynamic library loading and native symbol registration.
//!
//! Manages the loaded-DLL table, symbol registration for .C/.Call/.Fortran/.External,
//! symbol lookup (registered first, then dynamic), and the R-level interfaces
//! `do_dynload`, `do_dynunload`, `getSymbolInfo`, `getLoadedDLLs`, etc.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::rc::Rc;

mod registered;
use registered::{Interface, Registry, Routine};

use crate::mainutils::memory_main::{R_ExternalPtrAddr, R_MakeExternalPtr};
use crate::mainutils::relop::checkArity;
use crate::sexp::accessors::{
    CAR, CDR, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TYPEOF, translateChar,
};
use crate::sexp::attrib_core::{R_ClassSymbol, R_NamesSymbol, setAttrib};
use crate::sexp::constructors::{
    Rf_ScalarLogical, Rf_allocVector, Rf_isString, Rf_length, Rf_mkChar,
};
use crate::sexp::envir::{R_NewHashedEnv, R_findVarInFrame, defineVar};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::symbol::Rf_install;
use crate::unix::dynload::{DL_FUNC, InitFunctionHashing};

// ---------------------------------------------------------------------------
// Local error helper
// ---------------------------------------------------------------------------

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

fn errorcall(_call: SEXP, msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

fn native_dynamic_loading_policy_error(entrypoint: &str) -> ! {
    {
        error(&format!(
            "{entrypoint} loads foreign native code, which cannot exist in the pure-Rust runtime; use bundled pure-R packages"
        ))
    }
}

fn native_dynamic_loading_policy_enabled() -> bool {
    !native_extensions_enabled()
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

// ---------------------------------------------------------------------------
// DllInfo — tracks a loaded shared library and its registered symbols
// ---------------------------------------------------------------------------

/// Native extension metadata behind the opaque C `DllInfo *` handle.
#[repr(C)]
pub struct DllInfo {
    path: CString,
    name: CString,
    handle: *mut c_void,
    use_dynamic_lookup: Cell<bool>,
    force_symbols: Cell<bool>,

    registrations: RefCell<Registry>,
}

impl DllInfo {
    fn new(
        path: CString,
        name: CString,
        handle: *mut c_void,
        use_dynamic_lookup: bool,
    ) -> Self {
        DllInfo {
            path,
            name,
            handle,
            use_dynamic_lookup: Cell::new(use_dynamic_lookup),
            force_symbols: Cell::new(false),
            registrations: RefCell::default(),
        }
    }
}

/// Physical DLL storage is retained by each resolved invocation. Removal from
/// the visible registry does not invalidate its metadata or unmap executing
/// code. Final destruction never calls an R unload hook.
struct LoadedDll {
    info: DllInfo,
}
impl LoadedDll {
    fn new(info: DllInfo) -> Rc<Self> {
        Rc::new(Self { info })
    }
    /// Stable `Rc`-owned address; the mutable spelling matches the opaque C
    /// handle ABI. Mutation goes through the interior `Cell`/`RefCell` fields.
    fn pointer(&self) -> *mut DllInfo {
        ptr::from_ref(&self.info).cast_mut()
    }
}
impl Drop for LoadedDll {
    fn drop(&mut self) {
        let handle = self.info.handle;
        if !handle.is_null() {
            // SAFETY: `handle` is owned by this record and this is its final
            // lease; no unload hook runs at drop.
            #[cfg(not(target_arch = "wasm32"))]
            unsafe {
                dlclose(handle);
            }
            #[cfg(target_arch = "wasm32")]
            crate::unix::dynload::dlclose(handle);
        }
    }
}

#[derive(Clone)]
pub(crate) struct ResolvedForeignRoutine {
    function: DL_FUNC,
    declaration: Option<Routine>,
    _library: Rc<LoadedDll>,
}
impl ResolvedForeignRoutine {
    pub(crate) fn function(&self) -> DL_FUNC {
        self.function
    }
    pub(crate) fn validate(&self, interface: c_int, count: usize) -> Result<(), String> {
        self.declaration
            .as_ref()
            .map_or(Ok(()), |routine| routine.validate(interface, count))
    }
    pub(crate) fn argument_types(&self) -> Option<&[c_int]> {
        self.declaration.as_ref()?.types.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Per-session DLL table
// ---------------------------------------------------------------------------

/// Dynamic-loader state owned by one `RInstance`.
///
/// Native library handles are still process-loader handles (`dlopen`/`dlsym`),
/// but R's visible registry, external-pointer caches, error buffer, and
/// inter-package C-callable table are session-local. This is the boundary that
/// lets Android host multiple independent R sessions without process-global R
/// registry mutation.
pub(crate) struct DynloadState {
    loaded_dll: Vec<Rc<LoadedDll>>,
    pub dll_info_eptrs: SEXP,
    pub symbol_eptrs: SEXP,
    pub dll_error: [u8; 4000],
    pub max_num_dlls: usize,
    pub c_entry_table: SEXP,
    /// Trusted desktop hosts may opt into process-native extensions. Mobile
    /// and WASM sessions remain deny-by-default.
    pub allow_native_extensions: bool,
}

impl Default for DynloadState {
    fn default() -> Self {
        DynloadState {
            loaded_dll: Vec::new(),
            dll_info_eptrs: ptr::null_mut(),
            symbol_eptrs: ptr::null_mut(),
            dll_error: [0; 4000],
            max_num_dlls: MAX_NUM_DLLS_DEFAULT,
            c_entry_table: ptr::null_mut(),
            allow_native_extensions: false,
        }
    }
}

#[inline]
fn with_dynload_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut DynloadState) -> R,
{
    with_required_current_instance(|instance| unsafe { f(&mut (*instance).dynload_state) })
}

pub(crate) fn set_native_extensions_enabled(enabled: bool) {
    with_dynload_state(|state| state.allow_native_extensions = enabled);
}

pub(crate) fn native_extensions_enabled() -> bool {
    with_dynload_state(|state| state.allow_native_extensions)
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
    let retired = with_dynload_state(|state| {
        let retired = std::mem::take(&mut state.loaded_dll);
        state.dll_info_eptrs = ptr::null_mut();
        state.symbol_eptrs = ptr::null_mut();
        state.max_num_dlls = MAX_NUM_DLLS_DEFAULT;
        retired
    });
    // OS loader destructors can execute host code. End the state loan first.
    drop(retired);
}

/// Called at R startup to initialize dynamic loading.
pub fn InitDynload() {
    init_loaded_dll();
    let base = add_dll(c"base", c"base");
    crate::mainutils::registration::R_init_base(base.pointer());
    InitFunctionHashing();
}

// ---------------------------------------------------------------------------
// add_dll — low-level insertion into the DLL table
// ---------------------------------------------------------------------------

fn find_by_path(path: &CStr) -> Option<Rc<LoadedDll>> {
    with_dynload_state(|state| {
        state
            .loaded_dll
            .iter()
            .find(|dll| dll.info.path.as_c_str() == path)
            .cloned()
    })
}

/// Add a DLL entry. For the base/embedding handles and tests only; real
/// libraries enter through `AddDLL`.
fn add_dll(path: &CStr, name: &CStr) -> Rc<LoadedDll> {
    let dll = LoadedDll::new(DllInfo::new(
        path.to_owned(),
        name.to_owned(),
        ptr::null_mut(),
        true,
    ));
    with_dynload_state(|state| state.loaded_dll.push(dll.clone()));
    dll
}

// ---------------------------------------------------------------------------
// Native loader FFI
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" {
    fn dlopen(path: *const c_char, flag: c_int) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
}

/// Look up a raw symbol address in a loaded library.
///
/// # Safety
/// `handle` must be a live `dlopen` handle for the call; `name` is a C string.
unsafe fn lookup_address(handle: *mut c_void, name: &CStr) -> *mut c_void {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // SAFETY: caller supplies live non-null handle, name is a C string.
        unsafe { dlsym(handle, name.as_ptr()) }
    }
    #[cfg(target_arch = "wasm32")]
    {
        crate::unix::dynload::dlsym(handle, name.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// R_getDllInfo / R_getDllIndex
// ---------------------------------------------------------------------------

/// Find a DllInfo by its path.
///
/// # Safety
/// `path` must be a live NUL-terminated string for the call. The result is a
/// borrowed opaque handle that stays valid while the DLL remains loaded.
pub unsafe fn R_getDllInfo(path: *const c_char) -> *mut DllInfo {
    if path.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: `path` is non-null and NUL-terminated for the call.
    find_by_path(unsafe { CStr::from_ptr(path) })
        .map_or(ptr::null_mut(), |dll| dll.pointer())
}

/// Find the index of a DllInfo in the table.
fn R_getDllIndex(info: *mut DllInfo) -> isize {
    with_dynload_state(|state| {
        for (i, library) in state.loaded_dll.iter().enumerate() {
            let dll = library.pointer();
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

pub fn R_getEmbeddingDllInfo() -> *mut DllInfo {
    let path = c"(embedding)";
    if let Some(dll) = find_by_path(path) {
        return dll.pointer();
    }
    let dll = add_dll(path, path);
    dll.info.use_dynamic_lookup.set(false);
    dll.pointer()
}

// ---------------------------------------------------------------------------
// R_useDynamicSymbols / R_forceSymbols
// ---------------------------------------------------------------------------

/// Toggle dynamic lookup for a DLL, returning the previous setting.
///
/// # Safety
/// `info` must be null or a live `DllInfo` handle from the DLL table.
pub unsafe fn R_useDynamicSymbols(info: *mut DllInfo, value: bool) -> bool {
    // SAFETY: `info` satisfies this function's contract; null maps to `None`.
    unsafe { info.as_ref() }.is_some_and(|info| info.use_dynamic_lookup.replace(value))
}

/// Toggle registered-symbol-only lookup for a DLL, returning the previous
/// setting.
///
/// # Safety
/// `info` must be null or a live `DllInfo` handle from the DLL table.
pub unsafe fn R_forceSymbols(info: *mut DllInfo, value: bool) -> bool {
    // SAFETY: `info` satisfies this function's contract; null maps to `None`.
    unsafe { info.as_ref() }.is_some_and(|info| info.force_symbols.replace(value))
}

// ---------------------------------------------------------------------------
// Registration is an explicitly unsafe, null-terminated foreign input seam.
// All accepted names, arities and primitive types become owned Rust values.
// ---------------------------------------------------------------------------

unsafe fn copy_types(types: *const c_int, count: c_int) -> Option<Vec<c_int>> {
    if types.is_null() || count < 0 {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(types, count as usize) }.to_vec())
}

/// # Safety
/// `info` must be a live DLL handle on this runtime's thread. Non-null tables
/// must end with a null-name entry. Their strings and any argument type arrays
/// must remain readable for the call.
pub unsafe fn R_registerRoutines(
    info: *mut DllInfo,
    croutines: *const R_CMethodDef,
    call_routines: *const R_CallMethodDef,
    fortran_routines: *const R_FortranMethodDef,
    external_routines: *const R_ExternalMethodDef,
) -> c_int {
    // SAFETY: `info` satisfies this function's contract: null is rejected.
    let Some(info) = (unsafe { info.as_ref() }) else {
        error("R_registerRoutines called with invalid DllInfo object.");
    };
    // Capture every supplied table before publication. A replacement does
    // not alter declarations already captured by an active invocation.
    // SAFETY: the routine tables satisfy this function's contract: each is
    // null or a NUL-terminated array of method definitions for the call.
    let mut replacements = Vec::new();
    unsafe {
        macro_rules! table {
            ($table:expr, $interface:expr, $types:expr) => {
                if !$table.is_null() {
                    let mut entries = Vec::new();
                    let mut cursor = $table;
                    while !(*cursor).name.is_null() {
                        let definition = &*cursor;
                        entries.push(Routine {
                            name: CStr::from_ptr(definition.name).to_owned(),
                            function: definition.fun,
                            interface: $interface,
                            arity: registered::Arity::from_native(definition.num_args),
                            types: $types(definition),
                        });
                        cursor = cursor.add(1);
                    }
                    replacements.push(($interface, entries));
                }
            };
        }
        table!(croutines, Interface::C, |d: &R_CMethodDef| copy_types(
            d.types, d.num_args
        ));
        table!(call_routines, Interface::Call, |_: &R_CallMethodDef| None);
        table!(
            fortran_routines,
            Interface::Fortran,
            |d: &R_FortranMethodDef| copy_types(d.types, d.num_args)
        );
        table!(
            external_routines,
            Interface::External,
            |_: &R_ExternalMethodDef| None
        );
    }
    let mut registry = info.registrations.borrow_mut();
    for (interface, entries) in replacements {
        registry.replace(interface, entries);
    }
    drop(registry);
    info.use_dynamic_lookup.set(!info.handle.is_null());
    info.force_symbols.set(false);
    1
}


/// Dynamic `dlsym` fallback after registered lookup misses.
fn lookup_dynamic(info: &DllInfo, name: &CStr) -> DL_FUNC {
    if !info.use_dynamic_lookup.get() || info.handle.is_null() {
        return None;
    }
    // SAFETY: the DLL lease keeps the handle live and `name` is a C string.
    let sym = unsafe { lookup_address(info.handle, name) };
    if sym.is_null() {
        return None;
    }
    // SAFETY: a POSIX `dlsym` address is used at the requested native ABI.
    unsafe { std::mem::transmute::<*mut c_void, DL_FUNC>(sym) }
}

fn resolve_in_library(
    library: Rc<LoadedDll>,
    name: &CStr,
    kind: c_int,
) -> Option<ResolvedForeignRoutine> {
    let info = &library.info;
    let declaration = info.registrations.borrow().lookup(name, kind);
    let function = declaration
        .as_ref()
        .and_then(|routine| routine.function)
        .or_else(|| lookup_dynamic(info, name));
    function.map(|_| ResolvedForeignRoutine {
        function,
        declaration,
        _library: library,
    })
}

pub(crate) unsafe fn resolve_foreign_symbol(
    name: *const c_char,
    package: *const c_char,
    kind: c_int,
    handle: *mut c_void,
) -> Option<ResolvedForeignRoutine> {
    if name.is_null() {
        return None;
    }
    let name = unsafe { CStr::from_ptr(name) };
    let package = if package.is_null() {
        c""
    } else {
        unsafe { CStr::from_ptr(package) }
    };
    let libraries = with_dynload_state(|state| state.loaded_dll.clone());
    for library in libraries.into_iter().rev() {
        // No shared DllInfo access survives without the physical lease above.
        let selected = if !handle.is_null() {
            library.info.handle == handle
        } else {
            package.to_bytes().is_empty() || library.info.name.as_c_str() == package
        };
        if !selected {
            continue;
        }
        if handle.is_null() && library.info.force_symbols.get() {
            continue;
        }
        let result = resolve_in_library(library, name, kind);
        if result.is_some() || !handle.is_null() || !package.to_bytes().is_empty() {
            return result;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// R_dlsym — look up a symbol (registered, then dynamic)
// ---------------------------------------------------------------------------

/// Look up a native symbol by name in a DLL.
/// Checks registered symbols first, then falls back to dynamic lookup.
///
/// # Safety
/// `info` must be null or a live `DllInfo` handle from the DLL table; `name`
/// must be null or a live NUL-terminated string for the call.
pub unsafe fn R_dlsym(info: *mut DllInfo, name: *const c_char, sym_type: c_int) -> DL_FUNC {
    // SAFETY: `info` satisfies this function's contract; null maps to `None`.
    let info = unsafe { info.as_ref() }?;
    if name.is_null() {
        return None;
    }
    // SAFETY: `name` is non-null and NUL-terminated for the call.
    let name = unsafe { CStr::from_ptr(name) };
    let registered = info
        .registrations
        .borrow()
        .lookup(name, sym_type)
        .and_then(|routine| routine.function);
    registered.or_else(|| lookup_dynamic(info, name))
}

// ---------------------------------------------------------------------------
// Handle lookup helper
// ---------------------------------------------------------------------------

/// Find a loaded DLL by its native handle.
pub(crate) fn R_findDllByHandle(handle: *mut c_void) -> *mut DllInfo {
    if handle.is_null() {
        return ptr::null_mut();
    }
    with_dynload_state(|state| {
        state
            .loaded_dll
            .iter()
            .find(|dll| dll.info.handle == handle)
            .map_or(ptr::null_mut(), |dll| dll.pointer())
    })
}

// ---------------------------------------------------------------------------
// R_FindSymbol — search all loaded DLLs for a symbol
// ---------------------------------------------------------------------------

/// # Safety
/// `name` must be null or a live NUL-terminated string for the call; `pkg`
/// must be null or a live NUL-terminated string for the call.
pub unsafe fn R_FindSymbol(name: *const c_char, pkg: *const c_char, sym_type: c_int) -> DL_FUNC {
    if name.is_null() {
        return None;
    }
    // SAFETY: `name` is non-null and NUL-terminated for the call.
    let name = unsafe { CStr::from_ptr(name) };
    // SAFETY: `pkg` is null or NUL-terminated for the call.
    let pkg_cstr = if pkg.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(pkg) }.to_str().unwrap_or("")
    };
    let all = pkg_cstr.is_empty();

    // Search in reverse order (most recently loaded first).
    let dlls = with_dynload_state(|state| state.loaded_dll.clone());
    for dll in dlls.iter().rev() {
        // Skip unless this is a global search or the DLL name matches.
        if !(all || dll.info.name.to_str().unwrap_or("") == pkg_cstr) {
            continue;
        }
        if dll.info.force_symbols.get() {
            continue;
        }
        let f = dll
            .info
            .registrations
            .borrow()
            .lookup(name, sym_type)
            .and_then(|routine| routine.function);
        let f = f.or_else(|| lookup_dynamic(&dll.info, name));
        if f.is_some() || !all {
            return f;
        }
    }
    None
}


// ---------------------------------------------------------------------------
// AddDLL — high-level DLL loading (with dlopen)
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// R_getSymbolInfo — resolve a symbol and return its info as an SEXP
// ---------------------------------------------------------------------------

pub fn R_getSymbolInfo(sname: SEXP, spackage: SEXP, _with_registration: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(spackage) == SEXPTYPE::EXTPTRSXP {
            let result = (|| {
                crate::sexp::owner::OwnerToken::with_current(|owner| {
                    let _pin = owner.pin()?;
                    owner.require_active()?;
                    let factory = owner.node_factory();
                    // Retain all caller graphs before an ALTREP name provider runs.
                    let name = factory.wrap(sname)?.into_owned()?;
                    let package = factory.wrap(spackage)?.into_owned()?;
                    let registration = factory.wrap(_with_registration)?.into_owned()?;
                    let library = crate::mainutils::native_symbols::library(owner, &package)?;
                    if name.typeof_() != SEXPTYPE::STRSXP || name.len() != 1 {
                        return Err(crate::mainutils::native_symbols::invalid_symbol());
                    }
                    let character = name.try_string_elt(0)?;
                    let name = crate::mainutils::native_symbols::name(owner, &character)?;
                    owner.require_active()?;
                    let keys = crate::mainutils::native_symbols::AttributeKeys {
                        names: factory.wrap(R_NamesSymbol())?,
                        class: factory.wrap(R_ClassSymbol())?,
                    };
                    let registered = registration.try_logical_elt(0)? != FALSE;
                    let package = library.package()?;
                    let result = crate::mainutils::native_symbols::symbol_info(
                        owner,
                        name.as_str(),
                        package,
                        registered,
                        &keys,
                    )?;
                    library.package()?;
                    Ok(result.unwrap_or_else(|| factory.nil().into_owned().unwrap()))
                })?
            })();
            return result
                .unwrap_or_else(|e: crate::sexp::object::SexpError| error(&e.to_string()))
                .as_raw();
        }
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
        SET_STRING_ELT(names, 0, crate::sexp::constructors::mk_char_str("name"));

        // Address as external pointer (simplified)
        SET_VECTOR_ELT(
            sym,
            1,
            crate::sexp::constructors::mk_string_str("<native symbol>"),
        );
        SET_STRING_ELT(names, 1, crate::sexp::constructors::mk_char_str("address"));

        SET_VECTOR_ELT(
            sym,
            2,
            crate::sexp::constructors::mk_string_str("<unknown>"),
        );
        SET_STRING_ELT(names, 2, crate::sexp::constructors::mk_char_str("dll"));

        setAttrib(sym, R_NamesSymbol(), names);
        setAttrib(
            sym,
            R_ClassSymbol(),
            crate::sexp::constructors::mk_string_str("NativeSymbolInfo"),
        );

        sym
    }
}

// ---------------------------------------------------------------------------
// R_getDllTable — return info on all loaded DLLs
// ---------------------------------------------------------------------------

pub fn R_getDllTable() -> SEXP {
    if native_extensions_enabled() {
        return legacy_foreign_dll_table();
    }
    bundled_dll_table_entry()
}

fn bundled_dll_table_entry() -> SEXP {
    let result = (|| {
        crate::sexp::owner::OwnerToken::with_current(|owner| {
            let _pin = owner.pin()?;
            owner.require_active()?;
            let factory = owner.node_factory();
            let keys = crate::mainutils::native_symbols::AttributeKeys {
                names: factory.wrap(R_NamesSymbol())?,
                class: factory.wrap(R_ClassSymbol())?,
            };
            let table = crate::mainutils::native_symbols::dll_table(owner, &keys)?;
            owner.require_active()?;
            Ok(table)
        })?
    })();
    result
        .unwrap_or_else(|e: crate::sexp::object::SexpError| error(&e.to_string()))
        .as_raw()
}

fn legacy_foreign_dll_table() -> SEXP {
    let dlls = with_dynload_state(|state| state.loaded_dll.clone());
    // Root `ans` across the per-entry allocations below.
    let ans = crate::sexp::context::own_control_value(Rf_allocVector(
        SEXPTYPE::VECSXP,
        dlls.len() as c_int,
    ));
    for (index, dll) in dlls.iter().enumerate() {
        SET_VECTOR_ELT(
            ans.as_raw(),
            index as R_xlen_t,
            make_dll_info_sexp(&dll.info),
        );
    }
    setAttrib(
        ans.as_raw(),
        R_ClassSymbol(),
        crate::sexp::constructors::mk_string_str("DLLInfoList"),
    );
    ans.as_raw()
}

use crate::sexp::ffi::R_xlen_t;

/// Create an SEXP representing a DllInfo.
fn make_dll_info_sexp(info: &DllInfo) -> SEXP {
    // Root the pair across the element allocations below.
    let ref_vec = crate::sexp::context::own_control_value(Rf_allocVector(SEXPTYPE::VECSXP, 6));
    let el_names = crate::sexp::context::own_control_value(Rf_allocVector(SEXPTYPE::STRSXP, 6));

    // name
    SET_VECTOR_ELT(
        ref_vec.as_raw(),
        0,
        crate::sexp::constructors::mk_string_bytes(info.name.to_bytes()),
    );
    SET_STRING_ELT(
        el_names.as_raw(),
        0,
        crate::sexp::constructors::mk_char_str("name"),
    );

    // path
    SET_VECTOR_ELT(
        ref_vec.as_raw(),
        1,
        crate::sexp::constructors::mk_string_bytes(info.path.to_bytes()),
    );
    SET_STRING_ELT(
        el_names.as_raw(),
        1,
        crate::sexp::constructors::mk_char_str("path"),
    );

    // dynamicLookup
    SET_VECTOR_ELT(
        ref_vec.as_raw(),
        2,
        Rf_ScalarLogical(if info.use_dynamic_lookup.get() {
            TRUE
        } else {
            FALSE
        }),
    );
    SET_STRING_ELT(
        el_names.as_raw(),
        2,
        crate::sexp::constructors::mk_char_str("dynamicLookup"),
    );

    // handle (simplified as nil)
    SET_VECTOR_ELT(ref_vec.as_raw(), 3, R_NilValue());
    SET_STRING_ELT(
        el_names.as_raw(),
        3,
        crate::sexp::constructors::mk_char_str("handle"),
    );

    // info (simplified as nil)
    SET_VECTOR_ELT(ref_vec.as_raw(), 4, R_NilValue());
    SET_STRING_ELT(
        el_names.as_raw(),
        4,
        crate::sexp::constructors::mk_char_str("info"),
    );

    // forceSymbols
    SET_VECTOR_ELT(
        ref_vec.as_raw(),
        5,
        Rf_ScalarLogical(if info.force_symbols.get() {
            TRUE
        } else {
            FALSE
        }),
    );
    SET_STRING_ELT(
        el_names.as_raw(),
        5,
        crate::sexp::constructors::mk_char_str("forceSymbols"),
    );

    setAttrib(ref_vec.as_raw(), R_NamesSymbol(), el_names.as_raw());
    setAttrib(
        ref_vec.as_raw(),
        R_ClassSymbol(),
        crate::sexp::constructors::mk_string_str("DLLInfo"),
    );

    ref_vec.as_raw()
}

// ---------------------------------------------------------------------------
// R_getRegisteredRoutines — return registered routines for a DLL
// ---------------------------------------------------------------------------

pub fn R_getRegisteredRoutines(dll: SEXP) -> SEXP {
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
pub fn do_dynload(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    native_dynamic_loading_policy_error("dyn.load()");
}

/// `.Internal(dyn.unload(...))` handler.
pub fn do_dynunload(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    native_dynamic_loading_policy_error("dyn.unload()");
}

// ---------------------------------------------------------------------------
// do_getSymbolInfo / do_getDllTable / do_getRegisteredRoutines
// ---------------------------------------------------------------------------

pub fn do_getSymbolInfo(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    checkArity(op, args);
    // The public pure-R wrapper admits only authenticated bundled DLL
    // references. Name-only lookup cannot mint a typed library authority.
    if TYPEOF(CAR(CDR(args))) == SEXPTYPE::STRSXP {
        return R_NilValue();
    }
    if TYPEOF(CAR(CDR(args))) != SEXPTYPE::EXTPTRSXP {
        error("invalid DLLInfo reference");
    }
    R_getSymbolInfo(CAR(args), CAR(CDR(args)), CAR(CDR(CDR(args))))
}

pub fn do_getDllTable(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    checkArity(op, args);
    bundled_dll_table_entry()
}

pub fn do_getRegisteredRoutines(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    checkArity(op, args);
    R_getRegisteredRoutines(CAR(args))
}


// ---------------------------------------------------------------------------
// R_RegisterCCallable / R_GetCCallable — inter-package C function sharing
// ---------------------------------------------------------------------------

unsafe fn get_package_centry_table(package: *const c_char) -> SEXP {
    unsafe {
        let table = with_dynload_state(|state| state.c_entry_table);
        let table = if table.is_null() {
            let env = R_NewHashedEnv(R_NilValue(), 0);
            with_dynload_state(|state| state.c_entry_table = env);
            env
        } else {
            table
        };
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
// Rf_lookupCachedSymbol — compatibility wrapper
// ---------------------------------------------------------------------------

pub unsafe fn Rf_lookupCachedSymbol(
    name: *const c_char,
    pkg: *const c_char,
    all: c_int,
) -> DL_FUNC {
    unsafe {
        if name.is_null() {
            return None;
        }

        let pkg_ptr = if pkg.is_null() || all != 0 {
            b"\0".as_ptr() as *const c_char
        } else {
            pkg
        };

        R_FindSymbol(name, pkg_ptr, R_ANY_SYM)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod owned_registration_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;
    use std::ptr;

    fn with_session<F>(f: F)
    where
        F: FnOnce(),
    {
        let session = RSession::new();
        session.with_protected(f);
    }

    #[test]
    fn test_init_dynload() {
        with_session(|| {
            init_loaded_dll();
            with_dynload_state(|state| {
                assert!(state.loaded_dll.is_empty());
            });
        });
    }

    #[test]
    fn test_add_dll() {
        with_session(|| {
            init_loaded_dll();
            let dll = add_dll(c"test_path", c"test_name");
            with_dynload_state(|state| {
                assert_eq!(state.loaded_dll.len(), 1);
                assert_eq!(dll.pointer(), state.loaded_dll[0].pointer());
            });
        });
    }

    #[test]
    fn test_dll_metadata_lease_owns_sealed_names() {
        // Session-free: the Rc lease alone owns the opaque DllInfo surface.
        let library = LoadedDll::new(DllInfo::new(
            CString::new("/tmp/sample.so").unwrap(),
            c"sample".to_owned(),
            ptr::null_mut(),
            false,
        ));
        let pointer = library.pointer();
        let lease = library.clone();
        // SAFETY: `pointer` is a live DllInfo handle owned by the lease.
        assert!(!unsafe { R_useDynamicSymbols(pointer, true) });
        drop(library);
        assert_eq!(pointer, lease.pointer());
        assert_eq!(lease.info.name.as_c_str(), c"sample");
        assert_eq!(lease.info.path.as_c_str().to_str(), Ok("/tmp/sample.so"));
        assert!(lease.info.use_dynamic_lookup.get());
    }

    #[test]
    fn test_get_dll_info_not_found() {
        with_session(|| unsafe {
            init_loaded_dll();
            let result = R_getDllInfo(c"nonexistent".as_ptr());
            assert!(result.is_null());
        });
    }

    #[test]
    fn test_get_dll_info_found() {
        with_session(|| unsafe {
            init_loaded_dll();
            let _ = add_dll(c"test_path", c"test_name");
            let result = R_getDllInfo(c"test_path".as_ptr());
            assert!(!result.is_null());
        });
    }



    #[test]
    fn test_use_dynamic_symbols() {
        with_session(|| unsafe {
            init_loaded_dll();
            let _ = add_dll(c"path", c"name");
            let dll = R_getDllInfo(c"path".as_ptr());
            assert!(!dll.is_null());
            let old = R_useDynamicSymbols(dll, false);
            assert!(old); // default is true
            assert!(!(*dll).use_dynamic_lookup.get());
        });
    }

    #[test]
    fn test_force_symbols() {
        with_session(|| unsafe {
            init_loaded_dll();
            let _ = add_dll(c"path2", c"name2");
            let dll = R_getDllInfo(c"path2".as_ptr());
            assert!(!dll.is_null());
            let old = R_forceSymbols(dll, true);
            assert!(!old); // default is false
            assert!((*dll).force_symbols.get());
        });
    }


    #[test]
    fn test_find_symbol_empty() {
        with_session(|| unsafe {
            init_loaded_dll();
            let f = R_FindSymbol(c"nonexistent".as_ptr(), c"".as_ptr(), R_ANY_SYM);
            assert!(f.is_none());
        });
    }

    extern "C" fn test_registered_call() {}

    #[test]
    fn test_find_symbol_registered_call() {
        with_session(|| unsafe {
            init_loaded_dll();
            add_dll(c"/tmp/test.so", c"testpkg");
            let info = R_getDllInfo(c"/tmp/test.so".as_ptr());
            let routines = [
                R_CallMethodDef {
                    name: b"registered\0".as_ptr() as *const c_char,
                    fun: Some(test_registered_call),
                    num_args: 0,
                },
                R_CallMethodDef {
                    name: ptr::null(),
                    fun: None,
                    num_args: 0,
                },
            ];
            R_registerRoutines(
                info,
                ptr::null(),
                routines.as_ptr(),
                ptr::null(),
                ptr::null(),
            );

            let found = R_FindSymbol(c"registered".as_ptr(), c"testpkg".as_ptr(), R_CALL_SYM);
            assert!(found.is_some());
            assert_eq!(
                found.unwrap() as *const (),
                test_registered_call as *const ()
            );

            let loaded = Rf_lookupCachedSymbol(c"registered".as_ptr(), c"testpkg".as_ptr(), 0);
            assert!(loaded.is_some());
            assert_eq!(
                loaded.unwrap() as *const (),
                test_registered_call as *const ()
            );
        });
    }

    #[test]
    fn test_dynload_tables_are_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();
        extern "C" fn left_only() {}
        unsafe fn stats_registration() -> (SEXP, *mut c_void) {
            unsafe {
                let root = with_dynload_state(|state| state.c_entry_table);
                assert!(!root.is_null());
                let stats = R_findVarInFrame(root, crate::sexp::symbol::install_str("stats"));
                assert_ne!(stats, R_UnboundValue());
                let callable = R_findVarInFrame(stats, crate::sexp::symbol::install_str("rcont2"));
                assert_eq!(TYPEOF(callable), SEXPTYPE::EXTPTRSXP);
                (callable, R_ExternalPtrAddr(callable))
            }
        }
        let left_stats = left.with_protected(|| unsafe { stats_registration() });
        let right_stats = right.with_protected(|| unsafe { stats_registration() });

        let (left_package_table, left_root_table) = left.with_protected(|| unsafe {
            init_loaded_dll();
            let _ = add_dll(c"left_path", c"left_name");
            R_RegisterCCallable(c"leftpkg".as_ptr(), c"leftonly".as_ptr(), Some(left_only));
            let table = get_package_centry_table(b"leftpkg\0".as_ptr() as *const c_char);
            let root = with_dynload_state(|state| {
                assert_eq!(state.loaded_dll.len(), 1);
                assert!(!state.c_entry_table.is_null());
                state.c_entry_table
            });
            (table, root)
        });

        right.with_protected(|| unsafe {
            let root = with_dynload_state(|state| {
                assert!(state.loaded_dll.is_empty());
                state.c_entry_table
            });
            // Startup registers stats callables in each session. Isolation
            // means no left-only registration, rather than an empty table.
            assert_ne!(root, left_root_table);
            assert_eq!(
                R_findVarInFrame(root, crate::sexp::symbol::install_str("leftpkg")),
                R_UnboundValue()
            );
            assert_eq!(stats_registration(), right_stats);
        });

        let (right_package_table, right_root_table) = right.with_protected(|| unsafe {
            init_loaded_dll();
            let table = get_package_centry_table(b"rightpkg\0".as_ptr() as *const c_char);
            let root = with_dynload_state(|state| {
                assert!(state.loaded_dll.is_empty());
                assert!(!state.c_entry_table.is_null());
                state.c_entry_table
            });
            (table, root)
        });

        assert_ne!(left_root_table, right_root_table);
        assert_ne!(left_package_table, right_package_table);
        left.with_protected(|| unsafe {
            assert_eq!(stats_registration(), left_stats);
            let registered = R_GetCCallable(c"leftpkg".as_ptr(), c"leftonly".as_ptr());
            assert_eq!(
                registered.map(|f| f as *const ()),
                Some(left_only as *const ())
            );
            assert_eq!(
                R_findVarInFrame(
                    left_root_table,
                    crate::sexp::symbol::install_str("rightpkg")
                ),
                R_UnboundValue()
            );
        });
        right.with_protected(|| unsafe {
            assert_eq!(stats_registration(), right_stats);
            assert_eq!(
                R_findVarInFrame(
                    right_root_table,
                    crate::sexp::symbol::install_str("leftpkg")
                ),
                R_UnboundValue()
            );
        });
    }
}

#[cfg(test)]
#[path = "rdynload/native_symbol_handle_tests.rs"]
mod native_symbol_handle_tests;
