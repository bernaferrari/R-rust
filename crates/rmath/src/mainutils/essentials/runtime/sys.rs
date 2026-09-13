//! Sys.* functions, R.home, date/time coercion, timezone and locale.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Locale plumbing (libc setlocale on native; fixed values on wasm32)
// ---------------------------------------------------------------------------

/// Locale category codes forwarded to libc `setlocale(3)` on native targets.
/// The wasm32 sandbox has no locale subsystem, so the glibc numbering is
/// used there as a stable private encoding (the only consumer is the
/// setlocale stub below, which ignores it).
#[cfg(not(target_arch = "wasm32"))]
use libc::{LC_ALL, LC_COLLATE, LC_CTYPE, LC_MESSAGES, LC_MONETARY, LC_NUMERIC, LC_TIME};

#[cfg(target_arch = "wasm32")]
const LC_CTYPE: c_int = 0;
#[cfg(target_arch = "wasm32")]
const LC_NUMERIC: c_int = 1;
#[cfg(target_arch = "wasm32")]
const LC_TIME: c_int = 2;
#[cfg(target_arch = "wasm32")]
const LC_COLLATE: c_int = 3;
#[cfg(target_arch = "wasm32")]
const LC_MONETARY: c_int = 4;
#[cfg(target_arch = "wasm32")]
const LC_MESSAGES: c_int = 5;
#[cfg(target_arch = "wasm32")]
const LC_ALL: c_int = 6;

/// Query or set the process locale via libc `setlocale(3)`.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
unsafe fn r_setlocale(category: c_int, locale: *const c_char) -> *mut c_char {
    unsafe { libc::setlocale(category, locale) }
}

/// wasm32 stub: every query resolves to the "C" locale and setting any
/// locale leaves it unchanged ("C" is the effective locale either way).
#[cfg(target_arch = "wasm32")]
#[inline]
unsafe fn r_setlocale(_category: c_int, _locale: *const c_char) -> *mut c_char {
    b"C\0".as_ptr() as *mut c_char
}

// ---------------------------------------------------------------------------
// Complete R runtime — Sys.* functions, R.home
// ---------------------------------------------------------------------------

/// R's `R.home()` — R home directory (simplified).
pub unsafe fn do_R_home(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
        let s = CString::new(home).unwrap_or_default();
        Rf_mkString(s.as_ptr())
    }
}

/// R's `Sys.getenv(x)` — get environment variable.
pub unsafe fn do_Sys_getenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            // No names: return the whole environment as "NAME=VALUE" strings,
            // read live from libc's environ so Sys.setenv results are visible.
            let mut vars: Vec<String> = Vec::new();
            unsafe {
                let mut envp: *mut *mut c_char = environ;
                while !(*envp).is_null() {
                    let entry = CStr::from_ptr(*envp).to_string_lossy();
                    vars.push(entry.into_owned());
                    envp = envp.add(1);
                }
            }
            let n = vars.len() as R_xlen_t;
            let ans = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _ans_guard = protect(ans);
            for (i, var) in vars.iter().enumerate() {
                let c_str = CString::new(var.as_str()).unwrap_or_default();
                SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(c_str.as_ptr()));
            }
            return ans;
        }
        let unset_arg = arg_by_name_or_position(args, &["unset"], 1);
        let unset = if !unset_arg.is_null()
            && unset_arg != R_NilValue()
            && TYPEOF(unset_arg) == SEXPTYPE::STRSXP
            && XLENGTH(unset_arg) > 0
            && STRING_ELT(unset_arg, 0) == crate::sexp::globals::R_NaString()
        {
            None
        } else if !unset_arg.is_null() && unset_arg != R_NilValue() && XLENGTH(unset_arg) > 0 {
            Some(elt_to_string(unset_arg, 0))
        } else {
            Some(String::new())
        };

        let values = (0..XLENGTH(x))
            .map(|i| {
                let name = elt_to_string(x, i);
                libc_getenv(&name).or_else(|| unset.clone())
            })
            .collect::<Vec<_>>();
        let result = optional_string_vector(&values);
        if XLENGTH(x) > 1 {
            // Stock names the result vector when looking up more than one name.
            let n = XLENGTH(x);
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _names_guard = protect(names);
            for i in 0..n {
                SET_STRING_ELT(names, i, STRING_ELT(x, i));
            }
            let names_sym = Rf_install(c"names".as_ptr());
            crate::sexp::attrib_core::setAttrib(result, names_sym, names);
        }
        result
    }
}

/// Read a variable live via libc getenv (sees Sys.setenv writes).
fn libc_getenv(name: &str) -> Option<String> {
    // std::env is the live process environment on native hosts (upstream
    // Sys.setenv semantics: system() children see the writes) and a
    // permanently-empty environment on wasm — no libc.
    if name.contains('\0') {
        return None;
    }
    std::env::var(name).ok()
}

/// Set a variable live via std::env::set_var (overwrites); false on invalid
/// input. Empty names are rejected like libc setenv.
fn libc_setenv(name: &str, value: &str) -> bool {
    if name.is_empty() || name.contains('=') || name.contains('\0') || value.contains('\0') {
        return false;
    }
    // SAFETY: single-threaded engine session; no concurrent reader of the
    // process environment exists in-process.
    unsafe { std::env::set_var(name, value) };
    true
}

fn environment_mutation_allowed() -> bool {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).eval_state.capabilities.allow_environment_mutation
    })
}

unsafe fn denied_setenv_result(args: SEXP) -> SEXP {
    unsafe {
        let mut n = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            n += 1;
            current = CDR(current);
        }
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        for i in 0..n {
            *LOGICAL(result).add(i as usize) = FALSE;
        }
        result
    }
}

/// Unset a variable live via std::env::remove_var; false on invalid input.
fn libc_unsetenv(name: &str) -> bool {
    if name.is_empty() || name.contains('=') || name.contains('\0') {
        return false;
    }
    // SAFETY: see libc_setenv.
    unsafe { std::env::remove_var(name) };
    true
}

#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" {
    static mut environ: *mut *mut c_char;
}

/// wasm32 has no process environment: a null environ makes Sys.getenv's
/// whole-environment listing return empty, matching the facade's getenv.
#[cfg(target_arch = "wasm32")]
static mut environ: *mut *mut c_char = std::ptr::null_mut();

/// R's `Sys.setenv(...)` — set environment variables.
pub unsafe fn do_Sys_setenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if !environment_mutation_allowed() {
            return denied_setenv_result(args);
        }
        let mut results: Vec<c_int> = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            let ok = if !arg.is_null() && arg != R_NilValue() {
                if let Some(key) = tag_name(current)
                    && !key.is_empty()
                {
                    libc_setenv(&key, &elt_to_string(arg, 0))
                } else {
                    // Unnamed "NAME=value" argument; '=' in NAME fails like stock.
                    let s = elt_to_string(arg, 0);
                    match s.find('=') {
                        Some(pos) => libc_setenv(&s[..pos], &s[pos + 1..]),
                        None => false,
                    }
                }
            } else {
                false
            };
            results.push(if ok { TRUE } else { FALSE });
            current = CDR(current);
        }
        let n = results.len() as R_xlen_t;
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _ans_guard = protect(ans);
        for (i, ok) in results.iter().enumerate() {
            *LOGICAL(ans).add(i) = *ok;
        }
        ans
    }
}

