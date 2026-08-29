//! Port of R's src/main/Rstrptime.h (trunk, r90447+).
//!
//! A modified version of code from the GNU C library with locale support
//! removed, ported to Rust. The state machine operates on `char`s (Unicode
//! scalar values), which mirrors trunk's multibyte (`wchar_t`) path: the
//! input and format are decoded once up front and the parser walks code
//! points. Locale-dependent name tables use the C-locale English strings
//! (the port's `get_locale_strings()` equivalent is identity, matching the
//! "C" locale that `strftime` would produce there).
//!
//! Includes the recent trunk fixes:
//! - r90409: out-of-range `%OS` sets `tm_sec = NA_INTEGER, psecs = NA_REAL`
//!   and `Inf` is accepted (`R_strtod` reads "Inf").
//! - r90442: `%OS<n>` skips the `<n>` digits instead of failing
//!   (`strptime("56.789", "%OS3")`).
//! - r90447 (PR#19124): `strptime("0", "%w")` in a "C" locale works in
//!   valid cases -- the week reconciliation adds 7 to a negative computed
//!   `yday` instead of leaving it out of range.

use std::os::raw::{c_double, c_int};

use super::datetime::stm;
use crate::sexp::context::RError;

pub(crate) const NA_INTEGER: c_int = crate::sexp::ffi::NA_INTEGER;
pub(crate) const NA_REAL: c_double = crate::sexp::ffi::NA_REAL;

// ---------------------------------------------------------------------------
// Locale strings (C locale: what get_locale_strings() produces there)
// ---------------------------------------------------------------------------

const WEEKDAY_NAME: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const AB_WEEKDAY_NAME: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAME: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const AB_MONTH_NAME: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const AM_PM: [&str; 2] = ["AM", "PM"];

const HERE_D_T_FMT: &str = "%a %b %e %H:%M:%S %Y";
const HERE_D_FMT: &str = "%y/%m/%d";
const HERE_T_FMT_AMPM: &str = "%I:%M:%S %p";
const HERE_T_FMT: &str = "%H:%M:%S";

const MON_YDAY: [[c_int; 13]; 2] = [
    // Normal years.
    [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365],
    // Leap years.
    [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366],
];

#[inline]
fn isleap(year: c_int) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// C-locale `isspace`/`iswspace` (the parser's whitespace class).
#[inline]
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

// ---------------------------------------------------------------------------
// Compute the day of the week / year
// ---------------------------------------------------------------------------

/// Compute the day of the week.
///
/// R bug fix: needs year, month, mday set (NA guards from Rstrptime.h).
fn day_of_the_week(tm: &mut stm) {
    // We know that January 1st 1970 was a Thursday (= 4). Compute the
    // difference between this date and the one in TM and so determine
    // the weekday.
    if tm.tm_year == NA_INTEGER || tm.tm_mon == NA_INTEGER || tm.tm_mday == NA_INTEGER {
        return;
    }

    let corr_year = 1900 + tm.tm_year - c_int::from(tm.tm_mon < 2);
    let wday = -473
        + (365 * (tm.tm_year - 70))
        + (corr_year / 4)
        - ((corr_year / 4) / 25)
        + c_int::from((corr_year / 4) % 25 < 0)
        + (((corr_year / 4) / 25) / 4)
        + MON_YDAY[0][tm.tm_mon as usize] // no check on month in range
        + tm.tm_mday
        - 1;
    tm.tm_wday = ((wday % 7) + 7) % 7;
}

