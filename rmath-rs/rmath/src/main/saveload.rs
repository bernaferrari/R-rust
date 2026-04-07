#![allow(
    unsafe_op_in_unsafe_fn,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]

//! Port of R's src/main/saveload.c -- R data file I/O utilities.
//!
//! This module ports the standalone I/O functions used for reading/writing
//! R data files (.rda, .rds formats), including version 1, 2, and 3 save
//! formats, old-style workspace loading, and XDR encode/decode helpers.

use crate::sexp::ffi::{SEXP, SEXPTYPE};
use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::io::{self, BufRead, Read, Write};
use std::os::raw::{c_char, c_double, c_int, c_uint, c_void};
use std::ptr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const R_MAGIC_ASCII_V3: c_int = 3001;
pub const R_MAGIC_BINARY_V3: c_int = 3002;
pub const R_MAGIC_XDR_V3: c_int = 3003;
pub const R_MAGIC_ASCII_V2: c_int = 2001;
pub const R_MAGIC_BINARY_V2: c_int = 2002;
pub const R_MAGIC_XDR_V2: c_int = 2003;
pub const R_MAGIC_ASCII_V1: c_int = 1001;
pub const R_MAGIC_BINARY_V1: c_int = 1002;
pub const R_MAGIC_XDR_V1: c_int = 1003;
pub const R_MAGIC_EMPTY: c_int = 999;
pub const R_MAGIC_CORRUPT: c_int = 998;
pub const R_MAGIC_MAYBE_TOONEW: c_int = 997;

/// Pre-1 format magic numbers (R < 0.99.0)
pub const R_MAGIC_BINARY: c_int = 1975;
pub const R_MAGIC_ASCII: c_int = 1976;
pub const R_MAGIC_XDR: c_int = 1977;
pub const R_MAGIC_BINARY_VERSION16: c_int = 1971;
pub const R_MAGIC_ASCII_VERSION16: c_int = 1972;

pub const R_XDR_DOUBLE_SIZE: usize = 8;
pub const R_XDR_INTEGER_SIZE: usize = 4;

const SMBUF_SIZE: usize = 512;

const HASHSIZE: usize = 1099;

const MAXELTSIZE: usize = 8192;

// SEXPTYPE constants -- now imported from crate::sexp::ffi::SEXPTYPE

// ---------------------------------------------------------------------------
// Stub helper for error/warning (local to this module)
// ---------------------------------------------------------------------------

unsafe fn error(msg: &str) {
    eprintln!("ERROR: {}", msg);
    std::process::abort();
}

unsafe fn error_cstr(msg: *const c_char) {
    let s = CStr::from_ptr(msg);
    let msg_str = s.to_str().unwrap_or("unknown error");
    error(msg_str);
}

unsafe fn warning_cstr(msg: *const c_char) {
    let s = CStr::from_ptr(msg);
    let msg_str = s.to_str().unwrap_or("warning");
    eprintln!("Warning: {}", msg_str);
}

// ---------------------------------------------------------------------------
// R_StringBuffer (simplified stub)
// ---------------------------------------------------------------------------

struct R_StringBuffer {
    data: Vec<u8>,
    bufsize: usize,
}

impl R_StringBuffer {
    fn new() -> Self {
        R_StringBuffer {
            data: vec![0u8; MAXELTSIZE],
            bufsize: MAXELTSIZE,
        }
    }

    fn data_ptr(&mut self) -> *mut c_char {
        self.data.as_mut_ptr() as *mut c_char
    }

    fn data_as_cstr(&mut self) -> *mut c_char {
        self.data.as_mut_ptr() as *mut c_char
    }

    fn ensure_size(&mut self, needed: usize) {
        if needed >= self.bufsize {
            self.bufsize = needed + 1;
            self.data.resize(self.bufsize, 0);
        }
    }
}

// ---------------------------------------------------------------------------
// SaveLoadData (simplified -- no XDR for now, just buffer)
// ---------------------------------------------------------------------------

struct SaveLoadData {
    buffer: R_StringBuffer,
    smbuf: [u8; SMBUF_SIZE],
}

impl SaveLoadData {
    fn new() -> Self {
        SaveLoadData {
            buffer: R_StringBuffer::new(),
            smbuf: [0u8; SMBUF_SIZE],
        }
    }
}

// ---------------------------------------------------------------------------
// Rcomplex
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Rcomplex {
    r: c_double,
    i: c_double,
}

// ---------------------------------------------------------------------------
// NodeInfo (for old-style restore)
// ---------------------------------------------------------------------------

struct NodeInfo {
    NSymbol: c_int,
    NSave: c_int,
    NTotal: c_int,
    NVSize: c_int,
    OldOffset: Vec<c_int>,
    NewAddress: SEXP,
}

// ---------------------------------------------------------------------------
// File Magic Number Read/Write (exported)
// ---------------------------------------------------------------------------

/// Write R save format magic number to a file (C FILE* version).
///
/// Port of: static void R_WriteMagic(FILE *fp, int number)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_WriteMagic(fp: *mut c_void, number: c_int) {
    let number = number.abs();
    let mut buf: [u8; 5] = [0; 5];

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
            buf[0] = ((number / 1000) % 10 + b'0' as c_int) as u8;
            buf[1] = ((number / 100) % 10 + b'0' as c_int) as u8;
            buf[2] = ((number / 10) % 10 + b'0' as c_int) as u8;
            buf[3] = (number % 10 + b'0' as c_int) as u8;
        }
    }
    buf[4] = b'\n';
    // Write to the FILE* -- we use libc::fwrite via extern
    if !fp.is_null() {
        // For the C API version, we just write. The caller is responsible
        // for having a valid FILE*.
        unsafe extern "C" {
            fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
        }
        unsafe {
            let _res = fwrite(buf.as_ptr() as *const c_void, 1, 5, fp);
        }
    }
}

/// Read R save format magic number from a file (C FILE* version).
///
/// Port of: static int R_ReadMagic(FILE *fp)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ReadMagic(fp: *mut c_void) -> c_int {
    let mut buf: [u8; 6] = [0; 6];

    unsafe extern "C" {
        fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
        fn strncmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int;
    }

    let count = unsafe { fread(buf.as_mut_ptr() as *mut c_void, 1, 5, fp) };

    if count != 5 {
        if count == 0 {
            return R_MAGIC_EMPTY;
        } else {
            return R_MAGIC_CORRUPT;
        }
    }

    let buf_ptr = buf.as_ptr() as *const c_char;

    unsafe {
        if strncmp(buf_ptr, b"RDA1\n\0".as_ptr() as *const c_char, 5) == 0 {
            return R_MAGIC_ASCII_V1;
        }
        if strncmp(buf_ptr, b"RDB1\n\0".as_ptr() as *const c_char, 5) == 0 {
            return R_MAGIC_BINARY_V1;
        }
        if strncmp(buf_ptr, b"RDX1\n\0".as_ptr() as *const c_char, 5) == 0 {
            return R_MAGIC_XDR_V1;
        }
        if strncmp(buf_ptr, b"RDA2\n\0".as_ptr() as *const c_char, 5) == 0 {
            return R_MAGIC_ASCII_V2;
        }
        if strncmp(buf_ptr, b"RDB2\n\0".as_ptr() as *const c_char, 5) == 0 {
            return R_MAGIC_BINARY_V2;
        }
        if strncmp(buf_ptr, b"RDX2\n\0".as_ptr() as *const c_char, 5) == 0 {
            return R_MAGIC_XDR_V2;
        }
        if strncmp(buf_ptr, b"RDA3\n\0".as_ptr() as *const c_char, 5) == 0 {
            return R_MAGIC_ASCII_V3;
        }
        if strncmp(buf_ptr, b"RDB3\n\0".as_ptr() as *const c_char, 5) == 0 {
            return R_MAGIC_BINARY_V3;
        }
        if strncmp(buf_ptr, b"RDX3\n\0".as_ptr() as *const c_char, 5) == 0 {
            return R_MAGIC_XDR_V3;
        }
        if strncmp(buf_ptr, b"RD\0".as_ptr() as *const c_char, 2) == 0 {
            return R_MAGIC_MAYBE_TOONEW;
        }
    }

    // Try to parse as 4-digit number
    let d1 = (buf[3] as c_int - b'0' as c_int).rem_euclid(10);
    let d2 = (buf[2] as c_int - b'0' as c_int).rem_euclid(10);
    let d3 = (buf[1] as c_int - b'0' as c_int).rem_euclid(10);
    let d4 = (buf[0] as c_int - b'0' as c_int).rem_euclid(10);
    d1 + 10 * d2 + 100 * d3 + 1000 * d4
}

// ---------------------------------------------------------------------------
// Default save version
// ---------------------------------------------------------------------------

/// Port of: static int defaultSaveVersion(void)
pub unsafe fn defaultSaveVersion() -> c_int {
    thread_local! { static DFLT: Cell<c_int> = Cell::new(-1); }

    unsafe {
        if DFLT.with(|v| v.get()) < 0 {
            let val = std::env::var("R_DEFAULT_SAVE_VERSION")
                .ok()
                .and_then(|s| s.trim().parse::<c_int>().ok())
                .unwrap_or(-1);
            if val == 2 || val == 3 {
                DFLT.with(|v| v.set(val));
            } else {
                DFLT.with(|v| v.set(3));
            }
        }
        DFLT.with(|v| v.get())
    }
}

// ---------------------------------------------------------------------------
// Dummy placeholder routines (no-ops)
// ---------------------------------------------------------------------------

unsafe fn DummyInit(_fp: *mut c_void, _d: *mut SaveLoadData) {}

unsafe fn DummyOutSpace(_fp: *mut c_void, _nspace: c_int, _d: *mut SaveLoadData) {}

unsafe fn DummyOutNewline(_fp: *mut c_void, _d: *mut SaveLoadData) {}

unsafe fn DummyTerm(_fp: *mut c_void, _d: *mut SaveLoadData) {}

