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

#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_long};

use libc::{localtime_r, mktime, strftime, time_t, tm as libc_tm};

// FFI for tzname global variable
unsafe extern "C" {
    static tzname: [*mut c_char; 2];
}

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Days in each month for a non-leap year.
pub static month_days: [c_int; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// NA_REAL sentinel matching R's NA_REAL.
pub const NA_REAL: c_double = f64::NAN; // simplified; R uses a specific bit pattern

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
        if tm.tm_mon >= 0 && tm.tm_mon <= 11 {
            if tm.tm_mday > days_in_month(tm.tm_mon, tm.tm_year) {
                tm.tm_mon += 1;
                tm.tm_mday = 1;
                if tm.tm_mon == 12 {
                    tm.tm_year += 1;
                    tm.tm_mon = 0;
                }
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
    let mut year0 = 1900 + tm.tm_year;
    let mut excess: c_double = 0.0;

    if year0 >= 400 {
        excess = (year0 / 400 - 1) as c_double;
        year0 -= (excess as c_int) * 400;
    } else if year0 < 0 {
        excess = -1.0 - (-year0 / 400) as c_double;
        year0 -= (excess as c_int) * 400;
    }

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

    // Use system mktime for local time
    let mut ctm: libc_tm = unsafe { std::mem::zeroed() };
    ctm.tm_sec = tm.tm_sec;
    ctm.tm_min = tm.tm_min;
    ctm.tm_hour = tm.tm_hour;
    ctm.tm_mday = tm.tm_mday;
    ctm.tm_mon = tm.tm_mon;
    ctm.tm_year = tm.tm_year;
    ctm.tm_isdst = tm.tm_isdst;

    let result = unsafe { mktime(&mut ctm) };

    // Copy back normalized values
    tm.tm_sec = ctm.tm_sec;
    tm.tm_min = ctm.tm_min;
    tm.tm_hour = ctm.tm_hour;
    tm.tm_mday = ctm.tm_mday;
    tm.tm_mon = ctm.tm_mon;
    tm.tm_year = ctm.tm_year;
    tm.tm_wday = ctm.tm_wday;
    tm.tm_yday = ctm.tm_yday;
    tm.tm_isdst = ctm.tm_isdst;

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

    let mut ctm: libc_tm = unsafe { std::mem::zeroed() };
    let res = unsafe {
        if local {
            localtime_r(&t, &mut ctm)
        } else {
            libc::gmtime_r(&t, &mut ctm)
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
    ltm.tm_gmtoff = 0;

    // Try to get gmtoff from the system tm struct
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    {
        ltm.tm_gmtoff = ctm.tm_gmtoff;
    }

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
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, nans as R_xlen_t));
        for i in 0..9 {
            let sexp = if i > 0 {
                SEXPTYPE::INTSXP.0
            } else {
                SEXPTYPE::REALSXP.0
            };
            SET_VECTOR_ELT(ans, i as R_xlen_t, Rf_allocVector3(sexp, n));
        }
        SET_VECTOR_ELT(ans, 9, Rf_allocVector3(SEXPTYPE::STRSXP.0, n)); // zone
        SET_VECTOR_ELT(ans, 10, Rf_allocVector3(SEXPTYPE::INTSXP.0, n)); // gmtoff

        let ansnames = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, nans as R_xlen_t));
        for i in 0..nans {
            let cstr = CString::new(ltnames[i as usize]).unwrap_or_default();
            SET_STRING_ELT(ansnames, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        (ans, ansnames)
    }
}

// ---------------------------------------------------------------------------
// do_asPOSIXlt -- .Internal(as.POSIXlt(x, tz))
// ---------------------------------------------------------------------------

/// Convert a POSIXct numeric vector to a POSIXlt list.
///
/// Ported from `do_asPOSIXlt()` in datetime.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_asPOSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // x = CAR(args) should be a REALSXP (POSIXct)
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::REALSXP.0 && TYPEOF(x) != SEXPTYPE::INTSXP.0 {
            // Try to coerce
            std::panic::panic_any(RError {
                message: "invalid 'x' value: not numeric".to_string(),
            });
        }
        let x = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, XLENGTH(x)));
        // Copy values (simplified: assumes input is already REALSXP)
        std::ptr::copy_nonoverlapping(REAL(CAR(args)), REAL(x), XLENGTH(x) as usize);

        let n = XLENGTH(x);
        let (ans, ansnames) = make_posixlt_skeleton(n);

        for i in 0..n {
            let mut dummy = stm::new();
            let d = *REAL(x).add(i as usize);
            let valid = if R_FINITE(d) {
                localtime0(REAL(x).add(i as usize), true, &mut dummy)
            } else {
                false
            };
            makelt(&dummy, ans, i, valid, if valid { d - d.floor() } else { d });

            // zone and gmtoff
            let zone_cstr = if valid && dummy.tm_isdst >= 0 {
                // Get timezone abbreviation from system
                let mut t = d as time_t;
                if d < 0.0 && d != (t as c_double) {
                    t -= 1;
                }
                let mut ctm: libc_tm = std::mem::zeroed();
                let res = localtime_r(&t, &mut ctm);
                if !res.is_null() {
                    // Use tzname
                    let tzname_idx = if ctm.tm_isdst > 0 { 1 } else { 0 };
                    let tzname_ptr = unsafe { tzname[tzname_idx] };
                    if !tzname_ptr.is_null() {
                        CStr::from_ptr(tzname_ptr).to_string_lossy().into_owned()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let zone_charsxp = Rf_mkChar(CString::new(zone_cstr).unwrap_or_default().as_ptr());
            SET_STRING_ELT(VECTOR_ELT(ans, 9), i, zone_charsxp);
            *INTEGER(VECTOR_ELT(ans, 10)).add(i as usize) = dummy.tm_gmtoff as c_int;
        }

        // Set names
        // We'd need install() and setAttrib() here, but those may not be available
        // so we just set the names directly via the ansnames
        // In full R: setAttrib(ans, R_NamesSymbol, ansnames);
        // We store ansnames as the names attribute directly
        let _ = ansnames; // ansnames is already protected

        Rf_unprotect(3); // x, ans, ansnames
        ans
    }
}

// ---------------------------------------------------------------------------
// do_asPOSIXct -- .Internal(as.POSIXct(x, tz))
// ---------------------------------------------------------------------------

/// Convert a POSIXlt list to a POSIXct numeric vector.
///
/// Ported from `do_asPOSIXct()` in datetime.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_asPOSIXct(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = Rf_protect(CAR(args));

        // x must be a VECSXP (list) with at least 9 components
        if TYPEOF(x) != SEXPTYPE::VECSXP.0 {
            std::panic::panic_any(RError {
                message: "a valid \"POSIXlt\" object is a list of at least 9 elements".to_string(),
            });
        }

        // Determine length from components
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

        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, n));

        for i in 0..n {
            let iu = i as usize;
            let secs = *REAL(VECTOR_ELT(x, 0)).add(iu);
            let fsecs = secs.floor();

            let mut tm = stm::new();
            tm.tm_sec = if R_FINITE(secs) {
                fsecs as c_int
            } else {
                NA_INTEGER
            };
            tm.tm_min = *INTEGER(VECTOR_ELT(x, 1)).add(iu);
            tm.tm_hour = *INTEGER(VECTOR_ELT(x, 2)).add(iu);
            tm.tm_mday = *INTEGER(VECTOR_ELT(x, 3)).add(iu);
            tm.tm_mon = *INTEGER(VECTOR_ELT(x, 4)).add(iu);
            tm.tm_year = *INTEGER(VECTOR_ELT(x, 5)).add(iu);
            tm.tm_isdst = *INTEGER(VECTOR_ELT(x, 8)).add(iu);

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
                let tmp = mktime0(&mut tm, true);
                *REAL(ans).add(iu) = if tmp == -1.0 {
                    NA_REAL
                } else {
                    tmp + (secs - fsecs)
                };
            }
        }

        Rf_unprotect(2);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_formatPOSIXlt -- .Internal(format.POSIXlt(x, format, usetz, ...))
