use super::*;

pub(crate) type time_t = i64;

use crate::sexp::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};

use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Days in each month for a non-leap year.
pub static month_days: [c_int; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// NA_REAL sentinel matching R's NA_REAL.
pub const NA_REAL: c_double = crate::sexp::ffi::NA_REAL;

/// GNU datetime.c reads POSIXlt calendar fields via INTEGER() after
/// balance, but `$year <-` can leave a REALSXP (GNU `$<-` does not coerce).
/// Accept INTSXP/LGLSXP/REALSXP the way REAL_ELT accepts integers.
pub(crate) unsafe fn posixlt_int_elt(col: SEXP, i: usize) -> c_int {
    unsafe {
        if col.is_null() || col == R_NilValue() {
            return NA_INTEGER;
        }
        let n = XLENGTH(col);
        if n <= 0 {
            return NA_INTEGER;
        }
        let i = i % n as usize;
        match TYPEOF(col) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => *INTEGER(col).add(i),
            t if t == SEXPTYPE::REALSXP => {
                let v = *REAL(col).add(i);
                if !R_FINITE(v) || R_IsNA(v) {
                    NA_INTEGER
                } else {
                    v as c_int
                }
            }
            _ => NA_INTEGER,
        }
    }
}

pub(crate) unsafe fn posixlt_real_elt(col: SEXP, i: usize) -> f64 {
    unsafe {
        if col.is_null() || col == R_NilValue() {
            return NA_REAL;
        }
        let n = XLENGTH(col);
        if n <= 0 {
            return NA_REAL;
        }
        let i = i % n as usize;
        if TYPEOF(col) == SEXPTYPE::REALSXP {
            *REAL(col).add(i)
        } else if TYPEOF(col) == SEXPTYPE::INTSXP || TYPEOF(col) == SEXPTYPE::LGLSXP {
            let v = *INTEGER(col).add(i);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        }
    }
}

pub(crate) unsafe fn recycle_posixlt_component(x: SEXP, n: R_xlen_t) -> SEXP {
    unsafe {
        let nx = XLENGTH(x);
        if n <= 0 {
            return x;
        }
        if nx == n {
            return x;
        }
        if nx <= 0 {
            return crate::mainutils::builtin::xlengthgets(x, n);
        }
        let y = crate::mainutils::builtin::xlengthgets(x, n);
        let _y = protect(y);
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n {
                    *REAL(y).add(i as usize) = *REAL(x).add((i % nx) as usize);
                }
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                for i in 0..n {
                    *INTEGER(y).add(i as usize) = *INTEGER(x).add((i % nx) as usize);
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                for i in 0..n {
                    SET_STRING_ELT(y, i, STRING_ELT(x, i % nx));
                }
            }
            _ => {}
        }
        let names = getAttrib(x, R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            let nn = XLENGTH(names);
            if nn > 0 {
                let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
                let _on = protect(out_names);
                for i in 0..n {
                    SET_STRING_ELT(out_names, i, STRING_ELT(names, i % nn));
                }
                setAttrib(y, R_NamesSymbol(), out_names);
            }
        }
        y
    }
}

/// POSIXlt component names.
pub static ltnames: [&str; 11] = [
    "sec", "min", "hour", "mday", "mon", "year", "wday", "yday", "isdst", "zone", "gmtoff",
];

// ---------------------------------------------------------------------------
// Core date arithmetic macros / inline functions
// ---------------------------------------------------------------------------

/// Leap year test (works on absolute years, e.g. 2000, 1900).
/// Returns true if `year` is a leap year.
#[inline]
pub fn isleap(year: c_int) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Number of days in a year (absolute year, e.g. 2000).
/// Ported from `days_in_year` macro in datetime.c.
#[inline]
pub fn days_in_year(year: c_int) -> c_int {
    if isleap(year) { 366 } else { 365 }
}

