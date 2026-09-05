//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
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
    ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::*;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum DatetimeVectorClass {
    Date,
    Posixct,
}

pub(crate) unsafe fn leading_datetime_class(args: SEXP) -> Option<(DatetimeVectorClass, SEXP)> {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return None;
        }
        let first = CAR(args);
        if sexp_has_class(first, "POSIXct") {
            Some((DatetimeVectorClass::Posixct, first))
        } else if sexp_has_class(first, "Date") {
            Some((DatetimeVectorClass::Date, first))
        } else {
            None
        }
    }
}

pub(crate) unsafe fn posixct_tzone_string(source: SEXP) -> String {
    unsafe {
        let tzone = crate::sexp::attrib_core::getAttrib(source, Rf_install(c"tzone".as_ptr()));
        if tzone.is_null() || tzone == R_NilValue() || TYPEOF(tzone) != SEXPTYPE::STRSXP {
            return "UTC".to_string();
        }
        if XLENGTH(tzone) == 0 {
            return "UTC".to_string();
        }
        let value = STRING_ELT(tzone, 0);
        if value.is_null() || value == crate::sexp::globals::R_NaString() {
            return "UTC".to_string();
        }
        CStr::from_ptr(CHAR(value))
            .to_str()
            .unwrap_or("UTC")
            .to_string()
    }
}

pub(crate) unsafe fn set_datetime_class_from(
    result: SEXP,
    source: SEXP,
    class: DatetimeVectorClass,
) {
    unsafe {
        match class {
            DatetimeVectorClass::Date => set_single_class(result, "Date"),
            DatetimeVectorClass::Posixct => {
                set_posixct_class(result, &posixct_tzone_string(source))
            }
        }
    }
}

pub(crate) unsafe fn integer_or_logical_elt(value: SEXP, index: c_int) -> c_int {
    unsafe {
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => INTEGER_ELT(value, index),
            t if t == SEXPTYPE::LGLSXP => LOGICAL_ELT(value, index),
            _ => NA_INTEGER,
        }
    }
}

pub unsafe fn do_cache_class(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            std::panic::panic_any(RError {
                message: "invalid class argument to internal .class_cache".to_string(),
            });
        }

        let class = CAR(args);
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            std::panic::panic_any(RError {
                message: "invalid class argument to internal .class_cache".to_string(),
            });
        }

        CADR(args)
    }
}

pub unsafe fn do_xtfrm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        let x = CAR(args);
        match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP || t == SEXPTYPE::LGLSXP => x,
            t if t == SEXPTYPE::STRSXP => {
                let n = XLENGTH(x);
                let out = Rf_allocVector3(SEXPTYPE::INTSXP, n);
                let _out_guard = protect(out);

                let mut values = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let elt = STRING_ELT(x, i);
                    let value = if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                        None
                    } else {
                        Some(CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned())
                    };
                    values.push(value);
                }

                let mut sorted: Vec<String> = values.iter().filter_map(Clone::clone).collect();
                sorted.sort();
                sorted.dedup();

                let ranks: BTreeMap<String, c_int> = sorted
                    .into_iter()
                    .enumerate()
                    .map(|(idx, value)| (value, (idx + 1) as c_int))
                    .collect();

                let data = INTEGER(out);
                for (i, value) in values.into_iter().enumerate() {
                    *data.add(i) = value
                        .and_then(|value| ranks.get(&value).copied())
                        .unwrap_or(NA_INTEGER);
                }
                out
            }
            _ => x,
        }
    }
}

/// The `@` operator (main/attrib.c `do_AT`).
///
/// Upstream dispatches an internal generic for S3 objects first; with no
/// `@` method registered (the only case in this port) it falls through to
/// the stock checks: only S4 objects (or `.Data`) may be slot-extracted,
/// otherwise "no applicable method for `@` ..." with the object's class.
pub unsafe fn do_at(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            return R_NilValue();
        }
        let object = crate::eval::eval::Rf_eval(CAR(args), rho);
        let _object_guard = protect(object);
        let nlist = CADR(args);

        // do_AT name check: symbol or non-NA scalar string.
        let name_ok = !nlist.is_null()
            && (TYPEOF(nlist) == SEXPTYPE::SYMSXP
                || (TYPEOF(nlist) == SEXPTYPE::STRSXP
                    && LENGTH(nlist) == 1
                    && STRING_ELT(nlist, 0) != crate::sexp::globals::R_NaString()));
        if !name_ok {
            let msg = "invalid type or length for slot name".to_string();
            std::panic::panic_any(RError { message: msg });
        }

        // Only `.Data` bypasses the S4 requirement; anything else on a
        // non-S4 object is upstream's dispatch failure message.
        if crate::mainutils::essentials::s4::name_of(nlist).as_deref() != Some(".Data")
            && crate::mainutils::coerce::IS_S4_OBJECT(object) == crate::sexp::ffi::FALSE
        {
            let class_str = crate::mainutils::essentials::s4::first_class_display(object);
            let msg = format!(
                "no applicable method for `@` applied to an object of class \"{}\"",
                class_str
            );
            std::panic::panic_any(RError { message: msg });
        }

        let _ = call;
        let _ = op;
        crate::mainutils::essentials::s4::R_do_slot(object, nlist)
    }
}

pub unsafe fn do_at_set(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let object = if args.is_null() || args == R_NilValue() {
            R_NilValue()
        } else {
            CAR(args)
        };
        let _ = do_set_slot(call, op, args, rho);
        object
    }
}

pub(crate) unsafe fn replacement_name(arg: SEXP) -> String {
    unsafe {
        if arg.is_null() || arg == R_NilValue() {
            String::new()
        } else if TYPEOF(arg) == SEXPTYPE::SYMSXP {
            elt_to_string(arg, 0)
        } else {
            elt_to_string(arg, 0)
        }
    }
}

pub unsafe fn do_dollar_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            return R_NilValue();
        }

        let object = CAR(args);
        let field = replacement_name(CAR(CDR(args)));
        let value = CAR(CDR(CDR(args)));
        if object.is_null() || object == R_NilValue() || field.is_empty() {
            return object;
        }
        if TYPEOF(object) != SEXPTYPE::VECSXP {
            return object;
        }

        let names_sym = crate::sexp::attrib_core::R_NamesSymbol();
        let names = crate::sexp::attrib_core::getAttrib(object, names_sym);
        let n = XLENGTH(object);

        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let name = STRING_ELT(names, i);
                if !name.is_null() && CStr::from_ptr(CHAR(name)).to_string_lossy() == field {
                    SET_VECTOR_ELT(object, i, value);
                    return object;
                }
            }
        }

        let out = Rf_allocVector3(SEXPTYPE::VECSXP, n + 1);
        let _out_guard = protect(out);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, n + 1);
        let _names_guard = protect(out_names);
        let blank = Rf_mkChar(c"".as_ptr());

        for i in 0..n {
            SET_VECTOR_ELT(out, i, VECTOR_ELT(object, i));
            let name = if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && i < XLENGTH(names)
            {
                STRING_ELT(names, i)
            } else {
                blank
            };
            SET_STRING_ELT(out_names, i, name);
        }

        SET_VECTOR_ELT(out, n, value);
        let field_c = CString::new(field).unwrap_or_default();
        SET_STRING_ELT(out_names, n, Rf_mkChar(field_c.as_ptr()));
        crate::sexp::attrib_core::setAttrib(out, names_sym, out_names);

        // Preserve the object's remaining attributes (class, row.names, dim,
        // ...): appending a column must not demote a data.frame to a plain
        // list. `key$type <- factor(...)` in whisker's getKeyInfo relied on
        // the frame keeping its class and rows across the assignment.
        let mut ap = ATTRIB(object);
        while !ap.is_null() && ap != R_NilValue() {
            let tag = TAG(ap);
            if !tag.is_null() && tag != R_NilValue() && tag != names_sym {
                crate::sexp::attrib_core::setAttrib(out, tag, CAR(ap));
            }
            ap = CDR(ap);
        }

        // A data.frame's rows must match the appended column: extend the
        // compact row names when the new column is longer, and recycle
        // shorter ones like upstream `$<-.data.frame` -> `[[<-`.
        if sexp_has_class(out, "data.frame") {
            let rownames =
                crate::sexp::attrib_core::getAttrib(out, crate::sexp::attrib_core::R_RowNamesSymbol());
            let rows: R_xlen_t = if rownames.is_null() || rownames == R_NilValue() {
                0
            } else if TYPEOF(rownames) == SEXPTYPE::INTSXP
                && XLENGTH(rownames) == 2
                && *INTEGER(rownames) == NA_INTEGER
            {
                -(*INTEGER(rownames).add(1) as R_xlen_t)
            } else {
                XLENGTH(rownames)
            };
            let t = TYPEOF(value);
            let value_len: R_xlen_t = if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
                let mut len: R_xlen_t = 0;
                let mut p = value;
                while !p.is_null() && p != R_NilValue() {
                    len += 1;
                    p = CDR(p);
                }
                len
            } else if t == SEXPTYPE::VECSXP
                || t == SEXPTYPE::EXPRSXP
                || t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::CPLXSXP
                || t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP
            {
                XLENGTH(value)
            } else {
                1
            };
            let new_rows = if rows == 0 {
                value_len
            } else if value_len == rows || value_len == 1 || value_len == 0 {
                rows
            } else if value_len > rows && value_len % rows == 0 {
                value_len
            } else if rows % value_len == 0 {
                rows
            } else {
                base_error(format!(
                    "replacement has {value_len} rows, data has {rows}"
                ))
            };
            if new_rows != rows {
                crate::mainutils::essentials::functional::set_compact_row_names(out, new_rows);
            }
        }
        out
    }
}

