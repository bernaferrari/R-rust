//! Package system builtins — `.libPaths`, library, require, installed.packages,
//! find.package, packageVersion, packageDescription, namespace load/attach, data.

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
// Complete package system — library, require, installed.packages, find.package
// ---------------------------------------------------------------------------

/// R's `.libPaths()` — inspect or replace the session's library search path.
pub unsafe fn do_lib_paths(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if !args.is_null() && args != R_NilValue() {
            let value = CAR(args);
            if !value.is_null() && value != R_NilValue() && TYPEOF(value) == SEXPTYPE::STRSXP {
                let mut paths = Vec::with_capacity(LENGTH(value).max(0) as usize);
                for i in 0..LENGTH(value) {
                    let path = CStr::from_ptr(CHAR(STRING_ELT(value, i as R_xlen_t)))
                        .to_string_lossy()
                        .into_owned();
                    paths.push(PathBuf::from(path));
                }
                crate::sexp::instance::with_required_current_instance(|inst| {
                    inst.path_policy.set_library_paths(paths);
                });
            }
        }

        let paths = crate::sexp::instance::with_required_current_instance(|inst| {
            inst.path_policy
                .library_paths()
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        });
        string_vector(&paths)
    }
}

/// R's `library.dynam()` — native package loading is outside the pure-R Android runtime.
pub unsafe fn do_library_dynam(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    package_error(
        "library.dynam() loads native extension code, which is disabled in this pure-R Android runtime; use Rust-ported internals or a host-owned native-library policy",
    )
}

/// R's `library(package, ...)` — load a package.
pub unsafe fn do_library(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pkg_arg = CAR(args);
        if pkg_arg.is_null() || pkg_arg == R_NilValue() {
            package_error("no package specified");
        }
        let package_name = elt_to_string(pkg_arg, 0);
        if package_name.is_empty() || package_name == "NA" {
            package_error("invalid package name");
        }
        let lib_path = find_package_path(&package_name);
        if lib_path.is_empty() {
            package_error(format!("there is no package called '{}'", package_name));
        }
        if package_attached(&package_name) {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return R_NilValue();
        }
        match load_pure_r_package(&package_name, Path::new(&lib_path)) {
            Ok(()) => {
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(message) => package_error(message),
        }
    }
}

/// R's `require(package, ...)` — check if a package can be loaded.
pub unsafe fn do_require(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pkg_arg = CAR(args);
        if pkg_arg.is_null() || pkg_arg == R_NilValue() {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return Rf_ScalarLogical(FALSE);
        }
        let package_name = elt_to_string(pkg_arg, 0);
        let lib_path = find_package_path(&package_name);
        if lib_path.is_empty() {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return Rf_ScalarLogical(FALSE);
        }
        if package_attached(&package_name) {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return Rf_ScalarLogical(TRUE);
        }
        match load_pure_r_package(&package_name, Path::new(&lib_path)) {
            Ok(()) => {
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                Rf_ScalarLogical(TRUE)
            }
            Err(_) => {
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                Rf_ScalarLogical(FALSE)
            }
        }
    }
}

/// R's `installed.packages(...)` — list installed packages.
pub unsafe fn do_installed_packages(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let packages = installed_package_rows();
        installed_packages_matrix(&packages)
    }
}

/// R's `find.package(package, ...)` — find the path to a package.
pub unsafe fn do_find_package(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pkg_arg = CAR(args);
        if pkg_arg.is_null() || pkg_arg == R_NilValue() {
            return R_NilValue();
        }
        let package_name = elt_to_string(pkg_arg, 0);
        let path = find_package_path(&package_name);
        if path.is_empty() {
            return R_NilValue();
        }
        Rf_mkString(CString::new(path).unwrap_or_default().as_ptr())
    }
}

