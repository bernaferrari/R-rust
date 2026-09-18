//! Essentials domain module `s4` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::CString;
use std::collections::HashMap;


#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_lang3,
    Rf_mkChar, Rf_mkString,
};

use crate::sexp::context::RError;
use crate::sexp::ffi::{FALSE, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete S3/S4: class, isS4, is
// ---------------------------------------------------------------------------

/// R's `class(x)` — get S3 class vector.
pub unsafe fn do_S3_class(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_class_get(_call, _op, args, _rho) }
}

/// R's `isS4(x)` — check if object is S4.
pub unsafe fn do_isS4(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(crate::mainutils::objects::isS4(x))
    }
}

/// GNU `isNamespace(ns)` — `.Internal(isNamespaceEnv(ns))`.
pub unsafe fn do_isNamespace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let ns = CAR(args);
        if ns.is_null() || TYPEOF(ns) != SEXPTYPE::ENVSXP {
            return Rf_ScalarLogical(FALSE);
        }
        let info = crate::sexp::envir::R_findVarInFrame(
            ns,
            Rf_install(c".__NAMESPACE__.".as_ptr()),
        );
        Rf_ScalarLogical(
            if !info.is_null()
                && info != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(info) == SEXPTYPE::ENVSXP
            {
                TRUE
            } else {
                FALSE
            },
        )
    }
}


/// R's `is(x, class2)` — type/class check.
pub unsafe fn do_is(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class2_arg = CAR(CDR(args));
        if x.is_null() || class2_arg.is_null() || class2_arg == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let class2 = elt_to_string(class2_arg, 0);
        if x == R_NilValue() {
            return Rf_ScalarLogical(if class2 == "NULL" { TRUE } else { FALSE });
        }
        // Get the type of x
        let type_name = match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP => "logical",
            t if t == SEXPTYPE::INTSXP => "integer",
            t if t == SEXPTYPE::REALSXP => "double",
            t if t == SEXPTYPE::CPLXSXP => "complex",
            t if t == SEXPTYPE::STRSXP => "character",
            t if t == SEXPTYPE::VECSXP => "list",
            t if t == SEXPTYPE::LISTSXP => "pairlist",
            t if t == SEXPTYPE::LANGSXP => "language",
            t if t == SEXPTYPE::SYMSXP => "symbol",
            t if t == SEXPTYPE::CLOSXP => "closure",
            t if t == SEXPTYPE::ENVSXP => "environment",
            _ => "any",
        };
        // Check S3 class
        let class_sym = Rf_install(c"class".as_ptr());
        let class_val = crate::sexp::attrib_core::getAttrib(x, class_sym);
        if !class_val.is_null()
            && class_val != R_NilValue()
            && TYPEOF(class_val) == SEXPTYPE::STRSXP
        {
            let n = LENGTH(class_val);
            for i in 0..n {
                let charsxp = crate::sexp::accessors::STRING_ELT(class_val, i as R_xlen_t);
                if !charsxp.is_null() {
                    let s = crate::sexp::accessors::CHAR(charsxp);
                    if !s.is_null() {
                        let c = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                        if c == class2 {
                            return Rf_ScalarLogical(TRUE);
                        }
                        if crate::mainutils::objects::isS4(x) == TRUE
                            && crate::mainutils::objects::s4_class_extends(c, &class2)
                        {
                            return Rf_ScalarLogical(TRUE);
                        }
                    }
                }
            }
        }
        // Check type name
        let is_match = type_name == class2
            || (class2 == "numeric" && (type_name == "double" || type_name == "integer"))
            || (class2 == "vector"
                && (type_name == "logical"
                    || type_name == "integer"
                    || type_name == "double"
                    || type_name == "character"
                    || type_name == "complex"))
            || (class2 == "atomic"
                && type_name != "list"
                && type_name != "pairlist"
                && type_name != "language"
                && type_name != "closure"
                && type_name != "environment");
        Rf_ScalarLogical(if is_match { TRUE } else { FALSE })
    }
}

unsafe fn list_tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(tag);
        if printname.is_null() {
            return None;
        }
        let chars = CHAR(printname);
        if chars.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(chars)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

pub(crate) unsafe fn string_vector_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return Vec::new();
        }
        let mut values = Vec::with_capacity(LENGTH(x).max(0) as usize);
        for i in 0..LENGTH(x) {
            values.push(elt_to_string(x, i as R_xlen_t));
        }
        values
    }
}

pub(crate) unsafe fn coerce_string_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(x, i)).collect()
    }
}

unsafe fn string_vector_names_or_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return Vec::new();
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        let mut out = Vec::new();
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..LENGTH(names) {
                let name = elt_to_string(names, i as R_xlen_t);
                if !name.is_empty() {
                    out.push(name);
                }
            }
        }
        if out.is_empty() {
            out = string_vector_values(x);
        }
        out
    }
}

unsafe fn s4_slots_from_args(args: SEXP) -> Vec<String> {
    unsafe {
        let mut slots = Vec::new();
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = list_tag_name(current) {
                match name.as_str() {
                    "slots" | "representation" => {
                        for slot in string_vector_names_or_values(CAR(current)) {
                            if slots.iter().any(|existing| existing == &slot) {
                                std::panic::panic_any(RError {
                                    message: format!(
                                        "All slot names must be distinct in: ('{}')",
                                        slot
                                    ),
                                });
                            }
                            slots.push(slot);
                        }
                    }
                    "contains" | "where" | "prototype" | "validity" | "sealed" | "package" => {}
                    _ => {
                        if slots.iter().any(|existing| existing == &name) {
                            std::panic::panic_any(RError {
                                message: format!(
                                    "All slot names must be distinct in: ('{}')",
                                    name
                                ),
                            });
                        }
                        slots.push(name);
                    }
                }
            }
            current = CDR(current);
        }
        slots
    }
}
unsafe fn s4_slot_types_from_args(args: SEXP) -> Option<HashMap<String, String>> {
    unsafe {
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if matches!(
                list_tag_name(current).as_deref(),
                Some("slots") | Some("representation")
            ) {
                let spec = CAR(current);
                if spec.is_null() || spec == R_NilValue() || TYPEOF(spec) != SEXPTYPE::STRSXP {
                    return None;
                }
                let names = crate::sexp::attrib_core::getAttrib(
                    spec,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
                    return None;
                }
                let mut types = HashMap::new();
                for i in 0..LENGTH(spec) {
                    let name = elt_to_string(names, i as R_xlen_t);
                    let ty = elt_to_string(spec, i as R_xlen_t);
                    if !name.is_empty() && !ty.is_empty() {
                        types.insert(name, ty);
                    }
                }
                if types.is_empty() {
                    return None;
                }
                return Some(types);
            }
            current = CDR(current);
        }
        None
    }
}