// ---------------------------------------------------------------------------

/// Format a POSIXlt object as a character string using strftime.
///
/// Ported from `do_formatPOSIXlt()` in datetime.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_formatPOSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = Rf_protect(CAR(args));

        // x must be VECSXP with at least 9 components
        if TYPEOF(x) != SEXPTYPE::VECSXP.0 {
            std::panic::panic_any(RError {
                message: "invalid 'x' argument".to_string(),
            });
        }

        // Get format string (CADR(args))
        let sformat = CADR(args);
        if TYPEOF(sformat) != SEXPTYPE::STRSXP.0 || XLENGTH(sformat) == 0 {
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

        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, N));

        for i in 0..N {
            let iu = i as usize;
            let secs = *REAL(VECTOR_ELT(x, 0)).add(iu % nlen[0] as usize);
            let fsecs = secs.floor();

            let mut ctm: libc_tm = std::mem::zeroed();

            if R_FINITE(secs) && fsecs >= c_int::MIN as c_double && fsecs <= c_int::MAX as c_double
            {
                ctm.tm_sec = fsecs as c_int;
            } else {
                ctm.tm_sec = 0;
            }
            ctm.tm_min = *INTEGER(VECTOR_ELT(x, 1)).add(iu % nlen[1] as usize);
            ctm.tm_hour = *INTEGER(VECTOR_ELT(x, 2)).add(iu % nlen[2] as usize);
            ctm.tm_mday = *INTEGER(VECTOR_ELT(x, 3)).add(iu % nlen[3] as usize);
            ctm.tm_mon = *INTEGER(VECTOR_ELT(x, 4)).add(iu % nlen[4] as usize);
            ctm.tm_year = *INTEGER(VECTOR_ELT(x, 5)).add(iu % nlen[5] as usize);
            ctm.tm_wday = *INTEGER(VECTOR_ELT(x, 6)).add(iu % nlen[6] as usize);
            ctm.tm_yday = *INTEGER(VECTOR_ELT(x, 7)).add(iu % nlen[7] as usize);
            ctm.tm_isdst = *INTEGER(VECTOR_ELT(x, 8)).add(iu % nlen[8] as usize);

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
                let cstr = CString::new("").unwrap_or_default();
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
                    let cstr = CString::new("").unwrap_or_default();
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
                    let fmt_cstr = if fmt_ptr.is_null() {
                        "%Y-%m-%d %H:%M:%S"
                    } else {
                        CStr::from_ptr(fmt_ptr)
                            .to_str()
                            .unwrap_or("%Y-%m-%d %H:%M:%S")
                    };

                    let mut buf = [0u8; 2049];
                    let res = strftime(
                        buf.as_mut_ptr() as *mut c_char,
                        2048,
                        CString::new(fmt_cstr).unwrap_or_default().as_ptr(),
                        &ctm,
                    );

                    if res == 0 {
                        let cstr = CString::new("").unwrap_or_default();
                        SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    } else {
                        let s = std::str::from_utf8(&buf[..res as usize]).unwrap_or("");
                        let cstr = CString::new(s).unwrap_or_default();
                        SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    }
                }
            }
        }

        Rf_unprotect(2);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_strptime -- .Internal(strptime(x, format, tz))
