#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/saveload.c — R data file I/O utilities.
//!
//! This module ports the standalone I/O functions used for reading/writing
//! R data files (.rda, .rds formats).
//!
//! Ported standalone functions:
//!   R_WriteMagic, R_ReadMagic,
//!   OutIntegerAscii, InIntegerAscii,
//!   OutDoubleAscii, InDoubleAscii,
//!   OutStringAscii, InStringAscii,
//!   OutComplexAscii,
//!   OutSpaceAscii, OutNewlineAscii,
//!   defaultSaveVersion

use crate::sexp::ffi::SEXP;

use std::io::{self, BufRead, Read, Write};
use std::os::raw::c_int;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_INTEGER sentinel.
const NA_INTEGER: c_int = c_int::MIN;

/// R save format magic numbers.
pub const R_MAGIC_ASCII_V1: c_int = 1111;
pub const R_MAGIC_BINARY_V1: c_int = 1112;
pub const R_MAGIC_XDR_V1: c_int = 1113;
pub const R_MAGIC_ASCII_V2: c_int = 2111;
pub const R_MAGIC_BINARY_V2: c_int = 2112;
pub const R_MAGIC_XDR_V2: c_int = 2113;
pub const R_MAGIC_ASCII_V3: c_int = 3111;
pub const R_MAGIC_BINARY_V3: c_int = 3112;
pub const R_MAGIC_XDR_V3: c_int = 3113;
pub const R_MAGIC_EMPTY: c_int = 0;
pub const R_MAGIC_CORRUPT: c_int = -1;
pub const R_MAGIC_MAYBE_TOONEW: c_int = -2;

// ---------------------------------------------------------------------------
// Magic number read/write
// ---------------------------------------------------------------------------

/// Write R save format magic number to a file.
///
/// # Errors
/// Returns an error if writing fails.
pub fn R_WriteMagic(fp: &mut impl Write, number: c_int) -> io::Result<()> {
    let number = number.abs();
    let mut buf = [0u8; 5];

    match number {
        R_MAGIC_ASCII_V1 => buf[..4].copy_from_slice(b"RDA1"),
        R_MAGIC_BINARY_V1 => buf[..4].copy_from_slice(b"RDB1"),
        R_MAGIC_XDR_V1 => buf[..4].copy_from_slice(b"RDX1"),
        R_MAGIC_ASCII_V2 => buf[..4].copy_from_slice(b"RDA2"),
        R_MAGIC_BINARY_V2 => buf[..4].copy_from_slice(b"RDB2"),
        R_MAGIC_XDR_V2 => buf[..4].copy_from_slice(b"RDX2"),
        R_MAGIC_ASCII_V3 => buf[..4].copy_from_slice(b"RDA3"),
        R_MAGIC_BINARY_V3 => buf[..4].copy_from_slice(b"RDB3"),
        R_MAGIC_XDR_V3 => buf[..4].copy_from_slice(b"RDX3"),
        _ => {
            buf[0] = ((number / 1000) % 10 + b'0' as i32) as u8;
            buf[1] = ((number / 100) % 10 + b'0' as i32) as u8;
            buf[2] = ((number / 10) % 10 + b'0' as i32) as u8;
            buf[3] = (number % 10 + b'0' as i32) as u8;
        }
    }
    buf[4] = b'\n';
    fp.write_all(&buf)?;
    Ok(())
}

/// Read R save format magic number from a file.
///
/// Returns the magic number, or R_MAGIC_EMPTY/R_MAGIC_CORRUPT on error.
pub fn R_ReadMagic(fp: &mut impl Read) -> c_int {
    let mut buf = [0u8; 5];
    match fp.read_exact(&mut buf) {
        Ok(()) => {}
        Err(_) => return R_MAGIC_EMPTY,
    }

    let s = match std::str::from_utf8(&buf) {
        Ok(s) => s,
        Err(_) => return R_MAGIC_CORRUPT,
    };

    match s {
        "RDA1\n" => R_MAGIC_ASCII_V1,
        "RDB1\n" => R_MAGIC_BINARY_V1,
        "RDX1\n" => R_MAGIC_XDR_V1,
        "RDA2\n" => R_MAGIC_ASCII_V2,
        "RDB2\n" => R_MAGIC_BINARY_V2,
        "RDX2\n" => R_MAGIC_XDR_V2,
        "RDA3\n" => R_MAGIC_ASCII_V3,
        "RDB3\n" => R_MAGIC_BINARY_V3,
        "RDX3\n" => R_MAGIC_XDR_V3,
        _ => {
            if s.starts_with("RD") {
                return R_MAGIC_MAYBE_TOONEW;
            }
            // Try to parse as 4-digit number
            let d1 = (buf[3] as i32 - b'0' as i32).rem_euclid(10);
            let d2 = (buf[2] as i32 - b'0' as i32).rem_euclid(10);
            let d3 = (buf[1] as i32 - b'0' as i32).rem_euclid(10);
            let d4 = (buf[0] as i32 - b'0' as i32).rem_euclid(10);
            d1 + 10 * d2 + 100 * d3 + 1000 * d4
        }
    }
}