// ---------------------------------------------------------------------------
// ASCII I/O routines (old-style, FILE* based)
// ---------------------------------------------------------------------------

unsafe fn OutSpaceAscii(fp: *mut c_void, nspace: c_int, _d: *mut SaveLoadData) {
    unsafe extern "C" {
        fn fputc(c: c_int, stream: *mut c_void) -> c_int;
    }
    let mut n = nspace;
    while n > 0 {
        unsafe {
            fputc(b' ' as c_int, fp);
        }
        n -= 1;
    }
}

unsafe fn OutNewlineAscii(fp: *mut c_void, _d: *mut SaveLoadData) {
    unsafe extern "C" {
        fn fputc(c: c_int, stream: *mut c_void) -> c_int;
    }
    unsafe {
        fputc(b'\n' as c_int, fp);
    }
}

unsafe fn OutIntegerAscii(fp: *mut c_void, x: c_int, _d: *mut SaveLoadData) {
    unsafe extern "C" {
        fn fprintf(stream: *mut c_void, format: *const c_char, ...) -> c_int;
    }
    if x == c_int::MIN {
        unsafe {
            fprintf(fp, b"NA\0".as_ptr() as *const c_char);
        }
    } else {
        unsafe {
            fprintf(fp, b"%d\0".as_ptr() as *const c_char, x);
        }
    }
}

unsafe fn InIntegerAscii(fp: *mut c_void, _d: *mut SaveLoadData) -> c_int {
    unsafe extern "C" {
        fn fscanf(stream: *mut c_void, format: *const c_char, ...) -> c_int;
        fn sscanf(str: *const c_char, format: *const c_char, ...) -> c_int;
        fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int;
    }
    let mut buf: [c_char; 128] = [0; 128];
    let res = fscanf(fp, b"%127s\0".as_ptr() as *const c_char, buf.as_mut_ptr());
    if res != 1 {
        return c_int::MIN; // read error
    }
    if strcmp(buf.as_ptr(), b"NA\0".as_ptr() as *const c_char) == 0 {
        return c_int::MIN; // NA_INTEGER
    }
    let mut x: c_int = 0;
    let res2 = sscanf(buf.as_ptr(), b"%d\0".as_ptr() as *const c_char, &mut x);
    if res2 != 1 {
        return c_int::MIN; // read error
    }
    x
}

unsafe fn OutDoubleAscii(fp: *mut c_void, x: c_double, _d: *mut SaveLoadData) {
    unsafe extern "C" {
        fn fprintf(stream: *mut c_void, format: *const c_char, ...) -> c_int;
    }
    if x.is_nan() {
        unsafe {
            fprintf(fp, b"NA\0".as_ptr() as *const c_char);
        }
    } else if x.is_infinite() {
        if x < 0.0 {
            unsafe {
                fprintf(fp, b"-Inf\0".as_ptr() as *const c_char);
            }
        } else {
            unsafe {
                fprintf(fp, b"Inf\0".as_ptr() as *const c_char);
            }
        }
    } else {
        unsafe {
            fprintf(fp, b"%.16g\0".as_ptr() as *const c_char, x);
        }
    }
}

unsafe fn InDoubleAscii(fp: *mut c_void, _d: *mut SaveLoadData) -> c_double {
    unsafe extern "C" {
        fn fscanf(stream: *mut c_void, format: *const c_char, ...) -> c_int;
        fn sscanf(str: *const c_char, format: *const c_char, ...) -> c_int;
        fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int;
    }
    let mut buf: [c_char; 128] = [0; 128];
    let res = fscanf(fp, b"%127s\0".as_ptr() as *const c_char, buf.as_mut_ptr());
    if res != 1 {
        return f64::NAN; // read error
    }
    if strcmp(buf.as_ptr(), b"NA\0".as_ptr() as *const c_char) == 0 {
        return f64::NAN; // NA_REAL
    } else if strcmp(buf.as_ptr(), b"Inf\0".as_ptr() as *const c_char) == 0 {
        return f64::INFINITY; // R_PosInf
    } else if strcmp(buf.as_ptr(), b"-Inf\0".as_ptr() as *const c_char) == 0 {
        return f64::NEG_INFINITY; // R_NegInf
    }
    let mut x: c_double = 0.0;
    let res2 = sscanf(buf.as_ptr(), b"%lg\0".as_ptr() as *const c_char, &mut x);
    if res2 != 1 {
        return f64::NAN; // read error
    }
    x
}

unsafe fn OutComplexAscii(fp: *mut c_void, x: Rcomplex, _d: *mut SaveLoadData) {
    if x.r.is_nan() || x.i.is_nan() {
        unsafe extern "C" {
            fn fprintf(stream: *mut c_void, format: *const c_char, ...) -> c_int;
        }
        unsafe {
            fprintf(fp, b"NA NA\0".as_ptr() as *const c_char);
        }
    } else {
        unsafe {
            OutDoubleAscii(fp, x.r, _d);
            OutSpaceAscii(fp, 1, _d);
            OutDoubleAscii(fp, x.i, _d);
        }
    }
}

unsafe fn InComplexAscii(fp: *mut c_void, d: *mut SaveLoadData) -> Rcomplex {
    Rcomplex {
        r: InDoubleAscii(fp, d),
        i: InDoubleAscii(fp, d),
    }
}

unsafe fn OutStringAscii(fp: *mut c_void, x: *const c_char, _d: *mut SaveLoadData) {
    unsafe extern "C" {
        fn fprintf(stream: *mut c_void, format: *const c_char, ...) -> c_int;
        fn fputc(c: c_int, stream: *mut c_void) -> c_int;
        fn strlen(s: *const c_char) -> usize;
    }
    let nbytes = unsafe { strlen(x) };
    unsafe {
        fprintf(fp, b"%zu \0".as_ptr() as *const c_char, nbytes);
    }
    let bytes = unsafe { std::slice::from_raw_parts(x as *const u8, nbytes) };
    for i in 0..nbytes {
        match bytes[i] {
            b'\n' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'n' as c_int, fp);
            },
            b'\t' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b't' as c_int, fp);
            },
            b'\x0B' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'v' as c_int, fp);
            },
            b'\x08' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'b' as c_int, fp);
            },
            b'\r' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'r' as c_int, fp);
            },
            b'\x0C' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'f' as c_int, fp);
            },
            b'\x07' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'a' as c_int, fp);
            },
            b'\\' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'\\' as c_int, fp);
            },
            b'\x3F' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'?' as c_int, fp);
            },
            b'\'' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'\'' as c_int, fp);
            },
            b'"' => unsafe {
                fputc(b'\\' as c_int, fp);
                fputc(b'"' as c_int, fp);
            },
            _ => {
                if bytes[i] <= 32 || bytes[i] > 126 {
                    unsafe {
                        fprintf(fp, b"\\%03o\0".as_ptr() as *const c_char, bytes[i] as c_int);
                    }
                } else {
                    unsafe {
                        fputc(bytes[i] as c_int, fp);
                    }
                }
            }
        }
    }
}

unsafe fn InStringAscii(_fp: *mut c_void, _d: *mut SaveLoadData) -> *mut c_char {
    thread_local! { static BUF_ASCII: RefCell<[u8; 4096]> = RefCell::new([0u8; 4096]); }
    BUF_ASCII.with(|v| v.as_ptr() as *mut c_char)
}

// ---------------------------------------------------------------------------
// Binary I/O routines (old-style, FILE* based)
// ---------------------------------------------------------------------------

unsafe fn BinaryInInteger(fp: *mut c_void, _unused: *mut SaveLoadData) -> c_int {
    unsafe extern "C" {
        fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    }
    let mut i: c_int = 0;
    let count = unsafe {
        fread(
            &mut i as *mut c_int as *mut c_void,
            std::mem::size_of::<c_int>(),
            1,
            fp,
        )
    };
    if count != 1 {
        unsafe {
            error("a read error occurred");
        }
    }
    i
}

unsafe fn BinaryInReal(fp: *mut c_void, _unused: *mut SaveLoadData) -> c_double {
    unsafe extern "C" {
        fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    }
    let mut x: c_double = 0.0;
    let count = unsafe {
        fread(
            &mut x as *mut c_double as *mut c_void,
            std::mem::size_of::<c_double>(),
            1,
            fp,
        )
    };
    if count != 1 {
        unsafe {
            error("a read error occurred");
        }
    }
    x
}

unsafe fn BinaryInComplex(fp: *mut c_void, _unused: *mut SaveLoadData) -> Rcomplex {
    unsafe extern "C" {
        fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    }
    let mut x = Rcomplex { r: 0.0, i: 0.0 };
    let count = unsafe {
        fread(
            &mut x as *mut Rcomplex as *mut c_void,
            std::mem::size_of::<Rcomplex>(),
            1,
            fp,
        )
    };
    if count != 1 {
        unsafe {
            error("a read error occurred");
        }
    }
    x
}

unsafe fn BinaryInString(fp: *mut c_void, _d: *mut SaveLoadData) -> *mut c_char {
    thread_local! { static BUF_BINARY: RefCell<[u8; 65536]> = RefCell::new([0u8; 65536]); }
    unsafe extern "C" {
        fn R_fgetc(stream: *mut c_void) -> c_int;
    }
    let mut bufp = 0usize;
    loop {
        let c = unsafe { R_fgetc(fp) };
        BUF_BINARY.with(|v| v.borrow_mut()[bufp] = c as u8);
        bufp += 1;
        if c as u8 == 0 {
            break;
        }
        if bufp >= 65535 {
            break;
        }
    }
    BUF_BINARY.with(|v| v.as_ptr() as *mut c_char)
}

// ---------------------------------------------------------------------------
// XDR I/O routines (stubs -- real XDR would need libxdr or manual encoding)
// ---------------------------------------------------------------------------

unsafe fn XdrInInit(_fp: *mut c_void, _d: *mut SaveLoadData) {
    // XDR init stub -- would need xdrstdio_create
}

