//! Pinned GNU R zoneinfo data for hosts without a system zoneinfo directory.

use std::io::{self, Read};

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

pub(super) fn load(zone: &str) -> Option<Vec<u8>> {
    let id: usize = INDEX
        .lines()
        .find_map(|line| {
            let (name, entry) = line.split_once('\t')?;
            (zone == name).then_some(entry)
        })?
        .parse()
        .ok()?;
    let start = id.checked_mul(RECORD_BYTES)?;
    let record = RECORDS.get(start..start.checked_add(RECORD_BYTES)?)?;
    let from = u32::from_le_bytes(record[..4].try_into().ok()?) as usize;
    let to = u32::from_le_bytes(record[4..8].try_into().ok()?) as usize;
    let skip = u16::from_le_bytes(record[8..10].try_into().ok()?) as usize;
    let length = u16::from_le_bytes(record[10..12].try_into().ok()?) as usize;
    if length > MAX_ZONE_BYTES || skip + length > MAX_BLOCK_BYTES {
        return None;
    }
    // Skip neighboring zones without retaining a decoded block in memory.
    let mut decoder = flate2::read::ZlibDecoder::new(ZONES.get(from..to)?);
    if io::copy(&mut decoder.by_ref().take(skip as u64), &mut io::sink()).ok()? != skip as u64 {
        return None;
    }
    let mut data = vec![0; length];
    decoder.read_exact(&mut data).ok()?;
    data.starts_with(b"TZif").then_some(data)
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
            assert!(load(zone).is_some(), "missing TZif data for {zone}");
            previous = zone;
            count += 1;
        }
        assert_eq!(count, 597);
        assert_eq!(load("US/Eastern"), load("America/New_York"));
        assert!(load("Unknown/Timezone").is_none());
    }
}
