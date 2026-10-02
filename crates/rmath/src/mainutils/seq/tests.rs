use crate::sexp::constructors::*;

use super::*;

/// Helper: create an integer vector with given values.
unsafe fn make_int_vec(vals: &[c_int]) -> SEXP {
    unsafe {
        let v = Rf_allocVector(INTSXP_VAL, vals.len() as c_int);
        let data = INTEGER(v);
        for (i, &val) in vals.iter().enumerate() {
            *data.add(i) = val;
        }
        v
    }
}

/// Helper: create a real vector with given values.
unsafe fn make_real_vec(vals: &[c_double]) -> SEXP {
    unsafe {
        let v = Rf_allocVector(REALSXP_VAL, vals.len() as c_int);
        let data = REAL(v);
        for (i, &val) in vals.iter().enumerate() {
            *data.add(i) = val;
        }
        v
    }
}

// -----------------------------------------------------------------------
// seq_colon tests
// -----------------------------------------------------------------------

#[test]
fn test_seq_colon_simple_int_range() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let call = ptr::null_mut();
        let ans = seq_colon(1.0, 5.0, call);
        assert!(!ans.is_null());
        assert_eq!(TYPEOF(ans), INTSXP_VAL);
        assert_eq!(LENGTH(ans), 5);
        let data = INTEGER(ans);
        for i in 0..5 {
            assert_eq!(*data.add(i), (i + 1) as c_int);
        }
    }
}

#[test]
fn test_seq_colon_descending() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let ans = seq_colon(5.0, 1.0, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(LENGTH(ans), 5);
        let data = INTEGER(ans);
        assert_eq!(*data.add(0), 5);
        assert_eq!(*data.add(4), 1);
    }
}

#[test]
fn test_seq_colon_single_element() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let ans = seq_colon(3.0, 3.0, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(LENGTH(ans), 1);
        let data = INTEGER(ans);
        assert_eq!(*data.add(0), 3);
    }
}

#[test]
fn test_seq_colon_real_range() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        // Non-integer values produce REALSXP
        let ans = seq_colon(1.5, 3.5, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(TYPEOF(ans), REALSXP_VAL);
        assert_eq!(LENGTH(ans), 3);
        let data = REAL(ans);
        assert!((*data.add(0) - 1.5).abs() < 1e-10);
        assert!((*data.add(1) - 2.5).abs() < 1e-10);
        assert!((*data.add(2) - 3.5).abs() < 1e-10);
    }
}

#[test]
fn test_seq_colon_descending_real() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let ans = seq_colon(3.5, 1.5, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(TYPEOF(ans), REALSXP_VAL);
        assert_eq!(LENGTH(ans), 3);
        let data = REAL(ans);
        assert!((*data.add(0) - 3.5).abs() < 1e-10);
        assert!((*data.add(2) - 1.5).abs() < 1e-10);
    }
}

#[test]
fn test_seq_colon_large_range_still_int() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        // Large range that fits in integer
        let ans = seq_colon(1.0, 100.0, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(LENGTH(ans), 100);
        let data = INTEGER(ans);
        assert_eq!(*data.add(0), 1);
        assert_eq!(*data.add(99), 100);
    }
}

// -----------------------------------------------------------------------
// rep3 tests
// -----------------------------------------------------------------------

#[test]
fn test_rep3_basic_integer() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let s = make_int_vec(&[1, 2, 3]);
        let ans = rep3(s, 3, 9); // repeat 3-element vector 3 times
        assert!(!ans.is_null());
        assert_eq!(TYPEOF(ans), INTSXP_VAL);
        assert_eq!(LENGTH(ans), 9);
        let data = INTEGER(ans);
        assert_eq!(*data.add(0), 1);
        assert_eq!(*data.add(1), 2);
        assert_eq!(*data.add(2), 3);
        assert_eq!(*data.add(3), 1);
        assert_eq!(*data.add(4), 2);
        assert_eq!(*data.add(5), 3);
        assert_eq!(*data.add(6), 1);
        assert_eq!(*data.add(7), 2);
        assert_eq!(*data.add(8), 3);
    }
}

#[test]
fn test_rep3_partial_cycle() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let s = make_int_vec(&[10, 20, 30]);
        let ans = rep3(s, 3, 5); // only 5 of the 6
        assert!(!ans.is_null());
        assert_eq!(LENGTH(ans), 5);
        let data = INTEGER(ans);
        assert_eq!(*data.add(0), 10);
        assert_eq!(*data.add(1), 20);
        assert_eq!(*data.add(2), 30);
        assert_eq!(*data.add(3), 10);
        assert_eq!(*data.add(4), 20);
    }
}

#[test]
fn test_rep3_real_vector() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let s = make_real_vec(&[1.5, 2.5]);
        let ans = rep3(s, 2, 4);
        assert!(!ans.is_null());
        assert_eq!(TYPEOF(ans), REALSXP_VAL);
        assert_eq!(LENGTH(ans), 4);
        let data = REAL(ans);
        assert!((*data.add(0) - 1.5).abs() < 1e-10);
        assert!((*data.add(1) - 2.5).abs() < 1e-10);
        assert!((*data.add(2) - 1.5).abs() < 1e-10);
        assert!((*data.add(3) - 2.5).abs() < 1e-10);
    }
}

#[test]
fn test_rep3_zero_length_output() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let s = make_int_vec(&[1, 2, 3]);
        let ans = rep3(s, 3, 0);
        assert!(!ans.is_null());
        assert_eq!(LENGTH(ans), 0);
    }
}

#[test]
fn test_rep3_unsupported_type_errors() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let extptr = crate::sexp::memory_ext::allocSExp(crate::sexp::ffi::SEXPTYPE::EXTPTRSXP);
        let err = std::panic::catch_unwind(|| {
            let _ = rep3(extptr, 1, 1);
        })
        .expect_err("unsupported rep3 type should raise an RError");
        let message = err
            .downcast_ref::<crate::sexp::context::RError>()
            .map(|err| err.message.as_str())
            .unwrap_or("");
        assert!(message.contains("rep3: unsupported SEXPTYPE"));
    }
}

// -----------------------------------------------------------------------
// rep2 tests
// -----------------------------------------------------------------------

#[test]
fn test_rep2_vector_times() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let s = make_int_vec(&[1, 2, 3]);
        let ncopy = make_int_vec(&[2, 1, 3]);
        let ans = rep2(s, ncopy);
        assert!(!ans.is_null());
        assert_eq!(TYPEOF(ans), INTSXP_VAL);
        assert_eq!(LENGTH(ans), 6); // 2 + 1 + 3
        let data = INTEGER(ans);
        assert_eq!(*data.add(0), 1);
        assert_eq!(*data.add(1), 1);
        assert_eq!(*data.add(2), 2);
        assert_eq!(*data.add(3), 3);
        assert_eq!(*data.add(4), 3);
        assert_eq!(*data.add(5), 3);
    }
}