unsafe fn XdrInTerm(_fp: *mut c_void, _d: *mut SaveLoadData) {
    // XDR destroy stub
}

unsafe fn XdrInInteger(_fp: *mut c_void, _d: *mut SaveLoadData) -> c_int {
    unsafe {
        error("a I read error occurred");
    }
    unreachable!()
}

unsafe fn XdrInReal(_fp: *mut c_void, _d: *mut SaveLoadData) -> c_double {
    unsafe {
        error("a R read error occurred");
    }
    unreachable!()
}

unsafe fn XdrInComplex(_fp: *mut c_void, _d: *mut SaveLoadData) -> Rcomplex {
    unsafe {
        error("a C read error occurred");
    }
    unreachable!()
}

unsafe fn XdrInString(_fp: *mut c_void, _d: *mut SaveLoadData) -> *mut c_char {
    thread_local! { static BUF_XDRIN: RefCell<[u8; 65536]> = RefCell::new([0u8; 65536]); }
    BUF_XDRIN.with(|v| v.as_ptr() as *mut c_char)
}

unsafe fn OutInitXdr(_fp: *mut c_void, _d: *mut SaveLoadData) {}

unsafe fn OutTermXdr(_fp: *mut c_void, _d: *mut SaveLoadData) {}

unsafe fn InTermXdr(_fp: *mut c_void, _d: *mut SaveLoadData) {}

unsafe fn OutIntegerXdr(_fp: *mut c_void, _i: c_int, _d: *mut SaveLoadData) {
    // XDR integer write stub
}

unsafe fn InIntegerXdr(_fp: *mut c_void, _d: *mut SaveLoadData) -> c_int {
    unsafe {
        error("an xdr integer data read error occurred");
    }
    unreachable!()
}

unsafe fn OutStringXdr(_fp: *mut c_void, _s: *const c_char, _d: *mut SaveLoadData) {
    // XDR string write stub
}

unsafe fn InStringXdr(_fp: *mut c_void, _d: *mut SaveLoadData) -> *mut c_char {
    thread_local! { static BUF_INSTRINGXDR: RefCell<[u8; 65536]> = RefCell::new([0u8; 65536]); }
    BUF_INSTRINGXDR.with(|v| v.as_ptr() as *mut c_char)
}

unsafe fn OutRealXdr(_fp: *mut c_void, _x: c_double, _d: *mut SaveLoadData) {
    // XDR real write stub
}

unsafe fn InRealXdr(_fp: *mut c_void, _d: *mut SaveLoadData) -> c_double {
    unsafe {
        error("an xdr real data read error occurred");
    }
    unreachable!()
}

unsafe fn OutComplexXdr(_fp: *mut c_void, _x: Rcomplex, _d: *mut SaveLoadData) {
    // XDR complex write stub
}

unsafe fn InComplexXdr(_fp: *mut c_void, _d: *mut SaveLoadData) -> Rcomplex {
    unsafe {
        error("an xdr complex data read error occurred");
    }
    unreachable!()
}

// ---------------------------------------------------------------------------
// Binary input routines (version 1 new-style)
// ---------------------------------------------------------------------------

unsafe fn InIntegerBinary(_fp: *mut c_void, _unused: *mut SaveLoadData) -> c_int {
    unsafe {
        error("a binary read error occurred");
    }
    unreachable!()
}

unsafe fn InStringBinary(_fp: *mut c_void, _unused: *mut SaveLoadData) -> *mut c_char {
    thread_local! { static BUF_INSTRINGBINARY: RefCell<[u8; 65536]> = RefCell::new([0u8; 65536]); }
    BUF_INSTRINGBINARY.with(|v| v.as_ptr() as *mut c_char)
}

unsafe fn InRealBinary(_fp: *mut c_void, _unused: *mut SaveLoadData) -> c_double {
    unsafe {
        error("a read error occurred");
    }
    unreachable!()
}

unsafe fn InComplexBinary(_fp: *mut c_void, _unused: *mut SaveLoadData) -> Rcomplex {
    unsafe {
        error("a read error occurred");
    }
    unreachable!()
}

// ---------------------------------------------------------------------------
// Old-style Ascii I/O (pre-v1)
// ---------------------------------------------------------------------------

unsafe fn AsciiInInteger(_fp: *mut c_void, _d: *mut SaveLoadData) -> c_int {
    c_int::MIN
}

unsafe fn AsciiInReal(_fp: *mut c_void, _d: *mut SaveLoadData) -> c_double {
    f64::NAN
}

unsafe fn AsciiInComplex(_fp: *mut c_void, _d: *mut SaveLoadData) -> Rcomplex {
    Rcomplex {
        r: f64::NAN,
        i: f64::NAN,
    }
}

unsafe fn AsciiInString(_fp: *mut c_void, _d: *mut SaveLoadData) -> *mut c_char {
    thread_local! { static BUF_ASCIIIN: RefCell<[u8; 65536]> = RefCell::new([0u8; 65536]); }
    BUF_ASCIIIN.with(|v| v.as_ptr() as *mut c_char)
}

// ---------------------------------------------------------------------------
// FixupType -- remap old SEXPTYPEs for version compatibility
// ---------------------------------------------------------------------------

unsafe fn FixupType(mut type_: c_uint, VersionId: c_int) -> c_uint {
    if VersionId != 0 {
        match VersionId {
            16 => {
                // In the version 0.16.1 -> 0.50 switch we introduced complex values
                // and found that numeric/complex numbers had to be contiguous.
                if type_ == SEXPTYPE::STRSXP.0 as c_uint {
                    type_ = SEXPTYPE::CPLXSXP.0 as c_uint;
                } else if type_ == SEXPTYPE::CPLXSXP.0 as c_uint {
                    type_ = SEXPTYPE::STRSXP.0 as c_uint;
                }
            }
            _ => unsafe {
                error_cstr(
                    b"restore compatibility error - no version compatibility\0".as_ptr()
                        as *const c_char,
                );
            },
        }
    }
    // Map old factors to new (0.61->0.62): types 11, 12 -> 13 (INTSXP)
    if type_ == 11 || type_ == 12 {
        type_ = 13;
    }
    type_
}

// ---------------------------------------------------------------------------
// RestoreError -- error handler for old-style restore
// ---------------------------------------------------------------------------

unsafe fn RestoreError(msg: *const c_char, startup: c_int) {
    if startup != 0 {
        unsafe {
            error_cstr(msg);
        }
    } else {
        unsafe {
            error_cstr(msg);
        }
    }
}

// ---------------------------------------------------------------------------
// OffsetToNode -- binary search for offset in old-style restore
// ---------------------------------------------------------------------------

unsafe fn OffsetToNode(offset: c_int, node: *const NodeInfo) -> SEXP {
    let node_ref = &*node;

    if offset == -1 {
        return R_NilValue();
    }
    if offset == -2 {
        return R_GlobalEnv();
    }
    if offset == -3 {
        return R_UnboundValue();
    }
    if offset == -4 {
        return R_MissingArg();
    }

    // Binary search for offset
    let mut l: c_int = 0;
    let mut r = node_ref.NTotal - 1;
    loop {
        let m = (l + r) / 2;
        if offset < node_ref.OldOffset[m as usize] {
            r = m - 1;
        } else {
            l = m + 1;
        }
        if offset == node_ref.OldOffset[m as usize] || l > r {
            if offset == node_ref.OldOffset[m as usize] {
                return VECTOR_ELT(node_ref.NewAddress, m as usize);
            }
            break;
        }
    }

    // Not supposed to happen
    unsafe {
        warning_cstr(b"unresolved node during restore\0".as_ptr() as *const c_char);
    }
    R_NilValue()
}

// ---------------------------------------------------------------------------
// SEXP accessor helpers
// ---------------------------------------------------------------------------

unsafe fn R_GlobalEnv() -> SEXP {
    ptr::null_mut()
}

unsafe fn R_UnboundValue() -> SEXP {
    ptr::null_mut()
}

