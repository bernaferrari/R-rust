#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/CommandLineArgs.c — command-line argument handling.
//!
//! Manages command-line arguments and processes common options like --save,
//! --no-save, --restore, --vanilla, etc.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::mainutils::startup::StartupRuntimeState;
use crate::sexp::accessors::SET_STRING_ELT;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::constructors::Rf_mkChar;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::instance::with_required_current_instance;

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

fn with_startup_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut StartupRuntimeState) -> R,
{
    with_required_current_instance(|inst| f(&mut inst.startup_state))
}

// ---------------------------------------------------------------------------
// R_set_command_line_arguments
// ---------------------------------------------------------------------------

/// Copy the command-line arguments to session-owned storage.
///
/// This is called at startup to store a copy of argc/argv that can later
/// be retrieved via `commandArgs()`. R's C implementation intentionally leaks
/// these startup copies for process lifetime; this port keeps them owned by the
/// active `RInstance` so independent sessions do not share or leak argv state.
///
/// # Safety
/// - `argv` must point to a valid array of at least `argc` C-string pointers.
/// - Each `argv[i]` must be a valid null-terminated C string.
pub unsafe fn R_set_command_line_arguments(argc: c_int, argv: *mut *mut c_char) {
    unsafe {
        with_startup_state(|state| {
            state.command_line_args.clear();
        });

        if argc <= 0 || argv.is_null() {
            return;
        }

        let copied = (0..argc as usize)
            .map(|i| {
                let src = *argv.add(i);
                (!src.is_null()).then(|| CStr::from_ptr(src).to_owned())
            })
            .collect::<Vec<_>>();

        with_startup_state(|state| {
            state.command_line_args = copied;
        });
    }
}

#[derive(Clone, Copy)]
struct CommandLineOptionState {
    restore_history: c_int,
    save_action: c_int,
    restore_action: c_int,
    quiet: c_int,
    no_echo: c_int,
    interactive: c_int,
    verbose: c_int,
    load_site_file: c_int,
    load_init_file: c_int,
    no_renviron: c_int,
}

impl CommandLineOptionState {
    fn read() -> Self {
        with_startup_state(|state| Self {
            restore_history: state.restore_history,
            save_action: state.save_action,
            restore_action: state.restore_action,
            quiet: state.quiet,
            no_echo: state.no_echo,
            interactive: state.interactive,
            verbose: state.verbose,
            load_site_file: state.load_site_file,
            load_init_file: state.load_init_file,
            no_renviron: state.no_renviron,
        })
    }

    fn write(self) {
        with_startup_state(|state| {
            state.restore_history = self.restore_history;
            state.save_action = self.save_action;
            state.restore_action = self.restore_action;
            state.quiet = self.quiet;
            state.no_echo = self.no_echo;
            state.interactive = self.interactive;
            state.verbose = self.verbose;
            state.load_site_file = self.load_site_file;
            state.load_init_file = self.load_init_file;
            state.no_renviron = self.no_renviron;
        });
    }

    fn reset_to_defaults() {
        Self {
            restore_history: 1,
            save_action: SA_SAVEASK,
            restore_action: SA_RESTORE,
            quiet: 0,
            no_echo: 0,
            interactive: 1,
            verbose: 0,
            load_site_file: 1,
            load_init_file: 1,
            no_renviron: 0,
        }
        .write();
    }

    fn get(field: impl FnOnce(&Self) -> c_int) -> c_int {
        let state = Self::read();
        field(&state)
    }
}

