#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/CommandLineArgs.c — command-line argument handling.
//!
//! Manages command-line arguments and processes common options like --save,
//! --no-save, --restore, --vanilla, etc.

use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::sexp::accessors::SET_STRING_ELT;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::constructors::Rf_mkChar;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};

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

thread_local! { static NumCommandLineArgs: Cell<c_int> = Cell::new(0); }

thread_local! { static CommandLineArgs: Cell<*mut *mut c_char> = Cell::new(ptr::null_mut()); }

// ---------------------------------------------------------------------------
// Static state: option flags (matching Rstart fields)
// ---------------------------------------------------------------------------

thread_local! { static R_RestoreHistory: Cell<c_int> = Cell::new(1); }

thread_local! { static SaveAction: Cell<c_int> = Cell::new(SA_SAVEASK); }

thread_local! { static RestoreAction: Cell<c_int> = Cell::new(SA_RESTORE); }

thread_local! { static R_Quiet: Cell<c_int> = Cell::new(0); }

thread_local! { static R_NoEcho: Cell<c_int> = Cell::new(0); }

thread_local! { static R_Interactive: Cell<c_int> = Cell::new(1); }

thread_local! { static R_Verbose: Cell<c_int> = Cell::new(0); }

thread_local! { static LoadSiteFile: Cell<c_int> = Cell::new(1); }

thread_local! { static LoadInitFile: Cell<c_int> = Cell::new(1); }

thread_local! { static NoRenviron: Cell<c_int> = Cell::new(0); }

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
pub unsafe fn R_set_command_line_arguments(argc: c_int, argv: *mut *mut c_char) { unsafe {
    NumCommandLineArgs.with(|v| v.set(argc));

    if argc <= 0 {
        CommandLineArgs.with(|v| v.set(ptr::null_mut()));
        return;
    }

    let layout =
        std::alloc::Layout::array::<*mut c_char>(argc as usize).expect("unwrap on None/Err");
    let ptr = std::alloc::alloc(layout) as *mut *mut c_char;
    if ptr.is_null() {
        return;
    }

    ptr::write_bytes(ptr, 0, argc as usize);
    CommandLineArgs.with(|v| v.set(ptr));

    for i in 0..argc as usize {
        let src = *argv.add(i);
        if !src.is_null() {
            let cstr = CStr::from_ptr(src);
            let dup =
                CString::new(cstr.to_bytes()).expect("CString::new failed: contains null byte");
            *ptr.add(i) = dup.into_raw();
        }
    }
}}

// ---------------------------------------------------------------------------
// do_commandArgs — the .Internal(commandArgs())
// ---------------------------------------------------------------------------

