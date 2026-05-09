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

use crate::sexp::accessors::{
    ATTRIB, BODY, CAD5R, CADDR, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, INTEGER, LENGTH, LOGICAL,
    PRINTNAME, RAW, REAL, SET_ATTRIB, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETTAG, STRING_ELT,
    TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::attrib_core::{
    R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, R_RowNamesSymbol, getAttrib, setAttrib,
};
use crate::sexp::constructors::{Rf_allocList, Rf_allocVector, Rf_allocVector3, Rf_mkChar};
use crate::sexp::envir::{R_findVarInFrame, defineVar};
use crate::sexp::ffi::{R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NaString, R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::os::raw::c_int;

unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

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
            Ok(2 | 3) => val.trim().parse::<c_int>().unwrap_or(3),
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
                    write!(fp, "\\{:03o}", byte).map_err(io::Error::other)?;
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
        "NA" => Ok(f64::from_bits(R_NA_BIT_PATTERN)),
        "Inf" => Ok(f64::INFINITY),
        "-Inf" => Ok(f64::NEG_INFINITY),
        _ => buf
            .parse::<f64>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
    }
}

fn InComplexAscii(reader: &mut impl BufRead) -> io::Result<Rcomplex> {
    let line = read_ascii_token(reader)?;
    let mut parts = line.split_whitespace();
    let re = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing complex real part"))?;
    let im = parts.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing complex imaginary part")
    })?;
    if parts.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "malformed complex value",
        ));
    }
    Ok(Rcomplex {
        r: parse_ascii_double_token(re)?,
        i: parse_ascii_double_token(im)?,
    })
}

fn parse_ascii_double_token(token: &str) -> io::Result<f64> {
    match token {
        "NA" => Ok(f64::from_bits(R_NA_BIT_PATTERN)),
        "Inf" => Ok(f64::INFINITY),
        "-Inf" => Ok(f64::NEG_INFINITY),
        _ => token
            .parse::<f64>()
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err)),
    }
}

fn read_ascii_token(reader: &mut impl BufRead) -> io::Result<String> {
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

fn InStringAscii(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let line = read_ascii_token(reader)?;
    if line == "NA" {
        return Ok(None);
    }
    let Some((len, encoded)) = line.split_once(' ') else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "malformed ASCII string",
        ));
    };
    let expected_len = len
        .parse::<usize>()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut bytes = Vec::with_capacity(expected_len);
    let mut chars = encoded.bytes();
    while let Some(byte) = chars.next() {
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        match chars.next() {
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(b'r') => bytes.push(b'\r'),
            Some(b'v') => bytes.push(b'\x0B'),
            Some(b'b') => bytes.push(b'\x08'),
            Some(b'f') => bytes.push(b'\x0C'),
            Some(b'a') => bytes.push(b'\x07'),
            Some(b'\\') => bytes.push(b'\\'),
            Some(b'\'') => bytes.push(b'\''),
            Some(b'"') => bytes.push(b'"'),
            Some(first @ b'0'..=b'7') => {
                let second = chars.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "truncated octal escape")
                })?;
                let third = chars.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "truncated octal escape")
                })?;
                let oct = [first, second, third];
                let text = std::str::from_utf8(&oct)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                let value = u8::from_str_radix(text, 8)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                bytes.push(value);
            }
            Some(other) => bytes.push(other),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "trailing string escape",
                ));
            }
        }
    }
    if bytes.len() != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ASCII string length mismatch",
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