/// R's `packageVersion(pkg)` — read a package version from DESCRIPTION.
pub unsafe fn do_package_version(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["pkg", "package"], 0);
        let package = elt_to_string(package_arg, 0);
        match package_description_fields(&package) {
            Ok(fields) => match fields.get("Version") {
                Some(version) => string_vector(std::slice::from_ref(version)),
                None => package_error(format!("package '{}' has no Version field", package)),
            },
            Err(message) => package_error(message),
        }
    }
}

/// R's `packageDescription(pkg, fields = NULL)` — read DESCRIPTION metadata.
pub unsafe fn do_package_description(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["pkg", "package"], 0);
        let fields_arg = arg_by_name_or_position(args, &["fields"], 1);
        let package = elt_to_string(package_arg, 0);
        let fields = match package_description_fields(&package) {
            Ok(fields) => fields,
            Err(message) => package_error(message),
        };

        if !fields_arg.is_null() && fields_arg != R_NilValue() && XLENGTH(fields_arg) > 0 {
            let selected = (0..XLENGTH(fields_arg))
                .map(|i| {
                    let name = elt_to_string(fields_arg, i);
                    fields.get(&name).cloned()
                })
                .collect::<Vec<_>>();
            return optional_string_vector(&selected);
        }

        named_string_list(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        )
    }
}

/// R's `loadNamespace(package)` — load a package namespace without attaching it.
pub unsafe fn do_load_namespace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["package", "name"], 0);
        let package = elt_to_string(package_arg, 0);
        match load_package_namespace_by_name(&package) {
            Ok(env) => env,
            Err(message) => package_error(message),
        }
    }
}

/// R's `requireNamespace(package, quietly = FALSE)` — namespace availability probe.
pub unsafe fn do_require_namespace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["package", "quietly"], 0);
        let package = elt_to_string(package_arg, 0);
        Rf_ScalarLogical(if load_package_namespace_by_name(&package).is_ok() {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `getNamespace(name)` — return a loaded namespace, loading on demand.
pub unsafe fn do_get_namespace(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_load_namespace(_call, _op, args, rho) }
}

/// R's `asNamespace(ns)` — coerce a package name or environment to a namespace.
pub unsafe fn do_as_namespace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let ns = CAR(args);
        if !ns.is_null() && TYPEOF(ns) == SEXPTYPE::ENVSXP {
            return ns;
        }
        let package = elt_to_string(ns, 0);
        match load_package_namespace_by_name(&package) {
            Ok(env) => env,
            Err(message) => package_error(message),
        }
    }
}

/// R's `loadedNamespaces()` — list namespaces loaded in this session.
pub unsafe fn do_loaded_namespaces(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut names = crate::sexp::instance::with_required_current_instance(|inst| {
            inst.package_namespace_cache
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        });
        names.sort();
        string_vector(&names)
    }
}

/// R's `data(..., package, envir)` — load package data.
///
/// The Android runtime intentionally supports source-form package data
/// (`data/*.R`) and rejects serialized/lazy databases with an explicit error.
pub unsafe fn do_data(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let topic_arg = arg_by_name_or_position(args, &["list"], 0);
        let package_arg = arg_by_name_or_position(args, &["package"], 1);
        let envir_arg = arg_by_name_or_position(args, &["envir"], 2);
        let target_env = if !envir_arg.is_null() && TYPEOF(envir_arg) == SEXPTYPE::ENVSXP {
            envir_arg
        } else {
            rho
        };

        let packages = package_arg_values(package_arg);
        if topic_arg.is_null() || topic_arg == R_NilValue() || XLENGTH(topic_arg) == 0 {
            let names = list_package_data_sets(&packages);
            return string_vector(&names);
        }

        let mut loaded = Vec::<String>::new();
        for i in 0..XLENGTH(topic_arg) {
            let topic = elt_to_string(topic_arg, i);
            if topic.is_empty() || topic == "NA" {
                continue;
            }
            match load_package_data_set(&topic, &packages, target_env) {
                Ok(true) => push_unique(&mut loaded, topic),
                Ok(false) => package_error(format!("data set '{}' not found", topic)),
                Err(message) => package_error(message),
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        string_vector(&loaded)
    }
}