unsafe fn s4_prototype_for_type(type_name: &str) -> SEXP {
    unsafe {
        match type_name {
            "integer" => Rf_allocVector3(SEXPTYPE::INTSXP, 0),
            "numeric" | "double" => Rf_allocVector3(SEXPTYPE::REALSXP, 0),
            "logical" => Rf_allocVector3(SEXPTYPE::LGLSXP, 0),
            "character" => Rf_allocVector3(SEXPTYPE::STRSXP, 0),
            "raw" => Rf_allocVector3(SEXPTYPE::RAWSXP, 0),
            "complex" => Rf_allocVector3(SEXPTYPE::CPLXSXP, 0),
            "list" => Rf_allocVector3(SEXPTYPE::VECSXP, 0),
            "expression" => Rf_allocVector3(SEXPTYPE::EXPRSXP, 0),
            _ => R_NilValue(),
        }
    }
}



unsafe fn s4_contains_from_args(args: SEXP) -> Vec<String> {
    unsafe {
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if matches!(list_tag_name(current).as_deref(), Some("contains")) {
                let mut contains = string_vector_values(CAR(current));
                contains.retain(|name| !name.is_empty() && name != "VIRTUAL");
                let mut ordered = Vec::new();
                for parent in contains {
                    if !ordered.iter().any(|existing| existing == &parent) {
                        ordered.push(parent);
                    }
                }
                contains = ordered;
                return contains;
            }
            current = CDR(current);
        }
        Vec::new()
    }
}

unsafe fn string_vector_from_values(values: &[String]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        for (i, value) in values.iter().enumerate() {
            let cstr = CString::new(value.as_str()).unwrap_or_default();
            let charsxp = Rf_mkChar(cstr.as_ptr());
            SET_STRING_ELT(result, i as R_xlen_t, charsxp);
        }
        result
    }
}

unsafe fn s4_named_arg(args: SEXP, name: &str) -> Option<SEXP> {
    unsafe {
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if list_tag_name(current).as_deref() == Some(name) {
                return Some(CAR(current));
            }
            current = CDR(current);
        }
        None
    }
}


/// GNU `setClass(Class, ..., contains, validity)`.
pub unsafe fn do_setClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_arg = CAR(args);
        if class_arg.is_null() || class_arg == R_NilValue() {
            std::panic::panic_any(RError {
                message: "'Class' must name an S4 class".to_string(),
            });
        }
        let class_name = elt_to_string(class_arg, 0);
        let slots = s4_slots_from_args(args);
        let contains = s4_contains_from_args(args);
        let virtual_class = string_vector_values(CAR(CDR(args)))
            .iter()
            .any(|value| value == "VIRTUAL");
        crate::mainutils::objects::register_s4_class_with_extends(
            class_name.clone(),
            slots,
            contains,
            virtual_class,
        );
        if let Some(types) = s4_slot_types_from_args(args) {
            crate::mainutils::objects::set_s4_slot_types(&class_name, types);
        }

        if let Some(validity) = s4_named_arg(args, "validity") {
            crate::mainutils::objects::set_s4_validity_fn(&class_name, validity);
        }
        s4_class_generator(&class_name)
    }
}

unsafe fn s4_class_generator(class_name: &str) -> SEXP {
    unsafe {
        let dots = crate::sexp::symbol::R_DotsSymbol();
        let formals = Rf_cons(crate::sexp::globals::R_MissingArg(), R_NilValue());
        let _formals = protect(formals);
        SETTAG(formals, dots);
        let class_str = {
            let cstr = CString::new(class_name).unwrap_or_default();
            Rf_mkString(cstr.as_ptr())
        };
        let _class_str = protect(class_str);
        let body = crate::sexp::constructors::Rf_lang3(
            Rf_install(c"new".as_ptr()),
            class_str,
            dots,
        );
        let _body = protect(body);
        crate::mainutils::dstruct::mkCLOSXP(formals, body, crate::sexp::globals::R_BaseEnv())
    }
}

pub unsafe fn do_setValidity(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_name = elt_to_string(CAR(args), 0);
        let method = CAR(CDR(args));
        if !crate::mainutils::objects::set_s4_validity_fn(&class_name, method) {
            std::panic::panic_any(RError {
                message: format!("class '{}' is not defined", class_name),
            });
        }
        method
    }
}


/// R's `isVirtualClass(Class)` — check if a registered S4 class is virtual.
pub unsafe fn do_isVirtualClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_name = elt_to_string(CAR(args), 0);
        let is_virtual = crate::mainutils::objects::s4_class(&class_name)
            .map(|class_def| class_def.virtual_class)
            .unwrap_or(false);
        Rf_ScalarLogical(if is_virtual { TRUE } else { FALSE })
    }
}

/// Use the GNU `methods::new` closure once that namespace is loaded.
unsafe fn methods_new_closure() -> Option<SEXP> {
    unsafe {
        let namespace = crate::mainutils::essentials::cached_namespace_by_name("methods")?;
        let name = Rf_install(c"new".as_ptr());
        let mut value = crate::sexp::envir::R_findVarInFrame(namespace, name);
        if value.is_null() || value == crate::sexp::globals::R_UnboundValue() {
            return None;
        }
        if TYPEOF(value) == SEXPTYPE::PROMSXP {
            value = crate::sexp::envir::forcePromise(value);
        }
        if TYPEOF(value) == SEXPTYPE::CLOSXP {
            Some(value)
        } else {
            None
        }
    }
}