/// R's `Sys.unsetenv(x)` — unset environment variables.
pub unsafe fn do_Sys_unsetenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if !environment_mutation_allowed() {
            let x = arg_by_name_or_position(args, &["x"], 0);
            if x.is_null() || x == R_NilValue() {
                return Rf_ScalarLogical(FALSE);
            }
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, XLENGTH(x));
            for i in 0..XLENGTH(x) {
                *LOGICAL(result).add(i as usize) = FALSE;
            }
            return result;
        }
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _ans_guard = protect(ans);
        for i in 0..n {
            let name = elt_to_string(x, i);
            let ok = if name.is_empty() {
                false
            } else {
                libc_unsetenv(&name)
            };
            *LOGICAL(ans).add(i as usize) = if ok { TRUE } else { FALSE };
        }
        ans
    }
}

/// R's `Sys.which(names)` — resolve command names against PATH.
pub unsafe fn do_Sys_which(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names_arg = arg_by_name_or_position(args, &["names"], 0);
        if names_arg.is_null() || names_arg == R_NilValue() || names_arg == R_MissingArg() {
            base_error("argument \"names\" is missing, with no default");
        }

        let names = coerce_string_values(names_arg);
        let paths = names
            .iter()
            .map(|name| find_executable_on_path(name).unwrap_or_default())
            .collect::<Vec<_>>();
        named_string_vector(&paths, &names)
    }
}

fn find_executable_on_path(command: &str) -> Option<String> {
    if command.is_empty() || command == "NA" {
        return None;
    }
    if command.contains(std::path::MAIN_SEPARATOR)
        || command.contains('/')
        || command.contains('\\')
    {
        return executable_path_if_runnable(Path::new(command));
    }

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(command);
        if let Some(found) = executable_path_if_runnable(&candidate) {
            return Some(found);
        }

        #[cfg(windows)]
        {
            if Path::new(command).extension().is_none() {
                for ext in windows_path_extensions() {
                    let candidate = dir.join(format!("{command}{ext}"));
                    if let Some(found) = executable_path_if_runnable(&candidate) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

fn executable_path_if_runnable(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }

    Some(path.to_string_lossy().into_owned())
}

#[cfg(windows)]
fn windows_path_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|ext| !ext.is_empty())
        .map(|ext| ext.to_string())
        .collect()
}

/// R's `Sys.info()` — named character vector with host/user information.
pub unsafe fn do_Sys_info(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let host = sys_info_host_fields();
        let user = sys_info_user();
        let values = vec![
            host.sysname,
            host.release,
            host.version,
            host.nodename,
            host.machine,
            user.clone(),
            user.clone(),
            user,
        ];
        let names = vec![
            "sysname".to_string(),
            "release".to_string(),
            "version".to_string(),
            "nodename".to_string(),
            "machine".to_string(),
            "login".to_string(),
            "user".to_string(),
            "effective_user".to_string(),
        ];
        let result = string_vector(&values);
        let _result_guard = protect(result);
        let name_vec = string_vector(&names);
        let _name_guard = protect(name_vec);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            name_vec,
        );
        result
    }
}

struct SysInfoHostFields {
    sysname: String,
    release: String,
    version: String,
    nodename: String,
    machine: String,
}

fn sys_info_host_fields() -> SysInfoHostFields {
    #[cfg(unix)]
    {
        unsafe {
            let mut utsname = std::mem::MaybeUninit::<libc::utsname>::zeroed();
            if libc::uname(utsname.as_mut_ptr()) == 0 {
                let utsname = utsname.assume_init();
                return SysInfoHostFields {
                    sysname: CStr::from_ptr(utsname.sysname.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                    release: CStr::from_ptr(utsname.release.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                    version: CStr::from_ptr(utsname.version.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                    nodename: CStr::from_ptr(utsname.nodename.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                    machine: CStr::from_ptr(utsname.machine.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                };
            }
        }
    }

    SysInfoHostFields {
        sysname: std::env::consts::OS.to_string(),
        release: String::new(),
        version: String::new(),
        nodename: std::env::var("HOSTNAME").unwrap_or_default(),
        machine: std::env::consts::ARCH.to_string(),
    }
}

fn sys_info_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// R's `Sys.time()` — current time as REALSXP (seconds since epoch).
pub unsafe fn do_Sys_time(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs() as f64 + dur.subsec_nanos() as f64 / 1e9;
        let result = Rf_ScalarReal(secs);
        // Set class to c("POSIXct", "POSIXt").
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !class.is_null() {
            let _p2 = protect(class);
            SET_STRING_ELT(class, 0, Rf_mkChar(c"POSIXct".as_ptr()));
            SET_STRING_ELT(class, 1, Rf_mkChar(c"POSIXt".as_ptr()));
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class);
        }
        result
    }
}

/// R's `Sys.sleep(time)` — sleep for specified seconds.
pub unsafe fn do_Sys_sleep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let time_arg = CAR(args);
        let secs = real_or_default(time_arg, 0.0);
        if secs > 0.0 {
            let dur = std::time::Duration::from_secs_f64(secs);
            std::thread::sleep(dur);
        }
        R_NilValue()
    }
}

pub(crate) unsafe fn set_single_class(x: SEXP, class_name: &str) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class.is_null() {
            return;
        }
        let _guard = protect(class);
        let cstr = CString::new(class_name).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            SET_STRING_ELT(class, 0, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_ClassSymbol(), class);
    }
}

pub(crate) unsafe fn set_posixct_class(x: SEXP, tz: &str) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !class.is_null() {
            let _guard = protect(class);
            SET_STRING_ELT(class, 0, Rf_mkChar(c"POSIXct".as_ptr()));
            SET_STRING_ELT(class, 1, Rf_mkChar(c"POSIXt".as_ptr()));
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }

        let tz_cstr = CString::new(tz).unwrap_or_default();
        let tzone = Rf_mkString(tz_cstr.as_ptr());
        if !tzone.is_null() {
            crate::sexp::attrib_core::setAttrib(x, Rf_install(c"tzone".as_ptr()), tzone);
        }
    }
}

