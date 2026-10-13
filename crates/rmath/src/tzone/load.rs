use super::*;

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

pub(crate) fn settzname(g: &mut TzGlobals) {
    for buf in &mut g.tzname_bufs {
        buf.fill(0);
        buf[..WILDABBR.len()].copy_from_slice(WILDABBR);
    }

    // Get the latest zone names
    for i in 0..g.lclmem.typecnt as usize {
        let ttisp = &g.lclmem.ttis[i];
        let isdst = ttisp.tt_isdst;
        let abbr = get_abbr(&g.lclmem, ttisp.tt_abbrind as usize).to_vec();
        if isdst != 0 {
            copy_to_tzname_buf(g, 1, &abbr);
        } else {
            copy_to_tzname_buf(g, 0, &abbr);
        }
    }
    for i in 0..g.lclmem.timecnt as usize {
        let ttisp = &g.lclmem.ttis[g.lclmem.types[i] as usize];
        let isdst = ttisp.tt_isdst;
        let abbr = get_abbr(&g.lclmem, ttisp.tt_abbrind as usize).to_vec();
        if isdst != 0 {
            copy_to_tzname_buf(g, 1, &abbr);
        } else {
            copy_to_tzname_buf(g, 0, &abbr);
        }
    }

    // Scrub each abbreviation independently. NULs separate names in
    // `chars`; replacing them would glue "MMT" and "GMT" into "MMT_GMT_".
    let valid_set = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 :+-._";
    for i in 0..g.lclmem.typecnt as usize {
        let abbr_start = g.lclmem.ttis[i].tt_abbrind as usize;
        let abbr = get_abbr(&g.lclmem, abbr_start);
        let is_gp = std::str::from_utf8(abbr)
            .map(|s| s == GRANDPARENTED)
            .unwrap_or(false);
        if is_gp {
            continue;
        }
        let n = abbr.len();
        for j in 0..n {
            let c = g.lclmem.chars[abbr_start + j];
            if !valid_set.contains(&c) {
                g.lclmem.chars[abbr_start + j] = b'_';
            }
        }
        if n > TZ_ABBR_MAX_LEN {
            g.lclmem.chars[abbr_start + TZ_ABBR_MAX_LEN] = 0;
        }
    }

    g.sync_tzname_ptrs();
}

fn copy_to_tzname_buf(g: &mut TzGlobals, which: usize, abbr: &[u8]) {
    let len = abbr.len().min(TZ_MAX_CHARS);
    let buf = &mut g.tzname_bufs[which];
    buf[..len].copy_from_slice(&abbr[..len]);
    buf[len] = 0;
}

// ---------------------------------------------------------------------------
// tzload
// ---------------------------------------------------------------------------

pub(crate) fn tzload(name: Option<&str>, sp: &mut state, doextend: bool) -> i32 {
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

    #[allow(clippy::manual_strip)]
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

    // Check access / try to open. Search order for relative names:
    // configured zoneinfo dir, then the platform zoneinfo database
    // (/usr/share/zoneinfo on macOS/Linux — what the system libc would
    // use), then UTC.
    let mut file = match File::open(name_to_use) {
        Ok(f) => f,
        Err(_) => {
            let candidates = [
                format!("/usr/share/zoneinfo/{name_stripped}"),
                format!("/var/db/timezone/zoneinfo/{name_stripped}"),
            ];
            let mut opened = None;
            for candidate in &candidates {
                if let Ok(f) = File::open(candidate) {
                    opened = Some(f);
                    break;
                }
            }
            match opened {
                Some(f) => f,
                None => {
                    rf_warning(&format!("unknown timezone '{sname}'"));
                    return -1;
                }
            }
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