unsafe fn write_saved_object(writer: &mut impl Write, value: SEXP) -> io::Result<()> {
    unsafe {
        if value.is_null() {
            OutIntegerAscii(writer, SEXPTYPE::NILSXP.as_c_int())?;
            OutNewlineAscii(writer)?;
            return write_attribute_list(writer, R_NilValue());
        }
        if value == R_MissingArg() {
            OutIntegerAscii(writer, SEXPTYPE::ANYSXP.as_c_int())?;
            OutNewlineAscii(writer)?;
            OutIntegerAscii(writer, 0)?;
            OutNewlineAscii(writer)?;
            return Ok(());
        }
        let sexptype = TYPEOF(value);
        OutIntegerAscii(writer, sexptype)?;
        OutNewlineAscii(writer)?;

        match SEXPTYPE::from(sexptype) {
            SEXPTYPE::NILSXP => {
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::SYMSXP => {
                let name = PRINTNAME(value);
                if name.is_null() || name == R_NilValue() {
                    OutStringAscii(writer, "")?;
                } else {
                    let text = std::ffi::CStr::from_ptr(CHAR(name))
                        .to_str()
                        .map_err(io::Error::other)?;
                    OutStringAscii(writer, text)?;
                }
                OutNewlineAscii(writer)?;
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::CLOSXP => {
                write_saved_object(writer, FORMALS(value))?;
                write_saved_object(writer, BODY(value))?;
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                let len = pairlist_length(value);
                OutIntegerAscii(writer, len)?;
                OutNewlineAscii(writer)?;
                let mut current = value;
                while !current.is_null() && current != R_NilValue() {
                    write_saved_object(writer, TAG(current))?;
                    write_saved_object(writer, CAR(current))?;
                    current = CDR(current);
                }
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::LGLSXP => {
                let len = XLENGTH(value);
                OutIntegerAscii(writer, len as c_int)?;
                OutNewlineAscii(writer)?;
                for i in 0..len as usize {
                    OutIntegerAscii(writer, *LOGICAL(value).add(i))?;
                    OutNewlineAscii(writer)?;
                }
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::INTSXP => {
                let len = XLENGTH(value);
                OutIntegerAscii(writer, len as c_int)?;
                OutNewlineAscii(writer)?;
                for i in 0..len as usize {
                    OutIntegerAscii(writer, *INTEGER(value).add(i))?;
                    OutNewlineAscii(writer)?;
                }
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::REALSXP => {
                let len = XLENGTH(value);
                OutIntegerAscii(writer, len as c_int)?;
                OutNewlineAscii(writer)?;
                for i in 0..len as usize {
                    OutDoubleAscii(writer, *REAL(value).add(i))?;
                    OutNewlineAscii(writer)?;
                }
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::CPLXSXP => {
                let len = XLENGTH(value);
                OutIntegerAscii(writer, len as c_int)?;
                OutNewlineAscii(writer)?;
                for i in 0..len as usize {
                    let element = *COMPLEX(value).add(i);
                    OutComplexAscii(writer, element.r, element.i)?;
                    OutNewlineAscii(writer)?;
                }
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::STRSXP => {
                let len = XLENGTH(value);
                OutIntegerAscii(writer, len as c_int)?;
                OutNewlineAscii(writer)?;
                for i in 0..len {
                    let charsxp = STRING_ELT(value, i);
                    if charsxp.is_null() || charsxp == R_NaString() {
                        writer.write_all(b"NA\n")?;
                    } else {
                        let text = std::ffi::CStr::from_ptr(CHAR(charsxp))
                            .to_str()
                            .map_err(io::Error::other)?;
                        OutStringAscii(writer, text)?;
                        OutNewlineAscii(writer)?;
                    }
                }
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::RAWSXP => {
                let len = XLENGTH(value);
                OutIntegerAscii(writer, len as c_int)?;
                OutNewlineAscii(writer)?;
                for i in 0..len as usize {
                    OutIntegerAscii(writer, *RAW(value).add(i) as c_int)?;
                    OutNewlineAscii(writer)?;
                }
                write_attribute_list(writer, value)?;
                Ok(())
            }
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
                let len = XLENGTH(value);
                OutIntegerAscii(writer, len as c_int)?;
                OutNewlineAscii(writer)?;
                for i in 0..len {
                    write_saved_object(writer, VECTOR_ELT(value, i))?;
                }
                write_attribute_list(writer, value)?;
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported saved object type {sexptype}"),
            )),
        }
    }
}

unsafe fn pairlist_length(value: SEXP) -> c_int {
    unsafe {
        let mut len = 0;
        let mut current = value;
        while !current.is_null() && current != R_NilValue() {
            len += 1;
            current = CDR(current);
        }
        len
    }
}

unsafe fn write_attribute_list(writer: &mut impl Write, value: SEXP) -> io::Result<()> {
    unsafe {
        let attrs = ATTRIB(value);
        if attrs.is_null() || attrs == R_NilValue() {
            OutIntegerAscii(writer, 0)?;
            OutNewlineAscii(writer)?;
            return Ok(());
        }

        let mut entries = Vec::new();
        let mut current = attrs;
        while !current.is_null() && current != R_NilValue() {
            let tag = TAG(current);
            let car = CAR(current);
            if is_serializable_saved_object(tag) && is_serializable_saved_object(car) {
                entries.push((tag, car));
            }
            current = CDR(current);
        }

        if entries.is_empty() {
            OutIntegerAscii(writer, 0)?;
            OutNewlineAscii(writer)?;
            return Ok(());
        }

        OutIntegerAscii(writer, 1)?;
        OutNewlineAscii(writer)?;
        OutIntegerAscii(writer, SEXPTYPE::LISTSXP.as_c_int())?;
        OutNewlineAscii(writer)?;
        OutIntegerAscii(writer, entries.len() as c_int)?;
        OutNewlineAscii(writer)?;
        for (tag, car) in entries.into_iter().rev() {
            write_saved_object(writer, tag)?;
            write_saved_object(writer, car)?;
        }
        OutIntegerAscii(writer, 0)?;
        OutNewlineAscii(writer)
    }
}

unsafe fn is_serializable_saved_object(value: SEXP) -> bool {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return true;
        }
        match SEXPTYPE::from(TYPEOF(value)) {
            SEXPTYPE::NILSXP
            | SEXPTYPE::SYMSXP
            | SEXPTYPE::CLOSXP
            | SEXPTYPE::ANYSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::RAWSXP => true,
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
                let len = XLENGTH(value);
                (0..len).all(|i| is_serializable_saved_object(VECTOR_ELT(value, i)))
            }
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                let mut current = value;
                while !current.is_null() && current != R_NilValue() {
                    if !is_serializable_saved_object(TAG(current))
                        || !is_serializable_saved_object(CAR(current))
                    {
                        return false;
                    }
                    current = CDR(current);
                }
                true
            }
            _ => false,
        }
    }
}

unsafe fn write_names_attr(writer: &mut impl Write, value: SEXP) -> io::Result<()> {
    unsafe {
        let names = getAttrib(value, R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            OutIntegerAscii(writer, 0)?;
            OutNewlineAscii(writer)?;
            return Ok(());
        }

        OutIntegerAscii(writer, 1)?;
        OutNewlineAscii(writer)?;
        let len = XLENGTH(names);
        OutIntegerAscii(writer, len as c_int)?;
        OutNewlineAscii(writer)?;
        for i in 0..len {
            let charsxp = STRING_ELT(names, i);
            if charsxp.is_null() || charsxp == R_NaString() {
                writer.write_all(b"NA\n")?;
            } else {
                let text = std::ffi::CStr::from_ptr(CHAR(charsxp))
                    .to_str()
                    .map_err(io::Error::other)?;
                OutStringAscii(writer, text)?;
                OutNewlineAscii(writer)?;
            }
        }
        Ok(())
    }
}

unsafe fn write_dim_attr(writer: &mut impl Write, value: SEXP) -> io::Result<()> {
    unsafe {
        let dim = getAttrib(value, R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP {
            OutIntegerAscii(writer, 0)?;
            OutNewlineAscii(writer)?;
            return Ok(());
        }

        OutIntegerAscii(writer, 1)?;
        OutNewlineAscii(writer)?;
        let len = XLENGTH(dim);
        OutIntegerAscii(writer, len as c_int)?;
        OutNewlineAscii(writer)?;
        for i in 0..len as usize {
            OutIntegerAscii(writer, *INTEGER(dim).add(i))?;
            OutNewlineAscii(writer)?;
        }
        Ok(())
    }
}

unsafe fn write_dimnames_attr(writer: &mut impl Write, value: SEXP) -> io::Result<()> {
    unsafe {
        let dimnames = getAttrib(value, R_DimNamesSymbol());
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            OutIntegerAscii(writer, 0)?;
            OutNewlineAscii(writer)?;
            return Ok(());
        }

        OutIntegerAscii(writer, 1)?;
        OutNewlineAscii(writer)?;
        let len = XLENGTH(dimnames);
        OutIntegerAscii(writer, len as c_int)?;
        OutNewlineAscii(writer)?;
        for i in 0..len {
            let elt = VECTOR_ELT(dimnames, i);
            if elt.is_null() || elt == R_NilValue() {
                OutIntegerAscii(writer, 0)?;
                OutNewlineAscii(writer)?;
            } else if TYPEOF(elt) == SEXPTYPE::STRSXP {
                OutIntegerAscii(writer, 1)?;
                OutNewlineAscii(writer)?;
                write_string_vector_payload(writer, elt)?;
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsupported dimnames element",
                ));
            }
        }
        Ok(())
    }
}