pub(crate) unsafe fn datetime_c_value(
    source: SEXP,
    index: R_xlen_t,
    class: DatetimeVectorClass,
) -> f64 {
    unsafe {
        match class {
            DatetimeVectorClass::Date => {
                if TYPEOF(source) == SEXPTYPE::STRSXP {
                    let value = STRING_ELT(source, index);
                    if value.is_null() || value == crate::sexp::globals::R_NaString() {
                        return NA_REAL;
                    }
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    return parse_iso_date_days(text).unwrap_or_else(|| {
                        base_error("character string is not in a standard unambiguous format");
                    });
                }
                let value = real_elt_or_default(source, index, NA_REAL);
                if sexp_has_class(source, "POSIXct") && value.to_bits() != R_NA_BIT_PATTERN {
                    (value / 86_400.0).floor()
                } else {
                    value
                }
            }
            DatetimeVectorClass::Posixct => {
                if TYPEOF(source) == SEXPTYPE::STRSXP {
                    let value = STRING_ELT(source, index);
                    if value.is_null() || value == crate::sexp::globals::R_NaString() {
                        return NA_REAL;
                    }
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    return parse_iso_datetime_seconds(text).unwrap_or_else(|| {
                        base_error("character string is not in a standard unambiguous format");
                    });
                }
                let value = real_elt_or_default(source, index, NA_REAL);
                if sexp_has_class(source, "Date") && value.to_bits() != R_NA_BIT_PATTERN {
                    value.floor() * 86_400.0
                } else {
                    value
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) unsafe fn string_vector(values: &[String]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in values.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

pub(crate) unsafe fn static_string_vector(values: &[&str]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in values.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*value).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

pub(crate) unsafe fn optional_string_vector(values: &[Option<String>]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in values.iter().enumerate() {
            let charsxp = match value {
                Some(value) => Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
                None => crate::sexp::globals::R_NaString(),
            };
            SET_STRING_ELT(result, i as R_xlen_t, charsxp);
        }
        result
    }
}

pub(crate) unsafe fn named_string_vector(values: &[String], names: &[String]) -> SEXP {
    unsafe {
        let result = string_vector(values);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        set_string_names(result, names);
        result
    }
}

pub(crate) unsafe fn named_string_list(items: impl IntoIterator<Item = (String, String)>) -> SEXP {
    unsafe {
        let items = items.into_iter().collect::<Vec<_>>();
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, items.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, items.len() as R_xlen_t);
        if names.is_null() {
            return R_NilValue();
        }
        let _names_guard = protect(names);

        for (i, (name, value)) in items.iter().enumerate() {
            let value_vec = string_vector(std::slice::from_ref(value));
            SET_VECTOR_ELT(result, i as R_xlen_t, value_vec);
            SET_STRING_ELT(
                names,
                i as R_xlen_t,
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}

/// Try to find a package by name in this session's configured library paths.
pub(crate) fn find_package_path(package: &str) -> String {
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.path_policy
            .find_package_path(package)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

pub(crate) fn package_description_fields(
    package: &str,
) -> Result<BTreeMap<String, String>, String> {
    if package.is_empty() || package == "NA" {
        return Err("invalid package name".to_string());
    }

    let package_path = find_package_path(package);
    if package_path.is_empty() {
        return Err(format!("there is no package called '{}'", package));
    }

    let description = Path::new(&package_path).join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content))
}

pub(crate) unsafe fn load_package_namespace_by_name(package: &str) -> Result<SEXP, String> {
    unsafe {
        if package.is_empty() || package == "NA" {
            return Err("invalid package name".to_string());
        }

        let package_path = find_package_path(package);
        if package_path.is_empty() {
            return Err(format!("there is no package called '{}'", package));
        }

        let package_dir = Path::new(&package_path);
        let description = package_dir.join("DESCRIPTION");
        if package_needs_compilation(&description)? {
            return Err(format!(
                "package '{}' declares NeedsCompilation: yes; this pure-R Android runtime does not load compiled package code",
                package
            ));
        }

        let mut loading = vec![package.to_string()];
        let (env, _) = load_package_namespace(package, package_dir, &mut loading)?;
        Ok(env)
    }
}

pub(crate) const INSTALLED_PACKAGE_COLUMNS: [&str; 16] = [
    "Package",
    "LibPath",
    "Version",
    "Priority",
    "Depends",
    "Imports",
    "LinkingTo",
    "Suggests",
    "Enhances",
    "License",
    "License_is_FOSS",
    "License_restricts_use",
    "OS_type",
    "MD5sum",
    "NeedsCompilation",
    "Built",
];

#[derive(Debug)]
pub(crate) struct InstalledPackageRow {
    package: String,
    library_path: String,
    fields: BTreeMap<String, String>,
}

impl InstalledPackageRow {
    fn value_for(&self, column: &str) -> String {
        match column {
            "Package" => self.package.clone(),
            "LibPath" => self.library_path.clone(),
            _ => self.fields.get(column).cloned().unwrap_or_default(),
        }
    }
}

pub(crate) fn installed_package_rows() -> Vec<InstalledPackageRow> {
    let library_paths = crate::sexp::instance::with_required_current_instance(|inst| {
        inst.path_policy.library_paths().to_vec()
    });
    let mut packages = Vec::<InstalledPackageRow>::new();
    let mut seen = Vec::<String>::new();

    for library_path in library_paths {
        let Ok(entries) = std::fs::read_dir(&library_path) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let package_dir = entry.path();
            let description = package_dir.join("DESCRIPTION");
            if !package_dir.is_dir() || !description.is_file() {
                continue;
            }
            let Some(fallback_name) = package_dir.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(content) = std::fs::read_to_string(&description) else {
                continue;
            };
            let fields = description_fields(&content);
            let package = fields
                .get("Package")
                .cloned()
                .unwrap_or_else(|| fallback_name.to_string());
            if package.is_empty() || seen.contains(&package) {
                continue;
            }
            seen.push(package.clone());
            packages.push(InstalledPackageRow {
                package,
                library_path: library_path.to_string_lossy().into_owned(),
                fields,
            });
        }
    }

    packages.sort_by(|left, right| left.package.cmp(&right.package));
    packages
}

pub(crate) unsafe fn installed_packages_matrix(packages: &[InstalledPackageRow]) -> SEXP {
    unsafe {
        let nrow = packages.len();
        let ncol = INSTALLED_PACKAGE_COLUMNS.len();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, (nrow * ncol) as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (col_idx, column) in INSTALLED_PACKAGE_COLUMNS.iter().enumerate() {
            for (row_idx, package) in packages.iter().enumerate() {
                let value = package.value_for(column);
                SET_STRING_ELT(
                    result,
                    (col_idx * nrow + row_idx) as R_xlen_t,
                    Rf_mkChar(CString::new(value).unwrap_or_default().as_ptr()),
                );
            }
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if dim.is_null() {
            return R_NilValue();
        }
        let _dim_guard = protect(dim);
        *INTEGER(dim).add(0) = nrow as c_int;
        *INTEGER(dim).add(1) = ncol as c_int;
        crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);

        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if dimnames.is_null() {
            return R_NilValue();
        }
        let _dimnames_guard = protect(dimnames);
        let row_name_values = packages
            .iter()
            .map(|package| package.package.clone())
            .collect::<Vec<_>>();
        let row_names = string_vector(&row_name_values);
        if row_names.is_null() {
            return R_NilValue();
        }
        let _row_names_guard = protect(row_names);

        let col_name_values = INSTALLED_PACKAGE_COLUMNS
            .iter()
            .map(|column| column.to_string())
            .collect::<Vec<_>>();
        let col_names = string_vector(&col_name_values);
        if col_names.is_null() {
            return R_NilValue();
        }
        let _col_names_guard = protect(col_names);

        SET_VECTOR_ELT(dimnames, 0, row_names);
        SET_VECTOR_ELT(dimnames, 1, col_names);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dimnames,
        );

        result
    }
}

pub(crate) fn package_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

pub(crate) unsafe fn package_name_symbol() -> SEXP {
    unsafe { Rf_install(c".packageName".as_ptr()) }
}

pub(crate) unsafe fn name_symbol() -> SEXP {
    unsafe { Rf_install(c"name".as_ptr()) }
}

pub(crate) unsafe fn namespace_env_symbol() -> SEXP {
    unsafe { Rf_install(c".namespaceEnv".as_ptr()) }
}

pub(crate) unsafe fn lazy_data_names_symbol() -> SEXP {
    unsafe { Rf_install(c".lazyDataNames".as_ptr()) }
}

pub(crate) unsafe fn package_name_binding(env: SEXP) -> Option<String> {
    unsafe {
        let value = crate::sexp::envir::R_findVarInFrame(env, package_name_symbol());
        if value.is_null()
            || value == R_NilValue()
            || value == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(value) != SEXPTYPE::STRSXP
            || XLENGTH(value) < 1
        {
            return None;
        }
        Some(elt_to_string(value, 0))
    }
}

pub(crate) unsafe fn package_attached(package: &str) -> bool {
    unsafe {
        let mut env = crate::sexp::accessors::ENCLOS(crate::sexp::globals::R_GlobalEnv());
        let base = crate::sexp::globals::R_BaseEnv();
        while !env.is_null() && env != base {
            if package_name_binding(env).as_deref() == Some(package) {
                return true;
            }
            env = crate::sexp::accessors::ENCLOS(env);
        }
        false
    }
}

pub(crate) unsafe fn load_pure_r_package(package: &str, package_dir: &Path) -> Result<(), String> {
    let mut loading = Vec::<String>::new();
    unsafe { load_pure_r_package_recursive(package, package_dir, &mut loading) }
}

pub(crate) unsafe fn load_pure_r_package_recursive(
    package: &str,
    package_dir: &Path,
    loading_packages: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        let description = package_dir.join("DESCRIPTION");
        if !description.is_file() {
            return Err(format!("package '{}' has no DESCRIPTION", package));
        }
        if loading_packages.iter().any(|entry| entry == package) {
            return Err(format!(
                "cyclic package dependency while loading '{}': {} -> {}",
                package,
                loading_packages.join(" -> "),
                package
            ));
        }
        if package_needs_compilation(&description)? {
            return Err(format!(
                "package '{}' declares NeedsCompilation: yes; this pure-R Android runtime does not load compiled package code",
                package
            ));
        }

        loading_packages.push(package.to_string());
        let result = (|| {
            load_package_dependencies(package, package_dir, loading_packages)?;

            let mut loading = vec![package.to_string()];
            let (package_env, namespace) =
                load_package_namespace(package, package_dir, &mut loading)?;
            let _package_env_guard = crate::sexp::protect::protect(package_env);

            let attach_env = make_package_attach_env(package, namespace.as_ref(), package_env)?;
            attach_package_env(attach_env);
            Ok(())
        })();
        loading_packages.pop();
        result
    }
}

pub(crate) unsafe fn load_package_dependencies(
    package: &str,
    package_dir: &Path,
    loading_packages: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        for dependency in package_depends_names(package_dir)? {
            if is_builtin_package_dependency(&dependency) || package_attached(&dependency) {
                continue;
            }
            let dependency_path = find_package_path(&dependency);
            if dependency_path.is_empty() {
                return Err(format!(
                    "package '{}' depends on missing package '{}'",
                    package, dependency
                ));
            }
            load_pure_r_package_recursive(
                &dependency,
                Path::new(&dependency_path),
                loading_packages,
            )?;
        }
        Ok(())
    }
}

pub(crate) fn package_needs_compilation(description: &Path) -> Result<bool, String> {
    let content = std::fs::read_to_string(description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("NeedsCompilation")
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
        }))
}

