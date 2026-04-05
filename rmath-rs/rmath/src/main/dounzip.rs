#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/main/dounzip.c (2275 lines)
 *
 *  first part Copyright (C) 2002-2025  The R Core Team
 *  second part Copyright (C) 1998-2010 Gilles Vollant
 *
 *  This is a mini unzip library for reading .zip files (used to read .rds from zip).
 *  It is self-contained C code that doesn't depend on R internals.
 *  The second part (from minizip contribution to zlib) implements the actual
 *  zip decompression using zlib inflate.
 */

use libc::{self};
use std::ffi::{c_char, c_int, c_void};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::ptr;

// ---------------------------------------------------------------------------
// Zip/unzip constants (from unzip.h)
// ---------------------------------------------------------------------------

type ZPOS64_T = u64;
type uLong = u32;
type uInt = u32;
type voidpf = *mut c_void;
type Bytef = u8;

const ZLIB_FILEFUNC_SEEK_CUR: c_int = 1;
const ZLIB_FILEFUNC_SEEK_END: c_int = 2;
const ZLIB_FILEFUNC_SEEK_SET: c_int = 0;

const ZLIB_FILEFUNC_MODE_READ: c_int = 1;
const ZLIB_FILEFUNC_MODE_WRITE: c_int = 2;
const ZLIB_FILEFUNC_MODE_READWRITEFILTER: c_int = 3;
const ZLIB_FILEFUNC_MODE_EXISTING: c_int = 4;
const ZLIB_FILEFUNC_MODE_CREATE: c_int = 8;

const Z_BZIP2ED: uLong = 12;
const Z_DEFLATED: uLong = 8;

const MAXU32: uLong = 0xffffffff;

const UNZ_OK: c_int = 0;
const UNZ_END_OF_LIST_OF_FILE: c_int = -100;
const UNZ_EOF: c_int = 0;
const UNZ_ERRNO: c_int = libc::EIO as c_int;
const UNZ_PARAMERROR: c_int = -102;
const UNZ_BADZIPFILE: c_int = -103;
const UNZ_INTERNALERROR: c_int = -104;
const UNZ_CRCERROR: c_int = -105;

const UNZ_BUFSIZE: usize = 16384;
const UNZ_MAXFILENAMEINZIP: usize = 256;

const SIZECENTRALDIRITEM: usize = 0x2e;
const SIZEZIPLOCALHEADER: usize = 0x1e;

const BUFREADCOMMENT: usize = 0x400;
const BUF_SIZE: usize = 4096;

const FILESEP: &[u8] = b"/\0";

