use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn activate_test_session() -> crate::sexp::session::RSession {
        crate::sexp::session::RSession::new()
    }

    #[test]
    fn test_isleap() {
        assert!(!isleap(1900));
        assert!(!isleap(2001));
        assert!(isleap(2000));
        assert!(isleap(2020));
        assert!(isleap(2024));
        assert!(!isleap(2100));
        assert!(isleap(2400));
    }

    #[test]
    fn test_detzcode() {
        for n in [-1i32, -18000, -14400, i32::MIN + 1] {
            assert_eq!(detzcode(&n.to_be_bytes()), n);
        }
        // 0x00000000 = 0
        assert_eq!(detzcode(&[0, 0, 0, 0]), 0);
        // 0x00000001 = 1
        assert_eq!(detzcode(&[0, 0, 0, 1]), 1);
        // 0x7FFFFFFF = INT32_MAX
        assert_eq!(detzcode(&[0x7f, 0xff, 0xff, 0xff]), i32::MAX);
        // 0x80000000 = INT32_MIN
        assert_eq!(detzcode(&[0x80, 0, 0, 0]), i32::MIN);
    }

    #[test]
    fn test_detzcode64() {
        for n in [-1i64, -18000, -2147483649, i64::MIN + 1] {
            assert_eq!(detzcode64(&n.to_be_bytes()), n);
        }
        assert_eq!(detzcode64(&[0, 0, 0, 0, 0, 0, 0, 0]), 0);
        assert_eq!(detzcode64(&[0, 0, 0, 0, 0, 0, 0, 1]), 1);
    }

    #[test]
    fn test_increment_overflow() {
        let mut x: i32 = 100;
        assert!(!increment_overflow(&mut x, 200));
        assert_eq!(x, 300);
        assert!(increment_overflow(&mut x, i32::MAX));
    }

    #[test]
    fn test_leaps_thru_end_of() {
        assert_eq!(leaps_thru_end_of(0), 0);
        assert_eq!(leaps_thru_end_of(1), 0);
        assert_eq!(leaps_thru_end_of(4), 1);
        assert_eq!(leaps_thru_end_of(100), 24);
        assert_eq!(leaps_thru_end_of(400), 97);
    }

    #[test]
    fn test_transtime_julian() {
        let r = rule {
            r_type: JULIAN_DAY,
            r_day: 1,
            r_week: 0,
            r_mon: 0,
            r_time: 0,
        };
        // Jan 1 at offset 0 = 0 seconds into the year
        assert_eq!(transtime(2024, &r, 0), 0);
    }

    #[test]
    fn test_gmtime_epoch() {
        let _session = activate_test_session();
        unsafe {
            let t: i64 = 0;
            let result = R_gmtime(&t);
            assert!(!result.is_null());
            let tm = &*result;
            assert_eq!(tm.tm_year, 70); // 1970 - 1900
            assert_eq!(tm.tm_mon, 0); // January
            assert_eq!(tm.tm_mday, 1);
            assert_eq!(tm.tm_hour, 0);
            assert_eq!(tm.tm_min, 0);
            assert_eq!(tm.tm_sec, 0);
            assert_eq!(tm.tm_wday, 4); // Thursday
        }
    }

    #[test]
    fn timezone_override_is_session_local_and_restorable() {
        let host_tz = std::env::var_os("TZ");
        let _session = activate_test_session();
        unsafe {
            let timestamp = 0_i64;
            let mut tm = stm::default();
            set_timezone_override(Some("UTC".to_owned()));
            assert!(!R_localtime_r(&timestamp, &mut tm).is_null());
            assert_eq!(tm.tm_hour, 0);
            set_timezone_override(Some("America/New_York".to_owned()));
            assert!(!R_localtime_r(&timestamp, &mut tm).is_null());
            assert_eq!(
                (tm.tm_hour, tm.tm_min, tm.tm_sec, tm.tm_gmtoff),
                (19, 0, 0, -18000)
            );
            set_timezone_override(Some("UTC".to_owned()));
            assert!(!R_localtime_r(&timestamp, &mut tm).is_null());
            assert_eq!(tm.tm_hour, 0);
        }
        drop(_session);
        let _second = activate_test_session();
        set_timezone_override(Some("UTC".to_owned()));
        unsafe {
            let timestamp = 0_i64;
            let mut tm = stm::default();
            assert!(!R_localtime_r(&timestamp, &mut tm).is_null());
            assert_eq!(tm.tm_hour, 0);
        }
        assert_eq!(std::env::var_os("TZ"), host_tz);
    }

    #[test]
    fn test_mktime_epoch() {
        let _session = activate_test_session();
        set_timezone_override(Some("UTC".to_owned()));
        unsafe {
            let mut tm = stm {
                tm_sec: 0,
                tm_min: 0,
                tm_hour: 0,
                tm_mday: 1,
                tm_mon: 0,
                tm_year: 70,
                tm_wday: 0,
                tm_yday: 0,
                tm_isdst: -1,
                tm_gmtoff: 0,
                tm_zone: std::ptr::null(),
            };
            let result = R_mktime(&mut tm);
            assert_eq!(result, 0);
        }
    }

    #[test]
    fn test_timegm_epoch() {
        let _session = activate_test_session();
        unsafe {
            let mut tm = stm {
                tm_sec: 0,
                tm_min: 0,
                tm_hour: 0,
                tm_mday: 1,
                tm_mon: 0,
                tm_year: 70,
                tm_wday: 0,
                tm_yday: 0,
                tm_isdst: 0,
                tm_gmtoff: 0,
                tm_zone: std::ptr::null(),
            };
            let result = R_timegm(&mut tm);
            assert_eq!(result, 0);
        }
    }

    #[test]
    fn test_gmtime_known_date() {
        let _session = activate_test_session();
        unsafe {
            // 2024-01-01 00:00:00 UTC = 1704067200
            let t: i64 = 1704067200;
            let result = R_gmtime(&t);
            assert!(!result.is_null());
            let tm = &*result;
            assert_eq!(tm.tm_year, 124); // 2024 - 1900
            assert_eq!(tm.tm_mon, 0); // January
            assert_eq!(tm.tm_mday, 1);
            assert_eq!(tm.tm_hour, 0);
            assert_eq!(tm.tm_min, 0);
            assert_eq!(tm.tm_sec, 0);
            assert_eq!(tm.tm_wday, 1); // Monday
        }
    }

    #[test]
    fn test_gmtime_r_reentrant() {
        let _session = activate_test_session();
        unsafe {
            let t1: i64 = 0;
            let t2: i64 = 1704067200;
            let mut tm1 = stm::default();
            let mut tm2 = stm::default();
            let r1 = R_gmtime_r(&t1, &mut tm1);
            let r2 = R_gmtime_r(&t2, &mut tm2);
            assert!(!r1.is_null());
            assert!(!r2.is_null());
            assert_eq!((*r1).tm_year, 70);
            assert_eq!((*r2).tm_year, 124);
        }
    }

    #[test]
    fn test_tzone_cache_is_session_local_on_same_thread() {
        use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            let t: i64 = 0;
            let first_tm = R_gmtime(&t);
            assert!(!first_tm.is_null());

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            let second_tm = R_gmtime(&t);
            assert!(!second_tm.is_null());

            assert_ne!(first_tm, second_tm);
            clear_current_instance();
        }
    }
}