/// Number of days in a month.
///
/// `mon` is 0-based month (0=Jan, 11=Dec).
/// `yr` is years since 1900 (as in struct tm).
/// Ported from `days_in_month` macro in datetime.c.
#[inline]
pub fn days_in_month(mon: c_int, yr: c_int) -> c_int {
    if mon == 1 && isleap(1900 + yr) {
        29
    } else {
        month_days[mon as usize] as c_int
    }
}

// ---------------------------------------------------------------------------
// Rust representation of struct tm (simplified, standalone)
// ---------------------------------------------------------------------------

/// Simplified C `struct tm` equivalent, using R's NA_INTEGER for missing values.
///
/// Fields follow the C `struct tm` convention:
/// - `tm_year`: years since 1900
/// - `tm_mon`:  months since January (0-11)
/// - `tm_mday`: day of month (1-31)
/// - `tm_wday`: days since Sunday (0-6)
/// - `tm_yday`: days since January 1 (0-365)
/// - `tm_isdst`: Daylight Saving Time flag (-1=unknown, 0=no, 1=yes)
/// - `tm_gmtoff`: offset from UTC in seconds (BSD/glibc extension)
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct stm {
    pub tm_sec: c_int,
    pub tm_min: c_int,
    pub tm_hour: c_int,
    pub tm_mday: c_int,
    pub tm_mon: c_int,
    pub tm_year: c_int,
    pub tm_wday: c_int,
    pub tm_yday: c_int,
    pub tm_isdst: c_int,
    pub tm_gmtoff: c_long,
    pub tm_zone: *const std::os::raw::c_char,
}

impl stm {
    /// Create a new zero-initialized stm.
    pub fn new() -> Self {
        Self {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: -1,
            tm_gmtoff: 0,
            tm_zone: std::ptr::null(),
        }
    }
}

// ---------------------------------------------------------------------------
// validate_tm -- adjust a struct tm to be a valid scalar date-time
// ---------------------------------------------------------------------------

