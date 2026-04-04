#![allow(unreachable_code, clippy::comparison_to_empty, clippy::manual_memcpy)]
#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_camel_case_types)]
/*
 * Rust port of R's timezone library (src/extra/tzone/localtime.c).
 *
 * Original C code:
 *   Modifications copyright (C) 2007-2023 The R Core Team (GPL)
 *   Base tzcode: public domain by Arthur David Olson
 *
 * This port faithfully reproduces the C implementation in idiomatic Rust,
 * providing FFI-compatible entry points for R's datetime functionality.
 */

use std::env;
use std::fs::File;
use std::io::Read;
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Constants from tzfile.h
// ---------------------------------------------------------------------------

pub const TZ_MAX_TIMES: usize = 1200;
pub const TZ_MAX_TYPES: usize = 256;
pub const TZ_MAX_CHARS: usize = 100;
pub const TZ_MAX_LEAPS: usize = 50;

pub const SECSPERMIN: i32 = 60;
pub const MINSPERHOUR: i32 = 60;
pub const HOURSPERDAY: i32 = 24;
pub const DAYSPERWEEK: i32 = 7;
pub const DAYSPERNYEAR: i32 = 365;
pub const DAYSPERLYEAR: i32 = 366;
pub const SECSPERHOUR: i32 = SECSPERMIN * MINSPERHOUR;
pub const SECSPERDAY: i64 = (SECSPERHOUR as i64) * (HOURSPERDAY as i64);
pub const MONSPERYEAR: usize = 12;

pub const TM_SUNDAY: i32 = 0;
pub const TM_MONDAY: i32 = 1;
pub const TM_TUESDAY: i32 = 2;
pub const TM_WEDNESDAY: i32 = 3;
pub const TM_THURSDAY: i32 = 4;
pub const TM_FRIDAY: i32 = 5;
pub const TM_SATURDAY: i32 = 6;

pub const TM_JANUARY: i32 = 0;
pub const TM_FEBRUARY: i32 = 1;
pub const TM_MARCH: i32 = 2;
pub const TM_APRIL: i32 = 3;
pub const TM_MAY: i32 = 4;
pub const TM_JUNE: i32 = 5;
pub const TM_JULY: i32 = 6;
pub const TM_AUGUST: i32 = 7;
pub const TM_SEPTEMBER: i32 = 8;
pub const TM_OCTOBER: i32 = 9;
pub const TM_NOVEMBER: i32 = 10;
pub const TM_DECEMBER: i32 = 11;

pub const TM_YEAR_BASE: i32 = 1900;
pub const EPOCH_YEAR: i32 = 1970;
pub const EPOCH_WDAY: i32 = TM_THURSDAY;

pub const TZDIR: &str = "/usr/local/etc/zoneinfo";
pub const TZDEFAULT: &str = "UTC";
pub const TZDEFRULES: &str = "America/New_York";

pub const YEARSPERREPEAT: i32 = 400;
pub const AVGSECSPERYEAR: i64 = 31556952;
pub const SECSPERREPEAT: i64 = (YEARSPERREPEAT as i64) * AVGSECSPERYEAR;
pub const SECSPERREPEAT_BITS: i32 = 34;

pub const TZ_ABBR_MAX_LEN: usize = 16;
pub const TZ_STRLEN_MAX: usize = 255;
pub const MY_TZNAME_MAX: usize = 255;

pub const GRANDPARENTED: &str = "Local time zone must be set--see zic manual page";
pub const TZDEFRULESTRING: &str = ",M4.1.0,M10.5.0";

pub const TZ_MAGIC: &[u8; 4] = b"TZif";

pub const JULIAN_DAY: i32 = 0;
pub const DAY_OF_YEAR: i32 = 1;
pub const MONTH_NTH_DAY_OF_WEEK: i32 = 2;

pub const WRONG: i64 = -1;
pub const EOVERFLOW_VAL: i32 = 79;

// ---------------------------------------------------------------------------
// Types from datetime.h
// ---------------------------------------------------------------------------

/// Mirrors R's `struct Rtm` / `stm` from datetime.h.
/// All fields are `i32` to match C `int`, except `tm_gmtoff` which is `i64`
/// (matching C `long` / `R_time_t` semantics in the 64-bit R build).
///
/// For FFI consumers, this struct is #[repr(C)] and layout-compatible with
/// the C `struct Rtm`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
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

// We need Default for internal use.
impl Default for stm {
    fn default() -> Self {
        stm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            tm_gmtoff: 0,
            tm_zone: std::ptr::null(),
        }
    }
}

// Safety: stm is #[repr(C)] and only used behind a Mutex in TzGlobals.
// The raw pointer tm_zone points into the chars buffer of a state struct
// that lives as long as the Mutex guard is held.
unsafe impl Send for stm {}

pub type R_time_t = i64;

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

/// Time type information (mirrors C `struct ttinfo`).
#[derive(Clone, Copy, Debug, Default)]
struct ttinfo {
    tt_gmtoff: i32,  // UT offset in seconds
    tt_isdst: i32,   // used to set tm_isdst
    tt_abbrind: i32, // abbreviation list index
    tt_ttisstd: i32, // TRUE if transition is std time
    tt_ttisgmt: i32, // TRUE if transition is UT
}

/// Leap second information (mirrors C `struct lsinfo`).
#[derive(Clone, Copy, Debug, Default)]
struct lsinfo {
    ls_trans: i64, // transition time
    ls_corr: i64,  // correction to apply
}

/// Rule (mirrors C `struct rule`).
#[derive(Clone, Copy, Debug, Default)]
struct rule {
    r_type: i32, // type of rule
    r_day: i32,  // day number of rule
    r_week: i32, // week number of rule
    r_mon: i32,  // month number of rule
    r_time: i32, // transition time of rule
}

/// The `chars` buffer size in `state`.
/// Matches the C expression:
///   BIGGEST(BIGGEST(TZ_MAX_CHARS + 1, sizeof gmt), (2 * (MY_TZNAME_MAX + 1)))
/// sizeof "GMT" = 4 (including NUL), 2 * 256 = 512.
const CHARS_SIZE: usize = 512;

/// State (mirrors C `struct state`).
#[derive(Clone, Debug)]
struct state {
    leapcnt: i32,
    timecnt: i32,
    typecnt: i32,
    charcnt: i32,
    goback: i32,
    goahead: i32,
    ats: [i64; TZ_MAX_TIMES],
    types: [u8; TZ_MAX_TIMES],
    ttis: [ttinfo; TZ_MAX_TYPES],
    chars: [u8; CHARS_SIZE],
    lsis: [lsinfo; TZ_MAX_LEAPS],
    defaulttype: i32,
}

impl Default for state {
    fn default() -> Self {
        state {
            leapcnt: 0,
            timecnt: 0,
            typecnt: 0,
            charcnt: 0,
            goback: 0,
            goahead: 0,
            ats: [0i64; TZ_MAX_TIMES],
            types: [0u8; TZ_MAX_TIMES],
            ttis: [ttinfo::default(); TZ_MAX_TYPES],
            chars: [0u8; CHARS_SIZE],
            lsis: [lsinfo::default(); TZ_MAX_LEAPS],
            defaulttype: 0,
        }
    }
}

/// tzfile header (mirrors C `struct tzhead`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct tzhead {
    tzh_magic: [u8; 4],
    tzh_version: [u8; 1],
    tzh_reserved: [u8; 15],
    tzh_ttisgmtcnt: [u8; 4],
    tzh_ttisstdcnt: [u8; 4],
    tzh_leapcnt: [u8; 4],
    tzh_timecnt: [u8; 4],
    tzh_typecnt: [u8; 4],
    tzh_charcnt: [u8; 4],
}

// ---------------------------------------------------------------------------
// Static state -- protected by a global Mutex for thread safety.
//
// The C code uses file-scope statics.  We bundle them together behind a
// single Mutex to avoid deadlocks from lock ordering issues.
// ---------------------------------------------------------------------------

struct TzGlobals {
    lclmem: state,
    gmtmem: state,
    lcl_TZname: [u8; TZ_STRLEN_MAX + 1],
    lcl_is_set: i32,
    gmt_is_set: i32,
    tm: stm,
    // We store zone abbreviation strings in a separate buffer so that
    // the char slices within `state.chars` remain valid pointers.
    // In C, chars[] is a mutable array inside the struct, and pointers
    // into it are used as `*const char`. We emulate this by keeping
    // the chars arrays in the state structs and returning raw pointers
    // to them via `as_ptr()`.
}

impl Default for TzGlobals {
    fn default() -> Self {
        let mut g = TzGlobals {
            lclmem: state::default(),
            gmtmem: state::default(),
            lcl_TZname: [0u8; TZ_STRLEN_MAX + 1],
            lcl_is_set: 0,
            gmt_is_set: 0,
            tm: stm::default(),
        };
        // Initialize gmtmem with "GMT"
        let gmt_bytes = b"GMT";
        g.gmtmem.chars[..gmt_bytes.len()].copy_from_slice(gmt_bytes);
        g.gmtmem.charcnt = 3;
        g
    }
}

static TZ_GLOBALS: OnceLock<Mutex<TzGlobals>> = OnceLock::new();

fn get_tz_globals() -> &'static Mutex<TzGlobals> {
    TZ_GLOBALS.get_or_init(|| Mutex::new(TzGlobals::default()))
}

