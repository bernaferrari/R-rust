#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/iosupport.c — I/O support utilities.
//!
//! Provides IoBuffer and TextBuffer implementations used for parsing
//! input from consoles and character vectors.

use std::os::raw::c_int;

use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::accessors::*;
use crate::sexp::globals::R_NilValue;

pub const IOBSIZE: usize = 4096;

// ---------------------------------------------------------------------------
// IoBuffer — a growing byte buffer for reading/writing character streams
// ---------------------------------------------------------------------------

pub struct IoBuffer {
    data: Vec<u8>,
    write_pos: usize,
    read_pos: usize,
}

impl IoBuffer {
    pub fn new() -> Self {
        IoBuffer {
            data: Vec::new(),
            write_pos: 0,
            read_pos: 0,
        }
    }
}

pub unsafe fn R_IoBufferWriteReset(iob: &mut IoBuffer) -> c_int {
    iob.write_pos = 0;
    iob.read_pos = 0;
    1
}

pub unsafe fn R_IoBufferReadReset(iob: &mut IoBuffer) -> c_int {
    iob.read_pos = 0;
    1
}

pub unsafe fn R_IoBufferInit(iob: &mut IoBuffer) -> c_int {
    iob.data.clear();
    iob.write_pos = 0;
    iob.read_pos = 0;
    1
}

pub unsafe fn R_IoBufferFree(iob: &mut IoBuffer) -> c_int {
    iob.data.clear();
    iob.data.shrink_to_fit();
    iob.write_pos = 0;
    iob.read_pos = 0;
    1
}

pub unsafe fn R_IoBufferPutc(c: c_int, iob: &mut IoBuffer) -> c_int {
    if iob.write_pos >= iob.data.len() {
        iob.data.push(c as u8);
    } else {
        iob.data[iob.write_pos] = c as u8;
    }
    iob.write_pos += 1;
    0
}

pub  fn R_IoBufferPuts(s: &str, iob: &mut IoBuffer) -> c_int { let mut n: c_int = 0;
    for byte in s.bytes() {
        R_IoBufferPutc(byte as c_int, iob);
        n += 1;
    }
    n
}
    let mut n: c_int = 0;
    for byte in s.bytes() {
        R_IoBufferPutc(byte as c_int, iob);
        n += 1;
    }
    n
}}

pub unsafe fn R_IoBufferGetc(iob: &mut IoBuffer) -> c_int {
    if iob.read_pos >= iob.write_pos {
        return -1;
    }
    let c = iob.data[iob.read_pos];
    iob.read_pos += 1;
    c as c_int
}

pub unsafe fn R_IoBufferReadOffset(iob: &mut IoBuffer) -> c_int {
    iob.read_pos as c_int
}

// ---------------------------------------------------------------------------
// TextBuffer — for reading from a STRSXP character vector
// ---------------------------------------------------------------------------

pub struct TextBuffer {
    buf: Vec<u8>,
    bufp: usize,
    text: SEXP,
    ntext: c_int,
    offset: c_int,
}

impl TextBuffer {
    pub fn new() -> Self {
        TextBuffer {
            buf: Vec::new(),
            bufp: 0,
            text: std::ptr::null_mut(),
            ntext: 0,
            offset: 0,
        }
    }
}

fn transfer_chars(src: &str) -> Vec<u8> {
    let mut v: Vec<u8> = src.bytes().collect();
    v.push(b'\n');
    v.push(0);
    v
}

pub  fn R_TextBufferInit(txtb: &mut TextBuffer, text: SEXP) -> c_int { if text.is_null() || TYPEOF(text) != SEXPTYPE::STRSXP {
        txtb.buf = Vec::new();
        txtb.bufp = 0;
        txtb.text = R_NilValue();
        txtb.ntext = 0;
        txtb.offset = 1;
        return 0;
    }

    let n = LENGTH(text) as c_int;
    txtb.text = text;
    txtb.ntext = n;
    txtb.offset = 0;

    if n > 0 {
        let s = string_elt(text, 0);
        txtb.buf = transfer_chars(&s);
    } else {
        txtb.buf = Vec::new();
    }
    txtb.bufp = 0;
    txtb.offset = 1;
    1
}
    if text.is_null() || TYPEOF(text) != SEXPTYPE::STRSXP {
        txtb.buf = Vec::new();
        txtb.bufp = 0;
        txtb.text = R_NilValue();
        txtb.ntext = 0;
        txtb.offset = 1;
        return 0;
    }

    let n = LENGTH(text) as c_int;
    txtb.text = text;
    txtb.ntext = n;
    txtb.offset = 0;

    if n > 0 {
        let s = string_elt(text, 0);
        txtb.buf = transfer_chars(&s);
    } else {
        txtb.buf = Vec::new();
    }
    txtb.bufp = 0;
    txtb.offset = 1;
    1
}}

pub unsafe fn R_TextBufferFree(_txtb: &mut TextBuffer) -> c_int {
    0
}

pub  fn R_TextBufferGetc(txtb: &mut TextBuffer) -> c_int { if txtb.buf.is_empty() {
        return -1;
    }
    if txtb.bufp >= txtb.buf.len() || txtb.buf[txtb.bufp] == 0 {
        if txtb.offset >= txtb.ntext {
            txtb.buf.clear();
            return -1;
        }
    }
    if txtb.bufp >= txtb.buf.len() {
        return -1;
    }
    let c = txtb.buf[txtb.bufp];
    txtb.bufp += 1;
    c as c_int
}
    if txtb.buf.is_empty() {
        return -1;
    }
    if txtb.bufp >= txtb.buf.len() || txtb.buf[txtb.bufp] == 0 {
        if txtb.offset >= txtb.ntext {
            txtb.buf.clear();
            return -1;
        }
    }
    if txtb.bufp >= txtb.buf.len() {
        return -1;
    }
    let c = txtb.buf[txtb.bufp];
    txtb.bufp += 1;
    c as c_int
}}

unsafe fn string_elt(s: SEXP, i: R_xlen_t) -> String { unsafe {
    if s.is_null() {
        return String::new();
    }
    let elt = STRING_ELT(s, i);
    if elt.is_null() {
        return String::new();
    }
    let ptr = CHAR(elt);
    if ptr.is_null() {
        return String::new();
    }
    let cstr = std::ffi::CStr::from_ptr(ptr);
    cstr.to_string_lossy().into_owned()
}}
