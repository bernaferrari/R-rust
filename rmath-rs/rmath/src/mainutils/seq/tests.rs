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