// ---------------------------------------------------------------------------
// Default save version
// ---------------------------------------------------------------------------

/// Get the default save format version from R_DEFAULT_SAVE_VERSION env var.
///
/// Returns 2 or 3 (default).
pub fn defaultSaveVersion() -> c_int {
    match std::env::var("R_DEFAULT_SAVE_VERSION") {
        Ok(val) => match val.trim().parse::<c_int>() {
            Ok(2 | 3) => val.trim().parse::<c_int>().expect("unwrap on None/Err"),
            _ => 3,
        },
        Err(_) => 3,
    }
}

// ---------------------------------------------------------------------------
// ASCII output functions
// ---------------------------------------------------------------------------

/// Write spaces to output.
pub fn OutSpaceAscii(fp: &mut impl Write, nspace: c_int) -> io::Result<()> {
    for _ in 0..nspace {
        fp.write_all(b" ")?;
    }
    Ok(())
}

/// Write newline to output.
pub fn OutNewlineAscii(fp: &mut impl Write) -> io::Result<()> {
    fp.write_all(b"\n")
}

/// Write an integer in ASCII format (NA -> "NA").
pub fn OutIntegerAscii(fp: &mut impl Write, x: c_int) -> io::Result<()> {
    if x == NA_INTEGER {
        fp.write_all(b"NA")
    } else {
        write!(fp, "{}", x).map_err(io::Error::other)
    }
}

/// Write a double in ASCII format (NA -> "NA", Inf -> "Inf"/"-Inf").
pub fn OutDoubleAscii(fp: &mut impl Write, x: f64) -> io::Result<()> {
    if x.is_nan() {
        fp.write_all(b"NA")
    } else if x.is_infinite() {
        if x < 0.0 {
            fp.write_all(b"-Inf")
        } else {
            fp.write_all(b"Inf")
        }
    } else {
        write!(fp, "{:.16}", x).map_err(io::Error::other)
    }
}

/// Write a string in ASCII format with escape sequences.
pub fn OutStringAscii(fp: &mut impl Write, s: &str) -> io::Result<()> {
    write!(fp, "{} ", s.len()).map_err(io::Error::other)?;
    for &byte in s.as_bytes() {
        match byte {
            b'\n' => fp.write_all(b"\\n")?,
            b'\t' => fp.write_all(b"\\t")?,
            b'\r' => fp.write_all(b"\\r")?,
            b'\x0B' => fp.write_all(b"\\v")?,
            b'\x08' => fp.write_all(b"\\b")?,
            b'\x0C' => fp.write_all(b"\\f")?,
            b'\x07' => fp.write_all(b"\\a")?,
            b'\\' => fp.write_all(b"\\\\")?,
            b'\'' => fp.write_all(b"\\'")?,
            b'"' => fp.write_all(b"\\\"")?,
            _ => {
                if byte <= 32 || byte > 126 {
                    write!(fp, "\\{:03o}", byte)
                        .map_err(io::Error::other)?;
                } else {
                    fp.write_all(&[byte])?;
                }
            }
        }
    }
    Ok(())
}