// ---------------------------------------------------------------------------
// tm_unz and file info structures
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct tm_unz {
    pub tm_sec: uInt,
    pub tm_min: uInt,
    pub tm_hour: uInt,
    pub tm_mday: uInt,
    pub tm_mon: uInt,
    pub tm_year: uInt,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct unz_global_info64 {
    pub number_entry: ZPOS64_T,
    pub size_comment: uLong,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct unz_file_info64 {
    pub version: uLong,
    pub version_needed: uLong,
    pub flag: uLong,
    pub compression_method: uLong,
    pub dosDate: uLong,
    pub crc: uLong,
    pub compressed_size: ZPOS64_T,
    pub uncompressed_size: ZPOS64_T,
    pub size_filename: uLong,
    pub size_file_extra: uLong,
    pub size_file_comment: uLong,
    pub disk_num_start: uLong,
    pub internal_fa: uLong,
    pub external_fa: uLong,
    pub tmu_date: tm_unz,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct unz_file_info64_internal {
    offset_curfile: ZPOS64_T,
}

// ---------------------------------------------------------------------------
// File-in-zip read info structure
// ---------------------------------------------------------------------------

struct file_in_zip64_read_info_s {
    read_buffer: Vec<u8>,
    stream: Option<flate2::Decompress>,
    pos_in_zipfile: ZPOS64_T,
    stream_initialised: uLong,
    offset_local_extrafield: ZPOS64_T,
    size_local_extrafield: uInt,
    pos_local_extrafield: ZPOS64_T,
    total_out_64: ZPOS64_T,
    crc32: uLong,
    crc32_wait: uLong,
    rest_read_compressed: ZPOS64_T,
    rest_read_uncompressed: ZPOS64_T,
    filestream: File,
    compression_method: uLong,
    byte_before_the_zipfile: ZPOS64_T,
    raw: c_int,
}

// ---------------------------------------------------------------------------
// Unzip file structure
// ---------------------------------------------------------------------------

pub struct unz64_s {
    is64bitOpenFunction: c_int,
    filestream: File,
    gi: unz_global_info64,
    byte_before_the_zipfile: ZPOS64_T,
    num_file: ZPOS64_T,
    pos_in_central_dir: ZPOS64_T,
    current_file_ok: ZPOS64_T,
    central_pos: ZPOS64_T,
    size_central_dir: ZPOS64_T,
    offset_central_dir: ZPOS64_T,
    cur_file_info: unz_file_info64,
    cur_file_info_internal: unz_file_info64_internal,
    pfile_in_zip_read: Option<Box<file_in_zip64_read_info_s>>,
    encrypted: c_int,
    isZip64: c_int,
}

/// Opaque handle to an open zip file
pub type unzFile = *mut unz64_s;

// ---------------------------------------------------------------------------
// CRC32 computation
// ---------------------------------------------------------------------------

fn crc32(mut crc: u32, buf: &[u8]) -> u32 {
    for &byte in buf {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

fn fopen_func(filename: &str, mode: c_int) -> Option<File> {
    let mode_str = if (mode & ZLIB_FILEFUNC_MODE_READWRITEFILTER) == ZLIB_FILEFUNC_MODE_READ {
        "rb"
    } else if mode & ZLIB_FILEFUNC_MODE_EXISTING != 0 {
        "r+b"
    } else if mode & ZLIB_FILEFUNC_MODE_CREATE != 0 {
        "wb"
    } else {
        "rb"
    };
    File::open(filename).ok()
}

// ---------------------------------------------------------------------------
// Read byte/short/long from file
// ---------------------------------------------------------------------------

fn unz64local_getByte(f: &mut File) -> Result<u8, c_int> {
    let mut buf = [0u8; 1];
    match f.read_exact(&mut buf) {
        Ok(()) => Ok(buf[0]),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Err(UNZ_EOF),
        Err(_) => Err(UNZ_ERRNO),
    }
}

fn unz64local_getShort(f: &mut File) -> Result<uLong, c_int> {
    let b0 = unz64local_getByte(f)? as uLong;
    let b1 = unz64local_getByte(f)? as uLong;
    Ok(b0 | (b1 << 8))
}

fn unz64local_getLong(f: &mut File) -> Result<uLong, c_int> {
    let b0 = unz64local_getByte(f)? as uLong;
    let b1 = unz64local_getByte(f)? as uLong;
    let b2 = unz64local_getByte(f)? as uLong;
    let b3 = unz64local_getByte(f)? as uLong;
    Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
}

fn unz64local_getLong64(f: &mut File) -> Result<ZPOS64_T, c_int> {
    let b0 = unz64local_getByte(f)? as ZPOS64_T;
    let b1 = unz64local_getByte(f)? as ZPOS64_T;
    let b2 = unz64local_getByte(f)? as ZPOS64_T;
    let b3 = unz64local_getByte(f)? as ZPOS64_T;
    let b4 = unz64local_getByte(f)? as ZPOS64_T;
    let b5 = unz64local_getByte(f)? as ZPOS64_T;
    let b6 = unz64local_getByte(f)? as ZPOS64_T;
    let b7 = unz64local_getByte(f)? as ZPOS64_T;
    Ok(
        b0 | (b1 << 8)
            | (b2 << 16)
            | (b3 << 24)
            | (b4 << 32)
            | (b5 << 40)
            | (b6 << 48)
            | (b7 << 56),
    )
}

// ---------------------------------------------------------------------------
// Case-insensitive filename comparison
// ---------------------------------------------------------------------------

fn strcmpcasenosensitive_internal(fileName1: &[u8], fileName2: &[u8]) -> c_int {
    let mut i = 0;
    loop {
        let c1 = if i < fileName1.len() { fileName1[i] } else { 0 };
        let c2 = if i < fileName2.len() { fileName2[i] } else { 0 };
        let c1 = if c1 >= b'a' && c1 <= b'z' {
            c1 - 0x20
        } else {
            c1
        };
        let c2 = if c2 >= b'a' && c2 <= b'z' {
            c2 - 0x20
        } else {
            c2
        };
        if c1 == 0 {
            return if c2 == 0 { 0 } else { -1 };
        }
        if c2 == 0 {
            return 1;
        }
        if c1 < c2 {
            return -1;
        }
        if c1 > c2 {
            return 1;
        }
        i += 1;
    }
}

const CASESENSITIVITYDEFAULTVALUE: c_int = 1; // Unix default

fn unzStringFileNameCompare(fileName1: &[u8], fileName2: &[u8], iCaseSensitivity: c_int) -> c_int {
    if iCaseSensitivity == 0 {
        return strcmpcasenosensitive_internal(fileName1, fileName2);
    }
    if iCaseSensitivity == 1 {
        // Case sensitive
        for i in 0..fileName1.len().min(fileName2.len()) {
            if fileName1[i] != fileName2[i] {
                return if fileName1[i] < fileName2[i] { -1 } else { 1 };
            }
        }
        if fileName1.len() == fileName2.len() {
            return 0;
        }
        if fileName1.len() < fileName2.len() {
            return -1;
        }
        return 1;
    }
    strcmpcasenosensitive_internal(fileName1, fileName2)
}

// ---------------------------------------------------------------------------
// Search for Central Directory
// ---------------------------------------------------------------------------

fn unz64local_SearchCentralDir(f: &mut File) -> ZPOS64_T {
    let uSizeFile = match f.seek(SeekFrom::End(0)) {
        Ok(n) => n,
        Err(_) => return 0,
    };

    let uMaxBack = if 0xffff < uSizeFile {
        0xffff
    } else {
        uSizeFile
    };
    let mut uBackRead: ZPOS64_T = 4;
    let mut uPosFound: ZPOS64_T = 0;

    while uBackRead < uMaxBack {
        let uReadPos: ZPOS64_T;
        let uReadSize: usize;

        if uBackRead + (BUFREADCOMMENT as ZPOS64_T) > uMaxBack {
            uBackRead = uMaxBack;
        } else {
            uBackRead += BUFREADCOMMENT as ZPOS64_T;
        }
        uReadPos = uSizeFile - uBackRead;

        uReadSize = ((BUFREADCOMMENT + 4) as ZPOS64_T).min(uSizeFile - uReadPos) as usize;

        if f.seek(SeekFrom::Start(uReadPos)).is_err() {
            break;
        }

        let mut buf = vec![0u8; uReadSize];
        if f.read_exact(&mut buf).is_err() {
            break;
        }

        let mut i = buf.len() as isize - 3;
        loop {
            i -= 1;
            if i <= 0 {
                break;
            }
            if buf[i as usize] == 0x50
                && buf[i as usize + 1] == 0x4b
                && buf[i as usize + 2] == 0x05
                && buf[i as usize + 3] == 0x06
            {
                uPosFound = uReadPos + i as ZPOS64_T;
                break;
            }
        }

        if uPosFound != 0 {
            break;
        }
    }

    uPosFound
}

// ---------------------------------------------------------------------------
// Search for Central Directory 64
// ---------------------------------------------------------------------------

fn unz64local_SearchCentralDir64(f: &mut File) -> ZPOS64_T {
    let uSizeFile = match f.seek(SeekFrom::End(0)) {
        Ok(n) => n,
        Err(_) => return 0,
    };

    let uMaxBack = if 0xffff < uSizeFile {
        0xffff
    } else {
        uSizeFile
    };
    let mut uBackRead: ZPOS64_T = 4;
    let mut uPosFound: ZPOS64_T = 0;

    while uBackRead < uMaxBack {
        let uReadPos: ZPOS64_T;
        let uReadSize: usize;

        if uBackRead + (BUFREADCOMMENT as ZPOS64_T) > uMaxBack {
            uBackRead = uMaxBack;
        } else {
            uBackRead += BUFREADCOMMENT as ZPOS64_T;
        }
        uReadPos = uSizeFile - uBackRead;

        uReadSize = ((BUFREADCOMMENT + 4) as ZPOS64_T).min(uSizeFile - uReadPos) as usize;

        if f.seek(SeekFrom::Start(uReadPos)).is_err() {
            break;
        }

        let mut buf = vec![0u8; uReadSize];
        if f.read_exact(&mut buf).is_err() {
            break;
        }

        let mut i = buf.len() as isize - 3;
        loop {
            i -= 1;
            if i <= 0 {
                break;
            }
            if buf[i as usize] == 0x50
                && buf[i as usize + 1] == 0x4b
                && buf[i as usize + 2] == 0x06
                && buf[i as usize + 3] == 0x07
            {
                uPosFound = uReadPos + i as ZPOS64_T;
                break;
            }
        }

        if uPosFound != 0 {
            break;
        }
    }

    if uPosFound == 0 {
        return 0;
    }

    // Zip64 end of central directory locator
    if f.seek(SeekFrom::Start(uPosFound)).is_err() {
        return 0;
    }

    let _ = unz64local_getLong(f); // signature, already checked

    let uL = match unz64local_getLong(f) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    if uL != 0 {
        return 0;
    }

    let relativeOffset = match unz64local_getLong64(f) {
        Ok(v) => v,
        Err(_) => return 0,
    };

    let uL = match unz64local_getLong(f) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    if uL != 1 {
        return 0;
    }

    // Goto end of central directory record
    if f.seek(SeekFrom::Start(relativeOffset)).is_err() {
        return 0;
    }

    let uL = match unz64local_getLong(f) {
        Ok(v) => v,
        Err(_) => return 0,
    };

    if uL != 0x06064b50 {
        return 0;
    }

    relativeOffset
}

// ---------------------------------------------------------------------------
// Dos date to tm_unz conversion
// ---------------------------------------------------------------------------

fn unz64local_DosDateToTmuDate(ulDosDate: ZPOS64_T, ptm: &mut tm_unz) {
    let uDate = (ulDosDate >> 16) as uLong;
    ptm.tm_mday = (uDate & 0x1f) as uInt;
    ptm.tm_mon = (((uDate & 0x1E0) / 0x20) - 1) as uInt;
    ptm.tm_year = (((uDate & 0x0FE00) / 0x0200) + 1980) as uInt;
    ptm.tm_hour = ((ulDosDate as uLong & 0xF800) / 0x800) as uInt;
    ptm.tm_min = ((ulDosDate as uLong & 0x7E0) / 0x20) as uInt;
    ptm.tm_sec = (2 * (ulDosDate as uLong & 0x1f)) as uInt;
}

// ---------------------------------------------------------------------------
// Open a Zip file
// ---------------------------------------------------------------------------

fn unzOpenInternal(path: &str, is64bitOpenFunction: c_int) -> Option<Box<unz64_s>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return None,
    };

    let mut us = unz64_s {
        is64bitOpenFunction,
        filestream: file,
        gi: unz_global_info64::default(),
        byte_before_the_zipfile: 0,
        num_file: 0,
        pos_in_central_dir: 0,
        current_file_ok: 0,
        central_pos: 0,
        size_central_dir: 0,
        offset_central_dir: 0,
        cur_file_info: unz_file_info64::default(),
        cur_file_info_internal: unz_file_info64_internal::default(),
        pfile_in_zip_read: None,
        encrypted: 0,
        isZip64: 0,
    };

    let central_pos = unz64local_SearchCentralDir64(&mut us.filestream);
    if central_pos != 0 {
        us.isZip64 = 1;

        if us.filestream.seek(SeekFrom::Start(central_pos)).is_err() {
            return None;
        }

        let mut err = false;

        if unz64local_getLong(&mut us.filestream).is_err() {
            err = true;
        }
        if !err && unz64local_getLong64(&mut us.filestream).is_err() {
            err = true;
        }
        if !err && unz64local_getShort(&mut us.filestream).is_err() {
            err = true;
        }
        if !err && unz64local_getShort(&mut us.filestream).is_err() {
            err = true;
        }

        let number_disk = if !err {
            unz64local_getLong(&mut us.filestream).unwrap_or(0)
        } else {
            0
        };
        let number_disk_with_CD = if !err {
            unz64local_getLong(&mut us.filestream).unwrap_or(0)
        } else {
            0
        };
        let number_entry = if !err {
            unz64local_getLong64(&mut us.filestream).unwrap_or(0)
        } else {
            0
        };
        let number_entry_CD = if !err {
            unz64local_getLong64(&mut us.filestream).unwrap_or(0)
        } else {
            0
        };

        us.gi.number_entry = number_entry;

        if number_entry_CD != number_entry || number_disk_with_CD != 0 || number_disk != 0 {
            err = true;
        }

        if !err {
            us.size_central_dir = unz64local_getLong64(&mut us.filestream).unwrap_or(0);
        }
        if !err {
            us.offset_central_dir = unz64local_getLong64(&mut us.filestream).unwrap_or(0);
        }

        us.gi.size_comment = 0;

        if err {
            return None;
        }
    } else {
        let central_pos = unz64local_SearchCentralDir(&mut us.filestream);
        if central_pos == 0 {
            return None;
        }

        us.isZip64 = 0;

        if us.filestream.seek(SeekFrom::Start(central_pos)).is_err() {
            return None;
        }

        let mut err = false;

        if unz64local_getLong(&mut us.filestream).is_err() {
            err = true;
        }

        let number_disk = if !err {
            unz64local_getShort(&mut us.filestream).unwrap_or(0)
        } else {
            0
        };
        let number_disk_with_CD = if !err {
            unz64local_getShort(&mut us.filestream).unwrap_or(0)
        } else {
            0
        };
        let number_entry = if !err {
            unz64local_getShort(&mut us.filestream).unwrap_or(0)
        } else {
            0
        };
        let number_entry_CD = if !err {
            unz64local_getShort(&mut us.filestream).unwrap_or(0)
        } else {
            0
        };

        us.gi.number_entry = number_entry as ZPOS64_T;

        if number_entry_CD != number_entry || number_disk_with_CD != 0 || number_disk != 0 {
            err = true;
        }

        if !err {
            us.size_central_dir = unz64local_getLong(&mut us.filestream).unwrap_or(0) as ZPOS64_T;
        }
        if !err {
            us.offset_central_dir = unz64local_getLong(&mut us.filestream).unwrap_or(0) as ZPOS64_T;
        }
        if !err {
            us.gi.size_comment = unz64local_getShort(&mut us.filestream).unwrap_or(0);
        }

        if err {
            return None;
        }
    }

    if central_pos < us.offset_central_dir + us.size_central_dir {
        return None;
    }

    us.byte_before_the_zipfile = central_pos - (us.offset_central_dir + us.size_central_dir);
    us.central_pos = central_pos;
    us.pfile_in_zip_read = None;
    us.encrypted = 0;

    let mut s = Box::new(us);
    unzGoToFirstFile(&mut *s);
    Some(s)
}

/// Open a zip file (64-bit API)
pub fn unzOpen64(path: &str) -> Option<Box<unz64_s>> {
    unzOpenInternal(path, 1)
}

// ---------------------------------------------------------------------------
// Close a Zip file
// ---------------------------------------------------------------------------

pub fn unzClose(file: &mut Box<unz64_s>) -> c_int {
    if file.pfile_in_zip_read.is_some() {
        unzCloseCurrentFile(file);
    }
    UNZ_OK
}

// ---------------------------------------------------------------------------
// Get global info
// ---------------------------------------------------------------------------

pub fn unzGetGlobalInfo64(file: &unz64_s) -> Result<unz_global_info64, c_int> {
    Ok(file.gi)
}

// ---------------------------------------------------------------------------
// Get current file info (internal)
// ---------------------------------------------------------------------------

fn unz64local_GetCurrentFileInfoInternal(
    s: &mut unz64_s,
    pfile_info: &mut unz_file_info64,
    pfile_info_internal: &mut unz_file_info64_internal,
    szFileName: &mut [u8],
    extraField: &mut [u8],
    szComment: &mut [u8],
) -> c_int {
    let mut file_info = unz_file_info64::default();
    let mut file_info_internal = unz_file_info64_internal::default();
    let mut err = UNZ_OK;

    if s.filestream
        .seek(SeekFrom::Start(
            s.pos_in_central_dir + s.byte_before_the_zipfile,
        ))
        .is_err()
    {
        err = UNZ_ERRNO;
    }

    // Check magic
    if err == UNZ_OK {
        match unz64local_getLong(&mut s.filestream) {
            Ok(uMagic) => {
                if uMagic != 0x02014b50 {
                    err = UNZ_BADZIPFILE;
                }
            }
            Err(_) => {
                err = UNZ_ERRNO;
            }
        }
    }

    if unz64local_getShort(&mut s.filestream).is_err() {
        err = UNZ_ERRNO;
    } else {
        file_info.version = unz64local_getShort(&mut s.filestream).unwrap_or(0);
    }
    file_info.version_needed = unz64local_getShort(&mut s.filestream).unwrap_or(0);
    file_info.flag = unz64local_getShort(&mut s.filestream).unwrap_or(0);
    file_info.compression_method = unz64local_getShort(&mut s.filestream).unwrap_or(0);
    file_info.dosDate = unz64local_getLong(&mut s.filestream).unwrap_or(0);

    unz64local_DosDateToTmuDate(file_info.dosDate as ZPOS64_T, &mut file_info.tmu_date);

    file_info.crc = unz64local_getLong(&mut s.filestream).unwrap_or(0);
    file_info.compressed_size = unz64local_getLong(&mut s.filestream).unwrap_or(0) as ZPOS64_T;
    file_info.uncompressed_size = unz64local_getLong(&mut s.filestream).unwrap_or(0) as ZPOS64_T;
    file_info.size_filename = unz64local_getShort(&mut s.filestream).unwrap_or(0);
    file_info.size_file_extra = unz64local_getShort(&mut s.filestream).unwrap_or(0);
    file_info.size_file_comment = unz64local_getShort(&mut s.filestream).unwrap_or(0);
    file_info.disk_num_start = unz64local_getShort(&mut s.filestream).unwrap_or(0);
    file_info.internal_fa = unz64local_getShort(&mut s.filestream).unwrap_or(0);
    file_info.external_fa = unz64local_getLong(&mut s.filestream).unwrap_or(0);

    file_info_internal.offset_curfile =
        unz64local_getLong(&mut s.filestream).unwrap_or(0) as ZPOS64_T;

    // Read filename
    if err == UNZ_OK && !szFileName.is_empty() {
        let uSizeRead = (file_info.size_filename as usize).min(szFileName.len() - 1);
        if file_info.size_filename > 0 && !szFileName.is_empty() {
            if s.filestream
                .read_exact(&mut szFileName[..uSizeRead])
                .is_err()
            {
                err = UNZ_ERRNO;
            }
        }
        szFileName[uSizeRead] = 0;
    } else {
        // Skip filename
        if s.filestream
            .seek(SeekFrom::Current(file_info.size_filename as i64))
            .is_err()
        {
            err = UNZ_ERRNO;
        }
    }

    // Read extra field
    if err == UNZ_OK && !extraField.is_empty() {
        let uSizeRead = (file_info.size_file_extra as usize).min(extraField.len());
        if file_info.size_file_extra > 0 && !extraField.is_empty() {
            if s.filestream
                .read_exact(&mut extraField[..uSizeRead])
                .is_err()
            {
                err = UNZ_ERRNO;
            }
        }
        // Skip remaining extra field
        let remaining = file_info.size_file_extra as i64 - uSizeRead as i64;
        if remaining > 0 {
            if s.filestream.seek(SeekFrom::Current(remaining)).is_err() {
                err = UNZ_ERRNO;
            }
        }
    } else {
        if s.filestream
            .seek(SeekFrom::Current(file_info.size_file_extra as i64))
            .is_err()
        {
            err = UNZ_ERRNO;
        }
    }

    // Process extra field for zip64 info
    if err == UNZ_OK && file_info.size_file_extra != 0 {
        // Seek back to start of extra field
        let extra_start =
            s.filestream.seek(SeekFrom::Current(0)).unwrap_or(0) - file_info.size_file_extra as u64;
        if s.filestream.seek(SeekFrom::Start(extra_start)).is_err() {
            err = UNZ_ERRNO;
        }

        let mut acc: uLong = 0;
        while acc < file_info.size_file_extra && err == UNZ_OK {
            let headerId = unz64local_getShort(&mut s.filestream).unwrap_or(0);
            let dataSize = unz64local_getShort(&mut s.filestream).unwrap_or(0);

            if headerId == 0x0001 {
                if file_info.uncompressed_size == MAXU32 as u64 {
                    file_info.uncompressed_size =
                        unz64local_getLong64(&mut s.filestream).unwrap_or(0);
                }
                if file_info.compressed_size == MAXU32 as u64 {
                    file_info.compressed_size =
                        unz64local_getLong64(&mut s.filestream).unwrap_or(0);
                }
                if file_info_internal.offset_curfile == MAXU32 as u64 {
                    file_info_internal.offset_curfile =
                        unz64local_getLong64(&mut s.filestream).unwrap_or(0);
                }
                if file_info.disk_num_start == 0xffff {
                    let _ = unz64local_getLong(&mut s.filestream);
                }
            } else {
                if s.filestream
                    .seek(SeekFrom::Current(dataSize as i64))
                    .is_err()
                {
                    err = UNZ_ERRNO;
                }
            }
            acc += 2 + 2 + dataSize;
        }

        // Seek to after extra field + comment
        if s.filestream
            .seek(SeekFrom::Current(file_info.size_file_comment as i64))
            .is_err()
        {
            err = UNZ_ERRNO;
        }
    } else {
        // Skip comment
        if s.filestream
            .seek(SeekFrom::Current(file_info.size_file_comment as i64))
            .is_err()
        {
            err = UNZ_ERRNO;
        }
    }

    if err == UNZ_OK {
        *pfile_info = file_info;
    }
    if err == UNZ_OK {
        *pfile_info_internal = file_info_internal;
    }

    err
}

/// Get info about the current file in the zipfile
pub fn unzGetCurrentFileInfo64(
    s: &mut unz64_s,
    pfile_info: &mut unz_file_info64,
    szFileName: &mut [u8],
    extraField: &mut [u8],
    szComment: &mut [u8],
) -> c_int {
    let mut dummy_internal = unz_file_info64_internal::default();
    unz64local_GetCurrentFileInfoInternal(
        s,
        pfile_info,
        &mut dummy_internal,
        szFileName,
        extraField,
        szComment,
    )
}

// ---------------------------------------------------------------------------
// Go to first file
// ---------------------------------------------------------------------------

pub fn unzGoToFirstFile(s: &mut unz64_s) -> c_int {
    s.pos_in_central_dir = s.offset_central_dir;
    s.num_file = 0;
    let mut dummy_info = unz_file_info64::default();
    let mut dummy_internal = unz_file_info64_internal::default();
    let mut dummy_name = [0u8; 1];
    let mut dummy_extra = [0u8; 1];
    let mut dummy_comment = [0u8; 1];
    let err = unz64local_GetCurrentFileInfoInternal(
        s,
        &mut dummy_info,
        &mut dummy_internal,
        &mut dummy_name,
        &mut dummy_extra,
        &mut dummy_comment,
    );
    s.current_file_ok = if err == UNZ_OK { 1 } else { 0 };
    s.cur_file_info = dummy_info;
    s.cur_file_info_internal = dummy_internal;
    err
}

// ---------------------------------------------------------------------------
// Go to next file
// ---------------------------------------------------------------------------

pub fn unzGoToNextFile(s: &mut unz64_s) -> c_int {
    if s.current_file_ok == 0 {
        return UNZ_END_OF_LIST_OF_FILE;
    }
    if s.gi.number_entry != 0xffff {
        if s.num_file + 1 == s.gi.number_entry {
            return UNZ_END_OF_LIST_OF_FILE;
        }
    }

    s.pos_in_central_dir += (SIZECENTRALDIRITEM as ZPOS64_T)
        + s.cur_file_info.size_filename as ZPOS64_T
        + s.cur_file_info.size_file_extra as ZPOS64_T
        + s.cur_file_info.size_file_comment as ZPOS64_T;
    s.num_file += 1;

    let mut dummy_info = unz_file_info64::default();
    let mut dummy_internal = unz_file_info64_internal::default();
    let mut dummy_name = [0u8; 1];
    let mut dummy_extra = [0u8; 1];
    let mut dummy_comment = [0u8; 1];
    let err = unz64local_GetCurrentFileInfoInternal(
        s,
        &mut dummy_info,
        &mut dummy_internal,
        &mut dummy_name,
        &mut dummy_extra,
        &mut dummy_comment,
    );
    s.current_file_ok = if err == UNZ_OK { 1 } else { 0 };
    s.cur_file_info = dummy_info;
    s.cur_file_info_internal = dummy_internal;
    err
}

// ---------------------------------------------------------------------------
// Locate file in zip
// ---------------------------------------------------------------------------

pub fn unzLocateFile(s: &mut unz64_s, szFileName: &[u8], iCaseSensitivity: c_int) -> c_int {
    if szFileName.len() >= UNZ_MAXFILENAMEINZIP {
        return UNZ_PARAMERROR;
    }

    if s.current_file_ok == 0 {
        return UNZ_END_OF_LIST_OF_FILE;
    }

    // Save state
    let num_fileSaved = s.num_file;
    let pos_in_central_dirSaved = s.pos_in_central_dir;
    let cur_file_infoSaved = s.cur_file_info;
    let cur_file_info_internalSaved = s.cur_file_info_internal;

    let mut err = unzGoToFirstFile(s);

    while err == UNZ_OK {
        let mut szCurrentFileName = [0u8; UNZ_MAXFILENAMEINZIP + 1];
        let mut dummy_info = unz_file_info64::default();
        let mut dummy_internal = unz_file_info64_internal::default();
        let mut dummy_extra = [0u8; 1];
        let mut dummy_comment = [0u8; 1];

        err = unz64local_GetCurrentFileInfoInternal(
            s,
            &mut dummy_info,
            &mut dummy_internal,
            &mut szCurrentFileName,
            &mut dummy_extra,
            &mut dummy_comment,
        );

        if err == UNZ_OK {
            // Find null terminator in szCurrentFileName
            let name_len = szCurrentFileName
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(szCurrentFileName.len());
            if unzStringFileNameCompare(
                &szCurrentFileName[..name_len],
                szFileName,
                iCaseSensitivity,
            ) == 0
            {
                return UNZ_OK;
            }
            err = unzGoToNextFile(s);
        }
    }

    // Restore state
    s.num_file = num_fileSaved;
    s.pos_in_central_dir = pos_in_central_dirSaved;
    s.cur_file_info = cur_file_infoSaved;
    s.cur_file_info_internal = cur_file_info_internalSaved;
    err
}

// ---------------------------------------------------------------------------
// Check current file coherency header
// ---------------------------------------------------------------------------

fn unz64local_CheckCurrentFileCoherencyHeader(
    s: &unz64_s,
    piSizeVar: &mut uInt,
    poffset_local_extrafield: &mut ZPOS64_T,
    psize_local_extrafield: &mut uInt,
) -> c_int {
    *piSizeVar = 0;
    *poffset_local_extrafield = 0;
    *psize_local_extrafield = 0;

    let mut f = match s.filestream.try_clone() {
        Ok(f) => f,
        Err(_) => return UNZ_ERRNO,
    };

    if f.seek(SeekFrom::Start(
        s.cur_file_info_internal.offset_curfile + s.byte_before_the_zipfile,
    ))
    .is_err()
    {
        return UNZ_ERRNO;
    }

    let mut err = UNZ_OK;

    // Check magic
    match unz64local_getLong(&mut f) {
        Ok(uMagic) => {
            if uMagic != 0x04034b50 {
                err = UNZ_BADZIPFILE;
            }
        }
        Err(_) => {
            err = UNZ_ERRNO;
        }
    }

    if unz64local_getShort(&mut f).is_err() {
        err = UNZ_ERRNO;
    }
    if unz64local_getShort(&mut f).is_err() {
        err = UNZ_ERRNO;
    }

    let uData = unz64local_getShort(&mut f).unwrap_or(0);
    if err == UNZ_OK && uData != s.cur_file_info.compression_method {
        err = UNZ_BADZIPFILE;
    }

    if (err == UNZ_OK)
        && s.cur_file_info.compression_method != 0
        && s.cur_file_info.compression_method != Z_DEFLATED
    {
        err = UNZ_BADZIPFILE;
    }

    // date/time
    if unz64local_getLong(&mut f).is_err() {
        err = UNZ_ERRNO;
    }

    // crc
    let uData = unz64local_getLong(&mut f).unwrap_or(0);
    let uFlags = s.cur_file_info.flag;
    if err == UNZ_OK && uData != s.cur_file_info.crc && (uFlags & 8) == 0 {
        err = UNZ_BADZIPFILE;
    }

    // size compr
    let uData = unz64local_getLong(&mut f).unwrap_or(0);
    if uData != 0xFFFFFFFF
        && err == UNZ_OK
        && uData as ZPOS64_T != s.cur_file_info.compressed_size
        && (uFlags & 8) == 0
    {
        err = UNZ_BADZIPFILE;
    }

    // size uncompr
    let uData = unz64local_getLong(&mut f).unwrap_or(0);
    if uData != 0xFFFFFFFF
        && err == UNZ_OK
        && uData as ZPOS64_T != s.cur_file_info.uncompressed_size
        && (uFlags & 8) == 0
    {
        err = UNZ_BADZIPFILE;
    }

    let size_filename = unz64local_getShort(&mut f).unwrap_or(0);
    if err == UNZ_OK && size_filename != s.cur_file_info.size_filename {
        err = UNZ_BADZIPFILE;
    }

    *piSizeVar += size_filename;

    let size_extra_field = unz64local_getShort(&mut f).unwrap_or(0);
    *poffset_local_extrafield = s.cur_file_info_internal.offset_curfile
        + SIZEZIPLOCALHEADER as ZPOS64_T
        + size_filename as ZPOS64_T;
    *psize_local_extrafield = size_extra_field;

    *piSizeVar += size_extra_field;

    err
}

// ---------------------------------------------------------------------------
// Open current file for reading
// ---------------------------------------------------------------------------

pub fn unzOpenCurrentFile(s: &mut unz64_s) -> c_int {
    unzOpenCurrentFile3(s, 0)
}

fn unzOpenCurrentFile3(s: &mut unz64_s, raw: c_int) -> c_int {
    if s.current_file_ok == 0 {
        return UNZ_PARAMERROR;
    }

    if s.pfile_in_zip_read.is_some() {
        unzCloseCurrentFile(s);
    }

    let mut iSizeVar: uInt = 0;
    let mut offset_local_extrafield: ZPOS64_T = 0;
    let mut size_local_extrafield: uInt = 0;

    if unz64local_CheckCurrentFileCoherencyHeader(
        s,
        &mut iSizeVar,
        &mut offset_local_extrafield,
        &mut size_local_extrafield,
    ) != UNZ_OK
    {
        return UNZ_BADZIPFILE;
    }

    let read_buffer = vec![0u8; UNZ_BUFSIZE];

    if s.cur_file_info.compression_method != 0 && s.cur_file_info.compression_method != Z_DEFLATED {
        return UNZ_BADZIPFILE;
    }

    let stream = if s.cur_file_info.compression_method == Z_DEFLATED && raw == 0 {
        // Initialize inflate with raw deflate (no zlib header)
        Some(flate2::Decompress::new(false))
    } else {
        None
    };
    let stream_initialised = if stream.is_some() { Z_DEFLATED } else { 0 };

    let pfile_in_zip_read_info = Box::new(file_in_zip64_read_info_s {
        read_buffer,
        stream,
        pos_in_zipfile: s.cur_file_info_internal.offset_curfile
            + SIZEZIPLOCALHEADER as ZPOS64_T
            + iSizeVar as ZPOS64_T,
        stream_initialised,
        offset_local_extrafield,
        size_local_extrafield,
        pos_local_extrafield: 0,
        total_out_64: 0,
        crc32: 0,
        crc32_wait: s.cur_file_info.crc,
        rest_read_compressed: s.cur_file_info.compressed_size,
        rest_read_uncompressed: s.cur_file_info.uncompressed_size,
        filestream: match s.filestream.try_clone() {
            Ok(f) => f,
            Err(_) => return UNZ_INTERNALERROR,
        },
        compression_method: s.cur_file_info.compression_method,
        byte_before_the_zipfile: s.byte_before_the_zipfile,
        raw,
    });

    s.pfile_in_zip_read = Some(pfile_in_zip_read_info);
    s.encrypted = 0;

    UNZ_OK
}

// ---------------------------------------------------------------------------
// Read from current file
// ---------------------------------------------------------------------------

pub fn unzReadCurrentFile(s: &mut unz64_s, buf: &mut [u8]) -> isize {
    let pfile_in_zip_read_info = match &mut s.pfile_in_zip_read {
        Some(p) => p,
        None => return UNZ_PARAMERROR as isize,
    };

    if pfile_in_zip_read_info.read_buffer.is_empty() {
        return UNZ_END_OF_LIST_OF_FILE as isize;
    }
    if buf.is_empty() {
        return 0;
    }

    let len = buf.len();
    let mut iRead: usize = 0;
    let mut out_pos: usize = 0;

    let mut avail_out = if pfile_in_zip_read_info.raw == 0
        && len > pfile_in_zip_read_info.rest_read_uncompressed as usize
    {
        pfile_in_zip_read_info.rest_read_uncompressed as usize
    } else {
        len
    };

    while avail_out > 0 {
        // Read more compressed data if needed
        if pfile_in_zip_read_info.compression_method == 0 || pfile_in_zip_read_info.raw != 0 {
            // Stored (no compression)
            if pfile_in_zip_read_info.rest_read_compressed == 0 {
                return if iRead == 0 {
                    UNZ_EOF as isize
                } else {
                    iRead as isize
                };
            }

            let uReadThis =
                (UNZ_BUFSIZE as ZPOS64_T).min(pfile_in_zip_read_info.rest_read_compressed) as usize;

            if uReadThis == 0 {
                return UNZ_EOF as isize;
            }

            if pfile_in_zip_read_info
                .filestream
                .seek(SeekFrom::Start(
                    pfile_in_zip_read_info.pos_in_zipfile
                        + pfile_in_zip_read_info.byte_before_the_zipfile,
                ))
                .is_err()
            {
                return UNZ_ERRNO as isize;
            }

            if pfile_in_zip_read_info
                .filestream
                .read_exact(&mut pfile_in_zip_read_info.read_buffer[..uReadThis])
                .is_err()
            {
                return UNZ_ERRNO as isize;
            }

            pfile_in_zip_read_info.pos_in_zipfile += uReadThis as ZPOS64_T;
            pfile_in_zip_read_info.rest_read_compressed -= uReadThis as ZPOS64_T;

            let uDoCopy = avail_out.min(uReadThis);

            buf[out_pos..out_pos + uDoCopy]
                .copy_from_slice(&pfile_in_zip_read_info.read_buffer[..uDoCopy]);

            pfile_in_zip_read_info.total_out_64 += uDoCopy as ZPOS64_T;
            pfile_in_zip_read_info.crc32 = crc32(
                pfile_in_zip_read_info.crc32,
                &buf[out_pos..out_pos + uDoCopy],
            );
            pfile_in_zip_read_info.rest_read_uncompressed -= uDoCopy as ZPOS64_T;
            iRead += uDoCopy;
            out_pos += uDoCopy;
            avail_out -= uDoCopy;
        } else if pfile_in_zip_read_info.compression_method == Z_DEFLATED {
            // Deflate
            let uReadThis =
                (UNZ_BUFSIZE as ZPOS64_T).min(pfile_in_zip_read_info.rest_read_compressed) as usize;

            if uReadThis == 0 {
                return UNZ_EOF as isize;
            }

            if pfile_in_zip_read_info
                .filestream
                .seek(SeekFrom::Start(
                    pfile_in_zip_read_info.pos_in_zipfile
                        + pfile_in_zip_read_info.byte_before_the_zipfile,
                ))
                .is_err()
            {
                return UNZ_ERRNO as isize;
            }

            if pfile_in_zip_read_info
                .filestream
                .read_exact(&mut pfile_in_zip_read_info.read_buffer[..uReadThis])
                .is_err()
            {
                return UNZ_ERRNO as isize;
            }

            pfile_in_zip_read_info.pos_in_zipfile += uReadThis as ZPOS64_T;
            pfile_in_zip_read_info.rest_read_compressed -= uReadThis as ZPOS64_T;

            if let Some(ref mut stream) = pfile_in_zip_read_info.stream {
                let total_out_before = stream.total_out();
                let mut input = &pfile_in_zip_read_info.read_buffer[..uReadThis];
                let mut output = &mut buf[out_pos..out_pos + avail_out];
                let orig_in_len = input.len();
                let orig_out_len = output.len();

                match stream.decompress(&mut input, &mut output, flate2::FlushDecompress::None) {
                    Ok(_status) => {
                        let bytes_consumed = orig_in_len - input.len();
                        let bytes_written = orig_out_len - output.len();

                        pfile_in_zip_read_info.total_out_64 += bytes_written as ZPOS64_T;
                        pfile_in_zip_read_info.crc32 = crc32(
                            pfile_in_zip_read_info.crc32,
                            &buf[out_pos..out_pos + bytes_written],
                        );
                        pfile_in_zip_read_info.rest_read_uncompressed -= bytes_written as ZPOS64_T;
                        iRead += bytes_written;
                        out_pos += bytes_written;
                        avail_out -= bytes_written;

                        // If stream ended or no progress, return
                        if bytes_consumed == 0 {
                            return if iRead == 0 {
                                UNZ_EOF as isize
                            } else {
                                iRead as isize
                            };
                        }
                    }
                    Err(_) => {
                        return UNZ_ERRNO as isize;
                    }
                }
            } else {
                return UNZ_INTERNALERROR as isize;
            }
        } else {
            // Unknown compression
            break;
        }
    }

    iRead as isize
}

// ---------------------------------------------------------------------------
// Close current file
// ---------------------------------------------------------------------------

pub fn unzCloseCurrentFile(s: &mut unz64_s) -> c_int {
    let pfile_in_zip_read_info = match &s.pfile_in_zip_read {
        Some(p) => p,
        None => return UNZ_PARAMERROR,
    };

    let mut err = UNZ_OK;

    if pfile_in_zip_read_info.rest_read_uncompressed == 0 && pfile_in_zip_read_info.raw == 0 {
        if pfile_in_zip_read_info.crc32 != pfile_in_zip_read_info.crc32_wait {
            err = UNZ_CRCERROR;
        }
    }

    s.pfile_in_zip_read = None;
    err
}

// ---------------------------------------------------------------------------
// Stub functions for R integration (called from R's .External interface)
// ---------------------------------------------------------------------------

// TODO: These are stubs. The actual R integration (Runzip, R_newunz, etc.)
// requires R SEXP types and connection infrastructure.
// They are provided here as placeholders for when the full integration is needed.

/// Stub: Runzip - R's .External interface for unzipping files
/// TODO: Implement when R SEXP infrastructure is connected
pub unsafe fn Runzip(args: *mut c_void) -> *mut c_void {
    ptr::null_mut()
}

/// Stub: R_newunz - create an unz connection
/// TODO: Implement when R connection infrastructure is connected
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_newunz(description: *const c_char, mode: *const c_char) -> *mut c_void {
    ptr::null_mut()
}
