use super::*;

// Wild abbreviation (three spaces).
pub(crate) static WILDABBR: &[u8] = b"   ";

// ---------------------------------------------------------------------------
// Helper macros / inline functions
// ---------------------------------------------------------------------------

#[inline(always)]
pub(crate) fn isleap(y: i32) -> bool {
    (y % 4 == 0) && ((y % 100 != 0) || (y % 400 == 0))
}

#[inline(always)]
pub(crate) fn is_digit(c: u8) -> bool {
    c.wrapping_sub(b'0') <= 9
}

/// Compute min/max for i64 (time_t).
#[inline(always)]
pub(crate) fn time_t_min() -> i64 {
    i64::MIN
}

#[inline(always)]
pub(crate) fn time_t_max() -> i64 {
    i64::MAX
}

// ---------------------------------------------------------------------------
// Month/year length tables
// ---------------------------------------------------------------------------

pub(crate) static MON_LENGTHS: [[i32; MONSPERYEAR]; 2] = [
    [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
    [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
];

pub(crate) static YEAR_LENGTHS: [i32; 2] = [DAYSPERNYEAR, DAYSPERLYEAR];

// ---------------------------------------------------------------------------
// Decode timezone file codes
// ---------------------------------------------------------------------------

pub(crate) fn detzcode(codep: &[u8]) -> i32 {
    i32::from_be_bytes([codep[0], codep[1], codep[2], codep[3]])
}

pub(crate) fn detzcode64(codep: &[u8]) -> i64 {
    i64::from_be_bytes([
        codep[0], codep[1], codep[2], codep[3], codep[4], codep[5], codep[6], codep[7],
    ])
}

// ---------------------------------------------------------------------------
// Overflow-safe arithmetic
// ---------------------------------------------------------------------------

/// Returns true if overflow occurred.
pub(crate) fn increment_overflow(ip: &mut i32, j: i32) -> bool {
    let i = *ip;
    if (i >= 0 && j > i32::MAX - i) || (i < 0 && j < i32::MIN - i) {
        return true;
    }
    *ip += j;
    false
}

pub(crate) fn increment_overflow32(lp: &mut i32, m: i32) -> bool {
    let l = *lp;
    if (l >= 0 && m > i32::MAX - l) || (l < 0 && m < i32::MIN - l) {
        return true;
    }
    *lp += m;
    false
}

pub(crate) fn increment_overflow_time(tp: &mut i64, j: i64) -> bool {
    if j < 0 {
        // time_t_min - j <= *tp  (i.e., *tp + j >= time_t_min)
        if i64::MIN - j > *tp {
            return true;
        }
    } else {
        // *tp <= time_t_max - j
        if *tp > i64::MAX - j {
            return true;
        }
    }
    *tp += j;
    false
}

pub(crate) fn normalize_overflow(tensptr: &mut i32, unitsptr: &mut i32, base: i32) -> bool {
    let tensdelta = if *unitsptr >= 0 {
        *unitsptr / base
    } else {
        -1 - (-1 - *unitsptr) / base
    };
    *unitsptr -= tensdelta * base;
    increment_overflow(tensptr, tensdelta)
}

pub(crate) fn normalize_overflow32(tensptr: &mut i32, unitsptr: &mut i32, base: i32) -> bool {
    let tensdelta = if *unitsptr >= 0 {
        *unitsptr / base
    } else {
        -1 - (-1 - *unitsptr) / base
    };
    *unitsptr -= tensdelta * base;
    increment_overflow32(tensptr, tensdelta)
}

// ---------------------------------------------------------------------------
// getTZinfo stub
// ---------------------------------------------------------------------------