// -----------------------------------------------------------------------
// do_seq_len tests
// -----------------------------------------------------------------------

#[test]
fn test_do_seq_len_simple() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let len_arg = make_int_vec(&[5]);
        let args = Rf_cons(len_arg, R_NilValue());
        let ans = do_seq_len(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(TYPEOF(ans), INTSXP_VAL);
        assert_eq!(LENGTH(ans), 5);
        let data = INTEGER(ans);
        assert_eq!(*data.add(0), 1);
        assert_eq!(*data.add(4), 5);
    }
}

#[test]
fn test_do_seq_len_zero() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let len_arg = make_int_vec(&[0]);
        let args = Rf_cons(len_arg, R_NilValue());
        let ans = do_seq_len(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(LENGTH(ans), 0);
    }
}

#[test]
fn test_do_seq_len_one() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let len_arg = make_int_vec(&[1]);
        let args = Rf_cons(len_arg, R_NilValue());
        let ans = do_seq_len(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(LENGTH(ans), 1);
        let data = INTEGER(ans);
        assert_eq!(*data.add(0), 1);
    }
}

// -----------------------------------------------------------------------
// do_seq_along tests
// -----------------------------------------------------------------------

#[test]
fn test_do_seq_along() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let x = make_int_vec(&[10, 20, 30, 40]);
        let args = Rf_cons(x, R_NilValue());
        let ans = do_seq_along(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(TYPEOF(ans), INTSXP_VAL);
        assert_eq!(LENGTH(ans), 4);
        let data = INTEGER(ans);
        assert_eq!(*data.add(0), 1);
        assert_eq!(*data.add(3), 4);
    }
}

#[test]
fn test_do_seq_along_empty() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let x = Rf_allocVector(INTSXP_VAL, 0);
        let args = Rf_cons(x, R_NilValue());
        let ans = do_seq_along(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(LENGTH(ans), 0);
    }
}

// -----------------------------------------------------------------------
// do_sequence tests
// -----------------------------------------------------------------------

#[test]
fn test_do_sequence_basic() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let lengths = make_int_vec(&[3, 2]);
        let from = make_int_vec(&[1, 10]);
        let by = make_int_vec(&[1, 5]);
        let recycle = make_int_vec(&[1]);
        // args: (lengths, from, by, recycle)
        let a4 = Rf_cons(recycle, R_NilValue());
        let a3 = Rf_cons(by, a4);
        let a2 = Rf_cons(from, a3);
        let args = Rf_cons(lengths, a2);

        let ans = do_sequence(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(TYPEOF(ans), INTSXP_VAL);
        assert_eq!(LENGTH(ans), 5); // 3 + 2
        let data = INTEGER(ans);
        // First sequence: 1, 2, 3
        assert_eq!(*data.add(0), 1);
        assert_eq!(*data.add(1), 2);
        assert_eq!(*data.add(2), 3);
        // Second sequence: 10, 15
        assert_eq!(*data.add(3), 10);
        assert_eq!(*data.add(4), 15);
    }
}

#[test]
fn test_do_sequence_empty() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        // sequence() now exposes the user-facing signature
        // sequence(nvec, from = 1L, by = 1L, recycle = FALSE): the
        // defaults are supplied by the handler, so an empty nvec alone
        // must yield an empty result.
        let lengths = Rf_allocVector(INTSXP_VAL, 0);
        let args = Rf_cons(lengths, R_NilValue());

        let ans = do_sequence(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert!(!ans.is_null());
        assert_eq!(LENGTH(ans), 0);
    }
}

// -----------------------------------------------------------------------
// datetime seq tests (stock R 4.6.1 parity)
// -----------------------------------------------------------------------

#[test]
fn test_pmatch_one_semantics() {
    let posixct_table = [
        "secs", "mins", "hours", "days", "weeks", "months", "years", "DSTdays", "quarters",
    ];
    let date_table = ["days", "weeks", "months", "quarters", "years"];
    // Ambiguous prefix is NA for POSIXct ("m" -> mins|months).
    assert_eq!(pmatch_one("m", &posixct_table), None);
    // But unique in the Date table ("m" -> months).
    assert_eq!(pmatch_one("m", &date_table), Some(2));
    assert_eq!(pmatch_one("month", &date_table), Some(2));
    assert_eq!(pmatch_one("DSTday", &posixct_table), Some(7));
    assert_eq!(pmatch_one("day", &posixct_table), Some(3));
    assert_eq!(pmatch_one("days", &posixct_table), Some(3));
    assert_eq!(pmatch_one("quarter", &posixct_table), Some(8));
    assert_eq!(pmatch_one("", &posixct_table), None);
    assert_eq!(pmatch_one("3", &posixct_table), None);
    assert_eq!(pmatch_one("secs", &date_table), None);
}

#[test]
fn test_split_by_spaces_strsplit_semantics() {
    assert_eq!(split_by_spaces("3 months"), vec!["3", "months"]);
    assert_eq!(split_by_spaces("month"), vec!["month"]);
    assert_eq!(split_by_spaces(""), Vec::<&str>::new());
    // strsplit drops trailing empty strings only.
    assert_eq!(split_by_spaces("days "), vec!["days"]);
    assert_eq!(split_by_spaces(" days"), vec!["", "days"]);
    assert_eq!(split_by_spaces("1  days"), vec!["1", "", "days"]);
}

#[test]
fn test_as_integer_multiplier_truncates_like_r() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        assert_eq!(as_integer_multiplier(ptr::null_mut(), "3"), Some(3));
        // as.integer("1.5") == 1L: seq(..., by="1.5 days") steps a day.
        assert_eq!(as_integer_multiplier(ptr::null_mut(), "1.5"), Some(1));
        assert_eq!(as_integer_multiplier(ptr::null_mut(), "-2.9"), Some(-2));
        assert_eq!(as_integer_multiplier(ptr::null_mut(), "1e3"), Some(1000));
        assert_eq!(as_integer_multiplier(ptr::null_mut(), "abc"), None);
        assert_eq!(as_integer_multiplier(ptr::null_mut(), ""), None);
        // Out of integer range is NA too.
        assert_eq!(as_integer_multiplier(ptr::null_mut(), "1e10"), None);
    }
}

