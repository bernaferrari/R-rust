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
    CAD5R, CADDR, CADR, CAR, CDR, CHAR, COMPLEX, INTEGER, LOGICAL, PRINTNAME, RAW, REAL,
    SET_ATTRIB, SET_OBJECT, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETTAG, STRING_ELT, TAG,
    TYPEOF, XLENGTH,
};
use crate::sexp::attrib_core::{R_ClassSymbol, getAttrib};
use crate::sexp::constructors::{Rf_allocList, Rf_allocVector3, Rf_mkChar};
use crate::sexp::envir::R_findVar;
use crate::sexp::ffi::{R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NaString, R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use std::io::{self, BufRead, Cursor, Read, Write};
use std::os::raw::c_int;

mod workspace_file;
#[cfg(test)]
mod workspace_tests;

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
            if !buf[..4].iter().all(u8::is_ascii_digit) || buf[4] != b'\n' {
                return R_MAGIC_CORRUPT;
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
    if crate::sexp::ffi::R_IsNA(x) {
        fp.write_all(b"NA")
    } else if x.is_nan() {
        fp.write_all(b"NaN")
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
        "NaN" => Ok(f64::NAN),
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
        "NaN" => Ok(f64::NAN),
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
    let mut input = String::new();
    reader.read_line(&mut input)?;
    let line = input.trim_end_matches(['\r', '\n']);
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
    if expected_len > encoded.len() {
        return Err(io::Error::other("ASCII string length mismatch"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(encoded.len())
        .map_err(io::Error::other)?;
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

unsafe fn read_saved_object(reader: &mut Cursor<&[u8]>, depth: usize) -> io::Result<SEXP> {
    if depth >= 128 {
        return Err(io::Error::other("legacy workspace nesting limit exceeded"));
    }
    unsafe {
        let sexptype = InIntegerAscii(reader)?;
        let value = match SEXPTYPE::from(sexptype) {
            SEXPTYPE::NILSXP => Ok(R_NilValue()),
            SEXPTYPE::ANYSXP => Ok(R_MissingArg()),
            SEXPTYPE::SYMSXP => {
                let name = InStringAscii(reader)?
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| io::Error::other("invalid legacy workspace symbol"))?;
                let cstr = std::ffi::CString::new(name).map_err(io::Error::other)?;
                Ok(Rf_install(cstr.as_ptr()))
            }
            SEXPTYPE::CLOSXP => {
                let formals = read_saved_object(reader, depth + 1)?;
                let _formals_guard = protect(formals);
                if formals != R_NilValue() && TYPEOF(formals) != SEXPTYPE::LISTSXP {
                    return Err(io::Error::other("invalid legacy closure formals"));
                }
                let body = read_saved_object(reader, depth + 1)?;
                let _body_guard = protect(body);
                Ok(crate::mainutils::dstruct::mkCLOSXP(
                    formals,
                    body,
                    crate::sexp::globals::R_GlobalEnv(),
                ))
            }
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                let len = legacy_length(reader)?;
                let value = Rf_allocList(len);
                let _guard = protect(value);
                if len != 0 && SEXPTYPE::from(sexptype) == SEXPTYPE::LANGSXP {
                    crate::sexp::accessors::SET_TYPEOF(value, SEXPTYPE::LANGSXP.as_c_int());
                }
                let mut current = value;
                for _ in 0..len {
                    let tag = read_saved_object(reader, depth + 1)?;
                    if tag != R_NilValue() && TYPEOF(tag) != SEXPTYPE::SYMSXP {
                        return Err(io::Error::other("invalid legacy pairlist tag"));
                    }
                    SETTAG(current, tag);
                    let car = read_saved_object(reader, depth + 1)?;
                    SETCAR(current, car);
                    current = CDR(current);
                }
                Ok(value)
            }
            SEXPTYPE::LGLSXP => {
                let len = legacy_length(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::LGLSXP, len as R_xlen_t);
                for i in 0..len as usize {
                    *LOGICAL(value).add(i) = InIntegerAscii(reader)?;
                }
                Ok(value)
            }
            SEXPTYPE::INTSXP => {
                let len = legacy_length(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::INTSXP, len as R_xlen_t);
                for i in 0..len as usize {
                    *INTEGER(value).add(i) = InIntegerAscii(reader)?;
                }
                Ok(value)
            }
            SEXPTYPE::REALSXP => {
                let len = legacy_length(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::REALSXP, len as R_xlen_t);
                for i in 0..len as usize {
                    *REAL(value).add(i) = InDoubleAscii(reader)?;
                }
                Ok(value)
            }
            SEXPTYPE::CPLXSXP => {
                let len = legacy_length(reader)?;
                let value = Rf_allocVector3(SEXPTYPE::CPLXSXP, len as R_xlen_t);
                for i in 0..len as usize {
                    *COMPLEX(value).add(i) = InComplexAscii(reader)?;
                }
                Ok(value)
            }
            SEXPTYPE::STRSXP => {
                let len = legacy_length(reader)?;
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
            SEXPTYPE::RAWSXP => {
                let len = legacy_length(reader)?;
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
                let len = legacy_length(reader)?;
                let value = Rf_allocVector3(SEXPTYPE(sexptype), len as R_xlen_t);
                let _guard = protect(value);
                for i in 0..len as R_xlen_t {
                    let element = read_saved_object(reader, depth + 1)?;
                    SET_VECTOR_ELT(value, i, element);
                }
                Ok(value)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported saved object type {sexptype}"),
            )),
        }?;
        let _guard = protect(value);
        read_attribute_list(reader, value, depth)?;
        Ok(value)
    }
}

unsafe fn read_attribute_list(
    reader: &mut Cursor<&[u8]>,
    value: SEXP,
    depth: usize,
) -> io::Result<()> {
    unsafe {
        let has_attrs = InIntegerAscii(reader)?;
        if has_attrs == 0 {
            return Ok(());
        }
        if has_attrs != 1 || value == R_NilValue() || TYPEOF(value) == SEXPTYPE::SYMSXP {
            return Err(io::Error::other("invalid legacy workspace attributes"));
        }
        let attrs = read_saved_object(reader, depth + 1)?;
        let mut cell = attrs;
        while cell != R_NilValue() {
            if TYPEOF(cell) != SEXPTYPE::LISTSXP || TYPEOF(TAG(cell)) != SEXPTYPE::SYMSXP {
                return Err(io::Error::other(
                    "saved attribute payload is not a named pairlist",
                ));
            }
            cell = CDR(cell);
        }
        SET_ATTRIB(value, attrs);
        if getAttrib(value, R_ClassSymbol()) != R_NilValue() {
            SET_OBJECT(value, 1);
        }
        Ok(())
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

unsafe fn load_arguments(args: SEXP) -> [Option<SEXP>; 3] {
    const NAMES: [&str; 3] = ["file", "envir", "verbose"];
    let mut selected = [None; 3];
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = tag_name(current) {
                let index = NAMES
                    .iter()
                    .position(|candidate| *candidate == name)
                    .unwrap_or_else(|| error(&format!("unused argument ({name} = ...)")));
                if selected[index].replace(CAR(current)).is_some() {
                    error(&format!(
                        "formal argument \"{name}\" matched by multiple actual arguments"
                    ));
                }
            }
            current = CDR(current);
        }
        let mut current = args;
        let mut available = selected.iter_mut().filter(|slot| slot.is_none());
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                *available
                    .next()
                    .unwrap_or_else(|| error("unused load() argument")) = Some(CAR(current));
            }
            current = CDR(current);
        }
    }
    selected
}

unsafe fn save_arguments(args: SEXP) -> ([Option<SEXP>; 9], Vec<String>) {
    const NAMES: [&str; 9] = [
        "list",
        "file",
        "ascii",
        "version",
        "envir",
        "compress",
        "compression_level",
        "eval.promises",
        "precheck",
    ];
    let mut options = [None; 9];
    let mut names = Vec::new();
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let name = tag_name(current);
            let expr = CAR(current);
            if let Some(index) = name
                .as_deref()
                .and_then(|name| NAMES.iter().position(|option| *option == name))
            {
                if options[index].replace(expr).is_some() {
                    error(&format!(
                        "formal argument \"{}\" matched by multiple actual arguments",
                        NAMES[index]
                    ));
                }
            } else {
                if expr.is_null() || TYPEOF(expr) != SEXPTYPE::SYMSXP || expr == R_MissingArg() {
                    error("save() requires object names");
                }
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(expr)));
                names.push(name.to_string_lossy().into_owned());
            }
            current = CDR(current);
        }
    }
    (options, names)
}

