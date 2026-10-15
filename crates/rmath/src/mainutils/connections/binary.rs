//! Binary I/O: `readBin` / `writeBin` machinery — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::os::raw::{c_double, c_int};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::ptr;

use bzip2::Compression as BzCompression;
use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use flate2::Compression as GzCompression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::instance::{RInstance, with_current_instance, with_required_current_instance};
use crate::sexp::protect::*;

mod filename_owned;
pub use filename_owned::Buffer as AdmittedBinarySource;
#[cfg(test)]
mod filename_tests;

struct FilenameReadParameters {
    kind: BinaryKind,
    count: usize,
    size: usize,
    signed: bool,
    order: ByteOrder,
}

impl FilenameReadParameters {
    fn limit(&self) -> Option<usize> {
        match self.kind {
            BinaryKind::Raw => Some(self.count),
            BinaryKind::Character if self.count == 0 => Some(0),
            BinaryKind::Character => None,
            _ => Some(self.count.saturating_mul(self.size)),
        }
    }

    fn strings(&self) -> Option<usize> {
        (self.kind == BinaryKind::Character).then_some(self.count)
    }

    fn workspace(&self, bytes: usize) -> Result<usize, String> {
        let count = if self.kind == BinaryKind::Character {
            let count = bytes
                .min(self.count)
                .checked_mul(3)
                .ok_or("binary decoder workspace is too large")?;
            // Vec<String> initially allocates four slots, then grows
            // geometrically. Reallocation may retain old and new arrays
            // simultaneously. Empty input needs no temporary allocation.
            if count == 0 { 0 } else { count.max(4) }
        } else {
            (bytes / self.size).min(self.count)
        };
        let width = match self.kind {
            BinaryKind::Raw => return Ok(0),
            BinaryKind::Integer | BinaryKind::Logical => std::mem::size_of::<i32>(),
            BinaryKind::Numeric => std::mem::size_of::<f64>(),
            BinaryKind::Complex => 2 * std::mem::size_of::<f64>(),
            BinaryKind::Character => std::mem::size_of::<String>(),
        };
        count
            .checked_mul(width)
            .and_then(|workspace| {
                workspace.checked_add(if self.kind == BinaryKind::Character {
                    // Invalid UTF-8 expands to three-byte replacements; the
                    // CString producer temporarily copies one selected string
                    // while the original decoded strings remain retained.
                    bytes.checked_mul(6)?
                } else {
                    0
                })
            })
            .ok_or_else(|| "binary decoder workspace is too large".into())
    }
}

fn filename_read_parameters(
    access: &crate::sexp::owner::RuntimeAccess,
    values: [&crate::sexp::Sexp<'static>; 5],
) -> crate::sexp::object::SexpResult<FilenameReadParameters> {
    for value in values {
        access.domain().link(value)?;
    }
    access.with_native(|_| unsafe {
        let [what, count, size, signed, endian] = values;
        let kind = binary_kind_from_what(what.as_raw());
        Ok(FilenameReadParameters {
            kind,
            count: binary_count(count.as_raw()),
            size: binary_size(size.as_raw(), kind),
            signed: logical_arg_or(signed.as_raw(), "signed", 1) != 0,
            order: byte_order_from_arg(endian.as_raw(), "endian"),
        })
    })
}

fn filename_decode(
    access: &crate::sexp::owner::RuntimeAccess,
    parameters: &FilenameReadParameters,
    bytes: &[u8],
) -> crate::sexp::object::SexpResult<crate::sexp::Sexp<'static>> {
    access.with_native(|owner| unsafe {
        owner
            .sexp(decode_binary_result(
                parameters.kind,
                bytes,
                parameters.count,
                parameters.size,
                parameters.signed,
                parameters.order,
            ))?
            .into_owned()
    })
}

