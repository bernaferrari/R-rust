use super::*;

/// R's `Sys.Date()` — current date as REALSXP (days since epoch).
pub fn do_Sys_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let now = crate::mainutils::times::currentTime();
    let seconds = now.floor() as i64;
    let mut local = crate::tzone::stm::default();
    let days = if !now.is_finite() || crate::tzone::R_localtime_r(&seconds, &mut local).is_null() {
        crate::sexp::ffi::NA_REAL
    } else {
        (seconds + local.tm_gmtoff).div_euclid(86_400) as f64
    };
    let result = Rf_ScalarReal(days);
    let _result = protect(result);
    set_single_class(result, "Date");
    result
}

/// R's `Sys.timezone()` — current timezone (simplified).
pub fn do_Sys_timezone(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let tz = system_timezone_name();
    let s = CString::new(tz).unwrap_or_default();
    mk_string_cstr(&s)
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
pub fn do_sessionInfo(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
    let _r = protect(result);
    SET_VECTOR_ELT(result, 0, mk_string_cstr(c"internal"));
    crate::mainutils::essentials::set_string_names(result, &["tzcode_type".to_string()]);
    result
}

/// GNU `.POSIXct(xx, tz = NULL, cl = c("POSIXct", "POSIXt"))`.
pub fn do_dot_POSIXct(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
            SET_STRING_ELT(names, 0, mk_char_cstr(c"POSIXct"));
            SET_STRING_ELT(names, 1, mk_char_cstr(c"POSIXt"));
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
            crate::sexp::attrib_core::setAttrib(result, install_str("tzone"), tz);
        }
        result
    }
}

/// R's `OlsonNames()` — known IANA timezone names on the current host.
pub fn do_OlsonNames(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
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

#[cfg(target_arch = "wasm32")]
fn olson_names() -> Vec<String> {
    crate::tzone::bundled_zone_names()
        .map(str::to_owned)
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn olson_names() -> Vec<String> {
    let mut names = BTreeSet::new();
    for root in ["/var/db/timezone/zoneinfo", "/usr/share/zoneinfo"] {
        collect_olson_names(Path::new(root), Path::new(""), &mut names);
    }
    names.into_iter().collect()
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn skip_olson_component(name: &str) -> bool {
    let metadata_extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "tab" | "list" | "zi"));
    name.starts_with('.') || matches!(name, "posix" | "right" | "SystemV") || metadata_extension
}

/// GNU `q(save="no")` / `quit` — end the script without tearing down the process.
pub fn do_quit(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    {
        crate::sexp::globals::set_R_Visible(0);
        R_NilValue()
    }
}
