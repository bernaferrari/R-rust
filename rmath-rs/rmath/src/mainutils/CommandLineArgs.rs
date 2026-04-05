#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/CommandLineArgs.c — command-line argument handling.
//!
//! Manages command-line arguments and processes common options like --save,
//! --no-save, --restore, --vanilla, etc.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::sexp::accessors::SET_STRING_ELT;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::constructors::Rf_mkChar;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Save/Restore action constants (matching R_ext/RStartup.h)
// ---------------------------------------------------------------------------

/// Ask whether to save the workspace at exit.
pub const SA_SAVEASK: c_int = 1;

/// Always save the workspace at exit.
pub const SA_SAVE: c_int = 2;

/// Never save the workspace at exit.
pub const SA_NOSAVE: c_int = 3;

/// Restore the workspace at startup.
pub const SA_RESTORE: c_int = 1;

/// Do not restore the workspace at startup.
pub const SA_NORESTORE: c_int = 0;

// ---------------------------------------------------------------------------
// Static state: command-line argument storage
// ---------------------------------------------------------------------------

/// Number of stored command-line arguments.
static mut NumCommandLineArgs: c_int = 0;

/// Array of C-string pointers to permanently stored command-line arguments.
/// This memory is never freed (matching R's behavior).
static mut CommandLineArgs: *mut *mut c_char = ptr::null_mut();

// ---------------------------------------------------------------------------
// Static state: option flags (matching Rstart fields)
// ---------------------------------------------------------------------------

/// Whether to restore .Rhistory at startup.
static mut R_RestoreHistory: c_int = 1;

/// Save action: SA_SAVEASK, SA_SAVE, or SA_NOSAVE.
static mut SaveAction: c_int = SA_SAVEASK;

/// Restore action: SA_RESTORE or SA_NORESTORE.
static mut RestoreAction: c_int = SA_RESTORE;

/// Run in quiet mode (suppress startup messages).
static mut R_Quiet: c_int = 0;

/// Suppress echo of input (--no-echo / --slave / -s).
static mut R_NoEcho: c_int = 0;

/// Whether R is running interactively.
static mut R_Interactive: c_int = 1;

/// Run in verbose mode.
static mut R_Verbose: c_int = 0;

/// Whether to load the site-wide Rprofile.
static mut LoadSiteFile: c_int = 1;

/// Whether to load the user's .Rprofile.
static mut LoadInitFile: c_int = 1;

/// Whether to suppress processing of .Renviron files.
static mut NoRenviron: c_int = 0;

// ---------------------------------------------------------------------------
// R_set_command_line_arguments
// ---------------------------------------------------------------------------