#[test]
fn test_calendar_seq_months_matches_stock_dates() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let iso = |secs: c_double| {
            crate::mainutils::essentials::date_days_to_iso(secs / 86_400.0).unwrap()
        };
        // seq(as.Date('2020-01-31'), by = 'month', length.out = 3):
        // Feb 31 normalizes to Mar 2 (2020 is a leap year), then Mar 31.
        let anchor =
            crate::mainutils::essentials::days_from_civil(2020, 1, 31) as c_double * 86_400.0;
        let out = calendar_seq(
            ptr::null_mut(),
            CalendarField::Months,
            1,
            anchor,
            c_double::NAN,
            true,
            false,
            3,
        );
        let got: Vec<String> = out.iter().map(|s| iso(*s)).collect();
        assert_eq!(got, ["2020-01-31", "2020-03-02", "2020-03-31"]);

        // seq(as.Date('2020-02-29'), by = 'year', length.out = 3):
        // Feb 29 normalizes to Mar 1 in non-leap years.
        let anchor =
            crate::mainutils::essentials::days_from_civil(2020, 2, 29) as c_double * 86_400.0;
        let out = calendar_seq(
            ptr::null_mut(),
            CalendarField::Years,
            1,
            anchor,
            c_double::NAN,
            true,
            false,
            3,
        );
        let got: Vec<String> = out.iter().map(|s| iso(*s)).collect();
        assert_eq!(got, ["2020-02-29", "2021-03-01", "2022-03-01"]);
    }
}

#[test]
fn test_calendar_seq_from_to_filters_endpoint() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let iso = |secs: c_double| {
            crate::mainutils::essentials::date_days_to_iso(secs / 86_400.0).unwrap()
        };
        // seq(as.Date('2020-06-30'), as.Date('2020-12-31'), by='month'):
        // day-30 stepping never hits Dec 31, so the endpoint is not
        // included (stock: 2020-06-30 .. 2020-12-30).
        let from =
            crate::mainutils::essentials::days_from_civil(2020, 6, 30) as c_double * 86_400.0;
        let to = crate::mainutils::essentials::days_from_civil(2020, 12, 31) as c_double * 86_400.0;
        let out = calendar_seq(
            ptr::null_mut(),
            CalendarField::Months,
            1,
            from,
            to,
            false,
            false,
            NA_INTEGER as R_xlen_t,
        );
        let got: Vec<String> = out.iter().map(|s| iso(*s)).collect();
        assert_eq!(
            got,
            [
                "2020-06-30",
                "2020-07-30",
                "2020-08-30",
                "2020-09-30",
                "2020-10-30",
                "2020-11-30",
                "2020-12-30"
            ]
        );

        // to-anchored quarters keep the day-of-month:
        // seq(to = as.Date('2020-06-30'), by = 'quarter', length.out = 3)
        let to = crate::mainutils::essentials::days_from_civil(2020, 6, 30) as c_double * 86_400.0;
        let out = calendar_seq(
            ptr::null_mut(),
            CalendarField::Months,
            3,
            to,
            c_double::NAN,
            false,
            true,
            3,
        );
        let got: Vec<String> = out.iter().map(|s| iso(*s)).collect();
        assert_eq!(got, ["2019-12-30", "2020-03-30", "2020-06-30"]);

        // DSTdays over-estimate + filter:
        // seq(POSIXct 2020-01-01 .. 2020-01-05, by = '2 DSTdays')
        let from = crate::mainutils::essentials::days_from_civil(2020, 1, 1) as c_double * 86_400.0;
        let to = crate::mainutils::essentials::days_from_civil(2020, 1, 5) as c_double * 86_400.0;
        let out = calendar_seq(
            ptr::null_mut(),
            CalendarField::Dstdays,
            2,
            from,
            to,
            false,
            false,
            NA_INTEGER as R_xlen_t,
        );
        let got: Vec<i64> = out.iter().map(|s| (s / 86_400.0) as i64).collect();
        assert_eq!(
            got,
            [
                crate::mainutils::essentials::days_from_civil(2020, 1, 1),
                crate::mainutils::essentials::days_from_civil(2020, 1, 3),
                crate::mainutils::essentials::days_from_civil(2020, 1, 5),
            ]
        );
    }
}

#[test]
fn test_check1arg_partial_match_warning() {
    let mut session = crate::sexp::session::RSession::new();
    let _ = session.eval_script_with_output_capture("options(warnPartialMatchArgs = TRUE)");

    unsafe {
        // args cell: (l = 3L) checked against formal "length.out" —
        // "l" is a strict prefix, so a partial-argument-match warning
        // must be collected (default warn = 0).
        let args = Rf_cons(Rf_ScalarInteger(3), R_NilValue());
        let _args_guard = crate::sexp::protect::protect(args);
        SETTAG(args, Rf_install_stub(b"l\0".as_ptr() as *const c_char));
        check1arg(
            args,
            ptr::null_mut(),
            b"length.out\0".as_ptr() as *const c_char,
        );

        assert_eq!(crate::mainutils::errors::collect_warnings(), 1);
        let msg = crate::mainutils::errors::last_collected_warning_message();
        assert_eq!(msg.trim(), "partial argument match of 'l' to 'length.out'");

        // Full tag: no additional warning, no error.
        SETTAG(
            args,
            Rf_install_stub(b"length.out\0".as_ptr() as *const c_char),
        );
        check1arg(
            args,
            ptr::null_mut(),
            b"length.out\0".as_ptr() as *const c_char,
        );
        assert_eq!(crate::mainutils::errors::collect_warnings(), 1);

        // Non-matching tag errors (upstream: supplied argument name
        // '%s' does not match '%s').
        SETTAG(args, Rf_install_stub(b"bogus\0".as_ptr() as *const c_char));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            check1arg(
                args,
                ptr::null_mut(),
                b"length.out\0".as_ptr() as *const c_char,
            );
        }));
        let err = result.unwrap_err();
        let payload = err
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("RError payload");
        assert_eq!(
            payload.message.trim(),
            "supplied argument name 'bogus' does not match 'length.out'"
        );
    }
}

fn root_global(name: &str, value: SEXP) {
    let c_name = std::ffi::CString::new(name).unwrap();
    unsafe {
        let sym = crate::sexp::symbol::Rf_install(c_name.as_ptr());
        crate::sexp::envir::defineVar(sym, value, crate::sexp::globals::R_GlobalEnv());
    }
}

unsafe fn attribute_list(value: SEXP) -> SEXP {
    unsafe {
        let args = Rf_cons(value, R_NilValue());
        crate::mainutils::essentials::do_attributes(
            ptr::null_mut(),
            ptr::null_mut(),
            args,
            ptr::null_mut(),
        )
    }
}

fn attribute_name(shown: SEXP) -> String {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            shown,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let chars = crate::sexp::accessors::CHAR(crate::sexp::accessors::STRING_ELT(names, 0));
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .unwrap_or("")
            .to_string()
    }
}

