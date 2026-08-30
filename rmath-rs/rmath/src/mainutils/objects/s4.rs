#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_imports
)]

use super::*;

// ---------------------------------------------------------------------------
// S4 class infrastructure
// ---------------------------------------------------------------------------

pub(crate) unsafe fn sexp_to_string(x: SEXP) -> Option<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return None;
        }
        let chars = match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => {
                if LENGTH(x) < 1 {
                    return None;
                }
                STRING_ELT(x, 0)
            }
            t if t == SEXPTYPE::CHARSXP => x,
            t if t == SEXPTYPE::SYMSXP => asChar(x),
            _ => return None,
        };
        if chars.is_null() {
            return None;
        }
        let ptr = CHAR(chars);
        if ptr.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

unsafe fn c_string_to_string(ptr: *const c_char) -> Option<String> {
    unsafe {
        if ptr.is_null() {
            None
        } else {
            Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned())
        }
    }
}

unsafe fn make_string_vector(values: &[String]) -> SEXP {
    unsafe {
        let out = Rf_allocVector(SEXPTYPE::STRSXP, values.len() as c_int);
        let _out_guard = protect(out);
        for (i, value) in values.iter().enumerate() {
            let cstr = CString::new(value.as_str()).unwrap_or_default();
            SET_STRING_ELT(out, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }
        out
    }
}

pub(crate) unsafe fn named_vec_elt(x: SEXP, name: &str) -> SEXP {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names = getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let wanted = CString::new(name).unwrap_or_default();
        for i in 0..LENGTH(names) {
            let current = STRING_ELT(names, i as R_xlen_t);
            if current.is_null() {
                continue;
            }
            let current = CHAR(current);
            if !current.is_null() && libc::strcmp(current, wanted.as_ptr()) == 0 {
                return VECTOR_ELT(x, i as R_xlen_t);
            }
        }
        R_NilValue()
    }
}

unsafe fn class_name_from_def(class_def: SEXP) -> Option<String> {
    unsafe {
        if TYPEOF(class_def) == SEXPTYPE::VECSXP {
            let class_name = named_vec_elt(class_def, "className");
            sexp_to_string(class_name)
        } else {
            sexp_to_string(class_def)
        }
    }
}

unsafe fn class_def_to_sexp(class_name: &str, class_def: &S4ClassDef) -> SEXP {
    unsafe {
        let out = Rf_allocVector(SEXPTYPE::VECSXP, 5);
        if out.is_null() {
            return R_NilValue();
        }
        let _out_guard = protect(out);

        let names = make_string_vector(&[
            "className".to_string(),
            "slots".to_string(),
            "contains".to_string(),
            "virtual".to_string(),
            "validity".to_string(),
        ]);
        let _names_guard = protect(names);

        let class_name_c = CString::new(class_name).unwrap_or_default();
        SET_VECTOR_ELT(out, 0, Rf_mkString(class_name_c.as_ptr()));
        SET_VECTOR_ELT(out, 1, make_string_vector(&class_def.slots));
        SET_VECTOR_ELT(out, 2, make_string_vector(&class_def.contains));
        SET_VECTOR_ELT(
            out,
            3,
            Rf_ScalarLogical(if class_def.virtual_class { TRUE } else { FALSE }),
        );
        SET_VECTOR_ELT(
            out,
            4,
            Rf_ScalarLogical(if class_def.has_validity { TRUE } else { FALSE }),
        );

        setAttrib(out, crate::sexp::attrib_core::R_NamesSymbol(), names);
        setAttrib(
            out,
            R_ClassSymbol(),
            make_string_vector(&["classRepresentation".to_string()]),
        );
        out
    }
}

pub unsafe fn R_do_MAKE_CLASS(what: *const c_char) -> SEXP {
    unsafe {
        let Some(name) = c_string_to_string(what) else {
            error("C level MAKE_CLASS macro called with NULL string pointer");
        };
        if s4_class(&name).is_none() {
            register_s4_class(name.clone(), Vec::new(), false);
        }
        R_getClassDef(what)
    }
}

pub unsafe fn R_getClassDef(what: *const c_char) -> SEXP {
    unsafe {
        let Some(name) = c_string_to_string(what) else {
            error("R_getClassDef(.) called with NULL string pointer");
        };
        match s4_class(&name) {
            Some(class_def) => class_def_to_sexp(&name, &class_def),
            None => R_NilValue(),
        }
    }
}

pub unsafe fn R_getClassDef_R(what: SEXP) -> SEXP {
    unsafe {
        if what.is_null() || what == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(what) == SEXPTYPE::VECSXP {
            let class_name = class_name_from_def(what);
            if class_name.is_some() {
                return what;
            }
        }
        let Some(name) = sexp_to_string(what) else {
            return R_NilValue();
        };
        let cstr = CString::new(name).unwrap_or_default();
        R_getClassDef(cstr.as_ptr())
    }
}

pub unsafe fn R_isVirtualClass(class_def: SEXP, _env: SEXP) -> c_int {
    unsafe {
        if class_def.is_null() || class_def == R_NilValue() {
            return FALSE;
        }
        if TYPEOF(class_def) == SEXPTYPE::VECSXP {
            let virtual_value = named_vec_elt(class_def, "virtual");
            if !virtual_value.is_null() && virtual_value != R_NilValue() {
                return if asLogical(virtual_value) == TRUE {
                    TRUE
                } else {
                    FALSE
                };
            }
        }
        class_name_from_def(class_def)
            .and_then(|name| s4_class(&name))
            .map(|class_def| if class_def.virtual_class { TRUE } else { FALSE })
            .unwrap_or(FALSE)
    }
}