unsafe fn write_string_attr(writer: &mut impl Write, value: SEXP, symbol: SEXP) -> io::Result<()> {
    unsafe {
        let attr = getAttrib(value, symbol);
        if attr.is_null() || attr == R_NilValue() || TYPEOF(attr) != SEXPTYPE::STRSXP {
            OutIntegerAscii(writer, 0)?;
            OutNewlineAscii(writer)?;
            return Ok(());
        }

        OutIntegerAscii(writer, 1)?;
        OutNewlineAscii(writer)?;
        write_string_vector_payload(writer, attr)
    }
}

unsafe fn write_real_attr(writer: &mut impl Write, value: SEXP, symbol: SEXP) -> io::Result<()> {
    unsafe {
        let attr = getAttrib(value, symbol);
        if attr.is_null() || attr == R_NilValue() || TYPEOF(attr) != SEXPTYPE::REALSXP {
            OutIntegerAscii(writer, 0)?;
            OutNewlineAscii(writer)?;
            return Ok(());
        }

        OutIntegerAscii(writer, 1)?;
        OutNewlineAscii(writer)?;
        let len = XLENGTH(attr);
        OutIntegerAscii(writer, len as c_int)?;
        OutNewlineAscii(writer)?;
        for i in 0..len as usize {
            OutDoubleAscii(writer, *REAL(attr).add(i))?;
            OutNewlineAscii(writer)?;
        }
        Ok(())
    }
}