unsafe fn R_MissingArg() -> SEXP {
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Local wrappers delegating to existing Rust implementations
// ---------------------------------------------------------------------------

use crate::sexp::accessors::{
    ATTRIB as r_ATTRIB, BODY as r_BODY, CAR as r_CAR, CDR as r_CDR, CHAR as r_CHAR,
    CHAR_RW as r_CHAR_RW, CLOENV as r_CLOENV, COMPLEX as r_COMPLEX, ENCLOS as r_ENCLOS,
    FORMALS as r_FORMALS, FRAME as r_FRAME, INTEGER as r_INTEGER, LENGTH as r_LENGTH,
    LEVELS as r_LEVELS, NAMED as r_NAMED, OBJECT as r_OBJECT, PRINTNAME as r_PRINTNAME,
    REAL as r_REAL, SET_ATTRIB as r_SET_ATTRIB, SET_ENCLOS as r_SET_ENCLOS,
    SET_FRAME as r_SET_FRAME, SET_NAMED as r_SET_NAMED, SET_OBJECT as r_SET_OBJECT,
    SET_PRIMOFFSET as r_SET_PRIMOFFSET, SET_STRING_ELT as r_SET_STRING_ELT,
    SET_TRUELENGTH as r_SET_TRUELENGTH, SET_VECTOR_ELT as r_SET_VECTOR_ELT,
    SETLEVELS as r_SETLEVELS, SETTAG as r_SETTAG, STRING_ELT as r_STRING_ELT, TAG as r_TAG,
    TRUELENGTH as r_TRUELENGTH, TYPEOF as r_TYPEOF, VECTOR_ELT as r_VECTOR_ELT,
};
use crate::sexp::constructors::{
    Rf_allocList as r_allocList, Rf_allocVector as r_allocVector, Rf_cons as r_cons,
    Rf_isNull as r_isNull, Rf_mkChar as r_mkChar,
};
use crate::sexp::ffi::NA_INTEGER;
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::allocSExp as r_allocSExp;
use crate::sexp::protect::{Rf_protect as r_protect, Rf_unprotect as r_unprotect};
use crate::sexp::symbol::Rf_install as r_install;

#[inline(always)]
unsafe fn TYPEOF(s: SEXP) -> c_int {
    if s.is_null() {
        SEXPTYPE::NILSXP.0
    } else {
        r_TYPEOF(s)
    }
}
#[inline(always)]
unsafe fn LENGTH(s: SEXP) -> c_int {
    r_LENGTH(s)
}
#[inline(always)]
unsafe fn CAR(s: SEXP) -> SEXP {
    r_CAR(s)
}
#[inline(always)]
unsafe fn CDR(s: SEXP) -> SEXP {
    r_CDR(s)
}
#[inline(always)]
unsafe fn TAG(s: SEXP) -> SEXP {
    r_TAG(s)
}
#[inline(always)]
unsafe fn ATTRIB(s: SEXP) -> SEXP {
    r_ATTRIB(s)
}
#[inline(always)]
unsafe fn OBJECT(s: SEXP) -> c_int {
    r_OBJECT(s)
}
#[inline(always)]
unsafe fn LEVELS(s: SEXP) -> c_int {
    r_LEVELS(s)
}
#[inline(always)]
unsafe fn CHAR(s: SEXP) -> *const c_char {
    r_CHAR(s)
}
#[inline(always)]
unsafe fn PRINTNAME(s: SEXP) -> SEXP {
    r_PRINTNAME(s)
}
#[inline(always)]
unsafe fn REAL(s: SEXP) -> *mut c_double {
    r_REAL(s)
}
#[inline(always)]
unsafe fn INTEGER(s: SEXP) -> *mut c_int {
    r_INTEGER(s)
}
#[inline(always)]
unsafe fn COMPLEX(s: SEXP) -> *mut crate::sexp::ffi::Rcomplex {
    r_COMPLEX(s)
}
#[inline(always)]
unsafe fn STRING_ELT(s: SEXP, i: usize) -> SEXP {
    r_STRING_ELT(s, i as i64)
}
#[inline(always)]
unsafe fn SET_STRING_ELT(s: SEXP, i: usize, val: SEXP) {
    r_SET_STRING_ELT(s, i as i64, val);
}
#[inline(always)]
unsafe fn VECTOR_ELT(s: SEXP, i: usize) -> SEXP {
    r_VECTOR_ELT(s, i as i64)
}
#[inline(always)]
unsafe fn SET_VECTOR_ELT(s: SEXP, i: usize, val: SEXP) {
    r_SET_VECTOR_ELT(s, i as i64, val);
}
#[inline(always)]
unsafe fn SET_TAG(s: SEXP, val: SEXP) {
    r_SETTAG(s, val);
}
#[inline(always)]
unsafe fn SET_ATTRIB(s: SEXP, val: SEXP) {
    r_SET_ATTRIB(s, val);
}
#[inline(always)]
unsafe fn SET_OBJECT(s: SEXP, v: c_int) {
    r_SET_OBJECT(s, v);
}
#[inline(always)]
unsafe fn SETLEVELS(s: SEXP, v: c_int) {
    r_SETLEVELS(s, v);
}
#[inline(always)]
unsafe fn SET_ENCLOS(s: SEXP, val: SEXP) {
    r_SET_ENCLOS(s, val);
}
#[inline(always)]
unsafe fn SET_FRAME(s: SEXP, val: SEXP) {
    r_SET_FRAME(s, val);
}
#[inline(always)]
unsafe fn ENCLOS(s: SEXP) -> SEXP {
    r_ENCLOS(s)
}
#[inline(always)]
unsafe fn FRAME(s: SEXP) -> SEXP {
    r_FRAME(s)
}
#[inline(always)]
unsafe fn CLOENV(s: SEXP) -> SEXP {
    r_CLOENV(s)
}
#[inline(always)]
unsafe fn FORMALS(s: SEXP) -> SEXP {
    r_FORMALS(s)
}
#[inline(always)]
unsafe fn BODY(s: SEXP) -> SEXP {
    r_BODY(s)
}
#[inline(always)]
unsafe fn EXTPTR_PROT(s: SEXP) -> SEXP {
    (*s).data.extptr[1] as SEXP
}
#[inline(always)]
unsafe fn EXTPTR_TAG(s: SEXP) -> SEXP {
    (*s).data.extptr[2] as SEXP
}
#[inline(always)]
unsafe fn SET_PRIMOFFSET(s: SEXP, offset: c_int) {
    r_SET_PRIMOFFSET(s, offset);
}
#[inline(always)]
unsafe fn TRUELENGTH(s: SEXP) -> c_int {
    r_TRUELENGTH(s)
}
#[inline(always)]
unsafe fn SET_TRUELENGTH(s: SEXP, v: c_int) {
    r_SET_TRUELENGTH(s, v);
}
#[inline(always)]
unsafe fn Rf_isNull(s: SEXP) -> c_int {
    r_isNull(s)
}
#[inline(always)]
unsafe fn PROTECT(s: SEXP) -> SEXP {
    r_protect(s)
}
#[inline(always)]
unsafe fn UNPROTECT(n: c_int) {
    r_unprotect(n);
}
#[inline(always)]
unsafe fn install(name: *const c_char) -> SEXP {
    r_install(name)
}
#[inline(always)]
unsafe fn mkChar(s: *const c_char) -> SEXP {
    r_mkChar(s)
}

#[inline(always)]
unsafe fn SETCAR(s: SEXP, val: SEXP) {
    crate::sexp::accessors::SETCAR(s, val);
}
#[inline(always)]
unsafe fn SETCDR(s: SEXP, val: SEXP) {
    crate::sexp::accessors::SETCDR(s, val);
}
#[inline(always)]
unsafe fn allocVector(type_: c_int, len: c_int) -> SEXP {
    r_allocVector(type_, len)
}
#[inline(always)]
unsafe fn allocSExp(type_: c_int) -> SEXP {
    // SAFETY: SEXPTYPE is #[repr(transparent)] over c_int, so this is a no-op transmute
    r_allocSExp(std::mem::transmute::<c_int, SEXPTYPE>(type_))
}
#[inline(always)]
unsafe fn allocList(len: c_int) -> SEXP {
    r_allocList(len)
}
#[inline(always)]
unsafe fn CONS(car: SEXP, cdr: SEXP) -> SEXP {
    r_cons(car, cdr)
}
#[inline(always)]
unsafe fn SETCAR_list(s: SEXP, val: SEXP) {
    crate::sexp::accessors::SETCAR(s, val);
}

unsafe fn checkArity(op: SEXP, args: SEXP) {
    crate::main::errors::Rf_checkArityCall(op, args, crate::main::errors::getCurrentCall());
}

#[inline(always)]
unsafe fn coerceVector(s: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(s, type_)
}

#[inline(always)]
unsafe fn asInteger(s: SEXP) -> c_int {
    crate::main::coerce::asInteger(s)
}

#[inline(always)]
unsafe fn asLogical(s: SEXP) -> c_int {
    crate::main::coerce::asLogical(s)
}

#[inline(always)]
unsafe fn nthcdr(s: SEXP, n: c_int) -> SEXP {
    let mut x = s;
    for _ in 0..n {
        if x.is_null() {
            break;
        }
        x = r_CDR(x);
    }
    x
}

#[inline(always)]
unsafe fn R_findVar(sym: SEXP, env: SEXP) -> SEXP {
    if sym.is_null() || env.is_null() {
        return ptr::null_mut();
    }
    if TYPEOF(env) != SEXPTYPE::ENVSXP.0 {
        return ptr::null_mut();
    }
    let mut frame = FRAME(env);
    while !frame.is_null() {
        if TAG(frame) == sym {
            return CAR(frame);
        }
        frame = CDR(frame);
    }
    let enc = ENCLOS(env);
    if !enc.is_null() {
        return R_findVar(sym, enc);
    }
    ptr::null_mut()
}

#[inline(always)]
unsafe fn installTrChar(s: SEXP) -> SEXP {
    crate::main::sysutils::installTrChar(s)
}

#[inline(always)]
unsafe fn isValidString(s: SEXP) -> c_int {
    if s.is_null() || TYPEOF(s) != SEXPTYPE::STRSXP.0 || LENGTH(s) != 1 {
        return 0;
    }
    let x = STRING_ELT(s, 0);
    if x.is_null() || TYPEOF(x) != SEXPTYPE::CHARSXP.0 {
        return 0;
    }
    let cs = CHAR(x);
    if cs.is_null() || *cs == 0 {
        return 0;
    }
    1
}

#[inline(always)]
unsafe fn isValidStringF(s: SEXP) -> c_int {
    if s.is_null() || TYPEOF(s) != SEXPTYPE::STRSXP.0 || LENGTH(s) != 1 {
        return 0;
    }
    let x = STRING_ELT(s, 0);
    if x.is_null() || TYPEOF(x) != SEXPTYPE::CHARSXP.0 {
        return 0;
    }
    1
}

#[inline(always)]
unsafe fn EncodeChar(s: SEXP) -> *const c_char {
    crate::main::printutils::EncodeChar(s)
}

#[inline(always)]
unsafe fn defineVar(sym: SEXP, val: SEXP, env: SEXP) {
    if sym.is_null() || env.is_null() {
        return;
    }
    if TYPEOF(env) != SEXPTYPE::ENVSXP.0 {
        return;
    }
    // Simple implementation: search for existing binding, else cons onto frame
    let mut frame = FRAME(env);
    let mut prev: SEXP = ptr::null_mut();
    let mut cell = frame;
    while !cell.is_null() {
        if TAG(cell) == sym {
            crate::sexp::accessors::SETCAR(cell, val);
            return;
        }
        prev = cell;
        cell = CDR(cell);
    }
    let new_cell = CONS(sym, val);
    SET_TAG(new_cell, sym);
    if !prev.is_null() {
        SETCDR(prev, new_cell);
    } else {
        SET_FRAME(env, new_cell);
    }
}

#[inline(always)]
unsafe fn getAttrib(s: SEXP, what: SEXP) -> SEXP {
    crate::main::relop::getAttrib(s, what)
}

#[inline(always)]
unsafe fn R_NamesSymbol() -> SEXP {
    crate::main::relop::R_NamesSymbol()
}

#[inline(always)]
unsafe fn isList(s: SEXP) -> c_int {
    let t = TYPEOF(s);
    if t == SEXPTYPE::LISTSXP.0 { 1 } else { 0 }
}

#[inline(always)]
unsafe fn length(s: SEXP) -> c_int {
    if s.is_null() {
        return 0;
    }
    let t = TYPEOF(s);
    match t {
        t if t == SEXPTYPE::LISTSXP.0
            || t == SEXPTYPE::LANGSXP.0
            || t == SEXPTYPE::DOTSXP.0
            || t == SEXPTYPE::CLOSXP.0
            || t == SEXPTYPE::PROMSXP.0
            || t == SEXPTYPE::ENVSXP.0
            || t == SEXPTYPE::EXTPTRSXP.0
            || t == SEXPTYPE::WEAKREFSXP.0 =>
        {
            let mut n: c_int = 0;
            let mut x = s;
            while !x.is_null() && TYPEOF(x) == t {
                n += 1;
                x = CDR(x);
            }
            n
        }
        _ => LENGTH(s),
    }
}

#[inline(always)]
unsafe fn R_seemsOldStyleS4Object(s: SEXP) -> c_int {
    crate::main::objects::R_seemsOldStyleS4Object(s)
}

unsafe fn warningcall(call: SEXP, format: *const c_char) {
    let _ = call;
    let s = CStr::from_ptr(format);
    let msg = s.to_str().unwrap_or("warning");
    eprintln!("Warning: {}", msg);
}

#[inline(always)]
unsafe fn LCONS(car: SEXP, cdr: SEXP) -> SEXP {
    r_cons(car, cdr)
}

#[inline(always)]
unsafe fn R_BaseEnv() -> SEXP {
    crate::sexp::globals::R_BaseEnv()
}

#[inline(always)]
unsafe fn R_BaseNamespace() -> SEXP {
    ptr::null_mut()
}

#[inline(always)]
unsafe fn R_IsNamespaceEnv(s: SEXP) -> c_int {
    if s.is_null() {
        return 0;
    }
    if TYPEOF(s) != SEXPTYPE::ENVSXP.0 {
        return 0;
    }
    0
}

#[inline(always)]
unsafe fn R_HasFancyBindings(s: SEXP) -> c_int {
    0
}

unsafe fn mkTrue() -> SEXP {
    let x = r_allocVector(SEXPTYPE::LGLSXP.0, 1);
    *INTEGER(x) = 1;
    x
}

unsafe fn mkFalse() -> SEXP {
    let x = r_allocVector(SEXPTYPE::LGLSXP.0, 1);
    *INTEGER(x) = 0;
    x
}

unsafe fn eval(expr: SEXP, env: SEXP) -> SEXP {
    crate::eval::eval::Rf_eval(expr, env)
}

#[inline(always)]
unsafe fn CHAR_RW(s: SEXP) -> *mut c_char {
    r_CHAR(s) as *mut c_char
}

#[inline(always)]
unsafe fn allocCharsxp(len: c_int) -> SEXP {
    if len < 0 {
        return ptr::null_mut();
    }
    crate::sexp::memory::with_arena(|arena| {
        arena.alloc_charsxp_empty(len as crate::sexp::ffi::R_xlen_t)
    })
}

unsafe fn R_AllocStringBuffer(len: usize, buf: *mut R_StringBuffer) {
    unsafe {
        (*buf).ensure_size(len);
    }
}

unsafe fn R_FreeStringBufferL(_buf: *mut R_StringBuffer) {
    // no-op in our implementation
}

unsafe fn asBool2(s: SEXP, _call: SEXP) -> c_int {
    let v = asLogical(s);
    if v == NA_INTEGER {
        error("missing value where TRUE/FALSE needed");
    }
    v
}

unsafe fn getConnection(_idx: c_int) -> *mut c_void {
    ptr::null_mut()
}

#[inline(always)]
unsafe fn PRIMVAL(op: SEXP) -> c_int {
    (*op).data.primsxp.offset
}

#[inline(always)]
unsafe fn R_SerializeInfo(in_: *mut c_void) -> SEXP {
    crate::main::serialize::R_SerializeInfo(in_ as crate::main::serialize::R_inpstream_t)
}

#[inline(always)]
unsafe fn ScalarInteger(v: c_int) -> SEXP {
    let x = r_allocVector(SEXPTYPE::INTSXP.0, 1);
    *INTEGER(x) = v;
    x
}

#[inline(always)]
unsafe fn ScalarString(s: SEXP) -> SEXP {
    let x = r_allocVector(SEXPTYPE::STRSXP.0, 1);
    r_SET_STRING_ELT(x, 0, s);
    x
}

#[inline(always)]
unsafe fn StrToInternal(s: *const c_char) -> c_int {
    crate::main::names::StrToInternal(s)
}

#[inline(always)]
unsafe fn mkPRIMSXP(offset: c_int, builtin: c_int) -> SEXP {
    crate::main::dstruct::mkPRIMSXP(offset, builtin)
}

#[inline(always)]
unsafe fn R_MakeWeakRef(key: SEXP, val: SEXP, fin: SEXP, terminal: c_int) -> SEXP {
    crate::main::memory_main::R_MakeWeakRef(key, val, fin, terminal)
}

#[inline(always)]
unsafe fn R_SetExternalPtrAddr(s: SEXP, p: *mut c_void) {
    (*s).data.extptr[0] = p;
}

#[inline(always)]
unsafe fn R_SetExternalPtrProtected(s: SEXP, p: SEXP) {
    (*s).data.extptr[1] = p as *mut c_void;
}

#[inline(always)]
unsafe fn R_SetExternalPtrTag(s: SEXP, p: SEXP) {
    (*s).data.extptr[2] = p as *mut c_void;
}

#[inline(always)]
unsafe fn PRIMNAME(s: SEXP) -> *const c_char {
    crate::main::names::getPRIMNAME(s)
}

#[inline(always)]
unsafe fn R_RestoreHashCount(_s: SEXP) {
    // stub - hash tables not yet implemented
}

// ---------------------------------------------------------------------------
// Old-style DataLoad (pre-version 1)
// ---------------------------------------------------------------------------

unsafe fn DataLoad(fp: *mut c_void, startup: c_int, version: c_int, _d: *mut SaveLoadData) -> SEXP {
    // This is a simplified stub for the old-style DataLoad.
    // The full implementation requires proper file I/O and SEXP manipulation.
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Old-style load functions
// ---------------------------------------------------------------------------

unsafe fn AsciiLoad(fp: *mut c_void, startup: c_int, d: *mut SaveLoadData) -> SEXP {
    unsafe { DataLoad(fp, startup, 0, d) }
}

unsafe fn AsciiLoadOld(
    fp: *mut c_void,
    version: c_int,
    startup: c_int,
    d: *mut SaveLoadData,
) -> SEXP {
    unsafe { DataLoad(fp, startup, version, d) }
}

unsafe fn XdrLoad(fp: *mut c_void, startup: c_int, d: *mut SaveLoadData) -> SEXP {
    unsafe { DataLoad(fp, startup, 0, d) }
}

unsafe fn BinaryLoad(fp: *mut c_void, startup: c_int, d: *mut SaveLoadData) -> SEXP {
    unsafe { DataLoad(fp, startup, 0, d) }
}

unsafe fn BinaryLoadOld(
    fp: *mut c_void,
    version: c_int,
    startup: c_int,
    d: *mut SaveLoadData,
) -> SEXP {
    unsafe { DataLoad(fp, startup, version, d) }
}

// ---------------------------------------------------------------------------
// NewSaveSpecialHook / NewLoadSpecialHook
// ---------------------------------------------------------------------------

unsafe fn NewSaveSpecialHook(item: SEXP) -> c_int {
    if item == R_NilValue() {
        return -1;
    }
    if item == R_GlobalEnv() {
        return -2;
    }
    if item == R_UnboundValue() {
        return -3;
    }
    if item == R_MissingArg() {
        return -4;
    }
    0
}

unsafe fn NewLoadSpecialHook(type_: c_int) -> SEXP {
    match type_ {
        -1 => R_NilValue(),
        -2 => R_GlobalEnv(),
        -3 => R_UnboundValue(),
        -4 => R_MissingArg(),
        _ => ptr::null_mut(),
    }
}

// ---------------------------------------------------------------------------
// Hash table helpers (for version 1 save/load)
// ---------------------------------------------------------------------------

unsafe fn PTRHASH(obj: SEXP) -> usize {
    (obj as usize) >> 2
}

// ---------------------------------------------------------------------------
// Version 1 NewDataSave / NewDataLoad (stubs)
// ---------------------------------------------------------------------------

unsafe fn NewDataSave(_s: SEXP, _fp: *mut c_void, _m: *mut OutputRoutines, _d: *mut SaveLoadData) {
    // Full implementation requires SEXP manipulation infrastructure
}

unsafe fn NewDataLoad(_fp: *mut c_void, _m: *mut InputRoutines, _d: *mut SaveLoadData) -> SEXP {
    // Full implementation requires SEXP manipulation infrastructure
    R_NilValue()
}

// Output/Input routine structs
struct OutputRoutines {
    OutInit: unsafe fn(*mut c_void, *mut SaveLoadData),
    OutInteger: unsafe fn(*mut c_void, c_int, *mut SaveLoadData),
    OutReal: unsafe fn(*mut c_void, c_double, *mut SaveLoadData),
    OutComplex: unsafe fn(*mut c_void, Rcomplex, *mut SaveLoadData),
    OutString: unsafe fn(*mut c_void, *const c_char, *mut SaveLoadData),
    OutSpace: unsafe fn(*mut c_void, c_int, *mut SaveLoadData),
    OutNewline: unsafe fn(*mut c_void, *mut SaveLoadData),
    OutTerm: unsafe fn(*mut c_void, *mut SaveLoadData),
}

struct InputRoutines {
    InInit: unsafe fn(*mut c_void, *mut SaveLoadData),
    InInteger: unsafe fn(*mut c_void, *mut SaveLoadData) -> c_int,
    InReal: unsafe fn(*mut c_void, *mut SaveLoadData) -> c_double,
    InComplex: unsafe fn(*mut c_void, *mut SaveLoadData) -> Rcomplex,
    InString: unsafe fn(*mut c_void, *mut SaveLoadData) -> *mut c_char,
    InTerm: unsafe fn(*mut c_void, *mut SaveLoadData),
}

// ---------------------------------------------------------------------------
// NewAsciiSave / NewAsciiLoad / NewBinaryLoad / NewXdrSave / NewXdrLoad
// ---------------------------------------------------------------------------

unsafe fn NewAsciiSave(s: SEXP, fp: *mut c_void, d: *mut SaveLoadData) {
    let m = OutputRoutines {
        OutInit: DummyInit,
        OutInteger: OutIntegerAscii,
        OutReal: OutDoubleAscii,
        OutComplex: OutComplexAscii,
        OutString: OutStringAscii,
        OutSpace: OutSpaceAscii,
        OutNewline: OutNewlineAscii,
        OutTerm: DummyTerm,
    };
    unsafe {
        NewDataSave(s, fp, &m as *const OutputRoutines as *mut OutputRoutines, d);
    }
}

unsafe fn NewAsciiLoad(fp: *mut c_void, d: *mut SaveLoadData) -> SEXP {
    let m = InputRoutines {
        InInit: DummyInit,
        InInteger: InIntegerAscii,
        InReal: InDoubleAscii,
        InComplex: InComplexAscii,
        InString: InStringAscii,
        InTerm: DummyTerm,
    };
    unsafe { NewDataLoad(fp, &m as *const InputRoutines as *mut InputRoutines, d) }
}

unsafe fn NewBinaryLoad(fp: *mut c_void, d: *mut SaveLoadData) -> SEXP {
    let m = InputRoutines {
        InInit: DummyInit,
        InInteger: InIntegerBinary,
        InReal: InRealBinary,
        InComplex: InComplexBinary,
        InString: InStringBinary,
        InTerm: DummyTerm,
    };
    unsafe { NewDataLoad(fp, &m as *const InputRoutines as *mut InputRoutines, d) }
}

unsafe fn NewXdrSave(s: SEXP, fp: *mut c_void, d: *mut SaveLoadData) {
    let m = OutputRoutines {
        OutInit: OutInitXdr,
        OutInteger: OutIntegerXdr,
        OutReal: OutRealXdr,
        OutComplex: OutComplexXdr,
        OutString: OutStringXdr,
        OutSpace: DummyOutSpace,
        OutNewline: DummyOutNewline,
        OutTerm: OutTermXdr,
    };
    unsafe {
        NewDataSave(s, fp, &m as *const OutputRoutines as *mut OutputRoutines, d);
    }
}

unsafe fn NewXdrLoad(fp: *mut c_void, d: *mut SaveLoadData) -> SEXP {
    let m = InputRoutines {
        InInit: XdrInInit,
        InInteger: InIntegerXdr,
        InReal: InRealXdr,
        InComplex: InComplexXdr,
        InString: InStringXdr,
        InTerm: InTermXdr,
    };
    unsafe { NewDataLoad(fp, &m as *const InputRoutines as *mut InputRoutines, d) }
}

// ---------------------------------------------------------------------------
// R_SaveToFileV -- the main external save entry point
// ---------------------------------------------------------------------------

/// Port of: attribute_hidden void R_SaveToFileV(SEXP obj, FILE *fp, int ascii, int version)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SaveToFileV(obj: SEXP, fp: *mut c_void, ascii: c_int, version: c_int) {
    let mut data = SaveLoadData::new();

    if version == 1 {
        if ascii != 0 {
            unsafe {
                R_WriteMagic(fp, R_MAGIC_ASCII_V1);
            }
            unsafe {
                NewAsciiSave(obj, fp, &mut data);
            }
        } else {
            unsafe {
                R_WriteMagic(fp, R_MAGIC_XDR_V1);
            }
            unsafe {
                NewXdrSave(obj, fp, &mut data);
            }
        }
    } else {
        // version 2 or 3 uses the pstream serialization mechanism
        unsafe extern "C" {
            fn R_InitFileOutPStream(
                out: *mut c_void,
                fp: *mut c_void,
                type_: c_int,
                version: c_int,
                hook: *mut c_void,
                data: *mut c_void,
            );
            fn R_Serialize(s: SEXP, out: *mut c_void);
        }

        let v = if version == 0 {
            unsafe { defaultSaveVersion() }
        } else {
            version
        };
        let magic;
        let ptype;
        if ascii != 0 {
            magic = if v == 2 {
                R_MAGIC_ASCII_V2
            } else {
                R_MAGIC_ASCII_V3
            };
            ptype = 1; // R_pstream_ascii_format
        } else {
            magic = if v == 2 {
                R_MAGIC_XDR_V2
            } else {
                R_MAGIC_XDR_V3
            };
            ptype = 2; // R_pstream_xdr_format
        }

        unsafe {
            R_WriteMagic(fp, magic);
        }

        let mut out: [u8; 256] = [0; 256];
        unsafe {
            R_InitFileOutPStream(
                out.as_mut_ptr() as *mut c_void,
                fp,
                ptype,
                version,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            R_Serialize(obj, out.as_mut_ptr() as *mut c_void);
        }
    }
}

/// Port of: attribute_hidden void R_SaveToFile(SEXP obj, FILE *fp, int ascii)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SaveToFile(obj: SEXP, fp: *mut c_void, ascii: c_int) {
    unsafe {
        R_SaveToFileV(obj, fp, ascii, defaultSaveVersion());
    }
}

// ---------------------------------------------------------------------------
// R_LoadFromFile -- the main external load entry point
// ---------------------------------------------------------------------------

/// Port of: attribute_hidden SEXP R_LoadFromFile(FILE *fp, int startup)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_LoadFromFile(fp: *mut c_void, startup: c_int) -> SEXP {
    unsafe extern "C" {
        fn R_InitFileInPStream(
            in_: *mut c_void,
            fp: *mut c_void,
            type_: c_int,
            hook: *mut c_void,
            data: *mut c_void,
        );
        fn R_Unserialize(in_: *mut c_void) -> SEXP;
    }

    let mut data = SaveLoadData::new();
    let magic = unsafe { R_ReadMagic(fp) };

    match magic {
        R_MAGIC_XDR => unsafe { XdrLoad(fp, startup, &mut data) },
        R_MAGIC_BINARY => unsafe { BinaryLoad(fp, startup, &mut data) },
        R_MAGIC_ASCII => unsafe { AsciiLoad(fp, startup, &mut data) },
        R_MAGIC_BINARY_VERSION16 => unsafe { BinaryLoadOld(fp, 16, startup, &mut data) },
        R_MAGIC_ASCII_VERSION16 => unsafe { AsciiLoadOld(fp, 16, startup, &mut data) },
        R_MAGIC_ASCII_V1 => unsafe { NewAsciiLoad(fp, &mut data) },
        R_MAGIC_BINARY_V1 => unsafe { NewBinaryLoad(fp, &mut data) },
        R_MAGIC_XDR_V1 => unsafe { NewXdrLoad(fp, &mut data) },
        R_MAGIC_ASCII_V2 | R_MAGIC_ASCII_V3 => {
            let mut in_: [u8; 256] = [0; 256];
            unsafe {
                R_InitFileInPStream(
                    in_.as_mut_ptr() as *mut c_void,
                    fp,
                    1, // R_pstream_ascii_format
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                R_Unserialize(in_.as_mut_ptr() as *mut c_void)
            }
        }
        R_MAGIC_BINARY_V2 | R_MAGIC_BINARY_V3 => {
            let mut in_: [u8; 256] = [0; 256];
            unsafe {
                R_InitFileInPStream(
                    in_.as_mut_ptr() as *mut c_void,
                    fp,
                    3, // R_pstream_binary_format
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                R_Unserialize(in_.as_mut_ptr() as *mut c_void)
            }
        }
        R_MAGIC_XDR_V2 | R_MAGIC_XDR_V3 => {
            let mut in_: [u8; 256] = [0; 256];
            unsafe {
                R_InitFileInPStream(
                    in_.as_mut_ptr() as *mut c_void,
                    fp,
                    2, // R_pstream_xdr_format
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                R_Unserialize(in_.as_mut_ptr() as *mut c_void)
            }
        }
        _ => {
            match magic {
                R_MAGIC_EMPTY => unsafe {
                    error("restore file may be empty -- no data loaded");
                },
                R_MAGIC_MAYBE_TOONEW => unsafe {
                    error("restore file may be from a newer version of R -- no data loaded");
                },
                _ => unsafe {
                    error(
                        "bad restore file magic number (file may be corrupted) -- no data loaded",
                    );
                },
            }
            unreachable!()
        }
    }
}

// ---------------------------------------------------------------------------
// do_savefile / do_loadfile
// ---------------------------------------------------------------------------

/// Port of: attribute_hidden SEXP do_savefile(SEXP call, SEXP op, SEXP args, SEXP env)
pub unsafe fn do_savefile(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe extern "C" {
        fn RC_fopen(path: SEXP, mode: *const c_char, warn: c_int) -> *mut c_void;
        fn fclose(fp: *mut c_void) -> c_int;
    }

    unsafe {
        checkArity(op, args);
    }

    let version;
    if unsafe { Rf_isNull(CAR(nthcdr(args, 3))) } != 0 {
        version = unsafe { defaultSaveVersion() };
    } else {
        version = unsafe { asInteger(CAR(nthcdr(args, 3))) };
    }

    let fp = unsafe {
        RC_fopen(
            STRING_ELT(CAR(nthcdr(args, 1)), 0),
            b"wb\0".as_ptr() as *const c_char,
            1,
        )
    };

    if fp.is_null() {
        unsafe {
            error("unable to open 'file'");
        }
    }

    let ascii = unsafe { *INTEGER(CAR(nthcdr(args, 2))).add(0) };
    unsafe {
        R_SaveToFileV(CAR(nthcdr(args, 0)), fp, ascii, version);
    }

    unsafe {
        fclose(fp);
    }
    R_NilValue()
}

/// Port of: attribute_hidden SEXP do_loadfile(SEXP call, SEXP op, SEXP args, SEXP env)
pub unsafe fn do_loadfile(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe extern "C" {
        fn RC_fopen(path: SEXP, mode: *const c_char, warn: c_int) -> *mut c_void;
        fn fclose(fp: *mut c_void) -> c_int;
    }

    unsafe {
        checkArity(op, args);
    }

    let file = unsafe { PROTECT(coerceVector(CAR(nthcdr(args, 0)), SEXPTYPE::STRSXP.0)) };

    if unsafe { isValidStringF(file) } == 0 {
        unsafe {
            error("bad file name");
        }
    }

    let fp = unsafe { RC_fopen(STRING_ELT(file, 0), b"rb\0".as_ptr() as *const c_char, 1) };
    if fp.is_null() {
        unsafe {
            error("unable to open 'file'");
        }
    }

    let s = unsafe { R_LoadFromFile(fp, 0) };
    unsafe {
        fclose(fp);
    }
    unsafe {
        UNPROTECT(1);
    }
    s
}

// ---------------------------------------------------------------------------
// do_save -- the main save() builtin
// ---------------------------------------------------------------------------

/// Port of: attribute_hidden SEXP do_save(SEXP call, SEXP op, SEXP args, SEXP env)
pub unsafe fn do_save(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe extern "C" {
        fn RC_fopen(path: SEXP, mode: *const c_char, warn: c_int) -> *mut c_void;
        fn fclose(fp: *mut c_void) -> c_int;
        fn strerror(errnum: c_int) -> *const c_char;
        static errno: c_int;
    }

    unsafe {
        checkArity(op, args);
    }

    // save(list, file, ascii, version, environment)
    if unsafe { TYPEOF(CAR(nthcdr(args, 0))) } != SEXPTYPE::STRSXP.0 {
        unsafe {
            error("first argument must be a character vector");
        }
    }
    if unsafe { isValidStringF(CAR(nthcdr(args, 1))) } == 0 {
        unsafe {
            error("'file' must be non-empty string");
        }
    }
    if unsafe { TYPEOF(CAR(nthcdr(args, 2))) } != SEXPTYPE::LGLSXP.0 {
        unsafe {
            error("'ascii' must be logical");
        }
    }

    let version;
    if unsafe { Rf_isNull(CAR(nthcdr(args, 3))) } != 0 {
        version = unsafe { defaultSaveVersion() };
    } else {
        version = unsafe { asInteger(CAR(nthcdr(args, 3))) };
    }
    if version == c_int::MIN || version <= 0 {
        unsafe {
            error("invalid '%s' argument");
        }
    }

    let source = unsafe { CAR(nthcdr(args, 4)) };
    if unsafe { Rf_isNull(source) } == 0 && unsafe { TYPEOF(source) } != SEXPTYPE::ENVSXP.0 {
        unsafe {
            error("invalid '%s' argument");
        }
    }

    let ep = unsafe { asLogical(CAR(nthcdr(args, 5))) };
    if ep == c_int::MIN {
        unsafe {
            error("invalid '%s' argument");
        }
    }

    let fp = unsafe {
        RC_fopen(
            STRING_ELT(CAR(nthcdr(args, 1)), 0),
            b"wb\0".as_ptr() as *const c_char,
            1,
        )
    };
    if fp.is_null() {
        unsafe {
            error("cannot open file");
        }
    }

    let len = unsafe { length(CAR(nthcdr(args, 0))) };
    let s = unsafe { PROTECT(allocList(len)) };

    let mut t = s;
    let mut j = 0;
    while j < len {
        unsafe {
            let tag = installTrChar(STRING_ELT(CAR(nthcdr(args, 0)), j as usize));
            SET_TAG(t, tag);
            let tmp = R_findVar(TAG(t), source);
            if tmp == R_UnboundValue() {
                error("object not found");
            }
            if ep != 0 && TYPEOF(tmp) == SEXPTYPE::PROMSXP.0 {
                let evaluated = eval(tmp, source);
                SETCAR(t, evaluated);
            } else {
                SETCAR(t, tmp);
            }
        }
        t = unsafe { CDR(t) };
        j += 1;
    }

    let ascii = unsafe { *INTEGER(CAR(nthcdr(args, 2))).add(0) };
    unsafe {
        R_SaveToFileV(s, fp, ascii, version);
    }

    unsafe {
        UNPROTECT(1);
    }
    unsafe {
        fclose(fp);
    }
    R_NilValue()
}

// ---------------------------------------------------------------------------
// RestoreToEnv -- helper to restore loaded data into an environment
// ---------------------------------------------------------------------------

unsafe fn RestoreToEnv(ans: SEXP, aenv: SEXP) -> SEXP {
    // Allow ans to be a vector-style list
    if unsafe { TYPEOF(ans) } == SEXPTYPE::VECSXP.0 {
        unsafe {
            PROTECT(ans);
            let names = getAttrib(ans, R_NamesSymbol());
            PROTECT(names);
            if TYPEOF(names) != SEXPTYPE::STRSXP.0 || LENGTH(names) != LENGTH(ans) {
                error("not a valid named list");
            }
            let len = LENGTH(ans);
            let mut i = 0;
            while i < len {
                let sym = installTrChar(STRING_ELT(names, i as usize));
                let obj = VECTOR_ELT(ans, i as usize);
                defineVar(sym, obj, aenv);
                i += 1;
            }
            UNPROTECT(2);
            names
        }
    } else if unsafe { isList(ans) } != 0 {
        unsafe {
            PROTECT(ans);
            let mut cnt = 0;
            let mut a = ans;
            while a != R_NilValue() {
                a = CDR(a);
                cnt += 1;
            }
            let names = PROTECT(allocVector(SEXPTYPE::STRSXP.0, cnt));
            cnt = 0;
            a = ans;
            while a != R_NilValue() {
                SET_STRING_ELT(names, cnt as usize, PRINTNAME(TAG(a)));
                defineVar(TAG(a), CAR(a), aenv);
                a = CDR(a);
                cnt += 1;
            }
            UNPROTECT(2);
            names
        }
    } else {
        unsafe {
            error("loaded data is not in pair list form");
        }
        unreachable!()
    }
}

unsafe fn R_LoadSavedData(fp: *mut c_void, aenv: SEXP) -> SEXP {
    unsafe { RestoreToEnv(R_LoadFromFile(fp, 0), aenv) }
}

// ---------------------------------------------------------------------------
// do_load -- the main load() builtin
// ---------------------------------------------------------------------------

/// Port of: attribute_hidden SEXP do_load(SEXP call, SEXP op, SEXP args, SEXP env)
pub unsafe fn do_load(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe extern "C" {
        fn RC_fopen(path: SEXP, mode: *const c_char, warn: c_int) -> *mut c_void;
        fn fclose(fp: *mut c_void) -> c_int;
    }

    unsafe {
        checkArity(op, args);
    }

    let fname = unsafe { CAR(nthcdr(args, 0)) };
    if unsafe { isValidString(fname) } == 0 {
        unsafe {
            error("first argument must be a file name");
        }
    }

    let aenv = unsafe { CAR(nthcdr(args, 1)) };
    if unsafe { TYPEOF(aenv) } == SEXPTYPE::NILSXP.0 {
        unsafe {
            error("use of NULL environment is defunct");
        }
    } else if unsafe { TYPEOF(aenv) } != SEXPTYPE::ENVSXP.0 {
        unsafe {
            error("invalid '%s' argument");
        }
    }

    let fp = unsafe { RC_fopen(STRING_ELT(fname, 0), b"rb\0".as_ptr() as *const c_char, 1) };
    if fp.is_null() {
        unsafe {
            error("unable to open file");
        }
    }

    let val = unsafe { PROTECT(R_LoadSavedData(fp, aenv)) };
    unsafe {
        fclose(fp);
    }
    unsafe {
        UNPROTECT(1);
    }
    val
}

// ---------------------------------------------------------------------------
// XDR encode/decode helpers
// ---------------------------------------------------------------------------

/// Port of: attribute_hidden void R_XDREncodeDouble(double d, void *buf)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_XDREncodeDouble(d: c_double, buf: *mut c_void) {
    // Manual XDR encoding of a double (big-endian IEEE 754)
    let bytes = d.to_be_bytes();
    if !buf.is_null() {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, R_XDR_DOUBLE_SIZE);
    }
}

/// Port of: attribute_hidden double R_XDRDecodeDouble(void *buf)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_XDRDecodeDouble(buf: *mut c_void) -> c_double {
    let mut bytes = [0u8; 8];
    if !buf.is_null() {
        ptr::copy_nonoverlapping(buf as *const u8, bytes.as_mut_ptr(), R_XDR_DOUBLE_SIZE);
    }
    c_double::from_be_bytes(bytes)
}

