#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/startup.c
//!
//! Startup/shutdown configuration, workspace save/restore, file opening.

use std::env;
use std::ffi::CStr;
use std::fs::File;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use super::saveload::R_RestoreGlobalEnvFromFile;

// ---------------------------------------------------------------------------
// Save/Restore action types
// ---------------------------------------------------------------------------
// Save/Restore action types
// ---------------------------------------------------------------------------

pub const SA_SAVEASK: c_int = 0;
pub const SA_SAVE: c_int = 1;
pub const SA_RESTORE: c_int = 2;
pub const SA_NOSAVE: c_int = 3;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether to save workspace on exit.
pub static mut SaveAction: c_int = SA_SAVEASK;

/// Whether to restore workspace on startup.
pub static mut RestoreAction: c_int = SA_RESTORE;

/// Whether to load the init file (.Rprofile).
pub static mut LoadInitFile: bool = true;

static mut LoadSiteFile: bool = true;

// ---------------------------------------------------------------------------
// Default sizes
// ---------------------------------------------------------------------------

const R_VSIZE: usize = 64 * 1024 * 1024; // 64 MB default
const R_NSIZE: usize = 350000;
const R_PPSSIZE: usize = 50000;
const Mega: usize = 1024 * 1024;

const Max_Nsize: usize = 50_000_000;
const Min_Nsize: usize = 50_000;
const Min_Vsize: usize = 262_144;

// ---------------------------------------------------------------------------
// Workspace management
// ---------------------------------------------------------------------------

/// Get the workspace file name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn get_workspace_name() -> *const c_char {
    static WORKSPACE_NAME: &[u8] = b".RData\0";
    WORKSPACE_NAME.as_ptr() as *const c_char
}

/// Restore the global environment from the workspace file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RestoreGlobalEnv() {
    unsafe {
        if RestoreAction == SA_RESTORE {
            let name = get_workspace_name();
            R_RestoreGlobalEnvFromFile(name, 0);
        }
    }
}

/// Save the global environment to .RData.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SaveGlobalEnv() {
    // Stub: full implementation requires saveload integration
}

/// Initialize data (restore workspace).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitialData() {
    unsafe {
        R_RestoreGlobalEnv();
    }
}

// ---------------------------------------------------------------------------
// File opening helpers
// ---------------------------------------------------------------------------

/// Open a library file relative to R_HOME/library/base/R/.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_OpenLibraryFile(file: *const c_char) -> *mut std::ffi::c_void {
    unsafe {
        if file.is_null() {
            return ptr::null_mut();
        }
        let fname = match CStr::from_ptr(file).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        };
        let r_home = env::var("R_HOME").unwrap_or_default();
        let path = format!("{}/library/base/R/{}", r_home, fname);
        match File::open(&path) {
            Ok(f) => Box::into_raw(Box::new(f)) as *mut std::ffi::c_void,
            Err(_) => ptr::null_mut(),
        }
    }
}

/// Get the library file path into a buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_LibraryFileName(
    file: *const c_char,
    buf: *mut c_char,
    bsize: usize,
) -> *mut c_char {
    unsafe {
        if file.is_null() || buf.is_null() || bsize == 0 {
            return buf;
        }
        let fname = match CStr::from_ptr(file).to_str() {
            Ok(s) => s,
            Err(_) => return buf,
        };
        let r_home = env::var("R_HOME").unwrap_or_default();
        let path = format!("{}/library/base/R/{}", r_home, fname);
        let bytes = path.as_bytes();
        let copy_len = bytes.len().min(bsize - 1);
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, copy_len);
        *buf.add(copy_len) = 0;
        buf
    }
}

/// Open the system init file (Rprofile).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_OpenSysInitFile() -> *mut std::ffi::c_void {
    let r_home = env::var("R_HOME").unwrap_or_default();
    let path = format!("{}/library/base/R/Rprofile", r_home);
    match File::open(&path) {
        Ok(f) => Box::into_raw(Box::new(f)) as *mut std::ffi::c_void,
        Err(_) => ptr::null_mut(),
    }
}

/// Open the site init file (Rprofile.site).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_OpenSiteFile() -> *mut std::ffi::c_void {
    unsafe {
        if !LoadSiteFile {
            return ptr::null_mut();
        }
        if let Ok(p) = env::var("R_PROFILE") {
            if p.is_empty() {
                return ptr::null_mut();
            }
            match File::open(&p) {
                Ok(f) => return Box::into_raw(Box::new(f)) as *mut std::ffi::c_void,
                Err(_) => return ptr::null_mut(),
            }
        }
        let r_home = env::var("R_HOME").unwrap_or_default();
        let path = format!("{}/etc/Rprofile.site", r_home);
        match File::open(&path) {
            Ok(f) => Box::into_raw(Box::new(f)) as *mut std::ffi::c_void,
            Err(_) => ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// Restore/Save from/to file (stubs — full implementation in saveload)
// ---------------------------------------------------------------------------

pub unsafe extern "C" fn _rmath_R_RestoreGlobalEnvFromFile(_name: *const c_char, _quiet: c_int) {
    // Stub: full implementation in saveload.rs
}

// R_SaveGlobalEnvToFile is defined in saveload.rs

// ---------------------------------------------------------------------------
// Rstart parameter management (stubs)
// ---------------------------------------------------------------------------

/// Set default startup parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DefParamsEx(_rp: *mut c_void, _version: c_int) -> c_int {
    0
}

/// Set default startup parameters (version 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DefParams(_rp: *mut c_void) {
    unsafe {
        R_DefParamsEx(_rp, 0);
    }
}

/// Apply startup parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetParams(_rp: *mut c_void) {
    // Stub: full implementation requires Rstart struct
}

/// Read sizes from environment variables.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SizeFromEnv(_rp: *mut c_void) {
    // Stub: reads R_MAX_VSIZE, R_VSIZE, R_NSIZE from environment
}

/// Set max vector heap size.
// no_mangle removed (duplicate)
pub unsafe extern "C" fn R_SetMaxVSize(_size: usize) -> c_int {
    1 // success
}

/// Set max cons cell heap size.
// no_mangle removed (duplicate)
pub unsafe extern "C" fn R_SetMaxNSize(_size: usize) -> c_int {
    1 // success
}

/// Set pushdown stack size.
// no_mangle removed (duplicate)
pub unsafe extern "C" fn R_SetPPSize(_size: usize) {}

/// Set max number of connections.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetNconn(_n: c_int) {}

// ---------------------------------------------------------------------------
// Globals used by other modules
// ---------------------------------------------------------------------------

pub static mut R_Quiet: c_int = 0;
pub static mut R_NoEcho: c_int = 0;
pub static mut R_Interactive: c_int = 1;
pub static mut R_Verbose: c_int = 0;
pub static mut R_VSize: usize = 0;
pub static mut R_NSize: usize = 0;
