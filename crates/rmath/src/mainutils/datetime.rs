#![allow(unused_variables)]
#![allow(unused_assignments)]
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2000-2026  The R Core Team.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *
 *      Interfaces to POSIX date-time conversion functions.
 *
 *  Ported from R source: src/main/datetime.c
 */
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_double, c_int, c_long};

// Timezone/datetime go through the ported tzone layer (R's own
// extra/tzone) instead of libc: one implementation on every host.
use crate::tzone::stm as tz_tm;
use crate::tzone::{R_gmtime_r, R_localtime_r, R_mktime, R_tzname};
use crate::tzone_strftime::R_strftime;
use crate::tzone_strftime::stm as sf_tm;

type time_t = i64;

use crate::mainutils::rstrptime::R_strptime;
use crate::sexp::accessors::*;
use crate::sexp::attrib_core::{R_ClassSymbol, R_NamesSymbol, R_classgets, getAttrib, setAttrib};

use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::*;
use crate::sexp::globals::{R_MissingArg, R_NaString, R_NilValue};
use crate::sexp::protect::*;
use crate::sexp::symbol::Rf_install;

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
            if v == NA_INTEGER {
                NA_REAL
            } else {
                v as f64
            }
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

    // Avoid overflows
    let year0 = y - 1900 + (rounds as c_int) * 400;
    if year0 > c_int::MAX || year0 < c_int::MIN {
        return false;
    }

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

/// Convert a `stm` to seconds since the Unix epoch.
///
/// If `local` is true, uses `mktime` (local timezone); otherwise uses
/// `timegm00` (UTC). Returns -1.0 on error.
///
/// Ported from `mktime0()` in datetime.c.
fn mktime0(tm: &mut stm, local: bool) -> c_double {
    if validate_tm(tm) < 0 {
        return -1.0;
    }
    if !local {
        return timegm00(tm);
    }

    let mut ctm: tz_tm = unsafe { std::mem::zeroed() };
    ctm.tm_sec = tm.tm_sec;
    ctm.tm_min = tm.tm_min;
    ctm.tm_hour = tm.tm_hour;
    ctm.tm_mday = tm.tm_mday;
    ctm.tm_mon = tm.tm_mon;
    ctm.tm_year = tm.tm_year;
    ctm.tm_isdst = tm.tm_isdst;
    let result = unsafe { R_mktime(&mut ctm) };
    tm.tm_sec = ctm.tm_sec;
    tm.tm_min = ctm.tm_min;
    tm.tm_hour = ctm.tm_hour;
    tm.tm_mday = ctm.tm_mday;
    tm.tm_mon = ctm.tm_mon;
    tm.tm_year = ctm.tm_year;
    tm.tm_isdst = ctm.tm_isdst;
    tm.tm_wday = ctm.tm_wday;
    tm.tm_yday = ctm.tm_yday;
    tm.tm_gmtoff = ctm.tm_gmtoff;
    tm.tm_zone = ctm.tm_zone;

    if result == -1 {
        return -1.0;
    }

    result as c_double
}

// ---------------------------------------------------------------------------
// localtime0 -- convert seconds since epoch to stm (UTC or local)
// ---------------------------------------------------------------------------

/// Convert a timestamp (seconds since epoch) to a `stm`.
///
/// If `local` is true, uses `localtime_r` (local timezone); otherwise uses
/// UTC conversion via the internal algorithm.
///
/// Ported from `localtime0()` in datetime.c.
fn localtime0(tp: *const c_double, local: bool, ltm: &mut stm) -> bool {
    let d = unsafe { *tp };
    if !d.is_finite() {
        ltm.tm_year = NA_INTEGER;
        ltm.tm_mon = NA_INTEGER;
        ltm.tm_mday = NA_INTEGER;
        ltm.tm_yday = NA_INTEGER;
        ltm.tm_wday = NA_INTEGER;
        ltm.tm_hour = NA_INTEGER;
        ltm.tm_min = NA_INTEGER;
        ltm.tm_sec = NA_INTEGER;
        ltm.tm_isdst = -1;
        return false;
    }

    // Bail out for unreasonable values
    let dday = (d / 86400.0).floor();
    if dday.abs() > 784368402400.0 {
        ltm.tm_year = NA_INTEGER;
        ltm.tm_mon = NA_INTEGER;
        ltm.tm_mday = NA_INTEGER;
        ltm.tm_yday = NA_INTEGER;
        ltm.tm_wday = NA_INTEGER;
        ltm.tm_hour = NA_INTEGER;
        ltm.tm_min = NA_INTEGER;
        ltm.tm_sec = NA_INTEGER;
        ltm.tm_isdst = -1;
        return false;
    }

    // Convert double to time_t (handle negative values correctly)
    let mut t = d as time_t;
    if d < 0.0 && d != (t as c_double) {
        t -= 1;
    }

    let mut ctm: tz_tm = unsafe { std::mem::zeroed() };
    let res = unsafe {
        if local {
            R_localtime_r(&t, &mut ctm)
        } else {
            R_gmtime_r(&t, &mut ctm)
        }
    };

    if res.is_null() {
        return false;
    }

    ltm.tm_sec = ctm.tm_sec;
    ltm.tm_min = ctm.tm_min;
    ltm.tm_hour = ctm.tm_hour;
    ltm.tm_mday = ctm.tm_mday;
    ltm.tm_mon = ctm.tm_mon;
    ltm.tm_year = ctm.tm_year;
    ltm.tm_wday = ctm.tm_wday;
    ltm.tm_yday = ctm.tm_yday;
    ltm.tm_isdst = ctm.tm_isdst;
    ltm.tm_gmtoff = ctm.tm_gmtoff;
    ltm.tm_zone = ctm.tm_zone;

    true
}

// ---------------------------------------------------------------------------
// makelt -- populate POSIXlt vector elements from stm
// ---------------------------------------------------------------------------

