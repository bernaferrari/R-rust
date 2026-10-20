/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/utils.c
 *
 *  methods package utility functions.
 */

use crate::mainutils::objects::class_name::AdmittedClassName;

#[cfg(test)]
use std::ffi::CStr;

use crate::sexp::accessors::{
    CHAR, COMPLEX_ELT, INTEGER_ELT, LOGICAL_ELT, PRINTNAME, REAL_ELT, STRING_ELT, TYPEOF, XLENGTH,
};
use crate::sexp::ffi::{SEXP, SEXPTYPE};

unsafe fn class_name_owned(class_name: SEXP) -> AdmittedClassName {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|failure| crate::sexp::context::r_error(failure.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|failure| crate::sexp::context::r_error(failure.to_string()));
        let value = owner
            .sexp(class_name)
            .and_then(|v| v.into_owned())
            .unwrap_or_else(|failure| crate::sexp::context::r_error(failure.to_string()));
        // Match GNU tests.c: coerce through asChar, including symbols, empty
        // values and scalars. Keep both original and converted nodes rooted.
        let numeric = matches!(
            value.typeof_(),
            SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::CPLXSXP
        );
        // Bound the scalar formatting temporaries before native conversion.
        let _numeric_workspace = if numeric {
            Some(
                owner
                    .with_arena(|arena| arena.reserve_workspace(8192))
                    .unwrap_or_else(|failure| crate::sexp::context::r_error(failure.to_string()))
                    .unwrap_or_else(|| {
                        crate::sexp::context::r_error(
                            "class name workspace exceeds session memory limit",
                        )
                    }),
            )
        } else {
            None
        };
        let chars = class_name_chars(value.as_raw(), owner);
        owner
            .require_active()
            .unwrap_or_else(|failure| crate::sexp::context::r_error(failure.to_string()));
        let chars = owner
            .sexp(chars)
            .and_then(|v| v.into_owned())
            .unwrap_or_else(|failure| crate::sexp::context::r_error(failure.to_string()));
        let bytes = std::ffi::CStr::from_ptr(CHAR(chars.as_raw())).to_bytes();
        AdmittedClassName::from_bytes(owner, bytes)
            .unwrap_or_else(|failure| crate::sexp::context::r_error(failure))
    }
}

// GNU util.c asChar: this is scalar coercion, without R-level dispatch.
unsafe fn class_name_chars(value: SEXP, owner: crate::sexp::owner::OwnerToken<'_>) -> SEXP {
    unsafe {
        let kind = SEXPTYPE(TYPEOF(value));
        if kind == SEXPTYPE::CHARSXP {
            return value;
        }
        if kind == SEXPTYPE::SYMSXP {
            return PRINTNAME(value);
        }
        if !matches!(
            kind,
            SEXPTYPE::STRSXP
                | SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
        ) {
            return crate::sexp::globals::R_NaString();
        }
        if XLENGTH(value) < 1 {
            return crate::sexp::globals::R_NaString();
        }
        match kind {
            SEXPTYPE::STRSXP => STRING_ELT(value, 0),
            SEXPTYPE::LGLSXP => crate::mainutils::coerce::StringFromLogical(LOGICAL_ELT(value, 0)),
            SEXPTYPE::INTSXP => crate::mainutils::coerce::StringFromInteger(
                INTEGER_ELT(value, 0),
                std::ptr::null_mut(),
            ),
            SEXPTYPE::REALSXP | SEXPTYPE::CPLXSXP => {
                crate::mainutils::print::PrintDefaults();
                let mark = crate::mainutils::options::GetOption(c"OutDec".as_ptr());
                let mark = owner
                    .sexp(mark)
                    .and_then(|v| v.into_owned())
                    .unwrap_or_else(|failure| crate::sexp::context::r_error(failure.to_string()));
                let decimal =
                    if TYPEOF(mark.as_raw()) == SEXPTYPE::STRSXP && XLENGTH(mark.as_raw()) > 0 {
                        CHAR(STRING_ELT(mark.as_raw(), 0))
                    } else {
                        c".".as_ptr()
                    };
                let (mut w, mut d, mut e) = (0, 0, 0);
                let text = if kind == SEXPTYPE::REALSXP {
                    let x = REAL_ELT(value, 0);
                    crate::mainutils::format::formatReal(&x, 1, &mut w, &mut d, &mut e, 0);
                    crate::mainutils::printutils::EncodeReal0(x, w, d, e, decimal)
                } else {
                    let x = COMPLEX_ELT(value, 0);
                    let (mut wi, mut di, mut ei) = (0, 0, 0);
                    crate::mainutils::format::formatComplex(
                        &x, 1, &mut w, &mut d, &mut e, &mut wi, &mut di, &mut ei, 0,
                    );
                    crate::mainutils::printutils::EncodeComplex(x, w, d, e, wi, di, ei, decimal)
                };
                crate::sexp::constructors::Rf_mkChar(text)
            }
            _ => crate::sexp::globals::R_NaString(),
        }
    }
}