#[test]
fn altseq_compact_integer_colon_stays_lazy_through_gc_and_hides_its_formula() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(1.0, 5.0, ptr::null_mut());
        root_global("alt_lazy_int", seq);
        assert_eq!(TYPEOF(seq), INTSXP_VAL);
        assert_eq!(XLENGTH(seq), 5);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert!((*seq).gengc_next_node.is_null());
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 0), 1);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 4), 5);
        assert_eq!(
            crate::sexp::accessors::INTEGER_ELT(seq, 5),
            crate::sexp::ffi::NA_INTEGER
        );
        assert_eq!(
            crate::sexp::accessors::INTEGER_ELT(seq, -1),
            crate::sexp::ffi::NA_INTEGER
        );
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert!((*seq).gengc_next_node.is_null());
        crate::sexp::gengc::full_gc();
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 2), 3);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert!((*seq).gengc_next_node.is_null());
        let shown = attribute_list(seq);
        assert!(shown.is_null() || shown == R_NilValue());
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);

        let down = seq_colon(5.0, 1.0, ptr::null_mut());
        root_global("alt_lazy_down", down);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(down, 0), 5);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(down, 4), 1);
        assert_eq!(crate::sexp::accessors::ALTREP(down), 1);
    }
}

#[test]
fn altseq_compact_integer_colon_matches_a_plain_vector() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(1.0, 5.0, ptr::null_mut());
        let plain = make_int_vec(&[1, 2, 3, 4, 5]);
        root_global("alt_ident_seq", seq);
        root_global("alt_ident_plain", plain);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert_eq!(
            crate::mainutils::identical::R_compute_identical(seq, plain, 0),
            1
        );
        assert_eq!(*INTEGER(seq).add(4), 5);
        crate::sexp::gengc::full_gc();
        assert_eq!(*INTEGER(seq).add(0), 1);
        assert_eq!(*INTEGER(plain).add(4), 5);
    }
}

#[test]
fn altseq_materializing_integer_colon_registers_a_buffer_that_survives_gc() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(1.0, 5.0, ptr::null_mut());
        root_global("alt_materialize", seq);
        assert!(!crate::sexp::memory::vector_payload_is_tracked(seq));
        let data = INTEGER(seq);
        assert_eq!(*data.add(0), 1);
        assert_eq!(*data.add(4), 5);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 0);
        assert!(crate::sexp::memory::vector_payload_is_tracked(seq));
        let shown = attribute_list(seq);
        assert!(shown.is_null() || shown == R_NilValue());
        crate::sexp::gengc::full_gc();
        assert_eq!(*INTEGER(seq).add(3), 4);
        assert!(crate::sexp::memory::vector_payload_is_tracked(seq));
    }
}

#[test]
fn altseq_compact_real_colon_matches_plain_reals() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(1.5, 3.5, ptr::null_mut());
        root_global("alt_real", seq);
        assert_eq!(TYPEOF(seq), REALSXP_VAL);
        assert_eq!(XLENGTH(seq), 3);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert!((crate::sexp::accessors::REAL_ELT(seq, 0) - 1.5).abs() < 1e-10);
        assert!((crate::sexp::accessors::REAL_ELT(seq, 1) - 2.5).abs() < 1e-10);
        assert!((crate::sexp::accessors::REAL_ELT(seq, 2) - 3.5).abs() < 1e-10);
        assert_eq!(
            crate::sexp::accessors::REAL_ELT(seq, 3).to_bits(),
            crate::sexp::ffi::NA_REAL.to_bits()
        );
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert!((*seq).gengc_next_node.is_null());

        let down = seq_colon(3.5, 1.5, ptr::null_mut());
        root_global("alt_real_down", down);
        assert!((crate::sexp::accessors::REAL_ELT(down, 0) - 3.5).abs() < 1e-10);
        assert!((crate::sexp::accessors::REAL_ELT(down, 2) - 1.5).abs() < 1e-10);
        assert_eq!(crate::sexp::accessors::ALTREP(down), 1);

        let plain = make_real_vec(&[1.5, 2.5, 3.5]);
        root_global("alt_real_plain", plain);
        assert_eq!(
            crate::mainutils::identical::R_compute_identical(seq, plain, 0),
            1
        );
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 0);
        crate::sexp::gengc::full_gc();
        assert!((*REAL(seq).add(2) - 3.5).abs() < 1e-10);
    }
}

#[test]
fn altseq_length_one_colon_is_a_plain_vector() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(3.0, 3.0, ptr::null_mut());
        assert_eq!(LENGTH(seq), 1);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 0);
        assert_eq!(*INTEGER(seq), 3);
        assert!(!(*seq).gengc_next_node.is_null());
    }
}

#[test]
fn altseq_dataptr_during_arena_lend_registers_the_buffer() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(1.0, 4.0, ptr::null_mut());
        root_global("alt_lend", seq);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        crate::sexp::memory::with_arena(|_arena| unsafe {
            let data = INTEGER(seq);
            assert_eq!(*data.add(3), 4);
            assert!(crate::sexp::memory::vector_payload_is_tracked(seq));
        });
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 0);
        assert!(crate::sexp::memory::vector_payload_is_tracked(seq));
        crate::sexp::gengc::full_gc();
        assert_eq!(*INTEGER(seq).add(3), 4);
    }
}

#[test]
fn altseq_million_step_integer_colon_does_not_allocate_its_payload() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(1.0, 1_000_000.0, ptr::null_mut());
        root_global("alt_million", seq);
        assert_eq!(XLENGTH(seq), 1_000_000);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert!((*seq).gengc_next_node.is_null());
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 0), 1);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 999_999), 1_000_000);
        let view = crate::sexp::Sexp::from_raw(seq).unwrap();
        assert_eq!(view.integer_elt(0), Some(1));
        assert_eq!(view.integer_elt(999_999), Some(1_000_000));
        assert!((*seq).gengc_next_node.is_null());
        assert!(!crate::sexp::memory::vector_payload_is_tracked(seq));
        crate::sexp::gengc::full_gc();
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 999_999), 1_000_000);
        assert!((*seq).gengc_next_node.is_null());
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
    }
}

#[test]
fn altseq_duplicate_of_a_lazy_colon_copies_the_values() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(1.0, 5.0, ptr::null_mut());
        root_global("alt_dup_src", seq);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        let copy = crate::mainutils::duplicate::duplicate(seq);
        root_global("alt_dup_copy", copy);
        assert_eq!(crate::sexp::accessors::ALTREP(copy), 0);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(copy, 0), 1);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(copy, 4), 5);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 0);
        crate::sexp::gengc::full_gc();
        assert_eq!(*INTEGER(copy).add(2), 3);
        assert_eq!(*INTEGER(seq).add(4), 5);
    }
}

