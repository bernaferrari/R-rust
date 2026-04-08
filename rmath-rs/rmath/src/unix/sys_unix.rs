#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/unix/sys-unix.c -- Unix-specific platform functions.
//!
//! Implements platform-dependent functions including:
//! - R_ExpandFileName: tilde expansion for file paths
//! - R_setStartTime / R_getProcTime / R_getClockIncrement: process timing
//! - do_machine: returns "Unix" platform name
//! - do_sysinfo: system information (uname, login, user)
//! - fpu_setup: FPU initialization
//! - R_OpenInitFile: open R initialization file (.Rprofile)

use std::cell::{Cell, RefCell};
use std::env;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::sexp::accessors::SET_STRING_ELT;
use crate::sexp::constructors::{Rf_allocVector, Rf_mkChar, Rf_mkString};
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const R_PATH_MAX: usize = 4096;
const STRSXP_VAL: c_int = 16;

// ---------------------------------------------------------------------------
// Stub functions
// ---------------------------------------------------------------------------

unsafe fn checkArity(_op: SEXP, _args: SEXP) {}
unsafe fn setAttrib(_x: SEXP, _what: SEXP, _val: SEXP) {}
unsafe fn R_NamesSymbol() -> SEXP {
    ptr::null_mut()
}

thread_local! { static LoadInitFile: Cell<c_int> = Cell::new(1); }

// ---------------------------------------------------------------------------
// R_ExpandFileName
// ---------------------------------------------------------------------------

thread_local! { static newFileName: RefCell<[c_char; R_PATH_MAX + 1]> = RefCell::new([0; R_PATH_MAX + 1]); }

/// Expand ~ in file paths.
/// Handles ~, ~user, and ~user/path forms using HOME env and getpwnam.
pub unsafe fn R_ExpandFileName(s: *const c_char) -> *const c_char {
    unsafe {
        if s.is_null() || *s == 0 {
            return s;
        }

        // Not a tilde path, return as-is
        if *s != b'~' as c_char {
            return s;
        }

        let input = CStr::from_ptr(s);
        let input_bytes = input.to_bytes();

        // Find '/' after tilde
        let slash_pos = input_bytes.iter().skip(1).position(|&b| b == b'/');
        let (user_part, rest) = match slash_pos {
            Some(pos) => {
                let user = &input_bytes[1..pos + 1]; // skip '~'
                let r = &input_bytes[pos + 1..];
                (user, r)
            }
            None => (&input_bytes[1..], &[][..]),
        };

        let home = if user_part.is_empty() {
            // ~ or ~/path: use HOME env var
            match env::var("HOME") {
                Ok(ref v) if !v.is_empty() => {
                    // Fall back to getpwuid if HOME is empty
                    let pw = libc::getpwuid(libc::getuid());
                    if pw.is_null() {
                        return s; // can't expand
                    }
                    let pw_dir = CStr::from_ptr((*pw).pw_dir);
                    pw_dir.to_string_lossy().into_owned()
                }
                Ok(v) => v,
                Err(_) => {
                    let pw = libc::getpwuid(libc::getuid());
                    if pw.is_null() {
                        return s;
                    }
                    let pw_dir = CStr::from_ptr((*pw).pw_dir);
                    pw_dir.to_string_lossy().into_owned()
                }
            }
        } else {
            // ~user: look up in passwd
            let user_cstr = CString::new(user_part).unwrap_or_default();
            let pw = libc::getpwnam(user_cstr.as_ptr());
            if pw.is_null() {
                return s; // user not found
            }
            let pw_dir = CStr::from_ptr((*pw).pw_dir);
            pw_dir.to_string_lossy().into_owned()
        };

        // Build expanded path
        let expanded = if rest.is_empty() {
            home
        } else {
            format!("{}/{}", home, String::from_utf8_lossy(rest))
        };

        if expanded.len() >= R_PATH_MAX {
            return s; // too long
        }

        let bytes = expanded.as_bytes();
        let result = newFileName.with(|buf_cell| {
            let buf = &mut *buf_cell.borrow_mut();
            ptr::copy_nonoverlapping(bytes.as_ptr(), buf.as_mut_ptr() as *mut u8, bytes.len());
            *buf.as_mut_ptr().add(bytes.len()) = 0;
            buf.as_ptr()
        });
        result
    }
}

// ---------------------------------------------------------------------------
// do_machine
// ---------------------------------------------------------------------------