unsafe fn write_row_names_attr(writer: &mut impl Write, value: SEXP) -> io::Result<()> {
    unsafe {
        let attr = getAttrib(value, R_RowNamesSymbol());
        if attr.is_null() || attr == R_NilValue() {
            OutIntegerAscii(writer, 0)?;
            OutNewlineAscii(writer)?;
            return Ok(());
        }

        match SEXPTYPE::from(TYPEOF(attr)) {
            SEXPTYPE::INTSXP => {
                OutIntegerAscii(writer, SEXPTYPE::INTSXP.as_c_int())?;
                OutNewlineAscii(writer)?;
                let len = XLENGTH(attr);
                OutIntegerAscii(writer, len as c_int)?;
                OutNewlineAscii(writer)?;
                for i in 0..len as usize {
                    OutIntegerAscii(writer, *INTEGER(attr).add(i))?;
                    OutNewlineAscii(writer)?;
                }
            }
            SEXPTYPE::STRSXP => {
                OutIntegerAscii(writer, SEXPTYPE::STRSXP.as_c_int())?;
                OutNewlineAscii(writer)?;
                write_string_vector_payload(writer, attr)?;
            }
            _ => {
                OutIntegerAscii(writer, 0)?;
                OutNewlineAscii(writer)?;
            }
        }
        Ok(())
    }
}

unsafe fn write_string_vector_payload(writer: &mut impl Write, value: SEXP) -> io::Result<()> {
    unsafe {
        let len = XLENGTH(value);
        OutIntegerAscii(writer, len as c_int)?;
        OutNewlineAscii(writer)?;
        for i in 0..len {
            let charsxp = STRING_ELT(value, i);
            if charsxp.is_null() || charsxp == R_NaString() {
                writer.write_all(b"NA\n")?;
            } else {
                let text = std::ffi::CStr::from_ptr(CHAR(charsxp))
                    .to_str()
                    .map_err(io::Error::other)?;
                OutStringAscii(writer, text)?;
                OutNewlineAscii(writer)?;
            }
        }
        Ok(())
    }
}

unsafe fn read_saved_object(reader: &mut impl BufRead) -> io::Result<SEXP> {
    unsafe {
        let sexptype = InIntegerAscii(reader)?;
        let value = match SEXPTYPE::from(sexptype) {
            SEXPTYPE::NILSXP => Ok(R_NilValue()),
            SEXPTYPE::ANYSXP => Ok(R_MissingArg()),
            SEXPTYPE::SYMSXP => {
                let name = InStringAscii(reader)?.unwrap_or_default();
                let cstr = std::ffi::CString::new(name).map_err(io::Error::other)?;
                Ok(Rf_install(cstr.as_ptr()))
            }
            SEXPTYPE::CLOSXP => {
                let formals = read_saved_object(reader)?;
                let _formals_guard = protect(formals);
                let body = read_saved_object(reader)?;
                let _body_guard = protect(body);
                Ok(crate::mainutils::dstruct::mkCLOSXP(
                    formals,
                    body,
                    crate::sexp::globals::R_GlobalEnv(),
                ))
            }
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                let len = InIntegerAscii(reader)?;
                let value = Rf_allocList(len);
                let _guard = protect(value);
                let mut current = value;
                for _ in 0..len {
                    if SEXPTYPE::from(sexptype) == SEXPTYPE::LANGSXP {
                        (*current).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                    }
                    let tag = read_saved_object(reader)?;
                    let car = read_saved_object(reader)?;
                    SETTAG(current, tag);
                    SETCAR(current, car);
                    current = CDR(current);
                }
                Ok(value)
            }
            SEXPTYPE::LGLSXP => {
                let len = InIntegerAscii(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::LGLSXP, len as R_xlen_t);
                for i in 0..len as usize {
                    *LOGICAL(value).add(i) = InIntegerAscii(reader)?;
                }
                Ok(value)
            }
            SEXPTYPE::INTSXP => {
                let len = InIntegerAscii(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::INTSXP, len as R_xlen_t);
                for i in 0..len as usize {
                    *INTEGER(value).add(i) = InIntegerAscii(reader)?;
                }
                Ok(value)
            }
            SEXPTYPE::REALSXP => {
                let len = InIntegerAscii(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::REALSXP, len as R_xlen_t);
                for i in 0..len as usize {
                    *REAL(value).add(i) = InDoubleAscii(reader)?;
                }
                Ok(value)
            }
            SEXPTYPE::CPLXSXP => {
                let len = InIntegerAscii(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::CPLXSXP, len as R_xlen_t);
                for i in 0..len as usize {
                    *COMPLEX(value).add(i) = InComplexAscii(reader)?;
                }
                Ok(value)
            }
            SEXPTYPE::STRSXP => {
                let len = InIntegerAscii(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
                for i in 0..len as R_xlen_t {
                    match InStringAscii(reader)? {
                        Some(text) => {
                            let cstr = std::ffi::CString::new(text).map_err(io::Error::other)?;
                            SET_STRING_ELT(value, i, Rf_mkChar(cstr.as_ptr()));
                        }
                        None => SET_STRING_ELT(value, i, R_NaString()),
                    }
                }
                Ok(value)
            }
            SEXPTYPE::RAWSXP => {
                let len = InIntegerAscii(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::RAWSXP, len as R_xlen_t);
                for i in 0..len as usize {
                    let byte = InIntegerAscii(reader)?;
                    if !(0..=u8::MAX as c_int).contains(&byte) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "raw byte out of range",
                        ));
                    }
                    *RAW(value).add(i) = byte as u8;
                }
                Ok(value)
            }
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
                let len = InIntegerAscii(reader)?;
                let value = Rf_allocVector3(SEXPTYPE(sexptype), len as R_xlen_t);
                let _guard = protect(value);
                for i in 0..len as R_xlen_t {
                    let element = read_saved_object(reader)?;
                    SET_VECTOR_ELT(value, i, element);
                }
                Ok(value)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported saved object type {sexptype}"),
            )),
        }?;
        read_attribute_list(reader, value)?;
        Ok(value)
    }
}