/// Populate the POSIXlt list elements for index `i`.
///
/// `ans` is a VECSXP of 11 elements (sec, min, hour, mday, mon, year, wday,
/// yday, isdst, zone, gmtoff). Sets elements 0-8 based on `tm`.
fn makelt(tm: &stm, ans: SEXP, i: R_xlen_t, valid: bool, frac_secs: c_double) {
    if valid {
        unsafe {
            *REAL(VECTOR_ELT(ans, 0)).add(i as usize) = tm.tm_sec as c_double + frac_secs;
            *INTEGER(VECTOR_ELT(ans, 1)).add(i as usize) = tm.tm_min;
            *INTEGER(VECTOR_ELT(ans, 2)).add(i as usize) = tm.tm_hour;
            *INTEGER(VECTOR_ELT(ans, 3)).add(i as usize) = tm.tm_mday;
            *INTEGER(VECTOR_ELT(ans, 4)).add(i as usize) = tm.tm_mon;
            *INTEGER(VECTOR_ELT(ans, 5)).add(i as usize) = tm.tm_year;
            *INTEGER(VECTOR_ELT(ans, 6)).add(i as usize) = tm.tm_wday;
            *INTEGER(VECTOR_ELT(ans, 7)).add(i as usize) = tm.tm_yday;
            *INTEGER(VECTOR_ELT(ans, 8)).add(i as usize) = tm.tm_isdst;
        }
    } else {
        unsafe {
            *REAL(VECTOR_ELT(ans, 0)).add(i as usize) = frac_secs;
            for j in 1..8 {
                *INTEGER(VECTOR_ELT(ans, j)).add(i as usize) = NA_INTEGER;
            }
            *INTEGER(VECTOR_ELT(ans, 8)).add(i as usize) = -1; // isdst
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: create a POSIXlt result vector with names and class
// ---------------------------------------------------------------------------

/// Build the standard POSIXlt VECSXP with 11 named components.
/// Returns (ans, ansnames) both protected.
unsafe fn make_posixlt_skeleton(n: R_xlen_t) -> (SEXP, SEXP) {
    unsafe {
        let nans: c_int = 11;
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, nans as R_xlen_t);
        let _ans_guard = protect(ans);
        for i in 0..9 {
            let sexp: c_int = if i > 0 {
                SEXPTYPE::INTSXP.into()
            } else {
                SEXPTYPE::REALSXP.into()
            };
            SET_VECTOR_ELT(ans, i as R_xlen_t, Rf_allocVector3(sexp, n));
        }
        SET_VECTOR_ELT(ans, 9, Rf_allocVector3(SEXPTYPE::STRSXP, n)); // zone
        SET_VECTOR_ELT(ans, 10, Rf_allocVector3(SEXPTYPE::INTSXP, n)); // gmtoff

        let ansnames = Rf_allocVector3(SEXPTYPE::STRSXP, nans as R_xlen_t);
        let _ansnames_guard = protect(ansnames);
        for i in 0..nans {
            let cstr = CString::new(ltnames[i as usize]).unwrap_or_default();
            SET_STRING_ELT(ansnames, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        (ans, ansnames)
    }
}

fn tm_zone_string(p: *const std::os::raw::c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
    }
}

fn tz_is_utc(tz: &str) -> bool {
    tz == "GMT" || tz == "UTC"
}

/// GNU `as.POSIXlt.default` uses `missing(tz)`. A supplied non-empty tz
/// only relabels `tzone` on an existing POSIXlt.
unsafe fn posixlt_supplied_tz(args: SEXP) -> Option<String> {
    unsafe {
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let named_tz = !tag.is_null()
                && tag != R_NilValue()
                && TYPEOF(tag) == SEXPTYPE::SYMSXP
                && {
                    let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                        .to_string_lossy();
                    name == "tz"
                };
            let positional = tag.is_null() || tag == R_NilValue();
            if named_tz || positional {
                let t = CAR(cell);
                if t.is_null() || t == R_NilValue() || t == R_MissingArg() {
                    return None;
                }
                if TYPEOF(t) == SEXPTYPE::STRSXP && XLENGTH(t) > 0 {
                    return Some(charsxp_text(STRING_ELT(t, 0), "tz"));
                }
                return None;
            }
            cell = CDR(cell);
        }
        None
    }
}


unsafe fn set_posixlt_balanced(ans: SEXP) {
    unsafe {
        setAttrib(ans, Rf_install(c"balanced".as_ptr()), Rf_ScalarLogical(TRUE));
    }
}

unsafe fn posixlt_tzone_sexp(tz: &str, is_utc: bool) -> SEXP {
    unsafe {
        if is_utc {
            let name = if tz.is_empty() { "UTC" } else { tz };
            Rf_mkString(CString::new(name).unwrap_or_default().as_ptr())
        } else {
            let label = if tz.is_empty() {
                crate::tzone::timezone_override()
                    .or_else(|| std::env::var("TZ").ok())
                    .unwrap_or_default()
            } else {
                tz.to_string()
            };

            let tzone = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
            for (j, s) in [label, tzname_str(0), tzname_str(1)].iter().enumerate() {
                let cs = CString::new(s.as_str()).unwrap_or_default();
                SET_STRING_ELT(tzone, j as R_xlen_t, Rf_mkChar(cs.as_ptr()));
            }
            tzone
        }
    }
}

unsafe fn finish_posixlt(ans: SEXP, ansnames: SEXP, tzone: SEXP) {
    unsafe {
        setAttrib(ans, R_NamesSymbol(), ansnames);
        let klass = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _klass_guard = protect(klass);
        SET_STRING_ELT(klass, 0, Rf_mkChar(c"POSIXlt".as_ptr()));
        SET_STRING_ELT(klass, 1, Rf_mkChar(c"POSIXt".as_ptr()));
        R_classgets(ans, klass);
        if !tzone.is_null() && tzone != R_NilValue() && TYPEOF(tzone) == SEXPTYPE::STRSXP {
            setAttrib(ans, Rf_install(c"tzone".as_ptr()), tzone);
        }
        set_posixlt_balanced(ans);
    }
}

/// GNU `.Internal(as.POSIXlt(x, tz))` for numeric POSIXct seconds.
pub unsafe fn convert_posixct_to_posixlt(x: SEXP, tz: &str) -> SEXP {
    unsafe {
        let is_utc = tz_is_utc(tz);
        let tzsi = TzSetup::prepare();
        if !is_utc {
            if tz.is_empty() {
                if let Some(env_tz) = crate::tzone::timezone_override().or_else(|| std::env::var("TZ").ok())
                {
                    tzsi.set(&env_tz);
                }
            } else {
                tzsi.set(tz);
            }
        }

        let n = XLENGTH(x);
        let (ans, ansnames) = make_posixlt_skeleton(n);
        let _ans_guard = protect(ans);
        let _ansnames_guard = protect(ansnames);
        let tzone = posixlt_tzone_sexp(tz, is_utc);
        let _tz_guard = protect(tzone);
        for i in 0..n {
            let mut dummy = stm::new();
            let d = if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER {
                    NA_REAL
                } else {
                    v as c_double
                }
            } else {
                *REAL(x).add(i as usize)
            };

            let valid = if R_FINITE(d) {
                localtime0(&d as *const c_double, !is_utc, &mut dummy)
            } else {
                false
            };
            makelt(&dummy, ans, i, valid, if valid { d - d.floor() } else { d });
            let zone = if valid && dummy.tm_isdst >= 0 {
                let named = tm_zone_string(dummy.tm_zone);
                if named.is_empty() {
                    tzname_str(dummy.tm_isdst.clamp(0, 1) as usize)
                } else {
                    named
                }
            } else {
                String::new()
            };
            SET_STRING_ELT(
                VECTOR_ELT(ans, 9),
                i,
                Rf_mkChar(CString::new(zone).unwrap_or_default().as_ptr()),
            );
            *INTEGER(VECTOR_ELT(ans, 10)).add(i as usize) = if valid {
                dummy.tm_gmtoff as c_int
            } else {
                NA_INTEGER
            };
        }
        finish_posixlt(ans, ansnames, tzone);
        let names = getAttrib(x, R_NamesSymbol());
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == n
        {
            setAttrib(VECTOR_ELT(ans, 5), R_NamesSymbol(), names);
        }
        ans

    }
}

/// GNU `.Internal(as.POSIXct(x, tz))` for a POSIXlt list.
pub unsafe fn convert_posixlt_to_posixct(x: SEXP, tz: &str) -> SEXP {
    unsafe {
        let is_utc = tz_is_utc(tz);
        let tzsi = TzSetup::prepare();
        if !is_utc && !tz.is_empty() {
            tzsi.set(tz);
        }
        do_asPOSIXct(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            Rf_cons(
                x,
                Rf_cons(
                    Rf_mkString(CString::new(tz).unwrap_or_default().as_ptr()),
                    R_NilValue(),
                ),
            ),
            std::ptr::null_mut(),
        )
    }
}


// ---------------------------------------------------------------------------
// do_asPOSIXlt -- .Internal(as.POSIXlt(x, tz))
// ---------------------------------------------------------------------------

/// Convert a POSIXct numeric vector to a POSIXlt list.
///
/// Ported from `do_asPOSIXlt()` in datetime.c.
pub unsafe fn do_asPOSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::REALSXP && TYPEOF(x) != SEXPTYPE::INTSXP {
            std::panic::panic_any(RError {
                message: "invalid 'x' value: not numeric".to_string(),
            });
        }
        let stz = CADR(args);
        let tz = if stz.is_null() || stz == R_NilValue() || stz == R_MissingArg() {
            String::new()
        } else if TYPEOF(stz) == SEXPTYPE::STRSXP && XLENGTH(stz) > 0 {
            charsxp_text(STRING_ELT(stz, 0), "tz")
        } else {
            String::new()
        };
        convert_posixct_to_posixlt(x, &tz)
    }
}

// ---------------------------------------------------------------------------
// do_asPOSIXct -- .Internal(as.POSIXct(x, tz))
// ---------------------------------------------------------------------------

/// Convert a POSIXlt list to a POSIXct numeric vector.
///
/// Ported from `do_asPOSIXct()` in datetime.c.
pub unsafe fn do_asPOSIXct(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _x_guard = protect(x);
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "a valid \"POSIXlt\" object is a list of at least 9 elements".to_string(),
            });
        }
        let stz = CADR(args);
        let tz = if stz.is_null() || stz == R_NilValue() || stz == R_MissingArg() {
            String::new()
        } else if TYPEOF(stz) == SEXPTYPE::STRSXP && XLENGTH(stz) > 0 {
            charsxp_text(STRING_ELT(stz, 0), "tz")
        } else {
            String::new()
        };
        let is_utc = tz_is_utc(&tz);

        let mut n: R_xlen_t = 0;
        let mut nlen = [0i64; 9];
        for i in 0..6 {
            let len = XLENGTH(VECTOR_ELT(x, i as R_xlen_t));
            nlen[i as usize] = len;
            if len > n {
                n = len;
            }
        }
        let len8 = XLENGTH(VECTOR_ELT(x, 8));
        nlen[8] = len8;
        if len8 > n {
            n = len8;
        }

        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _ans_guard = protect(ans);

        for i in 0..n {
            let iu = i as usize;
            let secs = posixlt_real_elt(VECTOR_ELT(x, 0), iu);

            let fsecs = secs.floor();

            let mut tm = stm::new();
            tm.tm_sec = if R_FINITE(secs) {
                fsecs as c_int
            } else {
                NA_INTEGER
            };
            tm.tm_min = posixlt_int_elt(VECTOR_ELT(x, 1), iu);
            tm.tm_hour = posixlt_int_elt(VECTOR_ELT(x, 2), iu);
            tm.tm_mday = posixlt_int_elt(VECTOR_ELT(x, 3), iu);
            tm.tm_mon = posixlt_int_elt(VECTOR_ELT(x, 4), iu);
            tm.tm_year = posixlt_int_elt(VECTOR_ELT(x, 5), iu);
            tm.tm_isdst = if is_utc {
                0
            } else {
                posixlt_int_elt(VECTOR_ELT(x, 8), iu)
            };

            if !R_FINITE(secs) {
                *REAL(ans).add(iu) = secs;
            } else if tm.tm_min == NA_INTEGER
                || tm.tm_hour == NA_INTEGER
                || tm.tm_mday == NA_INTEGER
                || tm.tm_mon == NA_INTEGER
                || tm.tm_year == NA_INTEGER
            {
                *REAL(ans).add(iu) = NA_REAL;
            } else {
                let tmp = mktime0(&mut tm, !is_utc);
                // GNU datetime.c:1240-1259 (no errno): -1 is NA unless
                // this is the epoch-minus-one gotcha (sec==59) or the
                // sec=58 probe returns -2.
                let failed = tmp == -1.0
                    && tm.tm_sec != 59
                    && {
                        let mut probe = tm;
                        probe.tm_sec = 58;
                        mktime0(&mut probe, !is_utc) != -2.0
                    };
                *REAL(ans).add(iu) = if failed {
                    NA_REAL
                } else {
                    tmp + (secs - fsecs)
                };
            }
        }

        let names = if XLENGTH(x) >= 6 {
            getAttrib(VECTOR_ELT(x, 5), R_NamesSymbol())
        } else {
            R_NilValue()
        };
        if !names.is_null() && names != R_NilValue() && XLENGTH(names) == n {
            setAttrib(ans, R_NamesSymbol(), names);
        }
        ans
    }
}



