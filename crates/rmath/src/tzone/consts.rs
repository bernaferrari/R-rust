// tzcode constants (TZ_MAGIC, TM_*, month/day tables) kept for port fidelity.
#![allow(dead_code)]

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
