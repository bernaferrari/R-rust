// tzcode type/constant surface kept for port fidelity.
#![allow(dead_code)]

use super::*;

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
    pub tm_zone: *const core::ffi::c_char,
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

// Safety: stm is #[repr(C)] and only used inside per-session TzGlobals state.
// The raw pointer tm_zone points into the chars buffer of a state struct
// that lives as long as the active RInstance timezone cache is held.
unsafe impl Send for stm {}

pub type R_time_t = i64;

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

/// Time type information (mirrors C `struct ttinfo`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ttinfo {
    pub(crate) tt_gmtoff: i32,  // UT offset in seconds
    pub(crate) tt_isdst: i32,   // used to set tm_isdst
    pub(crate) tt_abbrind: i32, // abbreviation list index
    pub(crate) tt_ttisstd: i32, // TRUE if transition is std time
    pub(crate) tt_ttisgmt: i32, // TRUE if transition is UT
}

/// Leap second information (mirrors C `struct lsinfo`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct lsinfo {
    pub(crate) ls_trans: i64, // transition time
    pub(crate) ls_corr: i64,  // correction to apply
}

/// Rule (mirrors C `struct rule`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct rule {
    pub(crate) r_type: i32, // type of rule
    pub(crate) r_day: i32,  // day number of rule
    pub(crate) r_week: i32, // week number of rule
    pub(crate) r_mon: i32,  // month number of rule
    pub(crate) r_time: i32, // transition time of rule
}

/// The `chars` buffer size in `state`.
/// Matches the C expression:
///   BIGGEST(BIGGEST(TZ_MAX_CHARS + 1, sizeof gmt), (2 * (MY_TZNAME_MAX + 1)))
/// sizeof "GMT" = 4 (including NUL), 2 * 256 = 512.
const CHARS_SIZE: usize = 512;

/// State (mirrors C `struct state`).
#[derive(Clone, Debug)]
pub(crate) struct state {
    pub(crate) leapcnt: i32,
    pub(crate) timecnt: i32,
    pub(crate) typecnt: i32,
    pub(crate) charcnt: i32,
    pub(crate) goback: i32,
    pub(crate) goahead: i32,
    pub(crate) ats: [i64; TZ_MAX_TIMES],
    pub(crate) types: [u8; TZ_MAX_TIMES],
    pub(crate) ttis: [ttinfo; TZ_MAX_TYPES],
    pub(crate) chars: [u8; CHARS_SIZE],
    pub(crate) lsis: [lsinfo; TZ_MAX_LEAPS],
    pub(crate) defaulttype: i32,
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
pub(crate) struct tzhead {
    pub(crate) tzh_magic: [u8; 4],
    pub(crate) tzh_version: [u8; 1],
    pub(crate) tzh_reserved: [u8; 15],
    pub(crate) tzh_ttisgmtcnt: [u8; 4],
    pub(crate) tzh_ttisstdcnt: [u8; 4],
    pub(crate) tzh_leapcnt: [u8; 4],
    pub(crate) tzh_timecnt: [u8; 4],
    pub(crate) tzh_typecnt: [u8; 4],
    pub(crate) tzh_charcnt: [u8; 4],
}

// ---------------------------------------------------------------------------
// Runtime state. The original C code uses file-scope statics; the compiled
// Rust port stores the cache on RInstance so parallel sessions do not share it.
// ---------------------------------------------------------------------------

pub(crate) struct TzGlobals {
    pub(crate) lclmem: state,
    pub(crate) gmtmem: state,
    pub(crate) lcl_TZname: [u8; TZ_STRLEN_MAX + 1],
    pub(crate) lcl_is_set: i32,
    pub(crate) gmt_is_set: i32,
    pub(crate) tm: stm,
    pub(crate) tzname_bufs: [Box<[u8; TZ_MAX_CHARS + 1]>; 2],
    pub(crate) tzname_ptrs: Box<[*mut core::ffi::c_char; 2]>,
    pub(crate) override_name: Option<String>,
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
            tzname_bufs: [
                Box::new([0u8; TZ_MAX_CHARS + 1]),
                Box::new([0u8; TZ_MAX_CHARS + 1]),
            ],
            tzname_ptrs: Box::new([std::ptr::null_mut(); 2]),
            override_name: None,
        };
        // Initialize gmtmem with "GMT"
        let gmt_bytes = b"GMT";
        g.gmtmem.chars[..gmt_bytes.len()].copy_from_slice(gmt_bytes);
        g.gmtmem.charcnt = 3;
        g.sync_tzname_ptrs();
        g
    }
}

impl TzGlobals {
    pub(crate) fn sync_tzname_ptrs(&mut self) {
        self.tzname_ptrs[0] = self.tzname_bufs[0].as_mut_ptr() as *mut core::ffi::c_char;
        self.tzname_ptrs[1] = self.tzname_bufs[1].as_mut_ptr() as *mut core::ffi::c_char;
    }
}

/// Per-session timezone runtime state.
pub struct TzRuntimeState {
    globals: TzGlobals,
}

impl TzRuntimeState {
    fn globals_mut(&mut self) -> &mut TzGlobals {
        &mut self.globals
    }

    pub(crate) fn set_override(&mut self, name: Option<String>) {
        self.globals.override_name = name;
        self.globals.lcl_is_set = 0;
    }
}

impl Default for TzRuntimeState {
    fn default() -> Self {
        Self {
            globals: TzGlobals::default(),
        }
    }
}

pub fn timezone_override() -> Option<String> {
    with_tz_globals(|g| g.override_name.clone())
}

pub fn set_timezone_override(name: Option<String>) {
    with_required_current_instance(|inst| unsafe {
        (*inst).tzone_state.set_override(name);
    });
}

pub(crate) fn with_tz_globals<R>(f: impl FnOnce(&mut TzGlobals) -> R) -> R {
    // P1: the &mut TzGlobals lend spans only pure C-translated timezone
    // arithmetic; tzone code never allocates in R or reenters the evaluator.
    with_required_current_instance(|inst| f(unsafe { (*inst).tzone_state.globals_mut() }))
}