fn use_dig_secs(secs: &[f64], digits: i32) -> i32 {
    let mut np = digits.min(6);
    if np < 1 {
        return 0;
    }
    let finite: Vec<f64> = secs.iter().copied().filter(|s| s.is_finite()).collect();
    if finite.is_empty() {
        return np;
    }
    for i in 0..np {
        let ti = 10f64.powi(i);
        if finite
            .iter()
            .all(|&s| (s - (s * ti).trunc() / ti).abs() < 1e-6)
        {
            return i;
        }
    }
    np
}

fn expand_os_format(fmt: &str, secs: f64, fsecs: f64, tm_sec: i32, digits: i32, ns0: &mut i32) -> String {
    let Some(pos) = fmt.find("%OS") else {
        return fmt.to_string();
    };
    let after = pos + 3;
    let next = fmt.as_bytes().get(after).copied();
    let (ns, nused) = match next {
        Some(b) if b.is_ascii_digit() => ((b - b'0') as i32, 4),
        _ => {
            if *ns0 < 0 {
                *ns0 = if digits == NA_INTEGER { 0 } else { digits };
            }
            (*ns0, 3)
        }
    };
    let ns = ns.clamp(0, 6);
    let mut out = String::new();
    out.push_str(&fmt[..pos]);
    if ns > 0 {
        let s = tm_sec as f64 + (secs - fsecs);
        let t = 10f64.powi(ns);
        let s = ((s * t) as i32) as f64 / t;
        out.push_str(&format!("{s:0width$.prec$}", width = (ns + 3) as usize, prec = ns as usize));
        out.push_str(&fmt[pos + nused..]);
    } else {
        out.push_str("%S");
        out.push_str(&fmt[pos + nused..]);
    }
    out
}

unsafe fn format_posix_named_arg(args: SEXP, name: &str, pos: usize) -> SEXP {
    unsafe {
        let mut cell = CDR(args);
        let mut i = 0usize;
        let mut positional = R_NilValue();
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let tagged = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if tagged == name {
                return CAR(cell);
            }
            if tagged.is_empty() {
                if i == pos {
                    positional = CAR(cell);
                }
                i += 1;
            }
            cell = CDR(cell);
        }
        positional
    }
}

/// Exact named match, then leftover positionals — GNU closure matching
/// without partial names. `sapply(dd, as.character.POSIXt, x = xf)` binds
/// `x = xf` and the untagged `dd[[i]]` to `digits`.
unsafe fn match_named_then_positional(args: SEXP, formals: &[&str]) -> Vec<SEXP> {
    unsafe {
        let n = formals.len();
        let mut out = vec![R_NilValue(); n];
        let mut filled = vec![false; n];
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy();
                if let Some(i) = formals.iter().position(|f| *f == name) {
                    out[i] = CAR(cell);
                    filled[i] = true;
                }
            }
            cell = CDR(cell);
        }
        let mut next = 0usize;
        cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let untagged = tag.is_null() || tag == R_NilValue();
            if untagged {
                while next < n && filled[next] {
                    next += 1;
                }
                if next < n {
                    out[next] = CAR(cell);
                    filled[next] = true;
                    next += 1;
                }
            }
            cell = CDR(cell);
        }
        out
    }
}



// ---------------------------------------------------------------------------
// do_formatPOSIXlt -- .Internal(format.POSIXlt(x, format, usetz, ...))
// ---------------------------------------------------------------------------