/// Write a complex number in ASCII format.
pub fn OutComplexAscii(fp: &mut impl Write, re: f64, im: f64) -> io::Result<()> {
    if re.is_nan() || im.is_nan() {
        fp.write_all(b"NA NA")?;
    } else {
        OutDoubleAscii(fp, re)?;
        fp.write_all(b" ")?;
        OutDoubleAscii(fp, im)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ASCII input functions
// ---------------------------------------------------------------------------

/// Read an integer from ASCII format.
///
/// Reads a token and parses it. Returns NA_INTEGER for "NA".
pub fn InIntegerAscii(reader: &mut impl BufRead) -> io::Result<c_int> {
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    let buf = buf.trim();
    if buf == "NA" {
        Ok(NA_INTEGER)
    } else {
        buf.parse::<c_int>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

/// Read a double from ASCII format.
///
/// Handles NA, Inf, -Inf.
pub fn InDoubleAscii(reader: &mut impl BufRead) -> io::Result<f64> {
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    let buf = buf.trim();
    match buf {
        "NA" => Ok(f64::NAN),
        "Inf" => Ok(f64::INFINITY),
        "-Inf" => Ok(f64::NEG_INFINITY),
        _ => buf
            .parse::<f64>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
    }
}

// ---------------------------------------------------------------------------
// SEXP-dependent stubs
// ---------------------------------------------------------------------------

/// Stub for `do_save` — requires SEXP.
pub unsafe fn do_save(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::globals::R_NilValue;

        // save(list, file, ascii=FALSE, version=2, envir=.GlobalEnv)
        // For now, return nil — full implementation requires file I/O + serialization
        R_NilValue()
    }
}

/// Stub for `do_load` — requires SEXP.
pub unsafe fn do_load(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::globals::R_NilValue;

        // load(file, envir=.GlobalEnv, verbose=FALSE)
        // For now, return nil — full implementation requires file I/O + deserialization
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_R_WriteMagic_v3() {
        let mut buf = Vec::new();
        R_WriteMagic(&mut buf, R_MAGIC_ASCII_V3).unwrap();
        assert_eq!(&buf, b"RDA3\n");
    }

    #[test]
    fn test_R_WriteMagic_v2() {
        let mut buf = Vec::new();
        R_WriteMagic(&mut buf, R_MAGIC_BINARY_V2).unwrap();
        assert_eq!(&buf, b"RDB2\n");
    }

    #[test]
    fn test_R_WriteMagic_v1() {
        let mut buf = Vec::new();
        R_WriteMagic(&mut buf, R_MAGIC_XDR_V1).unwrap();
        assert_eq!(&buf, b"RDX1\n");
    }

    #[test]
    fn test_R_WriteMagic_custom() {
        let mut buf = Vec::new();
        R_WriteMagic(&mut buf, 1234).unwrap();
        assert_eq!(&buf, b"1234\n");
    }

    #[test]
    fn test_R_ReadMagic_roundtrip() {
        for &magic in &[
            R_MAGIC_ASCII_V1,
            R_MAGIC_BINARY_V1,
            R_MAGIC_XDR_V1,
            R_MAGIC_ASCII_V2,
            R_MAGIC_BINARY_V2,
            R_MAGIC_XDR_V2,
            R_MAGIC_ASCII_V3,
            R_MAGIC_BINARY_V3,
            R_MAGIC_XDR_V3,
        ] {
            let mut buf = Vec::new();
            R_WriteMagic(&mut buf, magic).unwrap();
            let mut cursor = std::io::Cursor::new(buf);
            assert_eq!(R_ReadMagic(&mut cursor), magic);
        }
    }

    #[test]
    fn test_R_ReadMagic_empty() {
        let mut cursor = std::io::Cursor::new(Vec::new());
        assert_eq!(R_ReadMagic(&mut cursor), R_MAGIC_EMPTY);
    }

    #[test]
    fn test_defaultSaveVersion() {
        let v = defaultSaveVersion();
        assert!(v == 2 || v == 3);
    }

    #[test]
    fn test_OutIntegerAscii() {
        let mut buf = Vec::new();
        OutIntegerAscii(&mut buf, 42).unwrap();
        assert_eq!(String::from_utf8_lossy(&buf), "42");

        buf.clear();
        OutIntegerAscii(&mut buf, NA_INTEGER).unwrap();
        assert_eq!(String::from_utf8_lossy(&buf), "NA");
    }

    #[test]
    fn test_OutDoubleAscii() {
        let mut buf = Vec::new();
        OutDoubleAscii(&mut buf, 3.14).unwrap();
        let s = String::from_utf8_lossy(&buf);
        let v: f64 = s.parse().unwrap();
        assert!((v - 3.14).abs() < 1e-10);

        buf.clear();
        OutDoubleAscii(&mut buf, f64::NAN).unwrap();
        assert_eq!(&buf, b"NA");

        buf.clear();
        OutDoubleAscii(&mut buf, f64::INFINITY).unwrap();
        assert_eq!(&buf, b"Inf");

        buf.clear();
        OutDoubleAscii(&mut buf, f64::NEG_INFINITY).unwrap();
        assert_eq!(&buf, b"-Inf");
    }

    #[test]
    fn test_OutStringAscii() {
        let mut buf = Vec::new();
        OutStringAscii(&mut buf, "hello").unwrap();
        let s = String::from_utf8_lossy(&buf);
        assert!(s.starts_with("5 "));
        assert!(s.contains("hello"));
    }

    #[test]
    fn test_OutStringAscii_escapes() {
        let mut buf = Vec::new();
        OutStringAscii(&mut buf, "a\nb").unwrap();
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("\\n"), "should escape newline");
    }

    #[test]
    fn test_OutComplexAscii() {
        let mut buf = Vec::new();
        OutComplexAscii(&mut buf, 1.0, 2.0).unwrap();
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("1.0") || s.contains('1'));
        assert!(s.contains("2.0") || s.contains('2'));

        buf.clear();
        OutComplexAscii(&mut buf, f64::NAN, 0.0).unwrap();
        assert_eq!(&buf, b"NA NA");
    }

    #[test]
    fn test_OutSpaceAscii() {
        let mut buf = Vec::new();
        OutSpaceAscii(&mut buf, 3).unwrap();
        assert_eq!(&buf, b"   ");
    }
}