unsafe fn read_attribute_list(reader: &mut impl BufRead, value: SEXP) -> io::Result<()> {
    unsafe {
        let has_attrs = InIntegerAscii(reader)?;
        if has_attrs == 0 {
            return Ok(());
        }
        let attrs = read_saved_object(reader)?;
        if attrs.is_null() || attrs == R_NilValue() {
            SET_ATTRIB(value, R_NilValue());
        } else if TYPEOF(attrs) == SEXPTYPE::LISTSXP {
            SET_ATTRIB(value, attrs);
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "saved attribute payload is not a pairlist",
            ));
        }
        Ok(())
    }
}

unsafe fn read_names_attr(reader: &mut impl BufRead, value: SEXP) -> io::Result<()> {
    unsafe {
        let has_names = InIntegerAscii(reader)?;
        if has_names == 0 {
            return Ok(());
        }
        let len = InIntegerAscii(reader)?;
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
        let _guard = protect(names);
        for i in 0..len as R_xlen_t {
            match InStringAscii(reader)? {
                Some(text) => {
                    let cstr = std::ffi::CString::new(text).map_err(io::Error::other)?;
                    SET_STRING_ELT(names, i, Rf_mkChar(cstr.as_ptr()));
                }
                None => SET_STRING_ELT(names, i, R_NaString()),
            }
        }
        setAttrib(value, R_NamesSymbol(), names);
        Ok(())
    }
}

unsafe fn read_dim_attr(reader: &mut impl BufRead, value: SEXP) -> io::Result<()> {
    unsafe {
        let has_dim = InIntegerAscii(reader)?;
        if has_dim == 0 {
            return Ok(());
        }
        let len = InIntegerAscii(reader)?;
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, len as R_xlen_t);
        let _guard = protect(dim);
        for i in 0..len as usize {
            *INTEGER(dim).add(i) = InIntegerAscii(reader)?;
        }
        setAttrib(value, R_DimSymbol(), dim);
        Ok(())
    }
}

unsafe fn read_dimnames_attr(reader: &mut impl BufRead, value: SEXP) -> io::Result<()> {
    unsafe {
        let has_dimnames = InIntegerAscii(reader)?;
        if has_dimnames == 0 {
            return Ok(());
        }
        let len = InIntegerAscii(reader)?;
        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, len as R_xlen_t);
        let _guard = protect(dimnames);
        for i in 0..len as R_xlen_t {
            let has_elt = InIntegerAscii(reader)?;
            if has_elt == 0 {
                SET_VECTOR_ELT(dimnames, i, R_NilValue());
            } else {
                let elt = read_string_vector_payload(reader)?;
                SET_VECTOR_ELT(dimnames, i, elt);
            }
        }
        setAttrib(value, R_DimNamesSymbol(), dimnames);
        Ok(())
    }
}

unsafe fn read_string_attr(reader: &mut impl BufRead, value: SEXP, symbol: SEXP) -> io::Result<()> {
    unsafe {
        let has_attr = InIntegerAscii(reader)?;
        if has_attr == 0 {
            return Ok(());
        }
        let attr = read_string_vector_payload(reader)?;
        let _guard = protect(attr);
        setAttrib(value, symbol, attr);
        Ok(())
    }
}

