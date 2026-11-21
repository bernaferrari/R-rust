use super::*;
use crate::mainutils::rfile::{RFile, r_fflush, r_fgetc, r_fputc, r_fread, r_fwrite};

// ---------------------------------------------------------------------------
// Memory buffer operations
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct membuf_st {
    pub size: R_size_t,
    pub count: R_size_t,
    pub buf: *mut u8,
}

pub fn resize_buffer(mb: *mut membuf_st, needed: R_size_t) {
    // SAFETY: `mb` is a caller-owned membuf whose `buf` is a Vec handed
    // out as a raw pointer; `size` always equals its capacity, so the
    // Vec can be reconstructed, grown, and forgotten here.
    unsafe {
        if mb.is_null() {
            return;
        }
        let new_size = std::cmp::max(needed, (*mb).size * 2);
        // `buf` is a Vec<u8> handed out as a raw pointer; `size` always
        // equals its capacity so it can be reconstructed here and in
        // free_mem_buffer. (Input streams alias foreign memory and never
        // resize, matching the C original.)
        let mut v: Vec<u8> = if (*mb).buf.is_null() {
            Vec::with_capacity(new_size)
        } else {
            let mut v = Vec::from_raw_parts((*mb).buf, 0, (*mb).size);
            // reserve_exact(additional) only guarantees capacity >=
            // len + additional; len is 0 here, so the argument must be
            // the TOTAL requirement. The previous
            // `new_size - v.capacity()` delta under-allocated whenever
            // new_size > 2 * capacity: the following raw writes overflowed
            // the heap (silent allocator abort).
            v.reserve_exact(new_size);
            v
        };
        (*mb).buf = v.as_mut_ptr();
        (*mb).size = v.capacity();
        std::mem::forget(v);
    }
}

pub extern "C" fn OutCharMem(stream: R_outpstream_t, c: c_int) {
    // SAFETY: `stream` is the caller-owned output stream whose `data`
    // points at a live membuf; null/capacity checks precede the store.
    unsafe {
        if stream.is_null() {
            return;
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            return;
        }
        if (*mb).count >= (*mb).size {
            resize_buffer(mb, (*mb).count + 1);
        }
        if !(*mb).buf.is_null() {
            *(*mb).buf.add((*mb).count) = c as u8;
            (*mb).count += 1;
        }
    }
}

pub extern "C" fn OutBytesMem(stream: R_outpstream_t, buf: *const c_void, length: c_int) {
    // SAFETY: `stream` is the caller-owned output stream whose `data`
    // points at a live membuf, and `buf` is a caller-owned read buffer
    // of `length` bytes; capacity is grown before the copy.
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            return;
        }
        let needed = (*mb).count + (length as R_size_t);
        if needed > (*mb).size {
            resize_buffer(mb, needed);
        }
        if !(*mb).buf.is_null() {
            ptr::copy_nonoverlapping(
                buf as *const u8,
                (*mb).buf.add((*mb).count),
                length as usize,
            );
            (*mb).count = needed;
        }
    }
}

pub extern "C" fn InCharMem(stream: R_inpstream_t) -> c_int {
    // SAFETY: `stream` is the caller-owned input stream whose `data`
    // points at a live membuf; bounds checks precede the byte read.
    unsafe {
        if stream.is_null() {
            return -1;
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            return -1;
        }
        if (*mb).count >= (*mb).size {
            return -1;
        }
        let val = *(*mb).buf.add((*mb).count) as c_int;
        (*mb).count += 1;
        val
    }
}

pub extern "C" fn InBytesMem(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {
    // SAFETY: `stream` is the caller-owned input stream whose `data`
    // points at a live membuf, and `buf` is a caller-owned write buffer
    // of at least `length` bytes; bounds checks precede the copy.
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            return;
        }
        if (*mb).count + (length as R_size_t) > (*mb).size {
            return;
        }
        ptr::copy_nonoverlapping((*mb).buf.add((*mb).count), buf as *mut u8, length as usize);
        (*mb).count += length as R_size_t;
    }
}

pub fn InitMemInPStream(
    stream: R_inpstream_t,
    mb: *mut membuf_st,
    buf: *mut c_void,
    length: R_size_t,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    // SAFETY: `mb` is a caller-owned membuf struct and `buf` a
    // caller-owned input region of `length` bytes that outlives the
    // stream; the null check precedes the field stores.
    unsafe {
        if mb.is_null() {
            return;
        }
        (*mb).count = 0;
        (*mb).size = length;
        (*mb).buf = buf as *mut u8;
        R_InitInPStream(
            stream,
            mb as R_pstream_data_t,
            R_pstream_format_t::R_pstream_any_format,
            Some(InCharMem),
            Some(InBytesMem),
            phook,
            pdata,
        );
    }
}

pub fn InitMemOutPStream(
    stream: R_outpstream_t,
    mb: *mut membuf_st,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    // SAFETY: `mb` is a caller-owned membuf struct; the null check
    // precedes the initializing field stores.
    unsafe {
        if mb.is_null() {
            return;
        }
        (*mb).count = 0;
        (*mb).size = 0;
        (*mb).buf = ptr::null_mut();
        R_InitOutPStream(
            stream,
            mb as R_pstream_data_t,
            type_,
            version,
            Some(OutCharMem),
            Some(OutBytesMem),
            phook,
            pdata,
        );
    }
}

