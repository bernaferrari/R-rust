use super::*;

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

pub(crate) fn transtime(year: i32, rulep: &rule, offset: i32) -> i64 {
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

pub(crate) fn tzparse(name: &str, sp: &mut state, lastditch: bool) -> i32 {
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