/// Adjust a `stm` to be a valid scalar date-time.
///
/// Returns:
/// - `0` if already valid,
/// - a positive integer approximating the number of corrections done,
/// - `-1` if invalid and uncorrectable.
///
/// Ported from `validate_tm()` in datetime.c.
pub fn validate_tm(tm: &mut stm) -> c_int {
    if tm.tm_year > c_int::MAX - 1900 || tm.tm_year < c_int::MIN + 1900 {
        return -1;
    }
    let mut tmp: c_int;
    let mut res: c_int = 0;

    // Fix seconds
    if tm.tm_sec < 0 || tm.tm_sec > 60 {
        res += 1;
        tmp = tm.tm_sec / 60;
        tm.tm_sec -= 60 * tmp;
        tm.tm_min += tmp;
        if tm.tm_sec < 0 {
            tm.tm_sec += 60;
            tm.tm_min -= 1;
        }
    }

    // Fix minutes
    if tm.tm_min < 0 || tm.tm_min > 59 {
        res += 1;
        tmp = tm.tm_min / 60;
        tm.tm_min -= 60 * tmp;
        tm.tm_hour += tmp;
        if tm.tm_min < 0 {
            tm.tm_min += 60;
            tm.tm_hour -= 1;
        }
    }

    // Special case: 24:00:00
    if tm.tm_hour == 24 && tm.tm_min == 0 && tm.tm_sec == 0 {
        tm.tm_hour = 0;
        tm.tm_mday += 1;
        if tm.tm_mon >= 0 && tm.tm_mon <= 11 && tm.tm_mday > days_in_month(tm.tm_mon, tm.tm_year) {
            tm.tm_mon += 1;
            tm.tm_mday = 1;
            if tm.tm_mon == 12 {
                tm.tm_year += 1;
                tm.tm_mon = 0;
            }
        }
    } else if tm.tm_hour < 0 || tm.tm_hour > 23 {
        res += 1;
        tmp = tm.tm_hour / 24;
        tm.tm_hour -= 24 * tmp;
        tm.tm_mday += tmp;
        if tm.tm_hour < 0 {
            tm.tm_hour += 24;
            tm.tm_mday -= 1;
        }
    }

    // Fix months (defer fixing mday until we know the year)
    if tm.tm_mon < 0 || tm.tm_mon > 11 {
        res += 1;
        tmp = tm.tm_mon / 12;
        tm.tm_mon -= 12 * tmp;
        tm.tm_year += tmp;
        if tm.tm_mon < 0 {
            tm.tm_mon += 12;
            tm.tm_year -= 1;
        }
    }

    // A limit on the loops of about 3000x round
    if tm.tm_mday < -1000000 || tm.tm_mday > 1000000 {
        return -1;
    }

    // Handle day overflow > 366 or < -366
    if tm.tm_mday.abs() > 366 {
        res += 1;
        // First spin back until January
        while tm.tm_mon > 0 {
            tm.tm_mon -= 1;
            tm.tm_mday += days_in_month(tm.tm_mon, tm.tm_year);
        }
        // Then spin on/back by years
        while tm.tm_mday < 1 {
            tm.tm_year -= 1;
            tm.tm_mday += 365
                + if isleap(1900 + tm.tm_year) {
                    1i32
                } else {
                    0i32
                };
        }
        loop {
            tmp = 365
                + if isleap(1900 + tm.tm_year) {
                    1i32
                } else {
                    0i32
                };
            if tm.tm_mday <= tmp {
                break;
            }
            tm.tm_mday -= tmp;
            tm.tm_year += 1;
        }
    }

    while tm.tm_mday < 1 {
        res += 1;
        tm.tm_mon -= 1;
        if tm.tm_mon < 0 {
            tm.tm_mon += 12;
            tm.tm_year -= 1;
        }
        tm.tm_mday += days_in_month(tm.tm_mon, tm.tm_year);
    }

    loop {
        tmp = days_in_month(tm.tm_mon, tm.tm_year);
        if tm.tm_mday <= tmp {
            break;
        }
        res += 1;
        tm.tm_mon += 1;
        if tm.tm_mon > 11 {
            tm.tm_mon -= 12;
            tm.tm_year += 1;
        }
        tm.tm_mday -= tmp;
    }

    res
}

// ---------------------------------------------------------------------------
// likely_strftime_overflow
// ---------------------------------------------------------------------------

/// Check whether `tm_year + 1900` is likely to overflow a C `int`
/// when passed to strftime.
///
/// Ported from `likely_strftime_overflow()` in datetime.c.
pub fn likely_strftime_overflow(tm: &stm) -> bool {
    let year = 1900.0 + tm.tm_year as c_double;
    // Assume 32-bit int (SIZEOF_INT <= 4), which is the common case
    year > c_int::MAX as c_double || year < c_int::MIN as c_double
}

// ---------------------------------------------------------------------------
// mkdate00 -- compute day number and fix tm_yday/tm_wday
// ---------------------------------------------------------------------------

