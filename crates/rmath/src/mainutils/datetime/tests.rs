use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timezone_guard_restores_its_owner_when_another_session_is_active() {
        let _first = crate::sexp::session::RSession::new();
        crate::tzone::set_timezone_override(Some("UTC".to_owned()));
        let setup = TzSetup::prepare();
        setup.set("America/New_York");
        let owner = setup.owner;
        let _second = crate::sexp::session::RSession::new();
        crate::tzone::set_timezone_override(Some("Asia/Tokyo".to_owned()));
        drop(setup);
        assert_eq!(
            crate::tzone::timezone_override().as_deref(),
            Some("Asia/Tokyo")
        );
        let previous = unsafe { crate::sexp::instance::replace_current_instance(Some(owner)) };
        assert_eq!(crate::tzone::timezone_override().as_deref(), Some("UTC"));
        unsafe { crate::sexp::instance::replace_current_instance(previous) };
    }

    #[test]
    fn timezone_setup_restores_session_override_when_unwinding() {
        let _session = crate::sexp::session::RSession::new();
        crate::tzone::set_timezone_override(Some("UTC".to_owned()));
        let host_tz = std::env::var_os("TZ");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let setup = TzSetup::prepare();
            setup.set("America/New_York");
            panic!("exercise timezone guard unwind");
        }));
        assert!(result.is_err());
        assert_eq!(crate::tzone::timezone_override().as_deref(), Some("UTC"));
        assert_eq!(std::env::var_os("TZ"), host_tz);
    }

    #[test]
    fn test_isleap() {
        assert!(isleap(2000)); // divisible by 400
        assert!(isleap(2004)); // divisible by 4, not 100
        assert!(!isleap(1900)); // divisible by 100, not 400
        assert!(!isleap(2001)); // not divisible by 4
    }

    #[test]
    fn test_days_in_year() {
        assert_eq!(days_in_year(2000), 366);
        assert_eq!(days_in_year(2001), 365);
        assert_eq!(days_in_year(1900), 365);
        assert_eq!(days_in_year(2004), 366);
    }

    #[test]
    fn test_days_in_month() {
        // tm_year=70 means year 1970 (non-leap)
        assert_eq!(days_in_month(0, 70), 31); // Jan
        assert_eq!(days_in_month(1, 70), 28); // Feb 1970
        assert_eq!(days_in_month(1, 72), 29); // Feb 1972 (leap)
        assert_eq!(days_in_month(3, 70), 30); // Apr
        assert_eq!(days_in_month(11, 70), 31); // Dec
    }

    #[test]
    fn test_R_ISLeapYear() {
        // R_ISLeapYear takes years since 1900
        assert!(R_ISLeapYear(100)); // 2000
        assert!(!R_ISLeapYear(0)); // 1900
        assert!(R_ISLeapYear(4)); // 1904
    }

    #[test]
    fn test_R_isLeapYear_absolute() {
        // R_isLeapYear takes absolute years
        unsafe {
            assert_eq!(R_isLeapYear(2000), 1);
            assert_eq!(R_isLeapYear(1900), 0);
            assert_eq!(R_isLeapYear(2004), 1);
            assert_eq!(R_isLeapYear(2001), 0);
        }
    }

    #[test]
    fn test_validate_tm_already_valid() {
        let mut tm = stm::new();
        tm.tm_mday = 15;
        tm.tm_mon = 5; // June
        tm.tm_year = 70; // 1970
        assert_eq!(validate_tm(&mut tm), 0);
        assert_eq!(tm.tm_mday, 15);
    }

    #[test]
    fn test_validate_tm_overflow_seconds() {
        let mut tm = stm::new();
        tm.tm_sec = 125; // 2 min 5 sec
        tm.tm_mday = 1;
        tm.tm_mon = 0;
        tm.tm_year = 70;
        let res = validate_tm(&mut tm);
        assert!(res > 0);
        assert_eq!(tm.tm_sec, 5);
        assert_eq!(tm.tm_min, 2);
    }

    #[test]
    fn test_validate_tm_24_hour() {
        let mut tm = stm::new();
        tm.tm_hour = 24;
        tm.tm_min = 0;
        tm.tm_sec = 0;
        tm.tm_mday = 15;
        tm.tm_mon = 0;
        tm.tm_year = 70;
        validate_tm(&mut tm);
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_mday, 16);
    }

    #[test]
    fn test_validate_tm_overflow_day() {
        let mut tm = stm::new();
        tm.tm_mday = 32; // Jan 32
        tm.tm_mon = 0;
        tm.tm_year = 70; // 1970
        validate_tm(&mut tm);
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_mon, 1); // Feb
    }

    #[test]
    fn test_mkdate00_epoch() {
        let mut tm = stm::new();
        tm.tm_mday = 1;
        tm.tm_mon = 0; // Jan
        tm.tm_year = 70; // 1970
        let day = mkdate00(&mut tm);
        assert_eq!(day, 0.0);
        assert_eq!(tm.tm_wday, 4); // Thursday
        assert_eq!(tm.tm_yday, 0);
    }

    #[test]
    fn test_mkdate00_2000() {
        let mut tm = stm::new();
        tm.tm_mday = 1;
        tm.tm_mon = 0; // Jan
        tm.tm_year = 100; // 2000
        let day = mkdate00(&mut tm);
        // 2000-01-01 is 10957 days after 1970-01-01
        assert_eq!(day, 10957.0);
        assert_eq!(tm.tm_wday, 6); // Saturday
    }

    #[test]
    fn test_mkdate00_na() {
        let mut tm = stm::new();
        tm.tm_mday = NA_INTEGER;
        tm.tm_mon = 0;
        tm.tm_year = 70;
        let day = mkdate00(&mut tm);
        assert!(day.is_nan());
        assert_eq!(tm.tm_yday, NA_INTEGER);
        assert_eq!(tm.tm_wday, NA_INTEGER);
    }

    #[test]
    fn test_mkdate00_extreme_year_avoids_1900_overflow() {
        let mut tm = stm::new();
        tm.tm_mday = 1;
        tm.tm_mon = 0;
        tm.tm_year = c_int::MAX;
        let day = mkdate00(&mut tm);
        assert!(day.is_finite());
    }

    #[test]
    fn test_timegm00_epoch() {
        let mut tm = stm::new();
        tm.tm_mday = 1;
        tm.tm_mon = 0;
        tm.tm_year = 70;
        let t = timegm00(&mut tm);
        assert_eq!(t, 0.0);
    }

    #[test]
    fn test_timegm00_with_time() {
        let mut tm = stm::new();
        tm.tm_sec = 30;
        tm.tm_min = 15;
        tm.tm_hour = 10;
        tm.tm_mday = 2;
        tm.tm_mon = 0; // Jan
        tm.tm_year = 70; // 1970
        let t = timegm00(&mut tm);
        // 1 day + 10h + 15m + 30s = 86400 + 36000 + 900 + 30 = 123330
        assert_eq!(t, 123330.0);
    }

    #[test]
    fn test_julian2dtime_epoch() {
        let mut tm = stm::new();
        assert!(julian2dtime(0.0, &mut tm));
        assert_eq!(tm.tm_year, 70);
        assert_eq!(tm.tm_mon, 0);
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_wday, 4); // Thursday
    }

    #[test]
    fn test_julian2dtime_positive() {
        let mut tm = stm::new();
        assert!(julian2dtime(1.0, &mut tm));
        assert_eq!(tm.tm_year, 70);
        assert_eq!(tm.tm_mon, 0);
        assert_eq!(tm.tm_mday, 2);
    }

    #[test]
    fn test_dtime2julian_roundtrip() {
        // Set up a date: 1970-01-15
        let j = dtime2julian(0.0, 0, 0, 15, 0, 70);
        assert_eq!(j, 14.0); // 14 days since epoch

        // Round-trip: julian -> stm -> julian
        let mut tm = stm::new();
        assert!(julian2dtime(j, &mut tm));
        let j2 = dtime2julian(
            0.0, tm.tm_min, tm.tm_hour, tm.tm_mday, tm.tm_mon, tm.tm_year,
        );
        assert_eq!(j, j2);
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        let result = days_to_ymd(0.0);
        assert!(result.is_some());
        let (yr, yday, mon, mday) = result.unwrap_or_else(|| panic!("unexpected None in test"));
        assert_eq!(yr, 70); // 1970
        assert_eq!(yday, 0);
        assert_eq!(mon, 0);
        assert_eq!(mday, 1);
    }

    #[test]
    fn test_likely_strftime_overflow() {
        let mut tm = stm::new();
        tm.tm_year = 0; // 1900 -- fine
        assert!(!likely_strftime_overflow(&tm));

        tm.tm_year = c_int::MAX; // overflow
        assert!(likely_strftime_overflow(&tm));
    }

    #[test]
    fn test_lt_component_name() {
        assert_eq!(lt_component_name(0), "sec");
        assert_eq!(lt_component_name(5), "year");
        assert_eq!(lt_component_name(10), "gmtoff");
        assert_eq!(lt_component_name(11), ""); // out of range
    }

    #[test]
    fn test_mktime0_epoch_utc() {
        let mut tm = stm::new();
        tm.tm_mday = 1;
        tm.tm_mon = 0;
        tm.tm_year = 70;
        let t = mktime0(&mut tm, false);
        assert_eq!(t, 0.0);
    }

    #[test]
    fn test_mktime0_epoch_local() {
        // R_mktime reads the session's tzone globals; unit tests must hold
        // a session exactly like tzone's own tests do.
        let _session = crate::sexp::session::RSession::new();
        let mut tm = stm::new();
        tm.tm_mday = 1;
        tm.tm_mon = 0;
        tm.tm_year = 70;
        // mktime returns local time; epoch in local time depends on timezone
        let t = mktime0(&mut tm, true);
        // Just check it's finite and non-negative for most timezones
        assert!(t.is_finite());
    }

    #[test]
    fn test_mkdate00_pre_epoch() {
        let mut tm = stm::new();
        tm.tm_mday = 1;
        tm.tm_mon = 0; // Jan
        tm.tm_year = 60; // 1960
        let day = mkdate00(&mut tm);
        // 1960-01-01 is 3653 days before 1970-01-01
        assert_eq!(day, -3653.0);
    }

    #[test]
    fn test_mkdate00_leap_year_day() {
        let mut tm = stm::new();
        tm.tm_mday = 29;
        tm.tm_mon = 1; // Feb
        tm.tm_year = 100; // 2000 (leap year)
        let day = mkdate00(&mut tm);
        // 2000-02-29: Jan has 31 days, so Feb 29 = day 59 (0-indexed)
        assert_eq!(day, 10957.0f64 + 31.0 + 28.0);
        assert_eq!(tm.tm_yday, 59);
    }

    #[test]
    fn test_mkdate00_non_leap_feb29() {
        let mut tm = stm::new();
        tm.tm_mday = 29;
        tm.tm_mon = 1; // Feb
        tm.tm_year = 70; // 1970 (not leap)
        let day = mkdate00(&mut tm);
        // Feb 29 1970: mkdate00 doesn't normalize dates, just computes day number
        // day = 28 (tm_mday-1) + 31 (Jan) = 59 (which is Mar 1)
        assert_eq!(day, 59.0);
        assert_eq!(tm.tm_yday, 59);
    }

    #[test]
    fn test_julian2dtime_leap_year() {
        let mut tm = stm::new();
        // Feb 29 2000 = day 10957 + 31 + 28 = 11016
        // (10957 = Jan 1 2000, +31 = Feb 1, +28 = Feb 29)
        assert!(julian2dtime(11016.0, &mut tm));
        assert_eq!(tm.tm_year, 100); // 2000
        assert_eq!(tm.tm_mon, 1); // Feb
        assert_eq!(tm.tm_mday, 29);
    }

    #[test]
    fn test_mkdate00_century_boundary() {
        let mut tm = stm::new();
        tm.tm_mday = 31;
        tm.tm_mon = 11; // Dec
        tm.tm_year = 99; // 1999
        let day = mkdate00(&mut tm);
        // 1999-12-31 is one day before 2000-01-01 (10957)
        assert_eq!(day, 10956.0);
    }
}