/// GNU `new(Class, ...)` — construct an S4 object.
pub unsafe fn do_new(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(fun) = methods_new_closure() {
            return crate::eval::closure::applyClosure(
                call,
                fun,
                args,
                rho,
                R_NilValue(),
                TRUE,
            );
        }
        let class_arg = CAR(args);

        if class_arg.is_null() || class_arg == R_NilValue() {
            return R_NilValue();
        }
        let class_name = elt_to_string(class_arg, 0);
        let Some(class_def) = crate::mainutils::objects::s4_class(&class_name) else {
            std::panic::panic_any(RError {
                message: format!("class '{}' is not defined", class_name),
            });
        };
        let class_slots = crate::mainutils::objects::s4_all_slots(&class_name).unwrap_or_default();
        if class_def.virtual_class {
            std::panic::panic_any(RError {
                message: format!("class '{}' is virtual", class_name),
            });
        }
        if class_slots.iter().any(|slot| slot == ".Data") {
            let mut data = R_NilValue();
            let mut current = CDR(args);
            while !current.is_null() && current != R_NilValue() {
                let tag = list_tag_name(current);
                if tag.is_none() || tag.as_deref() == Some(".Data") {
                    data = CAR(current);
                    break;
                }
                current = CDR(current);
            }

            if data.is_null() || data == R_NilValue() {
                if class_def.contains.iter().any(|parent| parent == "list") {
                    data = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
                } else if class_def.contains.iter().any(|parent| parent == "expression") {
                    data = Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
                } else if class_def.contains.iter().any(|parent| parent == "integer") {
                    data = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
                } else if class_def
                    .contains
                    .iter()
                    .any(|parent| parent == "numeric" || parent == "double")
                {
                    data = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
                } else if class_def.contains.iter().any(|parent| parent == "character") {
                    data = Rf_allocVector3(SEXPTYPE::STRSXP, 0);
                } else if class_def.contains.iter().any(|parent| parent == "logical") {
                    data = Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
                }
            }
            let _data = protect(data);
            return finish_s4_object(data, &class_name);
        }

        let mut slots: Vec<(String, SEXP)> = Vec::new();
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            let slot_name =
                list_tag_name(current).unwrap_or_else(|| format!("slot{}", slots.len() + 1));
            if !class_slots.is_empty() && !class_slots.iter().any(|slot| slot == &slot_name) {
                std::panic::panic_any(RError {
                    message: format!(
                        "slot '{}' is not defined for class '{}'",
                        slot_name, class_name
                    ),
                });
            }
            slots.push((slot_name, arg));
            current = CDR(current);
        }
        let mut proto_guards = Vec::new();
        for slot in &class_slots {
            if slot == ".Data" {
                continue;
            }
            if !slots.iter().any(|(name, _)| name == slot) {
                let proto = class_def
                    .slot_types
                    .get(slot)
                    .map(|ty| s4_prototype_for_type(ty))
                    .unwrap_or_else(|| R_NilValue());
                if !proto.is_null() && proto != R_NilValue() {
                    proto_guards.push(protect(proto));
                }
                slots.push((slot.clone(), proto));
            }
        }

        let n = slots.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _np = protect(names);
        for (i, (name, val)) in slots.iter().enumerate() {
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as R_xlen_t, *val);
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(names, i as R_xlen_t, charsxp);
            }
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        let finished = finish_s4_object(result, &class_name);
        drop(proto_guards);
        finished

    }
}

unsafe fn finish_s4_object(object: SEXP, class_name: &str) -> SEXP {
    unsafe {
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let _class_guard = protect(class_vec);
            let cstr = CString::new(class_name).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(class_vec, 0, charsxp);
            }
            crate::sexp::attrib_core::setAttrib(
                object,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class_vec,
            );
        }
        crate::mainutils::objects::asS4(object, TRUE, 0)
    }
}

