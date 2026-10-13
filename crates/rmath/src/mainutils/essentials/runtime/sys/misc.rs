use super::*;

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

/// GNU `utils::sessionInfo()` — enough fields for datetime2.R.
pub unsafe fn do_sessionInfo(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_mkString(c"internal".as_ptr()));
        crate::mainutils::essentials::set_string_names(result, &["tzcode_type".to_string()]);
        result
    }
}

/// GNU `.POSIXct(xx, tz = NULL, cl = c("POSIXct", "POSIXt"))`.
pub unsafe fn do_dot_POSIXct(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let xx = CAR(args);
        if xx.is_null() || xx == R_NilValue() {
            return R_NilValue();
        }
        let tz = CADR(args);
        let cl_arg = CADDR(args);
        let cl = if cl_arg.is_null()
            || cl_arg == R_NilValue()
            || cl_arg == crate::sexp::globals::R_MissingArg()
        {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            let _n = protect(names);
            SET_STRING_ELT(names, 0, Rf_mkChar(c"POSIXct".as_ptr()));
            SET_STRING_ELT(names, 1, Rf_mkChar(c"POSIXt".as_ptr()));
            names
        } else {
            cl_arg
        };
        let pair = Rf_cons(cl, R_NilValue());
        let _p = protect(pair);
        let call_args = Rf_cons(xx, pair);
        let _a = protect(call_args);
        let result = crate::mainutils::attrib::do_classgets(_call, _op, call_args, rho);
        if !tz.is_null() && tz != R_NilValue() && tz != crate::sexp::globals::R_MissingArg() {
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"tzone".as_ptr()), tz);
        }
        result
    }
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
        } else if file_type.is_file() {
            // GNU OlsonNames: list.files(recursive=TRUE) filtered to names
            // starting with an uppercase letter (drops lowercase auxiliary
            // files and VERSION; keeps GMT, EST5EDT, UTC, ...).
            let path = next_relative.to_string_lossy().replace('\\', "/");
            if path.starts_with(|c: char| c.is_ascii_uppercase()) && path != "VERSION" {
                names.insert(path);
            }
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

/// GNU `q(save="no")` / `quit` — end the script without tearing down the process.
pub unsafe fn do_quit(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::sexp::globals::set_R_Visible(0);
        R_NilValue()
    }
}