fn filename_encode(
    access: &crate::sexp::owner::RuntimeAccess,
    values: [&crate::sexp::Sexp<'static>; 4],
) -> crate::sexp::object::SexpResult<(Vec<u8>, Vec<crate::sexp::memory::TransientReservation>)> {
    filename_encode_staged(access, values, || Ok(()), || Ok(()))
}

fn filename_encode_staged(
    access: &crate::sexp::owner::RuntimeAccess,
    values: [&crate::sexp::Sexp<'static>; 4],
    select_sink: impl FnOnce() -> Result<(), String>,
    open_sink: impl FnOnce() -> Result<(), String>,
) -> crate::sexp::object::SexpResult<(Vec<u8>, Vec<crate::sexp::memory::TransientReservation>)> {
    for value in values {
        access.domain().link(value)?;
    }
    access.with_native(|_| unsafe {
        let [object, size, endian, use_bytes] = values;
        let order = byte_order_from_arg(endian.as_raw(), "endian");
        if Rf_isVectorAtomic(object.as_raw()) == 0 {
            r_error("can only write vector objects");
        }
        let mut attribute = ATTRIB(object.as_raw());
        while !attribute.is_null() && attribute != R_NilValue() {
            if TAG(attribute) != crate::attrib_core::R_NamesSymbol() {
                r_error("can only write vector objects");
            }
            attribute = CDR(attribute);
        }
        select_sink().unwrap_or_else(|error| r_error(&error));
        let mut reservations = Vec::new();
        let mut admitted = 0;
        let peak_factor = if object.typeof_() == SEXPTYPE::STRSXP {
            4
        } else {
            3
        };
        let mut admit = |required: usize| {
            // Encoder Vec capacity can approach 2N. The browser sink
            // clones N bytes while it remains alive; small byte Vecs
            // allocate at least eight bytes. Raw input has a temporary
            // N-byte copy during encoding, covered by the same bound.
            // Character append can simultaneously retain old capacity N,
            // new capacity 2N and the current copied string N.
            let required = if required == 0 {
                0
            } else {
                required
                    .checked_mul(peak_factor)
                    .and_then(|peak| required.checked_add(8).map(|small| peak.max(small)))
                    .unwrap_or_else(|| r_error("binary encoder workspace is too large"))
            };
            let growth = required.saturating_sub(admitted);
            if growth != 0 {
                reservations.try_reserve(1).unwrap_or_else(|_| {
                    r_error("allocation failed while admitting binary encoder bytes")
                });
                let reservation =
                    filename_owned::reserve(access, growth).unwrap_or_else(|error| r_error(&error));
                reservations.push(reservation);
                admitted = required;
            }
        };
        // Admission precedes a provider or an externally visible sink effect.
        // Fixed-width data admits its largest supported encoding without
        // reading elements. Character widths need the existing admitted
        // encoder first; its owned bytes survive the later sink opening.
        let prepared = if object.typeof_() == SEXPTYPE::STRSXP {
            Some(encode_binary_object_selected_size(
                object.as_raw(),
                NA_INTEGER,
                order,
                &mut admit,
            ))
        } else {
            let width = match object.typeof_() {
                SEXPTYPE::RAWSXP => 1,
                SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => 4,
                SEXPTYPE::REALSXP => 8,
                SEXPTYPE::CPLXSXP => 16,
                _ => unreachable!(),
            };
            admit(
                (LENGTH(object.as_raw()) as usize)
                    .checked_mul(width)
                    .unwrap_or_else(|| r_error("binary encoder workspace is too large")),
            );
            None
        };
        open_sink().unwrap_or_else(|error| r_error(&error));
        // GNU coerces size once, before checking useBytes. A coercion warning
        // promoted to an error must follow opening and precede useBytes errors.
        let selected_size = crate::mainutils::coerce::asInteger(size.as_raw());
        let _use_bytes = logical_arg_or(use_bytes.as_raw(), "useBytes", 0);
        let bytes = prepared.unwrap_or_else(|| {
            encode_binary_object_selected_size(object.as_raw(), selected_size, order, &mut admit)
        });
        Ok((bytes, reservations))
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryKind {
    Raw,
    Integer,
    Logical,
    Numeric,
    Complex,
    Character,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    fn native() -> Self {
        if cfg!(target_endian = "little") {
            Self::Little
        } else {
            Self::Big
        }
    }

    fn swapped(self) -> Self {
        match self {
            Self::Little => Self::Big,
            Self::Big => Self::Little,
        }
    }
}

pub unsafe fn binary_kind_from_what(what: SEXP) -> BinaryKind {
    unsafe {
        if what.is_null() || what == R_NilValue() {
            r_error("invalid 'what' argument");
        }
        match TYPEOF(what) {
            t if t == SEXPTYPE::STRSXP => {
                if LENGTH(what) == 0 {
                    return BinaryKind::Character;
                }
                match string_elt(what, 0).as_str() {
                    "raw" => BinaryKind::Raw,
                    "integer" | "int" => BinaryKind::Integer,
                    "logical" => BinaryKind::Logical,
                    "numeric" | "double" => BinaryKind::Numeric,
                    "complex" => BinaryKind::Complex,
                    "character" => BinaryKind::Character,
                    _ => r_error("invalid 'what' argument"),
                }
            }
            t if t == SEXPTYPE::RAWSXP => BinaryKind::Raw,
            t if t == SEXPTYPE::INTSXP => BinaryKind::Integer,
            t if t == SEXPTYPE::LGLSXP => BinaryKind::Logical,
            t if t == SEXPTYPE::REALSXP => BinaryKind::Numeric,
            t if t == SEXPTYPE::CPLXSXP => BinaryKind::Complex,
            _ => r_error("invalid 'what' argument"),
        }
    }
}

pub unsafe fn byte_order_from_arg(arg: SEXP, name: &str) -> ByteOrder {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            return ByteOrder::native();
        }
        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            if LENGTH(arg) < 1 {
                r_error(&format!("invalid '{name}' argument"));
            }
            return match string_elt(arg, 0).as_str() {
                "little" => ByteOrder::Little,
                "big" => ByteOrder::Big,
                "swap" => ByteOrder::native().swapped(),
                _ => r_error(&format!("invalid '{name}' argument")),
            };
        }
        let swap = check_logical_arg(arg, name);
        if swap == 0 {
            ByteOrder::native()
        } else {
            ByteOrder::native().swapped()
        }
    }
}

pub unsafe fn binary_count(arg: SEXP) -> usize {
    unsafe {
        let n = as_integer(arg);
        if n < 0 || n == NA_INTEGER {
            r_error("invalid 'n' argument");
        }
        n as usize
    }
}

pub unsafe fn logical_arg_or(arg: SEXP, name: &str, default: c_int) -> c_int {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            default
        } else {
            check_logical_arg(arg, name)
        }
    }
}