/// Compute the day of the year.
///
/// R bug fix: needs year, month, mday set (NA guards from Rstrptime.h).
fn day_of_the_year(tm: &mut stm) {
    if tm.tm_year == NA_INTEGER || tm.tm_mon == NA_INTEGER || tm.tm_mday == NA_INTEGER {
        return;
    }

    tm.tm_yday = MON_YDAY[isleap(1900 + tm.tm_year) as usize][tm.tm_mon as usize] + tm.tm_mday - 1;
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// `match_char(ch1, ch2)`: fail unless the input char matches.
#[inline]
fn match_char(rp: &[char], pos: &mut usize, ch: char) -> Option<()> {
    if *pos >= rp.len() || rp[*pos] != ch {
        return None;
    }
    *pos += 1;
    Some(())
}

/// `match_string`: case-insensitive prefix match; advances `pos` on match.
fn match_string(cs: &str, rp: &[char], pos: &mut usize) -> bool {
    let mut scan = *pos;
    for c in cs.chars() {
        if scan >= rp.len() || rp[scan].to_ascii_lowercase() != c.to_ascii_lowercase() {
            return false;
        }
        scan += 1;
    }
    *pos = scan;
    true
}

/// `get_number(from, to, n)`: read up to `n` digits (at least one),
/// skipping leading spaces, and range-check the result.
fn get_number(rp: &[char], pos: &mut usize, from: c_int, to: c_int, n: c_int) -> Option<c_int> {
    let mut val: c_int = 0;
    while *pos < rp.len() && rp[*pos] == ' ' {
        *pos += 1;
    }
    if *pos >= rp.len() || !rp[*pos].is_ascii_digit() {
        return None;
    }
    let mut cnt = n;
    loop {
        val = val * 10 + (rp[*pos] as u8 - b'0') as c_int;
        *pos += 1;
        cnt -= 1;
        if !(cnt > 0 && *pos < rp.len() && rp[*pos].is_ascii_digit()) {
            break;
        }
    }
    if val < from || val > to {
        return None;
    }
    Some(val)
}

/// `R_strtod()` on the remaining input: parse a C99 double prefix.
///
/// Reuses the crate's `R_strtod` port (util_main.rs), which reads "Inf" and
/// "NaN" like R's version. Returns the value and the number of chars
/// consumed (0 on parse failure).
fn r_strtod_chars(rp: &[char], pos: usize) -> (c_double, usize) {
    let tail: String = rp[pos..].iter().collect();
    let Ok(cbuf) = std::ffi::CString::new(tail) else {
        return (NA_REAL, 0);
    };
    unsafe {
        let mut end: *mut std::os::raw::c_char = std::ptr::null_mut();
        let val = crate::mainutils::util_main::R_strtod(cbuf.as_ptr(), &mut end);
        let consumed = if end.is_null() {
            0
        } else {
            end as usize - cbuf.as_ptr() as usize
        };
        // Chars are 1..4 UTF-8 bytes; count how many code points the byte
        // offset covers.
        let mut used = 0usize;
        let mut off = 0usize;
        while off < consumed && pos + used < rp.len() {
            off += rp[pos + used].len_utf8();
            used += 1;
        }
        (val, used)
    }
}

// ---------------------------------------------------------------------------
// strptime_internal -- the state machine
// ---------------------------------------------------------------------------

/// Parse `rp` from `pos` according to `fmt`, updating `tm`, `psecs` and
/// `poffset`. Returns the new position on success.
///
/// Ported from `strptime_internal()` in Rstrptime.h.
fn strptime_internal(
    rp: &[char],
    mut pos: usize,
    fmt: &[char],
    tm: &mut stm,
    psecs: &mut c_double,
    poffset: &mut c_int,
) -> Option<usize> {
    let mut have_i = false;
    let mut is_pm = false;
    let mut century: c_int = -1;
    let mut want_century = false;
    let mut have_wday = false;
    let mut want_xday = false;
    let mut have_yday = false;
    let mut have_mon = false;
    let mut have_mday = false;
    let mut have_uweek = false;
    let mut have_wweek = false;
    let mut week_no: c_int = 0;

    let mut fi = 0usize;
    while fi < fmt.len() {
        // A white space in the format string matches 0 or more white
        // space in the input string.
        if is_space(fmt[fi]) {
            while pos < rp.len() && is_space(rp[pos]) {
                pos += 1;
            }
            fi += 1;
            continue;
        }

        // Any character but `%' must be matched by the same character
        // in the input string.
        if fmt[fi] != '%' {
            match_char(rp, &mut pos, fmt[fi])?;
            fi += 1;
            continue;
        }

        fi += 1;

        // We need this for handling the `E' modifier (start_over:).
        let c = loop {
            if fi >= fmt.len() {
                // *fmt == '\0' falls into the switch default: fail.
                return None;
            }
            let c = fmt[fi];
            fi += 1;
            if c == 'E' {
                // We have no information about the era format. Just use
                // the normal format.
                let next = if fi < fmt.len() { fmt[fi] } else { '\0' };
                if !matches!(next, 'c' | 'C' | 'y' | 'Y' | 'x' | 'X') {
                    // This is an illegal format.
                    return None;
                }
                continue; // goto start_over
            }
            break c;
        };

        match c {
            '%' => {
                // Match the `%' character itself.
                match_char(rp, &mut pos, '%')?;
            }
            'a' | 'A' => {
                // Match day of week; try full name first.
                let mut cnt = 0;
                while cnt < 7 && !match_string(WEEKDAY_NAME[cnt], rp, &mut pos) {
                    cnt += 1;
                }
                if cnt == 7 {
                    cnt = 0;
                    while cnt < 7 && !match_string(AB_WEEKDAY_NAME[cnt], rp, &mut pos) {
                        cnt += 1;
                    }
                }
                if cnt == 7 {
                    // Does not match a weekday name.
                    return None;
                }
                tm.tm_wday = cnt as c_int;
                have_wday = true;
            }
            'b' | 'B' | 'h' => {
                // Match month name; try full name first.
                let mut cnt = 0;
                while cnt < 12 && !match_string(MONTH_NAME[cnt], rp, &mut pos) {
                    cnt += 1;
                }
                if cnt == 12 {
                    // Try abbreviated names.
                    cnt = 0;
                    while cnt < 12 && !match_string(AB_MONTH_NAME[cnt], rp, &mut pos) {
                        cnt += 1;
                    }
                }
                if cnt == 12 {
                    // Does not match a month name.
                    return None;
                }
                tm.tm_mon = cnt as c_int;
                want_xday = true;
            }
            'c' => {
                // Match locale's date and time format.
                pos = strptime_internal(rp, pos, &to_chars(HERE_D_T_FMT), tm, psecs, poffset)?;
            }
            'C' => {
                // Match century number.
                century = get_number(rp, &mut pos, 0, 99, 2)?;
                want_xday = true;
            }
            'd' | 'e' => {
                // Match day of month.
                tm.tm_mday = get_number(rp, &mut pos, 1, 31, 2)?;
                have_mday = true;
                want_xday = true;
            }
            'F' => {
                pos = strptime_internal(rp, pos, &to_chars("%Y-%m-%d"), tm, psecs, poffset)?;
                want_xday = true;
            }
            'x' | 'D' => {
                // Match standard day format.
                pos = strptime_internal(rp, pos, &to_chars(HERE_D_FMT), tm, psecs, poffset)?;
                want_xday = true;
            }
            'k' | 'H' => {
                // Match hour in 24-hour clock.
                tm.tm_hour = get_number(rp, &mut pos, 0, 24, 2)?; // allow 24:00:00
                have_i = false;
            }
            'l' | 'I' => {
                // Match hour in 12-hour clock.
                tm.tm_hour = get_number(rp, &mut pos, 1, 12, 2)? % 12;
                have_i = true;
            }
            'j' => {
                // Match day number of year.
                tm.tm_yday = get_number(rp, &mut pos, 1, 366, 3)? - 1; // NB: 366 would be invalid in most years
                have_yday = true;
            }
            'm' => {
                // Match number of month.
                tm.tm_mon = get_number(rp, &mut pos, 1, 12, 2)? - 1;
                have_mon = true;
                want_xday = true;
            }
            'M' => {
                // Match minute.
                tm.tm_min = get_number(rp, &mut pos, 0, 59, 2)?;
            }
            'n' | 't' => {
                // Match any white space.
                while pos < rp.len() && is_space(rp[pos]) {
                    pos += 1;
                }
            }
            'p' => {
                // Match locale's equivalent of AM/PM.
                if !match_string(AM_PM[0], rp, &mut pos) {
                    if match_string(AM_PM[1], rp, &mut pos) {
                        is_pm = true;
                    } else {
                        return None;
                    }
                }
            }
            'r' => {
                pos = strptime_internal(rp, pos, &to_chars(HERE_T_FMT_AMPM), tm, psecs, poffset)?;
            }
            'R' => {
                pos = strptime_internal(rp, pos, &to_chars("%H:%M"), tm, psecs, poffset)?;
            }
            's' => {
                // The number of seconds may be very high so we cannot use
                // `get_number'. Instead read the number character for
                // character and construct the result while doing this.
                if pos >= rp.len() || !rp[pos].is_ascii_digit() {
                    // We need at least one digit.
                    return None;
                }
                let mut secs: i64 = 0;
                loop {
                    secs = secs
                        .wrapping_mul(10)
                        .wrapping_add((rp[pos] as u8 - b'0') as i64);
                    pos += 1;
                    if !(pos < rp.len() && rp[pos].is_ascii_digit()) {
                        break;
                    }
                }
                // localtime_r replaces the tm contents, as in trunk.
                let mut ctm: libc::tm = unsafe { std::mem::zeroed() };
                let secs_t = secs as libc::time_t;
                if unsafe { libc::localtime_r(&secs_t, &mut ctm) }.is_null() {
                    return None;
                }
                tm.tm_sec = ctm.tm_sec;
                tm.tm_min = ctm.tm_min;
                tm.tm_hour = ctm.tm_hour;
                tm.tm_mday = ctm.tm_mday;
                tm.tm_mon = ctm.tm_mon;
                tm.tm_year = ctm.tm_year;
                tm.tm_wday = ctm.tm_wday;
                tm.tm_yday = ctm.tm_yday;
                tm.tm_isdst = ctm.tm_isdst;
                tm.tm_gmtoff = ctm.tm_gmtoff;
            }
            'S' => {
                tm.tm_sec = get_number(rp, &mut pos, 0, 61, 2)?;
            }
            'X' | 'T' => {
                pos = strptime_internal(rp, pos, &to_chars(HERE_T_FMT), tm, psecs, poffset)?;
            }
            'u' => {
                tm.tm_wday = get_number(rp, &mut pos, 1, 7, 1)? % 7;
                have_wday = true;
            }
            'g' => {
                // XXX This cannot determine any field in TM.
                get_number(rp, &mut pos, 0, 99, 2)?;
            }
            'G' => {
                if pos >= rp.len() || !rp[pos].is_ascii_digit() {
                    return None;
                }
                // XXX Ignore the number since we would need some more
                // information to compute a real date.
                while pos < rp.len() && rp[pos].is_ascii_digit() {
                    pos += 1;
                }
            }
            'U' => {
                week_no = get_number(rp, &mut pos, 0, 53, 2)?;
                have_uweek = true;
            }
            'W' => {
                week_no = get_number(rp, &mut pos, 0, 53, 2)?;
                have_wweek = true;
            }
            'V' => {
                // XXX This cannot determine any field in TM without some
                // information.
                get_number(rp, &mut pos, 0, 53, 2)?;
            }
            'w' => {
                // Match number of weekday.
                tm.tm_wday = get_number(rp, &mut pos, 0, 6, 1)?;
                have_wday = true;
            }
            'y' => {
                // Match year within century.
                let ival = get_number(rp, &mut pos, 0, 99, 2)?;
                // The "Year 2000: The Millennium Rollover" paper suggests
                // that values in the range 69-99 refer to the twentieth
                // century. Mandated by POSIX 2001, with a caveat.
                tm.tm_year = if ival >= 69 { ival } else { ival + 100 };
                // Indicate that we want to use the century, if specified.
                want_century = true;
                want_xday = true;
            }
            'Y' => {
                // Match year including century number.
                tm.tm_year = get_number(rp, &mut pos, 0, 9999, 4)? - 1900;
                want_century = false;
                want_xday = true;
            }
            'z' => {
                // Only recognize RFC 822 form.
                while pos < rp.len() && rp[pos] == ' ' {
                    pos += 1;
                }
                if pos >= rp.len() || (rp[pos] != '+' && rp[pos] != '-') {
                    return None;
                }
                let neg = rp[pos] == '-';
                pos += 1;
                let mut val: c_int = 0;
                let mut n = 0;
                while n < 4 && pos < rp.len() && rp[pos].is_ascii_digit() {
                    val = val * 10 + (rp[pos] as u8 - b'0') as c_int;
                    pos += 1;
                    n += 1;
                }
                if n != 4 {
                    return None;
                } else {
                    // We have to convert the minutes into decimal.
                    if val % 100 >= 60 {
                        return None;
                    }
                    val = (val / 100) * 100 + ((val % 100) * 50) / 30;
                }
                // https://en.wikipedia.org/wiki/List_of_UTC_time_offsets
                if val > 1400 {
                    unsafe {
                        crate::mainutils::errors::Rf_warning1(
                            c"values for %z outside +/-1400 are an error\n".as_ptr(),
                        );
                    }
                    return None;
                }
                let mut off = (val * 3600) / 100;
                if neg {
                    off = -off;
                }
                *poffset = off;
            }
            'Z' => {
                std::panic::panic_any(RError {
                    message: "use of %Z for input is not supported".to_string(),
                })
            }
            'O' => {
                // Alternative numeric symbols; only %OS carries extra
                // information in this port.
                if fi >= fmt.len() {
                    return None;
                }
                let oc = fmt[fi];
                fi += 1;
                match oc {
                    'd' | 'e' => {
                        tm.tm_mday = get_number(rp, &mut pos, 1, 31, 2)?;
                        have_mday = true;
                        want_xday = true;
                    }
                    'H' => {
                        tm.tm_hour = get_number(rp, &mut pos, 0, 23, 2)?;
                        have_i = false;
                    }
                    'I' => {
                        tm.tm_hour = get_number(rp, &mut pos, 1, 12, 2)? % 12;
                        have_i = true;
                    }
                    'm' => {
                        tm.tm_mon = get_number(rp, &mut pos, 1, 12, 2)? - 1;
                        have_mon = true;
                        want_xday = true;
                    }
                    'M' => {
                        tm.tm_min = get_number(rp, &mut pos, 0, 59, 2)?;
                    }
                    'S' => {
                        // Match seconds using alternate numeric symbols.
                        // %OS<n>: the <n> digits are skipped (r90442).
                        if fi < fmt.len() && fmt[fi].is_ascii_digit() {
                            fi += 1;
                        }
                        let (sval, used) = r_strtod_chars(rp, pos);
                        if (0.0..=61.0).contains(&sval) {
                            tm.tm_sec = sval as c_int;
                            *psecs = sval;
                        } else if sval.is_infinite() {
                            *psecs = sval;
                        } else {
                            tm.tm_sec = NA_INTEGER;
                            *psecs = NA_REAL;
                        }
                        pos += used;
                    }
                    'U' => {
                        week_no = get_number(rp, &mut pos, 0, 53, 2)?;
                        have_uweek = true;
                    }
                    'W' => {
                        week_no = get_number(rp, &mut pos, 0, 53, 2)?;
                        have_wweek = true;
                    }
                    'V' => {
                        // XXX This cannot determine any field in TM without
                        // further information.
                        get_number(rp, &mut pos, 0, 53, 2)?;
                    }
                    'w' => {
                        tm.tm_wday = get_number(rp, &mut pos, 0, 6, 1)?;
                        have_wday = true;
                    }
                    'y' => {
                        let ival = get_number(rp, &mut pos, 0, 99, 2)?;
                        tm.tm_year = if ival >= 69 { ival } else { ival + 100 };
                        want_xday = true;
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }

    if have_i && is_pm {
        tm.tm_hour += 12;
    }

    if century != -1 {
        if want_century {
            tm.tm_year = tm.tm_year % 100 + (century - 19) * 100;
        } else {
            // Only the century, but not the year. Strange, but so be it.
            tm.tm_year = (century - 19) * 100;
        }
    }

    if want_xday && !have_wday {
        if !(have_mon && have_mday) && have_yday {
            // have_yday, so this must have come from %j.
            // We don't have tm_mon and/or tm_mday, compute them.
            let mut t_mon = 0usize;
            let yr = 1900 + tm.tm_year;
            if tm.tm_yday > if isleap(yr) { 365 } else { 364 } {
                let msg = std::ffi::CString::new(format!(
                    "day-of-year {} in year {} is invalid\n",
                    tm.tm_yday + 1,
                    yr
                ))
                .unwrap_or_default();
                unsafe {
                    crate::mainutils::errors::Rf_warning1(msg.as_ptr());
                }
                t_mon = 12; // this will give an invalid mday, so invalid tm
            } else {
                while MON_YDAY[isleap(yr) as usize][t_mon] <= tm.tm_yday {
                    t_mon += 1;
                }
            }
            if !have_mon {
                tm.tm_mon = t_mon as c_int - 1;
            }
            if !have_mday {
                tm.tm_mday = tm.tm_yday - MON_YDAY[isleap(yr) as usize][t_mon - 1] + 1;
            }
        }
        day_of_the_week(tm);
    }

    if want_xday && !have_yday {
        day_of_the_year(tm);
    }

    if (have_uweek || have_wweek) && have_wday {
        let save_wday = tm.tm_wday;
        let save_mday = tm.tm_mday;
        let save_mon = tm.tm_mon;
        let w_offset = c_int::from(!have_uweek);

        tm.tm_mday = 1;
        tm.tm_mon = 0;
        day_of_the_week(tm);
        if have_mday {
            tm.tm_mday = save_mday;
        }
        if have_mon {
            tm.tm_mon = save_mon;
        }

        if !have_yday {
            // Get yday from the week and day-of-the-week.
            // This does not validate yday against any upper limit.
            tm.tm_yday = (7 - (tm.tm_wday - w_offset)) % 7
                + (week_no - 1) * 7
                + save_wday
                - w_offset;
            if tm.tm_yday < 0 {
                tm.tm_yday += 7; // r90447 / PR#19124
            }
        }

        if !have_mday || !have_mon {
            let mut t_mon = 0usize;
            let yr = 1900 + tm.tm_year;
            if tm.tm_yday > if isleap(yr) { 365 } else { 364 } {
                let msg = std::ffi::CString::new(format!(
                    "(0-based) yday {} in year {} is invalid\n",
                    tm.tm_yday, yr
                ))
                .unwrap_or_default();
                unsafe {
                    crate::mainutils::errors::Rf_warning1(msg.as_ptr());
                }
                t_mon = 12;
            } else {
                while MON_YDAY[isleap(yr) as usize][t_mon] <= tm.tm_yday {
                    t_mon += 1;
                }
            }
            if !have_mon {
                tm.tm_mon = t_mon as c_int - 1;
            }
            if !have_mday {
                tm.tm_mday = tm.tm_yday - MON_YDAY[isleap(yr) as usize][t_mon - 1] + 1;
            }
        }

        tm.tm_wday = save_wday;
    }

    Some(pos)
}

/// Decode a `&str` into the char slice the parser walks.
#[inline]
fn to_chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

// ---------------------------------------------------------------------------
// R_strptime -- public entry point
// ---------------------------------------------------------------------------

/// Convert a string representation of a time to a broken-down time.
///
/// Ported from `R_strptime()` in Rstrptime.h. Returns `true` on success
/// (non-NULL return in C) with `tm`/`psecs`/`poffset` filled in; `false` on
/// parse failure. Errors ("input string is too long", "%Z") abort the call
/// like trunk's `error()` calls.
///
/// Trunk converts to `wchar_t` under a multibyte locale, which also
/// validates the encoding; here the caller supplies UTF-8-decoded `&str`s
/// (invalid multibyte input must be rejected before this point).
pub fn R_strptime(
    buf: &str,
    format: &str,
    tm: &mut stm,
    psecs: &mut c_double,
    poffset: &mut c_int,
) -> bool {
    let wbuf = to_chars(buf);
    let wfmt = to_chars(format);
    if wbuf.len() > 1000 {
        std::panic::panic_any(RError {
            message: "input string is too long".to_string(),
        });
    }
    if wfmt.len() > 1000 {
        std::panic::panic_any(RError {
            message: "format string is too long".to_string(),
        });
    }
    strptime_internal(&wbuf, 0, &wfmt, tm, psecs, poffset).is_some()
}