pub(crate) fn package_declares_lazy_data(package_dir: &Path) -> Result<bool, String> {
    let description = package_dir.join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("LazyData")
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
        }))
}

pub(crate) fn description_fields(description: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::<String, String>::new();
    let mut current_key: Option<String> = None;

    for line in description.lines() {
        if line.trim().is_empty() {
            break;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(key) = current_key.as_ref()
                && let Some(value) = fields.get_mut(key)
            {
                if !value.is_empty() {
                    value.push('\n');
                }
                value.push_str(line.trim());
            }
            continue;
        }

        let Some((field, value)) = line.split_once(':') else {
            current_key = None;
            continue;
        };
        let field = field.trim();
        if field.is_empty() || field.chars().any(char::is_whitespace) {
            current_key = None;
            continue;
        }
        let field = field.to_string();
        fields.insert(field.clone(), value.trim().to_string());
        current_key = Some(field);
    }

    fields
}

pub(crate) fn description_file_list(value: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut chars = value.char_indices().peekable();

    while let Some((_, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        let mut file = String::new();
        if ch == '\'' || ch == '"' {
            let quote = ch;
            chars.next();
            let mut escaped = false;
            for (_, next) in chars.by_ref() {
                if escaped {
                    file.push(next);
                    escaped = false;
                } else if next == '\\' {
                    escaped = true;
                } else if next == quote {
                    break;
                } else {
                    file.push(next);
                }
            }
        } else {
            while let Some((_, next)) = chars.peek().copied() {
                if next.is_whitespace() {
                    break;
                }
                file.push(next);
                chars.next();
            }
        }

        if !file.trim().is_empty() {
            push_unique(&mut files, file.trim().to_string());
        }
    }

    files
}

pub(crate) fn description_package_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(|entry| {
            let name = entry
                .split_once('(')
                .map(|(name, _)| name)
                .unwrap_or(entry)
                .trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .fold(Vec::<String>::new(), |mut names, name| {
            push_unique(&mut names, name);
            names
        })
}

pub(crate) fn package_depends_names(package_dir: &Path) -> Result<Vec<String>, String> {
    let description = package_dir.join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("Depends")
        .map(|value| description_package_list(value))
        .unwrap_or_default())
}
pub(crate) fn is_builtin_package_dependency(package: &str) -> bool {
    matches!(
        package,
        "R" | "base"
            | "compiler"
            | "datasets"
            | "grDevices"
            | "graphics"
            | "grid"
            | "methods"
            | "parallel"
            | "splines"
            | "stats"
            | "stats4"
            | "tcltk"
            | "tools"
            | "utils"
    )
}

pub(crate) unsafe fn load_package_namespace(
    package: &str,
    package_dir: &Path,
    loading: &mut Vec<String>,
) -> Result<(SEXP, Option<NamespaceDirectives>), String> {
    unsafe {
        if let Some(env) = cached_package_namespace(package, package_dir) {
            return Ok((env, read_namespace_directives(package_dir)?));
        }

        let package_env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        if package_env.is_null() {
            return Err(format!(
                "could not create namespace for package '{}'",
                package
            ));
        }
        let _package_env_guard = crate::sexp::protect::protect(package_env);

        define_package_metadata(package, package_env);
        reject_unsupported_internal_data(package, package_dir)?;
        reject_unsupported_lazyload_code(package, package_dir)?;
        // Register the in-flight namespace before sourcing its R files:
        // upstream loadNamespace records the namespace first, so package
        // code that calls `asNamespace(pkg)` mid-load (crayon's
        // `assign(style, make_style(style), envir = asNamespace("crayon"))`)
        // receives the environment under construction instead of triggering
        // a nested load of the same package.
        cache_package_namespace(package, package_dir, package_env);
        let populated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            populate_package_namespace(package, package_dir, package_env, loading)
        }));
        let namespace = match populated {
            Ok(result) => match result {
                Ok(namespace) => namespace,
                Err(message) => {
                    uncache_package_namespace(package);
                    return Err(message);
                }
            },
            Err(payload) => {
                // Sourced R code errors unwind as RSignal panics; drop the
                // half-built namespace so a later attempt reloads cleanly.
                uncache_package_namespace(package);
                std::panic::resume_unwind(payload);
            }
        };
        Ok((package_env, namespace))
    }
}

pub(crate) fn normalized_package_dir(package_dir: &Path) -> PathBuf {
    std::fs::canonicalize(package_dir).unwrap_or_else(|_| package_dir.to_path_buf())
}

pub(crate) fn cached_package_namespace(package: &str, package_dir: &Path) -> Option<SEXP> {
    let package_dir = normalized_package_dir(package_dir);
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.package_namespace_cache
            .get(package)
            .and_then(|(cached_dir, env)| (*cached_dir == package_dir).then_some(*env))
    })
}

