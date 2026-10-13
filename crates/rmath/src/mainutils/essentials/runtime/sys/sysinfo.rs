use super::*;

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