/// GNU `validObject(object)` — run the class validity method if present.
pub unsafe fn do_validObject(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let object = CAR(args);
        if object.is_null() || object == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        let class_val = crate::sexp::attrib_core::getAttrib(
            object,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if class_val.is_null() || class_val == R_NilValue() || TYPEOF(class_val) != SEXPTYPE::STRSXP
        {
            return Rf_ScalarLogical(TRUE);
        }
        let class_name = elt_to_string(class_val, 0);
        let Some(method) = crate::mainutils::objects::s4_validity_fn(&class_name) else {
            return Rf_ScalarLogical(TRUE);
        };
        if method.is_null() || method == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        let call_args = Rf_cons(object, R_NilValue());
        let _call_args = protect(call_args);
        let call = Rf_cons(method, call_args);
        let _call = protect(call);
        if !call.is_null() {
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        let result = crate::eval::eval::Rf_eval(call, rho);
        let _result = protect(result);
        if result.is_null() || result == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        if TYPEOF(result) == SEXPTYPE::LGLSXP {
            if XLENGTH(result) > 0 && *LOGICAL(result) == TRUE {
                return Rf_ScalarLogical(TRUE);
            }
            std::panic::panic_any(RError {
                message: format!("invalid class \"{class_name}\" object"),
            });
        }
        if TYPEOF(result) == SEXPTYPE::STRSXP && XLENGTH(result) > 0 {
            let reason = elt_to_string(result, 0);
            if !reason.is_empty() {
                std::panic::panic_any(RError { message: reason });
            }
        }
        Rf_ScalarLogical(TRUE)
    }
}


/// R's `show(object)` — display an S4 object (simplified).
pub unsafe fn do_show(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object = CAR(args);
        if object.is_null() || object == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let class_sym = Rf_install(c"class".as_ptr());
        let class_val = crate::sexp::attrib_core::getAttrib(object, class_sym);
        if !class_val.is_null()
            && class_val != R_NilValue()
            && TYPEOF(class_val) == SEXPTYPE::STRSXP
        {
            let charsxp = crate::sexp::accessors::STRING_ELT(class_val, 0);
            if !charsxp.is_null() {
                let s = crate::sexp::accessors::CHAR(charsxp);
                if !s.is_null() {
                    let class_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("unknown");
                    println!("An object of class \"{}\"", class_str);
                }
            }
        }
        if TYPEOF(object) == SEXPTYPE::VECSXP {
            let n = XLENGTH(object);
            let names_sym = Rf_install(c"names".as_ptr());
            let names_val = crate::sexp::attrib_core::getAttrib(object, names_sym);
            for i in 0..n {
                let slot_val = crate::sexp::accessors::VECTOR_ELT(object, i);
                let slot_name = if !names_val.is_null() && names_val != R_NilValue() {
                    let ns = crate::sexp::accessors::STRING_ELT(names_val, i);
                    if !ns.is_null() {
                        let s = crate::sexp::accessors::CHAR(ns);
                        if !s.is_null() {
                            std::ffi::CStr::from_ptr(s)
                                .to_str()
                                .unwrap_or("")
                                .to_string()
                        } else {
                            format!("Slot{}", i + 1)
                        }
                    } else {
                        format!("Slot{}", i + 1)
                    }
                } else {
                    format!("Slot{}", i + 1)
                };
                let val_str = elt_to_string(slot_val, 0);
                println!("Slot \"{}\":", slot_name);
                println!("  {}", val_str);
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        object
    }
}

unsafe fn R_data_part(obj: SEXP) -> SEXP {
    unsafe {
        if crate::mainutils::coerce::IS_S4_OBJECT(obj) != FALSE {
            if let Some(value) = s4_named_slot(obj, ".Data") {
                return value;
            }
            let class_val = crate::sexp::attrib_core::getAttrib(
                obj,
                crate::sexp::attrib_core::R_ClassSymbol(),
            );
            if TYPEOF(class_val) == SEXPTYPE::STRSXP && XLENGTH(class_val) > 0 {
                let class_name = elt_to_string(class_val, 0);
                if crate::mainutils::objects::s4_class(&class_name).is_some_and(|class_def| {
                    class_def.contains.iter().any(|parent| {
                        matches!(
                            parent.as_str(),
                            "list"
                                | "expression"
                                | "formula"
                                | "integer"
                                | "numeric"
                                | "double"
                                | "character"
                                | "logical"
                                | "raw"
                                | "complex"
                                | "language"
                                | "vector"
                        )
                    })
                }) {
                    return obj;
                }
            }
            let data_sym = Rf_install(c".Data".as_ptr());
            return crate::sexp::attrib_core::getAttrib(obj, data_sym);
        }
        // GNU methods::getDataPart on a base vector is the object itself.
        obj
    }
}


/// R's `slotNames(Class)` — get the names of slots of an S4 class.
pub unsafe fn do_slotNames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_arg = CAR(args);
        if class_arg.is_null() || class_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        if TYPEOF(class_arg) == SEXPTYPE::STRSXP {
            if let Some(slots) =
                crate::mainutils::objects::s4_all_slots(&elt_to_string(class_arg, 0))
            {
                return string_vector_from_values(&slots);
            }
        }
        if crate::mainutils::objects::isS4(class_arg) == TRUE {
            let class_sym = Rf_install(c"class".as_ptr());
            let class_val = crate::sexp::attrib_core::getAttrib(class_arg, class_sym);
            if !class_val.is_null()
                && class_val != R_NilValue()
                && TYPEOF(class_val) == SEXPTYPE::STRSXP
                && LENGTH(class_val) > 0
            {
                if let Some(slots) =
                    crate::mainutils::objects::s4_all_slots(&elt_to_string(class_val, 0))
                {
                    return string_vector_from_values(&slots);
                }
            }
        }
        // If it's an object with names, return names
        let names_sym = Rf_install(c"names".as_ptr());
        let names_val = crate::sexp::attrib_core::getAttrib(class_arg, names_sym);
        if !names_val.is_null()
            && names_val != R_NilValue()
            && TYPEOF(names_val) == SEXPTYPE::STRSXP
        {
            return names_val;
        }
        // If it's a string, treat as class name - return empty
        Rf_allocVector3(SEXPTYPE::STRSXP, 0)
    }
}

/// R's `slot(object, name)` — `.Call(C_R_get_slot, object, name)`.
///
/// Upstream funnels `slot()` and the `@` operator through `R_do_slot()`
/// (main/attrib.c). Port S4 objects store their slots as named vector
/// elements rather than attributes, so the lookup adapts that
/// representation while keeping upstream's exact error behavior.
pub unsafe fn do_slot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object = CAR(args);
        let name_arg = CADR(args);
        R_do_slot(object, name_arg)
    }
}


/// Look up a slot stored as a named vector element (port S4
/// representation). Exact, non-partial name matching.
unsafe fn s4_named_slot(obj: SEXP, name: &str) -> Option<SEXP> {
    unsafe {
        if obj.is_null() || TYPEOF(obj) != SEXPTYPE::VECSXP {
            return None;
        }
        let names_sym = Rf_install(c"names".as_ptr());
        let names = crate::sexp::attrib_core::getAttrib(obj, names_sym);
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return None;
        }
        let n = LENGTH(names);
        for i in 0..n {
            let s = STRING_ELT(names, i as R_xlen_t);
            if s.is_null() || s == crate::sexp::globals::R_NaString() {
                continue;
            }
            let cs = CHAR(s);
            if cs.is_null() {
                continue;
            }
            let current = std::ffi::CStr::from_ptr(cs);
            if current.to_bytes() == name.as_bytes() {
                return Some(VECTOR_ELT(obj, i as R_xlen_t));
            }
        }
        None
    }
}