pub(crate) fn cache_package_namespace(package: &str, package_dir: &Path, package_env: SEXP) {
    let package_dir = normalized_package_dir(package_dir);
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.package_namespace_cache
            .insert(package.to_string(), (package_dir, package_env));
    });
}

pub(crate) fn uncache_package_namespace(package: &str) {
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.package_namespace_cache.remove(package);
    });
}

pub(crate) fn package_arg_values(package_arg: SEXP) -> Vec<String> {
    unsafe {
        if package_arg.is_null() || package_arg == R_NilValue() || XLENGTH(package_arg) == 0 {
            return Vec::new();
        }
        (0..XLENGTH(package_arg))
            .map(|i| elt_to_string(package_arg, i))
            .filter(|package| !package.is_empty() && package != "NA")
            .collect()
    }
}

pub(crate) fn list_package_data_sets(packages: &[String]) -> Vec<String> {
    let mut names = Vec::<String>::new();
    for package_dir in data_package_dirs(packages) {
        let data_dir = package_dir.join("data");
        let Ok(entries) = std::fs::read_dir(data_dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
                && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
            {
                push_unique(&mut names, name.to_string());
            }
        }
    }
    names.sort();
    names
}

pub(crate) fn data_package_dirs(packages: &[String]) -> Vec<PathBuf> {
    crate::sexp::instance::with_required_current_instance(|inst| {
        if packages.is_empty() {
            return inst
                .path_policy
                .library_paths()
                .iter()
                .filter_map(|library| std::fs::read_dir(library).ok())
                .flat_map(|entries| entries.filter_map(Result::ok).map(|entry| entry.path()))
                .filter(|path| path.join("DESCRIPTION").is_file())
                .collect();
        }

        packages
            .iter()
            .filter_map(|package| inst.path_policy.find_package_path(package))
            .collect()
    })
}

pub(crate) unsafe fn load_package_data_set(
    topic: &str,
    packages: &[String],
    target_env: SEXP,
) -> Result<bool, String> {
    unsafe {
        let mut unsupported_data = None::<PathBuf>;
        for package_dir in data_package_dirs(packages) {
            let data_dir = package_dir.join("data");
            let source_file = data_dir.join(format!("{topic}.R"));
            if source_file.is_file() {
                source_r_file_into_env(&source_file, target_env)?;
                return Ok(true);
            }

            let unsupported = [
                data_dir.join(format!("{topic}.rda")),
                data_dir.join(format!("{topic}.RData")),
                data_dir.join("Rdata.rdb"),
                data_dir.join("Rdata.rdx"),
            ];
            if let Some(path) = unsupported.iter().find(|path| path.is_file()) {
                unsupported_data = Some(path.clone());
            }
        }
        if let Some(path) = unsupported_data {
            return Err(format!(
                "data set '{}' uses unsupported serialized/lazy data file {}; this pure-R Android runtime supports data/*.R only",
                topic,
                path.display()
            ));
        }
        Ok(false)
    }
}

pub(crate) unsafe fn source_r_file_into_env(file: &Path, env: SEXP) -> Result<(), String> {
    unsafe {
        let code = std::fs::read_to_string(file)
            .map_err(|err| format!("could not read {}: {err}", file.display()))?;
        let expr = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse(&code, arena).map_err(|err| err.to_string())
        })?;
        let expr = if expr.is_null() { R_NilValue() } else { expr };
        let _guard = crate::sexp::protect::protect(expr);
        let _ = crate::eval::eval::Rf_eval(expr, env);
        Ok(())
    }
}

pub(crate) unsafe fn define_package_metadata(package: &str, package_env: SEXP) {
    unsafe {
        let package_string = Rf_mkString(CString::new(package).unwrap_or_default().as_ptr());
        if !package_string.is_null() {
            crate::sexp::envir::defineVar(package_name_symbol(), package_string, package_env);
        }

        let search_name = format!("package:{package}");
        let search_string = Rf_mkString(CString::new(search_name).unwrap_or_default().as_ptr());
        if !search_string.is_null() {
            crate::sexp::attrib_core::setAttrib(package_env, name_symbol(), search_string);
        }
    }
}

pub(crate) fn reject_unsupported_internal_data(
    package: &str,
    package_dir: &Path,
) -> Result<(), String> {
    for path in [
        package_dir.join("R").join("sysdata.rda"),
        package_dir.join("R").join("sysdata.RData"),
        package_dir.join("R").join("sysdata.rdb"),
        package_dir.join("R").join("sysdata.rdx"),
    ] {
        if path.is_file() {
            return Err(format!(
                "package '{}' uses unsupported internal serialized data {}; this pure-R Android runtime supports R source files and data/*.R only",
                package,
                path.display()
            ));
        }
    }
    Ok(())
}

pub(crate) fn package_lazy_db_base(package_dir: &Path, package: &str) -> PathBuf {
    package_dir.join("R").join(package)
}

pub(crate) fn package_has_lazy_db(package_dir: &Path, package: &str) -> bool {
    let base = package_lazy_db_base(package_dir, package);
    base.with_extension("rdx").is_file() && base.with_extension("rdb").is_file()
}

pub(crate) fn reject_unsupported_lazyload_code(
    package: &str,
    package_dir: &Path,
) -> Result<(), String> {
    let r_dir = package_dir.join("R");
    if !r_dir.is_dir() {
        return Ok(());
    }

    let mut has_rdb = false;
    let mut has_rdx = false;
    let entries = std::fs::read_dir(&r_dir)
        .map_err(|err| format!("could not read R directory for package '{package}': {err}"))?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if ext.eq_ignore_ascii_case("rdb") {
            has_rdb = true;
        } else if ext.eq_ignore_ascii_case("rdx") {
            has_rdx = true;
        }
    }

    if has_rdb ^ has_rdx {
        let orphan = if has_rdb { "rdb" } else { "rdx" };
        return Err(format!(
            "package '{}' uses unsupported byte-compiled/lazyload R code {}; incomplete lazy-load database (missing .{orphan})",
            package,
            r_dir.display()
        ));
    }

    Ok(())
}

pub(crate) unsafe fn load_rds_file(path: &Path) -> Result<SEXP, String> {
    unsafe {
        let bytes = std::fs::read(path)
            .map_err(|err| format!("cannot read RDS file '{}': {err}", path.display()))?;
        let raw_vec = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t);
        if raw_vec.is_null() {
            return Err(format!(
                "allocation failed while reading '{}'",
                path.display()
            ));
        }
        let _raw_guard = protect(raw_vec);
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw_vec), bytes.len());
        }
        Ok(crate::mainutils::serialize::R_unserialize(
            raw_vec,
            R_NilValue(),
        ))
    }
}

pub(crate) unsafe fn lazy_load_db_fetch_value(
    key: SEXP,
    datafile: SEXP,
    compressed: SEXP,
) -> Result<SEXP, String> {
    unsafe {
        let args = Rf_cons(
            key,
            Rf_cons(
                datafile,
                Rf_cons(compressed, Rf_cons(R_NilValue(), R_NilValue())),
            ),
        );
        let value = crate::mainutils::serialize::do_lazyLoadDBfetch(
            R_NilValue(),
            R_NilValue(),
            args,
            R_NilValue(),
        );
        if value.is_null() {
            Err("lazy-load database fetch returned NULL".to_string())
        } else {
            Ok(value)
        }
    }
}

pub(crate) unsafe fn eager_lazy_load_package_db(
    filebase: &Path,
    envir: SEXP,
    skip: &[&str],
) -> Result<(), String> {
    unsafe { lazy_lazy_load_package_db(filebase, envir, skip) }
}