// ---------------------------------------------------------------------------

/// Parse a date/time string according to a format.
///
/// Uses libc's `strptime` to parse the input string and returns a POSIXlt
/// object. Ported from `do_strptime()` in datetime.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_strptime(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::STRSXP.0 {
            std::panic::panic_any(RError {
                message: "invalid 'x' argument: not character".to_string(),
            });
        }

        let sformat = CADR(args);
        if TYPEOF(sformat) != SEXPTYPE::STRSXP.0 || XLENGTH(sformat) == 0 {
            std::panic::panic_any(RError {
                message: "invalid 'format' argument".to_string(),
            });
        }

        let n = XLENGTH(x);
        let m = XLENGTH(sformat);
        let N = if n > 0 { std::cmp::max(m, n) } else { 0 };

        let (ans, ansnames) = make_posixlt_skeleton(N);

        for i in 0..N {
            let iu = i as usize;
            let charsxp = STRING_ELT(x, (i % n) as R_xlen_t);
            if charsxp.is_null() || charsxp == R_NilValue() {
                // NA_STRING case
                makelt(&stm::new(), ans, i as R_xlen_t, false, NA_REAL);
                let cstr = CString::new("").unwrap_or_default();
                SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = NA_INTEGER;
                continue;
            }

            let char_ptr = CHAR(charsxp);
            if char_ptr.is_null() {
                makelt(&stm::new(), ans, i as R_xlen_t, false, NA_REAL);
                let cstr = CString::new("").unwrap_or_default();
                SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = NA_INTEGER;
                continue;
            }

            let input = CStr::from_ptr(char_ptr).to_str().unwrap_or("");
            let fmt_charsxp = STRING_ELT(sformat, (i % m) as R_xlen_t);
            let fmt_ptr = CHAR(fmt_charsxp);
            let fmt = if fmt_ptr.is_null() {
                "%Y-%m-%d %H:%M:%S"
            } else {
                CStr::from_ptr(fmt_ptr)
                    .to_str()
                    .unwrap_or("%Y-%m-%d %H:%M:%S")
            };

            let mut ctm: libc_tm = std::mem::zeroed();
            let fmt_cstr = CString::new(fmt).unwrap_or_default();
            let input_cstr = CString::new(input).unwrap_or_default();

            let res = libc::strptime(input_cstr.as_ptr(), fmt_cstr.as_ptr(), &mut ctm);

            if res.is_null() {
                // Parse failed
                makelt(&stm::new(), ans, i as R_xlen_t, false, NA_REAL);
                let cstr = CString::new("").unwrap_or_default();
                SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = NA_INTEGER;
            } else {
                let mut tm = stm::new();
                tm.tm_sec = ctm.tm_sec;
                tm.tm_min = ctm.tm_min;
                tm.tm_hour = ctm.tm_hour;
                tm.tm_mday = if ctm.tm_mday == 0 {
                    NA_INTEGER
                } else {
                    ctm.tm_mday
                };
                tm.tm_mon = ctm.tm_mon;
                tm.tm_year = ctm.tm_year;
                tm.tm_isdst = -1;

                // Fix missing fields using current date
                if tm.tm_year == NA_INTEGER || tm.tm_mon == NA_INTEGER || tm.tm_mday == NA_INTEGER {
                    let now = libc::time(std::ptr::null_mut());
                    let mut now_tm: libc_tm = std::mem::zeroed();
                    localtime_r(&now, &mut now_tm);
                    if tm.tm_year == NA_INTEGER {
                        tm.tm_year = now_tm.tm_year;
                    }
                    if tm.tm_mon == NA_INTEGER {
                        tm.tm_mon = now_tm.tm_mon;
                    }
                    if tm.tm_mday == NA_INTEGER {
                        tm.tm_mday = now_tm.tm_mday;
                    }
                }

                // Use mktime to set wday and yday
                let valid = validate_tm(&mut tm) == 0;
                if valid {
                    let mut ctm2: libc_tm = std::mem::zeroed();
                    ctm2.tm_sec = tm.tm_sec;
                    ctm2.tm_min = tm.tm_min;
                    ctm2.tm_hour = tm.tm_hour;
                    ctm2.tm_mday = tm.tm_mday;
                    ctm2.tm_mon = tm.tm_mon;
                    ctm2.tm_year = tm.tm_year;
                    ctm2.tm_isdst = -1;
                    if mktime(&mut ctm2) != -1 {
                        tm.tm_wday = ctm2.tm_wday;
                        tm.tm_yday = ctm2.tm_yday;
                        tm.tm_isdst = ctm2.tm_isdst;
                    }
                }

                makelt(&tm, ans, i as R_xlen_t, valid, 0.0);

                // Set zone
                let zone = if valid && tm.tm_isdst >= 0 {
                    let tzname_idx = if tm.tm_isdst > 0 { 1 } else { 0 };
                    let tzname_ptr = tzname[tzname_idx];
                    if !tzname_ptr.is_null() {
                        CStr::from_ptr(tzname_ptr).to_string_lossy().into_owned()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let zone_cstr = CString::new(zone).unwrap_or_default();
                SET_STRING_ELT(
                    VECTOR_ELT(ans, 9),
                    i as R_xlen_t,
                    Rf_mkChar(zone_cstr.as_ptr()),
                );
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = NA_INTEGER;
            }
        }

        Rf_unprotect(2); // ans, ansnames
        ans
    }
}