/// R's `as.Date(x, origin)` — coerce ISO date strings or day counts to Date.
pub unsafe fn do_as_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if sexp_has_class(x, "Date") && TYPEOF(x) == SEXPTYPE::REALSXP {
            return x;
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        let out = REAL(result);

        if TYPEOF(x) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let value = STRING_ELT(x, i);
                let days = if value == crate::sexp::globals::R_NaString() {
                    NA_REAL
                } else {
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    parse_iso_date_days(text).unwrap_or_else(|| {
                        base_error("character string is not in a standard unambiguous format")
                    })
                };
                *out.add(i as usize) = days;
            }
        } else if sexp_has_class(x, "POSIXct") && TYPEOF(x) == SEXPTYPE::REALSXP {
            for i in 0..n {
                let seconds = *REAL(x).add(i as usize);
                *out.add(i as usize) = if seconds.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_REAL
                } else {
                    (seconds / 86_400.0).floor()
                };
            }
        } else if sexp_has_class(x, "POSIXlt") && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 6 {
            let sec = VECTOR_ELT(x, 0);
            let nlt = if TYPEOF(sec) == SEXPTYPE::REALSXP || TYPEOF(sec) == SEXPTYPE::INTSXP {
                XLENGTH(sec)
            } else {
                1
            };
            let result_lt = Rf_allocVector3(SEXPTYPE::REALSXP, nlt);
            let _lt = protect(result_lt);
            let mday = VECTOR_ELT(x, 3);
            let mon = VECTOR_ELT(x, 4);
            let year = VECTOR_ELT(x, 5);
            for i in 0..nlt {
                let y = if TYPEOF(year) == SEXPTYPE::INTSXP {
                    *INTEGER(year).add(i as usize) + 1900
                } else {
                    1970
                };
                let m = if TYPEOF(mon) == SEXPTYPE::INTSXP {
                    *INTEGER(mon).add(i as usize) + 1
                } else {
                    1
                };
                let d = if TYPEOF(mday) == SEXPTYPE::INTSXP {
                    *INTEGER(mday).add(i as usize)
                } else {
                    1
                };
                *REAL(result_lt).add(i as usize) =
                    parse_iso_date_days(&format!("{y:04}-{m:02}-{d:02}")).unwrap_or(NA_REAL);
            }
            set_single_class(result_lt, "Date");
            return result_lt;
        } else if TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP {
            let origin = arg_by_name_or_position(args, &["origin"], 1);
            if origin.is_null() || origin == R_NilValue() {
                base_error("'origin' must be supplied");
            }
            let origin_days = parse_iso_date_days(&elt_to_string(origin, 0))
                .unwrap_or_else(|| base_error("'origin' must be a character string"));
            for i in 0..n {
                let days = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    let v = *REAL(x).add(i as usize);
                    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_REAL
                    } else {
                        origin_days + v.floor()
                    }
                } else {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER {
                        NA_REAL
                    } else {
                        origin_days + f64::from(v)
                    }
                };
                *out.add(i as usize) = days;
            }
        } else {
            base_error("do not know how to convert 'x' to class \"Date\"");
        }

        set_single_class(result, "Date");
        result
    }
}

/// GNU `julian.Date(x, origin=as.Date("1970-01-01"))`.
pub unsafe fn do_julian(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let mut origin_days = 0.0;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let origin = CAR(rest);
            if !origin.is_null()
                && origin != R_NilValue()
                && TYPEOF(origin) == SEXPTYPE::REALSXP
                && XLENGTH(origin) > 0
            {
                origin_days = *REAL(origin);
            }
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER {
                    NA_REAL
                } else {
                    iv as f64
                }
            } else {
                NA_REAL
            };
            *REAL(result).add(i as usize) = v - origin_days;
        }
        let origin = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _o = protect(origin);
        *REAL(origin) = origin_days;
        set_single_class(origin, "Date");
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"origin".as_ptr()),
            origin,
        );
        result
    }
}


fn difftime_seconds(x: SEXP) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return f64::NAN;
        }
        let v = if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) > 0 {
            *REAL(x)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) > 0 {
            let iv = *INTEGER(x);
            if iv == NA_INTEGER {
                return f64::NAN;
            }
            iv as f64
        } else {
            return f64::NAN;
        };
        if crate::mainutils::objects::inherits2(x, c"Date".as_ptr()) != 0 {
            v * 86_400.0
        } else {
            v
        }
    }
}

/// GNU `difftime(time1, time2, units="auto")`.
pub unsafe fn do_difftime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let time1 = CAR(args);
        let time2 = CAR(CDR(args));
        let mut units = "auto".to_string();
        let mut cell = CDR(CDR(args));
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if named == "units"
                && TYPEOF(value) == SEXPTYPE::STRSXP
                && XLENGTH(value) > 0
            {
                let ch = STRING_ELT(value, 0);
                if !ch.is_null() {
                    units = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
            cell = CDR(cell);
        }
        let z = difftime_seconds(time1) - difftime_seconds(time2);
        if units == "auto" {
            let zz = z.abs();
            units = if !zz.is_finite() || zz < 60.0 {
                "secs".to_string()
            } else if zz < 3600.0 {
                "mins".to_string()
            } else if zz < 86400.0 {
                "hours".to_string()
            } else {
                "days".to_string()
            };
        }
        let scaled = match units.as_str() {
            "mins" => z / 60.0,
            "hours" => z / 3600.0,
            "days" => z / 86_400.0,
            "weeks" => z / (7.0 * 86_400.0),
            _ => z,
        };
        let result = Rf_ScalarReal(scaled);
        let _r = protect(result);
        set_single_class(result, "difftime");
        let u = Rf_mkString(
            std::ffi::CString::new(units.as_str())
                .unwrap_or_default()
                .as_ptr(),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"units".as_ptr()),
            u,
        );
        result
    }
}

/// GNU `as.difftime(tim, units)`.
pub unsafe fn do_as_difftime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let tim = CAR(args);
        if crate::mainutils::objects::inherits2(tim, c"difftime".as_ptr()) != 0 {
            return tim;
        }
        let mut units = String::new();
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if named == "units"
                && TYPEOF(value) == SEXPTYPE::STRSXP
                && XLENGTH(value) > 0
            {
                let ch = STRING_ELT(value, 0);
                if !ch.is_null() {
                    units = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
            cell = CDR(cell);
        }
        if units.is_empty() || units == "auto" {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need explicit units for numeric conversion",
            );
        }
        let n = XLENGTH(tim);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let v = if TYPEOF(tim) == SEXPTYPE::REALSXP {
                *REAL(tim).add(i as usize)
            } else if TYPEOF(tim) == SEXPTYPE::INTSXP {
                let iv = *INTEGER(tim).add(i as usize);
                if iv == NA_INTEGER {
                    NA_REAL
                } else {
                    iv as f64
                }
            } else {
                NA_REAL
            };
            *REAL(result).add(i as usize) = v;
        }
        set_single_class(result, "difftime");
        let u = Rf_mkString(CString::new(units.as_str()).unwrap_or_default().as_ptr());
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"units".as_ptr()),
            u,
        );
        result
    }
}