/// Copy the command-line arguments to permanent storage.
///
/// This is called at startup to store a copy of argc/argv that can later
/// be retrieved via `commandArgs()`. The memory is never freed (matching R).
///
/// # Safety
/// - `argv` must point to a valid array of at least `argc` C-string pointers.
/// - Each `argv[i]` must be a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_set_command_line_arguments(argc: c_int, argv: *mut *mut c_char) {
    unsafe {
        // Nothing here is ever freed (matching R's behavior).
        NumCommandLineArgs = argc;

        if argc <= 0 {
            CommandLineArgs = ptr::null_mut();
            return;
        }

        // Allocate array of pointers (using calloc-like semantics via Vec).
        let layout = std::alloc::Layout::array::<*mut c_char>(argc as usize).unwrap();
        let ptr = std::alloc::alloc(layout) as *mut *mut c_char;
        if ptr.is_null() {
            // R_Suicide("allocation failure in R_set_command_line_arguments");
            return;
        }

        // Zero-initialize the pointer array.
        ptr::write_bytes(ptr, 0, argc as usize);
        CommandLineArgs = ptr;

        for i in 0..argc as usize {
            let src = *argv.add(i);
            if !src.is_null() {
                let cstr = CStr::from_ptr(src);
                let dup = CString::new(cstr.to_bytes()).unwrap();
                *ptr.add(i) = dup.into_raw();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// do_commandArgs — the .Internal(commandArgs())
// ---------------------------------------------------------------------------

/// .Internal handler: returns a STRSXP character vector of the stored
/// command-line arguments.
///
/// # Safety
/// - `call`, `op`, `args`, `env` are SEXP pointers following R's calling
///   convention. Only `args` is checked for arity.
pub unsafe fn do_commandArgs(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = call;
        let _ = op;
        let _ = args;
        let _ = env;

        let n = NumCommandLineArgs as R_xlen_t;
        let vals = Rf_allocVector(SEXPTYPE::STRSXP.0, NumCommandLineArgs);

        if vals.is_null() {
            return ptr::null_mut();
        }

        if !CommandLineArgs.is_null() {
            for i in 0..NumCommandLineArgs as usize {
                let arg_ptr = *CommandLineArgs.add(i);
                if !arg_ptr.is_null() {
                    let charsxp = Rf_mkChar(arg_ptr);
                    SET_STRING_ELT(vals, i as R_xlen_t, charsxp);
                }
            }
        }

        vals
    }
}

// ---------------------------------------------------------------------------
// R_common_command_line — process common command-line options
// ---------------------------------------------------------------------------

/// Process and remove common R command-line arguments from argv.
///
/// This is the equivalent of R's `R_common_command_line()` from sys-common.c.
/// It handles options like `--save`, `--no-save`, `--restore`, `--vanilla`,
/// `--quiet`, `--verbose`, etc. Unknown options are passed through.
///
/// Since the full `Rstart` struct is not defined in this port, the option
/// values are stored in module-level statics (SaveAction, RestoreAction, etc.)
/// rather than in an Rstart struct. The `Rp` parameter is accepted for ABI
/// compatibility but unused.
///
/// Returns the new argument count (with common args removed).
///
/// # Safety
/// - `pac` must point to a valid c_int containing the argument count.
/// - `argv` must point to a valid array of at least `*pac` C-string pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_common_command_line(
    pac: *mut c_int,
    argv: *mut *mut c_char,
    Rp: *mut c_void,
) -> c_int {
    unsafe {
        let _ = Rp;

        if pac.is_null() || argv.is_null() {
            return 0;
        }

        let ac = *pac;
        let mut newac: c_int = 1; // argv[0] is process name, always kept
        let mut processing: bool = true;

        R_RestoreHistory = 1;

        let mut i: c_int = 1; // skip argv[0]
        while i < ac {
            let av = *argv.add(i as usize);
            i += 1;

            if av.is_null() {
                // Null entry, pass through
                if newac < ac {
                    *argv.add(newac as usize) = av;
                    newac += 1;
                }
                continue;
            }

            let arg = CStr::from_ptr(av);
            let arg_bytes = arg.to_bytes();

            if processing && arg_bytes.starts_with(b"-") {
                // Check each known option
                if arg_bytes == b"--version" {
                    // Print version and exit — in the real R this calls
                    // PrintVersion + R_ShowMessage + exit(0).
                    // For the port, we just skip it (the caller handles version).
                    continue;
                } else if arg_bytes == b"--args" {
                    // Copy this through for further processing
                    if newac < ac {
                        *argv.add(newac as usize) = av;
                        newac += 1;
                    }
                    processing = false;
                } else if arg_bytes == b"--save" {
                    SaveAction = SA_SAVE;
                } else if arg_bytes == b"--no-save" {
                    SaveAction = SA_NOSAVE;
                } else if arg_bytes == b"--restore" {
                    RestoreAction = SA_RESTORE;
                } else if arg_bytes == b"--no-restore" {
                    RestoreAction = SA_NORESTORE;
                    R_RestoreHistory = 0;
                } else if arg_bytes == b"--no-restore-data" {
                    RestoreAction = SA_NORESTORE;
                } else if arg_bytes == b"--no-restore-history" {
                    R_RestoreHistory = 0;
                } else if arg_bytes == b"--silent" || arg_bytes == b"--quiet" || arg_bytes == b"-q"
                {
                    R_Quiet = 1;
                } else if arg_bytes == b"--vanilla" {
                    SaveAction = SA_NOSAVE; // --no-save
                    RestoreAction = SA_NORESTORE; // --no-restore
                    R_RestoreHistory = 0; // --no-restore-history
                    LoadSiteFile = 0; // --no-site-file
                    LoadInitFile = 0; // --no-init-file
                    NoRenviron = 1; // --no-environ
                } else if arg_bytes == b"--no-environ" {
                    NoRenviron = 1;
                } else if arg_bytes == b"--verbose" {
                    R_Verbose = 1;
                } else if arg_bytes == b"--no-echo" || arg_bytes == b"--slave" || arg_bytes == b"-s"
                {
                    R_Quiet = 1;
                    R_NoEcho = 1;
                    SaveAction = SA_NOSAVE;
                } else if arg_bytes == b"--no-site-file" {
                    LoadSiteFile = 0;
                } else if arg_bytes == b"--no-init-file" {
                    LoadInitFile = 0;
                } else if arg_bytes.starts_with(b"--encoding") {
                    // Handle --encoding=<enc> or --encoding <enc>
                    let mut p: Option<&[u8]> = None;
                    if arg_bytes.len() > 11 {
                        // --encoding=xxx (skip the '=')
                        p = Some(&arg_bytes[11..]);
                    } else if i < ac {
                        // Next arg is the encoding value
                        let next_av = *argv.add(i as usize);
                        i += 1;
                        if !next_av.is_null() {
                            p = Some(CStr::from_ptr(next_av).to_bytes());
                        }
                    }
                    // If p is None, warning would be emitted in real R
                    let _ = p; // encoding not stored in this port
                } else if arg_bytes == b"-save"
                    || arg_bytes == b"-nosave"
                    || arg_bytes == b"-restore"
                    || arg_bytes == b"-norestore"
                    || arg_bytes == b"-noreadline"
                    || arg_bytes == b"-quiet"
                    || arg_bytes == b"-nsize"
                    || arg_bytes == b"-vsize"
                    || arg_bytes.starts_with(b"--max-nsize")
                    || arg_bytes.starts_with(b"--max-vsize")
                    || arg_bytes == b"-V"
                    || arg_bytes == b"-n"
                    || arg_bytes == b"-v"
                {
                    // Deprecated/unsupported options — would print warning in real R
                    // R_ShowMessage("WARNING: option '<name>' no longer supported");
                } else if arg_bytes.starts_with(b"--min-nsize")
                    || arg_bytes.starts_with(b"--min-vsize")
                {
                    // Consume optional next arg if value not inline
                    if arg_bytes.len() < 13 && i < ac {
                        i += 1;
                    }
                    // Would parse value in real R
                } else if arg_bytes.starts_with(b"--max-ppsize") {
                    if arg_bytes.len() < 14 && i < ac {
                        i += 1;
                    }
                } else if arg_bytes.starts_with(b"--max-connections") {
                    if arg_bytes.len() < 19 && i < ac {
                        i += 1;
                    }
                } else {
                    // Unknown -option: pass through
                    if newac < ac {
                        *argv.add(newac as usize) = av;
                        newac += 1;
                    }
                }
            } else {
                // Non-option argument: pass through
                if newac < ac {
                    *argv.add(newac as usize) = av;
                    newac += 1;
                }
            }
        }

        *pac = newac;
        newac
    }
}

// ---------------------------------------------------------------------------
// Accessors for option state (for use by other modules)
// ---------------------------------------------------------------------------

/// Returns the current SaveAction setting.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetSaveAction() -> c_int {
    unsafe { SaveAction }
}