pub unsafe fn connection_rw_mode(arg: SEXP) -> c_int {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            return 0;
        }
        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            if LENGTH(arg) < 1 {
                return 0;
            }
            return match string_elt(arg, 0).as_str() {
                "read" | "r" => 1,
                "write" | "w" => 2,
                _ => 0,
            };
        }
        as_integer(arg)
    }
}

pub unsafe fn binary_size(arg: SEXP, kind: BinaryKind) -> usize {
    unsafe {
        let default = match kind {
            BinaryKind::Raw | BinaryKind::Character => 1,
            BinaryKind::Integer | BinaryKind::Logical => 4,
            BinaryKind::Numeric => 8,
            BinaryKind::Complex => 16,
        };
        let size = as_integer(arg);
        if size == NA_INTEGER {
            return default;
        }
        let size = size as usize;
        let valid = match kind {
            BinaryKind::Raw | BinaryKind::Character => size == 1,
            BinaryKind::Integer | BinaryKind::Logical => matches!(size, 1 | 2 | 4),
            BinaryKind::Numeric => matches!(size, 4 | 8),
            BinaryKind::Complex => matches!(size, 8 | 16),
        };
        if !valid {
            r_error("invalid 'size' argument");
        }
        size
    }
}

pub unsafe fn raw_bytes_from_vector(raw: SEXP) -> Vec<u8> {
    unsafe {
        let len = LENGTH(raw) as usize;
        if len == 0 {
            return Vec::new();
        }
        let data = RAW(raw);
        if data.is_null() {
            return Vec::new();
        }
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

fn source_connection_index(
    access: &crate::sexp::owner::RuntimeAccess,
    con: &crate::sexp::Sexp<'static>,
) -> crate::sexp::object::SexpResult<ConnectionToken> {
    binary_connection_index(access, con, false)
}

fn binary_connection_index(
    access: &crate::sexp::owner::RuntimeAccess,
    con: &crate::sexp::Sexp<'static>,
    writing: bool,
) -> crate::sexp::object::SexpResult<ConnectionToken> {
    access.domain().link(con)?;
    access.with_native(|_| unsafe {
        if !inherits_class(con.as_raw(), "connection") {
            r_error("'con' is not a connection");
        }
        let index = as_integer(con.as_raw());
        let slot = checked_connection_index(index);
        let (text, open, permitted) = {
            let table = connection_table();
            let connection = table[slot].as_ref().unwrap();
            (
                connection.text,
                connection.isopen,
                if writing {
                    connection.canwrite
                } else {
                    connection.canread
                },
            )
        };
        if text {
            r_error(if writing {
                "can only write to a binary connection"
            } else {
                "can only read from a binary connection"
            });
        }
        if !open {
            r_error("connection is not open");
        }
        if !permitted {
            r_error(if writing {
                "cannot write to this connection"
            } else {
                "cannot read from this connection"
            });
        }
        connection_token(index)
            .map_err(|message| crate::sexp::SexpError::EvaluationFailed { message })
    })
}

fn raw_sink_result(
    access: &crate::sexp::owner::RuntimeAccess,
    bytes: &[u8],
) -> crate::sexp::object::SexpResult<crate::sexp::Sexp<'static>> {
    access.with_native(|owner| unsafe { owner.sexp(alloc_raw_result(bytes))?.into_owned() })
}

pub unsafe fn read_binary_source(con: SEXP, limit: Option<usize>) -> AdmittedBinarySource {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| r_error(&error.to_string()));
        let con = owner
            .sexp(con)
            .and_then(crate::sexp::Sexp::into_owned)
            .unwrap_or_else(|error| r_error(&error.to_string()));
        filename_owned::source_bytes(con, limit).unwrap_or_else(|error| r_error(&error))
    }
}