/// Port of: attribute_hidden void R_XDREncodeInteger(int i, void *buf)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_XDREncodeInteger(i: c_int, buf: *mut c_void) {
    let bytes = i.to_be_bytes();
    if !buf.is_null() {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, R_XDR_INTEGER_SIZE);
    }
}

/// Port of: attribute_hidden int R_XDRDecodeInteger(void *buf)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_XDRDecodeInteger(buf: *mut c_void) -> c_int {
    let mut bytes = [0u8; 4];
    if !buf.is_null() {
        ptr::copy_nonoverlapping(buf as *const u8, bytes.as_mut_ptr(), R_XDR_INTEGER_SIZE);
    }
    c_int::from_be_bytes(bytes)
}

// ---------------------------------------------------------------------------
// R_SaveGlobalEnvToFile / R_RestoreGlobalEnvFromFile
// ---------------------------------------------------------------------------

/// Port of: void R_SaveGlobalEnvToFile(const char *name)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SaveGlobalEnvToFile(name: *const c_char) {
    unsafe extern "C" {
        fn R_fopen(name: *const c_char, mode: *const c_char) -> *mut c_void;
        fn fclose(fp: *mut c_void) -> c_int;
    }

    let sym = unsafe { install(b"sys.save.image\0".as_ptr() as *const c_char) };

    let genv = R_GlobalEnv();

    // Check if sys.save.image is available
    let found = unsafe { R_findVar(sym, genv) };
    if found == R_UnboundValue() {
        let fp = unsafe { R_fopen(name, b"wb\0".as_ptr() as *const c_char) };
        if fp.is_null() {
            unsafe {
                error("cannot save data -- unable to open file");
            }
        }
        let frame = unsafe { FRAME(genv) };
        unsafe {
            R_SaveToFile(frame, fp, 0);
        }
        unsafe {
            fclose(fp);
        }
    } else {
        // Call sys.save.image(name) in R
        let args = unsafe { LCONS(ScalarString(mkChar(name)), R_NilValue()) };
        let call = unsafe { PROTECT(LCONS(sym, args)) };
        unsafe {
            eval(call, genv);
        }
        unsafe {
            UNPROTECT(1);
        }
    }
}

