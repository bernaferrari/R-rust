//! Pinned GNU R zoneinfo data for hosts without a system zoneinfo directory.

use std::io::Read;

const INDEX: &str = include_str!("zoneinfo/index.txt");
const ZONES: &[u8] = include_bytes!("zoneinfo/zones.bin");
const MAX_ZONE_BYTES: usize = 8192;

pub(crate) fn bundled_zone_names() -> impl Iterator<Item = &'static str> {
    INDEX
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(name, _)| name))
}

pub(super) fn load(zone: &str) -> Option<Vec<u8>> {
    let entry = INDEX.lines().find_map(|line| {
        let (name, entry) = line.split_once('\t')?;
        (zone == name).then_some(entry)
    })?;
    let (offset, length) = entry.split_once('\t')?;
    let offset: usize = offset.parse().ok()?;
    let length: usize = length.parse().ok()?;
    let compressed = ZONES.get(offset..offset.checked_add(length)?)?;
    let mut data = Vec::new();
    flate2::read::ZlibDecoder::new(compressed)
        .take((MAX_ZONE_BYTES + 1) as u64)
        .read_to_end(&mut data)
        .ok()?;
    (data.len() <= MAX_ZONE_BYTES && data.starts_with(b"TZif")).then_some(data)
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