pub fn read_integer_chunk(chunk: &[u8], order: ByteOrder, signed: bool) -> i32 {
    match chunk.len() {
        1 if signed => i8::from_ne_bytes([chunk[0]]) as i32,
        1 => chunk[0] as i32,
        2 => {
            let bytes = [chunk[0], chunk[1]];
            if signed {
                match order {
                    ByteOrder::Little => i16::from_le_bytes(bytes) as i32,
                    ByteOrder::Big => i16::from_be_bytes(bytes) as i32,
                }
            } else {
                match order {
                    ByteOrder::Little => u16::from_le_bytes(bytes) as i32,
                    ByteOrder::Big => u16::from_be_bytes(bytes) as i32,
                }
            }
        }
        4 => {
            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
            match order {
                ByteOrder::Little => i32::from_le_bytes(bytes),
                ByteOrder::Big => i32::from_be_bytes(bytes),
            }
        }
        _ => 0,
    }
}

pub fn write_integer_chunk(out: &mut Vec<u8>, value: i32, size: usize, order: ByteOrder) {
    match size {
        1 => out.push(value as u8),
        2 => match order {
            ByteOrder::Little => out.extend_from_slice(&(value as i16).to_le_bytes()),
            ByteOrder::Big => out.extend_from_slice(&(value as i16).to_be_bytes()),
        },
        4 => match order {
            ByteOrder::Little => out.extend_from_slice(&value.to_le_bytes()),
            ByteOrder::Big => out.extend_from_slice(&value.to_be_bytes()),
        },
        _ => {}
    }
}

pub unsafe fn alloc_raw_result(bytes: &[u8]) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(SEXPTYPE::RAWSXP, bytes.len() as c_int);
        if !ans.is_null() && !bytes.is_empty() {
            ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(ans), bytes.len());
        }
        ans
    }
}

