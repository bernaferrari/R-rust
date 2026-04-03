/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2005-2022   The R Core Team
 *
 *  Ported to Rust from src/main/rlocale.c
 *
 *  This module provides replacements for wctype (iswxxxxx) functions,
 *  wc[s]width, and towupper/towlower.  The naming is misleading: apart
 *  from the width data this is not locale-specific. It is rather about
 *  the use of non-Latin characters (including symbols, emojis, ...).
 */

#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::{c_int, c_uint, c_void};

// -----------------------------------------------------------------------
// Type aliases
// -----------------------------------------------------------------------

/// Equivalent to C wint_t (unsigned on most platforms).
pub type wint_t = c_uint;

/// Equivalent to C wchar_t on non-Windows platforms (signed 32-bit).
/// On Windows, R_wchar_t is unsigned int, but we use i32 here
/// matching the non-Windows convention.
pub type R_wchar_t = i32;

/// CJK locale ID constants (matching the C enum).
pub const MB_Default: c_int = 0;
pub const MB_ja_JP: c_int = 1;
pub const MB_ko_KR: c_int = 2;
pub const MB_zh_SG: c_int = 3;
pub const MB_zh_CN: c_int = 4;
pub const MB_zh_HK: c_int = 5;
pub const MB_zh_TW: c_int = 6;
pub const MB_SIZE: usize = 7;

// -----------------------------------------------------------------------
// Surrogate pair detection (from rlocale.h, MinGW-W64 winnls.h)
// -----------------------------------------------------------------------

pub const HIGH_SURROGATE_START: c_uint = 0xd800;
pub const HIGH_SURROGATE_END: c_uint = 0xdbff;
pub const LOW_SURROGATE_START: c_uint = 0xdc00;
pub const LOW_SURROGATE_END: c_uint = 0xdfff;

#[inline]
pub unsafe fn IS_HIGH_SURROGATE(wch: wint_t) -> bool {
    wch >= HIGH_SURROGATE_START && wch <= HIGH_SURROGATE_END
}

#[inline]
pub unsafe fn IS_LOW_SURROGATE(wch: wint_t) -> bool {
    wch >= LOW_SURROGATE_START && wch <= LOW_SURROGATE_END
}

#[inline]
pub unsafe fn IS_SURROGATE_PAIR(hs: wint_t, ls: wint_t) -> bool {
    unsafe { IS_HIGH_SURROGATE(hs) && IS_LOW_SURROGATE(ls) }
}

// -----------------------------------------------------------------------
// Data table structures
// -----------------------------------------------------------------------

/// Interval struct used for zero-width and wctype tables.
/// Equivalent to C `struct interval { int first; int last; };`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct interval {
    pub first: c_int,
    pub last: c_int,
}

/// Interval struct for wcwidth tables, with per-locale width values.
/// Equivalent to C `struct interval_wcwidth { int first; int last; char mb[MB_SIZE]; };`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct interval_wcwidth {
    pub first: c_int,
    pub last: c_int,
    pub mb: [i8; MB_SIZE],
}

/// Pair struct for toupper/tolower tables.
/// Equivalent to C `struct pair { int from; int to; };`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct pair {
    pub from: c_int,
    pub to: c_int,
}

// -----------------------------------------------------------------------
// Placeholder data tables (auto-generated from Unicode data in C builds)
// -----------------------------------------------------------------------

/// Table of character width intervals with per-CJK-locale widths.
/// Populated from rlocale_widths.h table_wcwidth[].
/// In a full port, this would contain all entries from that file.
static table_wcwidth: &[interval_wcwidth] = &[];

/// Count of entries in table_wcwidth.
static table_wcwidth_count: usize = 0;

/// Zero-width character intervals.
/// Populated from rlocale_widths.h zero_width[].
static zero_width: &[interval] = &[];

/// Count of entries in zero_width.
static zero_width_count: usize = 0;

// wctype tables (from rlocale_data.h)
static table_wupper: &[interval] = &[];
static table_wupper_count: usize = 0;

static table_wlower: &[interval] = &[];
static table_wlower_count: usize = 0;

static table_walpha: &[interval] = &[];
static table_walpha_count: usize = 0;

static table_wdigit: &[interval] = &[];
static table_wdigit_count: usize = 0;

static table_wxdigit: &[interval] = &[];
static table_wxdigit_count: usize = 0;