/// Format a POSIXlt object as a character string using strftime.
///
/// Ported from `do_formatPOSIXlt()` in datetime.c.
pub unsafe fn do_formatPOSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _x_guard = protect(x);

        // x must be VECSXP with at least 9 components
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "invalid 'x' argument".to_string(),
            });
        }

        // Get format string (CADR(args))
        let sformat = CADR(args);
        if TYPEOF(sformat) != SEXPTYPE::STRSXP || XLENGTH(sformat) == 0 {
            std::panic::panic_any(RError {
                message: "invalid 'format' argument".to_string(),
            });
        }

        // Determine lengths
        let nn = std::cmp::min(LENGTH(x), 11);
        let mut nlen = [0i64; 11];
        let mut n: R_xlen_t = 0;
        for i in 0..nn {
            let len = XLENGTH(VECTOR_ELT(x, i as R_xlen_t));
            nlen[i as usize] = len;
            if len > n {
                n = len;
            }
        }

        let m = XLENGTH(sformat);
        let N = if n > 0 { std::cmp::max(m, n) } else { 0 };
        let digits_arg = CADDDR(args);
        let digits = if digits_arg.is_null() || digits_arg == R_NilValue() {
            NA_INTEGER
        } else {
            crate::mainutils::coerce::asInteger(digits_arg)
        };
        let mut ns0 = -1i32;

        let ans = Rf_allocVector3(SEXPTYPE::STRSXP, N);
        let _ans_guard = protect(ans);

        for i in 0..N {
            let iu = i as usize;
            let secs = posixlt_real_elt(VECTOR_ELT(x, 0), iu);
            let fsecs = secs.floor();

            let mut ctm: tz_tm = std::mem::zeroed();

            if R_FINITE(secs) && fsecs >= c_int::MIN as c_double && fsecs <= c_int::MAX as c_double
            {
                ctm.tm_sec = fsecs as c_int;
            } else {
                ctm.tm_sec = 0;
            }
            ctm.tm_min = posixlt_int_elt(VECTOR_ELT(x, 1), iu);
            ctm.tm_hour = posixlt_int_elt(VECTOR_ELT(x, 2), iu);
            ctm.tm_mday = posixlt_int_elt(VECTOR_ELT(x, 3), iu);
            ctm.tm_mon = posixlt_int_elt(VECTOR_ELT(x, 4), iu);
            ctm.tm_year = posixlt_int_elt(VECTOR_ELT(x, 5), iu);
            ctm.tm_wday = posixlt_int_elt(VECTOR_ELT(x, 6), iu);
            ctm.tm_yday = posixlt_int_elt(VECTOR_ELT(x, 7), iu);
            ctm.tm_isdst = posixlt_int_elt(VECTOR_ELT(x, 8), iu);



            if !R_FINITE(secs) {
                // NA, NaN, Inf, -Inf
                let s = if R_IsNA(secs) {
                    // NA_STRING equivalent: use empty string
                    ""
                } else if ISNAN(secs) {
                    "NaN"
                } else if secs > 0.0 {
                    "Inf"
                } else {
                    "-Inf"
                };
                let cstr = CString::new(s).unwrap_or_default();
                SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
            } else if ctm.tm_min == NA_INTEGER
                || ctm.tm_hour == NA_INTEGER
                || ctm.tm_mday == NA_INTEGER
                || ctm.tm_mon == NA_INTEGER
                || ctm.tm_year == NA_INTEGER
            {
                // NA_STRING
                let cstr = c"";
                SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
            } else {
                let mut tm_check = stm::new();
                tm_check.tm_sec = ctm.tm_sec;
                tm_check.tm_min = ctm.tm_min;
                tm_check.tm_hour = ctm.tm_hour;
                tm_check.tm_mday = ctm.tm_mday;
                tm_check.tm_mon = ctm.tm_mon;
                tm_check.tm_year = ctm.tm_year;
                tm_check.tm_isdst = ctm.tm_isdst;

                if validate_tm(&mut tm_check) < 0 || likely_strftime_overflow(&tm_check) {
                    let cstr = c"";
                    SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                } else {
                    // Copy validated values back
                    ctm.tm_sec = tm_check.tm_sec;
                    ctm.tm_min = tm_check.tm_min;
                    ctm.tm_hour = tm_check.tm_hour;
                    ctm.tm_mday = tm_check.tm_mday;
                    ctm.tm_mon = tm_check.tm_mon;
                    ctm.tm_year = tm_check.tm_year;

                    // Get format string
                    let fmt_charsxp = STRING_ELT(sformat, (i % m) as R_xlen_t);
                    let fmt_ptr = CHAR(fmt_charsxp);
                    let fmt_raw = if fmt_ptr.is_null() {
                        "%Y-%m-%d %H:%M:%S"
                    } else {
                        CStr::from_ptr(fmt_ptr)
                            .to_str()
                            .unwrap_or("%Y-%m-%d %H:%M:%S")
                    };
                    let fmt_exp = expand_os_format(
                        fmt_raw,
                        secs,
                        fsecs,
                        ctm.tm_sec,
                        digits,
                        &mut ns0,
                    );

                    let mut sf_tm_ctm: sf_tm = std::mem::zeroed();
                    sf_tm_ctm.tm_sec = ctm.tm_sec;
                    sf_tm_ctm.tm_min = ctm.tm_min;
                    sf_tm_ctm.tm_hour = ctm.tm_hour;
                    sf_tm_ctm.tm_mday = ctm.tm_mday;
                    sf_tm_ctm.tm_mon = ctm.tm_mon;
                    sf_tm_ctm.tm_year = ctm.tm_year;
                    sf_tm_ctm.tm_wday = ctm.tm_wday;
                    sf_tm_ctm.tm_yday = ctm.tm_yday;
                    sf_tm_ctm.tm_isdst = ctm.tm_isdst;
                    let mut buf = [0u8; 2049];
                    let res = unsafe {
                        R_strftime(
                            buf.as_mut_ptr(),
                            2048,
                            CString::new(fmt_exp.as_str()).unwrap_or_default().as_ptr()
                                as *const core::ffi::c_char,
                            &sf_tm_ctm,
                        )
                    };

                    if res == 0 {
                        let cstr = c"";
                        SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    } else {
                        let s = std::str::from_utf8(&buf[..res as usize]).unwrap_or("");
                        let cstr = CString::new(s).unwrap_or_default();
                        SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    }
                }
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// glibc_fix -- fill in month/day omitted by the parser
// ---------------------------------------------------------------------------

/// Set mon and mday which the parser does not always set.
/// Use current year/... if none has been specified.
///
/// Specifying mon but not mday nor yday is invalid.
///
/// Ported from `glibc_fix()` in datetime.c.
fn glibc_fix(tm: &mut stm, invalid: &mut bool) {
    unsafe {
        // SystemTime works on every target (wasm yields the epoch,
        // matching the previous wasm-libc facade behavior).
        let t: time_t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut tm0: tz_tm = std::mem::zeroed();
        if unsafe { R_localtime_r(&t, &mut tm0) }.is_null() {
            return;
        }
        if tm.tm_year == NA_INTEGER {
            tm.tm_year = tm0.tm_year;
        }
        if tm.tm_mon != NA_INTEGER && tm.tm_mday != NA_INTEGER {
            return;
        }
        // At least one of the month and the day of the month is missing.
        if tm.tm_yday != NA_INTEGER {
            // Since we have yday, let that take precedence over mon/mday.
            let mut yday = tm.tm_yday;
            let mut mon = 0;
            while mon < 12 {
                let tmp = days_in_month(mon, tm.tm_year);
                if yday < tmp {
                    break;
                }
                yday -= tmp;
                mon += 1;
            }
            tm.tm_mon = mon;
            tm.tm_mday = yday + 1;
        } else {
            if tm.tm_mday == NA_INTEGER {
                if tm.tm_mon != NA_INTEGER {
                    *invalid = true;
                    return;
                }
                tm.tm_mday = tm0.tm_mday;
            }
            if tm.tm_mon == NA_INTEGER {
                tm.tm_mon = tm0.tm_mon;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TZ switch setup -- set_tz / reset_tz / prepare_reset_tz in datetime.c
// ---------------------------------------------------------------------------

/// Temporarily select a timezone in the owning session's ported timezone engine.
/// This guard never changes the host process environment or libc timezone.
struct TzSetup {
    old: Option<String>,
    owner: *mut crate::sexp::instance::RInstance,
}

impl TzSetup {
    /// Port of `prepare_reset_tz()`: snapshot the current TZ.
    fn prepare() -> Self {
        let owner = crate::sexp::instance::with_required_current_instance(|inst| inst);
        TzSetup {
            old: crate::tzone::timezone_override(),
            owner,
        }
    }

    /// Port of `set_tz()`.
    fn set(&self, tz: &str) {
        crate::tzone::set_timezone_override(Some(tz.to_owned()));
    }
}

impl Drop for TzSetup {
    fn drop(&mut self) {
        // The guard is private to a synchronous evaluation and cannot outlive
        // its session. Restore that owner even if a nested session is active;
        // accessing ambient state here could restore the wrong timezone.
        unsafe { (*self.owner).tzone_state.set_override(self.old.take()) };
    }
}

/// Decode a CHARSXP as UTF-8 text, mirroring trunk's multibyte-path
/// validation in `R_strptime()` (mbstowcs failing is an error).
unsafe fn charsxp_text(s: SEXP, what: &str) -> String {
    unsafe {
        let p = CHAR(s);
        let bytes: &[u8] = if p.is_null() {
            &[]
        } else {
            CStr::from_ptr(p).to_bytes()
        };
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => std::panic::panic_any(RError {
                message: format!("invalid multibyte {} string", what),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// do_strptime -- .Internal(strptime(x, format, tz))
// ---------------------------------------------------------------------------

/// Parse a date/time string according to a format, producing a POSIXlt
/// object.
///
/// Emulates R's base closure
/// `strptime <- function(x, format = "", tz = "")`
///   `.Internal(strptime(as.character(x), format, tz))` together with the
/// `.Internal` handler `do_strptime()` in datetime.c (trunk r90447).
pub unsafe fn do_strptime(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            // The closure applies as.character() before calling .Internal
            // (`storage.mode<-(x, "character")` for plain vectors).
            let arglist = Rf_cons(x, R_NilValue());
            let _arglist_guard = protect(arglist);
            x = crate::mainutils::essentials_basic::do_as_character(_call, _op, arglist, _env);
        }
        let _x_guard = protect(x);

        let sformat = CADR(args);
        if TYPEOF(sformat) != SEXPTYPE::STRSXP || XLENGTH(sformat) == 0 {
            std::panic::panic_any(RError {
                message: "invalid 'format' argument".to_string(),
            });
        }

        // GET_tz_n_CHECK: 'tz' must be a length-1 string; the closure
        // supplies the default tz = "" when the argument is missing.
        let mut stz = CADDR(args);
        if stz.is_null() || stz == R_NilValue() || stz == R_MissingArg() {
            stz = Rf_mkString(c"".as_ptr());
        } else if TYPEOF(stz) != SEXPTYPE::STRSXP || XLENGTH(stz) != 1 {
            std::panic::panic_any(RError {
                message: "invalid 'tz' value".to_string(),
            });
        }
        let _stz_guard = protect(stz);

        let mut tz = charsxp_text(STRING_ELT(stz, 0), "tz");
        let mut new_tz = tz.is_empty(); // tz = ""
        if new_tz {
            // Do a direct look up here as this does not otherwise work on
            // Windows.
            match std::env::var_os("TZ") {
                Some(v) => {
                    tz = v.to_string_lossy().into_owned();
                }
                None => new_tz = false,
            }
        }
        // isUTC here means that the timezone has been set to UTC either by
        // default, via TZ="UTC", or via a 'tz' argument. It controls setting
        // TZ, the use of gmtime vs localtime, forcing isdst = 0 and how the
        // "tzone" attribute is set.
        let isUTC = tz == "GMT" || tz == "UTC";
        let tzsi = TzSetup::prepare();
        if !isUTC {
            tzsi.set(&tz);
        }

        let n = XLENGTH(x);
        let m = XLENGTH(sformat);
        let N = if n > 0 { std::cmp::max(m, n) } else { 0 };

        let (ans, ansnames) = make_posixlt_skeleton(N);
        let _ans_guard = protect(ans);
        let _ansnames_guard = protect(ansnames);

        // SET_TZONE: set now in case this gets changed by conversions.
        let tzone = if isUTC {
            Rf_mkString(mk_char_str(&tz).as_ptr())
        } else {
            let tzone = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
            for (j, s) in [tz.clone(), tzname_str(0), tzname_str(1)]
                .iter()
                .enumerate()
            {
                let cs = CString::new(s.as_str()).unwrap_or_default();
                SET_STRING_ELT(tzone, j as R_xlen_t, Rf_mkChar(cs.as_ptr()));
            }
            tzone
        };
        let _tzone_guard = protect(tzone);

        for i in 0..N {
            let iu = i as usize;
            // For glibc's sake. That only sets some unspecified fields,
            // sometimes.
            let mut tm = stm::new();
            tm.tm_sec = 0;
            tm.tm_min = 0;
            tm.tm_hour = 0;
            tm.tm_year = NA_INTEGER;
            tm.tm_mon = NA_INTEGER;
            tm.tm_mday = NA_INTEGER;
            tm.tm_yday = NA_INTEGER;
            tm.tm_wday = NA_INTEGER;
            tm.tm_gmtoff = NA_INTEGER as c_long;
            tm.tm_isdst = -1;
            let mut psecs: c_double = 0.0;
            let mut offset: c_int = NA_INTEGER;

            let xs = STRING_ELT(x, (i % n) as R_xlen_t);
            let mut invalid = xs == R_NaString() || xs.is_null() || xs == R_NilValue();
            if !invalid {
                let input = charsxp_text(xs, "input");
                let fmt = charsxp_text(STRING_ELT(sformat, (i % m) as R_xlen_t), "format");
                invalid = !R_strptime(&input, &fmt, &mut tm, &mut psecs, &mut offset);
            }

            let mut tm2 = tm;
            let mut use_tm2 = false;
            if !invalid {
                // Solaris sets missing fields to 0.
                if tm.tm_mday == 0 {
                    tm.tm_mday = NA_INTEGER;
                }
                if tm.tm_mon == NA_INTEGER || tm.tm_mday == NA_INTEGER || tm.tm_year == NA_INTEGER {
                    glibc_fix(&mut tm, &mut invalid);
                }
                tm.tm_isdst = -1;
                if offset != NA_INTEGER {
                    tm.tm_gmtoff = offset as c_long; // not always correct; better than always NA
                }
                tm2 = tm;
                if offset != NA_INTEGER {
                    // We know the offset, but not the timezone; so all we
                    // can do is to convert to time_t, adjust and convert
                    // back.
                    let t0 = mktime0(&mut tm2, false);
                    if t0 != -1.0 {
                        let tt0 = t0 - offset as c_double;
                        localtime0(&tt0, !isUTC, &mut tm2);
                        use_tm2 = true;
                    } else {
                        invalid = true;
                    }
                } else {
                    // We do want to set wday, yday, isdst, but not to
                    // adjust the structure at DST boundaries.
                    if isUTC {
                        tm.tm_isdst = 0;
                    }
                    // mktime _may_ result in error e.g. during the
                    // spring-forward gap.
                    if mktime0(&mut tm2, !isUTC) != -1.0 {
                        tm.tm_wday = tm2.tm_wday;
                        tm.tm_yday = tm2.tm_yday;
                        tm.tm_zone = tm2.tm_zone;
                        if !isUTC && tm.tm_hour == tm2.tm_hour && tm.tm_min == tm2.tm_min {
                            tm.tm_isdst = tm2.tm_isdst;
                        }
                    }


                }
                invalid = validate_tm(&mut tm) != 0;
            }

            makelt(
                if use_tm2 { &tm2 } else { &tm },
                ans,
                i as R_xlen_t,
                !invalid,
                if invalid {
                    NA_REAL
                } else {
                    psecs - psecs.floor()
                },
            );

            if isUTC {
                let cs = CString::new(tz.as_str()).unwrap_or_default();
                SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(cs.as_ptr()));
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = 0; // gmtoff
            } else {
                let p = if !invalid && tm.tm_isdst >= 0 {
                    let named = tm_zone_string(tm.tm_zone);
                    if named.is_empty() {
                        tzname_str(tm.tm_isdst.clamp(0, 1) as usize)
                    } else {
                        named
                    }
                } else {
                    String::new()
                };
                let cs = CString::new(p).unwrap_or_default();
                SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(cs.as_ptr()));
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = if invalid {
                    NA_INTEGER
                } else {
                    tm.tm_gmtoff as c_int
                };
            }
        } // for(i ..)

        // END_MAKElt
        setAttrib(ans, R_NamesSymbol(), ansnames);
        let klass = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _klass_guard = protect(klass);
        SET_STRING_ELT(klass, 0, Rf_mkChar(c"POSIXlt".as_ptr()));
        SET_STRING_ELT(klass, 1, Rf_mkChar(c"POSIXt".as_ptr()));
        R_classgets(ans, klass);
        if TYPEOF(tzone) == SEXPTYPE::STRSXP {
            setAttrib(ans, Rf_install(c"tzone".as_ptr()), tzone);
        }
        set_posixlt_balanced(ans);


        // The base closure post-processes non-finite inputs: elements of
        // 'x' equal to "Inf" / "-Inf" are replaced by
        // as.POSIXlt.POSIXct(.POSIXct(+-Inf)), i.e. sec = +-Inf with all
        // other components NA and isdst = -1.
        for i in 0..N {
            let xi = STRING_ELT(x, (i % n) as R_xlen_t);
            if xi == R_NaString() || xi.is_null() {
                continue;
            }
            let s = charsxp_text(xi, "input");
            let v = match s.as_str() {
                "Inf" => Some(c_double::INFINITY),
                "-Inf" => Some(c_double::NEG_INFINITY),
                _ => None,
            };
            if let Some(v) = v {
                *REAL(VECTOR_ELT(ans, 0)).add(i as usize) = v;
                for j in 1..8 {
                    *INTEGER(VECTOR_ELT(ans, j)).add(i as usize) = NA_INTEGER;
                }
                *INTEGER(VECTOR_ELT(ans, 8)).add(i as usize) = -1;
                if isUTC {
                    let cs = CString::new(tz.as_str()).unwrap_or_default();
                    SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(cs.as_ptr()));
                    *INTEGER(VECTOR_ELT(ans, 10)).add(i as usize) = 0;
                } else {
                    SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(c"".as_ptr()));
                    *INTEGER(VECTOR_ELT(ans, 10)).add(i as usize) = NA_INTEGER;
                }
            }
        }
        let nm = getAttrib(x, R_NamesSymbol());
        if nm != R_NilValue() && TYPEOF(nm) == SEXPTYPE::STRSXP && XLENGTH(nm) > 0 {
            if N == n {
                setAttrib(VECTOR_ELT(ans, 5), R_NamesSymbol(), nm);
            } else if N > n {
                let nm3 = Rf_allocVector3(SEXPTYPE::STRSXP, N);
                let _nm3_guard = protect(nm3);
                for j in 0..N {
                    SET_STRING_ELT(nm3, j as R_xlen_t, STRING_ELT(nm, (j % n) as R_xlen_t));
                }
                setAttrib(VECTOR_ELT(ans, 5), R_NamesSymbol(), nm3);
            }
        }


        ans
    }
}

/// GNU `as.POSIXlt(x, tz="")`.
pub unsafe fn do_as_POSIXlt(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        if crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0 {
            if let Some(tz) = posixlt_supplied_tz(args) {
                if !tz.is_empty() {
                    let out = crate::mainutils::duplicate::Rf_duplicate(x);
                    let _o = protect(out);
                    setAttrib(
                        out,
                        Rf_install(c"tzone".as_ptr()),
                        Rf_mkString(CString::new(tz).unwrap_or_default().as_ptr()),
                    );
                    return out;
                }
            }
            return x;
        }

        let mut tz = Rf_mkString(c"".as_ptr());
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let t = CAR(rest);
            if TYPEOF(t) == SEXPTYPE::STRSXP && XLENGTH(t) > 0 {
                tz = t;
            }
        }
        let _tz = protect(tz);
        let mut tz_s = if TYPEOF(tz) == SEXPTYPE::STRSXP && XLENGTH(tz) > 0 {
            charsxp_text(STRING_ELT(tz, 0), "tz")
        } else {
            String::new()
        };
        if crate::mainutils::objects::inherits2(x, c"Date".as_ptr()) != 0 {
            let out = do_D2POSIXlt(call, op, Rf_cons(x, R_NilValue()), env);
            let _o = protect(out);
            if let Some(tz) = posixlt_supplied_tz(args) {
                if !tz.is_empty() {
                    setAttrib(
                        out,
                        Rf_install(c"tzone".as_ptr()),
                        Rf_mkString(CString::new(tz).unwrap_or_default().as_ptr()),
                    );
                }
            }
            return out;
        }

        if crate::mainutils::objects::inherits2(x, c"POSIXct".as_ptr()) != 0
            || TYPEOF(x) == SEXPTYPE::REALSXP
            || TYPEOF(x) == SEXPTYPE::INTSXP
            || TYPEOF(x) == SEXPTYPE::LGLSXP
        {

            if tz_s.is_empty() {
                let attr = getAttrib(x, Rf_install(c"tzone".as_ptr()));
                if !attr.is_null()
                    && attr != R_NilValue()
                    && TYPEOF(attr) == SEXPTYPE::STRSXP
                    && XLENGTH(attr) > 0
                {
                    tz_s = charsxp_text(STRING_ELT(attr, 0), "tz");
                }
            }
            return convert_posixct_to_posixlt(x, &tz_s);
        }

        let (text, fmt) = if TYPEOF(x) == SEXPTYPE::STRSXP {
            let ch = if XLENGTH(x) > 0 {
                STRING_ELT(x, 0)
            } else {
                std::ptr::null_mut()
            };
            let sample = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            let fmt = if sample.contains(' ') {
                "%Y-%m-%d %H:%M:%OS"
            } else {
                "%Y-%m-%d"
            };

            (x, fmt)
        } else if crate::mainutils::objects::inherits2(x, c"Date".as_ptr()) != 0 {
            let formatted = crate::mainutils::essentials::do_format_Date(
                call,
                op,
                Rf_cons(x, R_NilValue()),
                env,
            );
            (formatted, "%Y-%m-%d")
        } else {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "do not know how to convert 'x' to class \"POSIXlt\"",
            );
        };
        let _text = protect(text);
        let fmt_s = Rf_mkString(std::ffi::CString::new(fmt).unwrap_or_default().as_ptr());
        let _fmt = protect(fmt_s);
        do_strptime(
            call,
            op,
            Rf_cons(text, Rf_cons(fmt_s, Rf_cons(tz, R_NilValue()))),
            env,
        )

    }
}


/// GNU `format.POSIXlt(x, format, usetz, digits)`.
pub unsafe fn do_format_POSIXlt(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut format = format_posix_named_arg(args, "format", 0);
        let digits_arg = format_posix_named_arg(args, "digits", 2);
        if format.is_null() || format == R_NilValue() || TYPEOF(format) != SEXPTYPE::STRSXP {
            format = Rf_mkString(c"".as_ptr());
        }
        let _f0 = protect(format);
        let nf = XLENGTH(format);
        let mut filled = false;
        for i in 0..nf {
            let ch = STRING_ELT(format, i);
            let empty = ch.is_null()
                || ch == R_NaString()
                || CStr::from_ptr(CHAR(ch)).to_bytes().is_empty();
            if empty {
                filled = true;
                break;
            }
        }
        let opt_digits = {
            let opt = crate::mainutils::options::GetOption1(Rf_install(c"digits.secs".as_ptr()));
            if opt.is_null() || opt == R_NilValue() {
                0
            } else {
                crate::mainutils::coerce::asInteger(opt)
            }
        };
        let explicit_digits = !digits_arg.is_null() && digits_arg != R_NilValue();
        let mut digits = if explicit_digits {
            crate::mainutils::coerce::asInteger(digits_arg)
        } else {
            opt_digits
        };
        if explicit_digits && digits != opt_digits && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 1
        {
            let mut bare_os = false;
            for i in 0..nf {
                let ch = STRING_ELT(format, i);
                if ch.is_null() || ch == R_NaString() {
                    continue;
                }
                let s = CStr::from_ptr(CHAR(ch)).to_string_lossy();
                if let Some(pos) = s.find("%OS") {
                    match s.as_bytes().get(pos + 3) {
                        None => bare_os = true,
                        Some(b) if !b.is_ascii_digit() => bare_os = true,
                        _ => {}
                    }
                }
            }
            if bare_os {
                let sec = VECTOR_ELT(x, 0);
                let mut secs = Vec::new();
                if TYPEOF(sec) == SEXPTYPE::REALSXP {
                    for i in 0..XLENGTH(sec) {
                        secs.push(*REAL(sec).add(i as usize));
                    }
                }
                digits = use_dig_secs(&secs, digits);
            }
        }

        if filled && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 3 {
            let nsec = XLENGTH(VECTOR_ELT(x, 0)).max(0);
            let mut secs = Vec::with_capacity(nsec as usize);
            if TYPEOF(VECTOR_ELT(x, 0)) == SEXPTYPE::REALSXP {
                for i in 0..nsec {
                    secs.push(*REAL(VECTOR_ELT(x, 0)).add(i as usize));
                }
            }
            let np = use_dig_secs(&secs, digits);
            let times_zero = {
                let mut z = true;
                for comp in 0..3 {
                    let v = VECTOR_ELT(x, comp);
                    let n = XLENGTH(v);
                    for i in 0..n {
                        let val = if TYPEOF(v) == SEXPTYPE::REALSXP {
                            *REAL(v).add(i as usize)
                        } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                            let iv = *INTEGER(v).add(i as usize);
                            if iv == NA_INTEGER {
                                continue;
                            }
                            iv as f64
                        } else {
                            0.0
                        };
                        if val.is_finite() && val != 0.0 {
                            z = false;
                        }
                    }
                }
                z
            };
            let repl = if times_zero {
                "%Y-%m-%d"
            } else if np == 0 {
                "%Y-%m-%d %H:%M:%S"
            } else {
                // filled below per-element
                ""
            };
            let format2 = Rf_allocVector3(SEXPTYPE::STRSXP, nf);
            let _f2 = protect(format2);
            for i in 0..nf {
                let ch = STRING_ELT(format, i);
                let empty = ch.is_null()
                    || ch == R_NaString()
                    || CStr::from_ptr(CHAR(ch)).to_bytes().is_empty();
                if empty {
                    let s = if times_zero {
                        "%Y-%m-%d".to_string()
                    } else if np == 0 {
                        "%Y-%m-%d %H:%M:%S".to_string()
                    } else {
                        format!("%Y-%m-%d %H:%M:%OS{np}")
                    };
                    let cs = CString::new(s).unwrap_or_default();
                    SET_STRING_ELT(format2, i, Rf_mkChar(cs.as_ptr()));
                } else {
                    SET_STRING_ELT(format2, i, ch);
                }
            }
            format = format2;
            let _ = repl;
        }
        let usetz = Rf_ScalarLogical(0);
        let _u = protect(usetz);
        let digs = Rf_ScalarInteger(digits);
        let _d = protect(digs);
        let out = do_formatPOSIXlt(
            call,
            op,
            Rf_cons(x, Rf_cons(format, Rf_cons(usetz, Rf_cons(digs, R_NilValue())))),
            env,
        );
        let _out = protect(out);
        // Observation names live on POSIXlt components (year), never on
        // the list itself — list names are sec/min/hour/...
        let mut names = R_NilValue();
        if TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 6 {
            names = getAttrib(VECTOR_ELT(x, 5), R_NamesSymbol());
        } else {
            names = getAttrib(x, R_NamesSymbol());
        }
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == XLENGTH(out)
        {
            setAttrib(out, R_NamesSymbol(), names);
        }

        out
    }
}

