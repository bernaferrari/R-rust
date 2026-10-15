#![forbid(unsafe_code)]

//! Portable gzip compression through the Rust zlib implementation.
//!
//! GNU R uses a zero timestamp, no filename, and Unix OS byte in its gzip
//! containers. The same safe implementation serves native, Android and Wasm
//! callers without a system library or platform-dependent C structure layout.

use std::io::Write;

/// Compress with GNU's default deflate window, memory level and strategy.
pub(crate) fn gzip_compress(data: &[u8], level: i32) -> Result<Vec<u8>, String> {
    let level = match level {
        -1 => flate2::Compression::default(),
        0..=9 => flate2::Compression::new(level as u32),
        _ => return Err(format!("invalid gzip compression level: {level}")),
    };
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .operating_system(3)
        .write(Vec::new(), level);
    encoder.write_all(data).map_err(|error| error.to_string())?;
    let mut output = encoder.finish().map_err(|error| error.to_string())?;
    // GNU R's gzio.h writes zero xflags at every level, independently of
    // the deflater. flate2 labels the fastest and smallest levels instead.
    *output.get_mut(8).ok_or("incomplete gzip header")? = 0;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn data(kind: &str) -> Vec<u8> {
        match kind {
            "empty" => Vec::new(),
            "text" => "R belongs everywhere.\n\u{e9}\n".as_bytes().to_vec(),
            "bytes" => (0..=255).collect(),
            "window" => (0..100_000)
                .map(|i| ((i * 17 + i / 256) % 256) as u8)
                .collect(),
            _ => panic!("unknown oracle payload"),
        }
    }

    #[test]
    fn portable_gzip_preserves_independent_gnu_headers_and_payloads() {
        for row in include_str!("zlib/gnu-gzip.tsv").lines() {
            let columns: Vec<_> = row.split('\t').collect();
            let level: i32 = columns[1].parse().unwrap();
            let expected: Vec<u8> = columns[2]
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            let input = data(columns[0]);
            let compressed = gzip_compress(&input, level).unwrap();
            assert_eq!(
                &compressed[..10],
                &expected[..10],
                "{} level {level}",
                columns[0]
            );
            // Deflate block choices depend on the compressor implementation.
            // The independent GNU stream and our Rust stream must both
            // preserve the exact original bytes, including empty and raw data.
            for container in [&expected, &compressed] {
                let mut decoded = Vec::new();
                flate2::read::GzDecoder::new(container.as_slice())
                    .read_to_end(&mut decoded)
                    .unwrap();
                assert_eq!(decoded, input, "{} level {level}", columns[0]);
            }
        }
    }

    #[test]
    fn portable_gzip_validates_levels_and_default_matches_six() {
        assert_eq!(
            gzip_compress(b"R", -1).unwrap(),
            gzip_compress(b"R", 6).unwrap()
        );
        for level in [-2, 10, i32::MIN, i32::MAX] {
            assert!(
                gzip_compress(b"R", level)
                    .unwrap_err()
                    .contains("invalid gzip compression level")
            );
        }
    }
}
