use super::*;

pub(crate) fn gmtload(sp: &mut state) {
    if tzload(Some("GMT"), sp, true) != 0 {
        let _ = tzparse("GMT", sp, true);
    }
}

// ---------------------------------------------------------------------------
// leaps_thru_end_of
// ---------------------------------------------------------------------------

pub(crate) fn leaps_thru_end_of(y: i32) -> i32 {
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

pub(crate) fn localsub(
    g: &mut TzGlobals,
    timep: &i64,
    _offset: i32,
    tmp: &mut stm,
) -> Option<*mut stm> {
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
    // This is safe because the chars buffer lives in the active RInstance's
    // timezone state for at least as long as the returned stm is used.
    let abbr_ind = ttisp.tt_abbrind as usize;
    tmp.tm_zone = sp.chars[abbr_ind..].as_ptr() as *const core::ffi::c_char;

    Some(result)
}

// ---------------------------------------------------------------------------
// gmtsub
// ---------------------------------------------------------------------------

pub(crate) fn gmtsub(
    g: &mut TzGlobals,
    timep: &i64,
    offset: i32,
    tmp: &mut stm,
) -> Option<*mut stm> {
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

    if do_norm_secs && normalize_overflow(&mut yourtm.tm_min, &mut yourtm.tm_sec, SECSPERMIN) {
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
            if t > 0 { 1 } else { -1 }
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

pub(crate) fn time1(
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
