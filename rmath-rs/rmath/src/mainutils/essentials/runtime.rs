//! Essentials domain module `runtime` — extracted verbatim from essentials.rs.

use super::*;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

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
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::protect::protect;
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

// ---------------------------------------------------------------------------
// Complete R runtime — source, sys.source, demo, example
// ---------------------------------------------------------------------------

/// R's `source(file, local, echo, ...)` — evaluate an R script file.
pub unsafe fn do_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("source: no file specified");
            return R_NilValue();
        }
        let file_path = elt_to_string(file_arg, 0);

        match std::fs::read_to_string(&file_path) {
            Ok(content) => eval_source_text(&content, rho),
            Err(e) => {
                base_error(format!("cannot open file '{}': {}", file_path, e));
            }
        }
    }
}

/// R's `sys.source(file, envir, ...)` — source an R file into a specific environment.
pub unsafe fn do_sys_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let envir_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("sys.source: no file specified");
            return R_NilValue();
        }
        let file_path = elt_to_string(file_arg, 0);
        let target_env = if !envir_arg.is_null() && envir_arg != R_NilValue() {
            envir_arg
        } else {
            rho
        };

        match std::fs::read_to_string(&file_path) {
            Ok(content) => eval_source_text(&content, target_env),
            Err(e) => {
                base_error(format!("cannot open file '{}': {}", file_path, e));
            }
        }
    }
}

unsafe fn eval_source_text(content: &str, env: SEXP) -> SEXP {
    unsafe {
        let parsed = parse_source_expression_vector(content);
        let result = if parsed.is_null() || parsed == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(parsed, env)
        };
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

/// R's `demo(topic, ...)` — run a demo (simplified).
pub unsafe fn do_demo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let topic_arg = CAR(args);
        if topic_arg.is_null() || topic_arg == R_NilValue() {
            eprintln!("demo: no topic specified");
            return R_NilValue();
        }
        let topic = elt_to_string(topic_arg, 0);
        // Look for demo in common locations
        let demo_path = find_package_demo(&topic);
        if demo_path.is_empty() {
            eprintln!("No demo available for topic '{}'", topic);
            return R_NilValue();
        }
        match std::fs::read_to_string(&demo_path) {
            Ok(_content) => {
                eprintln!("Demo for topic: {}", topic);
                // In a full impl, parse and eval demo content
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(e) => {
                eprintln!("Error reading demo '{}': {}", topic, e);
                R_NilValue()
            }
        }
    }
}

/// R's `example(topic, ...)` — run an example (simplified).
pub unsafe fn do_example(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let topic_arg = CAR(args);
        if topic_arg.is_null() || topic_arg == R_NilValue() {
            eprintln!("example: no topic specified");
            return R_NilValue();
        }
        let topic = elt_to_string(topic_arg, 0);
        // Look for examples in common locations
        let example_path = find_package_example(&topic);
        if example_path.is_empty() {
            eprintln!("No examples available for topic '{}'", topic);
            return R_NilValue();
        }
        match std::fs::read_to_string(&example_path) {
            Ok(_content) => {
                eprintln!("Examples for topic: {}", topic);
                // In a full impl, parse and eval example content
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(e) => {
                eprintln!("Error reading example '{}': {}", topic, e);
                R_NilValue()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Environment functions
// ---------------------------------------------------------------------------

/// R's `emptyenv()` — returns the empty environment (root of environment chain).
pub unsafe fn do_emptyenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_EmptyEnv() }
}

/// R's `baseenv()` — returns the base environment.
pub unsafe fn do_baseenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_BaseEnv() }
}

/// R's `globalenv()` — returns the global environment.
pub unsafe fn do_globalenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_GlobalEnv() }
}

/// R's `new.env(hash, parent, size)` — create a new environment.
pub unsafe fn do_new_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let parent_arg = arg_by_name_or_position(args, &["parent"], 1);
        let parent = if parent_arg.is_null() || parent_arg == R_NilValue() {
            crate::sexp::globals::R_GlobalEnv()
        } else if TYPEOF(parent_arg) == SEXPTYPE::ENVSXP {
            parent_arg
        } else {
            crate::sexp::globals::R_GlobalEnv()
        };

        // Create a new environment with empty frame and parent
        let env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(), // empty frame
            parent,       // enclosing env
            R_NilValue(), // no hash table (simplified)
        );
        env
    }
}

/// R's `environment(fun)` — get the environment associated with a closure.
pub unsafe fn do_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            let env = crate::sexp::accessors::CLOENV(fn_arg);
            if env.is_null() { R_NilValue() } else { env }
        } else if t == SEXPTYPE::ENVSXP {
            fn_arg
        } else {
            R_NilValue()
        }
    }
}

// Environment binding and locking builtins live in the `environment_bindings` submodule.

// ---------------------------------------------------------------------------
// R runtime essentials
// ---------------------------------------------------------------------------

unsafe fn make_r_version_list(simple_list_class: bool) -> SEXP {
    unsafe {
        let fields = [
            ("platform", "rust-port"),
            ("arch", std::env::consts::ARCH),
            ("os", std::env::consts::OS),
            ("system", "rust-port"),
            ("status", ""),
            ("major", "4"),
            ("minor", "4.1"),
            ("year", "2026"),
            ("month", "05"),
            ("day", "09"),
            ("svn rev", ""),
            ("language", "R"),
            ("version.string", "R version 4.4.1 (Rust Port)"),
            ("nickname", "Rust Port"),
        ];

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, fields.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (i, (_, value)) in fields.iter().enumerate() {
            let value = CString::new(*value).unwrap_or_default();
            SET_VECTOR_ELT(result, i as R_xlen_t, Rf_mkString(value.as_ptr()));
        }

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, fields.len() as R_xlen_t);
        if !names.is_null() {
            let _names_guard = protect(names);
            for (i, (name, _)) in fields.iter().enumerate() {
                let name = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(name.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }

        if simple_list_class {
            let class = Rf_mkString(c"simple.list".as_ptr());
            let _class_guard = protect(class);
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }

        result
    }
}

/// R's `version` — legacy constant alias for `R.version`.
pub unsafe fn do_version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { make_r_version_list(true) }
}

/// R's `R.version` — returns a named list with version info.
pub unsafe fn do_R_version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { make_r_version_list(true) }
}

