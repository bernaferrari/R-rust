//! Original GNU scalar stream tokens, without an interpreter or ambient owner.
#![forbid(unsafe_code)]
use super::{BinaryReader, BinaryWriter};

pub(super) fn cases() -> impl Iterator<Item = (&'static str, f64, &'static str, &'static str)> {
    include_str!("fixtures/ascii-numbers.tsv")
        .lines()
        .skip(1)
        .map(|row| {
            let mut fields = row.split('\t');
            let name = fields.next().unwrap();
            let value = f64::from_bits(u64::from_str_radix(fields.next().unwrap(), 16).unwrap());
            let decimal = fields.next().unwrap();
            let hex = fields.next().unwrap();
            assert!(fields.next().is_none());
            (name, value, decimal, hex)
        })
}

#[test]
fn ascii_writer_integer_na_matches_gnu_token() {
    let mut writer = BinaryWriter::new();
    writer.set_ascii_body(true);
    for value in [i32::MIN, -1, 0, i32::MAX] {
        writer.write_i32(value);
    }
    assert_eq!(writer.into_vec(), b"NA\n-1\n0\n2147483647\n");
}

#[test]
fn ascii_writer_real_tokens_match_independent_gnu_streams() {
    let mut differences = Vec::new();
    let mut count = 0;
    for (name, value, decimal, _) in cases() {
        count += 1;
        let mut writer = BinaryWriter::new();
        writer.set_ascii_body(true);
        writer.write_f64(value);
        let actual = String::from_utf8(writer.into_vec()).unwrap();
        if actual != format!("{decimal}\n") {
            differences.push((name, actual, decimal));
        }
    }
    assert_eq!(count, 43);
    assert!(
        differences.is_empty(),
        "GNU token differences: {differences:?}"
    );
}

#[test]
fn ascii_reader_accepts_original_gnu_hexadecimal_tokens() {
    let mut differences = Vec::new();
    let mut count = 0;
    for (name, value, _, hex) in cases() {
        count += 1;
        let input = format!("{hex}\n");
        let mut reader = BinaryReader::new(input.as_bytes());
        reader.set_ascii_body(true);
        match reader.read_f64() {
            Ok(actual) if actual.to_bits() == value.to_bits() => {}
            // GNU canonicalizes ordinary NaN on input; the NA payload is tested
            // separately by exact bits and must not enter this branch.
            Ok(actual) if name == "nan" && actual.is_nan() => {}
            actual => differences.push((name, actual.map(f64::to_bits), value.to_bits())),
        }
    }
    assert_eq!(count, 43);
    assert!(
        differences.is_empty(),
        "GNU hex read differences: {differences:?}"
    );
}

#[test]
fn ascii_hex_writer_preserves_every_original_gnu_scalar_bit_pattern() {
    let mut count = 0;
    for (name, value, _, hex) in cases() {
        count += 1;
        let mut writer = BinaryWriter::new();
        writer.set_ascii_body(true);
        writer.set_ascii_hex(true);
        writer.write_f64(value);
        assert_eq!(writer.into_vec(), format!("{hex}\n").as_bytes(), "{name}");
    }
    assert_eq!(count, 43);
}

#[test]
fn ascii_hex_input_rounds_ieee_boundaries_to_even() {
    // Independent exact dyadic values: halfway around 1, subnormal zero, the
    // minimum normal number and maximum finite number. No decimal approximation.
    let cases = [
        ("0x1.00000000000008p+0", 0x3ff0000000000000),
        ("0x1.00000000000018p+0", 0x3ff0000000000002),
        ("0x1p-1075", 0),
        ("-0x1p-1075", 1_u64 << 63),
        ("0x3p-1075", 2),
        ("0x0.fffffffffffff8p-1022", 0x0010000000000000),
        ("0x1.fffffffffffff7p+1023", 0x7fefffffffffffff),
        ("0x1.fffffffffffff8p+1023", 0x7ff0000000000000),
    ];
    for (token, bits) in cases {
        assert_eq!(
            super::ascii_numbers::parse_real(token).unwrap().to_bits(),
            bits,
            "{token}"
        );
    }
}

#[test]
fn ascii_numeric_input_bounds_mantissas_and_exponents() {
    for token in [
        "",
        "0x",
        "0x.p0",
        "0x1p",
        "0x1p+-1",
        "0x1..1p0",
        "0x1p1junk",
        "١",
    ] {
        assert!(super::ascii_numbers::parse_real(token).is_err(), "{token}");
    }
    assert!(super::ascii_numbers::parse_real(&"1".repeat(128)).is_err());
    let largest = format!("0x{}p0", "f".repeat(123));
    assert_eq!(largest.len(), 127);
    assert!(
        super::ascii_numbers::parse_real(&largest)
            .unwrap()
            .is_finite()
    );
    for (token, bits) in [
        (
            "0x1p999999999999999999999999999999",
            f64::INFINITY.to_bits(),
        ),
        ("-0x1p-999999999999999999999999999999", 1_u64 << 63),
    ] {
        assert_eq!(
            super::ascii_numbers::parse_real(token).unwrap().to_bits(),
            bits
        );
    }
}

#[test]
fn ascii_reader_rejects_long_tokens_before_copying_the_input() {
    let input = vec![b'1'; 8192];
    let mut reader = BinaryReader::new(&input);
    reader.set_ascii_body(true);
    assert!(reader.read_f64().unwrap_err().contains("127 bytes"));
    assert_eq!(reader.pos, 127);
    let input = b"  0x1p999999999999999999999999999999\n";
    let mut reader = BinaryReader::new(input);
    reader.set_ascii_body(true);
    assert_eq!(reader.read_f64().unwrap(), f64::INFINITY);
}