unsafe fn eval_save_argument(arg: Option<SEXP>, rho: SEXP) -> SEXP {
    unsafe { arg.map_or_else(R_NilValue, |expr| crate::eval::eval::Rf_eval(expr, rho)) }
}

unsafe fn names_from_save_list(list: SEXP) -> Vec<String> {
    unsafe {
        if list == R_NilValue() {
            return Vec::new();
        }
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

/// Read the Rport-specific ASCII layout written before standard workspace
/// snapshots were supported. Only count-prefixed RDA files enter this reader.
fn read_legacy_workspace<'a>(
    owner: crate::sexp::owner::OwnerToken<'a>,
    bytes: &[u8],
) -> io::Result<crate::sexp::object::Sexp<'a>> {
    // At most the file's own size is used by each outstanding token,
    // decoded string, or line buffer.
    let _parse_memory = owner
        .with_arena(|arena| arena.reserve_workspace(bytes.len().saturating_mul(3)))
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::other("legacy workspace exceeds session memory limit"))?;
    let mut reader = Cursor::new(bytes);
    let count = legacy_length(&mut reader)?;
    let mut builder = crate::sexp::object::PairlistBuilder::new_in(owner);
    for _ in 0..count {
        let name = InStringAscii(&mut reader)?
            .filter(|name| !name.is_empty())
            .ok_or_else(|| io::Error::other("invalid legacy workspace binding name"))?;
        let name = std::ffi::CString::new(name).map_err(io::Error::other)?;
        let symbol = unsafe { Rf_install(name.as_ptr()) };
        let symbol = builder.wrap(symbol).map_err(io::Error::other)?;
        let value = unsafe { read_saved_object(&mut reader, 0) }?;
        let value = builder.wrap(value).map_err(io::Error::other)?;
        builder
            .push(value, Some(symbol))
            .map_err(io::Error::other)?;
    }
    if !bytes[reader.position() as usize..]
        .iter()
        .all(u8::is_ascii_whitespace)
    {
        return Err(io::Error::other("unexpected data after legacy workspace"));
    }
    builder.finish().map_err(io::Error::other)
}