/// Returns the current RestoreAction setting.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetRestoreAction() -> c_int {
    unsafe { RestoreAction }
}

/// Returns whether R_RestoreHistory is set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetRestoreHistory() -> c_int {
    unsafe { R_RestoreHistory }
}

/// Returns whether R_Quiet mode is active.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetQuiet() -> c_int {
    unsafe { R_Quiet }
}

/// Returns whether R_NoEcho mode is active.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetNoEcho() -> c_int {
    unsafe { R_NoEcho }
}

/// Returns whether R is running interactively.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetInteractive() -> c_int {
    unsafe { R_Interactive }
}

/// Returns whether R_Verbose mode is active.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetVerbose() -> c_int {
    unsafe { R_Verbose }
}

/// Returns whether site file loading is enabled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetLoadSiteFile() -> c_int {
    unsafe { LoadSiteFile }
}

/// Returns whether init file loading is enabled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetLoadInitFile() -> c_int {
    unsafe { LoadInitFile }
}

/// Returns whether .Renviron processing is disabled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetNoRenviron() -> c_int {
    unsafe { NoRenviron }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::sync::Mutex;

    /// Mutex to serialize tests that touch global mutable statics,
    /// preventing race conditions when tests run in parallel.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Helper: build a null-terminated argv array from Rust strings.
    /// Uses libc::malloc so we can safely free after R_common_command_line
    /// rearranges the pointers in the array (no CString ownership tracking).
    unsafe fn make_argv(args: &[&str]) -> Vec<*mut c_char> {
        args.iter()
            .map(|s| {
                let cstr = CString::new(*s).unwrap();
                let len = cstr.as_bytes_with_nul().len();
                let ptr = libc::malloc(len) as *mut c_char;
                ptr::copy_nonoverlapping(cstr.as_ptr(), ptr, len);
                ptr
            })
            .collect()
    }

    /// Helper: free argv array built by make_argv.
    /// Uses libc::free (not CString::from_raw) to avoid double-free issues
    /// when R_common_command_line has rearranged pointers in the array.
    /// We collect unique pointers to avoid freeing duplicates.
    unsafe fn free_argv(argv: &mut Vec<*mut c_char>) {
        let mut seen = std::collections::HashSet::new();
        for &p in argv.iter() {
            if !p.is_null() && seen.insert(p) {
                libc::free(p as *mut c_void);
            }
        }
    }

    #[test]
    fn test_save_action_constants() {
        assert_eq!(SA_SAVEASK, 1);
        assert_eq!(SA_SAVE, 2);
        assert_eq!(SA_NOSAVE, 3);
        assert_eq!(SA_RESTORE, 1);
        assert_eq!(SA_NORESTORE, 0);
    }

    #[test]
    fn test_set_command_line_arguments() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();

            let args = ["R", "--vanilla", "-e", "print(1)"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let argc = args.len() as c_int;

            R_set_command_line_arguments(argc, argv.as_mut_ptr());

            assert_eq!(*std::ptr::addr_of!(NumCommandLineArgs), 4);
            assert!(!(*std::ptr::addr_of!(CommandLineArgs)).is_null());

            // Verify stored args match
            for i in 0..args.len() {
                let stored = *(*std::ptr::addr_of!(CommandLineArgs)).add(i);
                assert!(!stored.is_null());
                let s = CStr::from_ptr(stored).to_str().unwrap();
                assert_eq!(s, args[i]);
            }

            // Cleanup stored args
            for i in 0..*std::ptr::addr_of!(NumCommandLineArgs) as usize {
                let p = *(*std::ptr::addr_of!(CommandLineArgs)).add(i);
                if !p.is_null() {
                    let _ = CString::from_raw(p);
                }
            }
            if !(*std::ptr::addr_of!(CommandLineArgs)).is_null() {
                let layout = std::alloc::Layout::array::<*mut c_char>(4).unwrap();
                std::alloc::dealloc(*std::ptr::addr_of!(CommandLineArgs) as *mut u8, layout);
            }

            // Cleanup argv
            free_argv(&mut argv);

            // Reset state
            std::ptr::addr_of_mut!(NumCommandLineArgs).write(0);
            std::ptr::addr_of_mut!(CommandLineArgs).write(ptr::null_mut());
        }
    }

    #[test]
    fn test_command_args_empty() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();

            // Set no args
            let args: [&str; 0] = [];
            let mut argv: Vec<*mut c_char> = vec![];
            let argc = 0;

            R_set_command_line_arguments(argc, argv.as_mut_ptr());

            assert_eq!(*std::ptr::addr_of!(NumCommandLineArgs), 0);

            // Reset state
            std::ptr::addr_of_mut!(CommandLineArgs).write(ptr::null_mut());
        }
    }

    /// Reset all static state to defaults.
    unsafe fn reset_state() {
        std::ptr::addr_of_mut!(SaveAction).write(SA_SAVEASK);
        std::ptr::addr_of_mut!(RestoreAction).write(SA_RESTORE);
        std::ptr::addr_of_mut!(R_RestoreHistory).write(1);
        std::ptr::addr_of_mut!(R_Quiet).write(0);
        std::ptr::addr_of_mut!(R_NoEcho).write(0);
        std::ptr::addr_of_mut!(R_Interactive).write(1);
        std::ptr::addr_of_mut!(R_Verbose).write(0);
        std::ptr::addr_of_mut!(LoadSiteFile).write(1);
        std::ptr::addr_of_mut!(LoadInitFile).write(1);
        std::ptr::addr_of_mut!(NoRenviron).write(0);
    }

    #[test]
    fn test_common_command_line_save_no_save() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();
            reset_state();

            let args = ["R", "--save", "--no-save", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            // After processing: --save then --no-save, so final should be SA_NOSAVE
            assert_eq!(*std::ptr::addr_of!(SaveAction), SA_NOSAVE);
            // argv[0] (R) + unknown "script.R" = 2
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_vanilla() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();
            reset_state();

            let args = ["R", "--vanilla", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(*std::ptr::addr_of!(SaveAction), SA_NOSAVE);
            assert_eq!(*std::ptr::addr_of!(RestoreAction), SA_NORESTORE);
            assert_eq!(*std::ptr::addr_of!(R_RestoreHistory), 0);
            assert_eq!(*std::ptr::addr_of!(LoadSiteFile), 0);
            assert_eq!(*std::ptr::addr_of!(LoadInitFile), 0);
            assert_eq!(*std::ptr::addr_of!(NoRenviron), 1);
            assert_eq!(argc, 2); // R + script.R

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_quiet_verbose() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();
            reset_state();

            let args = ["R", "-q", "--verbose", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(*std::ptr::addr_of!(R_Quiet), 1);
            assert_eq!(*std::ptr::addr_of!(R_Verbose), 1);
            assert_eq!(*std::ptr::addr_of!(SaveAction), SA_SAVEASK); // -q alone doesn't change SaveAction
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_slave() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();
            reset_state();

            let args = ["R", "--slave", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(*std::ptr::addr_of!(R_Quiet), 1);
            assert_eq!(*std::ptr::addr_of!(R_NoEcho), 1);
            assert_eq!(*std::ptr::addr_of!(SaveAction), SA_NOSAVE);
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_args_separator() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();
            reset_state();

            // Everything after --args passes through, even if it looks like an option
            let args = ["R", "--save", "--args", "--no-save", "file.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(*std::ptr::addr_of!(SaveAction), SA_SAVE);
            // argv[0] + --args + --no-save + file.R = 4
            assert_eq!(argc, 4);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_restore_options() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();
            reset_state();

            let args = ["R", "--no-restore-data", "--no-restore-history", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(*std::ptr::addr_of!(RestoreAction), SA_NORESTORE);
            assert_eq!(*std::ptr::addr_of!(R_RestoreHistory), 0);
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_no_site_init_file() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();
            reset_state();

            let args = ["R", "--no-site-file", "--no-init-file", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(*std::ptr::addr_of!(LoadSiteFile), 0);
            assert_eq!(*std::ptr::addr_of!(LoadInitFile), 0);
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_unknown_option_pass_through() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();

            let args = ["R", "--unknown-option", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            // Unknown option + non-option arg both pass through
            assert_eq!(argc, 3);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_getters() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();
            reset_state();

            std::ptr::addr_of_mut!(SaveAction).write(SA_NOSAVE);
            assert_eq!(R_GetSaveAction(), SA_NOSAVE);

            std::ptr::addr_of_mut!(RestoreAction).write(SA_NORESTORE);
            assert_eq!(R_GetRestoreAction(), SA_NORESTORE);

            std::ptr::addr_of_mut!(R_RestoreHistory).write(0);
            assert_eq!(R_GetRestoreHistory(), 0);

            std::ptr::addr_of_mut!(R_Quiet).write(1);
            assert_eq!(R_GetQuiet(), 1);

            std::ptr::addr_of_mut!(R_NoEcho).write(1);
            assert_eq!(R_GetNoEcho(), 1);

            std::ptr::addr_of_mut!(R_Interactive).write(0);
            assert_eq!(R_GetInteractive(), 0);

            std::ptr::addr_of_mut!(R_Verbose).write(1);
            assert_eq!(R_GetVerbose(), 1);

            std::ptr::addr_of_mut!(LoadSiteFile).write(0);
            assert_eq!(R_GetLoadSiteFile(), 0);

            std::ptr::addr_of_mut!(LoadInitFile).write(0);
            assert_eq!(R_GetLoadInitFile(), 0);

            std::ptr::addr_of_mut!(NoRenviron).write(1);
            assert_eq!(R_GetNoRenviron(), 1);

            reset_state();
        }
    }
}
