//! Proleptic Gregorian calendar arithmetic — Howard Hinnant's civil date
//! algorithms exactly as every correct implementation converges on. Days
//! count from the Unix epoch 1970-01-01 precisely as POSIX and R require.

/// Days since epoch for a (year, month, day) civil date.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month as i64 + 9) % 12; // mar=0..feb=11
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The (year, month, day) of a day count since epoch.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The weekday of a day count — 1970-01-01 was a Thursday; the result is
/// 0=Sunday exactly as R's POSIXlt wday convention.
pub fn weekday_of(days: i64) -> u32 {
    // 1970-01-01 is day 0 and was Thursday (wday 4).
    (((days % 7) + 7 + 4) % 7) as u32
}

/// ISO yyyy-mm-dd formatting of a day count.
pub fn format_iso(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse yyyy-mm-dd exactly — the ISO form as.Date tries first.
pub fn parse_iso(text: &str) -> Option<i64> {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i64 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;
    if month == 0 || month > 12 || day == 0 || day > 31 {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

pub const WEEKDAY_NAMES: [&str; 7] = [
    "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
];

pub const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August",
    "September", "October", "November", "December",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip_epoch_and_leap() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2020, 2, 3), 18295);
        assert_eq!(civil_from_days(18295), (2020, 2, 3));
        assert_eq!(format_iso(18295), "2020-02-03");
        assert_eq!(parse_iso("2020-02-03"), Some(18295));
        assert_eq!(weekday_of(18295), 1); // Monday
    }
}