pub(crate) unsafe fn lazy_lazy_load_package_db(
    filebase: &Path,
    envir: SEXP,
    skip: &[&str],
) -> Result<(), String> {
    unsafe {
        if envir.is_null() || TYPEOF(envir) != SEXPTYPE::ENVSXP {
            return Err("lazy-load requires a package environment".to_string());
        }

        let rdx_path = filebase.with_extension("rdx");
        let rdb_path = filebase.with_extension("rdb");
        let map = load_rds_file(&rdx_path)?;
        let _map_guard = protect(map);

        let variables = list_element_by_name(map, "variables").ok_or_else(|| {
            format!(
                "lazy-load map '{}' is missing a variables table",
                rdx_path.display()
            )
        })?;
        if TYPEOF(variables) != SEXPTYPE::VECSXP {
            return Err(format!(
                "lazy-load map '{}' has an invalid variables table",
                rdx_path.display()
            ));
        }

        let compressed = list_element_by_name(map, "compressed").unwrap_or(Rf_ScalarInteger(0));
        let datafile = Rf_mkString(
            CString::new(rdb_path.to_string_lossy().into_owned())
                .unwrap_or_default()
                .as_ptr(),
        );
        if datafile.is_null() {
            return Err(format!(
                "could not create lazy-load data path for '{}'",
                rdb_path.display()
            ));
        }
        let _datafile_guard = protect(datafile);

        let names = crate::sexp::attrib_core::getAttrib(
            variables,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );

        let mut lazy_names = Vec::new();
        let mut lazy_keys = Vec::new();
        for index in 0..XLENGTH(variables) {
            let name =
                if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
                    string_at_or_empty(names, index)
                } else {
                    String::new()
                };
            if name.is_empty() || skip.iter().any(|candidate| *candidate == name) {
                continue;
            }
            lazy_names.push(name);
            lazy_keys.push(VECTOR_ELT(variables, index));
        }

        if lazy_names.is_empty() {
            return Ok(());
        }

        let fetch_sym = Rf_install(c"lazyLoadDBfetch".as_ptr());
        let template = Rf_cons(
            fetch_sym,
            Rf_cons(
                R_NilValue(),
                Rf_cons(
                    datafile,
                    Rf_cons(compressed, Rf_cons(R_NilValue(), R_NilValue())),
                ),
            ),
        );
        if !template.is_null() {
            (*template).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        let _template_guard = protect(template);

        for (index, name) in lazy_names.iter().enumerate() {
            let key = lazy_keys[index];
            let expr0 = crate::mainutils::duplicate::duplicate(template);
            let _expr0_guard = protect(expr0);
            SETCAR(CDR(expr0), key);
            let symbol = Rf_install(CString::new(name.clone()).unwrap_or_default().as_ptr());
            crate::sexp::envir::defineVar(
                symbol,
                crate::sexp::memory_ext::mkPROMISE(expr0, envir),
                envir,
            );
        }

        Ok(())
    }
}

pub(crate) unsafe fn source_package_r_files(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
) -> Result<(), String> {
    unsafe {
        let r_dir = package_dir.join("R");
        if !r_dir.is_dir() {
            return Ok(());
        }

        let description = std::fs::read_to_string(package_dir.join("DESCRIPTION"))
            .map(|content| description_fields(&content))
            .unwrap_or_default();
        let collate_files = description
            .get("Collate")
            .map(|value| description_file_list(value))
            .unwrap_or_default();

        let mut files = std::fs::read_dir(&r_dir)
            .map_err(|err| format!("could not read R directory for package '{package}': {err}"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
            })
            .collect::<Vec<_>>();
        files.sort();

        if !collate_files.is_empty() {
            let mut ordered = Vec::with_capacity(files.len());
            let mut collated_names = BTreeSet::<String>::new();
            for file_name in collate_files {
                if file_name.contains('/') || file_name.contains('\\') {
                    return Err(format!(
                        "package '{}' has unsupported Collate entry '{}'",
                        package, file_name
                    ));
                }
                let file = r_dir.join(&file_name);
                if !file.is_file() {
                    return Err(format!(
                        "package '{}' Collate entry '{}' does not exist",
                        package, file_name
                    ));
                }
                collated_names.insert(file_name);
                ordered.push(file);
            }
            for file in files {
                let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !collated_names.contains(name) {
                    ordered.push(file);
                }
            }
            files = ordered;
        }

        // Source with the package directory installed as the resolution
        // root for relative file paths: package code evaluated at install
        // time upstream (crayon's ansi-palette.R reads
        // "tools/ansi-palettes.txt") sees the package root, not the host
        // process CWD.
        let saved_package_dir = crate::sexp::instance::with_required_current_instance(|inst| {
            inst.loading_package_dir
                .replace(normalized_package_dir(package_dir))
        });
        let sourced: Result<(), String> =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for file in files {
                    source_r_file_into_env(&file, package_env)?;
                }
                Ok(())
            }))
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
        crate::sexp::instance::with_required_current_instance(|inst| {
            inst.loading_package_dir = saved_package_dir;
        });
        sourced?;

        if package_has_lazy_db(package_dir, package) {
            let filebase = package_lazy_db_base(package_dir, package);
            eager_lazy_load_package_db(&filebase, package_env, &[".__NAMESPACE__."])?;
        }

        Ok(())
    }
}

pub(crate) unsafe fn source_package_lazy_data(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
) -> Result<Vec<String>, String> {
    unsafe {
        if !package_declares_lazy_data(package_dir)? {
            return Ok(Vec::new());
        }

        let data_dir = package_dir.join("data");
        if !data_dir.is_dir() {
            return Ok(Vec::new());
        }

        let before = frame_binding_names(package_env, true)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut files = std::fs::read_dir(&data_dir)
            .map_err(|err| format!("could not read data directory for package '{package}': {err}"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
            })
            .collect::<Vec<_>>();
        files.sort();

        for file in files {
            source_r_file_into_env(&file, package_env)?;
        }

        let mut names = frame_binding_names(package_env, true)
            .into_iter()
            .filter(|name| !before.contains(name.as_str()))
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Ok(names)
    }
}

pub(crate) unsafe fn define_lazy_data_names(package_env: SEXP, names: &[String]) {
    unsafe {
        let values = string_vector(names);
        if !values.is_null() {
            crate::sexp::envir::defineVar(lazy_data_names_symbol(), values, package_env);
        }
    }
}

pub(crate) unsafe fn lazy_data_names_binding(package_env: SEXP) -> Vec<String> {
    unsafe {
        let value = crate::sexp::envir::R_findVarInFrame(package_env, lazy_data_names_symbol());
        if value.is_null()
            || value == R_NilValue()
            || value == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(value) != SEXPTYPE::STRSXP
        {
            return Vec::new();
        }

        (0..XLENGTH(value))
            .map(|i| elt_to_string(value, i))
            .filter(|name| !name.is_empty() && name != "NA")
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NamespaceDirectives {
    pub(crate) exports: Vec<String>,
    pub(crate) export_patterns: Vec<String>,
    pub(crate) imports: Vec<NamespaceImport>,
    pub(crate) s3_methods: Vec<S3MethodDirective>,
    pub(crate) native_libraries: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum NamespaceImport {
    All { package: String },
    From { package: String, names: Vec<String> },
}

#[derive(Clone, Debug)]
pub(crate) struct S3MethodDirective {
    generic: String,
    class: String,
    method: Option<String>,
}

pub(crate) unsafe fn populate_package_namespace(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
    loading: &mut Vec<String>,
) -> Result<Option<NamespaceDirectives>, String> {
    unsafe {
        let namespace = read_namespace_directives(package_dir)?;
        apply_description_depends(package, package_dir, package_env, loading)?;
        if let Some(directives) = namespace.as_ref() {
            reject_native_namespace_directives(package, directives)?;
            apply_namespace_imports(package, package_env, directives, loading)?;
        }
        source_package_r_files(package, package_dir, package_env)?;
        let lazy_data_names = source_package_lazy_data(package, package_dir, package_env)?;
        define_lazy_data_names(package_env, &lazy_data_names);
        if let Some(directives) = namespace.as_ref() {
            register_namespace_s3_methods(package, package_env, directives)?;
        }
        Ok(namespace)
    }
}

pub(crate) unsafe fn apply_description_depends(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
    loading: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        for dependency in package_depends_names(package_dir)? {
            if is_builtin_package_dependency(&dependency) {
                continue;
            }
            if loading.iter().any(|entry| entry == &dependency) {
                return Err(format!(
                    "package '{}' has cyclic Depends dependency involving '{}'",
                    package, dependency
                ));
            }

            let dependency_dir = find_package_path(&dependency);
            if dependency_dir.is_empty() {
                return Err(format!(
                    "package '{}' depends on missing package '{}'",
                    package, dependency
                ));
            }

            loading.push(dependency.clone());
            let import = NamespaceImport::All {
                package: dependency.clone(),
            };
            let result = import_namespace_bindings(
                package,
                package_env,
                Path::new(&dependency_dir),
                &import,
                loading,
            );
            loading.pop();
            result?;
        }
        Ok(())
    }
}
pub(crate) unsafe fn apply_namespace_imports(
    package: &str,
    package_env: SEXP,
    directives: &NamespaceDirectives,
    loading: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        for import in &directives.imports {
            let import_package = match import {
                NamespaceImport::All { package } | NamespaceImport::From { package, .. } => {
                    package.as_str()
                }
            };

            // Base-distribution packages (grDevices, methods, utils, ...)
            // are satisfied by the engine itself: like DESCRIPTION Depends,
            // skip them instead of demanding an installed copy.
            if is_builtin_package_dependency(import_package) {
                continue;
            }
            if loading.iter().any(|entry| entry == import_package) {
                return Err(format!(
                    "package '{}' has cyclic namespace import involving '{}'",
                    package, import_package
                ));
            }
            let import_dir = find_package_path(import_package);
            if import_dir.is_empty() {
                return Err(format!(
                    "package '{}' imports missing package '{}'",
                    package, import_package
                ));
            }

            loading.push(import_package.to_string());
            let result = import_namespace_bindings(
                package,
                package_env,
                Path::new(&import_dir),
                import,
                loading,
            );
            loading.pop();
            result?;
        }
        Ok(())
    }
}

pub(crate) unsafe fn import_namespace_bindings(
    package: &str,
    package_env: SEXP,
    import_dir: &Path,
    import: &NamespaceImport,
    loading: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        let import_package = match import {
            NamespaceImport::All { package } | NamespaceImport::From { package, .. } => {
                package.as_str()
            }
        };

        let (import_env, namespace) = load_package_namespace(import_package, import_dir, loading)?;
        let _import_guard = crate::sexp::protect::protect(import_env);

        let imported_names = match import {
            NamespaceImport::All { .. } => namespace_exports(namespace.as_ref(), import_env),
            NamespaceImport::From { names, .. } => names.clone(),
        };

        let mut missing = Vec::new();
        for name in imported_names {
            let Ok(symbol_name) = CString::new(name.as_str()) else {
                missing.push(name);
                continue;
            };
            let symbol = Rf_install(symbol_name.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(import_env, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                missing.push(name);
            } else {
                crate::sexp::envir::defineVar(symbol, value, package_env);
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "package '{}' imports undefined objects from '{}': {}",
                package,
                import_package,
                missing.join(", ")
            ))
        }
    }
}