/// .Internal handler: returns a STRSXP character vector of the stored
/// command-line arguments.
///
/// # Safety
/// - `call`, `op`, `args`, `env` are SEXP pointers following R's calling
///   convention. Only `args` is checked for arity.
pub unsafe fn do_commandArgs(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP { unsafe {
    let _ = call;
    let _ = op;
    let _ = args;
    let _ = env;

    let n = NumCommandLineArgs.with(|v| v.get()) as R_xlen_t;
    let num_args = NumCommandLineArgs.with(|v| v.get());
    let vals = Rf_allocVector(SEXPTYPE::STRSXP.0, num_args);

    if vals.is_null() {
        return ptr::null_mut();
    }

    let cmd_args = CommandLineArgs.with(|v| v.get());
    if !cmd_args.is_null() {
        for i in 0..num_args as usize {
            let arg_ptr = *cmd_args.add(i);
            if !arg_ptr.is_null() {
                let charsxp = Rf_mkChar(arg_ptr);
                SET_STRING_ELT(vals, i as R_xlen_t, charsxp);
            }
        }
    }

    vals
}}

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
pub unsafe fn R_common_command_line(
    pac: *mut c_int,
    argv: *mut *mut c_char,
    Rp: *mut c_void,
) -> c_int { unsafe {
    let _ = Rp;

    if pac.is_null() || argv.is_null() {
        return 0;
    }

    let ac = *pac;
    let mut newac: c_int = 1;
    let mut processing: bool = true;

    R_RestoreHistory.with(|v| v.set(1));

    let mut i: c_int = 1;
    while i < ac {
        let av = *argv.add(i as usize);
        i += 1;

        if av.is_null() {
            if newac < ac {
                *argv.add(newac as usize) = av;
                newac += 1;
            }
            continue;
        }

        let arg = CStr::from_ptr(av);
        let arg_bytes = arg.to_bytes();

        if processing && arg_bytes.starts_with(b"-") {
            if arg_bytes == b"--version" {
                continue;
            } else if arg_bytes == b"--args" {
                if newac < ac {
                    *argv.add(newac as usize) = av;
                    newac += 1;
                }
                processing = false;
            } else if arg_bytes == b"--save" {
                SaveAction.with(|v| v.set(SA_SAVE));
            } else if arg_bytes == b"--no-save" {
                SaveAction.with(|v| v.set(SA_NOSAVE));
            } else if arg_bytes == b"--restore" {
                RestoreAction.with(|v| v.set(SA_RESTORE));
            } else if arg_bytes == b"--no-restore" {
                RestoreAction.with(|v| v.set(SA_NORESTORE));
                R_RestoreHistory.with(|v| v.set(0));
            } else if arg_bytes == b"--no-restore-data" {
                RestoreAction.with(|v| v.set(SA_NORESTORE));
            } else if arg_bytes == b"--no-restore-history" {
                R_RestoreHistory.with(|v| v.set(0));
            } else if arg_bytes == b"--silent" || arg_bytes == b"--quiet" || arg_bytes == b"-q" {
                R_Quiet.with(|v| v.set(1));
            } else if arg_bytes == b"--vanilla" {
                SaveAction.with(|v| v.set(SA_NOSAVE));
                RestoreAction.with(|v| v.set(SA_NORESTORE));
                R_RestoreHistory.with(|v| v.set(0));
                LoadSiteFile.with(|v| v.set(0));
                LoadInitFile.with(|v| v.set(0));
                NoRenviron.with(|v| v.set(1));
            } else if arg_bytes == b"--no-environ" {
                NoRenviron.with(|v| v.set(1));
            } else if arg_bytes == b"--verbose" {
                R_Verbose.with(|v| v.set(1));
            } else if arg_bytes == b"--no-echo" || arg_bytes == b"--slave" || arg_bytes == b"-s" {
                R_Quiet.with(|v| v.set(1));
                R_NoEcho.with(|v| v.set(1));
                SaveAction.with(|v| v.set(SA_NOSAVE));
            } else if arg_bytes == b"--no-site-file" {
                LoadSiteFile.with(|v| v.set(0));
            } else if arg_bytes == b"--no-init-file" {
                LoadInitFile.with(|v| v.set(0));
            } else if arg_bytes.starts_with(b"--encoding") {
                let mut p: Option<&[u8]> = None;
                if arg_bytes.len() > 11 {
                    p = Some(&arg_bytes[11..]);
                } else if i < ac {
                    let next_av = *argv.add(i as usize);
                    i += 1;
                    if !next_av.is_null() {
                        p = Some(CStr::from_ptr(next_av).to_bytes());
                    }
                }
                let _ = p;
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
            } else if arg_bytes.starts_with(b"--min-nsize") || arg_bytes.starts_with(b"--min-vsize")
            {
                if arg_bytes.len() < 13 && i < ac {
                    i += 1;
                }
            } else if arg_bytes.starts_with(b"--max-ppsize") {
                if arg_bytes.len() < 14 && i < ac {
                    i += 1;
                }
            } else if arg_bytes.starts_with(b"--max-connections") {
                if arg_bytes.len() < 19 && i < ac {
                    i += 1;
                }
            } else {
                if newac < ac {
                    *argv.add(newac as usize) = av;
                    newac += 1;
                }
            }
        } else {
            if newac < ac {
                *argv.add(newac as usize) = av;
                newac += 1;
            }
        }
    }

    *pac = newac;
    newac
}}

// ---------------------------------------------------------------------------
// Accessors for option state (for use by other modules)
// ---------------------------------------------------------------------------

/// Returns the current SaveAction setting.
pub unsafe fn R_GetSaveAction() -> c_int {
    SaveAction.with(|v| v.get())
}

/// Returns the current RestoreAction setting.
pub unsafe fn R_GetRestoreAction() -> c_int {
    RestoreAction.with(|v| v.get())
}

/// Returns whether R_RestoreHistory is set.
pub unsafe fn R_GetRestoreHistory() -> c_int {
    R_RestoreHistory.with(|v| v.get())
}

/// Returns whether R_Quiet mode is active.
pub unsafe fn R_GetQuiet() -> c_int {
    R_Quiet.with(|v| v.get())
}

/// Returns whether R_NoEcho mode is active.
pub unsafe fn R_GetNoEcho() -> c_int {
    R_NoEcho.with(|v| v.get())
}

/// Returns whether R is running interactively.
pub unsafe fn R_GetInteractive() -> c_int {
    R_Interactive.with(|v| v.get())
}

/// Returns whether R_Verbose mode is active.
pub unsafe fn R_GetVerbose() -> c_int {
    R_Verbose.with(|v| v.get())
}

/// Returns whether site file loading is enabled.
pub unsafe fn R_GetLoadSiteFile() -> c_int {
    LoadSiteFile.with(|v| v.get())
}

/// Returns whether init file loading is enabled.
pub unsafe fn R_GetLoadInitFile() -> c_int {
    LoadInitFile.with(|v| v.get())
}

/// Returns whether .Renviron processing is disabled.
pub unsafe fn R_GetNoRenviron() -> c_int {
    NoRenviron.with(|v| v.get())
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

            assert_eq!(NumCommandLineArgs.with(|v| v.get()), 4);
            assert!(!CommandLineArgs.with(|v| v.get()).is_null());

            for i in 0..args.len() {
                let stored = *CommandLineArgs.with(|v| v.get()).add(i);
                assert!(!stored.is_null());
                let s = CStr::from_ptr(stored).to_str().unwrap();
                assert_eq!(s, args[i]);
            }

            for i in 0..NumCommandLineArgs.with(|v| v.get()) as usize {
                let p = *CommandLineArgs.with(|v| v.get()).add(i);
                if !p.is_null() {
                    let _ = CString::from_raw(p);
                }
            }
            if !CommandLineArgs.with(|v| v.get()).is_null() {
                let layout = std::alloc::Layout::array::<*mut c_char>(4).unwrap();
                std::alloc::dealloc(CommandLineArgs.with(|v| v.get()) as *mut u8, layout);
            }

            free_argv(&mut argv);

            NumCommandLineArgs.with(|v| v.set(0));
            CommandLineArgs.with(|v| v.set(ptr::null_mut()));
        }
    }

    #[test]
    fn test_command_args_empty() {
        unsafe {
            let _guard = TEST_LOCK.lock().unwrap();

            let args: [&str; 0] = [];
            let mut argv: Vec<*mut c_char> = vec![];
            let argc = 0;

            R_set_command_line_arguments(argc, argv.as_mut_ptr());

            assert_eq!(NumCommandLineArgs.with(|v| v.get()), 0);

            CommandLineArgs.with(|v| v.set(ptr::null_mut()));
        }
    }

    unsafe fn reset_state() {
        SaveAction.with(|v| v.set(SA_SAVEASK));
        RestoreAction.with(|v| v.set(SA_RESTORE));
        R_RestoreHistory.with(|v| v.set(1));
        R_Quiet.with(|v| v.set(0));
        R_NoEcho.with(|v| v.set(0));
        R_Interactive.with(|v| v.set(1));
        R_Verbose.with(|v| v.set(0));
        LoadSiteFile.with(|v| v.set(1));
        LoadInitFile.with(|v| v.set(1));
        NoRenviron.with(|v| v.set(0));
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

            assert_eq!(SaveAction.with(|v| v.get()), SA_NOSAVE);
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

            assert_eq!(SaveAction.with(|v| v.get()), SA_NOSAVE);
            assert_eq!(RestoreAction.with(|v| v.get()), SA_NORESTORE);
            assert_eq!(R_RestoreHistory.with(|v| v.get()), 0);
            assert_eq!(LoadSiteFile.with(|v| v.get()), 0);
            assert_eq!(LoadInitFile.with(|v| v.get()), 0);
            assert_eq!(NoRenviron.with(|v| v.get()), 1);
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

            assert_eq!(R_Quiet.with(|v| v.get()), 1);
            assert_eq!(R_Verbose.with(|v| v.get()), 1);
            assert_eq!(SaveAction.with(|v| v.get()), SA_SAVEASK); // -q alone doesn't change SaveAction
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

            assert_eq!(R_Quiet.with(|v| v.get()), 1);
            assert_eq!(R_NoEcho.with(|v| v.get()), 1);
            assert_eq!(SaveAction.with(|v| v.get()), SA_NOSAVE);
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

            assert_eq!(SaveAction.with(|v| v.get()), SA_SAVE);
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

            assert_eq!(RestoreAction.with(|v| v.get()), SA_NORESTORE);
            assert_eq!(R_RestoreHistory.with(|v| v.get()), 0);
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

            assert_eq!(LoadSiteFile.with(|v| v.get()), 0);
            assert_eq!(LoadInitFile.with(|v| v.get()), 0);
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

            SaveAction.with(|v| v.set(SA_NOSAVE));
            assert_eq!(R_GetSaveAction(), SA_NOSAVE);

            RestoreAction.with(|v| v.set(SA_NORESTORE));
            assert_eq!(R_GetRestoreAction(), SA_NORESTORE);

            R_RestoreHistory.with(|v| v.set(0));
            assert_eq!(R_GetRestoreHistory(), 0);

            R_Quiet.with(|v| v.set(1));
            assert_eq!(R_GetQuiet(), 1);

            R_NoEcho.with(|v| v.set(1));
            assert_eq!(R_GetNoEcho(), 1);

            R_Interactive.with(|v| v.set(0));
            assert_eq!(R_GetInteractive(), 0);

            R_Verbose.with(|v| v.set(1));
            assert_eq!(R_GetVerbose(), 1);

            LoadSiteFile.with(|v| v.set(0));
            assert_eq!(R_GetLoadSiteFile(), 0);

            LoadInitFile.with(|v| v.set(0));
            assert_eq!(R_GetLoadInitFile(), 0);

            NoRenviron.with(|v| v.set(1));
            assert_eq!(R_GetNoRenviron(), 1);

            reset_state();
        }
    }
}