/// GNU `units(x)` is attr(x, "units").
pub unsafe fn do_units(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::sexp::attrib_core::getAttrib(
            CAR(args),
            crate::sexp::symbol::Rf_install(c"units".as_ptr()),
        )
    }
}


fn iso_arg_num(x: SEXP, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) > 0 {
            *REAL(x)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) > 0 {
            let v = *INTEGER(x);
            if v == NA_INTEGER {
                default
            } else {
                v as f64
            }
        } else {
            default
        }
    }
}

unsafe fn iso_posixct(year: f64, month: f64, day: f64, hour: f64, min: f64, sec: f64, tz: &str) -> SEXP {
    unsafe {
        let stamp = format!(
            "{:04}-{:02}-{:02}",
            year as i32,
            month as i32,
            day as i32
        );
        let days = crate::mainutils::essentials::parse_iso_date_days(&stamp).unwrap_or(f64::NAN);
        let seconds = days * 86_400.0 + hour * 3600.0 + min * 60.0 + sec;
        let result = Rf_ScalarReal(seconds);
        let _r = protect(result);
        set_posixct_class(result, tz);
        result
    }
}

/// GNU `ISOdatetime(year, month, day, hour, min, sec, tz)`.
pub unsafe fn do_ISOdatetime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let year = iso_arg_num(CAR(args), 1970.0);
        let month = iso_arg_num(CAR(CDR(args)), 1.0);
        let day = iso_arg_num(CAR(CDR(CDR(args))), 1.0);
        let hour = iso_arg_num(CAR(CDR(CDR(CDR(args)))), 0.0);
        let min = iso_arg_num(CAR(CDR(CDR(CDR(CDR(args))))), 0.0);
        let sec = iso_arg_num(CAR(CDR(CDR(CDR(CDR(CDR(args)))))), 0.0);
        let mut tz = String::new();
        let mut cell = CDR(CDR(CDR(CDR(CDR(CDR(args))))));
        if !cell.is_null() && cell != R_NilValue() {
            let t = CAR(cell);
            if TYPEOF(t) == SEXPTYPE::STRSXP && XLENGTH(t) > 0 {
                let ch = STRING_ELT(t, 0);
                if !ch.is_null() {
                    tz = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        iso_posixct(year, month, day, hour, min, sec, &tz)
    }
}

/// GNU `ISOdate(year, month, day)` is noon GMT.
pub unsafe fn do_ISOdate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let year = iso_arg_num(CAR(args), 1970.0);
        let month = iso_arg_num(CAR(CDR(args)), 1.0);
        let day = iso_arg_num(CAR(CDR(CDR(args))), 1.0);
        let mut hour = 12.0;
        let mut min = 0.0;
        let mut sec = 0.0;
        let mut tz = "GMT".to_string();
        let mut cell = CDR(CDR(CDR(args)));
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if named == "hour" || (named.is_empty() && pos == 0) {
                hour = iso_arg_num(value, 12.0);
            } else if named == "min" || (named.is_empty() && pos == 1) {
                min = iso_arg_num(value, 0.0);
            } else if named == "sec" || (named.is_empty() && pos == 2) {
                sec = iso_arg_num(value, 0.0);
            } else if named == "tz" || (named.is_empty() && pos == 3) {
                if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) > 0 {
                    let ch = STRING_ELT(value, 0);
                    if !ch.is_null() {
                        tz = std::ffi::CStr::from_ptr(CHAR(ch))
                            .to_string_lossy()
                            .into_owned();
                    }
                }
            }
            if named.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        iso_posixct(year, month, day, hour, min, sec, &tz)
    }
}

fn unix_secs_to_utc(secs: i64) -> crate::tzone_strftime::stm {
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let yday = if m > 2 {
        doy - 59
    } else {
        doy + 306
    };
    crate::tzone_strftime::stm {
        tm_sec: (sod % 60) as i32,
        tm_min: ((sod / 60) % 60) as i32,
        tm_hour: (sod / 3600) as i32,
        tm_mday: d as i32,
        tm_mon: (m as i32) - 1,
        tm_year: y as i32 - 1900,
        tm_wday: ((days + 4).rem_euclid(7)) as i32,
        tm_yday: yday as i32,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: b"GMT\0".as_ptr() as *const std::os::raw::c_char,
    }
}

/// GNU `strftime(x, format)`.
pub unsafe fn do_strftime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let mut fmt = "%Y-%m-%d %H:%M:%S".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let f = CAR(rest);
            if TYPEOF(f) == SEXPTYPE::STRSXP && XLENGTH(f) > 0 {
                let ch = STRING_ELT(f, 0);
                if !ch.is_null() {
                    let s = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                    if !s.is_empty() {
                        fmt = s;
                    }
                }
            }
        }
        let n = XLENGTH(x);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let secs = if crate::mainutils::objects::inherits2(x, c"Date".as_ptr()) != 0 {
                let days = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                    *INTEGER(x).add(i as usize) as f64
                } else {
                    f64::NAN
                };
                days * 86_400.0
            } else if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add(i as usize) as f64
            } else {
                f64::NAN
            };
            let formatted = if secs.is_finite() {
                let tm = unix_secs_to_utc(secs as i64);
                crate::tzone_strftime::strftime_safe(&fmt, &tm)
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let c = CString::new(formatted).unwrap_or_default();
            SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

/// GNU `format.Date(x, format="%Y-%m-%d")`.
pub unsafe fn do_format_Date(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut fmt = "%Y-%m-%d".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let f = CAR(rest);
            if TYPEOF(f) == SEXPTYPE::STRSXP && XLENGTH(f) > 0 {
                let ch = STRING_ELT(f, 0);
                if !ch.is_null() {
                    let s = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                    if !s.is_empty() {
                        fmt = s;
                    }
                }
            }
        }
        let fmt_s = Rf_mkString(CString::new(fmt.as_str()).unwrap_or_default().as_ptr());
        let _f = protect(fmt_s);
        do_strftime(call, op, Rf_cons(x, Rf_cons(fmt_s, R_NilValue())), rho)
    }
}

/// GNU `as.character.Date(x)`.
pub unsafe fn do_as_character_Date(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe { do_format_Date(call, op, args, rho) }
}

fn date_level_string(days: f64) -> String {
    let tm = unix_secs_to_utc((days * 86_400.0) as i64);
    crate::tzone_strftime::strftime_safe("%Y-%m-%d", &tm).unwrap_or_default()
}

