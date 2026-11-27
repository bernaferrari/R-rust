//! The pinned datasets database, loaded into the pure world: the same
//! RBG1-frame lazy database the legacy world regenerated during the wire
//! migration, decoded natively — per-object slices inflate through lzma2
//! exactly as the canonical container arranges, and the inflated frames
//! materialize through the flat-graph codec. The inventory is generated
//! from the pinned export; offsets address the database byte image.

use super::heap::Heap;
use super::id::ValueId;
use super::intern::SymbolInterner;

/// The database byte image — the canonical asset, shared with the legacy
/// world's copy rather than duplicated.
pub(crate) const DATABASE: &[u8] = include_bytes!("assets/datasets/Rdata.rdb");

/// Object inventory — name, database offset, slice length — generated
/// from the pinned export exactly as the legacy world's table.
pub(crate) const OBJECTS: &[(&str, u32, u32)] = &[
    ("AirPassengers", 0, 396),
    ("BJsales", 396, 438),
    ("BJsales.lead", 834, 517),
    ("BOD", 1351, 212),
    ("CO2", 1563, 991),
    ("ChickWeight", 2554, 1717),
    ("DNase", 4271, 1323),
    ("EuStockMarkets", 5594, 14474),
    ("Formaldehyde", 20068, 230),
    ("HairEyeColor", 20298, 270),
    ("Harman23.cor", 20568, 499),
    ("Harman74.cor", 21067, 1790),
    ("Indometh", 22857, 746),
    ("InsectSprays", 23603, 319),
    ("JohnsonJohnson", 23922, 387),
    ("LakeHuron", 24309, 403),
    ("LifeCycleSavings", 24712, 1428),
    ("Loblolly", 26140, 1135),
    ("Nile", 27275, 328),
    ("Orange", 27603, 603),
    ("OrchardSprays", 28206, 458),
    ("PlantGrowth", 28664, 345),
    ("Puromycin", 29009, 354),
    ("Seatbelts", 29363, 3637),
    ("Theoph", 33000, 1339),
    ("Titanic", 34339, 295),
    ("ToothGrowth", 34634, 379),
    ("UCBAdmissions", 35013, 274),
    ("UKDriverDeaths", 35287, 495),
    ("UKgas", 35782, 378),
    ("USAccDeaths", 36160, 284),
    ("USArrests", 36444, 1015),
    ("USJudgeRatings", 37459, 1384),
    ("USPersonalExpenditure", 38843, 328),
    ("UScitiesD", 39171, 424),
    ("VADeaths", 39595, 235),
    ("WWWusage", 39830, 272),
    ("WorldPhones", 40102, 331),
    ("ability.cov", 40433, 391),
    ("airmiles", 40824, 170),
    ("airquality", 40994, 1068),
    ("anscombe", 42062, 399),
    ("attenu", 42461, 2033),
    ("attitude", 44494, 555),
    ("austres", 45049, 410),
    ("beaver1", 45459, 717),
    ("beaver2", 46176, 675),
    ("cars", 46851, 261),
    ("chickwts", 47112, 423),
    ("co2", 47535, 1225),
    ("crimtab", 48760, 700),
    ("discoveries", 49460, 196),
    ("esoph", 49656, 583),
    ("euro", 50239, 210),
    ("euro.cross", 50449, 900),
    ("eurodist", 51349, 762),
    ("faithful", 52111, 1152),
    ("fdeaths", 53263, 272),
    ("freeny", 53535, 1461),
    ("freeny.x", 54996, 1150),
    ("freeny.y", 56146, 366),
    ("gait", 56512, 1899),
    ("infert", 58411, 1003),
    ("iris", 59414, 1038),
    ("iris3", 60452, 875),
    ("islands", 61327, 588),
    ("ldeaths", 61915, 273),
    ("lh", 62188, 190),
    ("longley", 62378, 734),
    ("lynx", 63112, 365),
    ("mdeaths", 63477, 272),
    ("morley", 63749, 537),
    ("mtcars", 64286, 1241),
    ("nhtemp", 65527, 239),
    ("nottem", 65766, 599),
    ("npk", 66365, 379),
    ("occupationalStatus", 66744, 273),
    ("penguins", 67017, 2464),
    ("penguins_raw", 69481, 8353),
    ("precip", 77834, 883),
    ("presidents", 78717, 300),
    ("pressure", 79017, 292),
    ("quakes", 79309, 7560),
    ("randu", 86869, 8835),
    ("rivers", 95704, 358),
    ("rock", 96062, 1000),
    ("sleep", 97062, 327),
    ("stack.loss", 97389, 71),
    ("stack.x", 97460, 204),
    ("stackloss", 97664, 296),
    ("state.abb", 97960, 217),
    ("state.area", 98177, 226),
    ("state.center", 98403, 768),
    ("state.division", 99171, 236),
    ("state.name", 99407, 480),
    ("state.region", 99887, 166),
    ("state.x77", 100053, 1642),
    ("sunspot.m2014", 101695, 5521),
    ("sunspot.month", 107216, 6090),
    ("sunspot.year", 113306, 735),
    ("sunspots", 114041, 4950),
    ("swiss", 118991, 1290),
    ("treering", 120281, 13358),
    ("trees", 133639, 379),
    ("uspop", 134018, 184),
    ("volcano", 134202, 2973),
    ("warpbreaks", 137175, 345),
    ("women", 137520, 211),
];