/// GNU `format.POSIXct(x, format, tz, usetz, digits)`.
pub unsafe fn do_format_POSIXct(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let format = format_posix_named_arg(args, "format", 0);
        let tz = format_posix_named_arg(args, "tz", 1);
        let digits = format_posix_named_arg(args, "digits", 3);
        let mut tz_s = tz;
        if tz_s.is_null() || tz_s == R_NilValue() {
            let attr = getAttrib(x, Rf_install(c"tzone".as_ptr()));
            if !attr.is_null() && attr != R_NilValue() {
                tz_s = attr;
            }
        }
        let lt_args = if tz_s.is_null() || tz_s == R_NilValue() {
            Rf_cons(x, R_NilValue())
        } else {
            Rf_cons(x, Rf_cons(tz_s, R_NilValue()))
        };
        let _la = protect(lt_args);
        let lt = do_as_POSIXlt(call, op, lt_args, env);
        let _lt = protect(lt);
        let mut rest = R_NilValue();
        if !digits.is_null() && digits != R_NilValue() {
            let cell = Rf_cons(digits, rest);
            SETTAG(cell, Rf_install(c"digits".as_ptr()));
            rest = cell;
        }
        if !format.is_null() && format != R_NilValue() {
            let cell = Rf_cons(format, rest);
            SETTAG(cell, Rf_install(c"format".as_ptr()));
            rest = cell;
        }
        let call_args = Rf_cons(lt, rest);
        let _ca = protect(call_args);
        let out = do_format_POSIXlt(call, op, call_args, env);
        let _o = protect(out);
        let names = getAttrib(x, R_NamesSymbol());
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == XLENGTH(out)
        {
            setAttrib(out, R_NamesSymbol(), names);
        }
        out

    }
}