#[test]
fn altseq_names_on_a_lazy_colon_do_not_expose_the_formula() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(1.0, 5.0, ptr::null_mut());
        root_global("alt_names", seq);
        let foo = Rf_ScalarInteger(7);
        let sym = crate::sexp::symbol::Rf_install(c"foo".as_ptr());
        crate::sexp::attrib_core::setAttrib(seq, sym, foo);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 4), 5);
        assert!((*seq).gengc_next_node.is_null());
        let shown = attribute_list(seq);
        assert_eq!(XLENGTH(shown), 1);
        assert_eq!(attribute_name(shown), "foo");
        assert_eq!(
            crate::sexp::accessors::INTEGER_ELT(crate::sexp::accessors::VECTOR_ELT(shown, 0), 0),
            7
        );

        crate::sexp::output::start_capture();
        crate::sexp::output::Rf_PrintValue(seq);
        let printed = crate::sexp::output::stop_capture();
        assert!(
            printed.stdout.contains("[1]"),
            "print should show the sequence, got {}",
            printed.stdout
        );
        assert!(
            !printed.stdout.contains(".InternalAltSeq"),
            "formula leaked into print: {}",
            printed.stdout
        );
        assert!(
            (*seq).gengc_next_node.is_null(),
            "printing allocated the payload"
        );
    }
}

#[test]
fn altseq_replacing_attributes_materializes_and_keeps_values() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(2.0, 4.0, ptr::null_mut());
        root_global("alt_clear_attr", seq);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        crate::sexp::accessors::SET_ATTRIB(seq, R_NilValue());
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 0);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 0), 2);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 2), 4);
        let shown = attribute_list(seq);
        assert!(shown.is_null() || shown == R_NilValue());
        assert!(crate::sexp::memory::vector_payload_is_tracked(seq));
        crate::sexp::gengc::full_gc();
        assert_eq!(*INTEGER(seq).add(1), 3);
    }
}

#[test]
fn altseq_arithmetic_and_matrix_print_read_the_formula_without_allocating() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let seq = seq_colon(1.0, 5.0, ptr::null_mut());
        root_global("alt_arith", seq);
        let view = crate::sexp::Sexp::from_raw(seq).unwrap();
        assert_eq!(view.integer_elt(0), Some(1));
        assert_eq!(view.integer_elt(4), Some(5));
        assert!(view.try_integer_elt(5).is_err());
        assert!(view.try_integer_elt(-1).is_err());
        assert!((*seq).gengc_next_node.is_null());

        let nums = crate::sexp::numeric::NumericVector::from_raw(seq).unwrap();
        assert_eq!(nums.clone().real_at(0), 1.0);
        assert_eq!(nums.real_at(4), 5.0);
        assert!((*seq).gengc_next_node.is_null());

        let one = Rf_ScalarInteger(1);
        root_global("alt_arith_one", one);
        let sum = crate::eval::arithmetic::real_binary("+", seq, one);
        root_global("alt_arith_sum", sum);
        assert_eq!(XLENGTH(sum), 5);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(sum, 0), 2);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(sum, 4), 6);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert!((*seq).gengc_next_node.is_null());
        assert!(!formula_tag_present(sum));

        let real = seq_colon(1.5, 3.5, ptr::null_mut());
        root_global("alt_arith_real", real);
        let real_view = crate::sexp::Sexp::from_raw(real).unwrap();
        assert_eq!(real_view.real_elt(0), Some(1.5));
        assert_eq!(real_view.real_elt(2), Some(3.5));
        assert!(real_view.try_real_elt(3).is_err());
        let real_sum = crate::eval::arithmetic::real_binary("+", real, one);
        root_global("alt_arith_real_sum", real_sum);
        assert_eq!(crate::sexp::accessors::REAL_ELT(real_sum, 0), 2.5);
        assert_eq!(crate::sexp::accessors::REAL_ELT(real_sum, 2), 4.5);
        assert_eq!(crate::sexp::accessors::ALTREP(real), 1);
        assert!((*real).gengc_next_node.is_null());
        assert!(!formula_tag_present(real_sum));

        let dim = Rf_allocVector(INTSXP_VAL, 2);
        root_global("alt_arith_dim", dim);
        *INTEGER(dim).add(0) = 5;
        *INTEGER(dim).add(1) = 1;
        crate::sexp::attrib_core::setAttrib(seq, crate::sexp::attrib_core::R_DimSymbol(), dim);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);
        assert!((*seq).gengc_next_node.is_null());
        crate::sexp::output::start_capture();
        crate::sexp::output::Rf_PrintValue(seq);
        let printed = crate::sexp::output::stop_capture();
        assert!(
            printed.stdout.contains("[5,]") && !printed.stdout.contains("no data buffer"),
            "matrix print should show the sequence, got {}",
            printed.stdout
        );
        assert!(
            !printed.stdout.contains(".InternalAltSeq"),
            "formula leaked into matrix print: {}",
            printed.stdout
        );
        assert!((*seq).gengc_next_node.is_null());
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 1);

        let plain = make_int_vec(&[8, 9]);
        root_global("alt_arith_plain", plain);
        let plain_view = crate::sexp::Sexp::from_raw(plain).unwrap();
        assert_eq!(plain_view.integer_elt(1), Some(9));

        let edited = seq_colon(1.0, 3.0, ptr::null_mut());
        root_global("alt_arith_edit", edited);
        let edited_view = crate::sexp::Sexp::from_raw(edited).unwrap();
        let mut edited_mut = crate::sexp::SexpMut::from_owned(edited_view);
        assert!(edited_mut.try_set_integer_elt(1, 9).is_ok());
        assert_eq!(crate::sexp::accessors::ALTREP(edited), 0);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(edited, 0), 1);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(edited, 1), 9);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(edited, 2), 3);

        let payload = view.try_data_ptr().unwrap();
        assert!(!payload.is_null());
        assert_eq!(*(payload as *const c_int), 1);
        assert_eq!(*(payload.cast::<c_int>().add(4)), 5);
        assert_eq!(crate::sexp::accessors::ALTREP(seq), 0);
        assert!(crate::sexp::memory::vector_payload_is_tracked(seq));
    }
}

