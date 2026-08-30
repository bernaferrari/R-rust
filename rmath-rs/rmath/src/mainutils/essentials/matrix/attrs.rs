//! Attribute access: names, class, attr, attributes, structure, comment, namespace lookup, storage.mode — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
use super::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::path::Path;

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
    FALSE, NA_INTEGER, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use crate::sexp::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol};

/// R's `storage.mode(x) <- value` — coerce storage while preserving attributes.
pub unsafe fn do_storage_mode_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        let target_type = match storage_mode_target(value) {
            Ok(target_type) => target_type,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };

        if TYPEOF(x) == target_type {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        if inherits_class(x, "factor") {
            std::panic::panic_any(RError {
                message: "invalid to change the storage mode of a factor".to_string(),
            });
        }

        let result = crate::mainutils::coerce::coerceVector(x, target_type);
        let _result_guard = protect(result);
        crate::sexp::accessors::SET_ATTRIB(result, ATTRIB(x));
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

pub unsafe fn storage_mode_target(value: SEXP) -> Result<c_int, String> {
    unsafe {
        if value.is_null()
            || value == R_NilValue()
            || TYPEOF(value) != SEXPTYPE::STRSXP
            || XLENGTH(value) < 1
            || is_string_na(value, 0)
        {
            return Err("'value' must be non-null character string".to_string());
        }

        let mode = elt_to_string(value, 0);
        match mode.as_str() {
            "logical" => Ok(SEXPTYPE::LGLSXP.as_c_int()),
            "integer" => Ok(SEXPTYPE::INTSXP.as_c_int()),
            "double" => Ok(SEXPTYPE::REALSXP.as_c_int()),
            "complex" => Ok(SEXPTYPE::CPLXSXP.as_c_int()),
            "character" => Ok(SEXPTYPE::STRSXP.as_c_int()),
            "raw" => Ok(SEXPTYPE::RAWSXP.as_c_int()),
            "list" => Ok(SEXPTYPE::VECSXP.as_c_int()),
            "expression" => Ok(SEXPTYPE::EXPRSXP.as_c_int()),
            "real" => Err("use of 'real' is defunct: use 'double' instead".to_string()),
            "single" => Err("use of 'single' is defunct: use mode<- instead".to_string()),
            _ => Err("invalid value".to_string()),
        }
    }
}

/// R's `rownames(x)` — get row names attribute.
pub unsafe fn do_rownames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        );
        if !dimnames.is_null() && TYPEOF(dimnames) == SEXPTYPE::VECSXP && LENGTH(dimnames) >= 1 {
            return VECTOR_ELT(dimnames, 0);
        }
        if is_data_frame_like(x) {
            return string_vector(&data_frame_row_names(x));
        }
        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
        )
    }
}

/// R's `colnames(x)` — get column names attribute.
pub unsafe fn do_colnames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        );
        if !dimnames.is_null() && TYPEOF(dimnames) == SEXPTYPE::VECSXP && LENGTH(dimnames) >= 2 {
            VECTOR_ELT(dimnames, 1)
        } else {
            R_NilValue()
        }
    }
}

/// R's `names(x)` — get names attribute (alias for do_names).
pub unsafe fn do_names_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_names(_call, _op, args, _rho) }
}

/// R's `names(x) <- value` — set names attribute.
pub unsafe fn do_names_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `dimnames(x) <- value` — set matrix/array dimension names.
pub unsafe fn do_dimnames_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `rownames(x) <- value` — set matrix row names through dimnames[[1]].
pub unsafe fn do_rownames_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { set_matrix_dimname(args, 0) }
}

/// R's `colnames(x) <- value` — set matrix column names through dimnames[[2]].
pub unsafe fn do_colnames_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { set_matrix_dimname(args, 1) }
}

pub unsafe fn set_matrix_dimname(args: SEXP, axis: i64) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let dimnames_sym = Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr());
        let mut dimnames = crate::sexp::attrib_core::getAttrib(x, dimnames_sym);
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            if dimnames.is_null() {
                return x;
            }
            let _dimnames_guard = protect(dimnames);
            SET_VECTOR_ELT(dimnames, 0, R_NilValue());
            SET_VECTOR_ELT(dimnames, 1, R_NilValue());
            crate::sexp::attrib_core::setAttrib(x, dimnames_sym, dimnames);
        }

        if LENGTH(dimnames) > axis as i32 {
            SET_VECTOR_ELT(dimnames, axis, value);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `class(x)` — get class attribute.
pub unsafe fn do_class_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if class.is_null() || class == R_NilValue() {
            let dim =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
            if !dim.is_null() && dim != R_NilValue() && TYPEOF(dim) == SEXPTYPE::INTSXP {
                if XLENGTH(dim) == 2 {
                    let result = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
                    if result.is_null() {
                        return result;
                    }
                    let _result_guard = protect(result);
                    SET_STRING_ELT(result, 0, Rf_mkChar(c"matrix".as_ptr()));
                    SET_STRING_ELT(result, 1, Rf_mkChar(c"array".as_ptr()));
                    return result;
                }
                return Rf_mkString(c"array".as_ptr());
            }
            let t = TYPEOF(x);
            let name = if t == SEXPTYPE::REALSXP {
                "numeric"
            } else if t == SEXPTYPE::INTSXP {
                "integer"
            } else if t == SEXPTYPE::LGLSXP {
                "logical"
            } else if t == SEXPTYPE::STRSXP {
                "character"
            } else if t == SEXPTYPE::VECSXP {
                "list"
            } else {
                "NULL"
            };
            let cstr = CString::new(name).unwrap_or_default();
            Rf_mkString(cstr.as_ptr())
        } else {
            class
        }
    }
}

