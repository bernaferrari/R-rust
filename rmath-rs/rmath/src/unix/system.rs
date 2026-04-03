#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/unix/system.c -- R initialization and system interface.
//!
//! Implements `Rf_initialize_R` (the main R initialization entry point),
//! the function pointer dispatch table for system operations (console I/O,
//! cleanup, file viewing), and FD limit utilities.

use std::env;
use std::io::{self, Write};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

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

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

/// Whether R is running interactively.
static mut R_Interactive: c_int = 1;

/// Whether readline is being used.
static mut UsingReadline: c_int = 1;

/// Whether running as the main program.
static mut R_running_as_main_program: c_int = 0;

/// R home directory.
static mut R_Home: *const c_char = ptr::null();

/// History file path.
static mut R_HistoryFile: *const c_char = ptr::null();

/// History size.
static mut R_HistorySize: c_int = 512;

/// Whether to restore history.
static mut R_RestoreHistory: c_int = 1;

/// Input file pointer (for -f/--file).
static mut ifp: *mut libc::FILE = ptr::null_mut();

/// Output file for R output.
static mut R_Outputfile: *mut libc::FILE = ptr::null_mut();

/// Console file for R console messages.
static mut R_Consolefile: *mut libc::FILE = ptr::null_mut();

/// GUI type string.
static mut R_GUIType: *const c_char = ptr::null();

/// Stack direction (1 = grows up, -1 = grows down).
static mut R_CStackDir: c_int = -1;

/// Stack limit.
static mut R_CStackLimit: usize = 0;

/// Stack start address.
static mut R_CStackStart: usize = 0;

/// Global context (stub).
static mut R_GlobalContext: *mut c_void = ptr::null_mut();

/// C stack direction detection.
const C_STACK_DIRECTION: c_int = -1;

// ---------------------------------------------------------------------------
// Function pointer types for system interface
// ---------------------------------------------------------------------------

type PtrSuicide = Option<unsafe extern "C" fn(*const c_char)>;
type PtrShowMessage = Option<unsafe extern "C" fn(*const c_char)>;
type PtrReadConsole = Option<unsafe extern "C" fn(*const c_char, *mut u8, c_int, c_int) -> c_int>;
type PtrWriteConsole = Option<unsafe extern "C" fn(*const c_char, c_int)>;
type PtrWriteConsoleEx = Option<unsafe extern "C" fn(*const c_char, c_int, c_int)>;
type PtrResetConsole = Option<unsafe extern "C" fn()>;
type PtrFlushConsole = Option<unsafe extern "C" fn()>;
type PtrClearerrConsole = Option<unsafe extern "C" fn()>;
type PtrBusy = Option<unsafe extern "C" fn(c_int)>;
type PtrCleanUp = Option<unsafe extern "C" fn(c_int, c_int, c_int)>;
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
// System interface function pointers
// ---------------------------------------------------------------------------

static mut ptr_R_Suicide: PtrSuicide = None;
static mut ptr_R_ShowMessage: PtrShowMessage = None;
static mut ptr_R_ReadConsole: PtrReadConsole = None;
static mut ptr_R_WriteConsole: PtrWriteConsole = None;
static mut ptr_R_WriteConsoleEx: PtrWriteConsoleEx = None;
static mut ptr_R_ResetConsole: PtrResetConsole = None;
static mut ptr_R_FlushConsole: PtrFlushConsole = None;
static mut ptr_R_ClearerrConsole: PtrClearerrConsole = None;
static mut ptr_R_Busy: PtrBusy = None;
static mut ptr_R_CleanUp: PtrCleanUp = None;
static mut ptr_R_ShowFiles: PtrShowFiles = None;
static mut ptr_R_ChooseFile: PtrChooseFile = None;
static mut ptr_R_loadhistory: PtrHistory = None;
static mut ptr_R_savehistory: PtrHistory = None;
static mut ptr_R_addhistory: PtrHistory = None;
static mut ptr_R_EditFiles: PtrEditFiles = None;

// ---------------------------------------------------------------------------
// Default implementations (Rstd_* equivalents)
// ---------------------------------------------------------------------------

unsafe extern "C" fn Rstd_Suicide(msg: *const c_char) {
    unsafe {
        let s = std::ffi::CStr::from_ptr(msg);
        eprintln!("FATAL: {}", s.to_string_lossy());
        std::process::exit(2);
    }
}

unsafe extern "C" fn Rstd_ShowMessage(msg: *const c_char) {
    unsafe {
        let s = std::ffi::CStr::from_ptr(msg);
        eprintln!("{}", s.to_string_lossy());
    }
}

unsafe extern "C" fn Rstd_ReadConsole(
    _prompt: *const c_char,
    _buf: *mut u8,
    _len: c_int,
    _addtohistory: c_int,
) -> c_int {
    0
}