#[test]
fn altseq_failed_allocation_keeps_the_formula() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let cleared = seq_colon(1.0, 16.0, ptr::null_mut());
        root_global("alt_budget_clear", cleared);
        let lent = seq_colon(1.0, 8.0, ptr::null_mut());
        root_global("alt_budget_lent", lent);
        let same_a = seq_colon(1.0, 4.0, ptr::null_mut());
        let same_b = seq_colon(1.0, 4.0, ptr::null_mut());
        root_global("alt_budget_same_a", same_a);
        root_global("alt_budget_same_b", same_b);
        // Built before the tight budget: cons cells themselves need a node.
        let eq_args = Rf_cons(same_a, Rf_cons(same_b, R_NilValue()));
        root_global("alt_budget_eq_args", eq_args);
        crate::sexp::memory::with_arena(|arena| {
            arena.set_budget(crate::sexp::memory::ArenaBudget::new(1, 0));
        });

        crate::sexp::accessors::SET_ATTRIB(cleared, R_NilValue());
        assert_eq!(crate::sexp::accessors::ALTREP(cleared), 1);
        assert!((*cleared).gengc_next_node.is_null());
        assert!(formula_tag_present(cleared));
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(cleared, 0), 1);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(cleared, 15), 16);

        // A refused buffer is never published, including inside the lend.
        crate::sexp::memory::with_arena(|_arena| unsafe {
            let data = INTEGER(lent);
            assert!(data.is_null());
            assert_eq!(crate::sexp::accessors::ALTREP(lent), 1);
            assert!((*lent).gengc_next_node.is_null());
            assert_eq!(crate::sexp::accessors::INTEGER_ELT(lent, 0), 1);
            assert_eq!(crate::sexp::accessors::INTEGER_ELT(lent, 7), 8);
            assert!(!crate::sexp::memory::vector_payload_is_tracked(lent));
        });
        assert_eq!(crate::sexp::accessors::ALTREP(lent), 1);
        assert!((*lent).gengc_next_node.is_null());
        assert!(formula_tag_present(lent));
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(lent, 0), 1);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(lent, 7), 8);
        assert!(!crate::sexp::memory::vector_payload_is_tracked(lent));

        assert_eq!(
            crate::mainutils::identical::R_compute_identical(same_a, same_b, 0),
            1
        );
        assert_eq!(
            crate::mainutils::identical::R_compute_identical(same_a, lent, 0),
            0
        );
        assert_eq!(
            crate::mainutils::all_equal::do_all_equal(
                ptr::null_mut(),
                ptr::null_mut(),
                eq_args,
                ptr::null_mut(),
            ),
            crate::sexp::globals::R_True()
        );
        assert_eq!(crate::sexp::accessors::ALTREP(same_a), 1);
        assert!((*same_a).gengc_next_node.is_null());
        assert!((*same_b).gengc_next_node.is_null());

        crate::sexp::memory::with_arena(|arena| {
            arena.set_budget(crate::sexp::memory::ArenaBudget::unlimited());
        });
        let shown = attribute_list(cleared);
        assert!(shown.is_null() || shown == R_NilValue());
    }
}

#[test]
fn altseq_all_equal_matches_a_plain_vector() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let lazy = seq_colon(1.0, 5.0, ptr::null_mut());
        let plain = make_int_vec(&[1, 2, 3, 4, 5]);
        root_global("alt_all_eq_lazy", lazy);
        root_global("alt_all_eq_plain", plain);
        let args = Rf_cons(lazy, Rf_cons(plain, R_NilValue()));
        let ans = crate::mainutils::all_equal::do_all_equal(
            ptr::null_mut(),
            ptr::null_mut(),
            args,
            ptr::null_mut(),
        );
        assert_eq!(ans, crate::sexp::globals::R_True());
    }
}

#[test]
fn altseq_printing_reads_the_formula_without_a_buffer() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let lazy_i = seq_colon(1.0, 12.0, ptr::null_mut());
        let plain_i = make_int_vec(&(1..=12).map(|v| v as c_int).collect::<Vec<_>>());
        let lazy_r = seq_colon(1.5, 4.5, ptr::null_mut());
        let plain_r = make_real_vec(&[1.5, 2.5, 3.5, 4.5]);
        let neg = seq_colon(-100.0, -90.0, ptr::null_mut());
        root_global("alt_print_lazy_i", lazy_i);
        root_global("alt_print_plain_i", plain_i);
        root_global("alt_print_lazy_r", lazy_r);
        root_global("alt_print_plain_r", plain_r);
        root_global("alt_print_neg", neg);
        let dim_li = set_matrix_dim(lazy_i, 3, 4);
        let dim_pi = set_matrix_dim(plain_i, 3, 4);
        let dim_lr = set_matrix_dim(lazy_r, 4, 1);
        let dim_pr = set_matrix_dim(plain_r, 4, 1);
        root_global("alt_print_dim_li", dim_li);
        root_global("alt_print_dim_pi", dim_pi);
        root_global("alt_print_dim_lr", dim_lr);
        root_global("alt_print_dim_pr", dim_pr);

        let mut wi_l = 0;
        let mut wi_p = 0;
        crate::mainutils::printarray::formatIntegerMatrix(lazy_i, 3, &mut wi_l);
        crate::mainutils::printarray::formatIntegerMatrix(plain_i, 3, &mut wi_p);
        assert_eq!(wi_l, 2);
        assert_eq!(wi_l, wi_p);
        assert!(still_lazy(lazy_i));

        let mut neg_w = 0;
        crate::mainutils::format::formatIntegerS(neg, XLENGTH(neg), &mut neg_w);
        assert_eq!(neg_w, 4);
        assert!(still_lazy(neg));

        let (mut wr_l, mut dr_l, mut er_l) = (0, 0, 0);
        let (mut wr_p, mut dr_p, mut er_p) = (0, 0, 0);
        crate::mainutils::printarray::formatRealMatrix(
            lazy_r, 4, &mut wr_l, &mut dr_l, &mut er_l,
        );
        crate::mainutils::printarray::formatRealMatrix(
            plain_r, 4, &mut wr_p, &mut dr_p, &mut er_p,
        );
        assert_eq!((wr_l, dr_l, er_l), (wr_p, dr_p, er_p));
        assert!(wr_l > 0);
        assert!(still_lazy(lazy_r));

        let nil = R_NilValue();
        let matrix_i_lazy = capture_stderr(|| unsafe {
            crate::mainutils::printarray::printMatrix(
                lazy_i,
                0,
                dim_li,
                1,
                0,
                nil,
                nil,
                ptr::null(),
                ptr::null(),
            );
        });
        let matrix_i_plain = capture_stderr(|| unsafe {
            crate::mainutils::printarray::printMatrix(
                plain_i,
                0,
                dim_pi,
                1,
                0,
                nil,
                nil,
                ptr::null(),
                ptr::null(),
            );
        });
        assert_eq!(matrix_i_lazy, matrix_i_plain);
        assert!(
            matrix_i_lazy.contains("[3,]") && matrix_i_lazy.contains("12"),
            "integer matrix print: {matrix_i_lazy}"
        );
        assert!(still_lazy(lazy_i));

        let matrix_r_lazy = capture_stderr(|| unsafe {
            crate::mainutils::printarray::printMatrix(
                lazy_r,
                0,
                dim_lr,
                1,
                0,
                nil,
                nil,
                ptr::null(),
                ptr::null(),
            );
        });
        let matrix_r_plain = capture_stderr(|| unsafe {
            crate::mainutils::printarray::printMatrix(
                plain_r,
                0,
                dim_pr,
                1,
                0,
                nil,
                nil,
                ptr::null(),
                ptr::null(),
            );
        });
        assert_eq!(matrix_r_lazy, matrix_r_plain);
        assert!(
            matrix_r_lazy.contains("1.5") && matrix_r_lazy.contains("4.5"),
            "real matrix print: {matrix_r_lazy}"
        );
        assert!(still_lazy(lazy_r));

        let vector_lazy = capture_stderr(|| unsafe {
            crate::mainutils::printvector::printVector(lazy_i, 1, 1);
        });
        let vector_plain = capture_stderr(|| unsafe {
            crate::mainutils::printvector::printVector(plain_i, 1, 1);
        });
        assert_eq!(vector_lazy, vector_plain);
        assert!(vector_lazy.contains("[1]") && vector_lazy.contains("12"));
        assert!(still_lazy(lazy_i));

        crate::sexp::output::start_capture();
        crate::sexp::output::Rf_PrintValue(lazy_r);
        let shown_lazy = crate::sexp::output::stop_capture();
        crate::sexp::output::start_capture();
        crate::sexp::output::Rf_PrintValue(plain_r);
        let shown_plain = crate::sexp::output::stop_capture();
        assert_eq!(shown_lazy.stdout, shown_plain.stdout);
        assert!(
            shown_lazy.stdout.contains("1.5") && shown_lazy.stdout.contains("4.5"),
            "captured real matrix: {}",
            shown_lazy.stdout
        );
        assert!(!shown_lazy.stdout.contains(".InternalAltSeq"));
        assert!(still_lazy(lazy_r));

        refuse_new_nodes();
        let under_budget = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            let mut w = 0;
            crate::mainutils::format::formatIntegerS(neg, XLENGTH(neg), &mut w);
            assert_eq!(w, 4);
            assert!(still_lazy(neg));
            let mut wi = 0;
            crate::mainutils::printarray::formatIntegerMatrix(lazy_i, 3, &mut wi);
            assert_eq!(wi, 2);
            assert!(still_lazy(lazy_i));
            let (mut wr, mut dr, mut er) = (0, 0, 0);
            crate::mainutils::format::formatRealS(
                lazy_r,
                XLENGTH(lazy_r),
                &mut wr,
                &mut dr,
                &mut er,
                0,
            );
            assert_eq!((wr, dr, er), (wr_l, dr_l, er_l));
            assert!(still_lazy(lazy_r));
            let again = capture_stderr(|| unsafe {
                crate::mainutils::printarray::printMatrix(
                    lazy_i,
                    0,
                    dim_li,
                    1,
                    0,
                    nil,
                    nil,
                    ptr::null(),
                    ptr::null(),
                );
            });
            assert_eq!(again, matrix_i_plain);
            assert!(still_lazy(lazy_i));
            assert_eq!(crate::sexp::accessors::INTEGER_ELT(lazy_i, 0), 1);
            assert_eq!(crate::sexp::accessors::INTEGER_ELT(lazy_i, 11), 12);
            crate::sexp::output::start_capture();
            crate::sexp::output::Rf_PrintValue(lazy_r);
            let shown = crate::sexp::output::stop_capture();
            assert_eq!(shown.stdout, shown_plain.stdout);
            assert!(still_lazy(lazy_r));
        }));
        crate::sexp::memory::with_arena(|arena| {
            arena.set_budget(crate::sexp::memory::ArenaBudget::unlimited());
        });
        if let Err(payload) = under_budget {
            let message = payload
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.clone())
                .unwrap_or_else(|| "print under a refused budget panicked".to_string());
            panic!("{message}");
        }
    }
}