#[cfg(test)]
fn command_line_args_for_test() -> Vec<String> {
    with_startup_state(|state| {
        state
            .command_line_args
            .iter()
            .map(|arg| {
                arg.as_ref()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect()
    })
}

pub(crate) fn sync_eval_control_from_command_line() {
    with_required_current_instance(|inst| {
        inst.eval_state.quiet = inst.startup_state.quiet;
        inst.eval_state.no_echo = inst.startup_state.no_echo;
        inst.eval_state.interactive = inst.startup_state.interactive;
        inst.eval_state.verbose = inst.startup_state.verbose;
    });
}

#[cfg(test)]
fn set_interactive(value: bool) {
    with_startup_state(|state| {
        state.interactive = c_int::from(value);
    });
    with_required_current_instance(|inst| {
        inst.eval_state.interactive = c_int::from(value);
    });
}

#[cfg(test)]
fn set_no_echo(value: bool) {
    with_startup_state(|state| {
        state.no_echo = c_int::from(value);
    });
    with_required_current_instance(|inst| {
        inst.eval_state.no_echo = c_int::from(value);
    });
}

#[cfg(test)]
fn set_quiet(value: bool) {
    with_startup_state(|state| {
        state.quiet = c_int::from(value);
    });
    with_required_current_instance(|inst| {
        inst.eval_state.quiet = c_int::from(value);
    });
}

#[cfg(test)]
fn set_verbose(value: bool) {
    with_startup_state(|state| {
        state.verbose = c_int::from(value);
    });
    with_required_current_instance(|inst| {
        inst.eval_state.verbose = c_int::from(value);
    });
}

#[cfg(test)]
fn set_restore_history(value: bool) {
    with_startup_state(|state| {
        state.restore_history = c_int::from(value);
    });
}

#[cfg(test)]
fn set_save_action(value: c_int) {
    with_startup_state(|state| {
        state.save_action = value;
    });
}

#[cfg(test)]
fn set_restore_action(value: c_int) {
    with_startup_state(|state| {
        state.restore_action = value;
    });
}

#[cfg(test)]
fn set_load_site_file(value: bool) {
    with_startup_state(|state| {
        state.load_site_file = c_int::from(value);
    });
}

#[cfg(test)]
fn set_load_init_file(value: bool) {
    with_startup_state(|state| {
        state.load_init_file = c_int::from(value);
    });
}

#[cfg(test)]
fn set_no_renviron(value: bool) {
    with_startup_state(|state| {
        state.no_renviron = c_int::from(value);
    });
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

        let command_line_args = with_startup_state(|state| state.command_line_args.clone());
        let num_args = command_line_args.len().min(c_int::MAX as usize);
        let vals = Rf_allocVector(SEXPTYPE::STRSXP, num_args as c_int);

        if vals.is_null() {
            return ptr::null_mut();
        }

        for (i, arg) in command_line_args.iter().take(num_args).enumerate() {
            if let Some(arg) = arg {
                let charsxp = Rf_mkChar(arg.as_ptr());
                SET_STRING_ELT(vals, i as R_xlen_t, charsxp);
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
/// Startup options are stored on the active `RInstance`, not in process-global
/// statics, so multiple sessions can parse command lines independently. The
/// `Rp` parameter is accepted for source compatibility but unused.
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
) -> c_int {
    unsafe {
        let _ = Rp;

        if pac.is_null() || argv.is_null() {
            return 0;
        }

        let ac = *pac;
        let mut newac: c_int = 1;
        let mut processing: bool = true;
        let mut options = CommandLineOptionState::read();
        options.restore_history = 1;

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
                    options.save_action = SA_SAVE;
                } else if arg_bytes == b"--no-save" {
                    options.save_action = SA_NOSAVE;
                } else if arg_bytes == b"--restore" {
                    options.restore_action = SA_RESTORE;
                } else if arg_bytes == b"--no-restore" {
                    options.restore_action = SA_NORESTORE;
                    options.restore_history = 0;
                } else if arg_bytes == b"--no-restore-data" {
                    options.restore_action = SA_NORESTORE;
                } else if arg_bytes == b"--no-restore-history" {
                    options.restore_history = 0;
                } else if arg_bytes == b"--silent" || arg_bytes == b"--quiet" || arg_bytes == b"-q"
                {
                    options.quiet = 1;
                } else if arg_bytes == b"--vanilla" {
                    options.save_action = SA_NOSAVE;
                    options.restore_action = SA_NORESTORE;
                    options.restore_history = 0;
                    options.load_site_file = 0;
                    options.load_init_file = 0;
                    options.no_renviron = 1;
                } else if arg_bytes == b"--no-environ" {
                    options.no_renviron = 1;
                } else if arg_bytes == b"--verbose" {
                    options.verbose = 1;
                } else if arg_bytes == b"--no-echo" || arg_bytes == b"--slave" || arg_bytes == b"-s"
                {
                    options.quiet = 1;
                    options.no_echo = 1;
                    options.save_action = SA_NOSAVE;
                } else if arg_bytes == b"--no-site-file" {
                    options.load_site_file = 0;
                } else if arg_bytes == b"--no-init-file" {
                    options.load_init_file = 0;
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
                } else if arg_bytes.starts_with(b"--min-nsize")
                    || arg_bytes.starts_with(b"--min-vsize")
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
        options.write();
        sync_eval_control_from_command_line();
        newac
    }
}

// ---------------------------------------------------------------------------
// Accessors for option state (for use by other modules)
// ---------------------------------------------------------------------------

/// Returns the current SaveAction setting.
pub unsafe fn R_GetSaveAction() -> c_int {
    CommandLineOptionState::get(|state| state.save_action)
}

/// Returns the current RestoreAction setting.
pub unsafe fn R_GetRestoreAction() -> c_int {
    CommandLineOptionState::get(|state| state.restore_action)
}

/// Returns whether R_RestoreHistory is set.
pub unsafe fn R_GetRestoreHistory() -> c_int {
    CommandLineOptionState::get(|state| state.restore_history)
}

/// Returns whether R_Quiet mode is active.
pub unsafe fn R_GetQuiet() -> c_int {
    CommandLineOptionState::get(|state| state.quiet)
}

/// Returns whether R_NoEcho mode is active.
pub unsafe fn R_GetNoEcho() -> c_int {
    CommandLineOptionState::get(|state| state.no_echo)
}

/// Returns whether R is running interactively.
pub unsafe fn R_GetInteractive() -> c_int {
    CommandLineOptionState::get(|state| state.interactive)
}

/// Returns whether R_Verbose mode is active.
pub unsafe fn R_GetVerbose() -> c_int {
    CommandLineOptionState::get(|state| state.verbose)
}

/// Returns whether site file loading is enabled.
pub unsafe fn R_GetLoadSiteFile() -> c_int {
    CommandLineOptionState::get(|state| state.load_site_file)
}

/// Returns whether init file loading is enabled.
pub unsafe fn R_GetLoadInitFile() -> c_int {
    CommandLineOptionState::get(|state| state.load_init_file)
}

/// Returns whether .Renviron processing is disabled.
pub unsafe fn R_GetNoRenviron() -> c_int {
    CommandLineOptionState::get(|state| state.no_renviron)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};
    use crate::sexp::session::RSession;
    use std::ffi::CString;

    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test failed: {e:?}"),
        }
    }

    /// Helper: build a null-terminated argv array from Rust strings.
    /// Uses libc::malloc so we can safely free after R_common_command_line
    /// rearranges the pointers in the array (no CString ownership tracking).
    unsafe fn make_argv(args: &[&str]) -> Vec<*mut c_char> {
        args.iter()
            .map(|s| {
                let cstr = CString::new(*s).unwrap_or_default();
                let len = cstr.as_bytes_with_nul().len();
                let ptr = unsafe { libc::malloc(len) as *mut c_char };
                unsafe { ptr::copy_nonoverlapping(cstr.as_ptr(), ptr, len) };
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
                unsafe { libc::free(p as *mut c_void) };
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
            let _session = RSession::new();

            let args = ["R", "--vanilla", "-e", "print(1)"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let argc = args.len() as c_int;

            R_set_command_line_arguments(argc, argv.as_mut_ptr());

            assert_eq!(command_line_args_for_test(), args);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_command_args_empty() {
        unsafe {
            let _session = RSession::new();

            let args: [&str; 0] = [];
            let mut argv: Vec<*mut c_char> = vec![];
            let argc = 0;

            R_set_command_line_arguments(argc, argv.as_mut_ptr());

            assert!(command_line_args_for_test().is_empty());
        }
    }

    #[test]
    fn command_line_args_are_session_local() {
        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            let first_args = ["R", "--first"];
            let mut first_argv = make_argv(&first_args);
            R_set_command_line_arguments(first_args.len() as c_int, first_argv.as_mut_ptr());
            assert_eq!(command_line_args_for_test(), first_args);

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            let second_args = ["R", "--second", "script.R"];
            let mut second_argv = make_argv(&second_args);
            R_set_command_line_arguments(second_args.len() as c_int, second_argv.as_mut_ptr());
            assert_eq!(command_line_args_for_test(), second_args);

            set_current_instance(&mut first);
            assert_eq!(command_line_args_for_test(), first_args);

            clear_current_instance();
            free_argv(&mut first_argv);
            free_argv(&mut second_argv);
        }
    }

    fn reset_state() {
        CommandLineOptionState::reset_to_defaults();
        sync_eval_control_from_command_line();
    }

    #[test]
    fn test_common_command_line_save_no_save() {
        unsafe {
            let _session = RSession::new();
            reset_state();

            let args = ["R", "--save", "--no-save", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(R_GetSaveAction(), SA_NOSAVE);
            // argv[0] (R) + unknown "script.R" = 2
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_vanilla() {
        unsafe {
            let _session = RSession::new();
            reset_state();

            let args = ["R", "--vanilla", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(R_GetSaveAction(), SA_NOSAVE);
            assert_eq!(R_GetRestoreAction(), SA_NORESTORE);
            assert_eq!(R_GetRestoreHistory(), 0);
            assert_eq!(R_GetLoadSiteFile(), 0);
            assert_eq!(R_GetLoadInitFile(), 0);
            assert_eq!(R_GetNoRenviron(), 1);
            assert_eq!(argc, 2); // R + script.R

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_quiet_verbose() {
        unsafe {
            let _session = RSession::new();
            reset_state();

            let args = ["R", "-q", "--verbose", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(R_GetQuiet(), 1);
            assert_eq!(R_GetVerbose(), 1);
            assert_eq!(R_GetSaveAction(), SA_SAVEASK); // -q alone doesn't change SaveAction
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_slave() {
        unsafe {
            let _session = RSession::new();
            reset_state();

            let args = ["R", "--slave", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(R_GetQuiet(), 1);
            assert_eq!(R_GetNoEcho(), 1);
            assert_eq!(R_GetSaveAction(), SA_NOSAVE);
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_args_separator() {
        unsafe {
            let _session = RSession::new();
            reset_state();

            // Everything after --args passes through, even if it looks like an option
            let args = ["R", "--save", "--args", "--no-save", "file.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(R_GetSaveAction(), SA_SAVE);
            // argv[0] + --args + --no-save + file.R = 4
            assert_eq!(argc, 4);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_restore_options() {
        unsafe {
            let _session = RSession::new();
            reset_state();

            let args = ["R", "--no-restore-data", "--no-restore-history", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(R_GetRestoreAction(), SA_NORESTORE);
            assert_eq!(R_GetRestoreHistory(), 0);
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_no_site_init_file() {
        unsafe {
            let _session = RSession::new();
            reset_state();

            let args = ["R", "--no-site-file", "--no-init-file", "script.R"];
            let mut argv: Vec<*mut c_char> = make_argv(&args);
            let mut argc = args.len() as c_int;

            R_common_command_line(&mut argc, argv.as_mut_ptr(), ptr::null_mut());

            assert_eq!(R_GetLoadSiteFile(), 0);
            assert_eq!(R_GetLoadInitFile(), 0);
            assert_eq!(argc, 2);

            free_argv(&mut argv);
        }
    }

    #[test]
    fn test_common_command_line_unknown_option_pass_through() {
        unsafe {
            let _session = RSession::new();
            reset_state();

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
            let _session = RSession::new();
            reset_state();

            set_save_action(SA_NOSAVE);
            assert_eq!(R_GetSaveAction(), SA_NOSAVE);

            set_restore_action(SA_NORESTORE);
            assert_eq!(R_GetRestoreAction(), SA_NORESTORE);

            set_restore_history(false);
            assert_eq!(R_GetRestoreHistory(), 0);

            set_quiet(true);
            assert_eq!(R_GetQuiet(), 1);

            set_no_echo(true);
            assert_eq!(R_GetNoEcho(), 1);

            set_interactive(false);
            assert_eq!(R_GetInteractive(), 0);

            set_verbose(true);
            assert_eq!(R_GetVerbose(), 1);

            set_load_site_file(false);
            assert_eq!(R_GetLoadSiteFile(), 0);

            set_load_init_file(false);
            assert_eq!(R_GetLoadInitFile(), 0);

            set_no_renviron(true);
            assert_eq!(R_GetNoRenviron(), 1);

            reset_state();
        }
    }
}
