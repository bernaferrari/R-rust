#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code
)]

//! Port of R's src/unix/dynload.c -- Unix dynamic loading via dlopen/dlsym.
//!
//! Implements `InitFunctionHashing` which sets up the OS-specific dynamic
//! loading vtable using POSIX dlopen/dlsym/dlclose.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

// ---------------------------------------------------------------------------
// Opaque types
// ---------------------------------------------------------------------------

/// Opaque DllInfo type (placeholder).
#[repr(C)]
pub struct DllInfo {
    _private: [u8; 0],
}

/// Function pointer type matching R's DL_FUNC.
pub type DL_FUNC = Option<unsafe extern "C" fn()>;

// ---------------------------------------------------------------------------
// Platform-specific dlopen constants
// ---------------------------------------------------------------------------

#[cfg(all(unix, not(target_os = "macos")))]
const RTLD_NOW: c_int = 0x2;
#[cfg(all(unix, not(target_os = "macos")))]
const RTLD_LAZY: c_int = 0x1;
#[cfg(all(unix, not(target_os = "macos")))]
const RTLD_GLOBAL: c_int = 0x100;
#[cfg(all(unix, not(target_os = "macos")))]
const RTLD_LOCAL: c_int = 0x0;

#[cfg(target_os = "macos")]
const RTLD_NOW: c_int = 0x2;
#[cfg(target_os = "macos")]
const RTLD_LAZY: c_int = 0x1;
#[cfg(target_os = "macos")]
const RTLD_GLOBAL: c_int = 0x8;
#[cfg(target_os = "macos")]
const RTLD_LOCAL: c_int = 0x4;
#[cfg(target_arch = "wasm32")]
const RTLD_NOW: c_int = 0x2;
#[cfg(target_arch = "wasm32")]
const RTLD_LAZY: c_int = 0x1;
#[cfg(target_arch = "wasm32")]
const RTLD_GLOBAL: c_int = 0x100;
#[cfg(target_arch = "wasm32")]
const RTLD_LOCAL: c_int = 0x0;

// ---------------------------------------------------------------------------
// OS dynamic symbol vtable
// ---------------------------------------------------------------------------

struct OsDynSymbolTable {
    loadLibrary:
        Option<unsafe extern "C" fn(*const c_char, c_int, c_int, *const c_char) -> *mut c_void>,
    dlsym_fn: Option<unsafe extern "C" fn(*mut DllInfo, *const c_char) -> DL_FUNC>,
    closeLibrary: Option<unsafe extern "C" fn(*mut c_void)>,
    getError: Option<unsafe extern "C" fn(*mut c_char, c_int)>,
}

fn os_dyn_symbol_table() -> OsDynSymbolTable {
    OsDynSymbolTable {
        loadLibrary: Some(load_library),
        dlsym_fn: Some(local_dlsym),
        closeLibrary: Some(close_library),
        getError: Some(get_system_error),
    }
}

// ---------------------------------------------------------------------------
// Internal helper functions
// ---------------------------------------------------------------------------

/// Compute the dlopen flag from asLocal and now parameters.
pub(crate) fn compute_dlopen_flag(as_local: c_int, now: c_int) -> c_int {
    let mut flag: c_int = 0;

    if as_local != 0 {
        flag = RTLD_LOCAL;
    } else {
        flag = RTLD_GLOBAL;
    }

    if now != 0 {
        flag |= RTLD_NOW;
    } else {
        flag |= RTLD_LAZY;
    }

    flag
}

/// Load a shared library using dlopen.
unsafe extern "C" fn load_library(
    path: *const c_char,
    as_local: c_int,
    now: c_int,
    _search: *const c_char,
) -> *mut c_void {
    unsafe {
        let open_flag = compute_dlopen_flag(as_local, now);
        libc_dlopen(path, open_flag)
    }
}

/// Look up a symbol in a shared library using dlsym.
unsafe extern "C" fn local_dlsym(info: *mut DllInfo, name: *const c_char) -> DL_FUNC {
    // In the full implementation, info->handle is used with dlsym.
    // For now, return null since DllInfo is a stub.
    let _ = info;
    let _ = name;
    None
}