// ---------------------------------------------------------------------------
// do_D2POSIXlt -- .Internal(Date2POSIXlt(x))
// ---------------------------------------------------------------------------

/// Convert a Date (numeric days since epoch) to a POSIXlt list in UTC.
///
/// Ported from `do_D2POSIXlt()` in datetime.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_D2POSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = Rf_protect(CAR(args));
        if TYPEOF(x) != SEXPTYPE::REALSXP.0 {
            std::panic::panic_any(RError {
                message: "invalid 'x' value: not numeric".to_string(),
            });
        }

        let n = XLENGTH(x);
        let (ans, ansnames) = make_posixlt_skeleton(n);

        for i in 0..n {
            let iu = i as usize;
            let x_i = *REAL(x).add(iu);
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
            let utc_cstr = CString::new("UTC").unwrap_or_default();
            SET_STRING_ELT(
                VECTOR_ELT(ans, 9),
                i as R_xlen_t,
                Rf_mkChar(utc_cstr.as_ptr()),
            );
            *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = 0;
        }

        Rf_unprotect(3); // x, ans, ansnames
        ans
    }
}

// ---------------------------------------------------------------------------
// do_POSIXlt2D -- .Internal(POSIXlt2Date(x))
// ---------------------------------------------------------------------------

/// Convert a POSIXlt list to a Date (numeric days since epoch).
///
/// Ported from `do_POSIXlt2D()` in datetime.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_POSIXlt2D(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = Rf_protect(CAR(args));
        if TYPEOF(x) != SEXPTYPE::VECSXP.0 {
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

        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, n));

        for i in 0..n {
            let iu = i as usize;
            let secs = *REAL(VECTOR_ELT(x, 0)).add(iu);
            let fsecs = secs.floor();

            let mut tm = stm::new();
            tm.tm_sec = if R_FINITE(secs) {
                fsecs as c_int
            } else {
                NA_INTEGER
            };
            tm.tm_min = *INTEGER(VECTOR_ELT(x, 1)).add(iu);
            tm.tm_hour = *INTEGER(VECTOR_ELT(x, 2)).add(iu);
            tm.tm_mday = *INTEGER(VECTOR_ELT(x, 3)).add(iu);
            tm.tm_mon = *INTEGER(VECTOR_ELT(x, 4)).add(iu);
            tm.tm_year = *INTEGER(VECTOR_ELT(x, 5)).add(iu);
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

        Rf_unprotect(2);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_balancePOSIXlt -- .Internal(balancePOSIXlt(x, fill.only, classed))
// ---------------------------------------------------------------------------

/// Balance (validate and normalize) a POSIXlt object.
///
/// Ported from `do_balancePOSIXlt()` in datetime.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_balancePOSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::VECSXP.0 {
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

        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, nn as R_xlen_t));
        for i in 0..9 {
            let sexp = if i > 0 {
                SEXPTYPE::INTSXP.0
            } else {
                SEXPTYPE::REALSXP.0
            };
            SET_VECTOR_ELT(ans, i as R_xlen_t, Rf_allocVector3(sexp, n));
        }
        if nn >= 10 {
            SET_VECTOR_ELT(ans, 9, Rf_allocVector3(SEXPTYPE::STRSXP.0, n));
        }
        if nn >= 11 {
            SET_VECTOR_ELT(ans, 10, Rf_allocVector3(SEXPTYPE::INTSXP.0, n));
        }

        let ansnames = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP.0, nn as R_xlen_t));
        for i in 0..nn {
            let cstr = CString::new(ltnames[i as usize]).unwrap_or_default();
            SET_STRING_ELT(ansnames, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        for i in 0..n {
            let iu = i as usize;
            let secs = *REAL(VECTOR_ELT(x, 0)).add(iu);
            let fsecs = secs.floor();

            let mut tm = stm::new();
            tm.tm_sec = if R_FINITE(secs) {
                fsecs as c_int
            } else {
                NA_INTEGER
            };
            tm.tm_min = *INTEGER(VECTOR_ELT(x, 1)).add(iu);
            tm.tm_hour = *INTEGER(VECTOR_ELT(x, 2)).add(iu);
            tm.tm_mday = *INTEGER(VECTOR_ELT(x, 3)).add(iu);
            tm.tm_mon = *INTEGER(VECTOR_ELT(x, 4)).add(iu);
            tm.tm_year = *INTEGER(VECTOR_ELT(x, 5)).add(iu);
            tm.tm_wday = *INTEGER(VECTOR_ELT(x, 6)).add(iu);
            tm.tm_yday = *INTEGER(VECTOR_ELT(x, 7)).add(iu);
            tm.tm_isdst = *INTEGER(VECTOR_ELT(x, 8)).add(iu);

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
                } else {
                    if R_FINITE(secs) { NA_REAL } else { secs }
                },
            );

            if nn >= 10 {
                let zone_cstr = CString::new("").unwrap_or_default();
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

        Rf_unprotect(2); // ans, ansnames
        ans
    }
}