static table_wspace: &[interval] = &[];
static table_wspace_count: usize = 0;

static table_wprint: &[interval] = &[];
static table_wprint_count: usize = 0;

static table_wblank: &[interval] = &[];
static table_wblank_count: usize = 0;

static table_wcntrl: &[interval] = &[];
static table_wcntrl_count: usize = 0;

static table_wpunct: &[interval] = &[];
static table_wpunct_count: usize = 0;

// toupper/tolower tables (from rlocale_toupper.h, rlocale_tolower.h)
static table_toupper: &[pair] = &[];
static table_tolower: &[pair] = &[];

// -----------------------------------------------------------------------
// CJK locale name table
// -----------------------------------------------------------------------

struct cjk_locale_name_t {
    name: &'static str,
    locale: c_int,
}

static cjk_locale_name: &[cjk_locale_name_t] = &[
    // Windows locale names
    cjk_locale_name_t {
        name: "CHINESE(SINGAPORE)_SINGAPORE",
        locale: MB_zh_SG,
    },
    cjk_locale_name_t {
        name: "CHINESE_SINGAPORE",
        locale: MB_zh_SG,
    },
    cjk_locale_name_t {
        name: "CHINESE(PRC)_PEOPLE'S REPUBLIC OF CHINA",
        locale: MB_zh_CN,
    },
    cjk_locale_name_t {
        name: "CHINESE_PEOPLE'S REPUBLIC OF CHINA",
        locale: MB_zh_CN,
    },
    cjk_locale_name_t {
        name: "CHINESE_MACAU S.A.R.",
        locale: MB_zh_HK,
    },
    cjk_locale_name_t {
        name: "CHINESE(PRC)_HONG KONG",
        locale: MB_zh_HK,
    },
    cjk_locale_name_t {
        name: "CHINESE_HONG KONG S.A.R.",
        locale: MB_zh_HK,
    },
    cjk_locale_name_t {
        name: "CHINESE(TAIWAN)_TAIWAN",
        locale: MB_zh_TW,
    },
    cjk_locale_name_t {
        name: "CHINESE_TAIWAN",
        locale: MB_zh_TW,
    },
    cjk_locale_name_t {
        name: "CHINESE-S",
        locale: MB_zh_CN,
    },
    cjk_locale_name_t {
        name: "CHINESE-T",
        locale: MB_zh_TW,
    },
    cjk_locale_name_t {
        name: "JAPANESE_JAPAN",
        locale: MB_ja_JP,
    },
    cjk_locale_name_t {
        name: "JAPANESE",
        locale: MB_ja_JP,
    },
    cjk_locale_name_t {
        name: "KOREAN_KOREA",
        locale: MB_ko_KR,
    },
    cjk_locale_name_t {
        name: "KOREAN",
        locale: MB_ko_KR,
    },
    // Other OSes, but only in default encodings.
    cjk_locale_name_t {
        name: "ZH_TW",
        locale: MB_zh_TW,
    },
    cjk_locale_name_t {
        name: "ZH_CN",
        locale: MB_zh_CN,
    },
    cjk_locale_name_t {
        name: "ZH_CN.BIG5",
        locale: MB_zh_TW,
    },
    cjk_locale_name_t {
        name: "ZH_HK",
        locale: MB_zh_HK,
    },
    cjk_locale_name_t {
        name: "ZH_SG",
        locale: MB_zh_SG,
    },
    cjk_locale_name_t {
        name: "JA_JP",
        locale: MB_ja_JP,
    },
    cjk_locale_name_t {
        name: "KO_KR",
        locale: MB_ko_KR,
    },
    cjk_locale_name_t {
        name: "ZH",
        locale: MB_zh_CN,
    },
    cjk_locale_name_t {
        name: "JA",
        locale: MB_ja_JP,
    },
    cjk_locale_name_t {
        name: "KO",
        locale: MB_ko_KR,
    },
    // Default, where all EA Ambiguous characters have width one.
    cjk_locale_name_t {
        name: "",
        locale: MB_Default,
    },
];

// -----------------------------------------------------------------------
// Core binary search functions
// -----------------------------------------------------------------------

