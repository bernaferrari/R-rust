//! Pinned GNU R zoneinfo data for hosts without a system zoneinfo directory.

use std::io;

use crate::mainutils::browser_files::{FileBytes, read_embedded_zlib_bytes};

const INDEX: &str = include_str!("zoneinfo/index.txt");
const RECORDS: &[u8] = include_bytes!("zoneinfo/records.bin");
const ZONES: &[u8] = include_bytes!("zoneinfo/zones.bin");
const MAX_ZONE_BYTES: usize = 8192;
const MAX_BLOCK_BYTES: usize = 32768;
const RECORD_BYTES: usize = 12;

pub(crate) fn bundled_zone_names() -> impl Iterator<Item = &'static str> {
    INDEX
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(name, _)| name))
}

fn invalid_data() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid bundled TZif data")
}

fn load(zone: &str) -> io::Result<Option<FileBytes>> {
    let Some(id) = INDEX.lines().find_map(|line| {
        let (name, entry) = line.split_once('\t')?;
        (zone == name).then_some(entry)
    }) else {
        return Ok(None);
    };
    let id: usize = id.parse().map_err(|_| invalid_data())?;
    let start = id.checked_mul(RECORD_BYTES).ok_or_else(invalid_data)?;
    let end = start.checked_add(RECORD_BYTES).ok_or_else(invalid_data)?;
    let record = RECORDS.get(start..end).ok_or_else(invalid_data)?;
    let from = u32::from_le_bytes(record[..4].try_into().map_err(|_| invalid_data())?) as usize;
    let to = u32::from_le_bytes(record[4..8].try_into().map_err(|_| invalid_data())?) as usize;
    let skip = u16::from_le_bytes(record[8..10].try_into().map_err(|_| invalid_data())?) as usize;
    let length =
        u16::from_le_bytes(record[10..12].try_into().map_err(|_| invalid_data())?) as usize;
    if length > MAX_ZONE_BYTES {
        return Err(invalid_data());
    }
    let compressed = ZONES.get(from..to).ok_or_else(invalid_data)?;
    let data = read_embedded_zlib_bytes(compressed, skip, length, MAX_BLOCK_BYTES)?;
    if !data.starts_with(b"TZif") {
        return Err(invalid_data());
    }
    Ok(Some(data))
}

/// Missing names may use the timezone parser's established fallback. A failed
/// decode of a known zone must raise an R error, including resource refusal.
pub(super) fn read_zone(zone: &str) -> Option<FileBytes> {
    load(zone).unwrap_or_else(|error| {
        crate::sexp::context::r_error(format!("cannot read timezone '{zone}': {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_zone_index_resolves_all_names_and_aliases() {
        let mut previous = "";
        let mut count = 0;
        for zone in bundled_zone_names() {
            assert!(previous < zone);
            assert!(read_zone(zone).is_some(), "missing TZif data for {zone}");
            previous = zone;
            count += 1;
        }
        assert_eq!(count, 597);
        assert_eq!(
            &*read_zone("US/Eastern").unwrap(),
            &*read_zone("America/New_York").unwrap()
        );
        assert!(read_zone("Unknown/Timezone").is_none());
    }

    #[test]
    fn bundled_timezone_charges_bytes_through_parse_and_recovers_from_denial() {
        use crate::sexp::{memory::ArenaBudget, session::RSession};

        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let baseline = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
        let data = load("America/New_York").unwrap().unwrap();
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            baseline + data.len()
        );
        let size = data.len();
        drop(data);
        owner
            .with_arena(|arena| {
                // Admit the decoder, but leave one byte too little for the zone.
                let limit = arena.total_bytes_allocated() + baseline + 64 * 1024 + size - 1;
                arena.set_budget(ArenaBudget::new(limit, 0));
            })
            .unwrap();

        assert!(load("Unknown/Timezone").unwrap().is_none());
        let denied = match load("America/New_York") {
            Err(denied) => denied,
            Ok(_) => panic!("timezone slice exceeded the remaining memory budget"),
        };
        assert_eq!(denied.kind(), io::ErrorKind::OutOfMemory);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            baseline
        );

        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        assert!(read_zone("US/Eastern").unwrap().starts_with(b"TZif"));
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            baseline
        );
    }
}