pub(crate) unsafe fn make_package_attach_env(
    package: &str,
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Result<SEXP, String> {
    unsafe {
        let Some(directives) = namespace else {
            return Ok(package_env);
        };

        let attach_env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        if attach_env.is_null() {
            return Err(format!(
                "could not create attach environment for package '{}'",
                package
            ));
        }
        let _attach_guard = crate::sexp::protect::protect(attach_env);

        define_package_metadata(package, attach_env);
        crate::sexp::envir::defineVar(namespace_env_symbol(), package_env, attach_env);
        let s3_table = crate::sexp::envir::R_findVarInFrame(package_env, s3_methods_table_symbol());
        if !s3_table.is_null()
            && s3_table != crate::sexp::globals::R_UnboundValue()
            && TYPEOF(s3_table) == SEXPTYPE::ENVSXP
        {
            crate::sexp::envir::defineVar(s3_methods_table_symbol(), s3_table, attach_env);
        }

        let mut exports = namespace_exports(Some(directives), package_env);
        for name in lazy_data_names_binding(package_env) {
            push_unique(&mut exports, name);
        }
        // Crayon-style dynamic exports: top-level package code may
        // `assign(name, value, envir = asNamespace("pkg"))` AFTER the
        // files are sourced (its `sapply(names(builtin_styles), ...)`
        // block). Those bindings live in the namespace env but were never
        // declared as lexical assignments, so a strict frame check misses
        // them. Fall back to the full namespace-env lookup before
        // declaring an export missing.
        let mut missing = Vec::new();
        for export in exports {
            let Ok(symbol_name) = CString::new(export.as_str()) else {
                missing.push(export);
                continue;
            };
            let symbol = Rf_install(symbol_name.as_ptr());
            let mut value = crate::sexp::envir::R_findVarInFrame(package_env, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                value = crate::sexp::envir::R_findVar(symbol, package_env);
            }
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                missing.push(export);
            } else {
                crate::sexp::envir::defineVar(symbol, value, attach_env);
            }
        }
        if missing.is_empty() {
            Ok(attach_env)
        } else {
            Err(format!(
                "package '{}' has undefined exports: {}",
                package,
                missing.join(", ")
            ))
        }
    }
}

pub(crate) fn read_namespace_directives(
    package_dir: &Path,
) -> Result<Option<NamespaceDirectives>, String> {
    let namespace = package_dir.join("NAMESPACE");
    if !namespace.is_file() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&namespace)
        .map_err(|err| format!("could not read {}: {err}", namespace.display()))?;
    Ok(Some(parse_namespace_directives(&content)))
}

pub(crate) fn parse_namespace_directives(content: &str) -> NamespaceDirectives {
    let mut directives = NamespaceDirectives::default();
    let uncommented = strip_namespace_comments(content);
    for (directive, args) in parse_namespace_calls(&uncommented) {
        match directive.as_str() {
            "export" => {
                for name in split_namespace_args(&args)
                    .into_iter()
                    .filter_map(clean_namespace_name)
                {
                    push_unique(&mut directives.exports, name);
                }
            }
            "exportPattern" => {
                if let Some(pattern) = split_namespace_args(&args)
                    .first()
                    .and_then(|arg| clean_namespace_name(arg))
                {
                    push_unique(&mut directives.export_patterns, pattern);
                }
            }
            "import" => {
                if let Some(package) = split_namespace_args(&args)
                    .first()
                    .and_then(|arg| clean_namespace_name(arg))
                {
                    directives.imports.push(NamespaceImport::All { package });
                }
            }
            "importFrom" => {
                let parts = split_namespace_args(&args);
                let Some(package) = parts.first().and_then(|arg| clean_namespace_name(arg)) else {
                    continue;
                };
                let names = parts
                    .iter()
                    .skip(1)
                    .filter_map(|arg| clean_namespace_name(arg))
                    .collect::<Vec<_>>();
                if !names.is_empty() {
                    directives
                        .imports
                        .push(NamespaceImport::From { package, names });
                }
            }
            "S3method" => {
                let parts = split_namespace_args(&args);
                let Some(generic) = parts.first().and_then(|arg| clean_namespace_name(arg)) else {
                    continue;
                };
                let Some(class) = parts.get(1).and_then(|arg| clean_namespace_name(arg)) else {
                    continue;
                };
                let method = parts.get(2).and_then(|arg| clean_namespace_name(arg));
                directives.s3_methods.push(S3MethodDirective {
                    generic,
                    class,
                    method,
                });
            }
            "useDynLib" => {
                for name in split_namespace_args(&args)
                    .into_iter()
                    .filter_map(clean_namespace_name)
                {
                    push_unique(&mut directives.native_libraries, name);
                }
            }
            _ => {}
        }
    }
    directives
}

pub(crate) fn reject_native_namespace_directives(
    package: &str,
    directives: &NamespaceDirectives,
) -> Result<(), String> {
    if directives.native_libraries.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "package '{}' requires native libraries via useDynLib({}); this pure-R Android runtime does not load native package code",
            package,
            directives.native_libraries.join(", ")
        ))
    }
}

pub(crate) unsafe fn register_namespace_s3_methods(
    package: &str,
    package_env: SEXP,
    directives: &NamespaceDirectives,
) -> Result<(), String> {
    unsafe {
        for method in &directives.s3_methods {
            // Upstream loadNamespace strips a "pkg::" qualifier from the
            // generic when deriving the default method function name:
            // S3method(utils::.DollarNames, R6) registers the function
            // `.DollarNames.R6` for generic `.DollarNames`
            // (base/R/namespace.R: paste0(sub("^.*::", "", generic), ".", class)).
            let local_generic = method.generic.rsplit("::").next().unwrap_or("");
            if local_generic.is_empty() {
                continue;
            }
            let method_name = method
                .method
                .clone()
                .unwrap_or_else(|| format!("{}.{}", local_generic, method.class));
            let Ok(method_cstr) = CString::new(method_name.as_str()) else {
                return Err(format!(
                    "package '{}' has invalid S3 method name '{}'",
                    package, method_name
                ));
            };
            let method_sym = Rf_install(method_cstr.as_ptr());
            let method_value = crate::sexp::envir::R_findVarInFrame(package_env, method_sym);
            if method_value.is_null()
                || method_value == R_NilValue()
                || method_value == crate::sexp::globals::R_UnboundValue()
            {
                return Err(format!(
                    "package '{}' declares missing S3 method '{}'",
                    package, method_name
                ));
            }
            define_s3_method(package_env, local_generic, &method.class, method_value)?;
        }
        Ok(())
    }
}

pub(crate) fn strip_namespace_comments(content: &str) -> String {
    let mut stripped = String::with_capacity(content.len());
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;
    let mut in_comment = false;

    for ch in content.chars() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
                stripped.push('\n');
            }
            continue;
        }

        if in_string {
            stripped.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != '`' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                quote = ch;
                stripped.push(ch);
            }
            '#' => {
                in_comment = true;
            }
            _ => stripped.push(ch),
        }
    }

    stripped
}