/// Port of: void R_RestoreGlobalEnvFromFile(const char *name, Rboolean quiet)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RestoreGlobalEnvFromFile(name: *const c_char, quiet: c_int) {
    unsafe extern "C" {
        fn R_fopen(name: *const c_char, mode: *const c_char) -> *mut c_void;
        fn fclose(fp: *mut c_void) -> c_int;
        fn Rprintf(format: *const c_char, ...) -> c_int;
    }

    let sym = unsafe { install(b"sys.load.image\0".as_ptr() as *const c_char) };

    let found = unsafe { R_findVar(sym, R_GlobalEnv()) };
    if found == R_UnboundValue() {
        let fp = unsafe { R_fopen(name, b"rb\0".as_ptr() as *const c_char) };
        if !fp.is_null() {
            unsafe {
                R_LoadSavedData(fp, R_GlobalEnv());
            }
            if quiet == 0 {
                unsafe {
                    Rprintf(
                        b"[Previously saved workspace restored]\n\n\0".as_ptr() as *const c_char
                    );
                }
            }
            unsafe {
                fclose(fp);
            }
        }
    } else {
        let genv = R_GlobalEnv();
        let sQuiet = if quiet != 0 {
            unsafe { mkTrue() }
        } else {
            unsafe { mkFalse() }
        };
        let args = unsafe { PROTECT(LCONS(sQuiet, R_NilValue())) };
        let args2 = unsafe { LCONS(ScalarString(mkChar(name)), args) };
        let call = unsafe { PROTECT(LCONS(sym, args2)) };
        unsafe {
            eval(call, genv);
        }
        unsafe {
            UNPROTECT(2);
        }
    }
}