/// R's `.class2(x)` — class vector including implicit primitive inheritance.
pub unsafe fn do_class2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(c"NULL".as_ptr());
        }

        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if !class.is_null() && class != R_NilValue() {
            return class;
        }

        let implicit: &[&std::ffi::CStr] = match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP => &[c"integer", c"numeric"],
            t if t == SEXPTYPE::REALSXP => &[c"numeric"],
            t if t == SEXPTYPE::LGLSXP => &[c"logical"],
            t if t == SEXPTYPE::CPLXSXP => &[c"complex"],
            t if t == SEXPTYPE::STRSXP => &[c"character"],
            t if t == SEXPTYPE::RAWSXP => &[c"raw"],
            t if t == SEXPTYPE::VECSXP => &[c"list"],
            t if t == SEXPTYPE::LANGSXP => &[c"call"],
            _ => &[c"NULL"],
        };

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, implicit.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        for (i, name) in implicit.iter().enumerate() {
            SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(name.as_ptr()));
        }
        result
    }
}

/// R's `class(x) <- value` — set class attribute.
pub unsafe fn do_class_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `oldClass(x)` — direct S3 class attribute without implicit defaults.
pub unsafe fn do_oldClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol())
    }
}

/// R's `oldClass(x) <- value` — set or remove the direct S3 class attribute.
pub unsafe fn do_oldClass_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_ClassSymbol(), value);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

// ---------------------------------------------------------------------------
// Attribute access helpers
// ---------------------------------------------------------------------------

/// R's `attr(x, which)` — get arbitrary attribute by name.
pub unsafe fn do_attr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let which = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
            return R_NilValue();
        }
        let attr_name = elt_to_string(which, 0);
        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
        )
    }
}

/// R's `attr(x, which) <- value` — set or remove a single attribute.
pub unsafe fn do_attr_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let which = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
            return R_NilValue();
        }
        let attr_name = elt_to_string(which, 0);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `attributes(x) <- value` — replace all attributes from a named list.
pub unsafe fn do_attributes_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        crate::sexp::accessors::SET_ATTRIB(x, R_NilValue());
        if value.is_null() || value == R_NilValue() {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        if TYPEOF(value) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "attributes must be a list or NULL".to_string(),
            });
        }

        let names =
            crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            std::panic::panic_any(RError {
                message: "attributes must be named".to_string(),
            });
        }

        for i in (0..XLENGTH(value)).rev() {
            let name_elt = STRING_ELT(names, i);
            if name_elt.is_null() || name_elt == crate::sexp::globals::R_NaString() {
                continue;
            }
            let name = CHAR(name_elt);
            if name.is_null() || CStr::from_ptr(name).to_bytes().is_empty() {
                continue;
            }
            crate::sexp::attrib_core::setAttrib(x, Rf_install(name), VECTOR_ELT(value, i));
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `comment(x)` — get the comment attribute.
pub unsafe fn do_comment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::getAttrib(x, comment_symbol())
    }
}

/// R's `comment(x) <- value` — set the comment attribute.
pub unsafe fn do_comment_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(x, comment_symbol(), value);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

pub unsafe fn comment_symbol() -> SEXP {
    unsafe { Rf_install(CString::new("comment").unwrap_or_default().as_ptr()) }
}