pub fn CloseMemOutPStream(stream: R_outpstream_t) -> SEXP {
    // SAFETY: `stream` is the caller-owned output stream whose `data`
    // points at a live membuf; the buffer is copied out before the
    // membuf is released.
    unsafe {
        if stream.is_null() {
            error("cannot allocate buffer");
        }
        let mb = (*stream).data as *mut membuf_st;
        if mb.is_null() {
            error("cannot allocate buffer");
        }
        let count = (*mb).count;
        let val = Rf_allocVector3(SEXPTYPE::RAWSXP, count as R_xlen_t);
        if !val.is_null() && count > 0 && !(*mb).buf.is_null() {
            let bytes = std::slice::from_raw_parts((*mb).buf, count);
            for (index, byte) in bytes.iter().enumerate() {
                SET_RAW_ELT(val, index as c_int, *byte);
            }
        }
        free_mem_buffer(mb as *mut c_void);
        val
    }
}

pub fn free_mem_buffer(data: *mut c_void) {
    // SAFETY: `data` is a membuf whose `buf` is a Vec handed out as a
    // raw pointer with `size` equal to its capacity; it is reclaimed
    // exactly once here and nulled out.
    unsafe {
        if data.is_null() {
            return;
        }
        let mb = data as *mut membuf_st;
        if !(*mb).buf.is_null() {
            drop(Vec::from_raw_parts((*mb).buf, 0, (*mb).size));
            (*mb).buf = ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// Buffer-connection operations
// ---------------------------------------------------------------------------

pub fn InitBConOutPStream(
    stream: R_outpstream_t,
    bbs: *mut c_void,
    con: *mut c_void,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    {
        if !bbs.is_null() {
            R_InitOutPStream(
                stream,
                bbs as R_pstream_data_t,
                type_,
                version,
                Some(OutCharMem),
                Some(OutBytesMem),
                phook,
                pdata,
            );
        } else {
            R_InitOutPStream(
                stream,
                con as R_pstream_data_t,
                type_,
                version,
                Some(OutCharFile),
                Some(OutBytesFile),
                phook,
                pdata,
            );
        }
    }
}

pub fn flush_bcon_buffer(bbs: *mut c_void) {
    // SAFETY: `bbs` is a caller-owned RFile cast; the null check
    // precedes the flush call.
    unsafe {
        if !bbs.is_null() {
            let fp = bbs as *mut RFile;
            r_fflush(fp);
        }
    }
}

// ---------------------------------------------------------------------------
// File I/O callbacks
// ---------------------------------------------------------------------------

pub extern "C" fn OutCharFile(stream: R_outpstream_t, c: c_int) {
    // SAFETY: `stream` is the caller-owned output stream whose `data`
    // points at a live RFile; null checks precede the write.
    unsafe {
        if stream.is_null() {
            return;
        }
        let fp = (*stream).data as *mut RFile;
        if fp.is_null() {
            return;
        }
        r_fputc(c, fp);
    }
}

pub extern "C" fn OutBytesFile(stream: R_outpstream_t, buf: *const c_void, length: c_int) {
    // SAFETY: `stream` is the caller-owned output stream whose `data`
    // points at a live RFile, and `buf` is a caller-owned read buffer
    // of `length` bytes; null checks precede the write.
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let fp = (*stream).data as *mut RFile;
        if fp.is_null() {
            error("write failed");
        }
        let wrote = r_fwrite(buf, 1, length as usize, fp);
        if wrote != length as usize {
            error("write failed");
        }
    }
}

pub extern "C" fn InCharFile(stream: R_inpstream_t) -> c_int {
    // SAFETY: `stream` is the caller-owned input stream whose `data`
    // points at a live RFile; null checks precede the read.
    unsafe {
        if stream.is_null() {
            return -1;
        }
        let fp = (*stream).data as *mut RFile;
        if fp.is_null() {
            return -1;
        }
        r_fgetc(fp)
    }
}

pub extern "C" fn InBytesFile(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {
    // SAFETY: `stream` is the caller-owned input stream whose `data`
    // points at a live RFile, and `buf` is a caller-owned write buffer
    // of at least `length` bytes; null checks precede the read.
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let fp = (*stream).data as *mut RFile;
        if fp.is_null() {
            error("read error");
        }
        let read_n = r_fread(buf, 1, length as usize, fp);
        if read_n != length as usize {
            error("read error");
        }
    }
}

pub fn InInit(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {
    {
        if !stream.is_null() && !buf.is_null() && length > 0 {
            InBytesFile(stream, buf, length);
        }
    }
}

// ---------------------------------------------------------------------------
// Connection I/O
// ---------------------------------------------------------------------------

pub fn R_WriteConnection(con: *mut c_void, buf: *const c_void, n: usize) -> usize {
    // SAFETY: `con` is a caller-owned RFile cast and `buf` a
    // caller-owned read buffer of `n` bytes; null checks precede
    // the write.
    unsafe {
        if con.is_null() || buf.is_null() || n == 0 {
            return 0;
        }
        let fp = con as *mut RFile;
        if fp.is_null() {
            return 0;
        }
        r_fwrite(buf, 1, n, fp)
    }
}
