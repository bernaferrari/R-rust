use super::*;

/// R's `R.home()` — R home directory (simplified).
pub unsafe fn do_R_home(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let home = std::env::var("R_HOME").unwrap_or_else(|_| discover_installed_r_home());
        let s = CString::new(home).unwrap_or_default();
        Rf_mkString(s.as_ptr())
    }
}

fn discover_installed_r_home() -> String {
    for bin in ["/opt/homebrew/bin/R", "/usr/local/bin/R", "R"] {
        if let Ok(out) = std::process::Command::new(bin).arg("RHOME").output()
            && let Ok(text) = String::from_utf8(out.stdout)
        {
            let home = text.trim();
            if home_has_rd_macros(home) {
                return home.to_string();
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir("/opt/homebrew/Cellar/r") {
        let mut homes: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|entry| entry.path().join("lib/R"))
            .filter(|home| home_has_rd_macros(&home.to_string_lossy()))
            .collect();
        homes.sort();
        if let Some(home) = homes.last() {
            return home.to_string_lossy().to_string();
        }
    }
    for home in ["/usr/local/lib/R", "/usr/lib/R"] {
        if home_has_rd_macros(home) {
            return home.to_string();
        }
    }
    "/usr/lib/R".to_string()
}

fn home_has_rd_macros(home: &str) -> bool {
    !home.is_empty()
        && std::path::Path::new(home)
            .join("share/Rd/macros/system.Rd")
            .is_file()
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
            if ok {
                let is_tz = (|| {
                    if let Some(key) = tag_name(current)
                        && !key.is_empty()
                    {
                        return key == "TZ";
                    }
                    let s = elt_to_string(arg, 0);
                    match s.find('=') {
                        Some(pos) => &s[..pos] == "TZ",
                        None => false,
                    }
                })();
                if is_tz {
                    // GNU tzcode: a changed TZ invalidates the cached zone.
                    crate::tzone::set_timezone_override(None);
                }
            }
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