/// Compute the day number (days since epoch 1970-01-01) from a `stm`,
/// and fix `tm_yday` and `tm_wday`.
///
/// Returns the day number as a double, or `NA_REAL` if inputs are NA.
///
/// Ported from `mkdate00()` in datetime.c.
pub fn mkdate00(tm: &mut stm) -> c_double {
    if tm.tm_mday == NA_INTEGER || tm.tm_year == NA_INTEGER || tm.tm_mon == NA_INTEGER {
        tm.tm_yday = NA_INTEGER;
        tm.tm_wday = NA_INTEGER;
        return NA_REAL;
    }

    let mut day = tm.tm_mday - 1;
    let mut year0 = tm.tm_year;
    let mut excess: c_double = 0.0;

    if year0 >= 400 {
        excess = (year0 / 400 - 1) as c_double;
        year0 -= (excess as c_int) * 400;
    } else if year0 < 0 {
        excess = -1.0 - (-year0 / 400) as c_double;
        year0 -= (excess as c_int) * 400;
    }
    year0 += 1900;

    // Add days for preceding months in the current year
    for i in 0..tm.tm_mon {
        day += month_days[i as usize];
    }
    if tm.tm_mon > 1 && isleap(year0) {
        day += 1;
    }
    tm.tm_yday = day;

    // Count days from 1970
    if year0 > 1970 {
        for year in 1970..year0 {
            day += days_in_year(year);
        }
    } else if year0 < 1970 {
        for year in (year0..1970).rev() {
            day -= days_in_year(year);
        }
    }

    // Weekday: Epoch day (1970-01-01) was a Thursday (4)
    tm.tm_wday = ((day % 7) + 4) % 7;
    if tm.tm_wday < 0 {
        tm.tm_wday += 7;
    }

    day as c_double + excess * 146097.0
}

// ---------------------------------------------------------------------------
// timegm00 -- convert struct tm to seconds since epoch (UTC)
// ---------------------------------------------------------------------------

/// Substitute for timegm (which is non-POSIX).
///
/// Converts a `stm` to seconds since the Unix epoch (1970-01-01 00:00:00 UTC),
/// without checking. Returns double for wider range than 32-bit time_t.
///
/// Ported from `timegm00()` in datetime.c.
pub fn timegm00(tm: &mut stm) -> c_double {
    let day = mkdate00(tm);
    if day == NA_REAL {
        return NA_REAL;
    }
    tm.tm_sec as c_double
        + (tm.tm_min * 60) as c_double
        + (tm.tm_hour * 3600) as c_double
        + day * 86400.0
}

// ---------------------------------------------------------------------------
// julian2dtime -- convert Julian date to POSIXct-like seconds
// ---------------------------------------------------------------------------

/// Convert a Julian date (days since 1970-01-01, R "Date" convention)
/// to a `stm` in UTC.
///
/// Returns true if the conversion was successful, false otherwise.
///
/// Ported from the date arithmetic in `do_D2POSIXlt()` in datetime.c.
#[allow(clippy::absurd_extreme_comparisons)]
pub fn julian2dtime(x_i: c_double, tm: &mut stm) -> bool {
    if !x_i.is_finite() {
        return false;
    }

    /* every 400 years is exactly 146097 days long and the pattern is repeated */
    let rounds = (x_i.floor() / 146097.0).floor();
    let mut day = (x_i.floor() - 146097.0 * rounds) as c_int;
    tm.tm_hour = 0;
    tm.tm_min = 0;
    tm.tm_sec = 0;

    /* weekday: 1970-01-01 was a Thursday */
    tm.tm_wday = ((day % 7) + 4) % 7;
    if tm.tm_wday < 0 {
        tm.tm_wday += 7;
    }

    /* year & day within year */
    let mut y: c_int = 1970;
    if day >= 0 {
        while day >= days_in_year(y) {
            day -= days_in_year(y);
            y += 1;
        }
    } else {
        while day < 0 {
            y -= 1;
            day += days_in_year(y);
        }
    }

    // Avoid overflows: GNU computes year0 in double to detect overflow
    // before narrowing to int.
    let year0_f = (y - 1900) as c_double + rounds * 400.0;
    if year0_f > c_int::MAX as c_double || year0_f < c_int::MIN as c_double {
        return false;
    }
    let year0 = year0_f as c_int;

    tm.tm_year = year0;
    tm.tm_yday = day;

    /* month within year */
    let mut mon: c_int = 0;
    while day >= days_in_month(mon, tm.tm_year) {
        day -= days_in_month(mon, tm.tm_year);
        mon += 1;
    }
    tm.tm_mon = mon;
    tm.tm_mday = day + 1;
    tm.tm_isdst = 0; /* no dst in GMT */

    true
}