unsafe fn read_real_attr(reader: &mut impl BufRead, value: SEXP, symbol: SEXP) -> io::Result<()> {
    unsafe {
        let has_attr = InIntegerAscii(reader)?;
        if has_attr == 0 {
            return Ok(());
        }
        let len = InIntegerAscii(reader)?;
        let attr = Rf_allocVector3(SEXPTYPE::REALSXP, len as R_xlen_t);
        let _guard = protect(attr);
        for i in 0..len as usize {
            *REAL(attr).add(i) = InDoubleAscii(reader)?;
        }
        setAttrib(value, symbol, attr);
        Ok(())
    }
}

unsafe fn read_row_names_attr(reader: &mut impl BufRead, value: SEXP) -> io::Result<()> {
    unsafe {
        let attr_type = InIntegerAscii(reader)?;
        match SEXPTYPE::from(attr_type) {
            SEXPTYPE::NILSXP => Ok(()),
            SEXPTYPE::INTSXP => {
                let len = InIntegerAscii(reader)?;
                let attr = Rf_allocVector3(SEXPTYPE::INTSXP, len as R_xlen_t);
                let _guard = protect(attr);
                for i in 0..len as usize {
                    *INTEGER(attr).add(i) = InIntegerAscii(reader)?;
                }
                setAttrib(value, R_RowNamesSymbol(), attr);
                Ok(())
            }
            SEXPTYPE::STRSXP => {
                let attr = read_string_vector_payload(reader)?;
                let _guard = protect(attr);
                setAttrib(value, R_RowNamesSymbol(), attr);
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported row.names attribute type",
            )),
        }
    }
}

unsafe fn read_string_vector_payload(reader: &mut impl BufRead) -> io::Result<SEXP> {
    unsafe {
        let len = InIntegerAscii(reader)?;
        let value = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
        let _guard = protect(value);
        for i in 0..len as R_xlen_t {
            match InStringAscii(reader)? {
                Some(text) => {
                    let cstr = std::ffi::CString::new(text).map_err(io::Error::other)?;
                    SET_STRING_ELT(value, i, Rf_mkChar(cstr.as_ptr()));
                }
                None => SET_STRING_ELT(value, i, R_NaString()),
            }
        }
        Ok(value)
    }
}

unsafe fn tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(tag);
        if printname.is_null() {
            return None;
        }
        let ptr = CHAR(printname);
        if ptr.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

unsafe fn arg_by_name_or_position(args: SEXP, name: &str, position: usize) -> SEXP {
    unsafe {
        let mut current = args;
        let mut index = 0;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some(name) {
                return CAR(current);
            }
            if index == position && tag_name(current).is_none() {
                return CAR(current);
            }
            current = CDR(current);
            index += 1;
        }
        R_NilValue()
    }
}

unsafe fn eval_named_arg(args: SEXP, rho: SEXP, name: &str) -> SEXP {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some(name) {
                return crate::eval::eval::Rf_eval(CAR(current), rho);
            }
            current = CDR(current);
        }
        R_NilValue()
    }
}

unsafe fn collect_save_object_names(args: SEXP, rho: SEXP) -> Vec<String> {
    unsafe {
        let mut names = Vec::new();
        let explicit_list = eval_named_arg(args, rho, "list");
        if !explicit_list.is_null() && explicit_list != R_NilValue() {
            names.extend(names_from_save_list(explicit_list));
        }

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                let expr = CAR(current);
                if !expr.is_null() && TYPEOF(expr) == SEXPTYPE::SYMSXP {
                    let printname = PRINTNAME(expr);
                    if !printname.is_null() {
                        let ptr = CHAR(printname);
                        if !ptr.is_null() {
                            names
                                .push(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned());
                        }
                    }
                } else {
                    error("save() only supports named objects in this runtime");
                }
            }
            current = CDR(current);
        }

        names
    }
}

