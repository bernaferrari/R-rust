//! One pinned GNU compatibility target for introspection and stream metadata.
//! This identifies the oracle, not a claim that every GNU feature is complete.
#![forbid(unsafe_code)]

macro_rules! target {
    ($major:literal, $minor:literal, $patch:literal, $year:literal, $month:literal,
     $day:literal, $revision:literal, $status:literal, $nickname:literal) => {
        pub const COMPONENTS: [i32; 3] = [$major, $minor, $patch];
        pub const PACKED_VERSION: i32 = ($major << 16) | ($minor << 8) | $patch;
        pub const MAJOR: &str = stringify!($major);
        pub const MINOR: &str = concat!(stringify!($minor), ".", stringify!($patch));
        pub const YEAR: &str = stringify!($year);
        pub const MONTH: &str = $month;
        pub const DAY: &str = $day;
        pub const REVISION: i32 = $revision;
        pub const REVISION_TEXT: &str = stringify!($revision);
        pub const STATUS: &str = $status;
        pub const NICKNAME: &str = $nickname;
        pub const NUMERIC_VERSION: &str = concat!(
            stringify!($major),
            ".",
            stringify!($minor),
            ".",
            stringify!($patch)
        );
        pub const VERSION_STRING: &str = concat!(
            "R version ",
            stringify!($major),
            ".",
            stringify!($minor),
            ".",
            stringify!($patch),
            " (Rust Port; GNU compatibility target)"
        );
        pub const MAJOR_C: &[u8] = concat!(stringify!($major), "\0").as_bytes();
        pub const MINOR_C: &[u8] =
            concat!(stringify!($minor), ".", stringify!($patch), "\0").as_bytes();
        pub const YEAR_C: &[u8] = concat!(stringify!($year), "\0").as_bytes();
        pub const MONTH_C: &[u8] = concat!($month, "\0").as_bytes();
        pub const DAY_C: &[u8] = concat!($day, "\0").as_bytes();
        pub const STATUS_C: &[u8] = concat!($status, "\0").as_bytes();
        pub const NICKNAME_C: &[u8] = concat!($nickname, "\0").as_bytes();
        pub const VERSION_STRING_C: &[u8] = concat!(
            "R version ",
            stringify!($major),
            ".",
            stringify!($minor),
            ".",
            stringify!($patch),
            " (Rust Port; GNU compatibility target)\0"
        )
        .as_bytes();
    };
}

// Authenticated by oracle/r-oracle.json; the test below rejects a stale pin.
target!(
    4,
    7,
    0,
    2026,
    "08",
    "27",
    90451,
    "Under development (unstable)",
    "Unsuffered Consequences"
);

#[cfg(target_os = "macos")]
pub const OS: &str = "darwin";
#[cfg(not(target_os = "macos"))]
pub const OS: &str = std::env::consts::OS;

pub const PLATFORM: &str = "rust-port";
pub const ARCH: &str = std::env::consts::ARCH;

const fn c_string<const N: usize>(text: &str) -> [u8; N] {
    assert!(N == text.len() + 1);
    let mut bytes = [0; N];
    let input = text.as_bytes();
    let mut index = 0;
    while index < input.len() {
        bytes[index] = input[index];
        index += 1;
    }
    bytes
}
pub const PLATFORM_C: [u8; PLATFORM.len() + 1] = c_string(PLATFORM);
pub const ARCH_C: [u8; ARCH.len() + 1] = c_string(ARCH);
pub const OS_C: [u8; OS.len() + 1] = c_string(OS);

pub const FIELDS: [(&str, &str); 14] = [
    ("platform", PLATFORM),
    ("arch", ARCH),
    ("os", OS),
    ("system", "rust-port"),
    ("status", STATUS),
    ("major", MAJOR),
    ("minor", MINOR),
    ("year", YEAR),
    ("month", MONTH),
    ("day", DAY),
    ("svn rev", REVISION_TEXT),
    ("language", "R"),
    ("version.string", VERSION_STRING),
    ("nickname", NICKNAME),
];

/// Copy into a caller-owned buffer, reserving the final cell for C termination.
pub fn write_version(output: &mut [u8]) -> Option<usize> {
    if output.is_empty() {
        return None;
    }
    let text = VERSION_STRING.as_bytes();
    let copied = text.len().min(output.len() - 1);
    output[..copied].copy_from_slice(&text[..copied]);
    output[copied] = 0;
    Some(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_target_matches_the_authenticated_oracle_manifest() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../../oracle/r-oracle.json")).unwrap();
        let runtime = &manifest["runtime"];
        assert_eq!(runtime["version"], format!("{NUMERIC_VERSION} {STATUS}"));
        assert_eq!(runtime["nickname"], NICKNAME);
        assert_eq!(runtime["svn_revision"], REVISION);
        assert_eq!(
            &runtime["source_date"].as_str().unwrap()[..10],
            format!("{YEAR}-{MONTH}-{DAY}")
        );
        assert_eq!(
            PACKED_VERSION,
            COMPONENTS[0] * 65536 + COMPONENTS[1] * 256 + COMPONENTS[2]
        );
        assert!(VERSION_STRING.contains("Rust Port"));
    }

    #[test]
    fn compatibility_version_buffer_reports_text_length_and_bounds_termination() {
        for length in [0, 1, 4, VERSION_STRING.len(), VERSION_STRING.len() + 1, 128] {
            let mut bytes = vec![0xa5; length];
            let written = write_version(&mut bytes);
            if length == 0 {
                assert_eq!(written, None);
            } else {
                let copied = VERSION_STRING.len().min(length - 1);
                assert_eq!(written, Some(copied));
                assert_eq!(&bytes[..copied], &VERSION_STRING.as_bytes()[..copied]);
                assert_eq!(bytes[copied], 0);
                assert!(bytes[copied + 1..].iter().all(|byte| *byte == 0xa5));
            }
        }
    }

    #[test]
    fn compatibility_c_strings_are_terminated_views_of_the_same_metadata() {
        for (text, bytes) in [
            (MAJOR, MAJOR_C),
            (MINOR, MINOR_C),
            (YEAR, YEAR_C),
            (MONTH, MONTH_C),
            (DAY, DAY_C),
            (STATUS, STATUS_C),
            (NICKNAME, NICKNAME_C),
            (VERSION_STRING, VERSION_STRING_C),
            (PLATFORM, PLATFORM_C.as_slice()),
            (ARCH, ARCH_C.as_slice()),
            (OS, OS_C.as_slice()),
        ] {
            assert_eq!(
                std::ffi::CStr::from_bytes_with_nul(bytes)
                    .unwrap()
                    .to_str()
                    .unwrap(),
                text
            );
        }
    }
}