#[test]
fn altseq_encode_element_does_not_materialize_or_null_deref() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let lazy = seq_colon(1.0, 12.0, ptr::null_mut());
        let plain = make_int_vec(&[1]);
        root_global("alt_encode_lazy", lazy);
        root_global("alt_encode_plain", plain);
        let dec = b".\0".as_ptr() as *const c_char;
        let lazy_text = encode_text(lazy, 0, dec);
        let plain_text = encode_text(plain, 0, dec);
        assert_eq!(lazy_text, plain_text);
        assert!(still_lazy(lazy));

        let missing = encode_text(lazy, -1, dec);
        let past = encode_text(lazy, 12, dec);
        assert!(missing.contains("NA"), "negative index encoded {missing}");
        assert!(past.contains("NA"), "past-the-end index encoded {past}");
        assert!(still_lazy(lazy));

        refuse_new_nodes();
        let under_budget = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            assert_eq!(encode_text(lazy, 0, dec), plain_text);
            assert!(still_lazy(lazy));
            let _ = encode_text(lazy, -1, dec);
            let _ = encode_text(lazy, 99, dec);
            assert!(still_lazy(lazy));
        }));
        restore_unlimited_budget();
        if let Err(payload) = under_budget {
            panic!("{}", unwind_message(payload));
        }
    }
}

#[test]
fn altseq_long_integer_colon_stays_lazy() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let n = (c_int::MAX as R_xlen_t) + 1;
        let seq = seq_colon(0.0, c_int::MAX as c_double, ptr::null_mut());
        root_global("alt_long_int", seq);
        assert_eq!(TYPEOF(seq), INTSXP_VAL);
        assert_eq!(XLENGTH(seq), n);
        assert!(still_lazy(seq));
        let mut width = 0;
        crate::mainutils::format::formatIntegerS(seq, XLENGTH(seq), &mut width);
        assert_eq!(width, 10);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(seq, 0), 0);
        assert_eq!(
            crate::sexp::accessors::INTEGER_ELT(seq, c_int::MAX),
            c_int::MAX
        );
        assert!(still_lazy(seq));

        let via = R_compact_intrange(0, c_int::MAX as R_xlen_t);
        root_global("alt_long_intrange", via);
        assert_eq!(TYPEOF(via), INTSXP_VAL);
        assert_eq!(XLENGTH(via), n);
        assert!(still_lazy(via));
        let mut via_width = 0;
        crate::mainutils::format::formatIntegerS(via, XLENGTH(via), &mut via_width);
        assert_eq!(via_width, 10);
        assert_eq!(crate::sexp::accessors::INTEGER_ELT(via, 0), 0);
        assert_eq!(
            crate::sexp::accessors::INTEGER_ELT(via, c_int::MAX),
            c_int::MAX
        );
        assert!(still_lazy(via));

        // Short runs, including ones that wrap past c_int::MAX, must match a
        // full walk. The long colon above is the constant-time case.
        for (from, step, len) in [
            (c_int::MAX - 1, 1, 4i64),
            (c_int::MIN + 2, -1, 5),
            (c_int::MAX - 10, 100, 5),
            (-40, 0, 8),
            (1, 1, 12),
            (-100, 1, 11),
        ] {
            let sample = crate::sexp::altseq::compact_int_seq(from, step, len as usize);
            root_global(&format!("alt_int_width_{from}_{step}"), sample);
            let mut fast = 0;
            crate::mainutils::format::formatIntegerS(sample, len, &mut fast);
            let view = crate::sexp::Sexp::from_raw(sample).unwrap();
            let formula = view.compact_seq().unwrap();
            let scanned =
                crate::mainutils::format::integer_field_width(len, |i| formula.int_or_na(i));
            assert_eq!(fast, scanned, "int width from={from} step={step} n={len}");
            assert!(still_lazy(sample));
        }

        // Two columns of 2^31. The second column starts at 2^31, past
        // c_int::MAX, and contains NA plus the widest negative integer.
        let matrix = crate::sexp::altseq::compact_int_seq(0, 1, 1usize << 32);
        root_global("alt_int_matrix_width", matrix);
        assert_eq!(XLENGTH(matrix), 1i64 << 32);
        assert!(still_lazy(matrix));
        let mut matrix_w = 0;
        crate::mainutils::printarray::formatIntegerMatrix(matrix, 1i64 << 31, &mut matrix_w);
        assert_eq!(matrix_w, 11);
        assert!(still_lazy(matrix));
    }
}