/// Slot name as text: a symbol's print name, or a non-NA scalar string's
/// first element; `None` for anything else (mirrors the R_SLOT_INIT
/// acceptance test in upstream's R_do_slot / do_AT).
pub(crate) unsafe fn name_of(name: SEXP) -> Option<String> {
    unsafe {
        if name.is_null() || name == R_NilValue() {
            return None;
        }
        if TYPEOF(name) == SEXPTYPE::SYMSXP {
            let chars = CHAR(PRINTNAME(name));
            if chars.is_null() {
                return Some(String::new());
            }
            return Some(
                std::ffi::CStr::from_ptr(chars)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        if TYPEOF(name) == SEXPTYPE::STRSXP
            && LENGTH(name) == 1
            && STRING_ELT(name, 0) != crate::sexp::globals::R_NaString()
        {
            let chars = crate::sexp::accessors::translateChar(STRING_ELT(name, 0));
            if chars.is_null() {
                return Some(String::new());
            }
            return Some(
                std::ffi::CStr::from_ptr(chars)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        None
    }
}

/// The class name do_AT reports for a non-S4 object: the class
/// attribute's first element when present, else the computed implicit
/// class (main/attrib.c uses R_data_class for that fallback).
pub(crate) unsafe fn first_class_display(obj: SEXP) -> String {
    unsafe {
        let klass =
            crate::sexp::attrib_core::getAttrib(obj, crate::sexp::attrib_core::R_ClassSymbol());
        if !klass.is_null()
            && klass != R_NilValue()
            && TYPEOF(klass) == SEXPTYPE::STRSXP
            && LENGTH(klass) > 0
        {
            let cs = STRING_ELT(klass, 0);
            if !cs.is_null() {
                let chars = crate::sexp::accessors::translateChar(cs);
                if !chars.is_null() {
                    return std::ffi::CStr::from_ptr(chars)
                        .to_string_lossy()
                        .into_owned();
                }
            }
            return String::new();
        }
        // Implicit class (R_data_class): NILSXP is "NULL", closures are
        // "function", pairlists and VECSXP are "list", etc.
        let implicit = match TYPEOF(obj) {
            t if t == SEXPTYPE::NILSXP => "NULL",
            t if t == SEXPTYPE::LGLSXP => "logical",
            t if t == SEXPTYPE::INTSXP => "integer",
            t if t == SEXPTYPE::REALSXP => "numeric",
            t if t == SEXPTYPE::CPLXSXP => "complex",
            t if t == SEXPTYPE::STRSXP => "character",
            t if t == SEXPTYPE::RAWSXP => "raw",
            t if t == SEXPTYPE::LISTSXP => "list",
            t if t == SEXPTYPE::VECSXP => "list",
            t if t == SEXPTYPE::CLOSXP => "function",
            t if t == SEXPTYPE::SPECIALSXP => "function",
            t if t == SEXPTYPE::BUILTINSXP => "function",
            _ => return String::new(),
        };
        implicit.to_string()
    }
}

/// Raise upstream's R_do_slot miss errors: the class attribute, when
/// present, produces "no slot of name ... for this object of class ...";
/// otherwise the TYPEOF-based "cannot get a slot ..." message.
unsafe fn raise_slot_miss(obj: SEXP, name: &str) -> ! {
    unsafe {
        let class_string =
            crate::sexp::attrib_core::getAttrib(obj, crate::sexp::attrib_core::R_ClassSymbol());
        let has_class = !class_string.is_null()
            && class_string != R_NilValue()
            && TYPEOF(class_string) == SEXPTYPE::STRSXP
            && LENGTH(class_string) > 0;
        let msg = if has_class {
            let cs = STRING_ELT(class_string, 0);
            let class_str = if cs.is_null() {
                String::new()
            } else {
                let chars = crate::sexp::accessors::translateChar(cs);
                if chars.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(chars)
                        .to_string_lossy()
                        .into_owned()
                }
            };
            format!(
                "no slot of name \"{}\" for this object of class \"{}\"",
                name, class_str
            )
        } else {
            let type_ptr =
                crate::mainutils::printvector::type2str_nowarn(TYPEOF(obj) as std::os::raw::c_int);
            let type_str = std::ffi::CStr::from_ptr(type_ptr).to_string_lossy();
            format!(
                "cannot get a slot (\"{}\") from an object of type \"{}\"",
                name, type_str
            )
        };
        std::panic::panic_any(RError { message: msg })
    }
}

/// GNU `pseudo_NULL` (`attrib.c`): interned `\001NULL\001`. Attributes
/// cannot store `R_NilValue`, so NULL slots use this sentinel. Pointer
/// identity is enough because `Rf_install` interns.
unsafe fn slot_pseudo_null() -> SEXP {
    unsafe { Rf_install(c"\x01NULL\x01".as_ptr()) }
}

unsafe fn unmap_slot_pseudo_null(value: SEXP) -> SEXP {
    unsafe {
        if !value.is_null() && value == slot_pseudo_null() {
            R_NilValue()
        } else {
            value
        }
    }
}

/// R_do_slot (main/attrib.c) — shared accessor behind `slot()` and `@`.
///
/// The name must be a symbol or a non-NA scalar string. Attribute-stored
/// slots satisfy the lookup first, then port S4 named-element slots;
/// `.Data` resolves through the data part, `.S3Class` defaults to the
/// computed class. Missing slots raise upstream's exact error messages.
pub unsafe fn R_do_slot(obj: SEXP, name: SEXP) -> SEXP {
    unsafe {
        // R_SLOT_INIT: symbol or non-NA scalar string, else upstream's
        // "invalid type or length for slot name".
        let name_str = if !name.is_null() && TYPEOF(name) == SEXPTYPE::SYMSXP {
            let chars = CHAR(PRINTNAME(name));
            if chars.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(chars)
                    .to_string_lossy()
                    .into_owned()
            }
        } else if !name.is_null()
            && TYPEOF(name) == SEXPTYPE::STRSXP
            && LENGTH(name) == 1
            && STRING_ELT(name, 0) != crate::sexp::globals::R_NaString()
        {
            let chars = crate::sexp::accessors::translateChar(STRING_ELT(name, 0));
            if chars.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(chars)
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            let msg = "invalid type or length for slot name".to_string();
            std::panic::panic_any(RError { message: msg });
        };

        if name_str == ".Data" {
            return R_data_part(obj);
        }

        if crate::mainutils::coerce::IS_S4_OBJECT(obj) != FALSE {
            // Port S4: slots live as named vector elements.
            if let Some(value) = s4_named_slot(obj, &name_str) {
                return unmap_slot_pseudo_null(value);
            }
            // Upstream's only other storage is attributes; "names" never
            // resolves for S4 objects (S4SXP in upstream, and the names
            // attribute here merely lists the slot names).
            if name_str != "names" {
                let name_sym =
                    Rf_install(CString::new(name_str.as_str()).unwrap_or_default().as_ptr());
                let value = crate::sexp::attrib_core::getAttrib(obj, name_sym);
                if !value.is_null() && value != R_NilValue() {
                    return unmap_slot_pseudo_null(value);
                }
            }
            if name_str == ".S3Class" {
                return crate::eval::attrib_core::R_data_class(obj);
            }
            raise_slot_miss(obj, &name_str);
        }

        // Non-S4: upstream stores everything in attributes.
        let name_sym = Rf_install(CString::new(name_str.as_str()).unwrap_or_default().as_ptr());
        let value = crate::sexp::attrib_core::getAttrib(obj, name_sym);
        if !value.is_null() && value != R_NilValue() {
            return unmap_slot_pseudo_null(value);
        }
        if name_str == ".S3Class" {
            return crate::eval::attrib_core::R_data_class(obj);
        }
        if name_str == "names" && TYPEOF(obj) == SEXPTYPE::VECSXP {
            // needed for the namedList class
            return R_NilValue();
        }
        raise_slot_miss(obj, &name_str);
    }
}

/// GNU `R_has_slot` (attrib.c) — attribute presence, including NULL sentinels.
pub unsafe fn R_has_slot(obj: SEXP, name: SEXP) -> SEXP {
    unsafe {
        let name_sym = if !name.is_null() && TYPEOF(name) == SEXPTYPE::SYMSXP {
            name
        } else if !name.is_null() && TYPEOF(name) == SEXPTYPE::STRSXP && LENGTH(name) == 1 {
            let chars = STRING_ELT(name, 0);
            if chars.is_null() {
                std::panic::panic_any(RError {
                    message: "invalid type or length for slot name".to_string(),
                });
            }
            Rf_install(crate::sexp::accessors::CHAR(chars))
        } else {
            std::panic::panic_any(RError {
                message: "invalid type or length for slot name".to_string(),
            });
        };
        let name_str = elt_to_string(name_sym, 0);
        if name_str == ".Data" && TYPEOF(obj) != SEXPTYPE::OBJSXP {
            return Rf_ScalarLogical(TRUE);
        }
        let value = crate::sexp::attrib_core::getAttrib(obj, name_sym);
        Rf_ScalarLogical(if !value.is_null() && value != R_NilValue() {
            TRUE
        } else {
            FALSE
        })
    }
}

/// GNU `R_do_slot_assign` — store a slot as an attribute.
///
/// S4 objects (including `new("classRepresentation")`) keep slots in
/// ATTRIB, not as VECSXP names. `NULL` is stored as GNU `pseudo_NULL`.
pub unsafe fn R_do_slot_assign(obj: SEXP, name: SEXP, value: SEXP) -> SEXP {
    unsafe {
        if obj.is_null() || obj == R_NilValue() {
            std::panic::panic_any(RError {
                message: "attempt to set slot on NULL object".to_string(),
            });
        }
        let name_sym = if !name.is_null() && TYPEOF(name) == SEXPTYPE::SYMSXP {
            name
        } else if !name.is_null() && TYPEOF(name) == SEXPTYPE::STRSXP && LENGTH(name) == 1 {
            let chars = STRING_ELT(name, 0);
            if chars.is_null() {
                std::panic::panic_any(RError {
                    message: "invalid type or length for slot name".to_string(),
                });
            }
            Rf_install(crate::sexp::accessors::CHAR(chars))
        } else {
            std::panic::panic_any(RError {
                message: "invalid type or length for slot name".to_string(),
            });
        };
        let name_str = elt_to_string(name_sym, 0);
        if name_str == ".Data" {
            // GNU setDataPart on a function S4 object (.mergeAttrs): the
            // closure IS the object. Copy formals/body/env from value.
            if TYPEOF(obj) == SEXPTYPE::CLOSXP && TYPEOF(value) == SEXPTYPE::CLOSXP {
                crate::sexp::accessors::SET_FORMALS(obj, crate::sexp::accessors::FORMALS(value));
                crate::sexp::accessors::SET_BODY(obj, crate::sexp::accessors::BODY(value));
                crate::sexp::accessors::SET_CLOENV(obj, crate::sexp::accessors::CLOENV(value));
                return obj;
            }
            crate::sexp::attrib_core::setAttrib(obj, name_sym, value);
            return obj;
        }

        // GNU stores NULL slots as a sentinel; setAttrib would drop them.
        let stored = if value.is_null() || value == R_NilValue() {
            slot_pseudo_null()
        } else {
            value
        };
        crate::sexp::attrib_core::setAttrib(obj, name_sym, stored);
        obj
    }
}


/// R's `set_slot(object, name, value)` — set the value of a slot.
pub unsafe fn do_set_slot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object = CAR(args);
        let name_arg = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if object.is_null()
            || object == R_NilValue()
            || name_arg.is_null()
            || name_arg == R_NilValue()
        {
            return object;
        }
        R_do_slot_assign(object, name_arg, value)
    }
}


