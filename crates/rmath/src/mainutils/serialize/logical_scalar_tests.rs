use super::*;

#[test]
fn decoded_plain_logicals_share_only_canonical_singleton_representations() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        for value in [0, 1, crate::sexp::ffi::NA_LOGICAL, 2] {
            let source = Rf_allocVector3(SEXPTYPE::LGLSXP, 1);
            let _source = protect(source);
            SET_LOGICAL_ELT(source, 0, value);
            let wire = R_serialize(
                source,
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
            );
            let _wire = protect(wire);
            let decoded = R_unserialize(wire, R_NilValue());
            assert_eq!(LOGICAL_ELT(decoded, 0), value);
            if value == 2 {
                assert_ne!(decoded, Rf_ScalarLogical(1));
            } else {
                assert_eq!(decoded, Rf_ScalarLogical(value));
            }
        }
    });
}

#[test]
fn decoded_logical_metadata_and_attributes_never_modify_shared_singletons() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        for metadata in [false, true] {
            let source = Rf_allocVector3(SEXPTYPE::LGLSXP, 1);
            let _source = protect(source);
            SET_LOGICAL_ELT(source, 0, 1);
            if metadata {
                SETLEVELS(source, 1);
                SET_OBJECT(source, 1);
            } else {
                let names = Rf_mkString(c"kept".as_ptr());
                setAttrib(source, R_NamesSymbol(), names);
            }
            let wire = R_serialize(
                source,
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
            );
            let _wire = protect(wire);
            let decoded = R_unserialize(wire, R_NilValue());
            let _decoded = protect(decoded);
            assert_ne!(decoded, Rf_ScalarLogical(1));
            assert_eq!(LOGICAL_ELT(decoded, 0), 1);
            if metadata {
                assert_eq!(LEVELS(decoded), 1);
                assert_eq!(OBJECT(decoded), 1);
            } else {
                let names = crate::eval::attrib_core::getAttrib(decoded, R_NamesSymbol());
                assert_eq!(CStr::from_ptr(CHAR(STRING_ELT(names, 0))).to_bytes(), b"kept");
            }
            let attributes = ATTRIB(Rf_ScalarLogical(1));
            assert!(attributes.is_null() || attributes == R_NilValue());
            assert_eq!(LEVELS(Rf_ScalarLogical(1)), 0);
            assert_eq!(OBJECT(Rf_ScalarLogical(1)), 0);
        }
    });
}