pub unsafe fn alloc_integer_result(values: &[i32], sexptype: SEXPTYPE) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(sexptype.0, values.len() as c_int);
        if !ans.is_null() {
            let dest = if sexptype == SEXPTYPE::LGLSXP {
                LOGICAL(ans)
            } else {
                INTEGER(ans)
            };
            for (index, value) in values.iter().enumerate() {
                *dest.add(index) = *value;
            }
        }
        ans
    }
}

pub unsafe fn alloc_real_result(values: &[f64]) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, values.len() as c_int);
        if !ans.is_null() {
            for (index, value) in values.iter().enumerate() {
                *REAL(ans).add(index) = *value;
            }
        }
        ans
    }
}

pub unsafe fn alloc_character_result(values: &[String]) -> SEXP {
    unsafe {
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, values.len() as c_int);
        if !ans.is_null() {
            for (index, value) in values.iter().enumerate() {
                let c_value = CString::new(value.as_str()).unwrap_or_default();
                SET_STRING_ELT(ans, index as R_xlen_t, Rf_mkChar(c_value.as_ptr()));
            }
        }
        ans
    }
}

pub unsafe fn decode_binary_result(
    kind: BinaryKind,
    bytes: &[u8],
    n: usize,
    size: usize,
    signed: bool,
    order: ByteOrder,
) -> SEXP {
    unsafe {
        match kind {
            BinaryKind::Raw => alloc_raw_result(&bytes[..bytes.len().min(n)]),
            BinaryKind::Integer | BinaryKind::Logical => {
                let values: Vec<i32> = bytes
                    .chunks_exact(size)
                    .take(n)
                    .map(|chunk| read_integer_chunk(chunk, order, signed))
                    .collect();
                let sexptype = if kind == BinaryKind::Logical {
                    SEXPTYPE::LGLSXP
                } else {
                    SEXPTYPE::INTSXP
                };
                alloc_integer_result(&values, sexptype)
            }
            BinaryKind::Numeric => {
                let values: Vec<f64> = bytes
                    .chunks_exact(size)
                    .take(n)
                    .map(|chunk| {
                        if size == 4 {
                            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
                            match order {
                                ByteOrder::Little => f32::from_le_bytes(bytes) as f64,
                                ByteOrder::Big => f32::from_be_bytes(bytes) as f64,
                            }
                        } else {
                            let bytes = [
                                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5],
                                chunk[6], chunk[7],
                            ];
                            match order {
                                ByteOrder::Little => f64::from_le_bytes(bytes),
                                ByteOrder::Big => f64::from_be_bytes(bytes),
                            }
                        }
                    })
                    .collect();
                alloc_real_result(&values)
            }
            BinaryKind::Complex => {
                let values: Vec<f64> = bytes
                    .chunks_exact(size / 2)
                    .take(n * 2)
                    .map(|chunk| {
                        if size == 8 {
                            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
                            match order {
                                ByteOrder::Little => f32::from_le_bytes(bytes) as f64,
                                ByteOrder::Big => f32::from_be_bytes(bytes) as f64,
                            }
                        } else {
                            let bytes = [
                                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5],
                                chunk[6], chunk[7],
                            ];
                            match order {
                                ByteOrder::Little => f64::from_le_bytes(bytes),
                                ByteOrder::Big => f64::from_be_bytes(bytes),
                            }
                        }
                    })
                    .collect();
                let count = values.len() / 2;
                let ans = Rf_allocVector(SEXPTYPE::CPLXSXP, count as c_int);
                if !ans.is_null() {
                    let dest = COMPLEX(ans);
                    for index in 0..count {
                        (*dest.add(index)).r = values[index * 2];
                        (*dest.add(index)).i = values[index * 2 + 1];
                    }
                }
                ans
            }
            BinaryKind::Character => {
                let mut values = Vec::new();
                let mut start = 0usize;
                while start < bytes.len() && values.len() < n {
                    let rel_end = bytes[start..].iter().position(|byte| *byte == 0);
                    match rel_end {
                        Some(len) => {
                            values.push(
                                String::from_utf8_lossy(&bytes[start..start + len]).into_owned(),
                            );
                            start += len + 1;
                        }
                        None => break,
                    }
                }
                alloc_character_result(&values)
            }
        }
    }
}