/// R's `extends(class1, class2)` — check if class1 extends class2.
pub unsafe fn do_extends(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class1_arg = CAR(args);
        let class2_arg = CAR(CDR(args));
        if class1_arg.is_null() || class2_arg.is_null() {
            return Rf_ScalarLogical(FALSE);
        }
        let class1 = elt_to_string(class1_arg, 0);
        let class2 = elt_to_string(class2_arg, 0);
        if class1 == class2 {
            return Rf_ScalarLogical(TRUE);
        }
        if crate::mainutils::objects::s4_class_extends(&class1, &class2) {
            return Rf_ScalarLogical(TRUE);
        }
        let extends = match class1.as_str() {
            "numeric" | "double" => class2 == "vector" || class2 == "atomic",
            "integer" => class2 == "numeric" || class2 == "vector" || class2 == "atomic",
            "logical" => class2 == "vector" || class2 == "atomic",
            "character" => class2 == "vector" || class2 == "atomic",
            "complex" => class2 == "vector" || class2 == "atomic",
            "matrix" => class2 == "array",
            "data.frame" => class2 == "list",
            "factor" => class2 == "integer" || class2 == "vector" || class2 == "atomic",
            "ordered" => class2 == "factor" || class2 == "integer",
            _ => false,
        };
        Rf_ScalarLogical(if extends { TRUE } else { FALSE })
    }
}