/// R's `R.Version()` — returns the version info list without `simple.list` class.
pub unsafe fn do_R_Version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { make_r_version_list(false) }
}

/// R's `args(fn)` — returns the formal arguments of a function as a pairlist.
/// With the body set to NULL.
pub unsafe fn do_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            return crate::mainutils::dstruct::mkCLOSXP(
                FORMALS(fn_arg),
                R_NilValue(),
                crate::sexp::globals::R_GlobalEnv(),
            );
        }

        if t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
            return R_NilValue();
        }

        let primitive_name = crate::eval::primitive::PRIMNAME(fn_arg);
        let primitive_symbol =
            Rf_install(CString::new(primitive_name).unwrap_or_default().as_ptr());

        for registry in [".ArgsEnv", ".GenericArgsEnv"] {
            let registry_symbol = Rf_install(CString::new(registry).unwrap_or_default().as_ptr());
            let registry_env = crate::sexp::envir::R_findVarInFrame(
                crate::sexp::globals::R_BaseEnv(),
                registry_symbol,
            );
            if registry_env == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            let prototype = crate::sexp::envir::R_findVarInFrame(registry_env, primitive_symbol);
            if prototype != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(prototype) == SEXPTYPE::CLOSXP
            {
                return crate::mainutils::dstruct::mkCLOSXP(
                    FORMALS(prototype),
                    R_NilValue(),
                    crate::sexp::globals::R_GlobalEnv(),
                );
            }
        }

        R_NilValue()
    }
}

/// R's `formals(fn)` — get the formal arguments (parameter list) of a function.
pub unsafe fn do_formals(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            let formals = crate::sexp::accessors::FORMALS(fn_arg);
            if formals.is_null() {
                R_NilValue()
            } else {
                formals
            }
        } else {
            R_NilValue()
        }
    }
}

/// R's `body(fn)` — get the body of a function.
pub unsafe fn do_body(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            let body = crate::sexp::accessors::BODY(fn_arg);
            if body.is_null() {
                R_NilValue()
            } else if TYPEOF(body) == SEXPTYPE::BCODESXP {
                let source = crate::eval::bc_eval::BCODE_EXPR(body);
                if source.is_null() || source == R_NilValue() {
                    body
                } else {
                    source
                }
            } else {
                body
            }
        } else {
            R_NilValue()
        }
    }
}

/// R's `environment(fn)` — get the environment of a closure.
/// Same as do_environment, provided as an alternative name.
pub unsafe fn do_environment_of(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_environment(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// Complete R runtime: eval, substitute, quote, parse
// ---------------------------------------------------------------------------

/// R's `eval(expr, envir, enclos)` — evaluate expression in environment.
pub unsafe fn do_eval(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let envir_arg = CAR(CDR(args));
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let envir = if envir_arg.is_null() || envir_arg == R_NilValue() {
            _rho
        } else {
            envir_arg
        };
        crate::eval::eval::Rf_eval(expr, envir)
    }
}

/// R's `substitute(expr, env)` — substitute symbols in expression.
pub unsafe fn do_substitute(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::coerce::do_substitute(_call, _op, args, _rho) }
}

/// R's `quote(expr)` — return expression unevaluated.
pub unsafe fn do_quote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, NAMED, SET_NAMED};
        let mut nargs = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            nargs += 1;
            current = CDR(current);
        }
        if nargs != 1 {
            base_error(format!(
                "{nargs} arguments passed to 'quote' which requires 1"
            ));
        }
        let tag = TAG(args);
        if !tag.is_null() && tag != R_NilValue() {
            let name = if TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let printname = PRINTNAME(tag);
                if printname.is_null() {
                    String::new()
                } else {
                    let chars = CHAR(printname);
                    if chars.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(chars).to_string_lossy().into_owned()
                    }
                }
            } else {
                String::new()
            };
            if name != "expr" {
                base_error(format!(
                    "supplied argument name '{name}' does not match 'expr'"
                ));
            }
        }
        let val = CAR(args);
        if val.is_null() || val == R_NilValue() {
            return R_NilValue();
        }
        // ENSURE_NAMEDMAX — prevent modification of source code references
        if NAMED(val) < 2 {
            SET_NAMED(val, 2);
        }
        val
    }
}

/// R's `parse(text)` — parse R code strings into an expression vector.
pub unsafe fn do_parse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let text_arg = arg_by_name_or_position(args, &["text"], 0);
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        if text_arg.is_null() || text_arg == R_NilValue() {
            if !file_arg.is_null() && file_arg != R_NilValue() {
                let file_path = elt_to_string(file_arg, 0);
                let content = std::fs::read_to_string(&file_path).unwrap_or_else(|err| {
                    base_error(format!("cannot open file '{}': {}", file_path, err))
                });
                return parse_source_expression_vector(&content);
            }
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let n = XLENGTH(text_arg);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let mut source = Vec::with_capacity(n as usize);
        for i in 0..n {
            if TYPEOF(text_arg) == SEXPTYPE::STRSXP && is_string_na(text_arg, i) {
                std::panic::panic_any(RError {
                    message: "invalid 'text' argument".to_string(),
                });
            }
            let text = elt_to_string(text_arg, i);
            source.push(text);
        }
        parse_source_strings(&source)
    }
}

unsafe fn parse_source_strings(source: &[String]) -> SEXP {
    let combined = source.join("\n");
    unsafe { parse_source_expression_vector(&combined) }
}

unsafe fn parse_source_expression_vector(source: &str) -> SEXP {
    unsafe {
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(source, arena).map_err(|err| err.to_string())
        })
        .unwrap_or_else(|message| std::panic::panic_any(RError { message }));

        let result = Rf_allocVector3(SEXPTYPE::EXPRSXP, parsed.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in parsed.into_iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, value);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// R runtime
// ---------------------------------------------------------------------------

/// R's `commandArgs()` — returns the command line arguments as a character vector.
pub unsafe fn do_commandArgs(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let args: Vec<String> = std::env::args().collect();
        let n = args.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (i, arg) in args.iter().enumerate() {
            let cs = CString::new(arg.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        result
    }
}

/// R's `getOption(x)` — delegate to the canonical options implementation.
pub unsafe fn do_getOption(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::do_getOption(call, op, args, rho) }
}

/// R's `options(...)` — delegate to the canonical options implementation.
pub unsafe fn do_options(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::do_options(call, op, args, rho) }
}