pub(crate) fn parse_namespace_calls(content: &str) -> Vec<(String, String)> {
    let mut calls = Vec::new();
    let mut chars = content.char_indices().peekable();

    while let Some((start, ch)) = chars.next() {
        if !(ch.is_ascii_alphabetic() || ch == '.') {
            continue;
        }

        let mut end = start + ch.len_utf8();
        while let Some(&(idx, next)) = chars.peek() {
            if next.is_ascii_alphanumeric() || next == '.' {
                chars.next();
                end = idx + next.len_utf8();
            } else {
                break;
            }
        }

        let directive = content[start..end].trim();
        let mut scan = chars.clone();
        while let Some(&(_, whitespace)) = scan.peek() {
            if whitespace.is_whitespace() {
                scan.next();
            } else {
                break;
            }
        }
        let Some((open_idx, '(')) = scan.next() else {
            continue;
        };

        let Some((close_idx, args)) = find_namespace_call_args(content, open_idx) else {
            continue;
        };
        calls.push((directive.to_string(), args.to_string()));

        while let Some(&(idx, _)) = chars.peek() {
            if idx <= close_idx {
                chars.next();
            } else {
                break;
            }
        }
    }

    calls
}

pub(crate) fn find_namespace_call_args(content: &str, open_idx: usize) -> Option<(usize, &str)> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;

    for (idx, ch) in content[open_idx..].char_indices() {
        let absolute_idx = open_idx + idx;
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != '`' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                quote = ch;
            }
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((absolute_idx, &content[open_idx + 1..absolute_idx]));
                }
            }
            _ => {}
        }
    }

    None
}

pub(crate) fn split_namespace_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;

    for (idx, ch) in args.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != '`' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                quote = ch;
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(args[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(args[start..].trim());
    parts
}

pub(crate) fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

pub(crate) unsafe fn namespace_exports(
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Vec<String> {
    unsafe {
        let Some(directives) = namespace else {
            return frame_binding_names(package_env, false);
        };

        let mut exports = directives.exports.clone();
        for name in frame_binding_names(package_env, false) {
            if directives
                .export_patterns
                .iter()
                .any(|pattern| simple_namespace_pattern_matches(pattern, &name))
            {
                push_unique(&mut exports, name);
            }
        }
        exports
    }
}

pub(crate) unsafe fn frame_binding_names(env: SEXP, include_hidden: bool) -> Vec<String> {
    unsafe {
        let mut names = Vec::new();
        let mut frame = FRAME(env);
        while !frame.is_null() && frame != R_NilValue() {
            let value = CAR(frame);
            if value != crate::sexp::globals::R_UnboundValue()
                && let Some(name) = symbol_name(TAG(frame))
                && (include_hidden || !name.starts_with('.'))
                && name != ".packageName"
                && name != ".namespaceEnv"
            {
                names.push(name);
            }
            frame = CDR(frame);
        }
        names.sort();
        names.dedup();
        names
    }
}

pub(crate) fn simple_namespace_pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }

    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$');
    let mut body = pattern;
    if anchored_start {
        body = &body[1..];
    }
    if anchored_end && !body.is_empty() {
        body = &body[..body.len() - 1];
    }

    let mut literal = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let Some(escaped) = chars.next() else {
                return false;
            };
            literal.push(escaped);
        } else if ".^$|?*+()[]{}".contains(ch) {
            return false;
        } else {
            literal.push(ch);
        }
    }

    if anchored_start && anchored_end {
        name == literal
    } else if anchored_start {
        name.starts_with(&literal)
    } else if anchored_end {
        name.ends_with(&literal)
    } else {
        name.contains(&literal)
    }
}

pub(crate) fn clean_namespace_name(raw: &str) -> Option<String> {
    let name = raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

pub(crate) unsafe fn attach_package_env(package_env: SEXP) {
    unsafe {
        let global = crate::sexp::globals::R_GlobalEnv();
        let old_enclos = crate::sexp::accessors::ENCLOS(global);
        crate::sexp::accessors::SET_ENCLOS(global, package_env);
        crate::sexp::accessors::SET_ENCLOS(package_env, old_enclos);
    }
}

/// Try to find a demo file for a topic.
pub(crate) fn find_package_demo(topic: &str) -> String {
    let r_home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let paths = [
        format!("{}/library/*/demo/{}.R", r_home, topic),
        format!("/usr/local/lib/R/site-library/*/demo/{}.R", topic),
    ];
    // Simplified: check a few common locations
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return p.clone();
        }
    }
    String::new()
}

/// Try to find an example file for a topic.
pub(crate) fn find_package_example(topic: &str) -> String {
    let r_home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let paths = [
        format!("{}/library/*/R-ex/{}.R", r_home, topic),
        format!("/usr/local/lib/R/site-library/*/R-ex/{}.R", topic),
    ];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return p.clone();
        }
    }
    String::new()
}

/// Read a scalar real from a numeric SEXP, with default.
pub(crate) fn real_or_default(x: SEXP, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            default
        }
    }
}

pub(crate) fn base_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

pub(crate) unsafe fn tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return None;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            None
        } else {
            Some(std::ffi::CStr::from_ptr(chars).to_str().ok()?.to_string())
        }
    }
}

pub(crate) fn real_elt_or_default(x: SEXP, i: R_xlen_t, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return default;
        }
        let idx = i % n;
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x).add(idx as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER { default } else { v as f64 }
        } else {
            default
        }
    }
}

pub(crate) fn numeric_elt_as_count(x: SEXP, i: R_xlen_t) -> usize {
    let value = real_elt_or_default(x, i, 0.0);
    if value.is_finite() {
        (value as i64).max(0) as usize
    } else {
        0
    }
}

pub(crate) fn named_logical_arg(args: SEXP, name: &str) -> Option<bool> {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some(name) {
                let value = CAR(current);
                if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
                    return None;
                }
                let raw = if TYPEOF(value) == SEXPTYPE::LGLSXP || TYPEOF(value) == SEXPTYPE::INTSXP
                {
                    *INTEGER(value)
                } else if TYPEOF(value) == SEXPTYPE::REALSXP {
                    *REAL(value) as c_int
                } else {
                    return None;
                };
                return (raw != NA_INTEGER).then_some(raw != 0);
            }
            current = CDR(current);
        }
        None
    }
}

pub(crate) fn logical_arg_by_name_or_position(
    args: SEXP,
    name: &str,
    position: usize,
) -> Option<bool> {
    unsafe {
        let value = arg_by_name_or_position(args, &[name], position);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::LGLSXP || TYPEOF(value) == SEXPTYPE::INTSXP {
            *INTEGER(value)
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if ISNAN(value) {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        (raw != NA_INTEGER).then_some(raw != 0)
    }
}

pub(crate) fn integer_arg_by_name_or_position(
    args: SEXP,
    name: &str,
    position: usize,
) -> Option<c_int> {
    unsafe {
        let value = arg_by_name_or_position(args, &[name], position);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::LGLSXP {
            *INTEGER(value)
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan() {
                NA_INTEGER
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        Some(raw)
    }
}

pub(crate) fn arg_by_name_or_position(args: SEXP, names: &[&str], position: usize) -> SEXP {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(tag) = tag_name(current) {
                if names.iter().any(|name| tag == *name) {
                    return CAR(current);
                }
            }
            current = CDR(current);
        }

        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                if positional == position {
                    return CAR(current);
                }
                positional += 1;
            }
            current = CDR(current);
        }
        R_NilValue()
    }
}

pub(crate) fn is_string_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return false;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return false;
        }
        STRING_ELT(x, i % n) == crate::sexp::globals::R_NaString()
    }
}

pub(crate) fn element_coerces_to_character_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        let n = XLENGTH(x);
        if x.is_null() || x == R_NilValue() || n == 0 {
            return false;
        }
        let idx = i % n;
        let ty = TYPEOF(x);
        if ty == SEXPTYPE::STRSXP {
            is_string_na(x, idx)
        } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
            *INTEGER(x).add(idx as usize) == NA_INTEGER
        } else if ty == SEXPTYPE::REALSXP {
            let value = *REAL(x).add(idx as usize);
            value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        } else {
            false
        }
    }
}

pub(crate) fn string_contains(text: &str, pattern: &str, ignore_case: bool) -> bool {
    fixed_find(text, pattern, ignore_case).is_some()
}