unsafe fn posixlt_as_date(call: SEXP, op: SEXP, x: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0 {
            crate::mainutils::essentials::do_as_Date(call, op, Rf_cons(x, R_NilValue()), env)
        } else {
            x
        }
    }
}

/// GNU `weekdays.POSIXt(x)`.
pub unsafe fn do_weekdays_POSIXt(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let x = posixlt_as_date(call, op, CAR(args), env);
        crate::mainutils::essentials::do_weekdays(call, op, Rf_cons(x, CDR(args)), env)
    }
}

/// GNU `months.POSIXt(x)`.
pub unsafe fn do_months_POSIXt(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let x = posixlt_as_date(call, op, CAR(args), env);
        crate::mainutils::essentials::do_months(call, op, Rf_cons(x, CDR(args)), env)
    }
}

/// GNU `quarters.POSIXt(x)`.
pub unsafe fn do_quarters_POSIXt(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let x = posixlt_as_date(call, op, CAR(args), env);
        crate::mainutils::essentials::do_quarters(call, op, Rf_cons(x, CDR(args)), env)
    }
}

/// GNU `as.character.POSIXt(x, digits, OutDec)`.
pub unsafe fn do_as_character_POSIXt(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let matched = match_named_then_positional(args, &["x", "digits", "OutDec"]);
        let x = matched[0];
        let digits_arg = matched[1];
        let outdec_arg = matched[2];
        let is_lt = crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0;
        let digits = if !digits_arg.is_null()
            && digits_arg != R_NilValue()
            && digits_arg != R_MissingArg()
            && (TYPEOF(digits_arg) == SEXPTYPE::INTSXP || TYPEOF(digits_arg) == SEXPTYPE::REALSXP)
        {
            crate::mainutils::coerce::asInteger(digits_arg)
        } else if is_lt {
            14
        } else {
            6
        };
        let outdec = if !outdec_arg.is_null()
            && outdec_arg != R_NilValue()
            && TYPEOF(outdec_arg) == SEXPTYPE::STRSXP
            && XLENGTH(outdec_arg) > 0
        {
            charsxp_text(STRING_ELT(outdec_arg, 0), "OutDec")
        } else {
            ".".to_string()
        };


        let lt = if is_lt {

            x
        } else {
            do_as_POSIXlt(call, op, Rf_cons(x, R_NilValue()), env)
        };
        let _lt0 = protect(lt);
        let lt = do_balancePOSIXlt(call, op, Rf_cons(lt, R_NilValue()), env);
        let _lt = protect(lt);
        if TYPEOF(lt) != SEXPTYPE::VECSXP || XLENGTH(lt) < 6 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let n = XLENGTH(VECTOR_ELT(lt, 0)).max(0);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        // GNU as.character.POSIXt: options(scipen = max(digits)+1) so
        // as.character(sec) does not go scientific for tiny fractions.
        let scipen_sym = Rf_install(c"scipen".as_ptr());
        let old_scipen = crate::mainutils::options::GetOption1(scipen_sym);
        let _old_s = protect(old_scipen);
        let want = digits.max(0) + 1;
        let cur = if !old_scipen.is_null() && old_scipen != R_NilValue() {
            crate::mainutils::coerce::asInteger(old_scipen)
        } else {
            0
        };
        let _scipen = ScipenGuard {
            old: old_scipen,
            active: cur <= digits,
        };
        if cur <= digits {
            let nv = Rf_ScalarInteger(want);
            let _nv = protect(nv);
            crate::mainutils::options::SetOptionByName("scipen", nv);
        }

        let sec = VECTOR_ELT(lt, 0);

        for i in 0..n {
            let iu = i as usize;
            let s = if TYPEOF(sec) == SEXPTYPE::REALSXP {
                *REAL(sec).add(iu)
            } else {
                NA_REAL
            };
            let hour = posixlt_int_elt(VECTOR_ELT(lt, 2), iu);
            let minu = posixlt_int_elt(VECTOR_ELT(lt, 1), iu);
            let mday = posixlt_int_elt(VECTOR_ELT(lt, 3), iu);
            let mon = posixlt_int_elt(VECTOR_ELT(lt, 4), iu);
            let year = posixlt_int_elt(VECTOR_ELT(lt, 5), iu);
            let time = f64::from(hour) + f64::from(minu) + s;
            let is_na = s.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN;
            let ok = time.is_finite()
                && hour != NA_INTEGER
                && minu != NA_INTEGER
                && mday != NA_INTEGER
                && mon != NA_INTEGER
                && year != NA_INTEGER;
            let text = if !ok && is_na {
                None
            } else if !ok {
                Some(as_character_real(call, op, env, s, &outdec))
            } else {
                let date = format!(
                    "{}-{:02}-{:02}",
                    1900 + year,
                    mon + 1,
                    mday
                );
                if time == 0.0 {
                    Some(date)
                } else {
                    let rounded = {
                        let sx = Rf_ScalarReal(s);
                        let _sx = protect(sx);
                        let dg = Rf_ScalarInteger(digits);
                        let _dg = protect(dg);
                        let rnd = crate::mainutils::essentials::do_round(
                            call,
                            op,
                            Rf_cons(sx, Rf_cons(dg, R_NilValue())),
                            env,
                        );
                        let _r = protect(rnd);
                        if TYPEOF(rnd) == SEXPTYPE::REALSXP && XLENGTH(rnd) > 0 {
                            *REAL(rnd)
                        } else {
                            s
                        }
                    };

                    let mut sch = as_character_real(call, op, env, rounded, &outdec);
                    if rounded < 10.0 && rounded >= 0.0 {
                        sch.insert(0, '0');
                    }
                    Some(format!("{date} {hour:02}:{minu:02}:{sch}"))
                }
            };
            match text {
                None => SET_STRING_ELT(out, i, R_NaString()),
                Some(s) => {
                    let cs = CString::new(s).unwrap_or_default();
                    SET_STRING_ELT(out, i, Rf_mkChar(cs.as_ptr()));
                }
            }
        }
        out

    }
}