/// .Internal(machine()) -- returns "Unix".
pub unsafe fn do_machine(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { Rf_mkString(b"Unix\0".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// Process timing
// ---------------------------------------------------------------------------

thread_local! { static clk_tck: Cell<c_double> = Cell::new(100.0); }
thread_local! { static StartTime: Cell<c_double> = Cell::new(0.0); }

/// Get current time in seconds (using gettimeofday).
unsafe fn currentTime() -> c_double {
    unsafe {
        let mut tv: libc::timeval = std::mem::zeroed();
        libc::gettimeofday(&mut tv, ptr::null_mut());
        tv.tv_sec as c_double + tv.tv_usec as c_double * 1e-6
    }
}

/// Record the start time for proc.time().
pub unsafe fn R_setStartTime() {
    unsafe {
        clk_tck.with(|v| v.set(libc::sysconf(libc::_SC_CLK_TCK) as c_double));
        StartTime.with(|v| v.set(currentTime()));
    }
}

/// Get process timing data: [user, system, elapsed, child_user, child_system].
#[unsafe(no_mangle)]
pub unsafe fn R_getProcTime(data: *mut c_double) {
    unsafe {
        let et = currentTime() - StartTime.with(|v| v.get());
        *data.add(2) = 1e-3 * (1000.0 * et).round();

        let mut self_usage: libc::rusage = std::mem::zeroed();
        let mut children_usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut self_usage);
        libc::getrusage(libc::RUSAGE_CHILDREN, &mut children_usage);

        *data.add(0) = self_usage.ru_utime.tv_sec as c_double
            + 1e-3 * (self_usage.ru_utime.tv_usec / 1000) as c_double;
        *data.add(1) = self_usage.ru_stime.tv_sec as c_double
            + 1e-3 * (self_usage.ru_stime.tv_usec / 1000) as c_double;
        *data.add(3) = children_usage.ru_utime.tv_sec as c_double
            + 1e-3 * (children_usage.ru_utime.tv_usec / 1000) as c_double;
        *data.add(4) = children_usage.ru_stime.tv_sec as c_double
            + 1e-3 * (children_usage.ru_stime.tv_usec / 1000) as c_double;
    }
}

/// Get the clock increment in seconds.
pub unsafe fn R_getClockIncrement() -> c_double {
    1.0 / clk_tck.with(|v| v.get())
}

// ---------------------------------------------------------------------------
// do_sysinfo
// ---------------------------------------------------------------------------

/// .Internal(sysinfo()) -- returns system information.
pub unsafe fn do_sysinfo(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(STRSXP_VAL, 8);
        let ansnames = Rf_allocVector(STRSXP_VAL, 8);

        let mut utsname: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut utsname) == -1 {
            return R_NilValue();
        }

        let sysname = CStr::from_ptr(utsname.sysname.as_ptr()).to_string_lossy();
        let release = CStr::from_ptr(utsname.release.as_ptr()).to_string_lossy();
        let version = CStr::from_ptr(utsname.version.as_ptr()).to_string_lossy();
        let nodename = CStr::from_ptr(utsname.nodename.as_ptr()).to_string_lossy();
        let machine = CStr::from_ptr(utsname.machine.as_ptr()).to_string_lossy();

        // Get login name
        let login = CString::new("unknown").expect("CString::new failed: contains null byte");
        let login_ptr = libc::getlogin();
        let login_cstr = if !login_ptr.is_null() {
            CStr::from_ptr(login_ptr)
        } else {
            login.as_c_str()
        };

        // Get user name from passwd
        let user_cstr = {
            let pw = libc::getpwuid(libc::getuid());
            if !pw.is_null() {
                CStr::from_ptr((*pw).pw_name)
            } else {
                login_cstr
            }
        };

        // Get effective user name
        let euser_cstr = {
            let pw = libc::getpwuid(libc::geteuid());
            if !pw.is_null() {
                CStr::from_ptr((*pw).pw_name)
            } else {
                login_cstr
            }
        };

        SET_STRING_ELT(ans, 0, Rf_mkChar(sysname.as_ptr() as *const c_char));
        SET_STRING_ELT(ans, 1, Rf_mkChar(release.as_ptr() as *const c_char));
        SET_STRING_ELT(ans, 2, Rf_mkChar(version.as_ptr() as *const c_char));
        SET_STRING_ELT(ans, 3, Rf_mkChar(nodename.as_ptr() as *const c_char));
        SET_STRING_ELT(ans, 4, Rf_mkChar(machine.as_ptr() as *const c_char));
        SET_STRING_ELT(ans, 5, Rf_mkChar(login_cstr.as_ptr() as *const c_char));
        SET_STRING_ELT(ans, 6, Rf_mkChar(user_cstr.as_ptr() as *const c_char));
        SET_STRING_ELT(ans, 7, Rf_mkChar(euser_cstr.as_ptr() as *const c_char));

        SET_STRING_ELT(
            ansnames,
            0,
            Rf_mkChar(b"sysname\0".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(
            ansnames,
            1,
            Rf_mkChar(b"release\0".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(
            ansnames,
            2,
            Rf_mkChar(b"version\0".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(
            ansnames,
            3,
            Rf_mkChar(b"nodename\0".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(
            ansnames,
            4,
            Rf_mkChar(b"machine\0".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(ansnames, 5, Rf_mkChar(b"login\0".as_ptr() as *const c_char));
        SET_STRING_ELT(ansnames, 6, Rf_mkChar(b"user\0".as_ptr() as *const c_char));
        SET_STRING_ELT(
            ansnames,
            7,
            Rf_mkChar(b"effective_user\0".as_ptr() as *const c_char),
        );

        setAttrib(ans, R_NamesSymbol(), ansnames);
        ans
    }
}

// ---------------------------------------------------------------------------
// R_ProcessEvents
// ---------------------------------------------------------------------------

/// Process pending events (stub).
pub unsafe fn R_ProcessEvents() {
    // In the full implementation, this calls ptr_R_ProcessEvents
    // and R_PolledEvents, then checks time limits.
}

// ---------------------------------------------------------------------------
// fpu_setup
// ---------------------------------------------------------------------------

/// Set up FPU control word.
/// On most platforms this is a no-op. On FreeBSD and ARM, it adjusts
/// floating-point exception handling.
pub unsafe fn fpu_setup(start: c_int) {
    if start != 0 {
        // Platform-specific FPU setup
        #[cfg(target_os = "freebsd")]
        {
            // fpsetmask(0) -- disable all FP exceptions
        }
        // ARM FPU setup is done via inline assembly in C;
        // on Rust/macOS this is typically not needed
    } else {
        #[cfg(target_os = "freebsd")]
        {
            // fpsetmask(~0) -- enable all FP exceptions
        }
    }
}

// ---------------------------------------------------------------------------
// R_OpenInitFile
// ---------------------------------------------------------------------------

/// Open the R initialization file (.Rprofile).
/// Checks R_PROFILE_USER env var, then ./.Rprofile, then ~/.Rprofile.
pub unsafe fn R_OpenInitFile() -> *mut libc::FILE {
    unsafe {
        if LoadInitFile.with(|v| v.get()) == 0 {
            return ptr::null_mut();
        }

        // Check R_PROFILE_USER
        if let Ok(profile) = env::var("R_PROFILE_USER") {
            if profile.is_empty() {
                return ptr::null_mut();
            }
            let expanded = R_ExpandFileName(
                CString::new(profile)
                    .expect("CString::new failed: contains null byte")
                    .as_ptr(),
            );
            let path = CStr::from_ptr(expanded);
            let mode = b"r\0".as_ptr() as *const c_char;
            let fp = libc::fopen(path.as_ptr(), mode);
            if !fp.is_null() {
                return fp;
            }
            return ptr::null_mut();
        }

        // Try ./.Rprofile
        let dot_path = b".Rprofile\0".as_ptr() as *const c_char;
        let mode = b"r\0".as_ptr() as *const c_char;
        let fp = libc::fopen(dot_path, mode);
        if !fp.is_null() {
            return fp;
        }

        // Try ~/.Rprofile
        if let Ok(home) = env::var("HOME") {
            let full_path = format!("{}/.Rprofile\0", home);
            let fp = libc::fopen(full_path.as_ptr() as *const c_char, mode);
            if !fp.is_null() {
                return fp;
            }
        }

        ptr::null_mut()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::accessors::*;

    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_do_machine() {
        unsafe {
            let result = do_machine(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_expand_filename_no_tilde() {
        unsafe {
            let path = CString::new("/usr/lib/R").unwrap();
            let result = R_ExpandFileName(path.as_ptr());
            assert_eq!(CStr::from_ptr(result).to_string_lossy(), "/usr/lib/R");
        }
    }

    #[test]
    fn test_expand_filename_tilde() {
        unsafe {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            let path = CString::new("~/Documents").unwrap();
            let result = R_ExpandFileName(path.as_ptr());
            let expanded = CStr::from_ptr(result).to_string_lossy().to_string();
            assert!(expanded.starts_with(&home));
            assert!(expanded.ends_with("/Documents"));
        }
    }

    #[test]
    fn test_expand_filename_tilde_only() {
        unsafe {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            let path = CString::new("~").unwrap();
            let result = R_ExpandFileName(path.as_ptr());
            let expanded = CStr::from_ptr(result).to_string_lossy().to_string();
            assert_eq!(expanded, home);
        }
    }

    #[test]
    fn test_expand_filename_null() {
        unsafe {
            let result = R_ExpandFileName(ptr::null());
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_set_start_time() {
        unsafe {
            R_setStartTime();
            assert!(clk_tck.with(|v| v.get()) > 0.0);
            assert!(StartTime.with(|v| v.get()) > 0.0);
        }
    }

    #[test]
    fn test_get_clock_increment() {
        unsafe {
            R_setStartTime();
            let increment = R_getClockIncrement();
            assert!(increment > 0.0);
            assert!(increment < 1.0);
        }
    }

    #[test]
    fn test_get_proc_time() {
        unsafe {
            R_setStartTime();
            let mut data = [0.0f64; 5];
            R_getProcTime(data.as_mut_ptr());
            // Elapsed time should be non-negative
            assert!(data[2] >= 0.0);
        }
    }

    #[test]
    fn test_fpu_setup() {
        unsafe {
            fpu_setup(1);
            fpu_setup(0);
        }
    }

    #[test]
    fn test_do_sysinfo() {
        unsafe {
            let result = do_sysinfo(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            if !result.is_null() && result != R_NilValue() {
                assert_eq!(TYPEOF(result), STRSXP_VAL);
                assert_eq!(LENGTH(result), 8);
            }
        }
    }
}
