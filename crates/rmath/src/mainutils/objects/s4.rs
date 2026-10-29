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

/// Resolve the original methods class with owning roots through evaluation.
pub(crate) fn make_class_by_name(name: &str) -> crate::sexp::object::Sexp<'static> {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|failure| error(&failure.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|failure| error(&failure.to_string()));
        let factory = owner.node_factory();
        let namespace = if let Some(pointer) =
            crate::mainutils::essentials::cached_namespace_by_name("methods")
        {
            factory
                .wrap(pointer)
                .unwrap_or_else(|failure| error(&failure.to_string()))
        } else {
            crate::library::methods::portable::namespace().unwrap_or_else(|failure| error(&failure))
        };
        factory
            .require_active()
            .unwrap_or_else(|failure| error(&failure.to_string()));
        let function = factory
            .wrap(Rf_install(c"getClass".as_ptr()))
            .unwrap_or_else(|failure| error(&failure.to_string()));
        let name = factory
            .strings(&[name])
            .unwrap_or_else(|failure| error(&failure.to_string()));
        let nil = factory.nil();
        let arguments = factory
            .pairlist_cell(&name, &nil, &nil)
            .unwrap_or_else(|failure| error(&failure.to_string()));
        let call = factory
            .call(&function, &arguments)
            .unwrap_or_else(|failure| error(&failure.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|failure| error(&failure.to_string()));
        let result = Rf_eval(call.as_raw(), namespace.as_raw());
        if let Some(pin) = &pin {
            pin.require_live()
                .unwrap_or_else(|failure| error(&failure.to_string()));
        }
        owner
            .require_active()
            .unwrap_or_else(|failure| error(&failure.to_string()));
        factory
            .wrap(result)
            .and_then(|value| value.into_owned())
            .unwrap_or_else(|failure| error(&failure.to_string()))
    }
}

pub unsafe fn R_do_new_object(class_def: SEXP) -> SEXP {
    unsafe {
        if class_def.is_null() {
            error("C level NEW macro called with null class definition pointer");
        }

        // GNU objects.c reads the classRepresentation slots even for an
        // invalid R value. R NULL is distinct from a null native pointer.
        let virtual_sym = crate::sexp::symbol::Rf_install(c"virtual".as_ptr());
        let prototype_sym = crate::sexp::symbol::Rf_install(c"prototype".as_ptr());
        let class_name_sym = crate::sexp::symbol::Rf_install(c"className".as_ptr());
        let virtual_flag = crate::mainutils::essentials::R_do_slot(class_def, virtual_sym);
        if crate::mainutils::coerce::asLogical(virtual_flag) != 0 {
            let name = crate::mainutils::essentials::R_do_slot(class_def, class_name_sym);
            let shown = sexp_to_string(name).unwrap_or_else(|| "<unknown>".to_string());
            error(&format!(
                "trying to generate an object from a virtual class (\"{shown}\")"
            ));
        }
        let class_name = crate::mainutils::essentials::R_do_slot(class_def, class_name_sym);
        let _class_name_guard = protect(class_name);
        let prototype = crate::mainutils::essentials::R_do_slot(class_def, prototype_sym);

        let value = crate::mainutils::duplicate::duplicate(prototype);
        let _value_guard = protect(value);
        let x_data = TYPEOF(value) == SEXPTYPE::ENVSXP
            || TYPEOF(value) == SEXPTYPE::SYMSXP
            || TYPEOF(value) == SEXPTYPE::EXTPTRSXP;
        let package_sym = crate::sexp::symbol::Rf_install(c"package".as_ptr());
        let has_package = {
            let pkg = crate::sexp::attrib_core::getAttrib(class_name, package_sym);
            !pkg.is_null() && pkg != R_NilValue()
        };
        if (TYPEOF(value) == SEXPTYPE::S4SXP || has_package) && !x_data {
            crate::sexp::attrib_core::setAttrib(
                value,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class_name,
            );
            SET_S4_OBJECT(value);
        }
        value
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
        let owner = crate::sexp::owner::OwnerToken::current().expect("active asS4 owner");
        let pin = owner.pin().expect("live asS4 owner");
        let factory = owner.node_factory();
        let _source_root = factory.wrap(s).expect("asS4 input belongs to active owner");
        if flag == IS_S4_OBJECT(s) {
            return s;
        }
        let _s_guard = protect(s);
        // GNU asS4: MAYBE_SHARED → shallow_duplicate so SET_S4_OBJECT
        // does not flip the bit on the caller's binding (reg-S4.R asS4(m)
        // must leave m non-S4 for identical(m, f@.Data)).
        let s = crate::mainutils::duplicate::shallow_duplicate_if_shared(s);
        if let Some(pin) = &pin {
            pin.require_live().expect("live asS4 owner after duplicate");
        }
        factory
            .require_active()
            .expect("original asS4 owner after duplicate");
        let _result = factory
            .wrap(s)
            .expect("asS4 duplicate belongs to original owner");
        let _dup = protect(s);

        if flag != FALSE {
            SET_S4_OBJECT(s);
        } else {
            if complete != FALSE {
                let value = crate::mainutils::subassign::R_getS4DataSlot(s, SEXPTYPE::ANYSXP.into());
                if value != R_NilValue() && IS_S4_OBJECT(value) == FALSE {
                    return value;
                }
                if complete == 1 {
                    let klass = R_data_class(s);
                    if let Some(pin) = &pin {
                        pin.require_live()
                            .expect("live asS4 owner after class lookup");
                    }
                    factory
                        .require_active()
                        .expect("original asS4 owner after class lookup");
                    let _class_root = factory
                        .wrap(klass)
                        .expect("asS4 class belongs to original owner");
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
