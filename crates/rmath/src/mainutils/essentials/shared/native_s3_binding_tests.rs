use crate::sexp::session::RSession;

#[test]
fn package_method_builtin_is_not_inherited_before_namespace_registration() {
    let session = RSession::new_without_default_packages();
    session.with_active(|| unsafe {
        assert!(crate::library::portable_package::is_namespace_s3_method_name(
            "predict.loess"
        ));
        assert!(
            super::cached_namespace_by_name("stats").is_none(),
            "the control must run before the stats namespace is loaded"
        );
        let method = crate::sexp::symbol::Rf_install(c"predict.loess".as_ptr());
        assert_eq!(
            crate::sexp::envir::R_findVar(method, crate::sexp::globals::R_GlobalEnv()),
            crate::sexp::globals::R_UnboundValue(),
            "the native fallback must not leak into inherited symbol lookup"
        );
        assert!(
            crate::eval::builtin::builtin_primitive_kind("predict.loess").is_some(),
            "the native implementation remains registered for S3 dispatch fallback"
        );
        let callable = crate::mainutils::objects::lookup_s3_method_symbol(
            method,
            crate::sexp::globals::R_GlobalEnv(),
            crate::sexp::globals::R_GlobalEnv(),
            crate::sexp::globals::R_BaseEnv(),
        );
        assert_ne!(callable, crate::sexp::globals::R_UnboundValue());
        assert!(
            matches!(
                crate::sexp::SEXPTYPE::from(crate::sexp::accessors::TYPEOF(callable)),
                crate::sexp::SEXPTYPE::BUILTINSXP
                    | crate::sexp::SEXPTYPE::SPECIALSXP
                    | crate::sexp::SEXPTYPE::CLOSXP
            ),
            "S3 dispatch must retain a callable native method fallback"
        );
    });
}