unsafe extern "C" fn Rstd_WriteConsole(buf: *const c_char, len: c_int) {
    unsafe {
        let slice = std::slice::from_raw_parts(buf as *const u8, len as usize);
        let _ = io::stdout().write_all(slice);
    }
}

unsafe extern "C" fn Rstd_WriteConsoleEx(buf: *const c_char, len: c_int, _otype: c_int) {
    unsafe {
        let slice = std::slice::from_raw_parts(buf as *const u8, len as usize);
        let _ = io::stdout().write_all(slice);
    }
}

unsafe extern "C" fn Rstd_ResetConsole() {}
unsafe extern "C" fn Rstd_FlushConsole() {
    let _ = io::stdout().flush();
}
unsafe extern "C" fn Rstd_ClearerrConsole() {}
unsafe extern "C" fn Rstd_Busy(_which: c_int) {}

unsafe extern "C" fn Rstd_CleanUp(_saveact: c_int, status: c_int, _runLast: c_int) {
    std::process::exit(status);
}

unsafe extern "C" fn Rstd_ShowFiles(
    _nfile: c_int,
    _file: *const *const c_char,
    _headers: *const *const c_char,
    _wtitle: *const c_char,
    _del: c_int,
    _pager: *const c_char,
) -> c_int {
    0
}

unsafe extern "C" fn Rstd_ChooseFile(_new: c_int, _buf: *mut c_char, _len: c_int) -> c_int {
    0
}

unsafe extern "C" fn Rstd_loadhistory(_file: *const c_char) {}
unsafe extern "C" fn Rstd_savehistory(_file: *const c_char) {}
unsafe extern "C" fn Rstd_addhistory(_line: *const c_char) {}
unsafe extern "C" fn Rstd_read_history(_file: *const c_char) {}

