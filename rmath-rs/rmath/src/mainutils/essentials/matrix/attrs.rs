//! Attribute access: names, class, attr, attributes, structure, comment, namespace lookup, storage.mode — extracted verbatim from the former single-file module.
use super::*;

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
        let dimnames = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dimnames".as_ptr()));
        if !dimnames.is_null() && TYPEOF(dimnames) == SEXPTYPE::VECSXP && LENGTH(dimnames) >= 1 {
            return VECTOR_ELT(dimnames, 0);
        }
        if is_data_frame_like(x) {
            return string_vector(&data_frame_row_names(x));
        }
        crate::sexp::attrib_core::getAttrib(x, Rf_install(c"row.names".as_ptr()))
    }
}

/// R's `colnames(x)` — get column names attribute.
pub unsafe fn do_colnames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dimnames".as_ptr()));
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
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"names".as_ptr()), value);
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
        set_array_dimnames(x, value);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// Normalize and install an array's dimnames using GNU R's `dimnamesgets`
/// contract.  In particular, axis labels are character vectors regardless of
/// the vector type supplied by the caller.
pub(super) unsafe fn set_array_dimnames(x: SEXP, value: SEXP) {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(x, R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP {
            std::panic::panic_any(RError {
                message: "'dimnames' applied to non-array".to_string(),
            });
        }

        let dimnames_sym = R_DimNamesSymbol();
        if value.is_null() || value == R_NilValue() {
            crate::sexp::attrib_core::setAttrib(x, dimnames_sym, R_NilValue());
            return;
        }
        if TYPEOF(value) != SEXPTYPE::VECSXP && TYPEOF(value) != SEXPTYPE::LISTSXP {
            std::panic::panic_any(RError {
                message: "'dimnames' must be a list".to_string(),
            });
        }

        let rank = XLENGTH(dim);
        let supplied = XLENGTH(value);
        if supplied > rank {
            std::panic::panic_any(RError {
                message: format!(
                    "length of 'dimnames' [{}] must match that of 'dims' [{}]",
                    supplied, rank
                ),
            });
        }
        if supplied == 0 {
            crate::sexp::attrib_core::setAttrib(x, dimnames_sym, R_NilValue());
            return;
        }

        // Always build a fresh list: coercing a shared list in place would also
        // change the caller's object (`dn <- list(1:2); dimnames(x) <- dn`).
        let normalized = Rf_allocVector3(SEXPTYPE::VECSXP, rank);
        let _normalized_guard = protect(normalized);
        let mut pair = value;
        for axis in 0..rank {
            let labels = if axis >= supplied {
                R_NilValue()
            } else if TYPEOF(value) == SEXPTYPE::VECSXP {
                VECTOR_ELT(value, axis)
            } else {
                let labels = CAR(pair);
                pair = CDR(pair);
                labels
            };

            if labels.is_null() || labels == R_NilValue() {
                SET_VECTOR_ELT(normalized, axis, R_NilValue());
                continue;
            }
            if crate::sexp::constructors::Rf_isVector(labels) == 0 {
                std::panic::panic_any(RError {
                    message: format!(
                        "invalid type ({}) for 'dimnames' (must be a vector)",
                        TYPEOF(labels)
                    ),
                });
            }
            let label_count = XLENGTH(labels);
            let extent = INTEGER_ELT(dim, axis as c_int) as R_xlen_t;
            if label_count != 0 && label_count != extent {
                std::panic::panic_any(RError {
                    message: format!(
                        "length of 'dimnames' [{}] not equal to array extent",
                        axis + 1
                    ),
                });
            }

            let labels = if label_count == 0 {
                R_NilValue()
            } else if inherits_class(labels, "factor") {
                crate::mainutils::coerce::asCharacterFactor(labels)
            } else if TYPEOF(labels) == SEXPTYPE::STRSXP {
                labels
            } else {
                let coerced =
                    crate::mainutils::coerce::coerceVector(labels, SEXPTYPE::STRSXP.as_c_int());
                crate::sexp::accessors::SET_ATTRIB(coerced, R_NilValue());
                crate::sexp::accessors::SET_OBJECT(coerced, FALSE);
                coerced
            };
            SET_VECTOR_ELT(normalized, axis, labels);
        }

        // `lengthgets()` retains names on a new-list dimnames value and pads
        // them with empty strings when fewer names than dimensions were given.
        if TYPEOF(value) == SEXPTYPE::VECSXP {
            let names = crate::sexp::attrib_core::getAttrib(value, R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
                let normalized_names = Rf_allocVector3(SEXPTYPE::STRSXP, rank);
                let _normalized_names_guard = protect(normalized_names);
                for axis in 0..rank {
                    let name = if axis < XLENGTH(names) {
                        STRING_ELT(names, axis)
                    } else {
                        Rf_mkChar(c"".as_ptr())
                    };
                    SET_STRING_ELT(normalized_names, axis, name);
                }
                crate::sexp::attrib_core::setAttrib(normalized, R_NamesSymbol(), normalized_names);
            }
        }

        crate::sexp::attrib_core::setAttrib(x, dimnames_sym, normalized);
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

        let dimnames_sym = Rf_install(c"dimnames".as_ptr());
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
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
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
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"class".as_ptr()), value);
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
    unsafe { Rf_install(c"comment".as_ptr()) }
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
pub unsafe fn do_structure(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        const SPECIALS: [&str; 5] = [".Dim", ".Dimnames", ".Names", ".Tsp", ".Label"];
        const REPLACEMENTS: [&str; 5] = ["dim", "dimnames", "names", "tsp", "levels"];

        let mut renamed: Vec<(String, String)> = Vec::new();

        // GNU R's language-level structure() retains compatibility with
        // factors deparsed before R 2.5.0, when their integer codes were
        // emitted as doubles. Coerce before attaching the factor class so the
        // ordinary storage.mode<- factor guard does not reject the repair.
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some("class") {
                // attrib[["class", exact = TRUE]] selects the first exact
                // match when duplicate names are present.
                if TYPEOF(x) == SEXPTYPE::REALSXP
                    && string_vector_contains_value(CAR(current), "factor")
                {
                    let coerced =
                        crate::mainutils::coerce::coerceVector(x, SEXPTYPE::INTSXP.as_c_int());
                    crate::sexp::accessors::SET_ATTRIB(coerced, ATTRIB(x));
                    x = coerced;
                }
                break;
            }
            current = CDR(current);
        }
        let _x_guard = protect(x);

        current = CDR(args);
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
            // `.Deprecated()` signals a classed warning, rather than a plain
            // `simpleWarning`, so calling handlers can distinguish API
            // deprecations while the default path retains structure()'s call.
            let condition = crate::mainutils::errors::R_makeWarningCondition(
                call,
                c"deprecatedWarning".as_ptr(),
                std::ptr::null(),
                0,
                c_msg.as_ptr(),
            );
            let _condition_guard = protect(condition);
            let muffled =
                crate::mainutils::essentials::signal_calling_warning_condition(condition, rho);
            if !muffled {
                crate::mainutils::errors::warning_condition_default(condition);
            }
        }

        x
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn attributes_preserve_assignment_order() {
        let mut session = crate::sexp::session::RSession::new();
        let (result, _, _) = session.eval_code_with_output_capture(
            "X <- matrix(1:4, 2, 2, dimnames = list(c('A', 'B'), 1:2));\
             y <- 1; attr(y, 'first') <- 1; attr(y, 'first') <- 3;\
             attr(y, 'second') <- 2;\
             z <- 1; attr(z, 'first') <- 1; attr(z, 'second') <- 2;\
             attr(z, 'first') <- NULL; attr(z, 'first') <- 4;\
             c(names(attributes(X)), names(attributes(y)), names(attributes(z)))",
        );

        let result = result.expect("attribute enumeration should evaluate");
        let names = (0..result.clone().len())
            .map(|index| result.clone().string_text_elt(index).flatten().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["dim", "dimnames", "first", "second", "second", "first"]
        );
    }

    #[test]
    fn dimnames_are_coerced_validated_and_do_not_mutate_the_input_list() {
        let mut session = crate::sexp::session::RSession::new();
        let (result, _, _) = session.eval_code_with_output_capture(
            "dn <- list(cols = 1:2);\
             X <- matrix(1:4, 2, 2, dimnames = list(c('A', 'B'), 1:2));\
             Y <- array(1:4, c(2, 2)); dimnames(Y) <- dn;\
             c(typeof(dimnames(X)[[2]]),\
               paste(dimnames(X)[[2]], collapse = ','),\
               typeof(dn[[1]]), length(dimnames(Y)),\
               names(dimnames(Y))[1], names(dimnames(Y))[2])",
        );

        let result = result.expect("valid dimnames should be normalized");
        let values = (0..result.clone().len())
            .map(|index| result.clone().string_text_elt(index).flatten().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values, ["character", "1,2", "integer", "2", "cols", ""]);

        let (result, _, _) = session.eval_code_with_output_capture(
            "X <- matrix(1:4, 2, 2); dimnames(X) <- list(letters[1:3])",
        );
        assert!(result.is_err(), "axis labels must match their array extent");

        let (result, _, _) = session
            .eval_code_with_output_capture("X <- matrix(1:4, 2, 2); dimnames(X) <- c('a', 'b')");
        assert!(result.is_err(), "dimnames must be supplied as a list");
    }

    #[test]
    fn structure_coerces_legacy_double_factor_codes_to_integer() {
        let mut session = crate::sexp::session::RSession::new();
        let (result, _, _) = session.eval_code_with_output_capture(
            "state <- structure(c(1, 2), levels = c('a', 'b'), class = 'factor');\
             c(typeof(state), storage.mode(state), identical(state,\
               structure(c(1L, 2L), levels = c('a', 'b'), class = 'factor')))",
        );

        let result = result.expect("legacy factor structure should evaluate");
        assert_eq!(result.clone().string_text_elt(0), Some(Some("integer")));
        assert_eq!(result.clone().string_text_elt(1), Some(Some("integer")));
        assert_eq!(result.string_text_elt(2), Some(Some("TRUE")));
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