/// R's `isSealedClass(Class)` — check if a class is sealed.
pub unsafe fn do_isSealedClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Built-in types are always sealed
        Rf_ScalarLogical(TRUE)
    }
}

/// R's `sealClass(Class, ...)` — seal a class definition.
pub unsafe fn do_sealClass(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // No-op in simplified implementation
        R_NilValue()
    }
}

/// R's `representation(...)` — define class representation.
pub unsafe fn do_representation(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Collect named args as slot name = type pairs
        let n_list = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        if n_list.is_null() {
            return R_NilValue();
        }
        let _p = protect(n_list);
        // Count args
        let mut count: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            count += 1;
            current = CDR(current);
        }
        if count == 0 {
            return n_list;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, count);
        if result.is_null() {
            return R_NilValue();
        }
        let rp = protect(result);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, count);
        let np = protect(names);
        let mut idx: R_xlen_t = 0;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            let tag = (*current).data.listsxp.tagval;
            let slot_name = if !tag.is_null() && tag != R_NilValue() {
                let sym_str = crate::sexp::accessors::CHAR(tag);
                if !sym_str.is_null() {
                    std::ffi::CStr::from_ptr(sym_str)
                        .to_str()
                        .unwrap_or("")
                        .to_string()
                } else {
                    format!("slot{}", idx + 1)
                }
            } else {
                format!("slot{}", idx + 1)
            };
            crate::sexp::accessors::SET_VECTOR_ELT(result, idx, arg);
            let cstr = CString::new(slot_name.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data.add(idx as usize) = charsxp;
            }
            idx += 1;
            current = CDR(current);
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        result
    }
}

/// R's `containsClass(class1, class2)` — check class containment.
pub unsafe fn do_containsClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Delegates to extends
        do_extends(_call, _op, args, _rho)
    }
}

/// R's `possibleExtends(class1, class2)` — check possible extensions.
pub unsafe fn do_possibleExtends(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: delegates to extends
        do_extends(_call, _op, args, _rho)
    }
}

/// R's `setReplaceMethod(f, signature, definition)` — set replace method.
pub unsafe fn do_setReplaceMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: return the definition
        let definition = CAR(CDR(CDR(args)));
        if !definition.is_null() && definition != R_NilValue() {
            definition
        } else {
            R_NilValue()
        }
    }
}

/// R's `getMethod(f, signature)` — get a specific S4 method.
pub unsafe fn do_getMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: return the function name or NULL
        let f_arg = CAR(args);
        if f_arg.is_null() || f_arg == R_NilValue() {
            return R_NilValue();
        }
        f_arg
    }
}

/// R's `removeGeneric(f)` — remove a generic.
pub unsafe fn do_removeGeneric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `removeMethod(f, signature)` — remove a method.
pub unsafe fn do_removeMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let _sig = CAR(CDR(args));
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `isGeneric(f)` — check if f is a generic.
pub unsafe fn do_isGeneric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `isMethod(f, signature)` — check if method exists.
pub unsafe fn do_isMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let _sig = CAR(CDR(args));
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `findMethod(f, signature)` — find S4 method.
pub unsafe fn do_findMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let _sig = CAR(CDR(args));
        R_NilValue()
    }
}

/// R's `findMethods(f)` — find all methods for a generic.
pub unsafe fn do_findMethods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        if result.is_null() {
            R_NilValue()
        } else {
            result
        }
    }
}

/// R's `showMethods(f)` — show methods for a generic.
pub unsafe fn do_showMethods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        println!("No methods found");
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `getGenerics(where)` — get all generics.
pub unsafe fn do_getGenerics(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _where = CAR(args);
        Rf_allocVector3(SEXPTYPE::STRSXP, 0)
    }
}

/// R's `getMethods(f)` — get all methods for a generic.
pub unsafe fn do_getMethods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        if result.is_null() {
            R_NilValue()
        } else {
            result
        }
    }
}

/// R's `existsMethod(f, signature)` — check if method exists.
pub unsafe fn do_existsMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { s4_method_exists(args, _rho, false) }
}

/// R's `hasMethod(f, signature)` — check exact or inherited methods.
pub unsafe fn do_hasMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { s4_method_exists(args, _rho, true) }
}