fn legacy_length(reader: &mut Cursor<&[u8]>) -> io::Result<c_int> {
    let value = InIntegerAscii(reader)?;
    let remaining = reader.get_ref().len() - reader.position() as usize;
    if value < 0 || value as usize > remaining {
        return Err(io::Error::other("invalid length in legacy workspace"));
    }
    Ok(value)
}

unsafe fn workspace_version(version: SEXP) -> c_int {
    unsafe {
        if version.is_null() || version == R_NilValue() {
            return defaultSaveVersion();
        }
        let version = crate::mainutils::coerce::asInteger(version);
        if !matches!(version, 2 | 3) {
            error(&format!("version {version} not supported"));
        }
        version
    }
}

unsafe fn workspace_flag(value: SEXP, default: bool) -> bool {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return default;
        }
        match crate::mainutils::coerce::asLogical(value) {
            0 => false,
            1 => true,
            _ => error("NA in coercion to boolean"),
        }
    }
}

unsafe fn workspace_codec(value: SEXP, ascii: bool) -> Option<&'static str> {
    unsafe {
        if value.is_null() {
            return (!ascii).then_some("gzip");
        }
        match SEXPTYPE::from(TYPEOF(value)) {
            SEXPTYPE::LGLSXP if XLENGTH(value) == 1 => {
                workspace_flag(value, !ascii).then_some("gzip")
            }
            SEXPTYPE::STRSXP if XLENGTH(value) == 1 => {
                let name = STRING_ELT(value, 0);
                if name == R_NaString() {
                    error("missing value where TRUE/FALSE needed");
                }
                match std::ffi::CStr::from_ptr(CHAR(name)).to_bytes() {
                    b"gzip" => Some("gzip"),
                    b"bzip2" => Some("bzip2"),
                    b"xz" => Some("xz"),
                    b"no compression" => None,
                    codec => error(&format!(
                        "'compress = \"{}\"' is invalid",
                        String::from_utf8_lossy(codec)
                    )),
                }
            }
            _ => error("'compress' must be logical or character"),
        }
    }
}

