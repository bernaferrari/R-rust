use super::*;


// ---------------------------------------------------------------------------
// Memory buffer operations
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct membuf_st {
    pub size: R_size_t,
    pub count: R_size_t,
    pub buf: *mut u8,
}

pub unsafe fn resize_buffer(mb: *mut membuf_st, needed: R_size_t) {
    unsafe {
        if mb.is_null() {
            return;
        }
        let new_size = std::cmp::max(needed, (*mb).size * 2);
        let new_buf = libc::realloc((*mb).buf as *mut c_void, new_size) as *mut u8;
        if !new_buf.is_null() {
            (*mb).buf = new_buf;
            (*mb).size = new_size;
        }
    }
}

pub unsafe extern "C" fn OutCharMem(stream: R_outpstream_t, c: c_int) {
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

pub unsafe extern "C" fn OutBytesMem(stream: R_outpstream_t, buf: *const c_void, length: c_int) {
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

pub unsafe extern "C" fn InCharMem(stream: R_inpstream_t) -> c_int {
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

pub unsafe extern "C" fn InBytesMem(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {
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

pub unsafe fn InitMemInPStream(
    stream: R_inpstream_t,
    mb: *mut membuf_st,
    buf: *mut c_void,
    length: R_size_t,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
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

pub unsafe fn InitMemOutPStream(
    stream: R_outpstream_t,
    mb: *mut membuf_st,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
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

pub unsafe fn CloseMemOutPStream(stream: R_outpstream_t) -> SEXP {
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
            ptr::copy_nonoverlapping((*mb).buf, RAW(val), count);
        }
        free_mem_buffer(mb as *mut c_void);
        val
    }
}

pub unsafe fn free_mem_buffer(data: *mut c_void) {
    unsafe {
        if data.is_null() {
            return;
        }
        let mb = data as *mut membuf_st;
        if !(*mb).buf.is_null() {
            libc::free((*mb).buf as *mut c_void);
            (*mb).buf = ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// Buffer-connection operations
// ---------------------------------------------------------------------------

pub unsafe fn InitBConOutPStream(
    stream: R_outpstream_t,
    bbs: *mut c_void,
    con: *mut c_void,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
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

pub unsafe fn flush_bcon_buffer(bbs: *mut c_void) {
    unsafe {
        if !bbs.is_null() {
            let fp = bbs as *mut libc::FILE;
            libc::fflush(fp);
        }
    }
}

// ---------------------------------------------------------------------------
// File I/O callbacks
// ---------------------------------------------------------------------------

pub unsafe extern "C" fn OutCharFile(stream: R_outpstream_t, c: c_int) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let fp = (*stream).data as *mut libc::FILE;
        if fp.is_null() {
            return;
        }
        libc::fputc(c, fp);
    }
}

pub unsafe extern "C" fn OutBytesFile(stream: R_outpstream_t, buf: *const c_void, length: c_int) {
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let fp = (*stream).data as *mut libc::FILE;
        if fp.is_null() {
            error("write failed");
        }
        let wrote = libc::fwrite(buf, 1, length as usize, fp);
        if wrote != length as usize {
            error("write failed");
        }
    }
}

pub unsafe extern "C" fn InCharFile(stream: R_inpstream_t) -> c_int {
    unsafe {
        if stream.is_null() {
            return -1;
        }
        let fp = (*stream).data as *mut libc::FILE;
        if fp.is_null() {
            return -1;
        }
        libc::fgetc(fp)
    }
}

pub unsafe extern "C" fn InBytesFile(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        let fp = (*stream).data as *mut libc::FILE;
        if fp.is_null() {
            error("read error");
        }
        let read_n = libc::fread(buf, 1, length as usize, fp);
        if read_n != length as usize {
            error("read error");
        }
    }
}

pub unsafe fn InInit(stream: R_inpstream_t, buf: *mut c_void, length: c_int) {
    unsafe {
        if !stream.is_null() && !buf.is_null() && length > 0 {
            InBytesFile(stream, buf, length);
        }
    }
}

// ---------------------------------------------------------------------------
// Connection I/O
// ---------------------------------------------------------------------------

pub unsafe fn R_WriteConnection(con: *mut c_void, buf: *const c_void, n: usize) -> usize {
    unsafe {
        if con.is_null() || buf.is_null() || n == 0 {
            return 0;
        }
        let fp = con as *mut libc::FILE;
        if fp.is_null() {
            return 0;
        }
        libc::fwrite(buf, 1, n, fp)
    }
}