/// GNU `cut.Date(x, breaks)` for week/month/year.
pub unsafe fn do_cut_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut days: Vec<f64> = Vec::with_capacity(n as usize);
        for i in 0..n {
            days.push(date_days_elt(x, i));
        }
        let mut units = "days".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let b = CAR(rest);
            if TYPEOF(b) == SEXPTYPE::STRSXP && XLENGTH(b) > 0 {
                let ch = STRING_ELT(b, 0);
                if !ch.is_null() {
                    units = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        let finite: Vec<f64> = days.iter().copied().filter(|d| d.is_finite()).collect();
        if finite.is_empty() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let min_d = finite.iter().copied().fold(f64::INFINITY, f64::min);
        let max_d = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut breaks: Vec<f64> = Vec::new();
        if units.starts_with("week") {
            let w = ((min_d as i64 + 4).rem_euclid(7)) as i32;
            let off = if w == 0 { 6 } else { w - 1 };
            let mut b = min_d as i64 - off as i64;
            while (b as f64) <= max_d {
                breaks.push(b as f64);
                b += 7;
            }
            breaks.push(b as f64);
        } else if units.starts_with("month") {
            let mut b = date_first_of_month(min_d);
            while b <= max_d {
                breaks.push(b);
                b = date_add_months(b, 1);
            }
            breaks.push(date_add_months(b, 0).max(date_add_months(breaks.last().copied().unwrap_or(b), 1)));
            if *breaks.last().unwrap() <= max_d {
                breaks.push(date_add_months(*breaks.last().unwrap(), 1));
            }
        } else if units.starts_with("year") {
            let mut b = date_first_of_year(min_d);
            while b <= max_d {
                breaks.push(b);
                b = date_first_of_year(b + 370.0);
            }
            breaks.push(date_first_of_year(b + 370.0));
        } else if units.starts_with("quarter") {
            let mut b = date_first_of_quarter(min_d);
            while b <= max_d {
                breaks.push(b);
                b = date_add_months(b, 3);
            }
            if breaks.is_empty() {
                breaks.push(b);
            }
            let last = *breaks.last().unwrap();
            if last <= max_d {
                breaks.push(date_add_months(last, 3));
            } else if breaks.len() < 2 {
                breaks.push(date_add_months(last, 3));
            }
        } else if units.starts_with("day") {
            let mut b = min_d.floor();
            while b <= max_d {
                breaks.push(b);
                b += 1.0;
            }
            breaks.push(b);
        } else {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid specification of 'breaks'",
            );
        }
        if breaks.len() < 2 {
            breaks.push(max_d + 1.0);
        }
        let nlev = breaks.len() - 1;
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        let _r = protect(result);
        for i in 0..n as usize {
            let d = days[i];
            let code = if !d.is_finite() {
                NA_INTEGER
            } else {
                let mut c = NA_INTEGER;
                for k in 0..nlev {
                    if d >= breaks[k] && d < breaks[k + 1] {
                        c = (k as i32) + 1;
                        break;
                    }
                }
                c
            };
            *INTEGER(result).add(i) = code;
        }
        let levels_vec = Rf_allocVector3(SEXPTYPE::STRSXP, nlev as i64);
        let _l = protect(levels_vec);
        for k in 0..nlev {
            let s = date_level_string(breaks[k]);
            let c = CString::new(s).unwrap_or_default();
            SET_STRING_ELT(levels_vec, k as i64, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            levels_vec,
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"factor".as_ptr()),
        );
        result
    }
}



fn date_units_arg(args: SEXP) -> String {
    unsafe {
        let mut units = "days".to_string();
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if (named == "units" || named.is_empty())
                && TYPEOF(value) == SEXPTYPE::STRSXP
                && XLENGTH(value) > 0
            {
                let ch = STRING_ELT(value, 0);
                if !ch.is_null() {
                    units = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
                break;
            }
            cell = CDR(cell);
        }
        units
    }
}

fn date_days_elt(x: SEXP, i: i64) -> f64 {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER {
                NA_REAL
            } else {
                v as f64
            }
        } else {
            NA_REAL
        }
    }
}

fn date_first_of_month(days: f64) -> f64 {
    let tm = unix_secs_to_utc((days * 86_400.0) as i64);
    let y = tm.tm_year + 1900;
    let m = tm.tm_mon + 1;
    crate::mainutils::essentials::parse_iso_date_days(&format!("{y:04}-{m:02}-01"))
        .unwrap_or(days)
}

fn date_first_of_quarter(days: f64) -> f64 {
    let tm = unix_secs_to_utc((days * 86_400.0) as i64);
    let y = tm.tm_year + 1900;
    let qmon = (tm.tm_mon / 3) * 3 + 1;
    crate::mainutils::essentials::parse_iso_date_days(&format!("{y:04}-{qmon:02}-01"))
        .unwrap_or(days)
}


fn date_first_of_year(days: f64) -> f64 {
    let tm = unix_secs_to_utc((days * 86_400.0) as i64);
    let y = tm.tm_year + 1900;
    crate::mainutils::essentials::parse_iso_date_days(&format!("{y:04}-01-01")).unwrap_or(days)
}

fn date_add_months(days: f64, add: i32) -> f64 {
    let tm = unix_secs_to_utc((days * 86_400.0) as i64);
    let mut y = tm.tm_year + 1900;
    let mut m = tm.tm_mon + 1 + add;
    while m > 12 {
        m -= 12;
        y += 1;
    }
    while m < 1 {
        m += 12;
        y -= 1;
    }
    crate::mainutils::essentials::parse_iso_date_days(&format!("{y:04}-{m:02}-01"))
        .unwrap_or(days)
}

/// GNU `trunc.Date(x, units)`.
pub unsafe fn do_trunc_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let units = date_units_arg(args);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let days = date_days_elt(x, i);
            let out = if !days.is_finite() {
                days
            } else if units.starts_with("month") {
                date_first_of_month(days)
            } else if units.starts_with("year") {
                date_first_of_year(days)
            } else {
                days.floor()
            };
            *REAL(result).add(i as usize) = out;
        }
        set_single_class(result, "Date");
        result
    }
}

/// GNU `round.Date(x, units)`.
pub unsafe fn do_round_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let units = date_units_arg(args);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let days = date_days_elt(x, i);
            let out = if !days.is_finite() {
                days
            } else if units.starts_with("month") {
                let lo = date_first_of_month(days);
                let hi = date_add_months(lo, 1);
                if (hi - days) <= (days - lo) {
                    hi
                } else {
                    lo
                }
            } else if units.starts_with("year") {
                let lo = date_first_of_year(days);
                let hi = date_first_of_year(lo + 370.0);
                if (hi - days) <= (days - lo) {
                    hi
                } else {
                    lo
                }
            } else {
                days.round()
            };
            *REAL(result).add(i as usize) = out;
        }
        set_single_class(result, "Date");
        result
    }
}