// Wild abbreviation (three spaces).
static WILDABBR: &[u8] = b"   ";

/// Exposed as `R_tzname` -- a pair of raw pointers to C strings.
/// The C code declares: `extern char *R_tzname[2];`
static mut R_TZNAME: [*mut i8; 2] = [std::ptr::null_mut(); 2];
static mut TZNAME_BUF0: [u8; TZ_MAX_CHARS + 1] = [0u8; TZ_MAX_CHARS + 1];
static mut TZNAME_BUF1: [u8; TZ_MAX_CHARS + 1] = [0u8; TZ_MAX_CHARS + 1];

// ---------------------------------------------------------------------------
// Helper macros / inline functions
// ---------------------------------------------------------------------------

#[inline(always)]
fn isleap(y: i32) -> bool {
    (y % 4 == 0) && ((y % 100 != 0) || (y % 400 == 0))
}

#[inline(always)]
fn is_digit(c: u8) -> bool {
    c.wrapping_sub(b'0') <= 9
}

/// Compute min/max for i64 (time_t).
#[inline(always)]
fn time_t_min() -> i64 {
    i64::MIN
}
#[inline(always)]
fn time_t_max() -> i64 {
    i64::MAX
}

// ---------------------------------------------------------------------------
// Month/year length tables
// ---------------------------------------------------------------------------