/// Binary search in interval tables (for zero_width and wctype tables).
///
/// Returns 1 if `wint` falls within any interval in `table`, 0 otherwise.
/// `max` is 1-based (number of elements in the table).
///
/// This is based on Markus Kuhn's function but with 1-based `max`.
fn wcsearch(wint: c_int, table: &[interval], max: c_int) -> c_int {
    let mut min: c_int = 0;
    let mut max = max - 1;
    let max_idx = max as usize;
    let min_idx = min as usize;

    if table.is_empty() {
        return 0;
    }

    if wint < table[0].first || wint > table[max_idx].last {
        return 0;
    }
    while max >= min {
        let mid = ((min + max) / 2) as usize;
        if wint > table[mid].last {
            min = (mid as c_int) + 1;
        } else if wint < table[mid].first {
            max = (mid as c_int) - 1;
        } else {
            return 1;
        }
    }
    0
}

/// Binary search in wcwidth tables.
///
/// Returns the character width for the given locale, or -1 if not found.
/// `max` is 1-based (number of elements in the table).
/// `locale` is the CJK locale ID (0 = MB_Default, etc.).
fn wcwidthsearch(wint: c_int, table: &[interval_wcwidth], max: c_int, locale: c_int) -> c_int {
    let mut min: c_int = 0;
    let mut max = max - 1;

    if table.is_empty() {
        return -1;
    }

    // This quickly gives one for printing ASCII characters
    if wint > 0x1F && wint < 0x7F {
        return 1;
    } else if wint < table[0].first || wint > table[max as usize].last {
        return -1;
    }
    while max >= min {
        let mid = ((min + max) / 2) as usize;
        if wint > table[mid].last {
            min = (mid as c_int) + 1;
        } else if wint < table[mid].first {
            max = (mid as c_int) - 1;
        } else {
            let loc = if locale >= 0 && (locale as usize) < MB_SIZE {
                locale as usize
            } else {
                0
            };
            return table[mid].mb[loc] as c_int;
        }
    }
    -1
}

/// Binary search in toupper/tolower tables.
///
/// Returns the mapped character if found, or -1 if not found.
/// `max` is 1-based (number of elements in the table).
fn tlsearch(wint: c_int, table: &[pair], max: c_int) -> c_int {
    let mut min: c_int = 0;
    let mut max = max - 1;

    if table.is_empty() {
        return -1;
    }

    if wint < table[0].from || wint > table[max as usize].from {
        return -1;
    }
    while max >= min {
        let mid = ((min + max) / 2) as usize;
        if wint > table[mid].from {
            min = (mid as c_int) + 1;
        } else if wint < table[mid].from {
            max = (mid as c_int) - 1;
        } else {
            return table[mid].to;
        }
    }
    -1
}

// -----------------------------------------------------------------------
// CJK locale detection
// -----------------------------------------------------------------------

/// Detect CJK locale from the current process locale.
///
/// In the C version this calls `setlocale(LC_CTYPE, NULL)` to get the
/// current locale string, uppercases it, and matches against the
/// `cjk_locale_name` table.
///
/// In this Rust port, we return 0 (MB_Default) as a standalone function.
/// A full implementation would need to query the system locale.
fn get_locale_id() -> c_int {
    // TODO: In a full implementation, this would:
    // 1. Get the current locale via std::env or platform-specific API
    // 2. Uppercase the locale string
    // 3. Match against cjk_locale_name table
    //
    // For now, return MB_Default (0).
    MB_Default
}

// -----------------------------------------------------------------------
// wctype helper functions (static in C, used by Ri18n_iswctype)
// -----------------------------------------------------------------------

fn Ri18n_iswupper(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_wupper, table_wupper_count as c_int)
}

fn Ri18n_iswlower(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_wlower, table_wlower_count as c_int)
}

fn Ri18n_iswalpha(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_walpha, table_walpha_count as c_int)
}

fn Ri18n_iswdigit(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_wdigit, table_wdigit_count as c_int)
}

fn Ri18n_iswxdigit(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_wxdigit, table_wxdigit_count as c_int)
}

fn Ri18n_iswspace(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_wspace, table_wspace_count as c_int)
}

fn Ri18n_iswprint(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_wprint, table_wprint_count as c_int)
}

fn Ri18n_iswblank(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_wblank, table_wblank_count as c_int)
}

fn Ri18n_iswcntrl(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_wcntrl, table_wcntrl_count as c_int)
}

fn Ri18n_iswpunct(wc: wint_t) -> c_int {
    wcsearch(wc as c_int, table_wpunct, table_wpunct_count as c_int)
}