/// R's namespace lookup operators, `pkg::name` and `pkg:::name`.
pub unsafe fn do_namespace_get(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package = CAR(args);
        let name = CAR(CDR(args));
        if package.is_null() || name.is_null() || package == R_NilValue() || name == R_NilValue() {
            return R_NilValue();
        }

        let package_name = if TYPEOF(package) == SEXPTYPE::SYMSXP {
            let pname = PRINTNAME(package);
            if pname.is_null() {
                String::new()
            } else {
                CStr::from_ptr(CHAR(pname)).to_string_lossy().into_owned()
            }
        } else {
            elt_to_string(package, 0)
        };

        if TYPEOF(name) != SEXPTYPE::SYMSXP {
            std::panic::panic_any(RError {
                message: "namespace lookup requires a name".to_string(),
            });
        }
        let lookup_name = symbol_name(name).unwrap_or_default();

        if package_name == "tools" {
            if lookup_name == "langElts" {
                let values = crate::sexp::init::LANGUAGE_ELEMENTS;
                let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                let _result_guard = protect(result);
                for (i, value) in values.iter().enumerate() {
                    let c_value =
                        CString::new(*value).expect("static language element has no NUL byte");
                    SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(c_value.as_ptr()));
                }
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
                return result;
            }
            std::panic::panic_any(RError {
                message: format!("object '{lookup_name}' not found in tools namespace"),
            });
        }

        if package_name != "base" {
            let namespace = match load_package_namespace_by_name(&package_name) {
                Ok(env) => env,
                Err(message) => {
                    std::panic::panic_any(RError { message });
                }
            };
            let private_lookup = symbol_name(CAR(call)).as_deref() == Some(":::")
                || crate::eval::builtin::PRIMNAME(op) == ":::";
            if !private_lookup {
                let package_path = find_package_path(&package_name);
                let directives = read_namespace_directives(Path::new(&package_path))
                    .ok()
                    .flatten();
                let exports = namespace_exports(directives.as_ref(), namespace);
                if !exports.iter().any(|export| export == &lookup_name) {
                    std::panic::panic_any(RError {
                        message: format!(
                            "'{lookup_name}' is not an exported object from namespace '{package_name}'"
                        ),
                    });
                }
            }
            let value = crate::sexp::envir::R_findVarInFrame(namespace, name);
            if value == crate::sexp::globals::R_UnboundValue() {
                std::panic::panic_any(RError {
                    message: format!(
                        "object '{lookup_name}' not found in namespace '{package_name}'"
                    ),
                });
            }
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
            return value;
        }

        let value = crate::sexp::envir::R_findVar(name, crate::sexp::globals::R_BaseEnv());
        if value == crate::sexp::globals::R_UnboundValue() {
            std::panic::panic_any(RError {
                message: format!("object '{lookup_name}' not found in base namespace"),
            });
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        value
    }
}

/// R's `attributes(x)` — return attributes as a named list.
pub unsafe fn do_attributes(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let attrs = ATTRIB(x);
        if attrs.is_null() || attrs == R_NilValue() {
            return R_NilValue();
        }

        let mut count = 0;
        let mut current = attrs;
        while !current.is_null() && current != R_NilValue() {
            count += 1;
            current = CDR(current);
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, count);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, count);
        if names.is_null() {
            return R_NilValue();
        }
        let _names_guard = protect(names);

        current = attrs;
        let mut i = 0;
        while !current.is_null() && current != R_NilValue() {
            SET_VECTOR_ELT(result, i, CAR(current));
            let name = tag_name(current).unwrap_or_default();
            SET_STRING_ELT(
                names,
                i,
                Rf_mkChar(CString::new(name).unwrap_or_default().as_ptr()),
            );
            i += 1;
            current = CDR(current);
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}

pub fn structure_attr_name(name: &str) -> &str {
    match name {
        ".Dim" => "dim",
        ".Dimnames" => "dimnames",
        ".Names" => "names",
        ".Tsp" => "tsp",
        ".Label" => "levels",
        other => other,
    }
}

/// R's `structure(.Data, ...)` — attach attributes to an object.
///
/// Mirrors src/library/base/R/structure.R: the historical dotted attribute
/// names `.Dim`, `.Dimnames`, `.Names`, `.Tsp` and `.Label` are renamed to
/// `dim`, `dimnames`, `names`, `tsp` and `levels` before being attached, and
/// the rename emits the upstream deprecation warning listing every renamed
/// name in call order (`structure(.OBJSXP, dim=)` construct side; the deparse
/// side landed separately with SyncErrDep).
pub unsafe fn do_structure(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        const SPECIALS: [&str; 5] = [".Dim", ".Dimnames", ".Names", ".Tsp", ".Label"];
        const REPLACEMENTS: [&str; 5] = ["dim", "dimnames", "names", "tsp", "levels"];

        let mut renamed: Vec<(String, String)> = Vec::new();

        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = tag_name(current) {
                let attr_name = structure_attr_name(&name);
                if let Some(slot) = SPECIALS.iter().position(|special| *special == name) {
                    renamed.push((name.clone(), REPLACEMENTS[slot].to_string()));
                }
                crate::sexp::attrib_core::setAttrib(
                    x,
                    Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
                    CAR(current),
                );
            }
            current = CDR(current);
        }

        if !renamed.is_empty() {
            // pc <- function(nms) paste0(sQuote(nms), collapse = ", ") —
            // sQuote renders ASCII quotes in the C locale the gates run in.
            let pc = |names: &[&str]| {
                names
                    .iter()
                    .map(|name| format!("'{name}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let msg = format!(
                "Replacing special names {} is deprecated; use {} instead.",
                pc(&renamed
                    .iter()
                    .map(|(from, _)| from.as_str())
                    .collect::<Vec<_>>()),
                pc(&renamed
                    .iter()
                    .map(|(_, to)| to.as_str())
                    .collect::<Vec<_>>()),
            );
            let c_msg = CString::new(msg).unwrap_or_default();
            // .Deprecated() warns with the structure() call so the deferred
            // print renders "In structure(...) :".
            crate::mainutils::errors::Rf_warningcall1(call, c_msg.as_ptr());
        }

        x
    }
}

/// R's `attr(x, which) <- value` — set arbitrary attribute by name.
pub unsafe fn do_setattr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let which = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
            return R_NilValue();
        }
        let attr_name = elt_to_string(which, 0);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}
