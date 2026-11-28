//! GNU's strptime exactly as datetime.c arranges — format-directed parsing
//! into the POSIXlt broken-down shape: directive scanning with C-locale
//! names, day-of-year and week reconciliation, fraction-of-second %OS
//! forms, zone-offset conversion, and the POSIXlt list assembly with its
//! classes and tzone attribute.

use r_core::heap::Heap;
use r_core::id::ValueId;
use r_core::intern::SymbolInterner;
use r_core::object::RObject;

/// One parsed field set — every slot optional exactly as the directive
/// scan arranges, reconciliation deciding the final broken-down shape.
#[derive(Default, Clone)]
struct Raw {
    year: Option<i64>,   // full year with century
    mon: Option<i64>,    // 1-based month as parsed
    mday: Option<i64>,
    hour: Option<i64>,
    min: Option<i64>,
    sec: Option<f64>,
    wday: Option<i64>,   // 0 = Sunday
    yday: Option<i64>,   // 1-based day-of-year as parsed
    week_u: Option<i64>,
    week_w: Option<i64>,
    hour12: Option<i64>,
    pm: Option<bool>,
    gmtoff: Option<i64>, // seconds east of Greenwich as parsed by %z
    inf_sec: bool,
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_year(year: i64) -> i64 {
    if is_leap(year) { 366 } else { 365 }
}

const WD_FULL: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const WD_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MON_FULL: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const MON_ABBR: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Case-insensitive prefix match against the name table exactly as glibc's
/// strptime arranges — the longest matching name wins, abbreviations
/// matching their own entries first.
fn match_name(input: &[u8], names: &[&str]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (idx, name) in names.iter().enumerate() {
        let nb = name.as_bytes();
        if input.len() >= nb.len() {
            let mut ok = true;
            for (k, ch) in nb.iter().enumerate() {
                if input[k].to_ascii_lowercase() != ch.to_ascii_lowercase() {
                    ok = false;
                    break;
                }
            }
            if ok && best.map(|(_, l)| nb.len() > l).unwrap_or(true) {
                best = Some((idx, nb.len()));
            }
        }
    }
    best
}

fn read_int(input: &[u8], pos: usize, max_digits: usize) -> Option<(i64, usize)> {
    let mut value: i64 = 0;
    let mut count = 0;
    while pos + count < input.len() && count < max_digits && input[pos + count].is_ascii_digit() {
        value = value * 10 + (input[pos + count] - b'0') as i64;
        count += 1;
    }
    if count == 0 { None } else { Some((value, count)) }
}

/// Scan one input string against the format exactly as glibc's directive
/// loop arranges. None answers the all-NA row.
fn scan(input: &str, format: &str) -> Option<Raw> {
    let inp = input.as_bytes();
    let fmt = format.as_bytes();
    let mut raw = Raw::default();
    let mut ip = 0usize;
    let mut fp = 0usize;
    let mut os_frac_digits: Option<usize> = None;
    while fp < fmt.len() {
        let c = fmt[fp];
        if c == b'%' {
            fp += 1;
            if fp >= fmt.len() { return None; }
            // %OS optionally carries a digit count exactly as GNU arranges.
            if fmt[fp] == b'O' && fp + 1 < fmt.len() && fmt[fp + 1] == b'S' {
                fp += 2;
                let mut digits = 0usize;
                let mut dcount = 0usize;
                while fp < fmt.len() && fmt[fp].is_ascii_digit() {
                    digits = digits * 10 + (fmt[fp] - b'0') as usize;
                    fp += 1;
                    dcount += 1;
                }
                if dcount > 0 { os_frac_digits = Some(digits); }
                scan_sec(inp, &mut ip, &mut raw, os_frac_digits)?;
                continue;
            }
            let d = fmt[fp];
            fp += 1;
            match d {
                b'%' => {
                    if ip >= inp.len() || inp[ip] != b'%' { return None; }
                    ip += 1;
                }
                b'Y' => {
                    let (v, n) = read_int(inp, ip, 8)?;
                    raw.year = Some(v);
                    ip += n;
                }
                b'y' => {
                    let (v, n) = read_int(inp, ip, 2)?;
                    // glibc's pivot exactly: 69-99 read 1969-1999, 00-68 read 2000-2068.
                    raw.year = Some(if v >= 69 { 1900 + v } else { 2000 + v });
                    ip += n;
                }
                b'm' => {
                    let (v, n) = read_int(inp, ip, 2)?;
                    raw.mon = Some(v);
                    ip += n;
                }
                b'd' | b'e' => {
                    if d == b'e' && ip < inp.len() && inp[ip] == b' ' { ip += 1; }
                    let (v, n) = read_int(inp, ip, 2)?;
                    raw.mday = Some(v);
                    ip += n;
                }
                b'H' => {
                    let (v, n) = read_int(inp, ip, 2)?;
                    raw.hour = Some(v);
                    ip += n;
                }
                b'M' => {
                    let (v, n) = read_int(inp, ip, 2)?;
                    raw.min = Some(v);
                    ip += n;
                }
                b'S' => {
                    scan_sec(inp, &mut ip, &mut raw, None)?;
                }
                b'w' => {
                    let (v, n) = read_int(inp, ip, 1)?;
                    raw.wday = Some(v);
                    ip += n;
                }
                b'j' => {
                    let (v, n) = read_int(inp, ip, 3)?;
                    raw.yday = Some(v);
                    ip += n;
                }
                b'U' => {
                    let (v, n) = read_int(inp, ip, 2)?;
                    raw.week_u = Some(v);
                    ip += n;
                }
                b'W' => {
                    let (v, n) = read_int(inp, ip, 2)?;
                    raw.week_w = Some(v);
                    ip += n;
                }
                b'I' => {
                    let (v, n) = read_int(inp, ip, 2)?;
                    raw.hour12 = Some(v);
                    ip += n;
                }
                b'p' => {
                    let rest = &inp[ip.min(inp.len())..];
                    if rest.len() >= 2 {
                        let pair = [rest[0].to_ascii_lowercase(), rest[1].to_ascii_lowercase()];
                        if pair == [b'a', b'm'] {
                            raw.pm = Some(false);
                            ip += 2;
                        } else if pair == [b'p', b'm'] {
                            raw.pm = Some(true);
                            ip += 2;
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                b'a' | b'A' => {
                    let rest = &inp[ip.min(inp.len())..];
                    let found = match_name(rest, &WD_FULL).or_else(|| match_name(rest, &WD_ABBR));
                    match found {
                        Some((idx, len)) => {
                            raw.wday = Some(idx as i64);
                            ip += len;
                        }
                        None => return None,
                    }
                }
                b'b' | b'B' | b'h' => {
                    let rest = &inp[ip.min(inp.len())..];
                    let found = match_name(rest, &MON_FULL).or_else(|| match_name(rest, &MON_ABBR));
                    match found {
                        Some((idx, len)) => {
                            raw.mon = Some(idx as i64 + 1);
                            ip += len;
                        }
                        None => return None,
                    }
                }
                b'z' => {
                    // RFC-822/GNU forms exactly: ±HHMM, ±HH:MM, or a bare Z.
                    if ip < inp.len() && (inp[ip] == b'Z' || inp[ip] == b'z') {
                        raw.gmtoff = Some(0);
                        ip += 1;
                    } else if ip < inp.len() && (inp[ip] == b'+' || inp[ip] == b'-') {
                        let sign = if inp[ip] == b'-' { -1i64 } else { 1i64 };
                        ip += 1;
                        let (hh, n1) = read_int(inp, ip, 2)?;
                        ip += n1;
                        let mut mm = 0i64;
                        if ip < inp.len() && inp[ip] == b':' {
                            ip += 1;
                            let (m, n2) = read_int(inp, ip, 2)?;
                            mm = m;
                            ip += n2;
                        } else {
                            let (m, n2) = read_int(inp, ip, 2).unwrap_or((0, 0));
                            mm = m;
                            ip += n2;
                        }
                        raw.gmtoff = Some(sign * (hh * 3600 + mm * 60));
                    } else {
                        return None;
                    }
                }
                b'n' | b't' => {
                    while ip < inp.len() && inp[ip].is_ascii_whitespace() { ip += 1; }
                }
                _ => {
                    // Unsupported directives consume nothing and fail the row
                    // exactly as glibc's unknown-directive return arranges.
                    return None;
                }
            }
        } else if c.is_ascii_whitespace() {
            // Format whitespace matches zero or more input whitespace exactly.
            fp += 1;
            while ip < inp.len() && inp[ip].is_ascii_whitespace() { ip += 1; }
        } else {
            if ip >= inp.len() || inp[ip] != c { return None; }
            ip += 1;
            fp += 1;
        }
    }
    // Trailing unparsed input fails the row exactly as glibc arranges.
    if ip < inp.len() { return None; }
    Some(raw)
}

/// Seconds scanning with fraction exactly as %OS arranges — the bare "Inf"
/// spelling answering positive infinity, fraction digits bounded by the
/// count form when present. True answers success.
fn scan_sec(inp: &[u8], ip: &mut usize, raw: &mut Raw, frac_digits: Option<usize>) -> Option<()> {
    let rest = &inp[(*ip).min(inp.len())..];
    if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"inf") {
        raw.sec = Some(f64::INFINITY);
        raw.inf_sec = true;
        *ip += 3;
        return Some(());
    }
    let (v, n) = read_int(inp, *ip, 2)?;
    let mut sec = v as f64;
    *ip += n;
    if *ip < inp.len() && inp[*ip] == b'.' {
        *ip += 1;
        let mut frac = 0f64;
        let mut scale = 0.1f64;
        let mut count = 0usize;
        let cap = frac_digits.unwrap_or(6);
        while *ip < inp.len() && inp[*ip].is_ascii_digit() && count < cap {
            frac += (inp[*ip] - b'0') as f64 * scale;
            scale /= 10.0;
            count += 1;
            *ip += 1;
        }
        sec += frac;
    }
    raw.sec = Some(sec);
    Some(())
}

/// The broken-down result — NA rows carry every component NA exactly as
/// GNU's failed-parse shape arranges.
pub struct Broken {
    pub sec: f64,
    pub min: i64,
    pub hour: i64,
    pub mday: i64,
    pub mon: i64,   // 0-based
    pub year: i64,  // years since 1900
    pub wday: i64,
    pub yday: i64,  // 0-based
    pub isdst: i64,
    pub gmtoff: f64,
    pub valid: bool,
    pub has_date: bool,
}

const NA_I: i64 = i64::MIN;
const NA_F: f64 = f64::NAN;

/// Reconcile one row exactly as GNU's post-scan logic arranges: range
/// validation, %j filling, week reconciliation, weekday/day-of-year
/// derivation, and the zone-offset conversion.
pub fn reconcile(raw: &Raw) -> Broken {
    let mut invalid = Broken {
        sec: NA_F, min: NA_I, hour: NA_I, mday: NA_I, mon: NA_I, year: NA_I,
        wday: NA_I, yday: NA_I, isdst: NA_I, gmtoff: NA_F, valid: false, has_date: false,
    };
    // Inf seconds stand alone exactly as the r90409 shape arranges — the
    // parse succeeds with every other field untouched and isdst at -1.
    if raw.inf_sec {
        invalid.sec = f64::INFINITY;
        invalid.isdst = -1;
        invalid.gmtoff = 0.0;
        invalid.valid = true;
        return invalid;
    }
    let mut year = raw.year;
    let mut mon = raw.mon;
    let mut mday = raw.mday;

    // Range validation exactly as the scan-time rejects arrange.
    if let Some(m) = mon && !(1..=12).contains(&m) { return invalid; }
    if let Some(d) = mday && !(1..=31).contains(&d) { return invalid; }
    if let Some(h) = raw.hour && !(0..=23).contains(&h) { return invalid; }
    if let Some(mi) = raw.min && !(0..=59).contains(&mi) { return invalid; }
    if let Some(s) = raw.sec && s.is_finite() && !(s >= 0.0 && s < 62.0) { return invalid; }
    if let Some(h) = raw.hour12 && !(1..=12).contains(&h) { return invalid; }
    if let Some(w) = raw.wday && !(0..=6).contains(&w) { return invalid; }

    // %j fills month and day exactly as glibc arranges, the leap-year
    // boundary deciding the maximum.
    if let (Some(y), Some(j)) = (year, raw.yday) {
        if j < 1 || j > days_in_year(y) { return invalid; }
        let days = r_core::civil::days_from_civil(y, 1, 1) + (j - 1);
        let (_, m, d) = r_core::civil::civil_from_days(days);
        mon = Some(m as i64);
        mday = Some(d as i64);
    }

    // Week reconciliation exactly as GNU arranges: %U counts Sunday-first
    // weeks, %W Monday-first, the weekday pinning the day inside the week.
    if let (Some(y), Some(wd)) = (year, raw.wday) {
        if let Some(w) = raw.week_u {
            if !(0..=53).contains(&w) { return invalid; }
            let jan1 = r_core::civil::days_from_civil(y, 1, 1);
            let jan1_wd = r_core::civil::weekday_of(jan1) as i64; // 0 = Sunday exactly as weekday_of arranges
            // first Sunday's 0-based yday: days ahead of Jan 1.
            let first_sun = (7 - jan1_wd) % 7;
            let yday0 = first_sun + (w - 1) * 7 + wd;
            if yday0 >= 0 && yday0 < days_in_year(y) {
                let days = jan1 + yday0;
                let (_, m, d) = r_core::civil::civil_from_days(days);
                mon = Some(m as i64);
                mday = Some(d as i64);
            }
        } else if let Some(w) = raw.week_w {
            if !(0..=53).contains(&w) { return invalid; }
            let jan1 = r_core::civil::days_from_civil(y, 1, 1);
            let jan1_wd = r_core::civil::weekday_of(jan1) as i64;
            // first Monday's 0-based yday.
            let first_mon = (8 - jan1_wd) % 7;
            let yday0 = first_mon + (w - 1) * 7 + wd - 1;
            if yday0 >= 0 && yday0 < days_in_year(y) {
                let days = jan1 + yday0;
                let (_, m, d) = r_core::civil::civil_from_days(days);
                mon = Some(m as i64);
                mday = Some(d as i64);
            }
        }
    }

    let has_date = year.is_some() && mon.is_some() && mday.is_some();

    // Hour reconciliation: %I with %p exactly as glibc arranges — 12 AM
    // midnight, 12 PM noon.
    let mut hour = raw.hour.unwrap_or(0);
    if let Some(h12) = raw.hour12 {
        hour = match raw.pm {
            Some(true) => if h12 == 12 { 12 } else { h12 + 12 },
            Some(false) => if h12 == 12 { 0 } else { h12 },
            None => h12,
        };
    }
    let min = raw.min.unwrap_or(0);
    let sec = raw.sec.unwrap_or(0.0);

    let mut out = Broken {
        sec,
        min,
        hour,
        mday: mday.unwrap_or(0),
        mon: mon.map(|m| m - 1).unwrap_or(0),
        year: year.map(|y| y - 1900).unwrap_or(0),
        wday: NA_I,
        yday: NA_I,
        isdst: if has_date { 0 } else { -1 },
        gmtoff: 0.0,
        valid: true,
        has_date,
    };

    if has_date {
        let days = r_core::civil::days_from_civil(year.unwrap(), mon.unwrap() as u32, mday.unwrap() as u32);
        out.wday = r_core::civil::weekday_of(days) as i64;
        let jan1 = r_core::civil::days_from_civil(year.unwrap(), 1, 1);
        out.yday = days - jan1;
    } else {
        out.wday = raw.wday.unwrap_or(NA_I);
        if out.wday == NA_I && raw.wday.is_none() { out.wday = NA_I; }
        out.yday = NA_I;
    }

    // %z conversion exactly as GNU arranges: the parsed time reads as local
    // at the offset, the output re-derived in the target zone — UTC here —
    // shifting the clock and potentially the date.
    if let Some(off) = raw.gmtoff {
        if has_date {
            let days = r_core::civil::days_from_civil(year.unwrap(), mon.unwrap() as u32, mday.unwrap() as u32);
            let total = days * 86400 + hour * 3600 + min * 60 + sec as i64 - off;
            let ndays = total.div_euclid(86400);
            let rem = total.rem_euclid(86400);
            let (ny, nm, nd) = r_core::civil::civil_from_days(ndays);
            out.year = ny - 1900;
            out.mon = nm as i64 - 1;
            out.mday = nd as i64;
            out.hour = rem / 3600;
            out.min = (rem % 3600) / 60;
            out.sec = (rem % 60) as f64 + (sec - sec.trunc());
            out.wday = r_core::civil::weekday_of(ndays) as i64;
            let jan1 = r_core::civil::days_from_civil(ny, 1, 1);
            out.yday = ndays - jan1;
        }
        out.gmtoff = 0.0;
    }

    if year.is_none() && mon.is_none() && mday.is_none() && raw.yday.is_none()
        && raw.week_u.is_none() && raw.week_w.is_none() && raw.sec.is_none()
        && raw.hour.is_none() && raw.hour12.is_none() {
        // Nothing parsed at all — the row is NA exactly as GNU arranges.
        return invalid;
    }
    out.valid = true;
    out
}

/// The POSIXlt assembly exactly as GNU arranges — eleven named components
/// in the documented order, the class pair and the tzone attribute.
pub fn assemble(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    rows: &[Broken],
    zone_name: &str,
) -> ValueId {
    let n = rows.len();
    let mut sec: Vec<f64> = Vec::with_capacity(n);
    let mut min: Vec<i32> = Vec::with_capacity(n);
    let mut hour: Vec<i32> = Vec::with_capacity(n);
    let mut mday: Vec<i32> = Vec::with_capacity(n);
    let mut mon: Vec<i32> = Vec::with_capacity(n);
    let mut year: Vec<i32> = Vec::with_capacity(n);
    let mut wday: Vec<i32> = Vec::with_capacity(n);
    let mut yday: Vec<i32> = Vec::with_capacity(n);
    let mut isdst: Vec<i32> = Vec::with_capacity(n);
    let mut zone: Vec<r_core::intern::InternedStr> = Vec::with_capacity(n);
    let mut gmtoff: Vec<f64> = Vec::with_capacity(n);
    let na32 = r_core::object::NA_INTEGER;
    let zone_id = interner.intern_string(zone_name);
    let zone_na = interner.intern_string("\0NA");
    for row in rows {
        if !row.valid {
            sec.push(f64::NAN);
            min.push(na32);
            hour.push(na32);
            mday.push(na32);
            mon.push(na32);
            year.push(na32);
            wday.push(na32);
            yday.push(na32);
            isdst.push(na32);
            zone.push(zone_na);
            gmtoff.push(f64::NAN);
        } else {
            sec.push(row.sec);
            min.push(row.min as i32);
            hour.push(row.hour as i32);
            mday.push(row.mday as i32);
            mon.push(row.mon as i32);
            year.push(row.year as i32);
            wday.push(if row.wday == NA_I { na32 } else { row.wday as i32 });
            yday.push(if row.yday == NA_I { na32 } else { row.yday as i32 });
            isdst.push(row.isdst as i32);
            zone.push(zone_id);
            gmtoff.push(row.gmtoff);
        }
    }
    let fields: Vec<(&str, RObject)> = vec![
        ("sec", RObject::Real(sec)),
        ("min", RObject::Integer(min)),
        ("hour", RObject::Integer(hour)),
        ("mday", RObject::Integer(mday)),
        ("mon", RObject::Integer(mon)),
        ("year", RObject::Integer(year)),
        ("wday", RObject::Integer(wday)),
        ("yday", RObject::Integer(yday)),
        ("isdst", RObject::Integer(isdst)),
        ("zone", RObject::String(zone)),
        ("gmtoff", RObject::Real(gmtoff)),
    ];
    let mut ids = Vec::with_capacity(fields.len());
    let mut names = Vec::with_capacity(fields.len());
    for (name, obj) in fields {
        ids.push(heap.alloc(obj));
        names.push(interner.intern_string(name));
    }
    let list = heap.alloc(RObject::List(ids));
    // Attributes in GNU's order exactly: names, class, tzone.
    let names_obj = heap.alloc(RObject::String(names));
let list = r_core::prim::copy_with_attrib(heap, interner, list, "names", names_obj).unwrap_or(list);
    let class_obj = heap.alloc(RObject::String(vec![
        interner.intern_string("POSIXlt"),
        interner.intern_string("POSIXt"),
    ]));
let list = r_core::prim::copy_with_attrib(heap, interner, list, "class", class_obj).unwrap_or(list);
    let tzone_obj = heap.alloc(RObject::String(vec![zone_id]));
let list = r_core::prim::copy_with_attrib(heap, interner, list, "tzone", tzone_obj).unwrap_or(list);
    list
}

/// Parse a vector of inputs exactly as the primitive arranges — format
/// recycled across rows, unparseable rows NA.
pub fn strptime_rows(inputs: &[String], formats: &[String]) -> Vec<Broken> {
    inputs.iter().enumerate().map(|(i, text)| {
        let fmt = &formats[i % formats.len()];
        match scan(text, fmt) {
            Some(raw) => reconcile(&raw),
            None => reconcile(&Raw::default()),
        }
    }).collect()
}
