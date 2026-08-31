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
    let c_name = CString::new(name).ok()?;
    unsafe {
        let val = libc::getenv(c_name.as_ptr());
        if val.is_null() {
            None
        } else {
            Some(CStr::from_ptr(val).to_string_lossy().into_owned())
        }
    }
}

/// Set a variable live via libc setenv (overwrites); false on invalid input.
fn libc_setenv(name: &str, value: &str) -> bool {
    let Ok(c_name) = CString::new(name) else {
        return false;
    };
    let Ok(c_value) = CString::new(value) else {
        return false;
    };
    unsafe { libc::setenv(c_name.as_ptr(), c_value.as_ptr(), 1) == 0 }
}

/// Unset a variable live via libc unsetenv; false on invalid input.
fn libc_unsetenv(name: &str) -> bool {
    let Ok(c_name) = CString::new(name) else {
        return false;
    };
    unsafe { libc::unsetenv(c_name.as_ptr()) == 0 }
}

unsafe extern "C" {
    static mut environ: *mut *mut c_char;
}

/// R's `Sys.setenv(...)` — set environment variables.
pub unsafe fn do_Sys_setenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
        let category = locale_category_from_arg(CAR(args));
        let locale_arg = CAR(CDR(args));
        let locale = locale_string_arg(locale_arg);
        let locale_ptr = match locale.as_ref() {
            Some(locale) => locale.as_ptr(),
            None => std::ptr::null(),
        };
        let result = libc::setlocale(category, locale_ptr);
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
            return libc::LC_ALL;
        }

        match TYPEOF(category) {
            t if t == SEXPTYPE::STRSXP => {
                let name = elt_to_string(category, 0);
                locale_category_from_name(&name)
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => match *INTEGER(category) {
                1 => libc::LC_ALL,
                2 => libc::LC_COLLATE,
                3 => libc::LC_CTYPE,
                4 => libc::LC_MONETARY,
                5 => libc::LC_NUMERIC,
                6 => libc::LC_TIME,
                7 => libc::LC_MESSAGES,
                _ => base_error("invalid 'category' argument"),
            },
            _ => base_error("invalid 'category' argument"),
        }
    }
}

fn locale_category_from_name(name: &str) -> c_int {
    match name {
        "LC_ALL" => libc::LC_ALL,
        "LC_COLLATE" => libc::LC_COLLATE,
        "LC_CTYPE" => libc::LC_CTYPE,
        "LC_MONETARY" => libc::LC_MONETARY,
        "LC_NUMERIC" => libc::LC_NUMERIC,
        "LC_TIME" => libc::LC_TIME,
        "LC_MESSAGES" => libc::LC_MESSAGES,
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
        let result = libc::setlocale(category, std::ptr::null());
        if result.is_null() {
            Rf_mkString(b"\0".as_ptr() as *const c_char)
        } else {
            Rf_mkString(result)
        }
    }
}