pub unsafe fn encode_binary_object(object: SEXP, size_arg: SEXP, order: ByteOrder) -> Vec<u8> {
    unsafe { encode_binary_object_admitted(object, size_arg, order, |_| {}) }
}

fn binary_write_size(size: c_int, kind: BinaryKind) -> usize {
    let default = match kind {
        BinaryKind::Raw | BinaryKind::Character => 1,
        BinaryKind::Integer | BinaryKind::Logical => 4,
        BinaryKind::Numeric => 8,
        BinaryKind::Complex => 16,
    };
    if size == NA_INTEGER {
        return default;
    }
    let valid = match kind {
        BinaryKind::Raw => size == 1,
        BinaryKind::Character => true,
        BinaryKind::Integer | BinaryKind::Logical => matches!(size, 1 | 2 | 4),
        BinaryKind::Numeric => matches!(size, 4 | 8),
        BinaryKind::Complex => size == 16,
    };
    if !valid {
        match kind {
            BinaryKind::Raw => r_error("size changing is not supported for raw vectors"),
            BinaryKind::Complex => r_error("size changing is not supported for complex vectors"),
            _ => r_error(&format!("size {size} is unknown on this machine")),
        }
    }
    size as usize
}

unsafe fn encode_binary_object_admitted(
    object: SEXP,
    size_arg: SEXP,
    order: ByteOrder,
    admit: impl FnMut(usize),
) -> Vec<u8> {
    unsafe {
        let size = crate::mainutils::coerce::asInteger(size_arg);
        encode_binary_object_selected_size(object, size, order, admit)
    }
}