/// R's `as.POSIXct(x, tz, origin)` — coerce simple UTC inputs to POSIXct.
pub unsafe fn do_as_POSIXct(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if sexp_has_class(x, "POSIXct") && TYPEOF(x) == SEXPTYPE::REALSXP {
            return x;
        }
        if sexp_has_class(x, "POSIXlt") && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 6 {
            let sec = VECTOR_ELT(x, 0);
            let minv = VECTOR_ELT(x, 1);
            let hour = VECTOR_ELT(x, 2);
            let mday = VECTOR_ELT(x, 3);
            let mon = VECTOR_ELT(x, 4);
            let year = VECTOR_ELT(x, 5);
            let nlt = if TYPEOF(sec) == SEXPTYPE::REALSXP || TYPEOF(sec) == SEXPTYPE::INTSXP {
                XLENGTH(sec)
            } else {
                1
            };
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, nlt);
            let _r = protect(result);
            for i in 0..nlt {
                let y = if TYPEOF(year) == SEXPTYPE::INTSXP {
                    *INTEGER(year).add(i as usize) + 1900
                } else {
                    1970
                };
                let m = if TYPEOF(mon) == SEXPTYPE::INTSXP {
                    *INTEGER(mon).add(i as usize) + 1
                } else {
                    1
                };
                let d = if TYPEOF(mday) == SEXPTYPE::INTSXP {
                    *INTEGER(mday).add(i as usize)
                } else {
                    1
                };
                let h = if TYPEOF(hour) == SEXPTYPE::INTSXP {
                    *INTEGER(hour).add(i as usize) as f64
                } else {
                    0.0
                };
                let mi = if TYPEOF(minv) == SEXPTYPE::INTSXP {
                    *INTEGER(minv).add(i as usize) as f64
                } else {
                    0.0
                };
                let s = if TYPEOF(sec) == SEXPTYPE::REALSXP {
                    *REAL(sec).add(i as usize)
                } else if TYPEOF(sec) == SEXPTYPE::INTSXP {
                    *INTEGER(sec).add(i as usize) as f64
                } else {
                    0.0
                };
                let days = parse_iso_date_days(&format!("{y:04}-{m:02}-{d:02}")).unwrap_or(0.0);
                *REAL(result).add(i as usize) = days * 86_400.0 + h * 3600.0 + mi * 60.0 + s;
            }
            let tz_arg = arg_by_name_or_position(args, &["tz"], 1);
            let tz = if tz_arg.is_null() || tz_arg == R_NilValue() || XLENGTH(tz_arg) == 0 {
                "UTC".to_string()
            } else {
                let value = elt_to_string(tz_arg, 0);
                if value.is_empty() {
                    "UTC".to_string()
                } else {
                    value
                }
            };
            set_posixct_class(result, &tz);
            return result;
        }


        let tz_arg = arg_by_name_or_position(args, &["tz"], 1);
        let tz = if tz_arg.is_null() || tz_arg == R_NilValue() || XLENGTH(tz_arg) == 0 {
            "UTC".to_string()
        } else {
            let value = elt_to_string(tz_arg, 0);
            if value.is_empty() {
                "UTC".to_string()
            } else {
                value
            }
        };

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        let out = REAL(result);

        if TYPEOF(x) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let value = STRING_ELT(x, i);
                let seconds = if value == crate::sexp::globals::R_NaString() {
                    NA_REAL
                } else {
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    parse_iso_datetime_seconds(text).unwrap_or_else(|| {
                        base_error("character string is not in a standard unambiguous format")
                    })
                };
                *out.add(i as usize) = seconds;
            }
        } else if sexp_has_class(x, "Date") && TYPEOF(x) == SEXPTYPE::REALSXP {
            for i in 0..n {
                let days = *REAL(x).add(i as usize);
                *out.add(i as usize) = if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_REAL
                } else {
                    days.floor() * 86_400.0
                };
            }
        } else if TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP {
            let origin = arg_by_name_or_position(args, &["origin"], 2);
            let origin_seconds = if origin.is_null() || origin == R_NilValue() {
                0.0
            } else {
                parse_iso_datetime_seconds(&elt_to_string(origin, 0))
                    .or_else(|| {
                        parse_iso_date_days(&elt_to_string(origin, 0)).map(|days| days * 86_400.0)
                    })
                    .unwrap_or_else(|| base_error("'origin' must be a character string"))
            };
            for i in 0..n {
                let seconds = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    let v = *REAL(x).add(i as usize);
                    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_REAL
                    } else {
                        origin_seconds + v
                    }
                } else {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER {
                        NA_REAL
                    } else {
                        origin_seconds + f64::from(v)
                    }
                };
                *out.add(i as usize) = seconds;
            }
        } else {
            base_error("do not know how to convert 'x' to class \"POSIXct\"");
        }

        set_posixct_class(result, &tz);
        result
    }
}

/// GNU `mean.Date(x)`.
pub unsafe fn do_mean_Date(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let m = crate::eval::arithmetic::do_mean(call, op, args, rho);
        let _m = protect(m);
        set_single_class(m, "Date");
        m
    }
}

/// GNU `mean.POSIXct(x)`.
pub unsafe fn do_mean_POSIXct(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let tz = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tzone".as_ptr()),
        );
        let mut tz_s = "UTC".to_string();
        if !tz.is_null() && tz != R_NilValue() && TYPEOF(tz) == SEXPTYPE::STRSXP && XLENGTH(tz) > 0 {
            let ch = STRING_ELT(tz, 0);
            if !ch.is_null() {
                tz_s = std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned();
            }
        }
        let m = crate::eval::arithmetic::do_mean(call, op, args, rho);
        let _m = protect(m);
        set_posixct_class(m, &tz_s);
        m
    }
}

/// GNU `mean.POSIXlt(x)`.
pub unsafe fn do_mean_POSIXlt(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let ct = do_as_POSIXct(call, op, Rf_cons(x, R_NilValue()), rho);
        let _ct = protect(ct);
        let m = do_mean_POSIXct(call, op, Rf_cons(ct, R_NilValue()), rho);
        let _m = protect(m);
        crate::mainutils::datetime::do_as_POSIXlt(call, op, Rf_cons(m, R_NilValue()), rho)
    }
}


/// GNU `diff.POSIXt(x)`.
pub unsafe fn do_diff_POSIXt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let x = if crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0 {
            do_as_POSIXct(
                _call,
                _op,
                Rf_cons(x, R_NilValue()),
                _rho,
            )
        } else {
            x
        };
        let _x = protect(x);
        let n = XLENGTH(x);
        if n < 2 {
            let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            let _e = protect(empty);
            set_single_class(empty, "difftime");
            crate::sexp::attrib_core::setAttrib(
                empty,
                crate::sexp::symbol::Rf_install(c"units".as_ptr()),
                Rf_mkString(c"secs".as_ptr()),
            );
            return empty;
        }
        let mut z = Vec::with_capacity((n - 1) as usize);
        for i in 0..(n - 1) {
            let a = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                0.0
            };
            let b = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add((i + 1) as usize)
            } else {
                0.0
            };
            z.push(b - a);
        }
        let zz = z
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .map(|v| v.abs())
            .fold(f64::INFINITY, f64::min);
        let units = if !zz.is_finite() || zz < 60.0 {
            "secs"
        } else if zz < 3600.0 {
            "mins"
        } else if zz < 86400.0 {
            "hours"
        } else {
            "days"
        };
        let scale = match units {
            "mins" => 60.0,
            "hours" => 3600.0,
            "days" => 86_400.0,
            _ => 1.0,
        };
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, z.len() as i64);
        let _r = protect(result);
        for (i, v) in z.iter().enumerate() {
            *REAL(result).add(i) = *v / scale;
        }
        set_single_class(result, "difftime");
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"units".as_ptr()),
            Rf_mkString(CString::new(units).unwrap_or_default().as_ptr()),
        );
        result
    }
}

