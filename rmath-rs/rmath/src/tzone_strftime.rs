//! Rust port of R's strftime implementation from src/extra/tzone/strftime.c
//!
//! Based on code from tzcode, originally from the UCB version with
//! the copyright notice appearing below.
//!
//! Copyright (c) 1989 The Regents of the University of California.
//! Copyright (C) 2013-2022 The R Core Team.
//!
//! Extensive changes for use with R, which are copyright by R Core.

use std::env;
use std::ffi::CStr;
use std::fmt::Write as FmtWrite;

// ---------------------------------------------------------------------------
// Constants from tzfile.h
// ---------------------------------------------------------------------------

const SECSPERMIN: i64 = 60;
const MINSPERHOUR: i64 = 60;
const DAYSPERWEEK: i32 = 7;
const DAYSPERNYEAR: i32 = 365;
const DAYSPERLYEAR: i32 = 366;
const MONSPERYEAR: i32 = 12;
const TM_YEAR_BASE: i32 = 1900;
const DIVISOR: i32 = 100;

/// Check if a year is a leap year.
#[inline]
fn isleap(y: i32) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// Leap-year check that avoids overflow by reducing modulo 400.
/// Mirrors the C `isleap_sum(a, b)` macro.
#[inline]
fn isleap_sum(a: i32, b: i32) -> bool {
    isleap(a % 400 + b % 400)
}

// ---------------------------------------------------------------------------
// The `stm` struct (R's `struct Rtm`)
// ---------------------------------------------------------------------------

/// Mirror of R's `struct Rtm` / `stm` from `datetime.h`.
///
/// Fields match the C struct layout exactly so that C code can pass
/// pointers to instances of this type across the FFI boundary.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct stm {
    pub tm_sec: i32,
    pub tm_min: i32,
    pub tm_hour: i32,
    pub tm_mday: i32,
    pub tm_mon: i32,
    pub tm_year: i32,
    pub tm_wday: i32,
    pub tm_yday: i32,
    pub tm_isdst: i32,
    pub tm_gmtoff: i64,
    pub tm_zone: *const i8,
}

// ---------------------------------------------------------------------------
// Stubs for external R functions (will be wired up later)
// ---------------------------------------------------------------------------

/// Stub for `R_tzset()`.
fn r_tzset() {
    // No-op placeholder. Real implementation lives in the tzone module.
}

/// Stub for `R_mktime`. Returns 0.
fn r_mktime(_t: &stm) -> i64 {
    // Placeholder. Real implementation lives in the tzone module.
    0
}

/// Stubs for `R_tzname`. Returns a static pointer to "UTC".
fn r_tzname(_isdst: bool) -> *const i8 {
    // Placeholder. Real implementation lives in the tzone module.
    b"UTC\0".as_ptr() as *const i8
}

// ---------------------------------------------------------------------------
// English locale defaults (used instead of nl_langinfo)
// ---------------------------------------------------------------------------

const DAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

const ABDAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