unsafe fn encode_binary_object_selected_size(
    object: SEXP,
    size_arg: c_int,
    order: ByteOrder,
    mut admit: impl FnMut(usize),
) -> Vec<u8> {
    unsafe {
        if object.is_null() || object == R_NilValue() {
            r_error("invalid 'object' argument");
        }
        let obj_type = TYPEOF(object);
        let obj_len = LENGTH(object) as usize;
        let mut bytes = Vec::new();
        match obj_type {
            t if t == SEXPTYPE::RAWSXP => {
                if obj_len != 0 {
                    binary_write_size(size_arg, BinaryKind::Raw);
                }
                admit(obj_len);
                bytes.extend_from_slice(&raw_bytes_from_vector(object));
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                let kind = if obj_type == SEXPTYPE::LGLSXP {
                    BinaryKind::Logical
                } else {
                    BinaryKind::Integer
                };
                let size = if obj_len == 0 {
                    4
                } else {
                    binary_write_size(size_arg, kind)
                };
                admit(
                    obj_len
                        .checked_mul(size)
                        .unwrap_or_else(|| r_error("binary encoder workspace is too large")),
                );
                let src = if obj_type == SEXPTYPE::LGLSXP {
                    LOGICAL(object)
                } else {
                    INTEGER(object)
                };
                for index in 0..obj_len {
                    write_integer_chunk(&mut bytes, *src.add(index), size, order);
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let size = if obj_len == 0 {
                    8
                } else {
                    binary_write_size(size_arg, BinaryKind::Numeric)
                };
                admit(
                    obj_len
                        .checked_mul(size)
                        .unwrap_or_else(|| r_error("binary encoder workspace is too large")),
                );
                for index in 0..obj_len {
                    let value = *REAL(object).add(index);
                    if size == 4 {
                        let value = value as f32;
                        match order {
                            ByteOrder::Little => bytes.extend_from_slice(&value.to_le_bytes()),
                            ByteOrder::Big => bytes.extend_from_slice(&value.to_be_bytes()),
                        }
                    } else {
                        match order {
                            ByteOrder::Little => bytes.extend_from_slice(&value.to_le_bytes()),
                            ByteOrder::Big => bytes.extend_from_slice(&value.to_be_bytes()),
                        }
                    }
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
                let size = if obj_len == 0 {
                    16
                } else {
                    binary_write_size(size_arg, BinaryKind::Complex)
                };
                admit(
                    obj_len
                        .checked_mul(size)
                        .unwrap_or_else(|| r_error("binary encoder workspace is too large")),
                );
                for index in 0..obj_len {
                    let value = *COMPLEX(object).add(index);
                    if size == 8 {
                        for part in [value.r as f32, value.i as f32] {
                            match order {
                                ByteOrder::Little => bytes.extend_from_slice(&part.to_le_bytes()),
                                ByteOrder::Big => bytes.extend_from_slice(&part.to_be_bytes()),
                            }
                        }
                    } else {
                        for part in [value.r, value.i] {
                            match order {
                                ByteOrder::Little => bytes.extend_from_slice(&part.to_le_bytes()),
                                ByteOrder::Big => bytes.extend_from_slice(&part.to_be_bytes()),
                            }
                        }
                    }
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                let owner = crate::sexp::owner::OwnerToken::current()
                    .unwrap_or_else(|error| r_error(&error.to_string()));
                for index in 0..obj_len as R_xlen_t {
                    // Select once and keep the actual character alive through
                    // admission callbacks. End the raw byte borrow before that
                    // boundary; copy only after its worst-case UTF-8 workspace
                    // has been admitted, using this saved owning character.
                    let character = owner
                        .sexp(STRING_ELT(object, index))
                        .and_then(crate::sexp::Sexp::into_owned)
                        .unwrap_or_else(|error| r_error(&error.to_string()));
                    let source_len = {
                        let pointer = CHAR(character.as_raw());
                        if pointer.is_null() {
                            0
                        } else {
                            CStr::from_ptr(pointer).to_bytes().len()
                        }
                    };
                    let length =
                        bytes
                            .len()
                            .checked_add(source_len.checked_mul(3).unwrap_or_else(|| {
                                r_error("binary encoder workspace is too large")
                            }))
                            .and_then(|length| length.checked_add(1))
                            .unwrap_or_else(|| r_error("binary encoder workspace is too large"));
                    admit(length);
                    let text = charsxp_to_string(character.as_raw());
                    bytes.extend_from_slice(text.as_bytes());
                    bytes.push(0);
                }
            }
            _ => r_error("can only write vector objects"),
        }
        bytes
    }
}

pub unsafe fn write_binary_sink(con: SEXP, bytes: &[u8]) -> SEXP {
    unsafe {
        if TYPEOF(con) == SEXPTYPE::RAWSXP {
            return alloc_raw_result(bytes);
        }
        if TYPEOF(con) == SEXPTYPE::STRSXP {
            let owner = crate::sexp::owner::OwnerToken::current()
                .unwrap_or_else(|error| r_error(&error.to_string()));
            let con = owner
                .sexp(con)
                .and_then(crate::sexp::Sexp::into_owned)
                .unwrap_or_else(|error| r_error(&error.to_string()));
            filename_owned::sink_bytes(con, bytes).unwrap_or_else(|error| r_error(&error));
            return R_NilValue();
        }
        if !inherits_class(con, "connection") {
            r_error("'con' is not a connection");
        }
        connection_write_bytes(as_integer(con), bytes);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_readBin — readBin(con, what, n, size = NA, signed = TRUE, endian/swap)
// ---------------------------------------------------------------------------

pub unsafe fn do_readBin(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| r_error(&error.to_string()));
        let arguments = owner
            .sexp(args)
            .and_then(crate::sexp::Sexp::into_owned)
            .unwrap_or_else(|error| r_error(&error.to_string()));
        filename_owned::read(arguments)
            .unwrap_or_else(|error| r_error(&error))
            .as_raw()
    }
}

// ---------------------------------------------------------------------------
// do_writeBin — writeBin(object, con, size = NA, endian/swap, useBytes = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_writeBin(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| r_error(&error.to_string()));
        let arguments = owner
            .sexp(args)
            .and_then(crate::sexp::Sexp::into_owned)
            .unwrap_or_else(|error| r_error(&error.to_string()));
        let result = filename_owned::write(arguments).unwrap_or_else(|error| r_error(&error));
        crate::sexp::globals::set_R_Visible(if result.is_nil() {
            crate::sexp::ffi::FALSE
        } else {
            crate::sexp::ffi::TRUE
        });
        result.as_raw()
    }
}