pub(crate) fn fixed_find(
    text: &str,
    pattern: &str,
    ignore_case: bool,
) -> Option<crate::mainutils::grep::RegexMatch> {
    let hay = text.as_bytes();
    let needle = pattern.as_bytes();
    if needle.is_empty() {
        return Some(crate::mainutils::grep::RegexMatch { start: 0, end: 0 });
    }
    if hay.len() < needle.len() {
        return None;
    }
    for start in 0..=(hay.len() - needle.len()) {
        let matched = needle.iter().enumerate().all(|(idx, expected)| {
            let actual = hay[start + idx];
            if ignore_case {
                actual.eq_ignore_ascii_case(expected)
            } else {
                actual == *expected
            }
        });
        if matched {
            return Some(crate::mainutils::grep::RegexMatch {
                start,
                end: start + needle.len(),
            });
        }
    }
    None
}

pub(crate) fn grep_value_matches(
    text: &str,
    pattern: &str,
    ignore_case: bool,
    perl: bool,
    fixed: bool,
) -> bool {
    if fixed {
        string_contains(text, pattern, ignore_case)
    } else if perl {
        crate::mainutils::grep::perl_find(pattern, text, ignore_case).is_some()
    } else {
        crate::mainutils::grep::ere_is_match(pattern, text, ignore_case)
    }
}

pub(crate) fn grep_match_indices(
    x: SEXP,
    pattern: &str,
    ignore_case: bool,
    perl: bool,
    fixed: bool,
    invert: bool,
) -> Vec<R_xlen_t> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        let n = XLENGTH(x);
        let mut matches = Vec::new();
        for i in 0..n {
            if is_string_na(x, i) {
                if invert {
                    matches.push(i);
                }
                continue;
            }
            let matched =
                grep_value_matches(&elt_to_string(x, i), pattern, ignore_case, perl, fixed);
            if if invert { !matched } else { matched } {
                matches.push(i);
            }
        }
        matches
    }
}

pub(crate) fn environment_arg_or_default(
    args: SEXP,
    names: &[&str],
    position: usize,
    default: SEXP,
) -> SEXP {
    unsafe {
        let arg = arg_by_name_or_position(args, names, position);
        if !arg.is_null() && arg != R_NilValue() && TYPEOF(arg) == SEXPTYPE::ENVSXP {
            arg
        } else {
            default
        }
    }
}

pub(crate) fn copy_vector_elt(dst: SEXP, dst_idx: R_xlen_t, src: SEXP, src_idx: R_xlen_t) {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::REALSXP => {
                *REAL(dst).add(dst_idx as usize) = *REAL(src).add(src_idx as usize);
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(dst).add(dst_idx as usize) = *INTEGER(src).add(src_idx as usize);
            }
            t if t == SEXPTYPE::STRSXP => {
                SET_STRING_ELT(dst, dst_idx, STRING_ELT(src, src_idx));
            }
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(dst).add(dst_idx as usize) = *COMPLEX(src).add(src_idx as usize);
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                SET_VECTOR_ELT(dst, dst_idx, VECTOR_ELT(src, src_idx));
            }
            t if t == SEXPTYPE::RAWSXP => {
                *RAW(dst).add(dst_idx as usize) = *RAW(src).add(src_idx as usize);
            }
            _ => {}
        }
    }
}

pub(crate) fn map_path_strings(x: SEXP, f: fn(&str) -> String) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            if TYPEOF(x) == SEXPTYPE::STRSXP {
                let idx = if XLENGTH(x) == 0 { 0 } else { i % XLENGTH(x) };
                if STRING_ELT(x, idx) == crate::sexp::globals::R_NaString() {
                    SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                    continue;
                }
            }
            let value = f(&elt_to_string(x, i));
            SET_STRING_ELT(
                result,
                i,
                Rf_mkChar(CString::new(value).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

pub(crate) fn trim_trailing_separators(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() && !path.is_empty() {
        &path[..1]
    } else {
        trimmed
    }
}

pub(crate) fn r_basename(path: &str) -> String {
    let trimmed = trim_trailing_separators(path);
    if trimmed == "/" || trimmed == "\\" {
        return trimmed.to_string();
    }
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

pub(crate) fn r_dirname(path: &str) -> String {
    let trimmed = trim_trailing_separators(path);
    if trimmed == "/" || trimmed == "\\" {
        return trimmed.to_string();
    }
    match trimmed.rfind(['/', '\\']) {
        Some(0) => trimmed[..1].to_string(),
        Some(pos) => trimmed[..pos].to_string(),
        None => ".".to_string(),
    }
}

/// Convert an element of a vector to a String.
pub(crate) fn elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    unsafe {
        if x.is_null() {
            return "NULL".to_string();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        if n == 0 {
            return String::new();
        }
        let idx = i % n;

        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(idx as usize);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                "NA".to_string()
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::INTSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else if let Some(label) = factor_label_at(x, v) {
                label
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::LGLSXP {
            let v = *LOGICAL(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else if v == TRUE {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        } else if t == SEXPTYPE::STRSXP {
            let charsxp = crate::sexp::accessors::STRING_ELT(x, idx);
            if charsxp.is_null() {
                "NA".to_string()
            } else {
                let s = crate::sexp::accessors::CHAR(charsxp);
                if s.is_null() {
                    "NA".to_string()
                } else {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("NA")
                        .to_string()
                }
            }
        } else if t == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(x);
            if pname.is_null() {
                "symbol".to_string()
            } else {
                let s = crate::sexp::accessors::CHAR(pname);
                if s.is_null() {
                    "symbol".to_string()
                } else {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("symbol")
                        .to_string()
                }
            }
        } else {
            format!("{:?}", t)
        }
    }
}

pub(crate) unsafe fn factor_label_at(x: SEXP, code: i32) -> Option<String> {
    unsafe {
        if code <= 0 {
            return None;
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        let index = (code - 1) as R_xlen_t;
        if index >= XLENGTH(levels) {
            return None;
        }
        let charsxp = STRING_ELT(levels, index);
        if charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString() {
            None
        } else {
            Some(CStr::from_ptr(CHAR(charsxp)).to_string_lossy().into_owned())
        }
    }
}

pub(crate) fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub(crate) fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

pub(crate) fn days_from_civil(mut year: i64, month: i64, day: i64) -> i64 {
    year -= if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn civil_from_days(mut days: i64) -> (i64, i64, i64) {
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_prime = (5 * doy + 2) / 153;
    let day = doy - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

pub(crate) fn parse_iso_date_days(text: &str) -> Option<f64> {
    let text = text.trim();
    let mut parts = text.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
    {
        return None;
    }
    Some(days_from_civil(year, month, day) as f64)
}

pub(crate) fn date_days_to_iso(days: f64) -> Option<String> {
    let (year, month, day) = date_days_to_civil(days)?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

pub(crate) fn date_days_to_civil(days: f64) -> Option<(i64, i64, i64)> {
    if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !days.is_finite() {
        return None;
    }
    Some(civil_from_days(days.floor() as i64))
}

pub(crate) fn parse_iso_datetime_seconds(text: &str) -> Option<f64> {
    let text = text.trim();
    let mut fields = text.split_whitespace();
    let date = fields.next()?;
    let time = fields.next().unwrap_or("00:00:00");
    if fields.next().is_some() {
        return None;
    }
    let days = parse_iso_date_days(date)?;
    let mut parts = time.split(':');
    let hour = parts.next()?.parse::<i64>().ok()?;
    let minute = parts.next()?.parse::<i64>().ok()?;
    let second = parts.next().unwrap_or("0").parse::<i64>().ok()?;
    if parts.next().is_some()
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    Some(days * 86_400.0 + (hour * 3_600 + minute * 60 + second) as f64)
}

pub(crate) fn posix_seconds_to_iso(seconds: f64, include_tz: bool) -> Option<String> {
    posix_seconds_to_iso_with_time(seconds, include_tz, false)
}

pub(crate) fn posix_seconds_to_iso_with_time(
    seconds: f64,
    include_tz: bool,
    force_time: bool,
) -> Option<String> {
    if seconds.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !seconds.is_finite() {
        return None;
    }
    let whole = seconds.floor() as i64;
    let days = whole.div_euclid(86_400);
    let rem = whole.rem_euclid(86_400);
    let date = date_days_to_iso(days as f64)?;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let mut out = if !force_time && hour == 0 && minute == 0 && second == 0 {
        date
    } else {
        format!("{date} {hour:02}:{minute:02}:{second:02}")
    };
    if include_tz {
        out.push_str(" UTC");
    }
    Some(out)
}

pub(crate) unsafe fn sexp_has_class(x: SEXP, class_name: &str) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == class_name)
    }
}

pub(crate) fn elt_real_safe(x: SEXP, i: R_xlen_t) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return NA_REAL;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return NA_REAL;
        }
        let idx = if n == 0 { 0 } else { i % n };
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x).add(idx as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        }
    }
}