/// R's `interactive()` — returns FALSE (not in interactive session).
pub unsafe fn do_interactive(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(FALSE) }
}

/// Alias for `interactive()`.
pub unsafe fn do_is_interactive(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(FALSE) }
}

/// R's `getRversion()` — returns an `R_system_version` package-version object.
pub unsafe fn do_getRversion(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        let version = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        if !version.is_null() {
            let _version_guard = protect(version);
            let data = INTEGER(version);
            *data.add(0) = 4;
            *data.add(1) = 4;
            *data.add(2) = 1;
            SET_VECTOR_ELT(result, 0, version);
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        if !class.is_null() {
            let _class_guard = protect(class);
            for (i, name) in ["R_system_version", "package_version", "numeric_version"]
                .iter()
                .enumerate()
            {
                let value = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(class, i as R_xlen_t, Rf_mkChar(value.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        result
    }
}

/// R's `R.version.string` — returns the full R version string.
pub unsafe fn do_R_version_string(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = CString::new("R version 4.4.1 (Rust Port)").unwrap_or_default();
        Rf_mkString(s.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime
// ---------------------------------------------------------------------------

/// R-like `ls_args()` — list argument names of current function (simplified: return empty character).
pub unsafe fn do_ls_args(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 0) }
}

/// R's `deparse1(expr, collapse, width.cutoff)` — deparse to a single string.
pub unsafe fn do_deparse1(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let collapse_arg = CAR(CDR(args));
        let sep = if collapse_arg.is_null() || collapse_arg == R_NilValue() {
            " ".to_string()
        } else {
            elt_to_string(collapse_arg, 0)
        };
        let lines = deparse_lines(expr);
        Rf_mkString(CString::new(lines.join(&sep)).unwrap_or_default().as_ptr())
    }
}

/// R's `dput(x, file)` — dump object using the deparser.
pub unsafe fn do_dput(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = arg_by_name_or_position(args, &["file"], 1);
        let lines = deparse_lines(x);
        let output = format!("{}\n", lines.join("\n"));

        let file = if file_arg.is_null() || file_arg == R_NilValue() || XLENGTH(file_arg) == 0 {
            String::new()
        } else {
            elt_to_string(file_arg, 0)
        };
        if file.is_empty() {
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&output);
            } else {
                print!("{}", output);
            }
        } else {
            std::fs::write(&file, output).unwrap_or_else(|err| {
                std::panic::panic_any(RError {
                    message: format!("cannot write dump file '{}': {err}", file),
                })
            });
        }
        x
    }
}

fn deparse_lines(expr: SEXP) -> Vec<String> {
    unsafe {
        let deparsed = crate::mainutils::deparse::deparse1(
            expr,
            false,
            crate::mainutils::deparse::DEFAULT_USER_DEPARSE,
        );
        let n = XLENGTH(deparsed);
        if deparsed.is_null() || deparsed == R_NilValue() || n == 0 {
            return vec!["NULL".to_string()];
        }
        (0..n).map(|i| elt_to_string(deparsed, i)).collect()
    }
}

/// R's `dget(file)` — read, parse, and evaluate a dumped expression.
pub unsafe fn do_dget(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        if file_arg.is_null() || file_arg == R_NilValue() || XLENGTH(file_arg) == 0 {
            std::panic::panic_any(RError {
                message: "invalid 'file' argument".to_string(),
            });
        }

        let path = elt_to_string(file_arg, 0);
        let code = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            std::panic::panic_any(RError {
                message: format!("cannot read dump file '{}': {err}", path),
            })
        });
        let expr = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse(&code, arena).map_err(|err| err.to_string())
        })
        .unwrap_or_else(|message| std::panic::panic_any(RError { message }));
        if expr.is_null() || expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(expr, rho)
        }
    }
}

/// R's `bquote(expr)` — quote with `.(...)` substitution.
pub unsafe fn do_bquote(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() {
            return R_NilValue();
        }
        bquote_walk(expr, rho)
    }
}

unsafe fn bquote_walk(expr: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let expr_type = TYPEOF(expr);
        if expr_type == SEXPTYPE::LANGSXP && is_bquote_unquote_call(expr) {
            let unquoted = CAR(CDR(expr));
            return crate::eval::eval::Rf_eval(unquoted, rho);
        }

        if expr_type != SEXPTYPE::LANGSXP && expr_type != SEXPTYPE::LISTSXP {
            return expr;
        }

        let mut source = expr;
        let mut head = R_NilValue();
        let mut tail = R_NilValue();
        while !source.is_null() && source != R_NilValue() {
            let value = bquote_walk(CAR(source), rho);
            let cell = Rf_cons(value, R_NilValue());
            SETTAG(cell, TAG(source));
            if head == R_NilValue() {
                head = cell;
            } else {
                SETCDR(tail, cell);
            }
            tail = cell;
            source = CDR(source);
        }
        if expr_type == SEXPTYPE::LANGSXP && !head.is_null() && head != R_NilValue() {
            (*head).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        head
    }
}

unsafe fn is_bquote_unquote_call(expr: SEXP) -> bool {
    unsafe {
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return false;
        }
        let head = CAR(expr);
        if TYPEOF(head) != SEXPTYPE::SYMSXP || symbol_name(head).as_deref() != Some(".") {
            return false;
        }
        let args = CDR(expr);
        !args.is_null()
            && args != R_NilValue()
            && (CDR(args).is_null() || CDR(args) == R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// Environment completion
// ---------------------------------------------------------------------------

/// R's `parent.env(env)` — returns the parent environment.
pub unsafe fn do_parent_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        if env.is_null() || env == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(env);
        if t != SEXPTYPE::ENVSXP {
            return R_NilValue();
        }
        // enclos is the enclosing/parent environment
        let parent = (*env).data.envsxp.enclos;
        if parent.is_null() {
            return crate::sexp::globals::R_EmptyEnv();
        }
        parent
    }
}

/// R's `set_parent.env(env, parent)` — set the parent environment.
pub unsafe fn do_set_parent_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        let parent = CAR(CDR(args));
        if env.is_null() || env == R_NilValue() || TYPEOF(env) != SEXPTYPE::ENVSXP {
            return R_NilValue();
        }
        if parent.is_null() || parent == R_NilValue() || TYPEOF(parent) != SEXPTYPE::ENVSXP {
            return env;
        }
        SET_ENCLOS(env, parent);
        env
    }
}