/// Internal helper: map a Rust &str to a wctype descriptor.
fn Ri18n_wctype_str(name: &str) -> c_uint {
    for entry in Ri18n_wctype_func.iter() {
        if entry.name == name {
            return entry.wctype;
        }
    }
    0
}

/// Internal helper: test whether a wide character belongs to a character class.
fn Ri18n_iswctype_internal(wc: wint_t, desc: c_uint) -> c_int {
    if desc == 0 {
        return 0;
    }
    for entry in Ri18n_wctype_func.iter() {
        if entry.wctype == desc {
            return unsafe { (entry.func)(wc) };
        }
    }
    0
}

/// Derived: iswalnum = iswdigit || iswalpha
fn Ri18n_iswalnum(wc: wint_t) -> c_int {
    if Ri18n_iswctype_internal(wc, Ri18n_wctype_str("digit")) != 0 {
        return 1;
    }
    if Ri18n_iswctype_internal(wc, Ri18n_wctype_str("alpha")) != 0 {
        return 1;
    }
    0
}

/// Derived: iswgraph = iswprint && !iswspace
fn Ri18n_iswgraph(wc: wint_t) -> c_int {
    if Ri18n_iswctype_internal(wc, Ri18n_wctype_str("print")) != 0
        && Ri18n_iswctype_internal(wc, Ri18n_wctype_str("space")) == 0
    {
        return 1;
    }
    0
}

// -----------------------------------------------------------------------
// wctype dispatch table
// -----------------------------------------------------------------------

type IswFunc = unsafe fn(wint_t) -> c_int;

struct Ri18n_wctype_func_l {
    name: &'static str,
    wctype: c_uint,
    func: IswFunc,
}

static Ri18n_wctype_func: &[Ri18n_wctype_func_l] = &[
    Ri18n_wctype_func_l {
        name: "upper",
        wctype: 1 << 0,
        func: Ri18n_iswupper,
    },
    Ri18n_wctype_func_l {
        name: "lower",
        wctype: 1 << 1,
        func: Ri18n_iswlower,
    },
    Ri18n_wctype_func_l {
        name: "alpha",
        wctype: 1 << 2,
        func: Ri18n_iswalpha,
    },
    Ri18n_wctype_func_l {
        name: "digit",
        wctype: 1 << 3,
        func: Ri18n_iswdigit,
    },
    Ri18n_wctype_func_l {
        name: "xdigit",
        wctype: 1 << 4,
        func: Ri18n_iswxdigit,
    },
    Ri18n_wctype_func_l {
        name: "space",
        wctype: 1 << 5,
        func: Ri18n_iswspace,
    },
    Ri18n_wctype_func_l {
        name: "print",
        wctype: 1 << 6,
        func: Ri18n_iswprint,
    },
    Ri18n_wctype_func_l {
        name: "graph",
        wctype: 1 << 7,
        func: Ri18n_iswgraph,
    },
    Ri18n_wctype_func_l {
        name: "blank",
        wctype: 1 << 8,
        func: Ri18n_iswblank,
    },
    Ri18n_wctype_func_l {
        name: "cntrl",
        wctype: 1 << 9,
        func: Ri18n_iswcntrl,
    },
    Ri18n_wctype_func_l {
        name: "punct",
        wctype: 1 << 10,
        func: Ri18n_iswpunct,
    },
    Ri18n_wctype_func_l {
        name: "alnum",
        wctype: 1 << 11,
        func: Ri18n_iswalnum,
    },
];

// -----------------------------------------------------------------------
// Public API: width functions
// -----------------------------------------------------------------------

/// Return the display width of a Unicode code point.
///
/// This is a replacement for `wcwidth()` that takes CJK locale into account.
/// Unlike the POSIX description, this does not return -1 for non-printable
/// Unicode points; unknown characters are assumed to have width one.
///
/// # Safety
/// The caller must ensure `c` is a valid Unicode code point.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Ri18n_wcwidth(c: R_wchar_t) -> c_int {
    // This quickly gives one for printing ASCII characters
    if c > 0x1F && c < 0x7F {
        return 1;
    }

    // Cache the locale id (in C this uses a static variable)
    let lc = get_locale_id();

    let wd = wcwidthsearch(c, table_wcwidth, table_wcwidth_count as c_int, lc);
    if wd >= 0 {
        return wd; // currently all are 1 or 2.
    }
    let zw = wcsearch(c, zero_width, zero_width_count as c_int);
    if zw != 0 { 0 } else { 1 } // assume unknown chars are width one.
}

