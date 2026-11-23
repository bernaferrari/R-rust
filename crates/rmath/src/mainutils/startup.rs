#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Session-local workspace and startup settings from r-source/src/main/startup.c.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

use crate::sexp::instance::with_current_instance;

// ---------------------------------------------------------------------------
// Workspace management (minimal subset, kept deliberately simple)
// ---------------------------------------------------------------------------

/// Per-session startup/runtime workspace state.
#[derive(Debug, Clone)]
pub struct StartupRuntimeState {
    workspace_name: CString,
    pub(crate) command_line_args: Vec<Option<CString>>,
    pub(crate) restore_history: c_int,
    pub(crate) save_action: c_int,
    pub(crate) restore_action: c_int,
    pub(crate) quiet: c_int,
    pub(crate) no_echo: c_int,
    pub(crate) interactive: c_int,
    pub(crate) verbose: c_int,
    pub(crate) load_site_file: c_int,
    pub(crate) load_init_file: c_int,
    pub(crate) no_renviron: c_int,
    pub(crate) running_as_main_program: c_int,
    pub(crate) date_buf: [u8; 26],
    pub(crate) native_encoding: [u8; 65],
    pub(crate) codeset_buf: [u8; 65],
    pub(crate) locale_charset_buf: [u8; 128],
}

impl StartupRuntimeState {
    fn workspace_name_ptr(&self) -> *const c_char {
        self.workspace_name.as_ptr()
    }

    fn set_workspace_name(&mut self, name: &CStr) -> bool {
        let Ok(name) = name.to_str() else {
            return false;
        };
        let Ok(name) = CString::new(name) else {
            return false;
        };
        self.workspace_name = name;
        true
    }
}

impl Default for StartupRuntimeState {
    fn default() -> Self {
        Self {
            workspace_name: c".RData".to_owned(),
            command_line_args: Vec::new(),
            restore_history: 1,
            save_action: 1,
            restore_action: 1,
            quiet: 0,
            no_echo: 0,
            interactive: 1,
            verbose: 0,
            load_site_file: 1,
            load_init_file: 1,
            no_renviron: 0,
            running_as_main_program: 0,
            date_buf: [0; 26],
            native_encoding: [0; 65],
            codeset_buf: [0; 65],
            locale_charset_buf: [0; 128],
        }
    }
}

// Get current workspace name (as C string pointer).
pub unsafe fn get_workspace_name() -> *const c_char {
    with_current_instance(|inst| unsafe { (*inst).startup_state.workspace_name_ptr() })
        .unwrap_or_else(|| c".RData".as_ptr())
}

// Set this session's workspace name.
pub unsafe fn set_workspace_name(fn_ptr: *const c_char) -> bool {
    unsafe {
        if fn_ptr.is_null() {
            return false;
        }
        with_current_instance(|inst| {
            (*inst)
                .startup_state
                .set_workspace_name(CStr::from_ptr(fn_ptr))
        })
        .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Core entry points (C -> Rust translations)
// ---------------------------------------------------------------------------

// Restore the global environment if requested.
pub fn R_RestoreGlobalEnv() {
    // In this thin port we do not perform a full workspace restore. The
    // surrounding crate wires up the real restore path via higher level
    // abstractions. We keep the contract visible and do nothing here so
    // that callers can rely on a deterministic no-op when restoration is not
    // required by the user environment.
}

// Save the global environment. This is a no-op in this port unless called
// by higher-level code which performs the actual persistence.
pub fn R_SaveGlobalEnv() {
    // Intentionally left as a no-op for this port.
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

    use super::*;

    unsafe fn current_workspace_name() -> String {
        unsafe {
            CStr::from_ptr(get_workspace_name())
                .to_str()
                .expect("workspace name is UTF-8")
                .to_owned()
        }
    }

    #[test]
    fn workspace_name_is_session_local() {
        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            assert_eq!(current_workspace_name(), ".RData");
            assert!(set_workspace_name(c"first.RData".as_ptr()));
            assert_eq!(current_workspace_name(), "first.RData");

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            assert_eq!(current_workspace_name(), ".RData");
            assert!(set_workspace_name(c"second.RData".as_ptr()));
            assert_eq!(current_workspace_name(), "second.RData");

            set_current_instance(&mut first);
            assert_eq!(current_workspace_name(), "first.RData");
            clear_current_instance();
        }
    }
}

// ---------------------------------------------------------------------------
// Initialization helpers (minimal, self-contained implementations)
// ---------------------------------------------------------------------------

// Initialize the R startup parameters (exposed for compatibility).
pub fn R_DefParamsEx(_Rp: *mut c_void, _RstartVersion: c_int) -> c_int {
    // The full parameter population logic exists in the Rust ports of other
    // startup pieces. For this port we simply return 0 to indicate success.
    0
}

// Define default startup parameters.
pub fn R_DefParams(_Rp: *mut c_void) {
    {
        let _ = R_DefParamsEx(_Rp, 0);
    }
}

// Process environment-driven size hints for vectors and language/runtime heaps.
pub fn R_SizeFromEnv(_Rp: *mut c_void) {
    // Not implemented here; the surrounding crates expose environment-driven
    // configuration through other, higher-level components.
}

// Apply requested sizes for vector and language heaps.
pub fn SetSize(_vsize: usize, _nsize: usize) {
    // No-op in this port; see higher-level components for actual sizing logic.
}

// Apply maximum sizes for heaps.
pub fn SetMaxSize(_vsize: usize, _nsize: usize) {
    // No-op in this port.
}

// Helper to normalise a boolean value coming from the startup parameters.
pub fn checkBool(inVal: c_int, _name: *const c_char) -> bool {
    inVal != 0
}

// Apply runtime parameters to the system. This is a lightweight port and does
// not mutate global state directly here.
pub fn R_SetParams(_Rp: *mut c_void) {
    // Intentionally left as a no-op in this port.
}

// ---------------------------------------------------------------------------
// Public re-exports / module glue
// ---------------------------------------------------------------------------

// The real project wires up these functions via the `pub mod startup;` entry
// point. No additional exports are required here.