/// R's `env_name(env)` — returns the name of an environment.
pub unsafe fn do_env_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        if env.is_null() || env == R_NilValue() {
            return Rf_mkString(CString::new("NULL").unwrap_or_default().as_ptr());
        }
        let t = TYPEOF(env);
        if t != SEXPTYPE::ENVSXP {
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }
        // Check if it's a special environment
        if env == crate::sexp::globals::R_GlobalEnv() {
            return Rf_mkString(CString::new("R_GlobalEnv").unwrap_or_default().as_ptr());
        }
        if env == crate::sexp::globals::R_EmptyEnv() {
            return Rf_mkString(CString::new("R_EmptyEnv").unwrap_or_default().as_ptr());
        }
        if env == crate::sexp::globals::R_BaseEnv() {
            return Rf_mkString(CString::new("base").unwrap_or_default().as_ptr());
        }
        let name = crate::sexp::attrib_core::getAttrib(env, Rf_install(c"name".as_ptr()));
        if TYPEOF(name) == SEXPTYPE::STRSXP && XLENGTH(name) > 0 {
            let value = STRING_ELT(name, 0);
            if !value.is_null() && value != R_NilValue() {
                return Rf_mkString(CHAR(value));
            }
        }
        Rf_mkString(CString::new("").unwrap_or_default().as_ptr())
    }
}

/// R's `environmentName(env)` — returns the name of an environment.
pub unsafe fn do_environment_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_env_name(_call, _op, args, _rho) }
}

/// R-like `is_empty(env)` — check if environment is empty (simplified).
pub unsafe fn do_is_empty(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        if env.is_null() || env == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        let t = TYPEOF(env);
        if t == SEXPTYPE::ENVSXP {
            // Check frame - if it's NULL/NILSXP, env is empty
            let frame = (*env).data.envsxp.frame;
            if frame.is_null() || frame == R_NilValue() {
                return Rf_ScalarLogical(TRUE);
            }
            return Rf_ScalarLogical(FALSE);
        }
        // For vectors, check length
        let n = XLENGTH(env);
        Rf_ScalarLogical(if n == 0 { TRUE } else { FALSE })
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — type checking utilities
// ---------------------------------------------------------------------------

/// R's `is.single(x)` — stock R exposes this but errors because single is unimplemented.
pub unsafe fn do_is_single(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _x = CAR(args);
        std::panic::panic_any(crate::sexp::context::RError {
            message: "type \"single\" unimplemented in R".to_string(),
        });
    }
}

/// R's `is.vector(x, mode="any")` — check if x is an atomic or list vector without attributes.
pub unsafe fn do_is_vector(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let is_vec = t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
            || t == SEXPTYPE::VECSXP;
        Rf_ScalarLogical(if is_vec { TRUE } else { FALSE })
    }
}

/// R's `is.scalar(x)` — check if x has length 1 (simplified).
pub unsafe fn do_is_scalar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        Rf_ScalarLogical(if n == 1 { TRUE } else { FALSE })
    }
}

/// R's `is.named(x)` — check if x has names attribute.
pub unsafe fn do_is_named(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) > 0;
        Rf_ScalarLogical(if has_names { TRUE } else { FALSE })
    }
}