/// Return the display width of a wide-character string.
///
/// This is a replacement for `wcswidth()`. Strings in R are restricted
/// to 2^31-1 bytes but could conceivably have a width exceeding that.
///
/// Unlike the POSIX description, this does not return -1 for strings
/// containing non-printable Unicode points.
///
/// # Safety
/// `wc` must point to a valid null-terminated wide-character string,
/// or `n` must specify a valid length. The caller must ensure the
/// pointer is valid for reads up to `n` elements or until a null terminator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Ri18n_wcswidth(wc: *const c_int, n: usize) -> c_int {
    unsafe {
        if wc.is_null() {
            return 0;
        }

        let mut rs: c_int = 0;
        let mut remaining = n;
        let mut idx = 0usize;

        while remaining > 0 {
            let ch = *wc.add(idx);
            if ch == 0 {
                break;
            }

            if idx + 1 < n {
                let next_ch = *wc.add(idx + 1);
                if IS_SURROGATE_PAIR(ch as wint_t, next_ch as wint_t) {
                    // Surrogate pairs should only occur with 'short' wchar_t,
                    // that is Windows and perhaps 32-bit AIX.
                    let val: R_wchar_t = ((ch & 0x3FF) << 10) + (next_ch & 0x3FF) + 0x010000;
                    let now = Ri18n_wcwidth(val);
                    if now == -1 {
                        return -1;
                    }
                    rs += now;
                    idx += 2;
                    remaining -= 2;
                    continue;
                }
            }

            let now = Ri18n_wcwidth(ch);
            if now == -1 {
                return -1;
            }
            rs += now;
            idx += 1;
            remaining -= 1;
        }

        rs
    }
}

// -----------------------------------------------------------------------
// Public API: case conversion
// -----------------------------------------------------------------------

/// Convert a wide character to uppercase.
///
/// If `wc` has a defined uppercase mapping, returns that mapping.
/// Otherwise returns `wc` unchanged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Ri18n_towupper(wc: R_wchar_t) -> R_wchar_t {
    let res = tlsearch(wc, table_toupper, table_toupper.len() as c_int);
    if res >= 0 { res } else { wc }
}

/// Convert a wide character to lowercase.
///
/// If `wc` has a defined lowercase mapping, returns that mapping.
/// Otherwise returns `wc` unchanged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Ri18n_towlower(wc: R_wchar_t) -> R_wchar_t {
    let res = tlsearch(wc, table_tolower, table_tolower.len() as c_int);
    if res >= 0 { res } else { wc }
}

// -----------------------------------------------------------------------
// Public API: wctype / iswctype
// -----------------------------------------------------------------------

/// Map a character class name to a wctype descriptor.
///
/// Recognized names: "upper", "lower", "alpha", "digit", "xdigit",
/// "space", "print", "graph", "blank", "cntrl", "punct", "alnum".
///
/// Returns 0 if the name is not recognized.
///
/// # Safety
/// `name` must point to a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Ri18n_wctype(name: *const u8) -> c_uint {
    unsafe {
        if name.is_null() {
            return 0;
        }

        // Convert C string to Rust &str
        let c_name = std::ffi::CStr::from_ptr(name as *const i8);
        let name_str = match c_name.to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };

        for entry in Ri18n_wctype_func.iter() {
            if entry.name == name_str {
                return entry.wctype;
            }
        }
        0
    }
}

/// Test whether a wide character belongs to a character class.
///
/// `desc` should be a value returned by `Ri18n_wctype`.
/// Returns non-zero if the character matches, 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Ri18n_iswctype(wc: wint_t, desc: c_uint) -> c_int {
    Ri18n_iswctype_internal(wc, desc)
}

// -----------------------------------------------------------------------
// Internal (non-FFI) helper: get locale ID as a public function for testing
// -----------------------------------------------------------------------

/// Standalone CJK locale detection function.
///
/// Returns the locale ID constant (MB_Default, MB_ja_JP, etc.) for the
/// current process locale. Currently returns MB_Default as a stub;
/// a full implementation would query the system locale.
#[unsafe(no_mangle)]
pub extern "C" fn get_locale_id_c() -> c_int {
    get_locale_id()
}