/// GNU `trunc.POSIXt(x, units)`.
pub unsafe fn do_trunc_POSIXt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let x = if crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0 {
            do_as_POSIXct(_call, _op, Rf_cons(x, R_NilValue()), _rho)
        } else {
            x
        };
        let _x = protect(x);
        let mut units = "secs".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let u = CAR(rest);
            if TYPEOF(u) == SEXPTYPE::STRSXP && XLENGTH(u) > 0 {
                let ch = STRING_ELT(u, 0);
                if !ch.is_null() {
                    units = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let secs = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                0.0
            };
            let out = if !secs.is_finite() {
                secs
            } else if units.starts_with("min") {
                (secs / 60.0).floor() * 60.0
            } else if units.starts_with("hour") {
                (secs / 3600.0).floor() * 3600.0
            } else if units.starts_with("day") {
                (secs / 86_400.0).floor() * 86_400.0
            } else if units.starts_with("month") {
                date_first_of_month((secs / 86_400.0).floor()) * 86_400.0
            } else if units.starts_with("year") {
                date_first_of_year((secs / 86_400.0).floor()) * 86_400.0
            } else {
                secs.floor()
            };
            *REAL(result).add(i as usize) = out;
        }
        set_posixct_class(result, "GMT");
        result
    }
}

/// GNU `round.POSIXt(x, units)`.
pub unsafe fn do_round_POSIXt(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let mut units = "secs".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let u = CAR(rest);
            if TYPEOF(u) == SEXPTYPE::STRSXP && XLENGTH(u) > 0 {
                let ch = STRING_ELT(u, 0);
                if !ch.is_null() {
                    units = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        let x = if crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0 {
            do_as_POSIXct(call, op, Rf_cons(x, R_NilValue()), rho)
        } else {
            x
        };
        let _x = protect(x);
        let half = if units.starts_with("min") {
            30.0
        } else if units.starts_with("hour") {
            1800.0
        } else if units.starts_with("day") {
            43200.0
        } else if units.starts_with("month") || units.starts_with("year") {
            0.0
        } else {
            0.5
        };
        let n = XLENGTH(x);
        let shifted = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _s = protect(shifted);
        for i in 0..n {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                0.0
            };
            *REAL(shifted).add(i as usize) = v + half;
        }
        set_posixct_class(shifted, "GMT");
        let u = Rf_mkString(CString::new(units.as_str()).unwrap_or_default().as_ptr());
        let _u = protect(u);
        do_trunc_POSIXt(call, op, Rf_cons(shifted, Rf_cons(u, R_NilValue())), rho)
    }
}

/// GNU `c.Date(...)`.
pub unsafe fn do_c_Date(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let r = crate::mainutils::bind::do_c_dflt(call, op, args, rho);
        let _r = protect(r);
        set_single_class(r, "Date");
        r
    }
}

/// GNU `c.POSIXct(...)`.
pub unsafe fn do_c_POSIXct(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let tz = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tzone".as_ptr()),
        );
        let mut tz_s = String::new();
        if !tz.is_null() && tz != R_NilValue() && TYPEOF(tz) == SEXPTYPE::STRSXP && XLENGTH(tz) > 0 {
            let ch = STRING_ELT(tz, 0);
            if !ch.is_null() {
                tz_s = std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned();
            }
        }
        let r = crate::mainutils::bind::do_c_dflt(call, op, args, rho);
        let _r = protect(r);
        if tz_s.is_empty() {
            set_posixct_class(r, "UTC");
        } else {
            set_posixct_class(r, &tz_s);
        }
        r
    }
}

/// GNU `c.POSIXlt(...)`.
pub unsafe fn do_c_POSIXlt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut converted = R_NilValue();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = crate::eval::eval::Rf_eval(CAR(cell), rho);
            let _v = protect(v);
            let tag = TAG(cell);
            let skip = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    == "recursive"
            } else {
                false
            };
            if !skip {
                let ct = if sexp_has_class(v, "POSIXct") && TYPEOF(v) == SEXPTYPE::REALSXP {
                    v
                } else {
                    do_as_POSIXct(call, op, Rf_cons(v, R_NilValue()), rho)
                };
                let _ct_one = protect(ct);
                let node = Rf_cons(ct, converted);
                SETTAG(node, tag);
                converted = node;
                let _converted = protect(converted);
            }
            cell = CDR(cell);
        }
        let mut rev = R_NilValue();
        let mut c = converted;
        while !c.is_null() && c != R_NilValue() {
            let node = Rf_cons(CAR(c), rev);
            SETTAG(node, TAG(c));
            rev = node;
            c = CDR(c);
        }
        let _a = protect(rev);
        let ct = do_c_POSIXct(call, op, rev, rho);
        let _ct = protect(ct);
        crate::mainutils::datetime::do_as_POSIXlt(call, op, Rf_cons(ct, R_NilValue()), rho)
    }
}


/// GNU `is.numeric.Date` / `is.numeric.POSIXt`.
pub unsafe fn do_is_numeric_Date(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _rho: SEXP,
) -> SEXP {
    unsafe { Rf_ScalarLogical(FALSE) }
}

/// GNU `diff.Date(x)`.
pub unsafe fn do_diff_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            let _e = protect(empty);
            set_single_class(empty, "difftime");
            crate::sexp::attrib_core::setAttrib(
                empty,
                crate::sexp::symbol::Rf_install(c"units".as_ptr()),
                Rf_mkString(c"days".as_ptr()),
            );
            return empty;
        }
        let n = XLENGTH(x);
        if n < 2 {
            let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            let _e = protect(empty);
            set_single_class(empty, "difftime");
            crate::sexp::attrib_core::setAttrib(
                empty,
                crate::sexp::symbol::Rf_install(c"units".as_ptr()),
                Rf_mkString(c"days".as_ptr()),
            );
            return empty;
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n - 1);
        let _r = protect(result);
        for i in 0..(n - 1) {
            let a = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add(i as usize) as f64
            } else {
                0.0
            };
            let b = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add((i + 1) as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add((i + 1) as usize) as f64
            } else {
                0.0
            };
            *REAL(result).add(i as usize) = b - a;
        }
        set_single_class(result, "difftime");
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"units".as_ptr()),
            Rf_mkString(c"days".as_ptr()),
        );
        result
    }
}