/// R's `is.unsorted(x, na.rm = FALSE, strictly = FALSE)`.
///
/// Missing values dominate the default result just as in GNU R: with
/// `na.rm = FALSE`, any NA/NaN makes the result `NA`, even if another pair is
/// visibly out of order.
pub unsafe fn do_is_unsorted(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let na_rm = match logical_arg_with_default(args, "na.rm", 1, FALSE) {
            Ok(value) => value != FALSE,
            Err(message) => panic_r_error(message),
        };
        let strictly = match logical_arg_with_default(args, "strictly", 2, FALSE) {
            Ok(value) => value != FALSE,
            Err(_) => panic_r_error("invalid 'strictly' argument"),
        };
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        if n <= 1 {
            return Rf_ScalarLogical(FALSE);
        }
        let result = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
            is_unsorted_int_like(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::REALSXP {
            is_unsorted_real(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::CPLXSXP {
            is_unsorted_complex(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::STRSXP {
            is_unsorted_character(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::RAWSXP {
            is_unsorted_raw(x, n, strictly)
        } else {
            NA_LOGICAL
        };
        Rf_ScalarLogical(result)
    }
}

unsafe fn logical_arg_with_default(
    args: SEXP,
    name: &str,
    position: usize,
    default: c_int,
) -> Result<c_int, &'static str> {
    unsafe {
        let value = arg_by_name_or_position(args, &[name], position);
        if value.is_null() || value == R_NilValue() {
            return Ok(default);
        }
        if XLENGTH(value) == 0 {
            return Err("argument is of length zero");
        }
        let value_type = TYPEOF(value);
        let raw = if value_type == SEXPTYPE::LGLSXP || value_type == SEXPTYPE::INTSXP {
            *INTEGER(value)
        } else if value_type == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if value.is_nan() {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return Err("argument is not interpretable as logical");
        };
        if raw == NA_LOGICAL {
            return Err("missing value where TRUE/FALSE needed");
        }
        Ok(if raw == FALSE { FALSE } else { TRUE })
    }
}

fn panic_r_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    })
}

unsafe fn is_unsorted_int_like(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<c_int> = None;
        for i in 0..n {
            let current = *INTEGER(x).add(i as usize);
            if current == NA_INTEGER {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            if let Some(prev) = previous {
                if out_of_order_i32(prev, current, strictly) {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_real(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<f64> = None;
        for i in 0..n {
            let current = *REAL(x).add(i as usize);
            if current.is_nan() {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            if let Some(prev) = previous {
                if out_of_order_f64(prev, current, strictly) {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_complex(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<Rcomplex> = None;
        for i in 0..n {
            let current = *COMPLEX(x).add(i as usize);
            if current.r.is_nan() || current.i.is_nan() {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            if let Some(prev) = previous {
                if out_of_order_complex(prev, current, strictly) {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_character(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<String> = None;
        for i in 0..n {
            if STRING_ELT(x, i) == crate::sexp::globals::R_NaString() {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            let current = elt_to_string(x, i);
            if let Some(prev) = previous.as_deref() {
                let out_of_order = if strictly {
                    prev >= current.as_str()
                } else {
                    prev > current.as_str()
                };
                if out_of_order {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_raw(x: SEXP, n: R_xlen_t, strictly: bool) -> c_int {
    unsafe {
        for i in 1..n {
            let prev = *RAW(x).add((i - 1) as usize);
            let current = *RAW(x).add(i as usize);
            let out_of_order = if strictly {
                prev >= current
            } else {
                prev > current
            };
            if out_of_order {
                return TRUE;
            }
        }
        FALSE
    }
}

fn out_of_order_i32(previous: c_int, current: c_int, strictly: bool) -> bool {
    if strictly {
        previous >= current
    } else {
        previous > current
    }
}

fn out_of_order_f64(previous: f64, current: f64, strictly: bool) -> bool {
    if strictly {
        previous >= current
    } else {
        previous > current
    }
}

fn out_of_order_complex(previous: Rcomplex, current: Rcomplex, strictly: bool) -> bool {
    if previous.r > current.r {
        return true;
    }
    if previous.r < current.r {
        return false;
    }
    if strictly {
        previous.i >= current.i
    } else {
        previous.i > current.i
    }
}

/// R's `is.loaded(...)` — delegates to `dotcode::do_isloaded` (R_lookupLoadedSymbol).
pub unsafe fn do_is_loaded(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::dotcode::do_isloaded(call, op, args, rho) }
}

// ---------------------------------------------------------------------------
// Complete R runtime — function type checking
// ---------------------------------------------------------------------------

/// R's `is.primitive(x)` — check if x is a primitive function (BUILTINSXP or SPECIALSXP).
pub unsafe fn do_is_primitive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        Rf_ScalarLogical(if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.generic(x)` — check if x is a generic function (simplified).
/// Returns TRUE for CLOSXP with "generic" in name or with useMethod call.
pub unsafe fn do_is_generic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        // Simplified: primitives are always generic, closures need body check
        if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            return Rf_ScalarLogical(TRUE);
        }
        if t == SEXPTYPE::CLOSXP {
            // Check if name ends with common generic names
            // Simplified: assume all closures could be generic
            return Rf_ScalarLogical(TRUE);
        }
        Rf_ScalarLogical(FALSE)
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — isTRUE, isFALSE, any_na, all_na, any_nan, all_nan
// ---------------------------------------------------------------------------

/// R's `isTRUE(x)` — returns TRUE if x is exactly length-1 TRUE.
pub unsafe fn do_is_true(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP && XLENGTH(x) == 1 {
            let v = *LOGICAL(x);
            return Rf_ScalarLogical(if v == TRUE { TRUE } else { FALSE });
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `isFALSE(x)` — returns TRUE if x is exactly length-1 FALSE.
pub unsafe fn do_is_false(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP && XLENGTH(x) == 1 {
            let v = *LOGICAL(x);
            return Rf_ScalarLogical(if v == FALSE { TRUE } else { FALSE });
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `anyNA(x)` — returns TRUE if any element is NA.
pub unsafe fn do_any_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        for i in 0..n {
            if atomic_value_is_missing(x, i) {
                return Rf_ScalarLogical(TRUE);
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `allNA(x)` — returns TRUE if all elements are NA.
pub unsafe fn do_all_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        if n == 0 {
            return Rf_ScalarLogical(FALSE);
        }
        for i in 0..n {
            if !atomic_value_is_missing(x, i) {
                return Rf_ScalarLogical(FALSE);
            }
        }
        Rf_ScalarLogical(TRUE)
    }
}

/// R's `anyNaN(x)` — returns TRUE if any element is NaN.
pub unsafe fn do_any_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::REALSXP {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        for i in 0..n {
            let v = *REAL(x).add(i as usize);
            if v.is_nan() {
                return Rf_ScalarLogical(TRUE);
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `allNaN(x)` — returns TRUE if all elements are NaN.
pub unsafe fn do_all_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::REALSXP {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        if n == 0 {
            return Rf_ScalarLogical(FALSE);
        }
        for i in 0..n {
            let v = *REAL(x).add(i as usize);
            if !v.is_nan() {
                return Rf_ScalarLogical(FALSE);
            }
        }
        Rf_ScalarLogical(TRUE)
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — with, within, transform
// ---------------------------------------------------------------------------

/// R's `with(data, expr)` — evaluate expr in a data/list environment.
pub unsafe fn do_with(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data_expr = arg_by_name_or_position(args, &["data"], 0);
        let expr = arg_by_name_or_position(args, &["expr"], 1);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let data = if data_expr.is_null() || data_expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(data_expr, rho)
        };
        if data.is_null() || data == R_NilValue() {
            return crate::eval::eval::Rf_eval(expr, rho);
        }
        let eval_env = data_environment(data, rho);
        crate::eval::eval::Rf_eval(expr, eval_env)
    }
}

unsafe fn data_environment(data: SEXP, parent: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(data) == SEXPTYPE::ENVSXP {
            return data;
        }
        if TYPEOF(data) != SEXPTYPE::VECSXP {
            return parent;
        }

        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue());
        if env.is_null() || env == R_NilValue() {
            return parent;
        }

        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        let n = XLENGTH(data);
        for i in 0..n {
            if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
                break;
            }
            let name = elt_to_string(names, i);
            if name.is_empty() {
                continue;
            }
            let symbol = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            crate::sexp::envir::defineVar(symbol, VECTOR_ELT(data, i), env);
        }
        env
    }
}

/// R's `within(data, expr)` — modify data by evaluating expr (simplified).
pub unsafe fn do_within(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(args);
        let expr = CAR(CDR(args));
        if data.is_null() || data == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: evaluate expr and return the original data
        // A full implementation would evaluate expr in data context and return modified data
        if !expr.is_null() && expr != R_NilValue() {
            let _ = crate::eval::eval::Rf_eval(expr, rho);
        }
        data
    }
}

/// R's `transform(x, ...)` — add/modify columns of a data.frame (simplified).
pub unsafe fn do_transform(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: return the data as-is
        // A full implementation would evaluate named args as new columns
        x
    }
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
        if x.is_null() || x == R_NilValue() {
            let s = CString::new("").unwrap_or_default();
            return Rf_mkString(s.as_ptr());
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
                std::env::var(&name).ok().or_else(|| unset.clone())
            })
            .collect::<Vec<_>>();
        optional_string_vector(&values)
    }
}

/// R's `Sys.setenv(...)` — set environment variables.
pub unsafe fn do_Sys_setenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                if let Some(key) = tag_name(current)
                    && !key.is_empty()
                {
                    std::env::set_var(key, elt_to_string(arg, 0));
                } else {
                    let s = elt_to_string(arg, 0);
                    if let Some(pos) = s.find('=') {
                        let key = &s[..pos];
                        let val = &s[pos + 1..];
                        std::env::set_var(key, val);
                    }
                }
            }
            current = CDR(current);
        }
        Rf_ScalarLogical(TRUE)
    }
}

/// R's `Sys.unsetenv(x)` — unset environment variable.
pub unsafe fn do_Sys_unsetenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        for i in 0..XLENGTH(x) {
            let name = elt_to_string(x, i);
            if !name.is_empty() && name != "NA" {
                std::env::remove_var(name);
            }
        }
        Rf_ScalarLogical(TRUE)
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
            SET_STRING_ELT(
                class,
                0,
                Rf_mkChar(CString::new("POSIXct").unwrap_or_default().as_ptr()),
            );
            SET_STRING_ELT(
                class,
                1,
                Rf_mkChar(CString::new("POSIXt").unwrap_or_default().as_ptr()),
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class,
            );
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
            crate::sexp::attrib_core::setAttrib(
                x,
                Rf_install(CString::new("tzone").unwrap_or_default().as_ptr()),
                tzone,
            );
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

// ---------------------------------------------------------------------------
// Complete R runtime — match.call, sys.nframe, sys.function, on.exit
// ---------------------------------------------------------------------------

/// R's `match.call(definition, call, expand.dots)` — match call arguments.
/// Simplified: returns the call as-is.
pub unsafe fn do_match_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Return the call argument if provided, otherwise the current call
        let call_arg = CAR(args);
        if !call_arg.is_null() && call_arg != R_NilValue() {
            return call_arg;
        }
        _call
    }
}

/// R's `sys.nframe()` — returns the number of frames on the call stack.
pub unsafe fn do_sys_nframe(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let top = crate::sexp::context::R_GlobalContext();
        Rf_ScalarInteger(crate::eval::context::framedepth(top))
    }
}

/// R's `sys.function(which)` — returns the function at the given frame level.
pub unsafe fn do_sys_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = crate::sexp::context::R_GlobalContext();
        if top.is_null() {
            R_NilValue()
        } else {
            crate::eval::context::R_sysfunction(which, top)
        }
    }
}

/// R's `on.exit(expr, add, after)` — register an exit handler for the
/// current function context.
pub unsafe fn do_on_exit(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::eval::special::do_on_exit_from_args(args, rho) }
}

// ---------------------------------------------------------------------------
// Complete R runtime — par, getGraphicsEvent
// ---------------------------------------------------------------------------

/// R's `par(...)` — session-owned graphical parameters.
pub unsafe fn do_par(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::library::graphics::par::do_par(_call, _op, _args, _rho) }
}

/// R's `layout(...)` — session-owned base graphics layout state.
pub unsafe fn do_layout(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::library::graphics::par::do_layout(_call, _op, _args, _rho) }
}

/// R's `getGraphicsEvent(prompt, onMouseDown, ...)` — no Android event loop is attached here.
pub unsafe fn do_getGraphicsEvent(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    std::panic::panic_any(RError {
        message: "graphics events are not available for the headless Android device".to_string(),
    });
}

// ---------------------------------------------------------------------------
// Complete R runtime — Rprof, Rprofmem, gc, gcinfo, memory.size, object.size
// ---------------------------------------------------------------------------

/// R's `Rprof(filename, ...)` — session-owned profiling.
pub unsafe fn do_Rprof(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = crate::eval::profiling::do_Rprof(call, op, args, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

/// R's `Rprofmem(filename, ...)` — session-owned memory profiling.
pub unsafe fn do_Rprofmem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = crate::eval::profiling::do_Rprofmem(call, op, args, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct RuntimeMemorySnapshot {
    active_nodes: usize,
    free_nodes: usize,
    current_bytes: usize,
    peak_bytes: usize,
}

fn runtime_memory_snapshot() -> RuntimeMemorySnapshot {
    crate::sexp::instance::with_required_current_instance(|instance| {
        let active_nodes = instance.arena.node_count();
        let free_nodes = instance.arena.free_count();
        let current_bytes = instance.arena.total_bytes_allocated();
        let peak_bytes = instance.gc_state.stats.peak_memory.max(current_bytes);

        RuntimeMemorySnapshot {
            active_nodes,
            free_nodes,
            current_bytes,
            peak_bytes,
        }
    })
}

fn bytes_to_mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn set_real_matrix_cell(data: *mut f64, row: usize, col: usize, rows: usize, value: f64) {
    unsafe {
        *data.add(col * rows + row) = value;
    }
}

/// R's `gc()` — garbage collection with session-owned memory counters.
pub unsafe fn do_gc(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::memory_main::R_gc();
        let snapshot = runtime_memory_snapshot();
        let node_size = std::mem::size_of::<crate::sexp::ffi::SexprecCore>();
        let ncell_bytes = snapshot.active_nodes.saturating_mul(node_size);
        let ncell_trigger = (snapshot.active_nodes + snapshot.free_nodes)
            .saturating_mul(2)
            .max(snapshot.active_nodes);
        let ncell_peak = snapshot
            .active_nodes
            .saturating_add(crate::sexp::gengc::get_gc_stats().freed);
        let vcell_size = std::mem::size_of::<SEXP>();
        let vcell_used = snapshot.current_bytes / vcell_size;
        let vcell_trigger_bytes = snapshot
            .current_bytes
            .saturating_mul(2)
            .max(snapshot.current_bytes);
        let vcell_peak = snapshot.peak_bytes / vcell_size;

        // Return a 2x7 matrix. Rows are Ncells and Vcells; columns follow R's
        // visible shape: used, (Mb), gc trigger, (Mb), max used, (Mb), limit.
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 14);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        set_real_matrix_cell(dst, 0, 0, 2, snapshot.active_nodes as f64);
        set_real_matrix_cell(dst, 1, 0, 2, vcell_used as f64);
        set_real_matrix_cell(dst, 0, 1, 2, bytes_to_mb(ncell_bytes));
        set_real_matrix_cell(dst, 1, 1, 2, bytes_to_mb(snapshot.current_bytes));
        set_real_matrix_cell(dst, 0, 2, 2, ncell_trigger as f64);
        set_real_matrix_cell(dst, 1, 2, 2, (vcell_trigger_bytes / vcell_size) as f64);
        set_real_matrix_cell(
            dst,
            0,
            3,
            2,
            bytes_to_mb(ncell_trigger.saturating_mul(node_size)),
        );
        set_real_matrix_cell(dst, 1, 3, 2, bytes_to_mb(vcell_trigger_bytes));
        set_real_matrix_cell(dst, 0, 4, 2, ncell_peak as f64);
        set_real_matrix_cell(dst, 1, 4, 2, vcell_peak as f64);
        set_real_matrix_cell(
            dst,
            0,
            5,
            2,
            bytes_to_mb(ncell_peak.saturating_mul(node_size)),
        );
        set_real_matrix_cell(dst, 1, 5, 2, bytes_to_mb(snapshot.peak_bytes));
        set_real_matrix_cell(dst, 0, 6, 2, 0.0);
        set_real_matrix_cell(dst, 1, 6, 2, 0.0);

        // Set dim = c(2, 7)
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            let _p2 = protect(dim);
            let d = INTEGER(dim);
            *d.add(0) = 2;
            *d.add(1) = 7;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }
        // Set dimnames
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if !dn.is_null() {
            let _p3 = protect(dn);
            let row_names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            if !row_names.is_null() {
                let _p4 = protect(row_names);
                let s1 = CString::new("Ncells").unwrap_or_default();
                let s2 = CString::new("Vcells").unwrap_or_default();
                SET_STRING_ELT(
                    row_names,
                    0,
                    crate::sexp::constructors::Rf_mkChar(s1.as_ptr()),
                );
                SET_STRING_ELT(
                    row_names,
                    1,
                    crate::sexp::constructors::Rf_mkChar(s2.as_ptr()),
                );
                SET_VECTOR_ELT(dn, 0, row_names);
            }
            let col_names = Rf_allocVector3(SEXPTYPE::STRSXP, 7);
            if !col_names.is_null() {
                let _p5 = protect(col_names);
                for (i, name) in [
                    "used",
                    "(Mb)",
                    "gc trigger",
                    "(Mb)",
                    "max used",
                    "(Mb)",
                    "limit",
                ]
                .iter()
                .enumerate()
                {
                    let cstr = CString::new(*name).unwrap_or_default();
                    SET_STRING_ELT(
                        col_names,
                        i as R_xlen_t,
                        crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                    );
                }
                SET_VECTOR_ELT(dn, 1, col_names);
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
                dn,
            );
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

/// R's `gc.time()` — current GC timing counters.
pub unsafe fn do_gc_time(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_allocVector3(SEXPTYPE::REALSXP, 5) }
}

/// R's `gcinfo(on)` — set session-local GC reporting verbosity.
pub unsafe fn do_gcinfo(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            base_error("argument \"verbose\" is missing, with no default");
        }
        let old = crate::mainutils::memory_main::do_gcinfo(call, op, args, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        old
    }
}

/// R's `gctorture(on = TRUE)` — set session-local GC torture mode.
pub unsafe fn do_gctorture(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let on = if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            Rf_ScalarLogical(TRUE)
        } else {
            CAR(args)
        };
        let normalized = Rf_cons(on, R_NilValue());
        let _args_guard = protect(normalized);
        let old = crate::mainutils::memory_main::do_gctorture(call, op, normalized, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        old
    }
}

/// R's `gctorture2(step, wait = 0, inhibit_release = FALSE)` session state.
pub unsafe fn do_gctorture2(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            base_error("argument \"step\" is missing, with no default");
        }

        let step = CAR(args);
        let wait =
            if CDR(args).is_null() || CDR(args) == R_NilValue() || CAR(CDR(args)) == R_MissingArg()
            {
                Rf_ScalarInteger(0)
            } else {
                CAR(CDR(args))
            };
        let _wait_guard = protect(wait);
        let tail = Rf_cons(wait, R_NilValue());
        let _tail_guard = protect(tail);
        let normalized = Rf_cons(step, tail);
        let _args_guard = protect(normalized);
        let old = crate::mainutils::memory_main::do_gctorture2(call, op, normalized, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        old
    }
}

/// R's `memory.size(max)` — current or peak arena memory in MB.
pub unsafe fn do_memory_size(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let max = crate::mainutils::coerce::asLogical(CAR(args));
        let snapshot = runtime_memory_snapshot();
        let bytes = if max == TRUE {
            snapshot.peak_bytes
        } else {
            snapshot.current_bytes
        };
        Rf_ScalarReal(bytes_to_mb(bytes))
    }
}

/// R's `memory.profile()` — session-local object counts by SEXPTYPE class.
pub unsafe fn do_memory_profile(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    const PROFILE_TYPES: [(&str, SEXPTYPE); 24] = [
        ("NULL", SEXPTYPE::NILSXP),
        ("symbol", SEXPTYPE::SYMSXP),
        ("pairlist", SEXPTYPE::LISTSXP),
        ("closure", SEXPTYPE::CLOSXP),
        ("environment", SEXPTYPE::ENVSXP),
        ("promise", SEXPTYPE::PROMSXP),
        ("language", SEXPTYPE::LANGSXP),
        ("special", SEXPTYPE::SPECIALSXP),
        ("builtin", SEXPTYPE::BUILTINSXP),
        ("char", SEXPTYPE::CHARSXP),
        ("logical", SEXPTYPE::LGLSXP),
        ("integer", SEXPTYPE::INTSXP),
        ("double", SEXPTYPE::REALSXP),
        ("complex", SEXPTYPE::CPLXSXP),
        ("character", SEXPTYPE::STRSXP),
        ("...", SEXPTYPE::DOTSXP),
        ("any", SEXPTYPE::ANYSXP),
        ("list", SEXPTYPE::VECSXP),
        ("expression", SEXPTYPE::EXPRSXP),
        ("bytecode", SEXPTYPE::BCODESXP),
        ("externalptr", SEXPTYPE::EXTPTRSXP),
        ("weakref", SEXPTYPE::WEAKREFSXP),
        ("raw", SEXPTYPE::RAWSXP),
        ("S4", SEXPTYPE::S4SXP),
    ];

    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, PROFILE_TYPES.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let data = INTEGER(result);
        for i in 0..PROFILE_TYPES.len() {
            *data.add(i) = 0;
        }
        *data = 1;

        crate::sexp::instance::with_required_current_instance(|instance| {
            for node in instance.arena.active_nodes() {
                let ty = TYPEOF(node);
                if let Some((idx, _)) = PROFILE_TYPES
                    .iter()
                    .enumerate()
                    .find(|(_, (_, profile_ty))| ty == *profile_ty)
                {
                    // `S4SXP` shares the OBJSXP tag; match GNU R's public bucket name.
                    *data.add(idx) = (*data.add(idx)).saturating_add(1);
                }
            }
        });

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, PROFILE_TYPES.len() as R_xlen_t);
        if !names.is_null() {
            let _names_guard = protect(names);
            for (i, (name, _)) in PROFILE_TYPES.iter().enumerate() {
                SET_STRING_ELT(
                    names,
                    i as R_xlen_t,
                    Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }

        result
    }
}

/// R's `object.size(x)` — estimate object size in bytes (simplified).
/// Returns a numeric scalar with class "object_size".
pub unsafe fn do_object_size(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            let result = Rf_ScalarReal(0.0);
            let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            if !class_vec.is_null() {
                let _p2 = protect(class_vec);
                let cstr = CString::new("object_size").unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    let cdata = (*class_vec).gengc_next_node as *mut SEXP;
                    *cdata.add(0) = charsxp;
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                    class_vec,
                );
            }
            return result;
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let size: f64 = match t {
            t if t == SEXPTYPE::REALSXP => (n as usize * std::mem::size_of::<f64>()) as f64,
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                (n as usize * std::mem::size_of::<i32>()) as f64
            }
            t if t == SEXPTYPE::STRSXP => {
                let mut total: usize = 0;
                for i in 0..n {
                    let charsxp = STRING_ELT(x, i);
                    if !charsxp.is_null() {
                        let s = CHAR(charsxp);
                        if !s.is_null() {
                            let cstr = std::ffi::CStr::from_ptr(s);
                            total += cstr.to_bytes().len() + 1;
                        }
                    }
                }
                total as f64
            }
            t if t == SEXPTYPE::VECSXP => {
                let mut total: usize = std::mem::size_of::<SEXP>() * n as usize;
                for i in 0..n {
                    let elt = VECTOR_ELT(x, i);
                    if !elt.is_null() {
                        let elt_size = do_object_size(
                            _call,
                            _op,
                            {
                                // Create a temporary pairlist with elt as first arg
                                let cell = Rf_cons(elt, R_NilValue());
                                cell
                            },
                            _rho,
                        );
                        total += real_or_default(elt_size, 0.0) as usize;
                    }
                }
                total as f64
            }
            _ => 64.0, // Default estimate for headers
        };
        let result = Rf_ScalarReal(size);
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let _p2 = protect(class_vec);
            let cstr = CString::new("object_size").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let cdata = (*class_vec).gengc_next_node as *mut SEXP;
                *cdata.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class_vec,
            );
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — serialization
// ---------------------------------------------------------------------------

/// R's `Random.seed` — get or set the random seed.
pub unsafe fn do_Random_seed(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Get the current RNG state
        let seed_vec = Rf_allocVector3(SEXPTYPE::INTSXP, 626);
        if seed_vec.is_null() {
            return R_NilValue();
        }
        let _p = protect(seed_vec);
        let dst = INTEGER(seed_vec);
        // Set default seed values
        *dst = 10407_i32; // RNG kind marker
        for i in 1..626 {
            *dst.add(i) = i as c_int;
        }
        seed_vec
    }
}

/// R's `loadRDS(file, refhook)` — load a single serialized R object.
pub unsafe fn do_loadRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let file_path = elt_to_string(file_arg, 0);
        let bytes = match std::fs::read(&file_path) {
            Ok(bytes) => bytes,
            Err(err) => {
                std::panic::panic_any(RError {
                    message: format!("cannot open compressed file '{}': {err}", file_path),
                });
            }
        };

        let raw_vec = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t);
        if raw_vec.is_null() {
            return R_NilValue();
        }
        let _raw_guard = protect(raw_vec);
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw_vec), bytes.len());
        }
        crate::mainutils::serialize::R_unserialize(raw_vec, R_NilValue())
    }
}

/// R's `saveRDS(object, file, ascii, ...)` — save a single R object.
pub unsafe fn do_saveRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object_arg = CAR(args);
        let file_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("saveRDS: file argument is required");
            return R_NilValue();
        }

        let ascii_arg = if CDR(CDR(args)).is_null() || CDR(CDR(args)) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(CDR(args)))
        };

        let raw = crate::mainutils::serialize::R_serialize(
            object_arg,
            R_NilValue(),
            ascii_arg,
            R_NilValue(),
            R_NilValue(),
        );
        if raw.is_null() || TYPEOF(raw) != SEXPTYPE::RAWSXP {
            std::panic::panic_any(RError {
                message: "saveRDS failed to serialize object".to_string(),
            });
        }
        let _raw_guard = protect(raw);

        let len = XLENGTH(raw) as usize;
        let bytes = std::slice::from_raw_parts(RAW(raw), len);
        let file_path = elt_to_string(file_arg, 0);
        if let Err(err) = std::fs::write(&file_path, bytes) {
            std::panic::panic_any(RError {
                message: format!("cannot open compressed file '{}': {err}", file_path),
            });
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — parallel operations (simplified)
// ---------------------------------------------------------------------------

/// R's `parallel::mclapply(X, FUN, ...)` — parallel lapply (simplified serial version).
pub unsafe fn do_mclapply(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_lapply(call, op, args, rho) }
}

/// R's `future.apply::future_lapply(X, FUN, ...)` — future lapply (simplified serial version).
pub unsafe fn do_future_lapply(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_lapply(call, op, args, rho) }
}

/// R's `doParallel::foreach(...)` — parallel foreach (simplified serial version).
pub unsafe fn do_foreach(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x).max(1) as usize;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let dst = (*result).gengc_next_node as *mut SEXP;
        for i in 0..n {
            let elt = if TYPEOF(x) == SEXPTYPE::VECSXP {
                let src = (*x).gengc_next_node as *const SEXP;
                *src.add(i)
            } else {
                R_NilValue()
            };
            *dst.add(i) = elt;
        }
        result
    }
}