/// Close a shared library using dlclose.
unsafe extern "C" fn close_library(handle: *mut c_void) {
    unsafe {
        libc_dlclose(handle);
    }
}

/// Get the last dlerror message.
unsafe extern "C" fn get_system_error(buf: *mut c_char, len: c_int) {
    unsafe {
        if len > 0 {
            let err = libc_dlerror();
            if !err.is_null() {
                let err_str = CStr::from_ptr(err);
                let bytes = err_str.to_bytes();
                let copy_len = bytes.len().min(len as usize - 1);
                ptr::copy_nonoverlapping(err_str.as_ptr(), buf as *mut libc::c_char, copy_len);
                *buf.add(copy_len) = 0;
            } else {
                *buf = 0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Raw libc FFI bindings for dlopen/dlsym/dlclose/dlerror
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

/// wasm32 sandbox: dynamic loading is unsupported — same reject policy as the
/// Android sandbox. `dyn.load` fails cleanly; nothing is ever loaded.
#[cfg(target_arch = "wasm32")]
pub(crate) unsafe fn dlopen(_filename: *const c_char, _flag: c_int) -> *mut c_void {
    std::ptr::null_mut()
}
#[cfg(target_arch = "wasm32")]
pub(crate) unsafe fn dlsym(_handle: *mut c_void, _symbol: *const c_char) -> *mut c_void {
    std::ptr::null_mut()
}
#[cfg(target_arch = "wasm32")]
pub(crate) unsafe fn dlclose(_handle: *mut c_void) -> c_int {
    0
}
#[cfg(target_arch = "wasm32")]
pub(crate) unsafe fn dlerror() -> *mut c_char {
    static MSG: &[u8] = b"dynamic loading is not supported on this platform\0";
    MSG.as_ptr() as *mut c_char
}

unsafe fn libc_dlopen(path: *const c_char, flag: c_int) -> *mut c_void {
    unsafe { dlopen(path, flag) }
}

unsafe fn libc_dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { dlsym(handle, name) }
}

unsafe fn libc_dlclose(handle: *mut c_void) {
    unsafe {
        dlclose(handle);
    }
}

unsafe fn libc_dlerror() -> *mut c_char {
    unsafe { dlerror() }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the function hashing / dynamic loading subsystem.
/// Compatibility entrypoint for R startup. The OS-specific vtable is immutable
/// in this port because it contains fixed platform function pointers.
pub fn InitFunctionHashing() {
    let _ = os_dyn_symbol_table();
}

/// Delete cached symbols for a DLL (stub).
pub unsafe fn Rf_deleteCachedSymbol(_name: *const c_char) -> c_int {
    0
}

/// Look up a cached symbol (stub).
pub unsafe fn Rf_lookupCachedSymbol(_name: *const c_char, _can_cache: c_int) -> DL_FUNC {
    None
}

/// Delete all cached symbols (stub).
pub fn Rf_deleteCachedSymbols() {}

/// Look up a cached symbol (stub).
pub unsafe fn Rf_lookupCachedSymbols(_name: *const c_char, _can_cache: c_int) -> DL_FUNC {
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_dlopen_flag_defaults() {
        // asLocal=1, now=1 => RTLD_LOCAL | RTLD_NOW
        let flag = compute_dlopen_flag(1, 1);
        assert_ne!(flag, 0);
    }

    #[test]
    fn test_compute_dlopen_flag_global_lazy() {
        // asLocal=0, now=0 => RTLD_GLOBAL | RTLD_LAZY
        let flag = compute_dlopen_flag(0, 0);
        assert_ne!(flag, 0);
    }

    #[test]
    fn test_init_function_hashing_runs() {
        InitFunctionHashing();
        let table = os_dyn_symbol_table();
        assert!(table.loadLibrary.is_some());
        assert!(table.dlsym_fn.is_some());
        assert!(table.closeLibrary.is_some());
        assert!(table.getError.is_some());
    }
}