/// R's `Sys.Date()` — current date as REALSXP (days since epoch).
pub unsafe fn do_Sys_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let days = (dur.as_secs() / 86400) as f64;
        let result = Rf_ScalarReal(days);
        set_single_class(result, "Date");
        result
    }
}

/// R's `Sys.timezone()` — current timezone (simplified).
pub unsafe fn do_Sys_timezone(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let tz = system_timezone_name();
        let s = CString::new(tz).unwrap_or_default();
        Rf_mkString(s.as_ptr())
    }
}

fn system_timezone_name() -> String {
    std::env::var("TZ")
        .ok()
        .and_then(|tz| {
            let tz = tz.trim_start_matches(':').to_string();
            (!tz.is_empty()).then_some(tz)
        })
        .or_else(|| {
            std::fs::read_link("/etc/localtime")
                .ok()
                .and_then(|path| timezone_name_from_zoneinfo_path(&path))
        })
        .unwrap_or_else(|| "UTC".to_string())
}

pub(crate) fn timezone_name_from_zoneinfo_path(path: &Path) -> Option<String> {
    let path = path.to_string_lossy();
    for prefix in [
        "/var/db/timezone/zoneinfo/",
        "/usr/share/zoneinfo/",
        "/usr/share/lib/zoneinfo/",
    ] {
        if let Some(zone) = path.strip_prefix(prefix) {
            if !zone.is_empty() {
                return Some(zone.to_string());
            }
        }
    }
    None
}

/// R's `OlsonNames()` — known IANA timezone names from the system zoneinfo DB.
pub unsafe fn do_OlsonNames(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let zones = olson_names();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, zones.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, zone) in zones.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(zone.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

fn olson_names() -> Vec<String> {
    let mut names = BTreeSet::new();
    for root in ["/var/db/timezone/zoneinfo", "/usr/share/zoneinfo"] {
        collect_olson_names(Path::new(root), Path::new(""), &mut names);
    }
    names.into_iter().collect()
}

fn collect_olson_names(root: &Path, relative: &Path, names: &mut BTreeSet<String>) {
    let current = root.join(relative);
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if skip_olson_component(&file_name) {
            continue;
        }

        let next_relative = relative.join(file_name.as_ref());
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_olson_names(root, &next_relative, names);
        } else if file_type.is_file() && next_relative.components().count() > 1 {
            names.insert(next_relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

pub(crate) fn skip_olson_component(name: &str) -> bool {
    let metadata_extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "tab" | "list" | "zi"));
    name.starts_with('.') || matches!(name, "posix" | "right" | "SystemV") || metadata_extension
}

/// R's `Sys.localeconv()` — locale formatting conventions.
pub unsafe fn do_Sys_localeconv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = [
            "decimal_point",
            "thousands_sep",
            "grouping",
            "int_curr_symbol",
            "currency_symbol",
            "mon_decimal_point",
            "mon_thousands_sep",
            "mon_grouping",
            "positive_sign",
            "negative_sign",
            "int_frac_digits",
            "frac_digits",
            "p_cs_precedes",
            "p_sep_by_space",
            "n_cs_precedes",
            "n_sep_by_space",
            "p_sign_posn",
            "n_sign_posn",
        ];
        let values = [
            ".", "", "", "", "", ".", "", "", "", "", "127", "127", "127", "127", "127", "127",
            "127", "127",
        ];
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let name_vec = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        let _names_guard = protect(name_vec);
        for (i, (name, value)) in names.iter().zip(values.iter()).enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*value).unwrap_or_default().as_ptr()),
            );
            SET_STRING_ELT(
                name_vec,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            name_vec,
        );
        result
    }
}

/// R's `Sys.getlocale(category)` — get locale (simplified).
pub unsafe fn do_Sys_getlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let category = locale_category_from_arg(CAR(args));
        locale_string_from_libc(category)
    }
}

/// R's `Sys.setlocale(category, locale)` — set locale (simplified).
pub unsafe fn do_Sys_setlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let allowed = crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).eval_state.capabilities.allow_environment_mutation
        });
        if !allowed {
            return Rf_mkString(c"".as_ptr());
        }

        let category = locale_category_from_arg(CAR(args));
        let locale_arg = CAR(CDR(args));
        let locale = locale_string_arg(locale_arg);
        let locale_ptr = match locale.as_ref() {
            Some(locale) => locale.as_ptr(),
            None => std::ptr::null(),
        };
        let result = r_setlocale(category, locale_ptr);
        if result.is_null() {
            Rf_mkString(b"\0".as_ptr() as *const c_char)
        } else {
            Rf_mkString(result)
        }
    }
}

unsafe fn locale_category_from_arg(category: SEXP) -> c_int {
    unsafe {
        if category.is_null() || category == R_NilValue() {
            return LC_ALL;
        }

        match TYPEOF(category) {
            t if t == SEXPTYPE::STRSXP => {
                let name = elt_to_string(category, 0);
                locale_category_from_name(&name)
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => match *INTEGER(category) {
                1 => LC_ALL,
                2 => LC_COLLATE,
                3 => LC_CTYPE,
                4 => LC_MONETARY,
                5 => LC_NUMERIC,
                6 => LC_TIME,
                7 => LC_MESSAGES,
                _ => base_error("invalid 'category' argument"),
            },
            _ => base_error("invalid 'category' argument"),
        }
    }
}

fn locale_category_from_name(name: &str) -> c_int {
    match name {
        "LC_ALL" => LC_ALL,
        "LC_COLLATE" => LC_COLLATE,
        "LC_CTYPE" => LC_CTYPE,
        "LC_MONETARY" => LC_MONETARY,
        "LC_NUMERIC" => LC_NUMERIC,
        "LC_TIME" => LC_TIME,
        "LC_MESSAGES" => LC_MESSAGES,
        _ => base_error("invalid 'category' argument"),
    }
}

unsafe fn locale_string_arg(locale: SEXP) -> Option<CString> {
    unsafe {
        if locale.is_null() || locale == R_NilValue() {
            return None;
        }
        if TYPEOF(locale) != SEXPTYPE::STRSXP || XLENGTH(locale) == 0 {
            base_error("invalid 'locale' argument");
        }
        CString::new(elt_to_string(locale, 0))
            .map(Some)
            .unwrap_or_else(|_| base_error("invalid 'locale' argument"))
    }
}

unsafe fn locale_string_from_libc(category: c_int) -> SEXP {
    unsafe {
        let result = r_setlocale(category, std::ptr::null());
        if result.is_null() {
            Rf_mkString(b"\0".as_ptr() as *const c_char)
        } else {
            Rf_mkString(result)
        }
    }
}