/// Inflate one lazy record: u32 big-endian uncompressed length, the 'Z'
/// lzma2 marker, then the raw lzma2 stream. Plain '0' records pass through
/// exactly as the canonical container's expansion fallback arranges.
fn inflate_record(slice: &[u8]) -> Option<Vec<u8>> {
    let prefix = slice.get(..4)?;
    let expected = u32::from_be_bytes(prefix.try_into().ok()?) as usize;
    let marker = *slice.get(4)?;
    let data = slice.get(5..)?;
    if marker == b'0' {
        return Some(data.get(..expected)?.to_vec());
    }
    if marker != b'Z' {
        return None;
    }
    // The dictionary derives from the expected output length exactly as
    // the canonical reader sizes it — no valid match refers farther back
    // than the complete output.
    let dictionary = expected
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX)
        .clamp(4096, 8 * 1024 * 1024) as u32;
    let mut decoder = lzma_rust2::Lzma2Stream::new(dictionary);
    let mut out: Vec<u8> = Vec::with_capacity(expected);
    let mut chunk = [0u8; 8192];
    let mut input = data;
    loop {
        let result = decoder
            .process(input, &mut chunk, lzma_rust2::Action::Finish)
            .ok()?;
        out.extend_from_slice(&chunk[..result.bytes_produced]);
        if out.len() > expected {
            return None;
        }
        input = &input[result.bytes_consumed..];
        if result.status == lzma_rust2::Status::StreamEnd {
            return if out.len() == expected { Some(out) } else { None };
        }
        if result.bytes_consumed == 0 && result.bytes_produced == 0 {
            return None;
        }
    }
}

/// Load one dataset by name — slice, inflate, materialize. Absent names
/// and corrupt records both answer None; the caller distinguishes with
/// the inventory exactly as GNU's lazyLoad error discipline does.
pub fn load_dataset(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    name: &str,
    default_env: ValueId,
) -> Option<ValueId> {
    let (_, offset, size) = OBJECTS.iter().find(|(object, _, _)| *object == name)?;
    let start = *offset as usize;
    let end = start.checked_add(*size as usize)?;
    let slice = DATABASE.get(start..end)?;
    let inflated = inflate_record(slice)?;
    super::rbg1::decode_rds(heap, interner, &inflated, default_env)
}

/// The object names in inventory order — the data() enumeration exactly.
pub fn dataset_names() -> impl Iterator<Item = &'static str> {
    OBJECTS.iter().map(|(name, _, _)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_session() -> (Heap, SymbolInterner, ValueId) {
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let env = crate::value::session::bootstrap_env(&mut heap, &mut interner);
        (heap, interner, env)
    }

    #[test]
    fn airpassengers_materializes_as_ts() {
        let (mut heap, mut interner, env) = test_session();
        let value = load_dataset(&mut heap, &mut interner, "AirPassengers", env)
            .expect("AirPassengers decodes");
        // GNU: a monthly time series 1949-1960 — 144 real observations
        // with the tsp attribute pinning the series geometry.
        match heap.object(value) {
            Some(crate::value::object::RObject::Real(items)) => {
                assert_eq!(items.len(), 144, "len {}", items.len());
                assert!((items[0] - 112.0).abs() < 1e-9, "first {}", items[0]);
            }
            other => panic!("expected real vector, got {other:?}"),
        }
    }

    #[test]
    fn iris_materializes_as_data_frame() {
        let (mut heap, mut interner, env) = test_session();
        let value = load_dataset(&mut heap, &mut interner, "iris", env)
            .expect("iris decodes");
        // The data frame shape: a list of five columns with names, class
        // and row.names attributes riding the slot chain.
        match heap.object(value) {
            Some(crate::value::object::RObject::List(items)) => {
                assert_eq!(items.len(), 5, "cols {}", items.len());
            }
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn every_inventory_object_decodes() {
        let (mut heap, mut interner, env) = test_session();
        let mut failures = Vec::new();
        for name in dataset_names() {
            if load_dataset(&mut heap, &mut interner, name, env).is_none() {
                failures.push(name);
            }
        }
        assert!(failures.is_empty(), "undecodable: {failures:?}");
    }
}