unsafe fn names_from_save_list(list: SEXP) -> Vec<String> {
    unsafe {
        if TYPEOF(list) != SEXPTYPE::STRSXP {
            error("'list' must be a character vector");
        }
        let mut names = Vec::new();
        for i in 0..XLENGTH(list) {
            let charsxp = STRING_ELT(list, i);
            if charsxp.is_null() || charsxp == R_NaString() {
                error("'list' contains missing object names");
            }
            names.push(
                std::ffi::CStr::from_ptr(CHAR(charsxp))
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        names
    }
}

unsafe fn string_vector_from_names(names: &[String]) -> SEXP {
    unsafe {
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        for (i, name) in names.iter().enumerate() {
            let c_name = match std::ffi::CString::new(name.as_str()) {
                Ok(name) => name,
                Err(_) => error("invalid object name"),
            };
            SET_STRING_ELT(out, i as R_xlen_t, Rf_mkChar(c_name.as_ptr()));
        }
        out
    }
}

unsafe fn save_ascii_objects(list: SEXP, file_sexp: SEXP, ascii_flag: SEXP, envir: SEXP) -> SEXP {
    unsafe {
        if file_sexp.is_null() {
            error("'file' must be non-empty string");
        }
        let charsxp = STRING_ELT(file_sexp, 0);
        if charsxp.is_null() {
            error("'file' must be non-empty string");
        }
        let cstr = CHAR(charsxp);
        if cstr.is_null() {
            error("'file' must be non-empty string");
        }
        let file_path = match std::ffi::CStr::from_ptr(cstr).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => error("'file' must be non-empty string"),
        };

        let use_ascii = crate::mainutils::coerce::asInteger(ascii_flag) != 0;
        if !use_ascii {
            error("only ASCII save files are supported by this Rust runtime");
        }

        let file = match std::fs::File::create(&file_path) {
            Ok(f) => f,
            Err(_) => error("cannot open file"),
        };
        let mut writer = BufWriter::new(file);

        let _ = R_WriteMagic(&mut writer, R_MAGIC_ASCII_V3);

        let n = if list.is_null() {
            0
        } else {
            LENGTH(list) as i32
        };
        let _ = OutIntegerAscii(&mut writer, n);
        let _ = OutNewlineAscii(&mut writer);

        for i in 0..n as i64 {
            let name_charsxp = STRING_ELT(list, i);
            if name_charsxp.is_null() {
                continue;
            }
            let name = CHAR(name_charsxp);
            if name.is_null() {
                continue;
            }
            let name_str = match std::ffi::CStr::from_ptr(name).to_str() {
                Ok(s) => s,
                Err(_) => continue,
            };

            let _ = OutStringAscii(&mut writer, name_str);
            let _ = OutNewlineAscii(&mut writer);

            let sym = Rf_install(name);
            let value = R_findVarInFrame(envir, sym);
            if value == R_UnboundValue() {
                error(&format!("object '{}' not found", name_str));
            }
            if write_saved_object(&mut writer, value).is_err() {
                error("save failed to serialize object");
            }
        }

        let _ = writer.flush();
        R_NilValue()
    }
}

unsafe fn load_ascii_objects(file_sexp: SEXP, envir: SEXP) -> SEXP {
    unsafe {
        if file_sexp.is_null() {
            error("first argument must be a file name");
        }
        let charsxp = STRING_ELT(file_sexp, 0);
        if charsxp.is_null() {
            error("first argument must be a file name");
        }
        let cstr = CHAR(charsxp);
        if cstr.is_null() {
            error("first argument must be a file name");
        }
        let file_path = match std::ffi::CStr::from_ptr(cstr).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => error("first argument must be a file name"),
        };

        let file = match std::fs::File::open(&file_path) {
            Ok(f) => f,
            Err(_) => error("unable to open file"),
        };
        let mut reader = BufReader::new(file);

        let magic = R_ReadMagic(&mut reader);
        if magic == R_MAGIC_EMPTY || magic == R_MAGIC_CORRUPT {
            error("bad restore file magic number (file may be corrupted) -- no data loaded");
        }

        let n = match InIntegerAscii(&mut reader) {
            Ok(v) => v,
            Err(_) => error("a read error occurred"),
        };

        if n <= 0 {
            error("restore file may be empty -- no data loaded");
        }

        let names = Rf_allocVector(crate::sexp::ffi::SEXPTYPE::STRSXP.as_c_int(), n);
        let _names_guard = protect(names);

        for i in 0..n as i64 {
            let name = match InStringAscii(&mut reader) {
                Ok(Some(name)) => name,
                _ => error("a read error occurred"),
            };
            let value = match read_saved_object(&mut reader) {
                Ok(value) => value,
                Err(_) => error("a read error occurred"),
            };
            let c_name = match std::ffi::CString::new(name.as_str()) {
                Ok(name) => name,
                Err(_) => error("a read error occurred"),
            };
            defineVar(Rf_install(c_name.as_ptr()), value, envir);

            SET_STRING_ELT(names, i, Rf_mkChar(c_name.as_ptr()));
        }

        names
    }
}

// ---------------------------------------------------------------------------
// SEXP-dependent stubs
// ---------------------------------------------------------------------------

/// Write R objects to a file in ASCII format.
///
/// Port of `do_save` from saveload.c. Serializes named objects from an environment
/// to a file using R's ASCII save format (version 3).
///
/// Supports basic types: NILSXP, LGLSXP, INTSXP, REALSXP, STRSXP, VECSXP,
/// EXPRSXP.
/// Complex types (closures, environments, etc.) return an error gracefully.
pub unsafe fn do_save(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(_op, args);

        let list = CAR(args);
        let file_sexp = CADR(args);
        let ascii_flag = CADDR(args);
        let envir = CAD5R(args);

        save_ascii_objects(list, file_sexp, ascii_flag, envir)
    }
}