struct ScipenGuard {
    old: SEXP,
    active: bool,
}

impl Drop for ScipenGuard {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                crate::mainutils::options::SetOptionByName("scipen", self.old);
            }
        }
    }
}


fn r_round_digits(x: f64, digits: i32) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let digits = digits.clamp(0, 18);
    let p = 10f64.powi(digits);
    (x * p).round() / p
}

unsafe fn as_character_real(call: SEXP, op: SEXP, env: SEXP, value: f64, outdec: &str) -> String {
    unsafe {
        let scalar = Rf_ScalarReal(value);
        let _s = protect(scalar);
        let ch = crate::mainutils::essentials::do_as_character(
            call,
            op,
            Rf_cons(scalar, R_NilValue()),
            env,
        );
        let _c = protect(ch);
        let mut text = if TYPEOF(ch) == SEXPTYPE::STRSXP && XLENGTH(ch) > 0 {
            charsxp_text(STRING_ELT(ch, 0), "as.character")
        } else {
            String::new()
        };
        if outdec != "." {
            text = text.replace('.', outdec);
        }
        text
    }
}

/// GNU `as.double.POSIXlt <- function(x, ...) as.double(as.POSIXct(x))`.
pub unsafe fn do_as_double_POSIXt(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let ct = if crate::mainutils::objects::inherits2(x, c"POSIXct".as_ptr()) != 0
            && TYPEOF(x) == SEXPTYPE::REALSXP
        {
            x
        } else {
            crate::mainutils::essentials::do_as_POSIXct(
                call,
                op,
                Rf_cons(x, R_NilValue()),
                env,
            )
        };
        let _ct = protect(ct);
        let n = XLENGTH(ct);
        let out = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _o = protect(out);
        if TYPEOF(ct) == SEXPTYPE::REALSXP {
            for i in 0..n {
                *REAL(out).add(i as usize) = *REAL(ct).add(i as usize);
            }
        }
        let names = getAttrib(ct, R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            setAttrib(out, R_NamesSymbol(), names);
        }
        out
    }
}






/// Build a CString from an owned string (helper for the code above).
fn mk_char_str(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

/// Read `tzname[idx]` as an owned string.
fn tzname_str(idx: usize) -> String {
    unsafe {
        let p = *R_tzname().add(idx);
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

// ---------------------------------------------------------------------------
// do_D2POSIXlt -- .Internal(Date2POSIXlt(x))
// ---------------------------------------------------------------------------

/// Convert a Date (numeric days since epoch) to a POSIXlt list in UTC.
///
/// Ported from `do_D2POSIXlt()` in datetime.c.
pub unsafe fn do_D2POSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _x_guard = protect(x);
        if TYPEOF(x) != SEXPTYPE::REALSXP && TYPEOF(x) != SEXPTYPE::INTSXP {
            std::panic::panic_any(RError {
                message: "invalid 'x' value: not numeric".to_string(),
            });
        }

        let n = XLENGTH(x);
        let (ans, ansnames) = make_posixlt_skeleton(n);
        let _ans_guard = protect(ans);
        let _ansnames_guard = protect(ansnames);

        for i in 0..n {
            let iu = i as usize;
            let x_i = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(iu)
            } else {
                let v = *INTEGER(x).add(iu);
                if v == NA_INTEGER {
                    NA_REAL
                } else {
                    v as f64
                }
            };
            let mut tm = stm::new();
            let valid = julian2dtime(x_i, &mut tm);

            makelt(
                &tm,
                ans,
                i as R_xlen_t,
                valid,
                if valid { 0.0 } else { x_i },
            );

            // zone = "UTC", gmtoff = 0
            let utc_cstr = c"UTC";
            SET_STRING_ELT(
                VECTOR_ELT(ans, 9),
                i as R_xlen_t,
                Rf_mkChar(utc_cstr.as_ptr()),
            );
            *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = 0;
        }
        let tzone = Rf_mkString(c"UTC".as_ptr());
        let _tz = protect(tzone);
        finish_posixlt(ans, ansnames, tzone);
        let names = getAttrib(x, R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            for j in 0..XLENGTH(ans) {
                let elt = VECTOR_ELT(ans, j);
                if !elt.is_null() && elt != R_NilValue() {
                    setAttrib(elt, R_NamesSymbol(), names);
                }
            }
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// do_POSIXlt2D -- .Internal(POSIXlt2Date(x))
// ---------------------------------------------------------------------------

/// Convert a POSIXlt list to a Date (numeric days since epoch).
///
/// Ported from `do_POSIXlt2D()` in datetime.c.
#[allow(clippy::if_same_then_else)]
pub unsafe fn do_POSIXlt2D(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _x_guard = protect(x);
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "a valid \"POSIXlt\" object is a list of at least 9 elements".to_string(),
            });
        }

        let mut n: R_xlen_t = 0;
        let mut nlen = [0i64; 9];
        for i in 0..6 {
            let len = XLENGTH(VECTOR_ELT(x, i as R_xlen_t));
            nlen[i as usize] = len;
            if len > n {
                n = len;
            }
        }
        let len8 = XLENGTH(VECTOR_ELT(x, 8));
        nlen[8] = len8;
        if len8 > n {
            n = len8;
        }

        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _ans_guard = protect(ans);

        for i in 0..n {
            let iu = i as usize;
            let secs = posixlt_real_elt(VECTOR_ELT(x, 0), iu);


            let fsecs = secs.floor();

            let mut tm = stm::new();
            tm.tm_sec = if R_FINITE(secs) {
                fsecs as c_int
            } else {
                NA_INTEGER
            };
            tm.tm_min = posixlt_int_elt(VECTOR_ELT(x, 1), iu);
            tm.tm_hour = posixlt_int_elt(VECTOR_ELT(x, 2), iu);
            tm.tm_mday = posixlt_int_elt(VECTOR_ELT(x, 3), iu);
            tm.tm_mon = posixlt_int_elt(VECTOR_ELT(x, 4), iu);
            tm.tm_year = posixlt_int_elt(VECTOR_ELT(x, 5), iu);

            tm.tm_isdst = 0; // always UTC for Date conversion

            if !R_FINITE(secs) {
                *REAL(ans).add(iu) = secs;
            } else if tm.tm_min == NA_INTEGER
                || tm.tm_hour == NA_INTEGER
                || tm.tm_mday == NA_INTEGER
                || tm.tm_mon == NA_INTEGER
                || tm.tm_year == NA_INTEGER
            {
                *REAL(ans).add(iu) = NA_REAL;
            } else if validate_tm(&mut tm) < 0 {
                *REAL(ans).add(iu) = NA_REAL;
            } else {
                *REAL(ans).add(iu) = mkdate00(&mut tm);
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_balancePOSIXlt -- .Internal(balancePOSIXlt(x, fill.only, classed))
// ---------------------------------------------------------------------------

/// Balance (validate and normalize) a POSIXlt object.
///
/// Ported from `do_balancePOSIXlt()` in datetime.c.
pub unsafe fn do_balancePOSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "a valid \"POSIXlt\" object is a list of at least 9 elements".to_string(),
            });
        }

        let n_comp = LENGTH(x);
        if n_comp < 9 {
            std::panic::panic_any(RError {
                message: "a valid \"POSIXlt\" object is a list of at least 9 elements".to_string(),
            });
        }

        let nn = std::cmp::min(n_comp, 11);
        let mut nlen = [0i64; 11];
        let mut n: R_xlen_t = 0;
        for i in 0..nn {
            let len = XLENGTH(VECTOR_ELT(x, i as R_xlen_t));
            nlen[i as usize] = len;
            if len > n {
                n = len;
            }
        }

        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, nn as R_xlen_t);
        let _ans_guard = protect(ans);
        for i in 0..9 {
            let sexp: c_int = if i > 0 {
                SEXPTYPE::INTSXP.into()
            } else {
                SEXPTYPE::REALSXP.into()
            };
            SET_VECTOR_ELT(ans, i as R_xlen_t, Rf_allocVector3(sexp, n));
        }
        if nn >= 10 {
            SET_VECTOR_ELT(ans, 9, Rf_allocVector3(SEXPTYPE::STRSXP, n));
        }
        if nn >= 11 {
            SET_VECTOR_ELT(ans, 10, Rf_allocVector3(SEXPTYPE::INTSXP, n));
        }

        let ansnames = Rf_allocVector3(SEXPTYPE::STRSXP, nn as R_xlen_t);
        let _ansnames_guard = protect(ansnames);
        for i in 0..nn {
            let cstr = CString::new(ltnames[i as usize]).unwrap_or_default();
            SET_STRING_ELT(ansnames, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        for i in 0..n {
            let iu = i as usize;
            let idx = |comp: usize| -> usize {
                let len = nlen[comp] as usize;
                if len == 0 { 0 } else { iu % len }
            };
            let sec_col = VECTOR_ELT(x, 0);
            let secs = if TYPEOF(sec_col) == SEXPTYPE::REALSXP && XLENGTH(sec_col) > 0 {
                *REAL(sec_col).add(idx(0))
            } else if TYPEOF(sec_col) == SEXPTYPE::INTSXP && XLENGTH(sec_col) > 0 {
                let v = *INTEGER(sec_col).add(idx(0));
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            let fsecs = secs.floor();

            let mut tm = stm::new();
            tm.tm_sec = if R_FINITE(secs) {
                fsecs as c_int
            } else {
                NA_INTEGER
            };
            tm.tm_min = posixlt_int_elt(VECTOR_ELT(x, 1), idx(1));
            tm.tm_hour = posixlt_int_elt(VECTOR_ELT(x, 2), idx(2));
            tm.tm_mday = posixlt_int_elt(VECTOR_ELT(x, 3), idx(3));
            tm.tm_mon = posixlt_int_elt(VECTOR_ELT(x, 4), idx(4));
            tm.tm_year = posixlt_int_elt(VECTOR_ELT(x, 5), idx(5));
            tm.tm_wday = posixlt_int_elt(VECTOR_ELT(x, 6), idx(6));
            tm.tm_yday = posixlt_int_elt(VECTOR_ELT(x, 7), idx(7));
            tm.tm_isdst = posixlt_int_elt(VECTOR_ELT(x, 8), idx(8));

            let valid = R_FINITE(secs)
                && tm.tm_min != NA_INTEGER
                && tm.tm_hour != NA_INTEGER
                && tm.tm_mday != NA_INTEGER
                && tm.tm_mon != NA_INTEGER
                && tm.tm_year != NA_INTEGER;

            if valid {
                validate_tm(&mut tm);
                mkdate00(&mut tm);
            }

            makelt(
                &tm,
                ans,
                i as R_xlen_t,
                valid,
                if valid {
                    secs - fsecs
                } else if R_FINITE(secs) {
                    NA_REAL
                } else {
                    secs
                },
            );

            if nn >= 10 {
                let zone_cstr = c"";
                SET_STRING_ELT(
                    VECTOR_ELT(ans, 9),
                    i as R_xlen_t,
                    Rf_mkChar(zone_cstr.as_ptr()),
                );
            }
            if nn >= 11 {
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = if valid {
                    tm.tm_gmtoff as c_int
                } else {
                    NA_INTEGER
                };
            }
        }

        setAttrib(ans, R_NamesSymbol(), ansnames);
        let klass = getAttrib(x, R_ClassSymbol());
        if !klass.is_null() && klass != R_NilValue() {
            setAttrib(ans, R_ClassSymbol(), klass);
        }
        let tzone = getAttrib(x, Rf_install(c"tzone".as_ptr()));
        if !tzone.is_null() && tzone != R_NilValue() {
            setAttrib(ans, Rf_install(c"tzone".as_ptr()), tzone);
        }
        setAttrib(
            ans,
            Rf_install(c"balanced".as_ptr()),
            Rf_ScalarLogical(TRUE),
        );
        ans

    }
}

// ---------------------------------------------------------------------------
// do_Sys_time -- Sys.time()
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// do_difftime -- difftime(time1, time2, units)
// ---------------------------------------------------------------------------

/// Compute the difference between two POSIXct times.
///
/// Returns a difftime object (numeric with "units" attribute).
/// Ported from the R difftime() logic.
pub unsafe fn do_difftime(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let time1 = CADR(args); // second arg in pairlist
        let time2 = CADDR(args); // third arg
        let units = CADDDR(args); // fourth arg

        // Get numeric values
        let d1 = if TYPEOF(time1) == SEXPTYPE::REALSXP {
            *REAL(time1)
        } else if TYPEOF(time1) == SEXPTYPE::INTSXP {
            *INTEGER(time1) as c_double
        } else {
            NA_REAL
        };

        let d2 = if TYPEOF(time2) == SEXPTYPE::REALSXP {
            *REAL(time2)
        } else if TYPEOF(time2) == SEXPTYPE::INTSXP {
            *INTEGER(time2) as c_double
        } else {
            NA_REAL
        };

        let mut diff = d1 - d2;

        // Get units string
        let units_str =
            if !units.is_null() && TYPEOF(units) == SEXPTYPE::STRSXP && XLENGTH(units) > 0 {
                let charsxp = STRING_ELT(units, 0);
                if !charsxp.is_null() {
                    let ptr = CHAR(charsxp);
                    if !ptr.is_null() {
                        CStr::from_ptr(ptr).to_str().unwrap_or("secs")
                    } else {
                        "secs"
                    }
                } else {
                    "secs"
                }
            } else {
                "secs"
            };

        // Apply unit conversion (R difftime returns difference in the requested unit)
        match units_str {
            "secs" => { /* no conversion */ }
            "mins" => diff /= 60.0,
            "hours" => diff /= 3600.0,
            "days" => diff /= 86400.0,
            "weeks" => diff /= 86400.0 * 7.0,
            _ => { /* unknown unit, return as-is */ }
        }

        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        *REAL(ans) = diff;
        ans
    }
}

