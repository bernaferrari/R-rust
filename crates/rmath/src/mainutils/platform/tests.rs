use super::*;
use super::*;
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};
use std::ffi::CStr;
#[test]
fn setlocale_denies_mutation_by_default() {
    let session = crate::sexp::session::RSession::new();
    let mutation = session.with_active(|| unsafe {
        let category = crate::sexp::constructors::Rf_ScalarInteger(1);
        let _category = crate::sexp::protect::protect(category);
        let denied_locale = crate::sexp::constructors::Rf_mkString(c"C".as_ptr());
        let _locale = crate::sexp::protect::protect(denied_locale);
        let tail =
            crate::sexp::constructors::Rf_cons(denied_locale, crate::sexp::globals::R_NilValue());
        let _tail = crate::sexp::protect::protect(tail);
        let denied_args = crate::sexp::constructors::Rf_cons(category, tail);
        let _args = crate::sexp::protect::protect(denied_args);
        do_setlocale(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            denied_args,
            std::ptr::null_mut(),
        )
    });
    unsafe {
        assert_eq!(crate::sexp::accessors::TYPEOF(mutation), SEXPTYPE::STRSXP);
        assert_eq!(crate::sexp::accessors::XLENGTH(mutation), 1);
        assert_eq!(
            CStr::from_ptr(crate::sexp::accessors::CHAR(
                crate::sexp::accessors::STRING_ELT(mutation, 0),
            ))
            .to_bytes(),
            b""
        );
    }
    let mut public_session = crate::sexp::session::RSession::new();
    let (mutation, output, _) = public_session
        .eval_script_with_output_capture("cat(typeof(Sys.setlocale(\"LC_ALL\", \"C\")))");
    mutation.expect("public Sys.setlocale should return a failure string");
    assert_eq!(output.stdout, "character");
    let (empty, empty_output, _) = public_session
        .eval_script_with_output_capture("cat(nchar(Sys.setlocale(\"LC_ALL\", \"\")))");
    empty.expect("empty locale also requests a mutation");
    assert_eq!(empty_output.stdout, "0");
    let (query, query_output, _) =
        public_session.eval_script_with_output_capture("cat(nchar(Sys.getlocale(\"LC_ALL\")) > 0)");
    query.expect("public Sys.getlocale should remain available");
    assert_eq!(query_output.stdout, "TRUE");
}
#[test]
fn platform_scratch_buffers_are_session_local() {
    unsafe {
        let mut first = RInstance::new();
        set_current_instance(&mut first);
        let first_date = R_Date();
        R_check_locale();
        let first_encoding = R_nativeEncoding();
        assert_eq!(CStr::from_ptr(first_encoding).to_bytes(), b"UTF-8");
        let mut second = RInstance::new();
        set_current_instance(&mut second);
        let second_date = R_Date();
        let second_encoding_before = R_nativeEncoding();
        assert_eq!(CStr::from_ptr(second_encoding_before).to_bytes(), b"");
        R_check_locale();
        let second_encoding = R_nativeEncoding();
        assert_eq!(CStr::from_ptr(second_encoding).to_bytes(), b"UTF-8");
        assert_ne!(first_date, second_date);
        assert_ne!(first_encoding, second_encoding);
        set_current_instance(&mut first);
        assert_eq!(CStr::from_ptr(R_nativeEncoding()).to_bytes(), b"UTF-8");
        clear_current_instance();
    }
}
#[test]
fn trunk_r90451_dircreate_rejects_a_path_past_path_max() {
    let session = crate::sexp::session::RSession::new();
    session.with_active(|| unsafe {
        let too_long = "a".repeat(libc::PATH_MAX as usize);
        let bytes = std::ffi::CString::new(too_long).unwrap();
        let path = crate::sexp::constructors::Rf_mkString(bytes.as_ptr());
        let args = crate::sexp::constructors::Rf_cons(path, crate::sexp::globals::R_NilValue());
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            do_dircreate(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
        }));
        let message = err
            .expect_err("path longer than PATH_MAX must error")
            .downcast::<crate::sexp::context::RError>()
            .expect("R error")
            .message;
        assert!(message.contains("invalid 'path' argument"), "{message}");
    });
}