/// User-facing `save()` wrapper for the evaluated `list=`, `file=`,
/// `ascii=`, and `envir=` path used by pure-R Android sessions.
pub unsafe fn do_save_user(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let names = collect_save_object_names(args, rho);
        if names.is_empty() {
            error("nothing specified to be save()d");
        }
        let list = string_vector_from_names(&names);
        let _list_guard = protect(list);

        let file = eval_named_arg(args, rho, "file");
        let ascii = eval_named_arg(args, rho, "ascii");
        let envir = {
            let candidate = eval_named_arg(args, rho, "envir");
            if candidate.is_null() || candidate == R_NilValue() {
                rho
            } else {
                candidate
            }
        };
        if ascii.is_null() || ascii == R_NilValue() {
            error("save() requires ascii = TRUE in this runtime");
        }
        save_ascii_objects(list, file, ascii, envir)
    }
}

/// Read R objects from a file.
///
/// Port of `do_load` from saveload.c. Deserializes objects from a file
/// using R's ASCII save format.
///
/// Supports basic types: NILSXP, LGLSXP, INTSXP, REALSXP, STRSXP, VECSXP,
/// EXPRSXP.
/// Returns a character vector of loaded object names.
pub unsafe fn do_load(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(_op, args);

        let file_sexp = CAR(args);
        let envir = CADR(args);
        let _verbose = CADDR(args);

        load_ascii_objects(file_sexp, envir)
    }
}

/// User-facing `load()` wrapper.
pub unsafe fn do_load_user(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file = arg_by_name_or_position(args, "file", 0);
        let envir = {
            let candidate = arg_by_name_or_position(args, "envir", 1);
            if candidate.is_null() || candidate == R_NilValue() {
                rho
            } else {
                candidate
            }
        };
        load_ascii_objects(file, envir)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test failed: {e:?}"),
        }
    }

    #[test]
    fn test_R_WriteMagic_v3() {
        let mut buf = Vec::new();
        must(R_WriteMagic(&mut buf, R_MAGIC_ASCII_V3));
        assert_eq!(&buf, b"RDA3\n");
    }

    #[test]
    fn test_R_WriteMagic_v2() {
        let mut buf = Vec::new();
        must(R_WriteMagic(&mut buf, R_MAGIC_BINARY_V2));
        assert_eq!(&buf, b"RDB2\n");
    }

    #[test]
    fn test_R_WriteMagic_v1() {
        let mut buf = Vec::new();
        must(R_WriteMagic(&mut buf, R_MAGIC_XDR_V1));
        assert_eq!(&buf, b"RDX1\n");
    }

    #[test]
    fn test_R_WriteMagic_custom() {
        let mut buf = Vec::new();
        must(R_WriteMagic(&mut buf, 1234));
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
            must(R_WriteMagic(&mut buf, magic));
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
        must(OutIntegerAscii(&mut buf, 42));
        assert_eq!(String::from_utf8_lossy(&buf), "42");

        buf.clear();
        must(OutIntegerAscii(&mut buf, NA_INTEGER));
        assert_eq!(String::from_utf8_lossy(&buf), "NA");
    }

    #[test]
    fn test_OutDoubleAscii() {
        let mut buf = Vec::new();
        must(OutDoubleAscii(&mut buf, 3.14));
        let s = String::from_utf8_lossy(&buf);
        let v: f64 = must(s.parse());
        assert!((v - 3.14).abs() < 1e-10);

        buf.clear();
        must(OutDoubleAscii(&mut buf, f64::NAN));
        assert_eq!(&buf, b"NA");

        buf.clear();
        must(OutDoubleAscii(&mut buf, f64::INFINITY));
        assert_eq!(&buf, b"Inf");

        buf.clear();
        must(OutDoubleAscii(&mut buf, f64::NEG_INFINITY));
        assert_eq!(&buf, b"-Inf");
    }

    #[test]
    fn test_OutStringAscii() {
        let mut buf = Vec::new();
        must(OutStringAscii(&mut buf, "hello"));
        let s = String::from_utf8_lossy(&buf);
        assert!(s.starts_with("5 "));
        assert!(s.contains("hello"));
    }

    #[test]
    fn test_OutStringAscii_escapes() {
        let mut buf = Vec::new();
        must(OutStringAscii(&mut buf, "a\nb"));
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("\\n"), "should escape newline");
    }

    #[test]
    fn test_OutComplexAscii() {
        let mut buf = Vec::new();
        must(OutComplexAscii(&mut buf, 1.0, 2.0));
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("1.0") || s.contains('1'));
        assert!(s.contains("2.0") || s.contains('2'));

        buf.clear();
        must(OutComplexAscii(&mut buf, f64::NAN, 0.0));
        assert_eq!(&buf, b"NA NA");
    }

    #[test]
    fn test_OutSpaceAscii() {
        let mut buf = Vec::new();
        must(OutSpaceAscii(&mut buf, 3));
        assert_eq!(&buf, b"   ");
    }
}