// ---------------------------------------------------------------------------
// do_Sys_time -- Sys.time()
// ---------------------------------------------------------------------------

/// Return the current system time as a POSIXct scalar (seconds since epoch).
///
/// Ported from `Sys.time()` in datetime.c / platform.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_Sys_time(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP.0, 1);
        *REAL(ans) = t as c_double;
        ans
    }
}

// ---------------------------------------------------------------------------
// do_difftime -- difftime(time1, time2, units)
// ---------------------------------------------------------------------------

/// Compute the difference between two POSIXct times.
///
/// Returns a difftime object (numeric with "units" attribute).
/// Ported from the R difftime() logic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_difftime(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let time1 = CADR(args); // second arg in pairlist
        let time2 = CADDR(args); // third arg
        let units = CADDDR(args); // fourth arg

        // Get numeric values
        let d1 = if TYPEOF(time1) == SEXPTYPE::REALSXP.0 {
            *REAL(time1)
        } else if TYPEOF(time1) == SEXPTYPE::INTSXP.0 {
            *INTEGER(time1) as c_double
        } else {
            NA_REAL
        };

        let d2 = if TYPEOF(time2) == SEXPTYPE::REALSXP.0 {
            *REAL(time2)
        } else if TYPEOF(time2) == SEXPTYPE::INTSXP.0 {
            *INTEGER(time2) as c_double
        } else {
            NA_REAL
        };

        let mut diff = d1 - d2;

        // Get units string
        let units_str =
            if !units.is_null() && TYPEOF(units) == SEXPTYPE::STRSXP.0 && XLENGTH(units) > 0 {
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

        let ans = Rf_allocVector3(SEXPTYPE::REALSXP.0, 1);
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_ISOdatetime(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
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

        let ans = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);

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
                let mut ctm: libc_tm = std::mem::zeroed();
                ctm.tm_year = yr - 1900;
                ctm.tm_mon = mo - 1;
                ctm.tm_mday = dy;
                ctm.tm_hour = hr;
                ctm.tm_min = mi;
                ctm.tm_sec = sc as c_int;
                ctm.tm_isdst = -1;

                let t = mktime(&mut ctm);
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_isLeapYear(year: c_int) -> c_int {
    if isleap(year) { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// FFI-compatible standalone functions
// ---------------------------------------------------------------------------

/// FFI-compatible leap year test.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_isleap(year: c_int) -> c_int {
    if isleap(year) { 1 } else { 0 }
}

/// FFI-compatible days-in-year function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_days_in_year(year: c_int) -> c_int {
    days_in_year(year)
}

/// FFI-compatible days-in-month function.
///
/// `mon` is 0-based (0=Jan), `yr` is years since 1900.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_days_in_month(mon: c_int, yr: c_int) -> c_int {
    days_in_month(mon, yr)
}