unsafe fn workspace_owner() -> crate::sexp::owner::OwnerToken<'static> {
    unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// The evaluated .Internal(save()) path writes an uncompressed workspace.
pub unsafe fn do_save(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(op, args);
        let owner = workspace_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let _args = protect(args);
        let names = names_from_save_list(CAR(args));
        let opts = workspace_file::SaveOptions {
            ascii: workspace_flag(CADDR(args), false),
            version: workspace_version(CAR(CDR(CDR(CDR(args))))),
            codec: None,
            eval_promises: workspace_flag(CAR(CDR(CDR(CDR(CDR(CDR(args)))))), true),
        };
        workspace_file::save(owner, &names, CADR(args), CAD5R(args), opts);
        R_NilValue()
    }
}

/// User-facing save() resolves every requested object before publishing bytes.
pub unsafe fn do_save_user(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = workspace_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let _args = protect(args);
        let _rho = protect(rho);
        let (
            [
                list,
                file,
                ascii,
                version,
                envir,
                compress,
                compression_level,
                eval_promises,
                _precheck,
            ],
            dots,
        ) = save_arguments(args);
        if list.is_none() && dots.is_empty() {
            error("nothing specified to be save()d");
        }
        let mut names = if list.is_some() {
            names_from_save_list(eval_save_argument(list, rho))
        } else {
            Vec::new()
        };
        names.extend(dots);
        let file_expr = file.unwrap_or_else(|| error("'file' must be specified"));
        let file = eval_save_argument(Some(file_expr), rho);
        let _file = protect(file);
        let envir = if envir.is_some() {
            eval_save_argument(envir, rho)
        } else {
            rho
        };
        let _envir = protect(envir);
        let version = workspace_version(eval_save_argument(version, rho));
        let ascii = workspace_flag(eval_save_argument(ascii, rho), false);
        let codec = if compress.is_some() {
            workspace_codec(eval_save_argument(compress, rho), ascii)
        } else {
            (!ascii).then_some("gzip")
        };
        if codec.is_some() && compression_level.is_some() {
            error("custom workspace compression_level is not supported");
        }
        let eval_promises = workspace_flag(eval_save_argument(eval_promises, rho), true);
        let opts = workspace_file::SaveOptions {
            ascii,
            version,
            codec,
            eval_promises,
        };
        // Always resolve the complete snapshot before replacing a workspace.
        workspace_file::save(owner, &names, file, envir, opts);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

pub unsafe fn do_load(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(op, args);
        let owner = workspace_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let _args = protect(args);
        workspace_file::load(owner, CAR(args), CADR(args))
    }
}

/// User-facing load() decodes and validates the snapshot before binding it.
pub unsafe fn do_load_user(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = workspace_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let _args = protect(args);
        let [file, envir, verbose] = load_arguments(args);
        let verbose = workspace_flag(verbose.unwrap_or(R_NilValue()), false);
        let names = workspace_file::load(owner, file.unwrap_or(R_NilValue()), envir.unwrap_or(rho));
        if verbose {
            let _names = protect(names);
            let mut text = String::from("Loading objects:\n");
            for index in 0..XLENGTH(names) {
                let name = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, index)));
                text.push_str("  ");
                text.push_str(&name.to_string_lossy());
                text.push('\n');
            }
            crate::sexp::output::capture_stdout(&text);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        names
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
    fn magic_rejects_non_digit_legacy_headers() {
        let mut invalid = std::io::Cursor::new(b"unsupported serialized data");
        assert_eq!(R_ReadMagic(&mut invalid), R_MAGIC_CORRUPT);
        let mut legacy = std::io::Cursor::new(b"1234\n");
        assert_eq!(R_ReadMagic(&mut legacy), 1234);
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
        assert_eq!(&buf, b"NaN");

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