/// R_methods_test_MAKE_CLASS - create a class definition for testing.
pub unsafe fn R_methods_test_MAKE_CLASS(className: SEXP) -> SEXP {
    unsafe {
        let class_name = class_name_owned(className);
        crate::mainutils::objects::make_class_by_name(class_name.as_str()).as_raw()
    }
}

/// R_methods_test_NEW - create a new object of a given class for testing.
pub unsafe fn R_methods_test_NEW(className: SEXP) -> SEXP {
    unsafe {
        let class_name = class_name_owned(className);
        let class_def = crate::mainutils::objects::make_class_by_name(class_name.as_str());
        crate::mainutils::objects::R_do_new_object(class_def.as_raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::attrib_core::{R_ClassSymbol, getAttrib};
    use crate::sexp::constructors::Rf_mkString;

    #[test]
    fn methods_class_name_copy_denial_releases_and_recovers() {
        use crate::sexp::memory::ArenaBudget;
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let name = owner.node_factory().strings(&["OwnedClassName"]).unwrap();
        let baseline = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        let budget = owner
            .with_arena(|a| {
                let old = a.budget();
                a.set_budget(ArenaBudget::new(
                    a.total_bytes_allocated() + baseline + 1,
                    0,
                ));
                old
            })
            .unwrap();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            class_name_owned(name.as_raw())
        }))
        .err()
        .expect("class name copy must be admitted before allocation");
        assert_eq!(
            failure
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            "class name workspace exceeds session memory limit"
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
        owner.with_arena(|a| a.set_budget(budget)).unwrap();
        let copy = unsafe { class_name_owned(name.as_raw()) };
        assert_eq!(copy.as_str(), "OwnedClassName");
        drop(copy);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
    }

    #[test]
    fn methods_class_name_copy_retains_original_memory_admission() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let name = owner.node_factory().strings(&["OwnedClassName"]).unwrap();
        let baseline = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
        let copy = unsafe { class_name_owned(name.as_raw()) };
        assert!(owner.with_arena(|arena| arena.workspace_bytes()).unwrap() >= baseline + 14);
        drop(copy);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            baseline
        );
    }

    #[test]
    fn methods_test_make_class_resolves_original_class_definition() {
        let mut session = crate::sexp::session::RSession::new();
        let (definition, _, _) = session.eval_script_with_output_capture(
            "setClass('UtilityClass', slots=c(x='numeric')); getClassDef('UtilityClass')",
        );
        let definition = definition.unwrap().into_owned().unwrap();
        unsafe {
            let class = Rf_mkString(c"UtilityClass".as_ptr());
            let class_def = R_methods_test_MAKE_CLASS(class);
            assert_eq!(session.sexp(class_def).unwrap(), definition);
        }
    }

    #[test]
    fn methods_test_new_creates_s4_object() {
        let mut session = crate::sexp::session::RSession::new();
        session
            .eval_script_with_output_capture(
                "setClass('UtilityObject', slots=c(x='numeric'), prototype=prototype(x=42))",
            )
            .0
            .unwrap();
        unsafe {
            let class = Rf_mkString(c"UtilityObject".as_ptr());
            let object = R_methods_test_NEW(class);

            assert!(!object.is_null());
            let class_attr = getAttrib(object, R_ClassSymbol());
            assert_eq!(TYPEOF(class_attr), SEXPTYPE::STRSXP);
            let first = STRING_ELT(class_attr, 0);
            assert_eq!(CStr::from_ptr(CHAR(first)).to_bytes(), b"UtilityObject");
        }
    }
}