pub unsafe fn R_extends(class1: SEXP, class2: SEXP, _env: SEXP) -> c_int {
    unsafe {
        let Some(class1) = class_name_from_def(class1) else {
            return FALSE;
        };
        let Some(class2) = class_name_from_def(class2) else {
            return FALSE;
        };
        if s4_class_extends(&class1, &class2) {
            TRUE
        } else {
            FALSE
        }
    }
}

pub unsafe fn R_do_new_object(class_def: SEXP) -> SEXP {
    unsafe {
        let Some(class_name) = class_name_from_def(class_def) else {
            return R_NilValue();
        };
        let Some(class_def) = s4_class(&class_name) else {
            return R_NilValue();
        };
        if class_def.virtual_class {
            error(&format!("class '{}' is virtual", class_name));
        }

        let out = Rf_allocVector(SEXPTYPE::VECSXP, class_def.slots.len() as c_int);
        let _out_guard = protect(out);
        let names = make_string_vector(&class_def.slots);
        let _names_guard = protect(names);
        for i in 0..class_def.slots.len() {
            SET_VECTOR_ELT(out, i as R_xlen_t, R_NilValue());
        }
        setAttrib(out, crate::sexp::attrib_core::R_NamesSymbol(), names);
        setAttrib(out, R_ClassSymbol(), make_string_vector(&[class_name]));
        asS4(out, TRUE, 0)
    }
}

// ---------------------------------------------------------------------------
// S4 object manipulation
// ---------------------------------------------------------------------------

pub unsafe fn R_seemsOldStyleS4Object(object: SEXP) -> c_int {
    unsafe {
        if object.is_null() {
            return FALSE;
        }
        if isObject(object) == FALSE || IS_S4_OBJECT(object) != FALSE {
            return FALSE;
        }
        let klass = getAttrib(object, R_ClassSymbol());
        if klass.is_null() || klass == R_NilValue() {
            return FALSE;
        }
        if LENGTH(klass) != 1 {
            return FALSE;
        }
        let pkg_sym = sym("package");
        let pkg = getAttrib(klass, pkg_sym);
        if pkg.is_null() || pkg == R_NilValue() {
            return FALSE;
        }
        TRUE
    }
}

pub unsafe fn isS4(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            return FALSE;
        }
        IS_S4_OBJECT(s)
    }
}

pub unsafe fn asS4(s: SEXP, flag: c_int, complete: c_int) -> SEXP {
    unsafe {
        if s.is_null() {
            return s;
        }
        if flag == IS_S4_OBJECT(s) {
            return s;
        }
        let _s_guard = protect(s);

        if flag != FALSE {
            SET_S4_OBJECT(s);
        } else {
            if complete != FALSE {
                // Check for S4 data slot
                // Full implementation would call R_getS4DataSlot
                if complete == 1 {
                    let klass = R_data_class(s);
                    let class_str = if !klass.is_null() && LENGTH(klass) > 0 {
                        let cs = CHAR(STRING_ELT(klass, 0));
                        if !cs.is_null() {
                            std::ffi::CStr::from_ptr(cs).to_string_lossy().into_owned()
                        } else {
                            "unknown".to_string()
                        }
                    } else {
                        "unknown".to_string()
                    };
                    let msg = format!(
                        "object of class \"{}\" does not correspond to a valid S3 object",
                        class_str
                    );
                    std::panic::panic_any(crate::sexp::context::RError { message: msg });
                } else {
                    // complete == 2: conditional, return unchanged
                    return s;
                }
            }
            UNSET_S4_OBJECT(s);
        }

        s
    }
}

// ---------------------------------------------------------------------------
// R_allocObject / do_objsxp -- bare OBJSXP allocation
// ---------------------------------------------------------------------------

/// Allocate a bare object of type "object" (OBJSXP without the S4 bit).
///
/// This is the equivalent of R's `R_allocObject()` — the constructor behind
/// `.OBJSXP()`, as used e.g. by S7.
pub unsafe fn R_allocObject() -> SEXP {
    unsafe { crate::sexp::memory_ext::allocSExp(SEXPTYPE::OBJSXP) }
}

/// R's `.OBJSXP()` — a bare OBJSXP object without the S4 bit.
pub unsafe fn do_objsxp(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_allocObject() }
}

// ---------------------------------------------------------------------------
// do_setS4Object -- internal .setS4Object()
// ---------------------------------------------------------------------------

pub unsafe fn do_setS4Object(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return R_NilValue();
        }
        let object = CAR(args);
        if object.is_null() {
            return R_NilValue();
        }

        let flag = if !CDR(args).is_null() && CDR(args) != R_NilValue() {
            asLogical(CADR(args))
        } else {
            TRUE
        };

        if flag == crate::sexp::ffi::NA_INTEGER {
            error("invalid 'flag' argument");
        }

        let complete = if !CDR(args).is_null()
            && CDR(args) != R_NilValue()
            && !CDDR(args).is_null()
            && CDDR(args) != R_NilValue()
        {
            asInteger(CAR(CDDR(args)))
        } else {
            TRUE as c_int
        };

        if complete == crate::sexp::ffi::NA_INTEGER {
            error("invalid 'complete' argument");
        }

        if flag == IS_S4_OBJECT(object) {
            return object;
        }
        asS4(object, flag, complete)
    }
}