// ---------------------------------------------------------------------------
// do_saveToConn / do_loadFromConn2 -- connection-based save/load
// ---------------------------------------------------------------------------

/// Port of: attribute_hidden SEXP do_saveToConn(SEXP call, SEXP op, SEXP args, SEXP env)
pub unsafe fn do_saveToConn(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    // Connection-based save -- stub implementation
    // Full implementation requires Rconnection infrastructure
    R_NilValue()
}

/// Port of: attribute_hidden SEXP do_loadFromConn2(SEXP call, SEXP op, SEXP args, SEXP env)
pub unsafe fn do_loadFromConn2(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    // Connection-based load -- stub implementation
    // Full implementation requires Rconnection infrastructure
    R_NilValue()
}

// ---------------------------------------------------------------------------
// NA_LOGICAL constant
// ---------------------------------------------------------------------------

const NA_LOGICAL: c_int = c_int::MIN;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_R_WriteMagic_v3() {
        let mut buf = Vec::new();
        unsafe {
            R_WriteMagic(buf.as_mut_ptr() as *mut c_void, R_MAGIC_ASCII_V3);
        }
        // Cannot easily test FILE*-based write, just verify no panic
    }

    #[test]
    fn test_R_ReadMagic_empty() {
        let result = unsafe { R_ReadMagic(ptr::null_mut()) };
        assert_eq!(result, R_MAGIC_CORRUPT);
    }

    #[test]
    fn test_defaultSaveVersion() {
        let v = unsafe { defaultSaveVersion() };
        assert!(v == 2 || v == 3);
    }

    #[test]
    fn test_FixupType_version16() {
        // STRSXP -> CPLXSXP under version 16
        let result = unsafe { FixupType(SEXPTYPE::STRSXP.0 as c_uint, 16) };
        assert_eq!(result, SEXPTYPE::CPLXSXP.0 as c_uint);
        // CPLXSXP -> STRSXP under version 16
        let result = unsafe { FixupType(SEXPTYPE::CPLXSXP.0 as c_uint, 16) };
        assert_eq!(result, SEXPTYPE::STRSXP.0 as c_uint);
    }

    #[test]
    fn test_FixupType_no_version() {
        // No version change
        let result = unsafe { FixupType(SEXPTYPE::INTSXP.0 as c_uint, 0) };
        assert_eq!(result, SEXPTYPE::INTSXP.0 as c_uint);
    }

    #[test]
    fn test_FixupType_old_factors() {
        // Types 11, 12 -> 13 (INTSXP)
        let result = unsafe { FixupType(11, 0) };
        assert_eq!(result, 13);
        let result = unsafe { FixupType(12, 0) };
        assert_eq!(result, 13);
    }

    #[test]
    fn test_R_XDREncodeDecodeDouble() {
        let original: c_double = 3.14159265358979;
        let mut buf = [0u8; 8];
        unsafe {
            R_XDREncodeDouble(original, buf.as_mut_ptr() as *mut c_void);
        }
        let decoded: c_double = unsafe { R_XDRDecodeDouble(buf.as_mut_ptr() as *mut c_void) };
        assert!((original - decoded).abs() < 1e-10);
    }

    #[test]
    fn test_R_XDREncodeDecodeDouble_nan() {
        let original: c_double = f64::NAN;
        let mut buf = [0u8; 8];
        unsafe {
            R_XDREncodeDouble(original, buf.as_mut_ptr() as *mut c_void);
        }
        let decoded: c_double = unsafe { R_XDRDecodeDouble(buf.as_mut_ptr() as *mut c_void) };
        assert!(decoded.is_nan());
    }

    #[test]
    fn test_R_XDREncodeDecodeInteger() {
        let original: c_int = 42;
        let mut buf = [0u8; 4];
        unsafe {
            R_XDREncodeInteger(original, buf.as_mut_ptr() as *mut c_void);
        }
        let decoded = unsafe { R_XDRDecodeInteger(buf.as_mut_ptr() as *mut c_void) };
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_R_XDREncodeDecodeInteger_negative() {
        let original: c_int = -12345;
        let mut buf = [0u8; 4];
        unsafe {
            R_XDREncodeInteger(original, buf.as_mut_ptr() as *mut c_void);
        }
        let decoded = unsafe { R_XDRDecodeInteger(buf.as_mut_ptr() as *mut c_void) };
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_R_XDREncodeDecodeInteger_na() {
        let original: c_int = c_int::MIN;
        let mut buf = [0u8; 4];
        unsafe {
            R_XDREncodeInteger(original, buf.as_mut_ptr() as *mut c_void);
        }
        let decoded = unsafe { R_XDRDecodeInteger(buf.as_mut_ptr() as *mut c_void) };
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_Rcomplex_default() {
        let c = Rcomplex::default();
        assert_eq!(c.r, 0.0);
        assert_eq!(c.i, 0.0);
    }

    #[test]
    fn test_R_SaveLoadData_new() {
        let _d = SaveLoadData::new();
        // Just verify construction works
    }

    #[test]
    fn test_R_StringBuffer_new() {
        let mut buf = R_StringBuffer::new();
        assert_eq!(buf.bufsize, MAXELTSIZE);
        buf.ensure_size(100);
        assert_eq!(buf.bufsize, MAXELTSIZE);
        buf.ensure_size(MAXELTSIZE + 100);
        assert!(buf.bufsize > MAXELTSIZE);
    }

    #[test]
    fn test_magic_constants() {
        assert_eq!(R_MAGIC_ASCII_V3, 3001);
        assert_eq!(R_MAGIC_BINARY_V3, 3002);
        assert_eq!(R_MAGIC_XDR_V3, 3003);
        assert_eq!(R_MAGIC_ASCII_V2, 2001);
        assert_eq!(R_MAGIC_BINARY_V2, 2002);
        assert_eq!(R_MAGIC_XDR_V2, 2003);
        assert_eq!(R_MAGIC_ASCII_V1, 1001);
        assert_eq!(R_MAGIC_BINARY_V1, 1002);
        assert_eq!(R_MAGIC_XDR_V1, 1003);
        assert_eq!(R_MAGIC_EMPTY, 999);
        assert_eq!(R_MAGIC_CORRUPT, 998);
        assert_eq!(R_MAGIC_MAYBE_TOONEW, 997);
        assert_eq!(R_MAGIC_BINARY, 1975);
        assert_eq!(R_MAGIC_ASCII, 1976);
        assert_eq!(R_MAGIC_XDR, 1977);
        assert_eq!(R_MAGIC_BINARY_VERSION16, 1971);
        assert_eq!(R_MAGIC_ASCII_VERSION16, 1972);
    }
}