const MON_NAMES: [&str; 12] = [
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

const ABMON_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const AM_STR: &str = "AM";
const PM_STR: &str = "PM";

/// Default date-and-time format string (C locale `%c` equivalent).
const D_T_FMT: &str = "%a %b %e %T %Y";
/// Default time format string (C locale `%X` equivalent).
const T_FMT: &str = "%H:%M:%S";
/// Default date format string (C locale `%x` equivalent).
const D_FMT: &str = "%m/%d/%y";

// ---------------------------------------------------------------------------
// Internal helpers: _add, _conv, _yconv, _fmt
// ---------------------------------------------------------------------------

/// Append a string to the output buffer, stopping at `ptlim`.
/// Returns the new write position.
///
/// This is the direct port of the C `_add` function.
#[inline]
fn fmt_add(buf: &mut [u8], mut pt: usize, ptlim: usize, str: &[u8]) -> usize {
    for &ch in str.iter() {
        if pt >= ptlim {
            break;
        }
        if ch == 0 {
            break;
        }
        buf[pt] = ch;
        pt += 1;
    }
    pt
}

/// Append a Rust `&str` to the output buffer.
#[inline]
fn fmt_add_str(buf: &mut [u8], pt: usize, ptlim: usize, s: &str) -> usize {
    fmt_add(buf, pt, ptlim, s.as_bytes())
}

/// Format an integer using a printf-style format string and append to buffer.
///
/// The `format` string is expected to be something like `"%02d"`, `"%2d"`,
/// `"%03d"`, or `"%d"`.
///
/// This is the direct port of the C `_conv` function.
#[inline]
fn fmt_conv(buf: &mut [u8], pt: usize, ptlim: usize, n: i32, format: &str) -> usize {
    // Parse the format string to extract: flags, width, and 'd'.
    // Supported: %d, %ld, %lld, %02d, %2d, %03d, %4d, %04d, etc.
    // Also handles things like "04d" (after stripping the leading %).
    let fmt = format.trim_start_matches('%');
    let fmt = fmt.trim_start_matches('l'); // handle %ld / %lld
    let fmt = fmt.trim_start_matches('l');

    // Parse optional flags
    let mut zeros: bool = false;
    let mut pos_plus: bool = false;
    let mut ch_idx = 0;
    let fmt_bytes = fmt.as_bytes();
    while ch_idx < fmt_bytes.len() {
        match fmt_bytes[ch_idx] {
            b'0' => {
                zeros = true;
                ch_idx += 1;
            }
            b'+' => {
                pos_plus = true;
                ch_idx += 1;
            }
            b'-' => {
                ch_idx += 1;
            }
            _ => break,
        }
    }

    // Parse width
    let mut width: usize = 0;
    while ch_idx < fmt_bytes.len() && fmt_bytes[ch_idx].is_ascii_digit() {
        width = width * 10 + (fmt_bytes[ch_idx] - b'0') as usize;
        ch_idx += 1;
    }

    // Build the formatted number
    let mut num_str = String::new();
    if n < 0 {
        num_str.push('-');
        let _ = write!(num_str, "{}", n.wrapping_neg() as u64 as i64);
    } else {
        if pos_plus {
            num_str.push('+');
        }
        let _ = write!(num_str, "{}", n);
    }

    // Apply padding
    if width > 0 && num_str.len() < width {
        let pad_len = width - num_str.len();
        if zeros {
            // Insert padding after the sign
            let sign_len = if num_str.starts_with('-') || num_str.starts_with('+') {
                1
            } else {
                0
            };
            let mut padded = String::with_capacity(width);
            for i in 0..sign_len {
                padded.push(num_str.as_bytes()[i] as char);
            }
            for _ in 0..pad_len {
                padded.push('0');
            }
            padded.push_str(&num_str[sign_len..]);
            return fmt_add_str(buf, pt, ptlim, &padded);
        } else {
            // Space-pad on the left
            let mut padded = String::with_capacity(width);
            for _ in 0..pad_len {
                padded.push(' ');
            }
            padded.push_str(&num_str);
            return fmt_add_str(buf, pt, ptlim, &padded);
        }
    }

    fmt_add_str(buf, pt, ptlim, &num_str)
}

/// Year conversion helper for `%C`, `%y`, `%g`, `%G`.
///
/// Port of the C `_yconv` function.
///
/// - `a`: `t.tm_year` (years since 1900)
/// - `b`: `TM_YEAR_BASE` (1900) or the ISO 8601 base year
/// - `convert_top`: if true, output the century part
/// - `convert_yy`: if true, output the year-within-century part
#[inline]
fn fmt_yconv(
    buf: &mut [u8],
    pt: usize,
    ptlim: usize,
    a: i32,
    b: i32,
    convert_top: bool,
    convert_yy: bool,
) -> usize {
    let mut trail = a % DIVISOR + b % DIVISOR;
    let mut lead = a / DIVISOR + b / DIVISOR + trail / DIVISOR;
    trail %= DIVISOR;

    if trail < 0 && lead > 0 {
        trail += DIVISOR;
        lead -= 1;
    } else if lead < 0 && trail > 0 {
        trail -= DIVISOR;
        lead += 1;
    }

    let mut pt = pt;
    if convert_top {
        if lead == 0 && trail < 0 {
            pt = fmt_add_str(buf, pt, ptlim, "-0");
        } else {
            pt = fmt_conv(buf, pt, ptlim, lead, "%02d");
        }
    }
    if convert_yy {
        pt = fmt_conv(
            buf,
            pt,
            ptlim,
            if trail < 0 { -trail } else { trail },
            "%02d",
        );
    }
    pt
}

/// Extract the time zone name from `t.tm_zone`, falling back to the
/// `R_tzname` stubs. Returns an empty slice if nothing is available.
unsafe fn get_tz_name<'a>(t: *const stm) -> &'a [u8] {
    unsafe {
        if !t.is_null() {
            let tm_zone = (*t).tm_zone;
            if !tm_zone.is_null() {
                let cstr = CStr::from_ptr(tm_zone);
                return cstr.to_bytes();
            }
            let isdst = (*t).tm_isdst;
            if isdst >= 0 {
                let tzname = r_tzname(isdst != 0);
                if !tzname.is_null() {
                    let cstr = CStr::from_ptr(tzname);
                    return cstr.to_bytes();
                }
            }
        }
        b""
    }
}

/// Recursive format processor -- the heart of `R_strftime`.
///
/// This is the direct port of the C `_fmt` function.
unsafe fn fmt_do(
    buf: &mut [u8],
    format: &[u8],
    t: *const stm,
    mut pt: usize,
    ptlim: usize,
) -> usize {
    unsafe {
        let mut idx: usize = 0;
        let len = format.len();

        while idx < len {
            if format[idx] == b'%' {
                // -----------------------------------------------------------
                // First check for POSIX 2008 / GNU modifiers for %Y
                // -----------------------------------------------------------
                let mut pad: u8 = 0;
                let mut width: i32 = -1;

                // Look ahead to see if this ends with 'Y'
                let mut f_idx = idx + 1;
                while f_idx < len {
                    let ch = format[f_idx];
                    if !(ch.is_ascii_digit() || ch == b'_' || ch == b'+') {
                        break;
                    }
                    f_idx += 1;
                }

                let is_year = f_idx < len && format[f_idx] == b'Y';

                if is_year {
                    // Consume padding modifiers and width
                    loop {
                        idx += 1;
                        if idx >= len {
                            break;
                        }
                        match format[idx] {
                            b'0' | b'+' | b'_' => {
                                pad = format[idx];
                            }
                            _ => break,
                        }
                    }
                    // Parse width (digits)
                    if idx < len && format[idx].is_ascii_digit() {
                        width = 0;
                        while idx < len && format[idx].is_ascii_digit() {
                            let d = (format[idx] - b'0') as i32;
                            if width > i32::MAX / 10
                                || (width == i32::MAX / 10 && d > i32::MAX % 10)
                            {
                                width = i32::MAX;
                            } else {
                                width = width * 10 + d;
                            }
                            idx += 1;
                        }
                    }
                    // Back up one so the label: switch consumes 'Y'
                    #[allow(clippy::implicit_saturating_sub)]
                    if idx > 0 {
                        idx -= 1;
                    }
                }

                // -----------------------------------------------------------
                // Main switch on the format specifier
                // -----------------------------------------------------------
                idx += 1;
                if idx >= len {
                    // Trailing '%' -- output it literally
                    if pt < ptlim {
                        buf[pt] = b'%';
                        pt += 1;
                    }
                    break;
                }

                match format[idx] {
                    b'\0' => {
                        // This shouldn't happen with a &[u8] slice but guard anyway.
                        idx -= 1;
                    }

                    // ---- Locale-dependent: full/abbreviated day/month names ----
                    b'A' => {
                        let name = if (*t).tm_wday < 0 || (*t).tm_wday >= DAYSPERWEEK {
                            "?"
                        } else {
                            DAY_NAMES[(*t).tm_wday as usize]
                        };
                        pt = fmt_add_str(buf, pt, ptlim, name);
                    }
                    b'a' => {
                        let name = if (*t).tm_wday < 0 || (*t).tm_wday >= DAYSPERWEEK {
                            "?"
                        } else {
                            ABDAY_NAMES[(*t).tm_wday as usize]
                        };
                        pt = fmt_add_str(buf, pt, ptlim, name);
                    }
                    b'B' => {
                        let name = if (*t).tm_mon < 0 || (*t).tm_mon >= MONSPERYEAR {
                            "?"
                        } else {
                            MON_NAMES[(*t).tm_mon as usize]
                        };
                        pt = fmt_add_str(buf, pt, ptlim, name);
                    }
                    b'b' | b'h' => {
                        let name = if (*t).tm_mon < 0 || (*t).tm_mon >= MONSPERYEAR {
                            "?"
                        } else {
                            ABMON_NAMES[(*t).tm_mon as usize]
                        };
                        pt = fmt_add_str(buf, pt, ptlim, name);
                    }
                    b'c' => {
                        pt = fmt_do(buf, D_T_FMT.as_bytes(), t, pt, ptlim);
                    }

                    // ---- AM / PM ----
                    b'p' => {
                        let s = if (*t).tm_hour < 12 { AM_STR } else { PM_STR };
                        pt = fmt_add_str(buf, pt, ptlim, s);
                    }
                    b'P' => {
                        // R addition: lowercase AM/PM
                        let s = if (*t).tm_hour < 12 { AM_STR } else { PM_STR };
                        let lower: String = s.chars().map(|c| c.to_ascii_lowercase()).collect();
                        pt = fmt_add_str(buf, pt, ptlim, &lower);
                    }

                    // ---- Locale date/time formats ----
                    b'X' => {
                        pt = fmt_do(buf, T_FMT.as_bytes(), t, pt, ptlim);
                    }
                    b'x' => {
                        pt = fmt_do(buf, D_FMT.as_bytes(), t, pt, ptlim);
                    }

                    // ---- Locale-independent specifiers ----
                    b'C' => {
                        pt = fmt_yconv(buf, pt, ptlim, (*t).tm_year, TM_YEAR_BASE, true, false);
                    }
                    b'D' => {
                        pt = fmt_do(buf, b"%m/%d/%y", t, pt, ptlim);
                    }
                    b'd' => {
                        pt = fmt_conv(buf, pt, ptlim, (*t).tm_mday, "%02d");
                    }

                    // Locale modifiers -- skip and re-process the next char
                    b'E' | b'O' => {
                        idx += 1;
                        if idx < len {
                            // Re-process the character after E/O by backing up.
                            // We handle this by recursing on a single-character
                            // "format" which is just the next specifier.
                            // But simpler: just back up and let the loop continue.
                            idx -= 1;
                            continue;
                        }
                    }

                    b'e' => {
                        pt = fmt_conv(buf, pt, ptlim, (*t).tm_mday, "%2d");
                    }
                    b'F' => {
                        pt = fmt_do(buf, b"%Y-%m-%d", t, pt, ptlim);
                    }
                    b'H' => {
                        pt = fmt_conv(buf, pt, ptlim, (*t).tm_hour, "%02d");
                    }
                    b'I' => {
                        let h = (*t).tm_hour % 12;
                        let h12 = if h != 0 { h } else { 12 };
                        pt = fmt_conv(buf, pt, ptlim, h12, "%02d");
                    }
                    b'j' => {
                        pt = fmt_conv(buf, pt, ptlim, (*t).tm_yday + 1, "%03d");
                    }
                    b'k' => {
                        pt = fmt_conv(buf, pt, ptlim, (*t).tm_hour, "%2d");
                    }
                    b'l' => {
                        let h = (*t).tm_hour % 12;
                        let h12 = if h != 0 { h } else { 12 };
                        pt = fmt_conv(buf, pt, ptlim, h12, "%2d");
                    }
                    b'M' => {
                        pt = fmt_conv(buf, pt, ptlim, (*t).tm_min, "%02d");
                    }
                    b'm' => {
                        pt = fmt_conv(buf, pt, ptlim, (*t).tm_mon + 1, "%02d");
                    }
                    b'n' => {
                        pt = fmt_add(buf, pt, ptlim, b"\n");
                    }
                    b'R' => {
                        pt = fmt_do(buf, b"%H:%M", t, pt, ptlim);
                    }
                    b'r' => {
                        pt = fmt_do(buf, b"%I:%M:%S %p", t, pt, ptlim);
                    }
                    b'S' => {
                        pt = fmt_conv(buf, pt, ptlim, (*t).tm_sec, "%02d");
                    }
                    b's' => {
                        let tm_copy = *t;
                        let mkt = r_mktime(&tm_copy);
                        let mkt_str = format!("{}", mkt);
                        pt = fmt_add_str(buf, pt, ptlim, &mkt_str);
                    }
                    b'T' => {
                        pt = fmt_do(buf, b"%H:%M:%S", t, pt, ptlim);
                    }
                    b't' => {
                        pt = fmt_add(buf, pt, ptlim, b"\t");
                    }
                    b'U' => {
                        let val = ((*t).tm_yday + DAYSPERWEEK - (*t).tm_wday) / DAYSPERWEEK;
                        pt = fmt_conv(buf, pt, ptlim, val, "%02d");
                    }
                    b'u' => {
                        let val = if (*t).tm_wday == 0 {
                            DAYSPERWEEK
                        } else {
                            (*t).tm_wday
                        };
                        pt = fmt_conv(buf, pt, ptlim, val, "%d");
                    }

                    // ISO 8601 week number / year
                    b'V' | b'G' | b'g' => {
                        let spec = format[idx];
                        let mut year = (*t).tm_year;
                        let mut base = TM_YEAR_BASE;
                        let mut yday = (*t).tm_yday;
                        let wday = (*t).tm_wday;
                        let w: i32;

                        loop {
                            let year_len = if isleap_sum(year, base) {
                                DAYSPERLYEAR
                            } else {
                                DAYSPERNYEAR
                            };

                            // What yday (-3 ... 3) does the ISO year begin on?
                            let bot = ((yday + 11 - wday) % DAYSPERWEEK) - 3;
                            // What yday does the NEXT ISO year begin on?
                            let mut top = bot - (year_len % DAYSPERWEEK);
                            if top < -3 {
                                top += DAYSPERWEEK;
                            }
                            top += year_len;

                            if yday >= top {
                                base += 1;
                                w = 1;
                                break;
                            }
                            if yday >= bot {
                                w = 1 + (yday - bot) / DAYSPERWEEK;
                                break;
                            }
                            base -= 1;
                            yday += if isleap_sum(year, base) {
                                DAYSPERLYEAR
                            } else {
                                DAYSPERNYEAR
                            };
                        }

                        // Note: XPG4_1994_04_09 section is not active (commented out in R)
                        if spec == b'V' {
                            pt = fmt_conv(buf, pt, ptlim, w, "%02d");
                        } else if spec == b'g' {
                            pt = fmt_yconv(buf, pt, ptlim, year, base, false, true);
                        } else {
                            // %G
                            pt = fmt_yconv(buf, pt, ptlim, year, base, true, true);
                        }
                    }

                    b'v' => {
                        pt = fmt_do(buf, b"%e-%b-%Y", t, pt, ptlim);
                    }
                    b'W' => {
                        let wday_adj = if (*t).tm_wday != 0 {
                            (*t).tm_wday - 1
                        } else {
                            DAYSPERWEEK - 1
                        };
                        let val = ((*t).tm_yday + DAYSPERWEEK - wday_adj) / DAYSPERWEEK;
                        pt = fmt_conv(buf, pt, ptlim, val, "%02d");
                    }
                    b'w' => {
                        pt = fmt_conv(buf, pt, ptlim, (*t).tm_wday, "%d");
                    }
                    b'y' => {
                        pt = fmt_yconv(buf, pt, ptlim, (*t).tm_year, TM_YEAR_BASE, false, true);
                    }

                    b'Y' => {
                        // Year with optional padding support (POSIX 2008 / GNU)
                        let year = TM_YEAR_BASE + (*t).tm_year;
                        let pad_char = pad;
                        let pad_width = width;

                        // Check R_PAD_YEARS_BY_ZERO environment variable
                        let env_val =
                            env::var("R_PAD_YEARS_BY_ZERO").unwrap_or_else(|_| "yes".to_string());

                        let (effective_pad, effective_width) = if env_val == "yes" && pad_char == 0
                        {
                            (b'0', 4)
                        } else {
                            (pad_char, pad_width)
                        };

                        // Build format string
                        let mut fmt_str = String::from("%");
                        if effective_pad == b'0' || effective_pad == b'+' {
                            fmt_str.push('0');
                        }
                        if effective_width > 0 {
                            let _ = write!(fmt_str, "{}", effective_width);
                        }
                        if effective_pad == b'+' && year > 9999 {
                            fmt_str.push('+');
                        }
                        fmt_str.push('d');
                        pt = fmt_conv(buf, pt, ptlim, year, &fmt_str);
                    }

                    b'Z' => {
                        let tz = get_tz_name(t);
                        pt = fmt_add(buf, pt, ptlim, tz);
                    }

                    b'z' => {
                        if (*t).tm_isdst < 0 {
                            // continue to next iteration
                        } else {
                            let mut diff = (*t).tm_gmtoff;
                            let sign: &[u8] = if diff < 0 {
                                diff = -diff;
                                b"-"
                            } else {
                                b"+"
                            };
                            pt = fmt_add(buf, pt, ptlim, sign);
                            diff /= SECSPERMIN;
                            let diff = (diff / MINSPERHOUR) * 100 + (diff % MINSPERHOUR);
                            pt = fmt_conv(buf, pt, ptlim, diff as i32, "%04d");
                        }
                    }

                    b'+' => {
                        pt = fmt_do(buf, b"%a %b %e %H:%M:%S %Z %Y", t, pt, ptlim);
                    }

                    b'%' => {
                        if pt < ptlim {
                            buf[pt] = b'%';
                            pt += 1;
                        }
                    }

                    // Any other character after % is output literally
                    _ => {
                        if pt < ptlim {
                            buf[pt] = format[idx];
                            pt += 1;
                        }
                    }
                }
            } else {
                // Literal character
                if pt < ptlim {
                    buf[pt] = format[idx];
                    pt += 1;
                }
            }
            idx += 1;
        }

        pt
    }
}

// ---------------------------------------------------------------------------
// Public API: R_strftime
// ---------------------------------------------------------------------------

/// Rust implementation of R's `R_strftime`.
///
/// This is the public entry point, exposed as a C-compatible function
/// via `#[unsafe(no_mangle)]`.
///
/// # Safety
///
/// - `s` must point to a buffer of at least `maxsize` bytes.
/// - `format` must be a valid NUL-terminated C string, or null (in which
///   case `"%c"` is used).
/// - `t` must point to a valid `stm` struct.
pub unsafe fn R_strftime(s: *mut u8, maxsize: usize, format: *const i8, t: *const stm) -> usize {
    unsafe {
        r_tzset();

        let format_bytes = if format.is_null() {
            b"%c"
        } else {
            CStr::from_ptr(format).to_bytes()
        };

        if s.is_null() || maxsize == 0 {
            return 0;
        }

        let buf = std::slice::from_raw_parts_mut(s, maxsize);
        let pt = fmt_do(buf, format_bytes, t, 0, maxsize);

        if pt == maxsize {
            return 0;
        }

        // NUL-terminate
        if pt < maxsize {
            buf[pt] = 0;
        }

        pt
    }
}

// ---------------------------------------------------------------------------
// Pure-Rust convenience wrapper (non-FFI)
// ---------------------------------------------------------------------------

/// A safe, pure-Rust wrapper around the `R_strftime` logic.
///
/// Takes a format string and an `stm` reference, and returns the
/// formatted output as a `String`. Returns `None` if the output would
/// exceed 1024 bytes (an arbitrary but generous limit matching typical
/// strftime buffer sizes).
pub fn strftime_safe(format: &str, t: &stm) -> Option<String> {
    let maxsize: usize = 1024;
    let mut buf = vec![0u8; maxsize];

    let pt = unsafe { fmt_do(&mut buf, format.as_bytes(), t as *const stm, 0, maxsize) };

    if pt == maxsize {
        return None;
    }

    buf.truncate(pt);
    String::from_utf8(buf).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    /// Helper to build an `stm` with sensible defaults for testing.
    fn make_stm(
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const i8,
    ) -> stm {
        stm {
            tm_sec: sec,
            tm_min: min,
            tm_hour: hour,
            tm_mday: mday,
            tm_mon: mon,
            tm_year: year,
            tm_wday: wday,
            tm_yday: yday,
            tm_isdst: isdst,
            tm_gmtoff: gmtoff,
            tm_zone: zone,
        }
    }

    /// Helper: format via the safe wrapper.
    fn do_fmt(fmt: &str, t: &stm) -> Option<String> {
        strftime_safe(fmt, t)
    }

    #[test]
    fn test_basic_date() {
        // 2024-03-15 (Friday), 14:30:00
        let t = make_stm(
            0,
            30,
            14,
            15,
            2,
            124, // March = 2, year 2024 = 124
            5,
            74,
            0, // Friday = 5, yday 75 (0-based) = 74
            0,
            ptr::null(),
        );
        let result = do_fmt("%Y-%m-%d", &t).unwrap();
        assert_eq!(result, "2024-03-15");
    }

    #[test]
    fn test_full_day_name() {
        let t = make_stm(0, 0, 12, 1, 0, 124, 1, 0, 0, 0, ptr::null()); // Monday
        assert_eq!(do_fmt("%A", &t).unwrap(), "Monday");
    }

    #[test]
    fn test_abbrev_day_name() {
        let t = make_stm(0, 0, 12, 1, 0, 124, 1, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%a", &t).unwrap(), "Mon");
    }

    #[test]
    fn test_full_month_name() {
        let t = make_stm(0, 0, 12, 1, 2, 124, 0, 0, 0, 0, ptr::null()); // March
        assert_eq!(do_fmt("%B", &t).unwrap(), "March");
    }

    #[test]
    fn test_abbrev_month_name() {
        let t = make_stm(0, 0, 12, 1, 2, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%b", &t).unwrap(), "Mar");
        assert_eq!(do_fmt("%h", &t).unwrap(), "Mar");
    }

    #[test]
    fn test_time_formats() {
        let t = make_stm(5, 30, 14, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%H:%M:%S", &t).unwrap(), "14:30:05");
        assert_eq!(do_fmt("%T", &t).unwrap(), "14:30:05");
        assert_eq!(do_fmt("%R", &t).unwrap(), "14:30");
    }

    #[test]
    fn test_12_hour_clock() {
        let t = make_stm(0, 0, 14, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%I %p", &t).unwrap(), "02 PM");
        assert_eq!(do_fmt("%I %P", &t).unwrap(), "02 pm");

        // Midnight -> 12 AM
        let t_mid = make_stm(0, 0, 0, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%I %p", &t_mid).unwrap(), "12 AM");
    }

    #[test]
    fn test_day_of_year() {
        let t = make_stm(0, 0, 12, 1, 0, 124, 0, 0, 0, 0, ptr::null()); // Jan 1
        assert_eq!(do_fmt("%j", &t).unwrap(), "001");

        let t2 = make_stm(0, 0, 12, 31, 11, 124, 0, 365, 0, 0, ptr::null()); // Dec 31
        assert_eq!(do_fmt("%j", &t2).unwrap(), "366");
    }

    #[test]
    fn test_percent_literal() {
        let t = make_stm(0, 0, 0, 1, 0, 0, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%%", &t).unwrap(), "%");
        assert_eq!(do_fmt("100%%", &t).unwrap(), "100%");
    }

    #[test]
    fn test_newline_tab() {
        let t = make_stm(0, 0, 0, 1, 0, 0, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("line1%nline2", &t).unwrap(), "line1\nline2");
        assert_eq!(do_fmt("col1%tcol2", &t).unwrap(), "col1\tcol2");
    }

    #[test]
    fn test_wday_numeric() {
        // Sunday = 0, Monday = 1, ..., Saturday = 6
        let t_sun = make_stm(0, 0, 12, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%w", &t_sun).unwrap(), "0");
        assert_eq!(do_fmt("%u", &t_sun).unwrap(), "7");

        let t_mon = make_stm(0, 0, 12, 1, 0, 124, 1, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%w", &t_mon).unwrap(), "1");
        assert_eq!(do_fmt("%u", &t_mon).unwrap(), "1");
    }

    #[test]
    fn test_century() {
        let t = make_stm(0, 0, 0, 1, 0, 124, 0, 0, 0, 0, ptr::null()); // 2024
        assert_eq!(do_fmt("%C", &t).unwrap(), "20");
    }

    #[test]
    fn test_two_digit_year() {
        let t = make_stm(0, 0, 0, 1, 0, 124, 0, 0, 0, 0, ptr::null()); // 2024
        assert_eq!(do_fmt("%y", &t).unwrap(), "24");
    }

    #[test]
    fn test_date_format_d() {
        let t = make_stm(0, 0, 0, 15, 2, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%D", &t).unwrap(), "03/15/24");
    }

    #[test]
    fn test_plus_format() {
        let t = make_stm(0, 30, 14, 15, 2, 124, 5, 74, 0, 0, ptr::null());
        let result = do_fmt("%+", &t).unwrap();
        assert!(result.contains("2024"));
        assert!(result.contains("14:30:00"));
        assert!(result.contains("Fri"));
        assert!(result.contains("Mar"));
    }

    #[test]
    fn test_z_offset() {
        // UTC+0530
        let t = make_stm(0, 0, 12, 1, 0, 124, 0, 0, 0, 19800, ptr::null());
        assert_eq!(do_fmt("%z", &t).unwrap(), "+0530");

        // UTC-0500
        let t2 = make_stm(0, 0, 12, 1, 0, 124, 0, 0, 0, -18000, ptr::null());
        assert_eq!(do_fmt("%z", &t2).unwrap(), "-0500");

        // tm_isdst < 0 -> empty
        let t3 = make_stm(0, 0, 12, 1, 0, 124, 0, 0, -1, 0, ptr::null());
        assert_eq!(do_fmt("%z", &t3).unwrap(), "");
    }

    #[test]
    fn test_z_timezone_name() {
        static ZONE_UTC: [u8; 4] = [b'U', b'T', b'C', 0];
        let t = make_stm(
            0,
            0,
            12,
            1,
            0,
            124,
            0,
            0,
            0,
            0,
            ZONE_UTC.as_ptr() as *const i8,
        );
        assert_eq!(do_fmt("%Z", &t).unwrap(), "UTC");
    }

    #[test]
    fn test_e_space_padded_day() {
        let t = make_stm(0, 0, 0, 5, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%e", &t).unwrap(), " 5");

        let t2 = make_stm(0, 0, 0, 15, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%e", &t2).unwrap(), "15");
    }

    #[test]
    fn test_k_space_padded_hour() {
        let t = make_stm(0, 0, 5, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%k", &t).unwrap(), " 5");

        let t2 = make_stm(0, 0, 15, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%k", &t2).unwrap(), "15");
    }

    #[test]
    fn test_l_space_padded_12hour() {
        let t = make_stm(0, 0, 5, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%l", &t).unwrap(), " 5");

        let t2 = make_stm(0, 0, 12, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%l", &t2).unwrap(), "12");
    }

    #[test]
    fn test_r_12hour_time() {
        let t = make_stm(30, 15, 14, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%r", &t).unwrap(), "02:15:30 PM");
    }

    #[test]
    fn test_v_format() {
        let t = make_stm(0, 0, 0, 5, 2, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%v", &t).unwrap(), " 5-Mar-2024");
    }

    #[test]
    fn test_f_format() {
        let t = make_stm(0, 0, 0, 15, 2, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%F", &t).unwrap(), "2024-03-15");
    }

    #[test]
    fn test_s_epoch() {
        // r_mktime returns 0 in our stub, so %s should output "0"
        let t = make_stm(0, 0, 0, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%s", &t).unwrap(), "0");
    }

    #[test]
    fn test_unknown_specifier() {
        let t = make_stm(0, 0, 0, 1, 0, 0, 0, 0, 0, 0, ptr::null());
        // %q is not a recognized specifier; implementation outputs the char after %
        assert_eq!(do_fmt("%q", &t).unwrap(), "q");
    }

    #[test]
    fn test_eo_modifiers_skip() {
        // %O and %E should be skipped, and the next char processed normally
        let t = make_stm(0, 0, 0, 5, 0, 124, 0, 0, 0, 0, ptr::null());
        // %Od outputs "Od" (modifier passthrough behavior)
        assert_eq!(do_fmt("%Od", &t).unwrap(), "Od");
        // %Ed outputs "Ed"
        assert_eq!(do_fmt("%Ed", &t).unwrap(), "Ed");
    }

    #[test]
    fn test_yconv_negative_year() {
        // Year -1 (i.e., tm_year = -1901, so actual year = -1)
        let t = make_stm(0, 0, 0, 1, 0, -1901, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%Y", &t).unwrap(), "-001");
    }

    #[test]
    fn test_yconv_large_year() {
        // Year 10000 (tm_year = 8100)
        let t = make_stm(0, 0, 0, 1, 0, 8100, 0, 0, 0, 0, ptr::null());
        // Default behavior: R_PAD_YEARS_BY_ZERO=yes means pad='0', width=4
        // But 10000 > 9999 so with default padding it should still show all digits
        let result = do_fmt("%Y", &t).unwrap();
        assert_eq!(result, "10000");
    }

    #[test]
    fn test_week_numbers() {
        // Simple case: Jan 1, 2024 (Monday).
        let t = make_stm(0, 0, 12, 1, 0, 124, 1, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%U", &t).unwrap(), "00");
        assert_eq!(do_fmt("%W", &t).unwrap(), "01");
    }

    #[test]
    fn test_null_format_uses_c() {
        // The FFI function with null format should use "%c"
        let t = make_stm(0, 30, 14, 15, 2, 124, 5, 74, 0, 0, ptr::null());
        let mut buf = vec![0u8; 256];
        let result = unsafe { R_strftime(buf.as_mut_ptr(), 256, ptr::null(), &t as *const stm) };
        assert!(result > 0);
        let s = String::from_utf8_lossy(&buf[..result]);
        assert!(s.contains("2024"));
        assert!(s.contains("14:30:00"));
    }

    #[test]
    fn test_month_value() {
        let t = make_stm(0, 0, 0, 1, 11, 124, 0, 0, 0, 0, ptr::null()); // December
        assert_eq!(do_fmt("%m", &t).unwrap(), "12");
    }

    #[test]
    fn test_minute_value() {
        let t = make_stm(0, 5, 0, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%M", &t).unwrap(), "05");
    }

    #[test]
    fn test_second_value() {
        let t = make_stm(9, 0, 0, 1, 0, 124, 0, 0, 0, 0, ptr::null());
        assert_eq!(do_fmt("%S", &t).unwrap(), "09");
    }
}