// ---------------------------------------------------------------------------
// dtime2julian -- convert POSIXct-like stm back to Julian date
// ---------------------------------------------------------------------------

/// Convert a `stm` back to a Julian date (days since 1970-01-01).
///
/// Handles NA and invalid inputs by returning NA_REAL.
///
/// Ported from the date arithmetic in `do_POSIXlt2D()` in datetime.c.
pub fn dtime2julian(
    secs: c_double,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
) -> c_double {
    if !secs.is_finite() {
        return secs;
    }
    if tm_min == NA_INTEGER
        || tm_hour == NA_INTEGER
        || tm_mday == NA_INTEGER
        || tm_mon == NA_INTEGER
        || tm_year == NA_INTEGER
    {
        return NA_REAL;
    }

    let fsecs = secs.floor();
    let mut tm = stm::new();
    // avoid (int) NAN
    tm.tm_sec = if secs.is_finite() {
        fsecs as c_int
    } else {
        NA_INTEGER
    };
    tm.tm_min = tm_min;
    tm.tm_hour = tm_hour;
    tm.tm_mday = tm_mday;
    tm.tm_mon = tm_mon;
    tm.tm_year = tm_year;
    tm.tm_isdst = 0;

    if validate_tm(&mut tm) < 0 {
        return NA_REAL;
    }

    mkdate00(&mut tm)
}

// ---------------------------------------------------------------------------
// POSIXlt component name accessors
// ---------------------------------------------------------------------------

/// Return the POSIXlt component name for the given index (0-based).
///
/// Valid indices are 0..10. Returns an empty string for out-of-range indices.
pub fn lt_component_name(index: usize) -> &'static str {
    if index < ltnames.len() {
        ltnames[index]
    } else {
        ""
    }
}

// ---------------------------------------------------------------------------
// R_ISLeapYear -- R-level leap year check (years since 1900)
// ---------------------------------------------------------------------------

/// Check whether a year (expressed as years since 1900, matching R's Date convention)
/// is a leap year.
///
/// This is the R-facing version; for absolute years use `isleap()`.
#[inline]
pub fn R_ISLeapYear(year: c_int) -> bool {
    isleap(year + 1900)
}

// ---------------------------------------------------------------------------
// Date arithmetic: days in 400-year cycle
// ---------------------------------------------------------------------------

/// Number of days in a 400-year Gregorian cycle.
pub const DAYS_IN_400_YEARS: c_double = 146097.0;

/// Convert a number of days since epoch to year, day-of-year, month, day-of-month.
///
/// This is the core algorithm extracted from the various conversion functions.
/// Returns `None` if the day is out of representable range.
pub fn days_to_ymd(mut dday: c_double) -> Option<(c_int, c_int, c_int, c_int)> {
    // Bail out for unreasonably large values
    if dday.abs() > 784368402400.0 {
        return None;
    }

    /* every 400 years is exactly 146097 days long and the pattern is repeated */
    let rounds = (dday.floor() / 146097.0).floor();
    dday -= 146097.0 * rounds;
    let mut y = (1970.0 + rounds * 400.0) as c_int;

    if dday >= 0.0 {
        while dday >= days_in_year(y) as c_double {
            dday -= days_in_year(y) as c_double;
            y += 1;
        }
    } else {
        while dday < 0.0 {
            y -= 1;
            dday += days_in_year(y) as c_double;
        }
    }

    let yr = y - 1900; // tm_year
    let mut day = dday as c_int; // tm_yday
    let yday = day;

    /* month within year */
    let mut mon: c_int = 0;
    while day >= days_in_month(mon, yr) {
        day -= days_in_month(mon, yr);
        mon += 1;
    }

    Some((yr, yday, mon, day + 1))
}

// ---------------------------------------------------------------------------
// mktime0 -- convert stm to seconds since epoch (UTC or local)
// ---------------------------------------------------------------------------