#[test]
fn altseq_long_real_colon_stays_lazy() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let n = (c_int::MAX as R_xlen_t) + 1;
        let seq = seq_colon(1.0, (c_int::MAX as c_double) + 1.0, ptr::null_mut());
        root_global("alt_long_real", seq);
        assert_eq!(TYPEOF(seq), REALSXP_VAL);
        assert_eq!(XLENGTH(seq), n);
        assert!(still_lazy(seq));
        let (mut w, mut d, mut e) = (0, 0, 0);
        crate::mainutils::format::formatRealS(seq, XLENGTH(seq), &mut w, &mut d, &mut e, 0);
        assert!(w > 0, "long real colon width was {w}");
        assert_eq!(crate::sexp::accessors::REAL_ELT(seq, 0), 1.0);
        assert!(still_lazy(seq));

        let via = R_compact_intrange(1, n);
        root_global("alt_long_seq_len", via);
        assert_eq!(TYPEOF(via), REALSXP_VAL);
        assert_eq!(XLENGTH(via), n);
        assert!(still_lazy(via));
        let via_view = crate::sexp::Sexp::from_raw(via).unwrap();
        assert_eq!(via_view.try_real_elt(0), Ok(1.0));
        assert_eq!(via_view.try_real_elt(n - 1), Ok((c_int::MAX as c_double) + 1.0));
        assert!(still_lazy(via));

        // `(-2147483649):1` does not fit in an integer sequence.
        let wide = R_compact_intrange(-2147483649, 1);
        root_global("alt_wide_real", wide);
        assert_eq!(TYPEOF(wide), REALSXP_VAL);
        assert_eq!(XLENGTH(wide), 2147483651);
        assert!(still_lazy(wide));
        let wide_view = crate::sexp::Sexp::from_raw(wide).unwrap();
        assert_eq!(wide_view.try_real_elt(0), Ok(-2147483649.0));
        assert_eq!(wide_view.try_real_elt(2147483650), Ok(1.0));
        assert!(still_lazy(wide));
    }
}

#[test]
fn altseq_sampled_real_field_matches_a_full_scan() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let cases = [
        (-120.0, 1.0, 250i64),
        (50.0, -1.0, 400),
        (9990.0, 1.0, 40),
        (999_000.0, 1.0, 2_500),
        (1_000_000.0, 1.0, 1_500),
        (1_000_000.5, 0.5, 400),
        (0.5, 0.5, 800),
        (-2.5, 0.5, 40),
        (9_999.5, 0.5, 20),
        (-1_000_010.0, 1.0, 40),
        (10010.0, -1.0, 40),
        (0.25, 0.25, 400),
        (-8.0, 0.5, 80),
    ];
    for (from, step, n) in cases {
        let full = crate::mainutils::format::real_field(n, 0, |i| from + (i as f64) * step);
        let sampled = crate::mainutils::format::sampled_real_field(from, step, n, 0);
        assert_eq!(
            (sampled.w, sampled.d, sampled.e),
            (full.w, full.d, full.e),
            "sampled real field from={from} step={step} n={n}"
        );
    }
}

fn encode_text(x: SEXP, index: R_xlen_t, dec: *const c_char) -> String {
    unsafe {
        let encoded = crate::mainutils::printutils::EncodeElement0(x, index, 0, dec);
        std::ffi::CStr::from_ptr(encoded)
            .to_string_lossy()
            .into_owned()
    }
}

fn refuse_new_nodes() {
    unsafe {
        crate::sexp::memory::with_arena(|arena| {
            let nodes = arena.node_count();
            let max_nodes = if nodes == 0 { 1 } else { nodes };
            arena.set_budget(crate::sexp::memory::ArenaBudget::new(1, max_nodes));
        });
    }
}

fn restore_unlimited_budget() {
    unsafe {
        crate::sexp::memory::with_arena(|arena| {
            arena.set_budget(crate::sexp::memory::ArenaBudget::unlimited());
        });
    }
}

fn unwind_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<crate::sexp::context::RError>()
        .map(|err| err.message.clone())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|message| (*message).to_string())
        })
        .unwrap_or_else(|| "print under a refused budget panicked".to_string())
}

fn still_lazy(value: SEXP) -> bool {
    unsafe {
        crate::sexp::accessors::ALTREP(value) == 1 && (*value).gengc_next_node.is_null()
    }
}

unsafe fn set_matrix_dim(x: SEXP, nrow: c_int, ncol: c_int) -> SEXP {
    unsafe {
        let dim = Rf_allocVector(INTSXP_VAL, 2);
        *INTEGER(dim).add(0) = nrow;
        *INTEGER(dim).add(1) = ncol;
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
        dim
    }
}

fn capture_stderr(f: impl FnOnce()) -> String {
    let buf = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    crate::mainutils::printutils::set_console_sink(Some(std::rc::Rc::clone(&buf)));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    crate::mainutils::printutils::set_console_sink(None);
    match result {
        Ok(()) => buf.borrow().clone(),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn formula_tag_present(value: SEXP) -> bool {
    unsafe {
        let mut cell = crate::sexp::accessors::ATTRIB(value);
        while !cell.is_null() && cell != R_NilValue() {
            if crate::sexp::altseq::is_formula_tag(crate::sexp::accessors::TAG(cell)) {
                return true;
            }
            cell = crate::sexp::accessors::CDR(cell);
        }
        false
    }
}
