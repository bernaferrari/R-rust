#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/unix/system.c -- R initialization and system interface.
//!
//! Implements `Rf_initialize_R` (the main R initialization entry point),
//! the function pointer dispatch table for system operations (console I/O,
//! cleanup, file viewing), and FD limit utilities.

use std::env;
use std::ffi::CString;
use std::io::{self, Write};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::sexp::instance::with_required_current_instance;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const R_PATH_MAX: usize = 4096;
const MSGSIZE: usize = R_PATH_MAX + 128;

/// Save action types (from R_ext/Constants.h).
const SA_SAVE: c_int = 1;
const SA_NOSAVE: c_int = 2;
const SA_DEFAULT: c_int = 0;
const SA_SUICIDE: c_int = 3;

/// C stack direction detection.
const C_STACK_DIRECTION: c_int = -1;

// ---------------------------------------------------------------------------
// Function pointer types for system interface
// ---------------------------------------------------------------------------

type PtrSuicide = Option<unsafe extern "C" fn(*const c_char)>;
type PtrShowMessage = Option<unsafe extern "C" fn(*const c_char)>;
// Read-console callbacks are installed as `unsafe extern "C" fn` by callers
// outside this module, so the slot keeps that type.
type PtrReadConsole = Option<unsafe extern "C" fn(*const c_char, *mut u8, c_int, c_int) -> c_int>;
type PtrWriteConsole = Option<unsafe extern "C" fn(*const c_char, c_int)>;
type PtrWriteConsoleEx = Option<unsafe extern "C" fn(*const c_char, c_int, c_int)>;
type PtrResetConsole = Option<extern "C" fn()>;
type PtrFlushConsole = Option<extern "C" fn()>;
type PtrClearerrConsole = Option<extern "C" fn()>;
type PtrBusy = Option<extern "C" fn(c_int)>;
type PtrCleanUp = Option<extern "C" fn(c_int, c_int, c_int)>;
type PtrShowFiles = Option<
    unsafe extern "C" fn(
        c_int,
        *const *const c_char,
        *const *const c_char,
        *const c_char,
        c_int,
        *const c_char,
    ) -> c_int,
>;
type PtrChooseFile = Option<unsafe extern "C" fn(c_int, *mut c_char, c_int) -> c_int>;
type PtrHistory = Option<unsafe extern "C" fn(*const c_char)>;
type PtrEditFiles = Option<
    unsafe extern "C" fn(c_int, *const *const c_char, *const *const c_char, *const c_char) -> c_int,
>;

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub(crate) struct UnixSystemCallbacks {
    suicide: PtrSuicide,
    show_message: PtrShowMessage,
    read_console: PtrReadConsole,
    write_console: PtrWriteConsole,
    write_console_ex: PtrWriteConsoleEx,
    reset_console: PtrResetConsole,
    flush_console: PtrFlushConsole,
    clearerr_console: PtrClearerrConsole,
    busy: PtrBusy,
    cleanup: PtrCleanUp,
    show_files: PtrShowFiles,
    choose_file: PtrChooseFile,
    load_history: PtrHistory,
    save_history: PtrHistory,
    add_history: PtrHistory,
    edit_files: PtrEditFiles,
}

pub(crate) struct UnixSystemRuntimeState {
    interactive: c_int,
    using_readline: c_int,
    running_as_main_program: c_int,
    home: CString,
    history_file: CString,
    history_size: c_int,
    restore_history: c_int,
    gui_type: Option<CString>,
    cstack_dir: c_int,
    cstack_limit: usize,
    cstack_start: usize,
    global_context: *mut c_void,
    num_initialized: c_int,
    callbacks: UnixSystemCallbacks,
}