// ---------------------------------------------------------------------------
// do_ISOdatetime -- ISOdatetime(year, month, day, hour, min, sec, tz)
// ---------------------------------------------------------------------------

/// Construct a POSIXct from date/time components.
///
/// Ported from `ISOdatetime()` in datetime.c.
pub unsafe fn do_ISOdatetime(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let year = CAR(args);
        let month = CADR(args);
        let day = CADDR(args);
        let hour = CADDDR(args);
        let min_arg = CAD5R(args); // 5th element
        let sec_arg = CAR(CDR(CDR(CDR(CDR(CDR(args)))))); // 6th element

        // Get vector lengths and find max
        let ny = XLENGTH(year);
        let nmo = XLENGTH(month);
        let nd = XLENGTH(day);
        let nh = XLENGTH(hour);
        let nmi = XLENGTH(min_arg);
        let ns = XLENGTH(sec_arg);
        let mut n: R_xlen_t = 1;
        for &len in &[ny, nmo, nd, nh, nmi, ns] {
            if len > n {
                n = len;
            }
        }

        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);

        for i in 0..n {
            let iu = i as usize;

            let yr = *INTEGER(year).add(iu % ny as usize);
            let mo = *INTEGER(month).add(iu % nmo as usize);
            let dy = *INTEGER(day).add(iu % nd as usize);
            let hr = *INTEGER(hour).add(iu % nh as usize);
            let mi = *INTEGER(min_arg).add(iu % nmi as usize);
            let sc = *REAL(sec_arg).add(iu % ns as usize);

            if yr == NA_INTEGER
                || mo == NA_INTEGER
                || dy == NA_INTEGER
                || hr == NA_INTEGER
                || mi == NA_INTEGER
                || !R_FINITE(sc)
            {
                *REAL(ans).add(iu) = NA_REAL;
            } else {
                let mut ctm: tz_tm = std::mem::zeroed();
                ctm.tm_year = yr - 1900;
                ctm.tm_mon = mo - 1;
                ctm.tm_mday = dy;
                ctm.tm_hour = hr;
                ctm.tm_min = mi;
                ctm.tm_sec = sc as c_int;
                ctm.tm_isdst = -1;

                let t = unsafe { R_mktime(&mut ctm) };
                if t == -1 {
                    *REAL(ans).add(iu) = NA_REAL;
                } else {
                    *REAL(ans).add(iu) = t as c_double + (sc - sc.floor());
                }
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// R_isLeapYear -- FFI-compatible leap year check (absolute year)
// ---------------------------------------------------------------------------

/// Check whether a year (absolute, e.g. 2000) is a leap year.
/// This is the FFI-compatible version using absolute years.
pub unsafe fn R_isLeapYear(year: c_int) -> c_int {
    if isleap(year) { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// FFI-compatible standalone functions
// ---------------------------------------------------------------------------

/// FFI-compatible leap year test.
pub unsafe fn R_isleap(year: c_int) -> c_int {
    if isleap(year) { 1 } else { 0 }
}

/// FFI-compatible days-in-year function.
pub unsafe fn R_days_in_year(year: c_int) -> c_int {
    days_in_year(year)
}

/// FFI-compatible days-in-month function.
///
/// `mon` is 0-based (0=Jan), `yr` is years since 1900.
pub unsafe fn R_days_in_month(mon: c_int, yr: c_int) -> c_int {
    days_in_month(mon, yr)
}

/// FFI-compatible validate_tm.
pub unsafe fn R_validate_tm(tm: *mut stm) -> c_int {
    unsafe {
        if tm.is_null() {
            return -1;
        }
        validate_tm(&mut *tm)
    }
}

/// FFI-compatible mktime-like function (UTC only, no timezone correction).
///
/// Returns seconds since epoch as a double, or NA_REAL on error.
pub unsafe fn R_timegm00_ffi(tm: *mut stm) -> c_double {
    unsafe {
        if tm.is_null() {
            return NA_REAL;
        }
        timegm00(&mut *tm)
    }
}

/// FFI-compatible mkdate00.
pub unsafe fn R_mkdate00(tm: *mut stm) -> c_double {
    unsafe {
        if tm.is_null() {
            return NA_REAL;
        }
        mkdate00(&mut *tm)
    }
}

/// FFI-compatible likely_strftime_overflow.
pub unsafe fn R_likely_strftime_overflow(tm: *const stm) -> c_int {
    unsafe {
        if tm.is_null() {
            return 0;
        }
        if likely_strftime_overflow(&*tm) { 1 } else { 0 }
    }
}

/// FFI-compatible julian2dtime.
pub unsafe fn R_julian2dtime(x_i: c_double, tm: *mut stm) -> c_int {
    unsafe {
        if tm.is_null() {
            return 0;
        }
        if julian2dtime(x_i, &mut *tm) { 1 } else { 0 }
    }
}

/// FFI-compatible dtime2julian.
pub unsafe fn R_dtime2julian(
    secs: c_double,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
) -> c_double {
    dtime2julian(secs, tm_min, tm_hour, tm_mday, tm_mon, tm_year)
}

/// FFI-compatible R_ISLeapYear.
pub unsafe fn R_R_ISLeapYear(year: c_int) -> c_int {
    if R_ISLeapYear(year) { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