unsafe fn s4_method_exists(args: SEXP, rho: SEXP, inherit: bool) -> SEXP {
    unsafe {
        let fail = |message: &str| -> ! {
            std::panic::panic_any(RError {
                message: message.into(),
            })
        };
        let f = arg_by_name_or_position(args, &["f"], 0);
        let signature = arg_by_name_or_position(args, &["signature"], 1);
        let where_arg = arg_by_name_or_position(args, &["where"], 2);
        let env = if where_arg.is_null() || where_arg == R_NilValue() {
            rho
        } else {
            where_arg
        };
        if TYPEOF(env) != SEXPTYPE::ENVSXP {
            fail("'where' must be an environment")
        }
        let mut generic = if TYPEOF(f) == SEXPTYPE::CLOSXP {
            f
        } else if !inherit && TYPEOF(f) == SEXPTYPE::SYMSXP {
            crate::sexp::envir::R_findVar(f, env)
        } else if TYPEOF(f) == SEXPTYPE::STRSXP && LENGTH(f) == 1 {
            crate::sexp::envir::R_findVar(Rf_install(CHAR(STRING_ELT(f, 0))), env)
        } else {
            fail("invalid generic function specification")
        };
        if generic == crate::sexp::globals::R_UnboundValue() || generic == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        if TYPEOF(generic) == SEXPTYPE::PROMSXP {
            generic = crate::sexp::envir::forcePromise(generic);
        }
        if TYPEOF(generic) != SEXPTYPE::CLOSXP {
            return Rf_ScalarLogical(FALSE);
        }
        let _generic_root = protect(generic);
        let generic_env = crate::sexp::accessors::CLOENV(generic);
        let table =
            crate::sexp::envir::R_findVarInFrame(generic_env, Rf_install(c".AllMTable".as_ptr()));
        let sigargs =
            crate::sexp::envir::R_findVarInFrame(generic_env, Rf_install(c".SigArgs".as_ptr()));
        if TYPEOF(table) != SEXPTYPE::ENVSXP || TYPEOF(sigargs) != SEXPTYPE::VECSXP {
            return Rf_ScalarLogical(FALSE);
        }
        if signature != R_NilValue()
            && !signature.is_null()
            && TYPEOF(signature) != SEXPTYPE::STRSXP
        {
            fail("invalid signature argument")
        }
        let mut targets = string_vector_values(signature);
        let n = LENGTH(sigargs) as usize;
        if targets.len() > n {
            fail("more elements in the method signature than in the generic signature")
        }
        if !inherit && signature != R_NilValue() {
            let names =
                crate::sexp::attrib_core::getAttrib(signature, Rf_install(c"names".as_ptr()));
            if TYPEOF(names) == SEXPTYPE::STRSXP && LENGTH(names) == targets.len() as i32 {
                let mut reordered = vec!["ANY".to_string(); n];
                let mut seen = vec![false; n];
                for (i, value) in targets.into_iter().enumerate() {
                    let name = elt_to_string(names, i as R_xlen_t);
                    let index = (0..n)
                        .find(|j| elt_to_string(VECTOR_ELT(sigargs, *j as R_xlen_t), 0) == name)
                        .unwrap_or_else(|| {
                            fail("named signature does not match generic arguments")
                        });
                    if seen[index] {
                        fail("duplicate names in signature")
                    }
                    seen[index] = true;
                    reordered[index] = value;
                }
                targets = reordered;
            }
        }
        targets.resize(n, "ANY".to_string());
        let exists = crate::library::methods::methods_list_dispatch::select_method_by_signature(
            table,
            &targets,
            &vec![inherit; n],
        )
        .is_some();
        Rf_ScalarLogical(i32::from(exists))
    }
}

/// R's `selectMethod(f, signature)` — select method for generic.
pub unsafe fn do_selectMethod(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fail = |message: &str| -> ! {
            std::panic::panic_any(RError {
                message: message.into(),
            })
        };
        let f = arg_by_name_or_position(args, &["f"], 0);
        let signature = arg_by_name_or_position(args, &["signature"], 1);
        let optional = arg_by_name_or_position(args, &["optional"], 2);
        let inherited = arg_by_name_or_position(args, &["useInherited"], 3);
        let fdef = arg_by_name_or_position(args, &["fdef"], 5);
        let generic = if fdef != R_NilValue() && !fdef.is_null() {
            fdef
        } else if TYPEOF(f) == SEXPTYPE::CLOSXP {
            f
        } else if TYPEOF(f) == SEXPTYPE::STRSXP && LENGTH(f) == 1 {
            crate::sexp::envir::findFun(Rf_install(CHAR(STRING_ELT(f, 0))), rho)
        } else {
            fail("invalid generic function specification")
        };
        if TYPEOF(generic) != SEXPTYPE::CLOSXP {
            fail("no generic function found")
        }
        let _generic_root = protect(generic);
        let env = crate::sexp::accessors::CLOENV(generic);
        let table = crate::sexp::envir::R_findVarInFrame(env, Rf_install(c".AllMTable".as_ptr()));
        let sigargs = crate::sexp::envir::R_findVarInFrame(env, Rf_install(c".SigArgs".as_ptr()));
        if TYPEOF(table) != SEXPTYPE::ENVSXP || TYPEOF(sigargs) != SEXPTYPE::VECSXP {
            fail("no generic function found")
        }
        if TYPEOF(signature) != SEXPTYPE::STRSXP {
            fail("invalid signature argument")
        }
        let n = LENGTH(sigargs) as usize;
        let supplied = string_vector_values(signature);
        if supplied.len() > n {
            fail("more elements in the method signature than in the generic signature")
        }
        // GNU selectMethod consumes signatures in generic order, even if named.
        let mut targets = supplied;
        targets.resize(n, "ANY".to_string());
        let mut inherit = vec![true; n];
        if inherited != R_NilValue() && !inherited.is_null() {
            if TYPEOF(inherited) != SEXPTYPE::LGLSXP
                || ![1, n].contains(&(LENGTH(inherited) as usize))
            {
                fail("invalid useInherited argument")
            }
            for (i, value) in inherit.iter_mut().enumerate() {
                let logical = LOGICAL_ELT(inherited, (i % LENGTH(inherited) as usize) as i32);
                if logical == i32::MIN {
                    fail("invalid useInherited argument")
                }
                *value = logical != 0;
            }
        }
        let selected = crate::library::methods::methods_list_dispatch::select_method_by_signature(
            table, &targets, &inherit,
        );
        let Some((method, defined)) = selected else {
            if optional != R_NilValue() && crate::mainutils::coerce::asLogical(optional) == TRUE {
                return R_NilValue();
            }
            fail("no method found for this signature")
        };
        let result = crate::mainutils::duplicate::Rf_duplicate(method);
        let _result_root = protect(result);
        for (name, values) in [("target", targets), ("defined", defined)] {
            let value = string_vector_from_values(&values);
            let _value_root = protect(value);
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new(name).unwrap().as_ptr()),
                value,
            );
        }
        let class = Rf_mkString(c"MethodDefinition".as_ptr());
        let _class_root = protect(class);
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class);
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c".Data".as_ptr()), method);
        crate::sexp::accessors::SET_S4_OBJECT(result);
        result
    }
}