impl Default for UnixSystemRuntimeState {
    fn default() -> Self {
        Self {
            interactive: 1,
            using_readline: 1,
            running_as_main_program: 0,
            home: CString::new("/usr/lib/R").unwrap(),
            history_file: CString::new(".Rhistory").unwrap(),
            history_size: 512,
            restore_history: 1,
            gui_type: None,
            cstack_dir: -1,
            cstack_limit: 0,
            cstack_start: 0,
            global_context: ptr::null_mut(),
            num_initialized: 0,
            callbacks: UnixSystemCallbacks::default(),
        }
    }
}

fn with_system_state<R>(f: impl FnOnce(&mut UnixSystemRuntimeState) -> R) -> R {
    // P1: the &mut UnixSystemRuntimeState lend spans only `f`; callers read
    // or write plain callback/config values without allocating or entering
    // the interpreter.
    with_required_current_instance(|instance| f(unsafe { &mut (*instance).unix_system_state }))
}

#[cfg(test)]
pub(crate) fn set_read_console_callback_for_tests(
    callback: Option<unsafe extern "C" fn(*const c_char, *mut u8, c_int, c_int) -> c_int>,
) {
    with_system_state(|state| {
        state.callbacks.read_console = callback;
    });
}

// ---------------------------------------------------------------------------
// Default implementations (Rstd_* equivalents)
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe extern "C" fn Rstd_Suicide(msg: *const c_char) {
    // SAFETY: the dispatch contract passes a NUL-terminated message.
    unsafe {
        let s = std::ffi::CStr::from_ptr(msg);
        eprintln!("FATAL: {}", s.to_string_lossy());
        std::process::exit(2);
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe extern "C" fn Rstd_ShowMessage(msg: *const c_char) {
    // SAFETY: the dispatch contract passes a NUL-terminated message.
    unsafe {
        let s = std::ffi::CStr::from_ptr(msg);
        eprintln!("{}", s.to_string_lossy());
    }
}

extern "C" fn Rstd_ReadConsole(
    _prompt: *const c_char,
    _buf: *mut u8,
    _len: c_int,
    _addtohistory: c_int,
) -> c_int {
    0
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe extern "C" fn Rstd_WriteConsole(buf: *const c_char, len: c_int) {
    // SAFETY: the dispatch contract passes `len` readable bytes at `buf`.
    unsafe {
        let slice = std::slice::from_raw_parts(buf as *const u8, len as usize);
        let _ = io::stdout().write_all(slice);
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe extern "C" fn Rstd_WriteConsoleEx(buf: *const c_char, len: c_int, _otype: c_int) {
    // SAFETY: the dispatch contract passes `len` readable bytes at `buf`.
    unsafe {
        let slice = std::slice::from_raw_parts(buf as *const u8, len as usize);
        let _ = io::stdout().write_all(slice);
    }
}

extern "C" fn Rstd_ResetConsole() {}
extern "C" fn Rstd_FlushConsole() {
    let _ = io::stdout().flush();
}
extern "C" fn Rstd_ClearerrConsole() {}
extern "C" fn Rstd_Busy(_which: c_int) {}

extern "C" fn Rstd_CleanUp(_saveact: c_int, status: c_int, _runLast: c_int) {
    std::process::exit(status);
}

extern "C" fn Rstd_ShowFiles(
    _nfile: c_int,
    _file: *const *const c_char,
    _headers: *const *const c_char,
    _wtitle: *const c_char,
    _del: c_int,
    _pager: *const c_char,
) -> c_int {
    0
}

extern "C" fn Rstd_ChooseFile(_new: c_int, _buf: *mut c_char, _len: c_int) -> c_int {
    0
}

extern "C" fn Rstd_loadhistory(_file: *const c_char) {}
extern "C" fn Rstd_savehistory(_file: *const c_char) {}
extern "C" fn Rstd_addhistory(_line: *const c_char) {}
fn Rstd_read_history(_file: *const c_char) {}

// ---------------------------------------------------------------------------
// Public system interface functions (dispatch through pointers)
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_Suicide(s: *const c_char) {
    if let Some(f) = with_system_state(|state| state.callbacks.suicide) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(s) };
    }
    std::process::exit(2);
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_ShowMessage(s: *const c_char) {
    if let Some(f) = with_system_state(|state| state.callbacks.show_message) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(s) };
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_ReadConsole(
    prompt: *const c_char,
    buf: *mut u8,
    len: c_int,
    addtohistory: c_int,
) -> c_int {
    if let Some(f) = with_system_state(|state| state.callbacks.read_console) {
        // SAFETY: callbacks are registered as `unsafe extern "C" fn`; the caller
        // upholds the prompt/buffer contract of the R_ReadConsole ABI.
        unsafe { f(prompt, buf, len, addtohistory) }
    } else {
        0
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_WriteConsole(buf: *const c_char, len: c_int) {
    let callbacks = with_system_state(|state| state.callbacks);
    if let Some(f) = callbacks.write_console {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(buf, len) };
    } else if let Some(f) = callbacks.write_console_ex {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(buf, len, 0) };
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_WriteConsoleEx(buf: *const c_char, len: c_int, otype: c_int) {
    let callbacks = with_system_state(|state| state.callbacks);
    if let Some(f) = callbacks.write_console {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(buf, len) };
    } else if let Some(f) = callbacks.write_console_ex {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(buf, len, otype) };
    }
}

pub fn R_ResetConsole() {
    if let Some(f) = with_system_state(|state| state.callbacks.reset_console) {
        f();
    }
}

pub fn R_FlushConsole() {
    if let Some(f) = with_system_state(|state| state.callbacks.flush_console) {
        f();
    }
}

pub fn R_ClearerrConsole() {
    if let Some(f) = with_system_state(|state| state.callbacks.clearerr_console) {
        f();
    }
}

pub fn R_Busy(which: c_int) {
    if let Some(f) = with_system_state(|state| state.callbacks.busy) {
        f(which);
    }
}

pub fn R_CleanUp(saveact: c_int, status: c_int, runLast: c_int) {
    if let Some(f) = with_system_state(|state| state.callbacks.cleanup) {
        f(saveact, status, runLast);
    }
    std::process::exit(status);
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_ShowFiles(
    nfile: c_int,
    file: *const *const c_char,
    headers: *const *const c_char,
    wtitle: *const c_char,
    del: c_int,
    pager: *const c_char,
) -> c_int {
    if let Some(f) = with_system_state(|state| state.callbacks.show_files) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(nfile, file, headers, wtitle, del, pager) }
    } else {
        0
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_ChooseFile(new: c_int, buf: *mut c_char, len: c_int) -> c_int {
    if let Some(f) = with_system_state(|state| state.callbacks.choose_file) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(new, buf, len) }
    } else {
        0
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn R_EditFiles(
    nfile: c_int,
    file: *const *const c_char,
    title: *const *const c_char,
    editor: *const c_char,
) -> c_int {
    if let Some(f) = with_system_state(|state| state.callbacks.edit_files) {
        // SAFETY: forwarded raw pointers satisfy the registered callback's contract per this function's own `# Safety` requirements.
        unsafe { f(nfile, file, title, editor) }
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// R_setupHistory
// ---------------------------------------------------------------------------

pub fn R_setupHistory() {
    let history_file = env::var("R_HISTFILE")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| CString::new(s).ok())
        .unwrap_or_else(|| CString::new(".Rhistory").unwrap());

    let history_size = env::var("R_HISTSIZE")
        .ok()
        .and_then(|s| s.parse::<c_int>().ok())
        .filter(|&val| val >= 0)
        .unwrap_or(512);

    with_system_state(|state| {
        state.history_file = history_file;
        state.history_size = history_size;
    });
}

pub fn R_GetFDLimit() -> c_int {
    // SAFETY: `rlim` is a valid out-pointer for getrlimit.
    unsafe {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) == 0 {
                let lim = rlim.rlim_cur as usize;
                if lim > c_int::MAX as usize {
                    return c_int::MAX;
                }
                return lim as c_int;
            }
        }
        #[cfg(target_os = "macos")]
        {
            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) == 0 {
                let lim = rlim.rlim_cur as usize;
                if lim > c_int::MAX as usize {
                    return c_int::MAX;
                }
                return lim as c_int;
            }
        }
        -1
    }
}

pub fn R_EnsureFDLimit(desired: c_int) -> c_int {
    // SAFETY: `rlim` is a valid out/in pointer for getrlimit/setrlimit.
    unsafe {
        #[cfg(unix)]
        {
            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) != 0 {
                return -1;
            }

            let cur = rlim.rlim_cur as c_int;
            let desired_usize = desired as libc::rlim_t;

            if rlim.rlim_cur == libc::RLIM_INFINITY || rlim.rlim_cur >= desired_usize {
                return desired;
            }

            if rlim.rlim_max == libc::RLIM_INFINITY || rlim.rlim_max >= desired_usize {
                rlim.rlim_cur = desired_usize;
            } else {
                rlim.rlim_cur = rlim.rlim_max;
            }

            if libc::setrlimit(libc::RLIMIT_NOFILE, &rlim) != 0 {
                return cur;
            }

            rlim.rlim_cur as c_int
        }
        #[cfg(not(unix))]
        {
            let _ = desired;
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Rf_initialize_R
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn unescape_arg(src: *const c_char, dst: *mut c_char) -> *mut c_char {
    // SAFETY: `src` is a NUL-terminated C string and `dst` has room for the
    // unescaped result; the caller (Rf_initialize_R) owns both buffers.
    unsafe {
        let mut q = dst;
        let mut p = src;
        while *p != 0 {
            if *p == b'~' as c_char && *p.add(1) == b'+' as c_char && *p.add(2) == b'~' as c_char {
                p = p.add(2);
                *q = b' ' as c_char;
            } else if *p == b'~' as c_char
                && *p.add(1) == b'n' as c_char
                && *p.add(2) == b'~' as c_char
            {
                p = p.add(2);
                *q = b'\n' as c_char;
            } else if *p == b'~' as c_char
                && *p.add(1) == b't' as c_char
                && *p.add(2) == b'~' as c_char
            {
                p = p.add(2);
                *q = b'\t' as c_char;
            } else {
                *q = *p;
            }
            q = q.add(1);
            p = p.add(1);
        }
        *q = 0;
        q
    }
}

fn R_Decode2Long(_s: *const c_char, _ierr: *mut c_int) -> i64 {
    0
}

fn R_HomeDir() -> CString {
    CString::new("/usr/lib/R").unwrap()
}

fn BindDomain(_home: *const c_char) {}
fn process_system_Renviron() {}
fn process_site_Renviron() {}
fn process_user_Renviron() {}
fn R_set_command_line_arguments(_ac: c_int, _av: *mut *mut c_char) {}
fn R_common_command_line(_ac: *const c_int, _av: *mut *mut c_char, _rp: *mut c_void) {}
fn R_DefParamsEx(_rp: *mut c_void, _version: c_int) {}
fn R_SetParams(_rp: *mut c_void) {}
fn R_SizeFromEnv(_rp: *mut c_void) {}
fn R_isatty(_fd: c_int) -> c_int {
    0
}
fn R_isWriteableDir(_path: *const c_char) -> c_int {
    0
}
fn R_setStartTime() {}
fn fpu_setup(_start: c_int) {}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn Rf_initialize_R(ac: c_int, av: *mut *mut c_char) -> c_int {
    unsafe {
        if with_system_state(|state| state.num_initialized) != 0 {
            eprintln!("R is already initialized\n");
            std::process::exit(1);
        }
        with_system_state(|state| state.num_initialized = 1);

        // --- Stack detection ---
        with_system_state(|state| state.cstack_dir = C_STACK_DIRECTION);

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_STACK, &mut rlim) == 0 {
                let lim = rlim.rlim_cur;
                if lim != libc::RLIM_INFINITY {
                    with_system_state(|state| state.cstack_limit = lim as usize);
                }
            }
            if with_system_state(|state| state.cstack_start) == 0 {
                with_system_state(|state| state.cstack_start = 0);
            }
        }

        #[cfg(target_os = "macos")]
        {
            let mut base: usize = 0;
            let mut len: usize = std::mem::size_of::<*const c_void>();
            let KERN_USRSTACK: core::ffi::c_int = 33;
            let mut mib: [core::ffi::c_int; 2] = [libc::CTL_KERN, KERN_USRSTACK];
            if libc::sysctl(
                mib.as_mut_ptr(),
                2,
                &mut base as *mut _ as *mut c_void,
                &mut len,
                ptr::null_mut(),
                0,
            ) == 0
            {
                with_system_state(|state| state.cstack_start = base);
            }

            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_STACK, &mut rlim) == 0 {
                let lim = rlim.rlim_cur;
                if lim != libc::RLIM_INFINITY {
                    with_system_state(|state| state.cstack_limit = lim as usize);
                }
            }
        }

        if with_system_state(|state| state.cstack_start) == usize::MAX {
            with_system_state(|state| state.cstack_limit = usize::MAX);
        }

        // --- Set up function pointer dispatch table ---
        with_system_state(|state| {
            state.callbacks = UnixSystemCallbacks {
                suicide: Some(Rstd_Suicide),
                show_message: Some(Rstd_ShowMessage),
                read_console: Some(Rstd_ReadConsole),
                write_console: Some(Rstd_WriteConsole),
                write_console_ex: None,
                reset_console: Some(Rstd_ResetConsole),
                flush_console: Some(Rstd_FlushConsole),
                clearerr_console: Some(Rstd_ClearerrConsole),
                busy: Some(Rstd_Busy),
                cleanup: Some(Rstd_CleanUp),
                show_files: Some(Rstd_ShowFiles),
                choose_file: Some(Rstd_ChooseFile),
                load_history: Some(Rstd_loadhistory),
                save_history: Some(Rstd_savehistory),
                add_history: Some(Rstd_addhistory),
                edit_files: None,
            };
            state.global_context = ptr::null_mut();
        });

        // --- R home and environment ---
        let home = R_HomeDir();
        let home_ptr = with_system_state(|state| {
            state.home = home;
            state.home.as_ptr()
        });
        if home_ptr.is_null() {
            R_Suicide(b"R home directory is not defined\0".as_ptr() as *const c_char);
        }
        BindDomain(home_ptr);
        process_system_Renviron();
        R_setStartTime();

        // --- Process command line ---
        R_DefParamsEx(ptr::null_mut(), 0);
        R_set_command_line_arguments(ac, av);
        R_common_command_line(&ac, av, ptr::null_mut());

        // Process remaining arguments
        let argc = ac;
        let mut argv = av;
        let mut force_interactive: bool = false;
        let mut save_action: c_int = SA_DEFAULT;

        let mut remaining = argc;
        while remaining > 0 {
            remaining -= 1;
            argv = argv.add(1);
            let arg = *argv;
            if arg.is_null() {
                break;
            }
            if *arg == b'-' as c_char {
                let arg_bytes = std::ffi::CStr::from_ptr(arg).to_bytes();
                if arg_bytes == b"--no-readline" {
                    with_system_state(|state| state.using_readline = 0);
                } else if arg_bytes == b"--vanilla" {
                    save_action = SA_NOSAVE;
                } else if arg_bytes == b"--save" {
                    save_action = SA_SAVE;
                } else if arg_bytes == b"--nosave" {
                    save_action = SA_NOSAVE;
                } else if arg_bytes == b"--interactive" {
                    force_interactive = true;
                    break;
                } else if arg_bytes == b"--args" {
                    break;
                }
            }
        }

        // --- Interactive mode ---
        if force_interactive || R_isatty(0) != 0 {
            with_system_state(|state| state.interactive = 1);
        } else {
            with_system_state(|state| state.interactive = 0);
        }

        // --- Save action check ---
        if with_system_state(|state| state.interactive) == 0
            && save_action != SA_SAVE
            && save_action != SA_NOSAVE
        {
            R_Suicide(
                b"you must specify '--save', '--no-save' or '--vanilla'\0".as_ptr()
                    as *const c_char,
            );
        }

        // --- History ---
        R_setupHistory();

        // --- FPU setup ---
        fpu_setup(1);

        0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utils_flushconsole_dispatches_session_callback_and_checks_arity() {
        std::thread_local! {
            static FLUSHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }
        extern "C" fn flush() {
            FLUSHES.set(FLUSHES.get() + 1);
            crate::sexp::gengc::full_gc();
        }
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| {
            FLUSHES.set(0);
            with_system_state(|state| state.callbacks.flush_console = Some(flush));
            let routine = crate::library::utils::lookup("C_flushconsole").unwrap();
            unsafe {
                assert_eq!(
                    routine.invoke_call(&[]).unwrap(),
                    crate::sexp::globals::R_NilValue()
                );
                assert_eq!(FLUSHES.get(), 1);
                assert!(
                    routine
                        .invoke_call(&[crate::sexp::globals::R_NilValue()])
                        .is_err()
                );
                assert_eq!(FLUSHES.get(), 1);
            }
            with_system_state(|state| state.callbacks.flush_console = None);
        });
    }

    #[test]
    fn test_system_dispatch_nulls() {
        let _session = crate::sexp::session::RSession::new();
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { R_ShowMessage(b"test\0".as_ptr() as *const c_char) };
    }

    #[test]
    fn test_setup_history() {
        let _session = crate::sexp::session::RSession::new();
        R_setupHistory();
        with_system_state(|state| {
            assert_eq!(state.history_file.to_string_lossy(), ".Rhistory");
            assert_eq!(state.history_size, 512);
        });
    }

    #[test]
    fn unix_system_runtime_state_is_session_local() {
        use crate::sexp::instance::{RInstance, replace_current_instance};

        let mut first = RInstance::new();
        let mut second = RInstance::new();

        unsafe {
            let previous = replace_current_instance(Some(&mut first as *mut RInstance));
            R_setupHistory();
            first.unix_system_state.history_size = 1024;
            first.unix_system_state.using_readline = 0;
            first.unix_system_state.callbacks.show_message = Some(Rstd_ShowMessage);
            replace_current_instance(previous);

            let previous = replace_current_instance(Some(&mut second as *mut RInstance));
            R_setupHistory();
            second.unix_system_state.history_size = 2048;
            second.unix_system_state.callbacks.show_message = None;
            replace_current_instance(previous);
        }

        assert_eq!(first.unix_system_state.history_size, 1024);
        assert_eq!(first.unix_system_state.using_readline, 0);
        assert!(first.unix_system_state.callbacks.show_message.is_some());
        assert_eq!(second.unix_system_state.history_size, 2048);
        assert_eq!(second.unix_system_state.using_readline, 1);
        assert!(second.unix_system_state.callbacks.show_message.is_none());
    }

    #[test]
    fn test_get_fd_limit() {
        let limit = R_GetFDLimit();
        #[cfg(unix)]
        assert!(limit > 0);
    }

    #[test]
    fn test_ensure_fd_limit() {
        let result = R_EnsureFDLimit(256);
        #[cfg(unix)]
        assert!(result > 0 || result == -1);
    }

    #[test]
    fn test_unescape_arg() {
        let src = b"hello~+~world\0";
        let mut dst = [0i8; 16];
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let end = unsafe { unescape_arg(src.as_ptr() as *const c_char, dst.as_mut_ptr()) };
        let written = (end as usize) - (dst.as_mut_ptr() as usize);
        assert_eq!(written, 11);
        assert_eq!(&dst[..11], b"hello world".map(|b| b as i8));
    }
}