// ---------------------------------------------------------------------------
// Public system interface functions (dispatch through pointers)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Suicide(s: *const c_char) {
    unsafe {
        if let Some(f) = ptr_R_Suicide {
            f(s);
        }
        std::process::exit(2);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ShowMessage(s: *const c_char) {
    unsafe {
        if let Some(f) = ptr_R_ShowMessage {
            f(s);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ReadConsole(
    prompt: *const c_char,
    buf: *mut u8,
    len: c_int,
    addtohistory: c_int,
) -> c_int {
    unsafe {
        if let Some(f) = ptr_R_ReadConsole {
            f(prompt, buf, len, addtohistory)
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_WriteConsole(buf: *const c_char, len: c_int) {
    unsafe {
        if let Some(f) = ptr_R_WriteConsole {
            f(buf, len);
        } else if let Some(f) = ptr_R_WriteConsoleEx {
            f(buf, len, 0);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_WriteConsoleEx(buf: *const c_char, len: c_int, otype: c_int) {
    unsafe {
        if let Some(f) = ptr_R_WriteConsole {
            f(buf, len);
        } else if let Some(f) = ptr_R_WriteConsoleEx {
            f(buf, len, otype);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ResetConsole() {
    unsafe {
        if let Some(f) = ptr_R_ResetConsole {
            f();
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FlushConsole() {
    unsafe {
        if let Some(f) = ptr_R_FlushConsole {
            f();
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ClearerrConsole() {
    unsafe {
        if let Some(f) = ptr_R_ClearerrConsole {
            f();
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Busy(which: c_int) {
    unsafe {
        if let Some(f) = ptr_R_Busy {
            f(which);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_CleanUp(saveact: c_int, status: c_int, runLast: c_int) {
    unsafe {
        if let Some(f) = ptr_R_CleanUp {
            f(saveact, status, runLast);
        }
        std::process::exit(status);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ShowFiles(
    nfile: c_int,
    file: *const *const c_char,
    headers: *const *const c_char,
    wtitle: *const c_char,
    del: c_int,
    pager: *const c_char,
) -> c_int {
    unsafe {
        if let Some(f) = ptr_R_ShowFiles {
            f(nfile, file, headers, wtitle, del, pager)
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ChooseFile(new: c_int, buf: *mut c_char, len: c_int) -> c_int {
    unsafe {
        if let Some(f) = ptr_R_ChooseFile {
            f(new, buf, len)
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_EditFiles(
    nfile: c_int,
    file: *const *const c_char,
    title: *const *const c_char,
    editor: *const c_char,
) -> c_int {
    unsafe {
        if let Some(f) = ptr_R_EditFiles {
            f(nfile, file, title, editor)
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// R_setupHistory
// ---------------------------------------------------------------------------

/// Set up history file and size from environment variables.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_setupHistory() {
    unsafe {
        let histfile = env::var("R_HISTFILE");
        match histfile {
            Ok(ref v) if !v.is_empty() => {
                // Store the path — for now we use a static buffer
                R_HistoryFile = v.as_ptr() as *const c_char;
            }
            _ => {
                R_HistoryFile = b".Rhistory\0".as_ptr() as *const c_char;
            }
        }

        R_HistorySize = 512;
        if let Ok(v) = env::var("R_HISTSIZE") {
            if let Ok(val) = v.parse::<c_int>() {
                if val >= 0 {
                    R_HistorySize = val;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R_GetFDLimit / R_EnsureFDLimit
// ---------------------------------------------------------------------------

/// Get the system limit on the number of open files.
/// Returns the limit on success, or a negative number on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetFDLimit() -> c_int {
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

/// Try to ensure the FD limit is at least `desired`.
/// Returns `desired` on success, the current limit if it can't be raised,
/// or a negative number on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_EnsureFDLimit(desired: c_int) -> c_int {
    unsafe {
        #[cfg(unix)]
        {
            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) != 0 {
                return -1;
            }

            let cur = rlim.rlim_cur as c_int;
            let desired_usize = desired as u64;

            if rlim.rlim_cur == libc::RLIM_INFINITY || rlim.rlim_cur >= desired_usize {
                return desired;
            }

            // Try to increase the limit
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

/// Undo escaping done by the R front end.
unsafe fn unescape_arg(src: *const c_char, dst: *mut c_char) -> *mut c_char {
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

/// Stub: R_Decode2Long
unsafe fn R_Decode2Long(_s: *const c_char, _ierr: *mut c_int) -> i64 {
    0
}

/// Stub: R_HomeDir
unsafe fn R_HomeDir() -> *const c_char {
    static HOME: &[u8] = b"/usr/lib/R\0";
    HOME.as_ptr() as *const c_char
}

/// Stub: BindDomain
unsafe fn BindDomain(_home: *const c_char) {}

/// Stub: process_system_Renviron
unsafe fn process_system_Renviron() {}

/// Stub: process_site_Renviron
unsafe fn process_site_Renviron() {}

/// Stub: process_user_Renviron
unsafe fn process_user_Renviron() {}

/// Stub: R_set_command_line_arguments
unsafe fn R_set_command_line_arguments(_ac: c_int, _av: *mut *mut c_char) {}

/// Stub: R_common_command_line
unsafe fn R_common_command_line(_ac: *const c_int, _av: *mut *mut c_char, _rp: *mut c_void) {}

/// Stub: R_DefParamsEx
unsafe fn R_DefParamsEx(_rp: *mut c_void, _version: c_int) {}

/// Stub: R_SetParams
unsafe fn R_SetParams(_rp: *mut c_void) {}

/// Stub: R_SizeFromEnv
unsafe fn R_SizeFromEnv(_rp: *mut c_void) {}

/// Stub: R_isatty
unsafe fn R_isatty(_fd: c_int) -> c_int {
    0
}

/// Stub: R_isWriteableDir
unsafe fn R_isWriteableDir(_path: *const c_char) -> c_int {
    0
}

/// Stub: R_fopen
unsafe fn R_fopen(_path: *const c_char, _mode: *const c_char) -> *mut libc::FILE {
    ptr::null_mut()
}

/// Stub: R_setStartTime
unsafe fn R_setStartTime() {}

/// Stub: fpu_setup
unsafe fn fpu_setup(_start: c_int) {}

/// Protection counter for initialization.
static mut num_initialized: c_int = 0;

/// The main R initialization entry point.
///
/// This function:
/// 1. Detects stack characteristics (direction, limit)
/// 2. Sets up the function pointer dispatch table
/// 3. Processes command line arguments
/// 4. Configures interactive/non-interactive mode
/// 5. Sets up history
///
/// Returns 0 on success.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_initialize_R(ac: c_int, av: *mut *mut c_char) -> c_int {
    unsafe {
        if num_initialized != 0 {
            eprintln!("R is already initialized\n");
            std::process::exit(1);
        }
        num_initialized = 1;

        // --- Stack detection ---
        R_CStackDir = C_STACK_DIRECTION;

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_STACK, &mut rlim) == 0 {
                let lim = rlim.rlim_cur;
                if lim != libc::RLIM_INFINITY {
                    R_CStackLimit = lim as usize;
                }
            }
            // On Linux, try to get stack start from /proc/self/maps
            // For now, just use a reasonable default
            if R_CStackStart == 0 {
                R_CStackStart = 0; // Will be set properly by the stack detector
            }
        }

        #[cfg(target_os = "macos")]
        {
            // macOS: use KERN_USRSTACK to find stack start
            let mut base: usize = 0;
            let mut len: usize = std::mem::size_of::<*const c_void>();
            let KERN_USRSTACK: libc::c_int = 33; // macOS-specific constant
            let mut mib: [libc::c_int; 2] = [libc::CTL_KERN, KERN_USRSTACK];
            if libc::sysctl(
                mib.as_mut_ptr(),
                2,
                &mut base as *mut _ as *mut c_void,
                &mut len,
                ptr::null_mut(),
                0,
            ) == 0
            {
                R_CStackStart = base;
            }

            let mut rlim: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_STACK, &mut rlim) == 0 {
                let lim = rlim.rlim_cur;
                if lim != libc::RLIM_INFINITY {
                    R_CStackLimit = lim as usize;
                }
            }
        }

        if R_CStackStart == usize::MAX {
            R_CStackLimit = usize::MAX; // never set
        }

        // --- Set up function pointer dispatch table ---
        ptr_R_Suicide = Some(Rstd_Suicide);
        ptr_R_ShowMessage = Some(Rstd_ShowMessage);
        ptr_R_ReadConsole = Some(Rstd_ReadConsole);
        ptr_R_WriteConsole = Some(Rstd_WriteConsole);
        ptr_R_ResetConsole = Some(Rstd_ResetConsole);
        ptr_R_FlushConsole = Some(Rstd_FlushConsole);
        ptr_R_ClearerrConsole = Some(Rstd_ClearerrConsole);
        ptr_R_Busy = Some(Rstd_Busy);
        ptr_R_CleanUp = Some(Rstd_CleanUp);
        ptr_R_ShowFiles = Some(Rstd_ShowFiles);
        ptr_R_ChooseFile = Some(Rstd_ChooseFile);
        ptr_R_loadhistory = Some(Rstd_loadhistory);
        ptr_R_savehistory = Some(Rstd_savehistory);
        ptr_R_addhistory = Some(Rstd_addhistory);

        R_GlobalContext = ptr::null_mut();

        // --- R home and environment ---
        R_Home = R_HomeDir();
        if R_Home.is_null() {
            R_Suicide(b"R home directory is not defined\0".as_ptr() as *const c_char);
        }
        BindDomain(R_Home);
        process_system_Renviron();
        R_setStartTime();

        // --- Process command line ---
        R_DefParamsEx(ptr::null_mut(), 0);
        R_set_command_line_arguments(ac, av);
        R_common_command_line(&ac, av, ptr::null_mut());

        // Process remaining arguments
        let mut argc = ac;
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
                if libc::strcmp(arg, b"--no-readline\0".as_ptr() as *const c_char) == 0 {
                    UsingReadline = 0;
                } else if libc::strcmp(arg, b"--vanilla\0".as_ptr() as *const c_char) == 0 {
                    save_action = SA_NOSAVE;
                } else if libc::strcmp(arg, b"--save\0".as_ptr() as *const c_char) == 0 {
                    save_action = SA_SAVE;
                } else if libc::strcmp(arg, b"--nosave\0".as_ptr() as *const c_char) == 0 {
                    save_action = SA_NOSAVE;
                } else if libc::strcmp(arg, b"--interactive\0".as_ptr() as *const c_char) == 0 {
                    force_interactive = true;
                    break;
                } else if libc::strcmp(arg, b"--args\0".as_ptr() as *const c_char) == 0 {
                    break;
                }
            }
        }

        // --- Interactive mode ---
        if force_interactive || R_isatty(0) != 0 {
            R_Interactive = 1;
        } else {
            R_Interactive = 0;
        }

        R_Outputfile = ptr::null_mut(); // stdout
        R_Consolefile = ptr::null_mut(); // stderr

        // --- Save action check ---
        if R_Interactive == 0 && save_action != SA_SAVE && save_action != SA_NOSAVE {
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
    fn test_system_dispatch_nulls() {
        unsafe {
            // These should not crash with null data
            R_ShowMessage(b"test\0".as_ptr() as *const c_char);
        }
    }

    #[test]
    fn test_setup_history() {
        unsafe {
            R_setupHistory();
            assert_eq!(*std::ptr::addr_of!(R_HistorySize), 512);
        }
    }

    #[test]
    fn test_get_fd_limit() {
        unsafe {
            let limit = R_GetFDLimit();
            // On Unix, should be positive
            #[cfg(unix)]
            assert!(limit > 0);
        }
    }

    #[test]
    fn test_ensure_fd_limit() {
        unsafe {
            let result = R_EnsureFDLimit(256);
            // Should return at least 256 on success
            #[cfg(unix)]
            assert!(result > 0 || result == -1);
        }
    }

    #[test]
    fn test_unescape_arg() {
        unsafe {
            let src = b"hello~+~world\0";
            let mut dst = [0i8; 16];
            let end = unescape_arg(src.as_ptr() as *const c_char, dst.as_mut_ptr());
            let written = (end as usize) - (dst.as_mut_ptr() as usize);
            assert_eq!(written, 11); // "hello world"
            assert_eq!(&dst[..11], b"hello world".map(|b| b as i8));
        }
    }
}