static MON_LENGTHS: [[i32; MONSPERYEAR]; 2] = [
    [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
    [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
];

static YEAR_LENGTHS: [i32; 2] = [DAYSPERNYEAR, DAYSPERLYEAR];

// ---------------------------------------------------------------------------
// Decode timezone file codes
// ---------------------------------------------------------------------------

fn detzcode(codep: &[u8]) -> i32 {
    let mut result: i32 = (codep[0] & 0x7f) as i32;
    for i in 1..4 {
        result = (result << 8) | (codep[i] & 0xff) as i32;
    }
    if (codep[0] & 0x80) != 0 {
        // Two's complement negation
        let minval: i32 = i32::MIN;
        result = if result != 0 { result - 1 } else { result };
        result = result.wrapping_add(minval);
    }
    result
}

fn detzcode64(codep: &[u8]) -> i64 {
    let mut result: i64 = (codep[0] & 0x7f) as i64;
    for i in 1..8 {
        result = (result << 8) | (codep[i] & 0xff) as i64;
    }
    if (codep[0] & 0x80) != 0 {
        let minval: i64 = i64::MIN;
        result = if result != 0 { result - 1 } else { result };
        result = result.wrapping_add(minval);
    }
    result
}

// ---------------------------------------------------------------------------
// Overflow-safe arithmetic
// ---------------------------------------------------------------------------

/// Returns true if overflow occurred.
fn increment_overflow(ip: &mut i32, j: i32) -> bool {
    let i = *ip;
    if (i >= 0 && j > i32::MAX - i) || (i < 0 && j < i32::MIN - i) {
        return true;
    }
    *ip += j;
    false
}

fn increment_overflow32(lp: &mut i32, m: i32) -> bool {
    let l = *lp;
    if (l >= 0 && m > i32::MAX - l) || (l < 0 && m < i32::MIN - l) {
        return true;
    }
    *lp += m;
    false
}

fn increment_overflow_time(tp: &mut i64, j: i64) -> bool {
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

fn normalize_overflow(tensptr: &mut i32, unitsptr: &mut i32, base: i32) -> bool {
    let tensdelta = if *unitsptr >= 0 {
        *unitsptr / base
    } else {
        -1 - (-1 - *unitsptr) / base
    };
    *unitsptr -= tensdelta * base;
    increment_overflow(tensptr, tensdelta)
}

fn normalize_overflow32(tensptr: &mut i32, unitsptr: &mut i32, base: i32) -> bool {
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

/// Stub for `getTZinfo()` from R's main/util.c.
/// Tries /etc/localtime symlink resolution, or returns "UTC".
fn get_tzinfo() -> String {
    // Try to resolve /etc/localtime
    if let Ok(target) = std::fs::read_link("/etc/localtime")
        && let Some(name) = target.to_str()
    {
        // The target is typically something like "/usr/share/zoneinfo/America/New_York"
        if let Some(pos) = name.rfind("zoneinfo/") {
            return name[pos + 9..].to_string();
        }
    }
    "UTC".to_string()
}

/// Emits an R-style warning to stderr.
fn rf_warning(msg: &str) {
    eprintln!("Warning: {}", msg);
}

// ---------------------------------------------------------------------------
// differ_by_repeat
// ---------------------------------------------------------------------------

fn differ_by_repeat(t1: i64, t0: i64) -> bool {
    // TYPE_BIT(i64) - TYPE_SIGNED(i64) = 64 - 1 = 63 >= 34 (SECSPERREPEAT_BITS)
    t1 - t0 == SECSPERREPEAT
}

// ---------------------------------------------------------------------------
// typesequiv
// ---------------------------------------------------------------------------

fn typesequiv(sp: &state, a: i32, b: i32) -> bool {
    if a < 0 || a >= sp.typecnt || b < 0 || b >= sp.typecnt {
        return false;
    }
    let ap = &sp.ttis[a as usize];
    let bp = &sp.ttis[b as usize];
    if ap.tt_gmtoff != bp.tt_gmtoff
        || ap.tt_isdst != bp.tt_isdst
        || ap.tt_ttisstd != bp.tt_ttisstd
        || ap.tt_ttisgmt != bp.tt_ttisgmt
    {
        return false;
    }
    // Compare abbreviation strings in chars
    let abbr_a = get_abbr(sp, ap.tt_abbrind as usize);
    let abbr_b = get_abbr(sp, bp.tt_abbrind as usize);
    abbr_a == abbr_b
}

/// Get a NUL-terminated abbreviation string from `state.chars` starting at `ind`.
fn get_abbr<'a>(sp: &'a state, ind: usize) -> &'a [u8] {
    let end = sp.charcnt as usize;
    if ind >= end {
        return b"";
    }
    let slice = &sp.chars[ind..end];
    match slice.iter().position(|&c| c == 0) {
        Some(pos) => &slice[..pos],
        None => slice,
    }
}

// ---------------------------------------------------------------------------
// settzname
// ---------------------------------------------------------------------------

fn settzname(g: &mut TzGlobals) {
    let sp = &mut g.lclmem;

    // Copy wildabbr into the static tzname buffers
    unsafe {
        let buf0 = std::ptr::addr_of_mut!(TZNAME_BUF1);
        let buf1 = std::ptr::addr_of_mut!(TZNAME_BUF1);
        for i in 0..TZ_MAX_CHARS + 1 {
            (*buf0)[i] = 0;
        }
        for i in 0..WILDABBR.len() {
            (*buf0)[i] = WILDABBR[i];
        }
        for i in 0..TZ_MAX_CHARS + 1 {
            (*buf1)[i] = 0;
        }
        for i in 0..WILDABBR.len() {
            (*buf1)[i] = WILDABBR[i];
        }
    }

    // Get the latest zone names
    for i in 0..sp.typecnt as usize {
        let ttisp = &sp.ttis[i];
        let isdst = ttisp.tt_isdst;
        let abbr = get_abbr(sp, ttisp.tt_abbrind as usize);
        if isdst != 0 {
            copy_to_tzname_buf(1, abbr);
        } else {
            copy_to_tzname_buf(0, abbr);
        }
    }
    for i in 0..sp.timecnt as usize {
        let ttisp = &sp.ttis[sp.types[i] as usize];
        let isdst = ttisp.tt_isdst;
        let abbr = get_abbr(sp, ttisp.tt_abbrind as usize);
        if isdst != 0 {
            copy_to_tzname_buf(1, abbr);
        } else {
            copy_to_tzname_buf(0, abbr);
        }
    }

    // Scrub abbreviations: replace bogus characters
    let valid_set = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 :+-._";
    for i in 0..sp.charcnt as usize {
        let c = sp.chars[i];
        if !valid_set.contains(&c) {
            sp.chars[i] = b'_';
        }
    }

    // Truncate long abbreviations
    for i in 0..sp.typecnt as usize {
        let ttisp = &sp.ttis[i];
        let abbr_start = ttisp.tt_abbrind as usize;
        let abbr = get_abbr(sp, abbr_start);
        if abbr.len() > TZ_ABBR_MAX_LEN {
            // Check if it's the GRANDPARENTED string
            let is_gp = std::str::from_utf8(abbr)
                .map(|s| s == GRANDPARENTED)
                .unwrap_or(false);
            if !is_gp {
                sp.chars[abbr_start + TZ_ABBR_MAX_LEN] = 0;
            }
        }
    }

    // Update the raw pointers
    unsafe {
        let rtz = std::ptr::addr_of_mut!(R_TZNAME);
        let buf0 = std::ptr::addr_of_mut!(TZNAME_BUF0);
        let buf1 = std::ptr::addr_of_mut!(TZNAME_BUF1);
        (*rtz)[0] = (*buf0).as_mut_ptr() as *mut i8;
        (*rtz)[1] = (*buf1).as_mut_ptr() as *mut i8;
    }
}

fn copy_to_tzname_buf(which: usize, abbr: &[u8]) {
    let buf = if which == 0 {
        std::ptr::addr_of_mut!(TZNAME_BUF0)
    } else {
        std::ptr::addr_of_mut!(TZNAME_BUF1)
    };
    let len = abbr.len().min(TZ_MAX_CHARS);
    unsafe {
        for i in 0..len {
            (*buf)[i] = abbr[i];
        }
        (*buf)[len] = 0;
    }
}

// ---------------------------------------------------------------------------
// tzload
// ---------------------------------------------------------------------------

fn tzload(name: Option<&str>, sp: &mut state, doextend: bool) -> i32 {
    sp.goback = 0;
    sp.goahead = 0;

    let effective_name: String;
    let sname: String;
    let name_ref: &str;

    match name {
        Some(n) => {
            effective_name = n.to_string();
            sname = effective_name.clone();
            name_ref = &effective_name;
        }
        None => {
            let tz_info = get_tzinfo();
            if tz_info == "unknown" {
                effective_name = TZDEFAULT.to_string();
            } else {
                effective_name = tz_info;
            }
            sname = effective_name.clone();
            name_ref = &effective_name;
        }
    }

    // Strip leading ':'
    let name_stripped = if name_ref.starts_with(':') {
        &name_ref[1..]
    } else {
        name_ref
    };

    // Build the full path
    let fullname: String;
    let name_to_use: &str;

    if name_stripped.starts_with('/') {
        name_to_use = name_stripped;
        fullname = name_stripped.to_string();
    } else {
        // Look in TZDIR, or R_SHARE_DIR/zoneinfo, or R_HOME/share/zoneinfo
        let tzdir = env::var("TZDIR").ok();
        let zoneinfo_path = if let Some(ref p) = tzdir {
            if p == "internal" || p.is_empty() {
                // Try R_SHARE_DIR/zoneinfo
                if let Ok(r_share) = env::var("R_SHARE_DIR") {
                    format!("{}/zoneinfo", r_share)
                } else if let Ok(r_home) = env::var("R_HOME") {
                    format!("{}/share/zoneinfo", r_home)
                } else {
                    TZDIR.to_string()
                }
            } else {
                p.clone()
            }
        } else {
            TZDIR.to_string()
        };

        fullname = format!("{}/{}", zoneinfo_path, name_stripped);
        name_to_use = &fullname;
    }

    // Check access / try to open
    let mut file = match File::open(name_to_use) {
        Ok(f) => f,
        Err(_) => {
            rf_warning(&format!("unknown timezone '{}'", sname));
            return -1;
        }
    };

    // Read the file content
    // C code uses a union buffer of size:
    //   2 * sizeof(tzhead) + 2 * sizeof(state) + 4 * TZ_MAX_TIMES
    // We'll just read the whole file into a Vec.
    let mut data = Vec::new();
    if file.read_to_end(&mut data).is_err() || data.is_empty() {
        rf_warning(&format!("unknown timezone '{}'", sname));
        return -1;
    }

    // Parse the file -- loop over stored sizes (4 then 8 bytes for v2+)
    let mut stored: usize = 4;
    let mut nread = data.len();

    loop {
        if data.len() < std::mem::size_of::<tzhead>() {
            return -1;
        }
        let head = unsafe { &*(data.as_ptr() as *const tzhead) };

        let ttisstdcnt = detzcode(&head.tzh_ttisstdcnt) as i32;
        let ttisgmtcnt = detzcode(&head.tzh_ttisgmtcnt) as i32;
        sp.leapcnt = detzcode(&head.tzh_leapcnt) as i32;
        sp.timecnt = detzcode(&head.tzh_timecnt) as i32;
        sp.typecnt = detzcode(&head.tzh_typecnt) as i32;
        sp.charcnt = detzcode(&head.tzh_charcnt) as i32;

        let mut p = std::mem::size_of::<tzhead>(); // points past the header

        if sp.leapcnt < 0
            || sp.leapcnt > TZ_MAX_LEAPS as i32
            || sp.typecnt <= 0
            || sp.typecnt > TZ_MAX_TYPES as i32
            || sp.timecnt < 0
            || sp.timecnt > TZ_MAX_TIMES as i32
            || sp.charcnt < 0
            || sp.charcnt > TZ_MAX_CHARS as i32
            || (ttisstdcnt != sp.typecnt && ttisstdcnt != 0)
            || (ttisgmtcnt != sp.typecnt && ttisgmtcnt != 0)
        {
            return -1;
        }

        // Check we have enough data
        let needed = (sp.timecnt as usize) * stored  // ats
            + (sp.timecnt as usize)                   // types
            + (sp.typecnt as usize) * 6               // ttinfos
            + (sp.charcnt as usize)                   // chars
            + (sp.leapcnt as usize) * (stored + 4)    // lsinfos
            + (ttisstdcnt as usize)                   // ttisstds
            + (ttisgmtcnt as usize); // ttisgmts

        if nread - p < needed {
            return -1;
        }

        // Read transition times
        let mut timecnt: usize = 0;
        for i in 0..sp.timecnt as usize {
            let at = if stored == 4 {
                detzcode(&data[p..p + 4]) as i64
            } else {
                detzcode64(&data[p..p + 8])
            };
            sp.types[i] = 1;
            if i > 0 && timecnt == 0 && at != i64::MIN {
                sp.types[i - 1] = 1;
                sp.ats[timecnt] = i64::MIN;
                timecnt += 1;
            }
            sp.ats[timecnt] = at;
            timecnt += 1;
            p += stored;
        }

        // Read types
        let mut timecnt2: usize = 0;
        for i in 0..sp.timecnt as usize {
            let typ = data[p];
            p += 1;
            if sp.typecnt as usize <= typ as usize {
                return -1;
            }
            if sp.types[i] != 0 {
                sp.types[timecnt2] = typ;
                timecnt2 += 1;
            }
        }
        sp.timecnt = timecnt2 as i32;

        // Read ttinfos
        for i in 0..sp.typecnt as usize {
            sp.ttis[i].tt_gmtoff = detzcode(&data[p..p + 4]);
            p += 4;
            sp.ttis[i].tt_isdst = data[p] as i32;
            p += 1;
            if sp.ttis[i].tt_isdst != 0 && sp.ttis[i].tt_isdst != 1 {
                return -1;
            }
            sp.ttis[i].tt_abbrind = data[p] as i32;
            p += 1;
            if sp.ttis[i].tt_abbrind < 0 || sp.ttis[i].tt_abbrind > sp.charcnt {
                return -1;
            }
        }

        // Read chars
        for i in 0..sp.charcnt as usize {
            sp.chars[i] = data[p];
            p += 1;
        }
        if (sp.charcnt as usize) < sp.chars.len() {
            sp.chars[sp.charcnt as usize] = 0; // ensure NUL
        }

        // Read leap second info
        for i in 0..sp.leapcnt as usize {
            sp.lsis[i].ls_trans = if stored == 4 {
                detzcode(&data[p..p + 4]) as i64
            } else {
                detzcode64(&data[p..p + 8])
            };
            p += stored;
            sp.lsis[i].ls_corr = detzcode(&data[p..p + 4]) as i64;
            p += 4;
        }

        // Read ttisstd
        for i in 0..sp.typecnt as usize {
            if ttisstdcnt == 0 {
                sp.ttis[i].tt_ttisstd = 0;
            } else {
                sp.ttis[i].tt_ttisstd = data[p] as i32;
                p += 1;
                if sp.ttis[i].tt_ttisstd != 0 && sp.ttis[i].tt_ttisstd != 1 {
                    return -1;
                }
            }
        }

        // Read ttisgmt
        for i in 0..sp.typecnt as usize {
            if ttisgmtcnt == 0 {
                sp.ttis[i].tt_ttisgmt = 0;
            } else {
                sp.ttis[i].tt_ttisgmt = data[p] as i32;
                p += 1;
                if sp.ttis[i].tt_ttisgmt != 0 && sp.ttis[i].tt_ttisgmt != 1 {
                    return -1;
                }
            }
        }

        // If this is an old file (version '\0'), we're done
        if head.tzh_version[0] == 0 {
            break;
        }

        // For version 2+, shift the data for the second pass
        nread -= p;
        // Copy remaining data to the front of the buffer
        for i in 0..nread {
            data[i] = data[p + i];
        }
        data.truncate(nread);

        // For signed 64-bit time_t, we're done after processing 8-byte entries
        // (stored >= sizeof(time_t) for time_t = i64)
        if stored >= 8 {
            break;
        }

        stored = 8;
    }

    // Handle POSIX TZ extension in the file
    if doextend && nread > 2 {
        // After the loop, data contains the remaining bytes after the last pass.
        // Re-read data from the current position.
        let remaining = &data[..nread];
        if remaining[0] == b'\n'
            && remaining[remaining.len() - 1] == b'\n'
            && (sp.typecnt as usize) + 2 <= TZ_MAX_TYPES
        {
            // NUL-terminate the POSIX string
            let tz_string = &remaining[1..remaining.len() - 1];
            let tz_str = match std::str::from_utf8(tz_string) {
                Ok(s) => s,
                Err(_) => return 0,
            };

            let mut ts = state::default();
            let result = tzparse(tz_str, &mut ts, false);
            if result == 0
                && ts.typecnt == 2
                && (sp.charcnt as usize) + (ts.charcnt as usize) <= TZ_MAX_CHARS
            {
                for i in 0..2 {
                    ts.ttis[i].tt_abbrind += sp.charcnt;
                }
                for i in 0..ts.charcnt as usize {
                    sp.chars[sp.charcnt as usize] = ts.chars[i];
                    sp.charcnt += 1;
                }
                let mut i = 0;
                while i < ts.timecnt as usize && ts.ats[i] <= sp.ats[sp.timecnt as usize - 1] {
                    i += 1;
                }
                while i < ts.timecnt as usize && (sp.timecnt as usize) < TZ_MAX_TIMES {
                    sp.ats[sp.timecnt as usize] = ts.ats[i];
                    sp.types[sp.timecnt as usize] = (sp.typecnt + ts.types[i] as i32) as u8;
                    sp.timecnt += 1;
                    i += 1;
                }
                sp.ttis[sp.typecnt as usize] = ts.ttis[0];
                sp.typecnt += 1;
                sp.ttis[sp.typecnt as usize] = ts.ttis[1];
                sp.typecnt += 1;
            }
        }
    }

    // Check for goback/goahead
    if sp.timecnt > 1 {
        let mut i = 1;
        while i < sp.timecnt as usize {
            if typesequiv(sp, sp.types[i] as i32, sp.types[0] as i32)
                && differ_by_repeat(sp.ats[i], sp.ats[0])
            {
                sp.goback = 1;
                break;
            }
            i += 1;
        }
        let mut i = sp.timecnt as usize - 2;
        loop {
            if typesequiv(
                sp,
                sp.types[sp.timecnt as usize - 1] as i32,
                sp.types[i] as i32,
            ) && differ_by_repeat(sp.ats[sp.timecnt as usize - 1], sp.ats[i])
            {
                sp.goahead = 1;
                break;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
    }

    // Find defaulttype
    let mut i = 0;
    while i < sp.typecnt as usize {
        if sp.types[i] == 0 {
            break;
        }
        i += 1;
    }
    let mut defaulttype = if i >= sp.typecnt as usize { 0 } else { -1 };

    // If the first transition is to DST, find the closest standard type
    if defaulttype < 0 && sp.timecnt > 0 && sp.ttis[sp.types[0] as usize].tt_isdst != 0 {
        let mut j = sp.types[0] as i32;
        while j >= 0 {
            if sp.ttis[j as usize].tt_isdst == 0 {
                break;
            }
            j -= 1;
        }
        defaulttype = j;
    }

    // If still no result, find first standard type
    if defaulttype < 0 {
        let mut j = 0;
        while j < sp.typecnt as usize {
            if sp.ttis[j].tt_isdst == 0 {
                break;
            }
            j += 1;
        }
        defaulttype = if j >= sp.typecnt as usize {
            0
        } else {
            j as i32
        };
    }

    sp.defaulttype = defaulttype;
    0
}

// ---------------------------------------------------------------------------
// TZ string parsing helpers
// ---------------------------------------------------------------------------

fn getzname(strp: &[u8]) -> usize {
    let mut i = 0;
    while i < strp.len() {
        let c = strp[i];
        if c == 0 || is_digit(c) || c == b',' || c == b'-' || c == b'+' {
            break;
        }
        i += 1;
    }
    i
}

fn getqzname(strp: &[u8], delim: u8) -> usize {
    let mut i = 0;
    while i < strp.len() && strp[i] != delim {
        i += 1;
    }
    i
}

/// Parse a number from a byte string. Returns (new_offset, number) or None on error.
fn getnum(strp: &[u8], min: i32, max: i32) -> Option<(usize, i32)> {
    if strp.is_empty() || !is_digit(strp[0]) {
        return None;
    }
    let mut num: i32 = 0;
    let mut i = 0;
    loop {
        let c = strp[i];
        if !is_digit(c) {
            break;
        }
        num = num * 10 + (c - b'0') as i32;
        if num > max {
            return None;
        }
        i += 1;
        if i >= strp.len() {
            break;
        }
    }
    if num < min {
        return None;
    }
    Some((i, num))
}

/// Parse seconds in hh[:mm[:ss]] form.
fn getsecs(strp: &[u8]) -> Option<(usize, i32)> {
    let (pos, num) = getnum(strp, 0, HOURSPERDAY * DAYSPERWEEK - 1)?;
    let mut secs = num * SECSPERHOUR;
    let mut i = pos;
    if i < strp.len() && strp[i] == b':' {
        i += 1;
        let (pos2, num2) = getnum(&strp[i..], 0, MINSPERHOUR - 1)?;
        secs += num2 * SECSPERMIN;
        i += pos2;
        if i < strp.len() && strp[i] == b':' {
            i += 1;
            let (pos3, num3) = getnum(&strp[i..], 0, SECSPERMIN)?;
            secs += num3;
            i += pos3;
        }
    }
    Some((i, secs))
}

/// Parse an offset in [+-]hh[:mm[:ss]] form.
fn getoffset(strp: &[u8]) -> Option<(usize, i32)> {
    let mut neg = false;
    let mut i = 0;
    if strp[i] == b'-' {
        neg = true;
        i += 1;
    } else if strp[i] == b'+' {
        i += 1;
    }
    let (pos, offset) = getsecs(&strp[i..])?;
    let result = if neg { -offset } else { offset };
    Some((i + pos, result))
}

/// Parse a rule in date[/time] form.
fn getrule(strp: &[u8]) -> Option<(usize, rule)> {
    let mut rp = rule::default();
    let mut i = 0;

    if strp[i] == b'J' {
        rp.r_type = JULIAN_DAY;
        i += 1;
        let (pos, day) = getnum(&strp[i..], 1, DAYSPERNYEAR)?;
        rp.r_day = day;
        i += pos;
    } else if strp[i] == b'M' {
        rp.r_type = MONTH_NTH_DAY_OF_WEEK;
        i += 1;
        let (pos, mon) = getnum(&strp[i..], 1, MONSPERYEAR as i32)?;
        rp.r_mon = mon;
        i += pos;
        if i >= strp.len() || strp[i] != b'.' {
            return None;
        }
        i += 1;
        let (pos, week) = getnum(&strp[i..], 1, 5)?;
        rp.r_week = week;
        i += pos;
        if i >= strp.len() || strp[i] != b'.' {
            return None;
        }
        i += 1;
        let (pos, day) = getnum(&strp[i..], 0, DAYSPERWEEK - 1)?;
        rp.r_day = day;
        i += pos;
    } else if i < strp.len() && is_digit(strp[i]) {
        rp.r_type = DAY_OF_YEAR;
        let (pos, day) = getnum(&strp[i..], 0, DAYSPERLYEAR - 1)?;
        rp.r_day = day;
        i += pos;
    } else {
        return None;
    }

    if i < strp.len() && strp[i] == b'/' {
        i += 1;
        let (pos, time) = getoffset(&strp[i..])?;
        rp.r_time = time;
        i += pos;
    } else {
        rp.r_time = 2 * SECSPERHOUR;
    }

    Some((i, rp))
}

// ---------------------------------------------------------------------------
// transtime
// ---------------------------------------------------------------------------

fn transtime(year: i32, rulep: &rule, offset: i32) -> i64 {
    let leapyear = isleap(year);
    let mut value: i64 = 0;
    let secperday_i32: i32 = (SECSPERHOUR * HOURSPERDAY) as i32;

    match rulep.r_type {
        JULIAN_DAY => {
            value = ((rulep.r_day - 1) * secperday_i32) as i64;
            if leapyear && rulep.r_day >= 60 {
                value += secperday_i32 as i64;
            }
        }
        DAY_OF_YEAR => {
            value = (rulep.r_day * secperday_i32) as i64;
        }
        MONTH_NTH_DAY_OF_WEEK => {
            let m1 = (rulep.r_mon + 9) % 12 + 1;
            let yy0 = if rulep.r_mon <= 2 { year - 1 } else { year };
            let yy1 = yy0 / 100;
            let yy2 = yy0 % 100;
            let mut dow = ((26 * m1 - 2) / 10 + 1 + yy2 + yy2 / 4 + yy1 / 4 - 2 * yy1) % 7;
            if dow < 0 {
                dow += DAYSPERWEEK;
            }

            let mut d = rulep.r_day - dow;
            if d < 0 {
                d += DAYSPERWEEK;
            }
            let mut week_i = 1;
            while week_i < rulep.r_week {
                if d + DAYSPERWEEK
                    >= MON_LENGTHS[if leapyear { 1 } else { 0 }][rulep.r_mon as usize - 1]
                {
                    break;
                }
                d += DAYSPERWEEK;
                week_i += 1;
            }

            value = (d * secperday_i32) as i64;
            for mi in 0..rulep.r_mon as usize - 1 {
                value += (MON_LENGTHS[if leapyear { 1 } else { 0 }][mi] * secperday_i32) as i64;
            }
        }
        _ => {}
    }

    value + (rulep.r_time + offset) as i64
}

// ---------------------------------------------------------------------------
// tzparse
// ---------------------------------------------------------------------------

fn tzparse(name: &str, sp: &mut state, lastditch: bool) -> i32 {
    let bytes = name.as_bytes();
    let mut pos = 0;

    let zttinfo = ttinfo::default();

    let (stdname_start, stdname_end);
    let stdoffset: i32;

    if lastditch {
        stdname_start = 0;
        stdname_end = bytes.len();
        pos = bytes.len();
        let stdlen = if bytes.len() >= sp.chars.len() {
            sp.chars.len() - 1
        } else {
            bytes.len()
        };
        // Copy std name
        for i in 0..stdlen {
            sp.chars[i] = bytes[i];
        }
        sp.chars[stdlen] = 0;
        stdoffset = 0;
    } else {
        if !bytes.is_empty() && bytes[pos] == b'<' {
            pos += 1;
            stdname_start = pos;
            let end = getqzname(&bytes[pos..], b'>');
            pos += end;
            if pos >= bytes.len() || bytes[pos] != b'>' {
                return -1;
            }
            stdname_end = pos;
            pos += 1;
        } else {
            stdname_start = pos;
            let end = getzname(&bytes[pos..]);
            pos += end;
            stdname_end = pos;
        }
        if pos >= bytes.len() || bytes[pos] == 0 {
            return -1;
        }
        let (off_pos, offset) = match getoffset(&bytes[pos..]) {
            Some(r) => r,
            None => return -1,
        };
        pos += off_pos;
        stdoffset = offset;
    }

    // Try to load TZDEFRULES for leap second info
    let load_result = tzload(Some(TZDEFRULES), sp, false);
    if load_result != 0 {
        sp.leapcnt = 0;
    }

    let mut dstname_start = 0;
    let mut dstname_end = 0;
    let mut dstlen: usize = 0;
    let mut dstoffset: i32 = stdoffset - SECSPERHOUR;

    if pos < bytes.len() && bytes[pos] != 0 {
        // Parse DST name
        if bytes[pos] == b'<' {
            pos += 1;
            dstname_start = pos;
            let end = getqzname(&bytes[pos..], b'>');
            pos += end;
            if pos >= bytes.len() || bytes[pos] != b'>' {
                return -1;
            }
            dstname_end = pos;
            pos += 1;
        } else {
            dstname_start = pos;
            let end = getzname(&bytes[pos..]);
            pos += end;
            dstname_end = pos;
        }
        dstlen = dstname_end - dstname_start;

        // Parse DST offset
        if pos < bytes.len() && bytes[pos] != 0 && bytes[pos] != b',' && bytes[pos] != b';' {
            let (off_pos, offset) = match getoffset(&bytes[pos..]) {
                Some(r) => r,
                None => return -1,
            };
            pos += off_pos;
            dstoffset = offset;
        }

        if pos >= bytes.len() || bytes[pos] == 0 {
            if pos >= bytes.len() && load_result != 0 {
                // Use TZDEFRULESTRING
                return tzparse_with_rules(
                    &bytes[stdname_start..stdname_end],
                    if dstlen > 0 {
                        Some(&bytes[dstname_start..dstname_end])
                    } else {
                        None
                    },
                    stdoffset,
                    dstoffset,
                    sp,
                    &zttinfo,
                );
            }
            if pos < bytes.len() && bytes[pos] == 0 {
                // No rules, just set type 0 (no DST)
                dstlen = 0;
                sp.typecnt = 1;
                sp.timecnt = 0;
                sp.ttis[0] = zttinfo;
                sp.ttis[0].tt_gmtoff = -stdoffset;
                sp.ttis[0].tt_isdst = 0;
                sp.ttis[0].tt_abbrind = 0;
            }
        }

        if pos < bytes.len() && (bytes[pos] == b',' || bytes[pos] == b';') {
            pos += 1;
            // Parse rules
            let (pos1, start) = match getrule(&bytes[pos..]) {
                Some(r) => r,
                None => return -1,
            };
            pos += pos1;
            if pos >= bytes.len() || bytes[pos] != b',' {
                return -1;
            }
            pos += 1;
            let (pos2, end) = match getrule(&bytes[pos..]) {
                Some(r) => r,
                None => return -1,
            };
            pos += pos2;
            if pos < bytes.len() && bytes[pos] != 0 {
                return -1;
            }

            // Build transition table
            sp.typecnt = 2;
            sp.ttis[0] = zttinfo;
            sp.ttis[1] = zttinfo;
            sp.ttis[0].tt_gmtoff = -dstoffset;
            sp.ttis[0].tt_isdst = 1;
            sp.ttis[0].tt_abbrind = (stdname_end - stdname_start + 1) as i32;
            sp.ttis[1].tt_gmtoff = -stdoffset;
            sp.ttis[1].tt_isdst = 0;
            sp.ttis[1].tt_abbrind = 0;

            let mut timecnt: usize = 0;
            let mut janfirst: i64 = 0;
            let mut yearlim = EPOCH_YEAR + YEARSPERREPEAT;

            let mut year = EPOCH_YEAR;
            while year < yearlim {
                let starttime = transtime(year, &start, stdoffset);
                let endtime = transtime(year, &end, dstoffset);
                let yearsecs = YEAR_LENGTHS[if isleap(year) { 1 } else { 0 }] as i64 * SECSPERDAY;
                let reversed = endtime < starttime;

                let (starttime, endtime) = if reversed {
                    (endtime, starttime)
                } else {
                    (starttime, endtime)
                };

                if reversed
                    || (starttime < endtime
                        && (endtime - starttime < yearsecs + (stdoffset - dstoffset) as i64))
                {
                    if TZ_MAX_TIMES - 2 < timecnt {
                        break;
                    }
                    yearlim = year + YEARSPERREPEAT + 1;

                    sp.ats[timecnt] = janfirst;
                    if increment_overflow_time(&mut sp.ats[timecnt], starttime) {
                        break;
                    }
                    sp.types[timecnt] = if reversed { 1 } else { 0 };
                    timecnt += 1;

                    sp.ats[timecnt] = janfirst;
                    if increment_overflow_time(&mut sp.ats[timecnt], endtime) {
                        break;
                    }
                    sp.types[timecnt] = if reversed { 0 } else { 1 };
                    timecnt += 1;
                }

                if increment_overflow_time(&mut janfirst, yearsecs) {
                    break;
                }
                year += 1;
            }
            sp.timecnt = timecnt as i32;
            if timecnt == 0 {
                sp.typecnt = 1;
            }
        } else if pos >= bytes.len() || bytes[pos] == 0 {
            // No DST rules after the DST offset -- adjust existing transitions
            let mut theirstdoffset: i32 = 0;
            for i in 0..sp.timecnt as usize {
                let j = sp.types[i] as usize;
                if sp.ttis[j].tt_isdst == 0 {
                    theirstdoffset = -sp.ttis[j].tt_gmtoff;
                    break;
                }
            }
            let mut theirdstoffset: i32 = 0;
            for i in 0..sp.timecnt as usize {
                let j = sp.types[i] as usize;
                if sp.ttis[j].tt_isdst != 0 {
                    theirdstoffset = -sp.ttis[j].tt_gmtoff;
                    break;
                }
            }

            let isdst = 0;
            let mut theiroffset = theirstdoffset;

            for i in 0..sp.timecnt as usize {
                let j = sp.types[i] as usize;
                sp.types[i] = if sp.ttis[j].tt_isdst != 0 { 1 } else { 0 };
                if sp.ttis[j].tt_ttisgmt == 0 {
                    if isdst != 0 && sp.ttis[j].tt_ttisstd == 0 {
                        sp.ats[i] += (dstoffset - theirdstoffset) as i64;
                    } else {
                        sp.ats[i] += (stdoffset - theirstdoffset) as i64;
                    }
                }
                theiroffset = -sp.ttis[j].tt_gmtoff;
                if sp.ttis[j].tt_isdst != 0 {
                    theirdstoffset = theiroffset;
                } else {
                    theirstdoffset = theiroffset;
                }
            }

            sp.ttis[0] = zttinfo;
            sp.ttis[1] = zttinfo;
            sp.ttis[0].tt_gmtoff = -stdoffset;
            sp.ttis[0].tt_isdst = 0;
            sp.ttis[0].tt_abbrind = 0;
            sp.ttis[1].tt_gmtoff = -dstoffset;
            sp.ttis[1].tt_isdst = 1;
            sp.ttis[1].tt_abbrind = (stdname_end - stdname_start + 1) as i32;
            sp.typecnt = 2;
        }
    } else {
        dstlen = 0;
        sp.typecnt = 1;
        sp.timecnt = 0;
        sp.ttis[0] = zttinfo;
        sp.ttis[0].tt_gmtoff = -stdoffset;
        sp.ttis[0].tt_isdst = 0;
        sp.ttis[0].tt_abbrind = 0;
    }

    // Copy names into chars
    let stdlen = stdname_end - stdname_start;
    sp.charcnt = (stdlen + 1) as i32;
    if dstlen != 0 {
        sp.charcnt += (dstlen + 1) as i32;
    }
    if sp.charcnt as usize > sp.chars.len() {
        return -1;
    }

    let mut cp = 0;
    for i in stdname_start..stdname_end {
        sp.chars[cp] = bytes[i];
        cp += 1;
    }
    sp.chars[cp] = 0;
    cp += 1;
    if dstlen != 0 {
        for i in dstname_start..dstname_end {
            sp.chars[cp] = bytes[i];
            cp += 1;
        }
        sp.chars[cp] = 0;
    }

    0
}

/// Helper for tzparse when TZDEFRULESTRING is used.
fn tzparse_with_rules(
    stdname: &[u8],
    dstname: Option<&[u8]>,
    stdoffset: i32,
    dstoffset: i32,
    sp: &mut state,
    zttinfo: &ttinfo,
) -> i32 {
    let rules = TZDEFRULESTRING.as_bytes();
    let mut pos = 1; // skip leading ','

    let (pos1, start) = match getrule(&rules[pos..]) {
        Some(r) => r,
        None => return -1,
    };
    pos += pos1;
    if pos >= rules.len() || rules[pos] != b',' {
        return -1;
    }
    pos += 1;
    let (pos2, end) = match getrule(&rules[pos..]) {
        Some(r) => r,
        None => return -1,
    };
    pos += pos2;

    let stdlen = stdname.len();
    let dstlen = dstname.map(|d| d.len()).unwrap_or(0);

    sp.typecnt = 2;
    sp.ttis[0] = *zttinfo;
    sp.ttis[1] = *zttinfo;
    sp.ttis[0].tt_gmtoff = -dstoffset;
    sp.ttis[0].tt_isdst = 1;
    sp.ttis[0].tt_abbrind = (stdlen + 1) as i32;
    sp.ttis[1].tt_gmtoff = -stdoffset;
    sp.ttis[1].tt_isdst = 0;
    sp.ttis[1].tt_abbrind = 0;

    let mut timecnt: usize = 0;
    let mut janfirst: i64 = 0;
    let mut yearlim = EPOCH_YEAR + YEARSPERREPEAT;

    let mut year = EPOCH_YEAR;
    while year < yearlim {
        let starttime = transtime(year, &start, stdoffset);
        let endtime = transtime(year, &end, dstoffset);
        let yearsecs = YEAR_LENGTHS[if isleap(year) { 1 } else { 0 }] as i64 * SECSPERDAY;
        let reversed = endtime < starttime;

        let (starttime, endtime) = if reversed {
            (endtime, starttime)
        } else {
            (starttime, endtime)
        };

        if reversed
            || (starttime < endtime
                && (endtime - starttime < yearsecs + (stdoffset - dstoffset) as i64))
        {
            if TZ_MAX_TIMES - 2 < timecnt {
                break;
            }
            yearlim = year + YEARSPERREPEAT + 1;

            sp.ats[timecnt] = janfirst;
            if increment_overflow_time(&mut sp.ats[timecnt], starttime) {
                break;
            }
            sp.types[timecnt] = if reversed { 1 } else { 0 };
            timecnt += 1;

            sp.ats[timecnt] = janfirst;
            if increment_overflow_time(&mut sp.ats[timecnt], endtime) {
                break;
            }
            sp.types[timecnt] = if reversed { 0 } else { 1 };
            timecnt += 1;
        }

        if increment_overflow_time(&mut janfirst, yearsecs) {
            break;
        }
        year += 1;
    }
    sp.timecnt = timecnt as i32;
    if timecnt == 0 {
        sp.typecnt = 1;
    }

    sp.charcnt = (stdlen + 1) as i32;
    if dstlen > 0 {
        sp.charcnt += (dstlen + 1) as i32;
    }
    if sp.charcnt as usize > sp.chars.len() {
        return -1;
    }

    let mut cp = 0;
    for i in 0..stdlen {
        sp.chars[cp] = stdname[i];
        cp += 1;
    }
    sp.chars[cp] = 0;
    cp += 1;
    if let Some(dn) = dstname {
        for i in 0..dstlen {
            sp.chars[cp] = dn[i];
            cp += 1;
        }
        sp.chars[cp] = 0;
    }

    0
}

// ---------------------------------------------------------------------------
// gmtload
// ---------------------------------------------------------------------------

fn gmtload(sp: &mut state) {
    if tzload(Some("GMT"), sp, true) != 0 {
        let _ = tzparse("GMT", sp, true);
    }
}

// ---------------------------------------------------------------------------
// leaps_thru_end_of
// ---------------------------------------------------------------------------

fn leaps_thru_end_of(y: i32) -> i32 {
    if y >= 0 {
        y / 4 - y / 100 + y / 400
    } else {
        -(leaps_thru_end_of(-(y + 1)) + 1)
    }
}

// ---------------------------------------------------------------------------
// timesub
// ---------------------------------------------------------------------------

fn timesub(timep: &i64, offset: i32, sp: &state, tmp: &mut stm) -> Option<*mut stm> {
    let t = *timep;

    // Leap second correction
    let mut corr: i64 = 0;
    let mut hit: i32 = 0;
    let mut i = sp.leapcnt;
    while i > 0 {
        i -= 1;
        let lp = &sp.lsis[i as usize];
        if t >= lp.ls_trans {
            if t == lp.ls_trans {
                hit = if (i == 0 && lp.ls_corr > 0)
                    || (i > 0 && lp.ls_corr > sp.lsis[(i - 1) as usize].ls_corr)
                {
                    1
                } else {
                    0
                };
                if hit != 0 {
                    while i > 0
                        && sp.lsis[i as usize].ls_trans == sp.lsis[(i - 1) as usize].ls_trans + 1
                        && sp.lsis[i as usize].ls_corr == sp.lsis[(i - 1) as usize].ls_corr + 1
                    {
                        hit += 1;
                        i -= 1;
                    }
                }
            }
            corr = lp.ls_corr;
            break;
        }
    }

    let mut y = EPOCH_YEAR;
    let mut tdays = t / SECSPERDAY;
    let mut rem = t - tdays * SECSPERDAY;

    while tdays < 0 || tdays >= YEAR_LENGTHS[if isleap(y) { 1 } else { 0 }] as i64 {
        let tdelta = tdays / DAYSPERLYEAR as i64;
        if !i32::try_from(tdelta).is_ok() {
            return None;
        }
        let mut idelta = tdelta as i32;
        if idelta == 0 {
            idelta = if tdays < 0 { -1 } else { 1 };
        }
        let mut newy = y;
        if increment_overflow(&mut newy, idelta) {
            return None;
        }
        let leapdays = leaps_thru_end_of(newy - 1) - leaps_thru_end_of(y - 1);
        tdays -= ((newy - y) as i64) * DAYSPERNYEAR as i64;
        tdays -= leapdays as i64;
        y = newy;
    }

    {
        let secperday_i32: i32 = (SECSPERHOUR * HOURSPERDAY) as i32;
        let seconds = (tdays * SECSPERDAY) as i32;
        tdays = (seconds / secperday_i32) as i64;
        rem += (seconds - tdays as i32 * secperday_i32) as i64;
    }

    let mut idays = tdays as i32;
    rem += offset as i64 - corr;

    while rem < 0 {
        rem += SECSPERDAY;
        idays -= 1;
    }
    while rem >= SECSPERDAY {
        rem -= SECSPERDAY;
        idays += 1;
    }
    while idays < 0 {
        if increment_overflow(&mut y, -1) {
            return None;
        }
        idays += YEAR_LENGTHS[if isleap(y) { 1 } else { 0 }];
    }
    while idays >= YEAR_LENGTHS[if isleap(y) { 1 } else { 0 }] {
        idays -= YEAR_LENGTHS[if isleap(y) { 1 } else { 0 }];
        if increment_overflow(&mut y, 1) {
            return None;
        }
    }

    tmp.tm_year = y;
    if increment_overflow(&mut tmp.tm_year, -TM_YEAR_BASE) {
        return None;
    }
    tmp.tm_yday = idays;

    tmp.tm_wday = EPOCH_WDAY
        + ((y - EPOCH_YEAR) % DAYSPERWEEK) * (DAYSPERNYEAR % DAYSPERWEEK)
        + leaps_thru_end_of(y - 1)
        - leaps_thru_end_of(EPOCH_YEAR - 1)
        + idays;
    tmp.tm_wday %= DAYSPERWEEK;
    if tmp.tm_wday < 0 {
        tmp.tm_wday += DAYSPERWEEK;
    }

    tmp.tm_hour = (rem / SECSPERHOUR as i64) as i32;
    rem %= SECSPERHOUR as i64;
    tmp.tm_min = (rem / SECSPERMIN as i64) as i32;
    tmp.tm_sec = (rem % SECSPERMIN as i64) as i32 + hit;

    let ip = &MON_LENGTHS[if isleap(y) { 1 } else { 0 }];
    tmp.tm_mon = 0;
    while idays >= ip[tmp.tm_mon as usize] {
        idays -= ip[tmp.tm_mon as usize];
        tmp.tm_mon += 1;
    }
    tmp.tm_mday = idays + 1;
    tmp.tm_isdst = 0;
    tmp.tm_gmtoff = offset as i64;

    Some(tmp as *mut stm)
}

// ---------------------------------------------------------------------------
// tmcomp
// ---------------------------------------------------------------------------

fn tmcomp(atmp: &stm, btmp: &stm) -> i32 {
    if atmp.tm_year != btmp.tm_year {
        return if atmp.tm_year < btmp.tm_year { -1 } else { 1 };
    }
    let mut result = atmp.tm_mon - btmp.tm_mon;
    if result == 0 {
        result = atmp.tm_mday - btmp.tm_mday;
    }
    if result == 0 {
        result = atmp.tm_hour - btmp.tm_hour;
    }
    if result == 0 {
        result = atmp.tm_min - btmp.tm_min;
    }
    if result == 0 {
        result = atmp.tm_sec - btmp.tm_sec;
    }
    result
}

// ---------------------------------------------------------------------------
// localsub
// ---------------------------------------------------------------------------

fn localsub(g: &mut TzGlobals, timep: &i64, _offset: i32, tmp: &mut stm) -> Option<*mut stm> {
    let t = *timep;

    // Read needed fields from lclmem to avoid holding a borrow across the recursive call
    let (goback, goahead, ats_first, ats_last) = {
        let sp = &g.lclmem;
        (
            sp.goback,
            sp.goahead,
            if sp.timecnt > 0 {
                Some(sp.ats[0])
            } else {
                None
            },
            if sp.timecnt > 0 {
                Some(sp.ats[sp.timecnt as usize - 1])
            } else {
                None
            },
        )
    };

    if (goback != 0 && ats_first.map_or(false, |a| t < a))
        || (goahead != 0 && ats_last.map_or(false, |a| t > a))
    {
        let ats_first = ats_first.unwrap_or(i64::MIN);
        let ats_last = ats_last.unwrap_or(i64::MAX);
        let mut newt = t;
        let seconds = if t < ats_first {
            ats_first - t
        } else {
            t - ats_last
        } - 1;
        let years = (seconds / SECSPERREPEAT + 1) * YEARSPERREPEAT as i64;
        let secs_adj = years * AVGSECSPERYEAR;

        if t < ats_first {
            newt += secs_adj;
        } else {
            newt -= secs_adj;
        }

        if newt < ats_first || newt > ats_last {
            return None;
        }

        let result = localsub(g, &newt, 0, tmp);
        if let Some(_ptr) = result {
            let mut newy = tmp.tm_year as i64;
            if t < ats_first {
                newy -= years;
            } else {
                newy += years;
            }
            if newy < i32::MIN as i64 || newy > i32::MAX as i64 {
                return None;
            }
            tmp.tm_year = newy as i32;
        }
        return result;
    }

    let sp = &g.lclmem;

    let idx: i32;
    if sp.timecnt == 0 || t < sp.ats[0] {
        idx = sp.defaulttype;
    } else {
        let mut lo = 1;
        let mut hi = sp.timecnt;
        while lo < hi {
            let mid = (lo + hi) >> 1;
            if t < sp.ats[mid as usize] {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        idx = sp.types[(lo - 1) as usize] as i32;
    }

    let ttisp = &sp.ttis[idx as usize];
    let result = timesub(&t, ttisp.tt_gmtoff, sp, tmp)?;
    tmp.tm_isdst = ttisp.tt_isdst;

    // Set tm_zone to point into the state's chars buffer.
    // This is safe because the chars buffer lives in the TzGlobals struct
    // which is behind a Mutex and persists for the lifetime of the program.
    let abbr_ind = ttisp.tt_abbrind as usize;
    tmp.tm_zone = sp.chars[abbr_ind..].as_ptr() as *const i8;

    Some(result)
}

// ---------------------------------------------------------------------------
// gmtsub
// ---------------------------------------------------------------------------

fn gmtsub(g: &mut TzGlobals, timep: &i64, offset: i32, tmp: &mut stm) -> Option<*mut stm> {
    if g.gmt_is_set == 0 {
        g.gmt_is_set = 1;
        gmtload(&mut g.gmtmem);
    }
    timesub(timep, offset, &g.gmtmem, tmp)
}

// ---------------------------------------------------------------------------
// time2sub, time2, time1 (mktime implementation)
// ---------------------------------------------------------------------------

fn time2sub(
    g: &mut TzGlobals,
    tmp: &mut stm,
    funcp: fn(&mut TzGlobals, &i64, i32, &mut stm) -> Option<*mut stm>,
    offset: i32,
    okayp: &mut bool,
    do_norm_secs: bool,
) -> i64 {
    *okayp = false;

    let mut yourtm = *tmp;
    let mut mytm = stm::default();

    if do_norm_secs
        && normalize_overflow(&mut yourtm.tm_min, &mut yourtm.tm_sec, SECSPERMIN)
    {
        return WRONG;
    }
    if normalize_overflow(&mut yourtm.tm_hour, &mut yourtm.tm_min, MINSPERHOUR) {
        return WRONG;
    }
    if normalize_overflow(&mut yourtm.tm_mday, &mut yourtm.tm_hour, HOURSPERDAY) {
        return WRONG;
    }
    let mut y = yourtm.tm_year;
    if normalize_overflow32(&mut y, &mut yourtm.tm_mon, MONSPERYEAR as i32) {
        return WRONG;
    }
    if increment_overflow32(&mut y, TM_YEAR_BASE) {
        return WRONG;
    }

    while yourtm.tm_mday <= 0 {
        if increment_overflow32(&mut y, -1) {
            return WRONG;
        }
        let li = y + (if yourtm.tm_mon > 0 { 1 } else { 0 });
        yourtm.tm_mday += YEAR_LENGTHS[if isleap(li) { 1 } else { 0 }];
    }
    while yourtm.tm_mday > DAYSPERLYEAR {
        let li = y + (if yourtm.tm_mon > 0 { 1 } else { 0 });
        yourtm.tm_mday -= YEAR_LENGTHS[if isleap(li) { 1 } else { 0 }];
        if increment_overflow32(&mut y, 1) {
            return WRONG;
        }
    }

    loop {
        let i = MON_LENGTHS[if isleap(y) { 1 } else { 0 }][yourtm.tm_mon as usize];
        if yourtm.tm_mday <= i {
            break;
        }
        yourtm.tm_mday -= i;
        yourtm.tm_mon += 1;
        if yourtm.tm_mon >= MONSPERYEAR as i32 {
            yourtm.tm_mon = 0;
            if increment_overflow32(&mut y, 1) {
                return WRONG;
            }
        }
    }

    if increment_overflow32(&mut y, -TM_YEAR_BASE) {
        return WRONG;
    }
    yourtm.tm_year = y;
    if yourtm.tm_year != y {
        return WRONG;
    }

    let saved_seconds: i32;
    if yourtm.tm_sec >= 0 && yourtm.tm_sec < SECSPERMIN {
        saved_seconds = 0;
    } else if y + TM_YEAR_BASE < EPOCH_YEAR {
        if increment_overflow(&mut yourtm.tm_sec, 1 - SECSPERMIN) {
            return WRONG;
        }
        saved_seconds = yourtm.tm_sec;
        yourtm.tm_sec = SECSPERMIN - 1;
    } else {
        saved_seconds = yourtm.tm_sec;
        yourtm.tm_sec = 0;
    }

    // Binary search
    let mut lo = time_t_min();
    let mut hi = time_t_max();
    let mut t: i64;

    loop {
        t = lo / 2 + hi / 2;
        if t < lo {
            t = lo;
        } else if t > hi {
            t = hi;
        }

        let dir = if funcp(g, &t, offset, &mut mytm).is_none() {
            if t > 0 {
                1
            } else {
                -1
            }
        } else {
            tmcomp(&mytm, &yourtm)
        };

        if dir != 0 {
            if t == lo {
                if t == time_t_max() {
                    return WRONG;
                }
                t += 1;
                lo += 1;
            } else if t == hi {
                if t == time_t_min() {
                    return WRONG;
                }
                t -= 1;
                hi -= 1;
            }
            if lo > hi {
                return WRONG;
            }
            if dir > 0 {
                hi = t;
            } else {
                lo = t;
            }
            continue;
        }

        if yourtm.tm_isdst < 0 || mytm.tm_isdst == yourtm.tm_isdst {
            break;
        }

        // Right time, wrong type -- hunt for right time, right type
        let sp: state = if std::ptr::eq(funcp as *const (), localsub as *const ()) {
            g.lclmem.clone()
        } else {
            g.gmtmem.clone()
        };

        let mut found = false;
        for ii in (0..sp.typecnt).rev() {
            if sp.ttis[ii as usize].tt_isdst != yourtm.tm_isdst {
                continue;
            }
            for jj in (0..sp.typecnt).rev() {
                if sp.ttis[jj as usize].tt_isdst == yourtm.tm_isdst {
                    continue;
                }
                let newt = t + sp.ttis[jj as usize].tt_gmtoff as i64
                    - sp.ttis[ii as usize].tt_gmtoff as i64;
                if funcp(g, &newt, offset, &mut mytm).is_none() {
                    continue;
                }
                if tmcomp(&mytm, &yourtm) != 0 {
                    continue;
                }
                if mytm.tm_isdst != yourtm.tm_isdst {
                    continue;
                }
                t = newt;
                found = true;
                break;
            }
            if found {
                break;
            }
        }
        if !found {
            return WRONG;
        }
        break;
    }

    let newt = t + saved_seconds as i64;
    if (newt < t) != (saved_seconds < 0) {
        return WRONG;
    }
    let t = newt;
    if funcp(g, &t, offset, tmp).is_some() {
        *okayp = true;
    }
    t
}

fn time2(
    g: &mut TzGlobals,
    tmp: &mut stm,
    funcp: fn(&mut TzGlobals, &i64, i32, &mut stm) -> Option<*mut stm>,
    offset: i32,
    okayp: &mut bool,
) -> i64 {
    let t = time2sub(g, tmp, funcp, offset, okayp, false);
    if *okayp {
        return t;
    }
    time2sub(g, tmp, funcp, offset, okayp, true)
}

fn time1(
    g: &mut TzGlobals,
    tmp: &mut stm,
    funcp: fn(&mut TzGlobals, &i64, i32, &mut stm) -> Option<*mut stm>,
    offset: i32,
) -> i64 {
    if tmp.tm_isdst > 1 {
        tmp.tm_isdst = 1;
    }
    let mut okay = false;
    let mut t = time2(g, tmp, funcp, offset, &mut okay);
    if okay || tmp.tm_isdst < 0 {
        return t;
    }

    // R change: try unknown DST setting
    if tmp.tm_isdst >= 0 {
        tmp.tm_isdst = -1;
        let mut okay2 = false;
        t = time2(g, tmp, funcp, offset, &mut okay2);
        if okay2 {
            return t;
        }
    }

    // Try different type adjustments
    let sp: state = if std::ptr::eq(funcp as *const (), localsub as *const ()) {
        g.lclmem.clone()
    } else {
        g.gmtmem.clone()
    };

    let mut seen = [false; TZ_MAX_TYPES];
    let mut types = [0i32; TZ_MAX_TYPES];
    let mut nseen: usize = 0;

    for i in (0..sp.timecnt).rev() {
        let ti = sp.types[i as usize] as usize;
        if !seen[ti] {
            seen[ti] = true;
            types[nseen] = ti as i32;
            nseen += 1;
        }
    }

    for sameind in 0..nseen {
        let samei = types[sameind] as usize;
        if sp.ttis[samei].tt_isdst != tmp.tm_isdst {
            continue;
        }
        for otherind in 0..nseen {
            let otheri = types[otherind] as usize;
            if sp.ttis[otheri].tt_isdst == tmp.tm_isdst {
                continue;
            }
            tmp.tm_sec += sp.ttis[otheri].tt_gmtoff - sp.ttis[samei].tt_gmtoff;
            tmp.tm_isdst = if tmp.tm_isdst != 0 { 0 } else { 1 };
            let mut okay3 = false;
            t = time2(g, tmp, funcp, offset, &mut okay3);
            if okay3 {
                return t;
            }
            tmp.tm_sec -= sp.ttis[otheri].tt_gmtoff - sp.ttis[samei].tt_gmtoff;
            tmp.tm_isdst = if tmp.tm_isdst != 0 { 0 } else { 1 };
        }
    }

    WRONG
}

// ---------------------------------------------------------------------------
// R_tzsetwall
// ---------------------------------------------------------------------------

fn r_tzsetwall(g: &mut TzGlobals) {
    if g.lcl_is_set < 0 {
        return;
    }
    g.lcl_is_set = -1;
    if tzload(None, &mut g.lclmem, true) != 0 {
        gmtload(&mut g.lclmem);
    }
    settzname(g);
}

// ---------------------------------------------------------------------------
// R_tzset (the main tzset implementation)
// ---------------------------------------------------------------------------

fn r_tzset_impl(g: &mut TzGlobals) {
    let name = match env::var("TZ") {
        Ok(n) => n,
        Err(_) => {
            r_tzsetwall(g);
            return;
        }
    };

    // Check if already set
    if g.lcl_is_set > 0 {
        let current = std::str::from_utf8(&g.lcl_TZname[..]).unwrap_or("");
        if current == name {
            return;
        }
    }

    g.lcl_is_set = if name.len() < TZ_STRLEN_MAX { 1 } else { 0 };
    if g.lcl_is_set > 0 {
        let bytes = name.as_bytes();
        let len = bytes.len().min(TZ_STRLEN_MAX);
        g.lcl_TZname[..len].copy_from_slice(&bytes[..len]);
        g.lcl_TZname[len] = 0;
    }

    if name.is_empty() {
        // Fast but wrong -- user wants UTC
        g.lclmem.leapcnt = 0;
        g.lclmem.timecnt = 0;
        g.lclmem.typecnt = 0;
        g.lclmem.charcnt = 0;
        g.lclmem.goback = 0;
        g.lclmem.goahead = 0;
        g.lclmem.ttis[0].tt_isdst = 0;
        g.lclmem.ttis[0].tt_gmtoff = 0;
        g.lclmem.ttis[0].tt_abbrind = 0;
        g.lclmem.ttis[0].tt_ttisstd = 0;
        g.lclmem.ttis[0].tt_ttisgmt = 0;
        let gmt_bytes = b"GMT";
        g.lclmem.chars[..gmt_bytes.len()].copy_from_slice(gmt_bytes);
        g.lclmem.chars[gmt_bytes.len()] = 0;
        g.lclmem.defaulttype = 0;
    } else if tzload(Some(&name), &mut g.lclmem, true) != 0 {
        // tzload failed, try other methods
        if !name.starts_with(':') && tzparse(&name, &mut g.lclmem, false) == 0 {
            // tzparse succeeded, keep result
        } else {
            gmtload(&mut g.lclmem);
        }
    }
    settzname(g);
}

// ===========================================================================
// Public FFI API
// ===========================================================================

/// `R_gmtime` -- convert time_t to UTC broken-down time (non-reentrant).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_gmtime(timep: *const i64) -> *mut stm {
    unsafe {
        let mut g = get_tz_globals().lock().expect("tz globals mutex poisoned");
        r_tzset_impl(&mut g);
        let t = if timep.is_null() { 0 } else { *timep };
        let mut tmp = stm::default();
        match gmtsub(&mut g, &t, 0, &mut tmp) {
            Some(_) => {
                g.tm = tmp;
                &mut g.tm as *mut stm
            }
            None => std::ptr::null_mut(),
        }
    }
}

/// `R_gmtime_r` -- convert time_t to UTC broken-down time (reentrant).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_gmtime_r(timep: *const i64, tmp: *mut stm) -> *mut stm {
    unsafe {
        let mut g = get_tz_globals().lock().expect("tz globals mutex poisoned");
        if timep.is_null() || tmp.is_null() {
            return std::ptr::null_mut();
        }
        let t = *timep;
        let mut result = stm::default();
        match gmtsub(&mut g, &t, 0, &mut result) {
            Some(_) => {
                std::ptr::copy_nonoverlapping(&result as *const stm, tmp, 1);
                tmp
            }
            None => std::ptr::null_mut(),
        }
    }
}

/// `R_localtime` -- convert time_t to local broken-down time (non-reentrant).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_localtime(timep: *const i64) -> *mut stm {
    unsafe {
        let mut g = get_tz_globals().lock().expect("tz globals mutex poisoned");
        r_tzset_impl(&mut g);
        let t = if timep.is_null() { 0 } else { *timep };
        let mut tmp = stm::default();
        match localsub(&mut g, &t, 0, &mut tmp) {
            Some(_) => {
                g.tm = tmp;
                &mut g.tm as *mut stm
            }
            None => std::ptr::null_mut(),
        }
    }
}

/// `R_localtime_r` -- convert time_t to local broken-down time (reentrant).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_localtime_r(timep: *const i64, tmp: *mut stm) -> *mut stm {
    unsafe {
        let mut g = get_tz_globals().lock().expect("tz globals mutex poisoned");
        if timep.is_null() || tmp.is_null() {
            return std::ptr::null_mut();
        }
        let t = *timep;
        let mut result = stm::default();
        match localsub(&mut g, &t, 0, &mut result) {
            Some(_) => {
                std::ptr::copy_nonoverlapping(&result as *const stm, tmp, 1);
                tmp
            }
            None => std::ptr::null_mut(),
        }
    }
}

/// `R_mktime` -- convert local broken-down time to time_t.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_mktime(tmp: *mut stm) -> i64 {
    unsafe {
        let mut g = get_tz_globals().lock().expect("tz globals mutex poisoned");
        r_tzset_impl(&mut g);
        if tmp.is_null() {
            return WRONG;
        }
        let mut t = std::ptr::read(tmp);
        time1(&mut g, &mut t, localsub, 0)
    }
}

/// `R_timegm` -- convert UTC broken-down time to time_t.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_timegm(tmp: *mut stm) -> i64 {
    unsafe {
        let mut g = get_tz_globals().lock().expect("tz globals mutex poisoned");
        if tmp.is_null() {
            return WRONG;
        }
        let mut t = std::ptr::read(tmp);
        t.tm_isdst = 0;
        time1(&mut g, &mut t, gmtsub, 0)
    }
}

/// `R_tzset` -- set timezone from TZ environment variable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tzset() {
    let mut g = get_tz_globals().lock().expect("tz globals mutex poisoned");
    r_tzset_impl(&mut g);
}

/// `R_tzsetwall` -- set timezone from system wall clock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tzsetwall() {
    let mut g = get_tz_globals().lock().expect("tz globals mutex poisoned");
    r_tzsetwall(&mut g);
}

/// `R_tzname` -- returns a pointer to the [2]-element array of timezone name
/// pointers (standard, daylight).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tzname() -> *mut *mut i8 {
    // Ensure tzset has been called so the names are populated
    let mut g = get_tz_globals().lock().expect("tz globals mutex poisoned");
    r_tzset_impl(&mut g);
    drop(g);
    std::ptr::addr_of_mut!(R_TZNAME) as *mut *mut i8
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_mktime_epoch() {
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
}
