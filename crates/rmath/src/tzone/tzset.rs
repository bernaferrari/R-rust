// tzcode C API surface (R_gmtime/R_localtime/R_tzset/...): the engine uses
// the _r variants; the non-_r wrappers are kept for tzcode port fidelity.
#![allow(dead_code)]

use super::*;

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
    let name = match g.override_name.clone().or_else(|| env::var("TZ").ok()) {
        Some(n) => n,
        None => {
            r_tzsetwall(g);
            return;
        }
    };

    // Check if already set
    if g.lcl_is_set > 0 {
        let end = g
            .lcl_TZname
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(g.lcl_TZname.len());
        let current = std::str::from_utf8(&g.lcl_TZname[..end]).unwrap_or("");
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
        // Missing tz is the session zone, not UTC. An explicit "UTC" stays
        // on the named path below.
        g.lcl_is_set = 0;
        r_tzsetwall(g);
        return;
    }
    if tzload(Some(&name), &mut g.lclmem, true) != 0 {
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
pub unsafe fn R_gmtime(timep: *const i64) -> *mut stm {
    unsafe {
        with_tz_globals(|g| {
            r_tzset_impl(g);
            let t = if timep.is_null() { 0 } else { *timep };
            let mut tmp = stm::default();
            match gmtsub(g, &t, 0, &mut tmp) {
                Some(_) => {
                    g.tm = tmp;
                    &mut g.tm as *mut stm
                }
                None => std::ptr::null_mut(),
            }
        })
    }
}

/// `R_gmtime_r` -- convert time_t to UTC broken-down time (reentrant).
pub unsafe fn R_gmtime_r(timep: *const i64, tmp: *mut stm) -> *mut stm {
    unsafe {
        if timep.is_null() || tmp.is_null() {
            return std::ptr::null_mut();
        }
        with_tz_globals(|g| {
            let t = *timep;
            let mut result = stm::default();
            match gmtsub(g, &t, 0, &mut result) {
                Some(_) => {
                    std::ptr::copy_nonoverlapping(&result as *const stm, tmp, 1);
                    tmp
                }
                None => std::ptr::null_mut(),
            }
        })
    }
}

/// `R_localtime` -- convert time_t to local broken-down time (non-reentrant).
pub unsafe fn R_localtime(timep: *const i64) -> *mut stm {
    unsafe {
        with_tz_globals(|g| {
            r_tzset_impl(g);
            let t = if timep.is_null() { 0 } else { *timep };
            let mut tmp = stm::default();
            match localsub(g, &t, 0, &mut tmp) {
                Some(_) => {
                    g.tm = tmp;
                    &mut g.tm as *mut stm
                }
                None => std::ptr::null_mut(),
            }
        })
    }
}

/// `R_localtime_r` -- convert time_t to local broken-down time (reentrant).
pub unsafe fn R_localtime_r(timep: *const i64, tmp: *mut stm) -> *mut stm {
    unsafe {
        if timep.is_null() || tmp.is_null() {
            return std::ptr::null_mut();
        }
        with_tz_globals(|g| {
            r_tzset_impl(g);
            let t = *timep;
            let mut result = stm::default();
            match localsub(g, &t, 0, &mut result) {
                Some(_) => {
                    std::ptr::copy_nonoverlapping(&result as *const stm, tmp, 1);
                    tmp
                }
                None => std::ptr::null_mut(),
            }
        })
    }
}

/// `R_mktime` -- convert local broken-down time to time_t.
pub unsafe fn R_mktime(tmp: *mut stm) -> i64 {
    unsafe {
        if tmp.is_null() {
            return WRONG;
        }
        with_tz_globals(|g| {
            r_tzset_impl(g);
            let mut t = std::ptr::read(tmp);
            let result = time1(g, &mut t, localsub, 0);
            std::ptr::write(tmp, t);
            result
        })
    }
}

/// `R_timegm` -- convert UTC broken-down time to time_t.
pub unsafe fn R_timegm(tmp: *mut stm) -> i64 {
    unsafe {
        if tmp.is_null() {
            return WRONG;
        }
        with_tz_globals(|g| {
            let mut t = std::ptr::read(tmp);
            t.tm_isdst = 0;
            let result = time1(g, &mut t, gmtsub, 0);
            std::ptr::write(tmp, t);
            result
        })
    }
}

/// `R_tzset` -- set timezone from TZ environment variable.
pub fn R_tzset() {
    with_tz_globals(r_tzset_impl);
}

/// `R_tzsetwall` -- set timezone from system wall clock.
pub fn R_tzsetwall() {
    with_tz_globals(r_tzsetwall);
}

/// `R_tzname` -- returns a pointer to the [2]-element array of timezone name
/// pointers (standard, daylight).
pub unsafe fn R_tzname() -> *mut *mut core::ffi::c_char {
    with_tz_globals(|g| {
        r_tzset_impl(g);
        g.sync_tzname_ptrs();
        g.tzname_ptrs.as_mut_ptr()
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