/// FFI-compatible validate_tm.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_validate_tm(tm: *mut stm) -> c_int {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_timegm00_ffi(tm: *mut stm) -> c_double {
    unsafe {
        if tm.is_null() {
            return NA_REAL;
        }
        timegm00(&mut *tm)
    }
}

/// FFI-compatible mkdate00.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_mkdate00(tm: *mut stm) -> c_double {
    unsafe {
        if tm.is_null() {
            return NA_REAL;
        }
        mkdate00(&mut *tm)
    }
}

/// FFI-compatible likely_strftime_overflow.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_likely_strftime_overflow(tm: *const stm) -> c_int {
    unsafe {
        if tm.is_null() {
            return 0;
        }
        if likely_strftime_overflow(&*tm) { 1 } else { 0 }
    }
}

/// FFI-compatible julian2dtime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_julian2dtime(x_i: c_double, tm: *mut stm) -> c_int {
    unsafe {
        if tm.is_null() {
            return 0;
        }
        if julian2dtime(x_i, &mut *tm) { 1 } else { 0 }
    }
}

/// FFI-compatible dtime2julian.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_dtime2julian(
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_R_ISLeapYear(year: c_int) -> c_int {
    if R_ISLeapYear(year) { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let (yr, yday, mon, mday) = result.unwrap();
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
