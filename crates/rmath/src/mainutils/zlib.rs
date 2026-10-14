//! System-zlib gzip compression.
//!
//! GNU R's `gzfile()` connections compress through the zlib C API. Its
//! deflate output (and gzip container header OS byte) differs from pure-Rust
//! deflate implementations, so files written for byte parity with GNU R —
//! `saveRDS(compress=)` in particular — must be deflated by zlib itself.
//! The system library is linked directly; there is no Rust build-time
//! dependency. Not available on wasm32, where no caller links file
//! compression.

use std::os::raw::{c_char, c_int};

#[repr(C)]
struct ZStream {
    next_in: *mut u8,
    avail_in: c_int,
    total_in: u64,
    next_out: *mut u8,
    avail_out: c_int,
    total_out: u64,
    msg: *mut c_char,
    state: *mut u8,
    zalloc: *mut u8,
    zfree: *mut u8,
    opaque: *mut u8,
    data_type: c_int,
    adler: u64,
    reserved: u64,
}

#[cfg(not(target_arch = "wasm32"))]
#[link(name = "z")]
unsafe extern "C" {
    fn deflateInit2_(
        strm: *mut ZStream,
        level: c_int,
        method: c_int,
        window_bits: c_int,
        mem_level: c_int,
        strategy: c_int,
        version: *const c_char,
        stream_size: c_int,
    ) -> c_int;
    fn deflate(strm: *mut ZStream, flush: c_int) -> c_int;
    fn deflateEnd(strm: *mut ZStream) -> c_int;
}

const Z_DEFLATED: c_int = 8;
const Z_DEFAULT_STRATEGY: c_int = 0;
const Z_FINISH: c_int = 4;
const Z_STREAM_END: c_int = 1;

/// Deflate `data` into a gzip container with zlib's default memory level
/// (8), matching `gzwrite()` output for the given compression level.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn gzip_compress(data: &[u8], level: c_int) -> Result<Vec<u8>, String> {
    let version = c"1.2.11";
    let mut stream = ZStream {
        next_in: std::ptr::null_mut(),
        avail_in: 0,
        total_in: 0,
        next_out: std::ptr::null_mut(),
        avail_out: 0,
        total_out: 0,
        msg: std::ptr::null_mut(),
        state: std::ptr::null_mut(),
        zalloc: std::ptr::null_mut(),
        zfree: std::ptr::null_mut(),
        opaque: std::ptr::null_mut(),
        data_type: 0,
        adler: 0,
        reserved: 0,
    };
    // 15-bit window + 16 selects the gzip wrapper.
    let rc = unsafe {
        deflateInit2_(
            &mut stream,
            level,
            Z_DEFLATED,
            15 + 16,
            8,
            Z_DEFAULT_STRATEGY,
            version.as_ptr(),
            std::mem::size_of::<ZStream>() as c_int,
        )
    };
    if rc != 0 {
        return Err(format!("deflateInit2 failed: {rc}"));
    }
    let mut input = data.to_vec();
    stream.next_in = input.as_mut_ptr();
    stream.avail_in = data.len() as c_int;
    let mut out = Vec::with_capacity(data.len() / 2 + 64);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        stream.next_out = chunk.as_mut_ptr();
        stream.avail_out = chunk.len() as c_int;
        let rc = unsafe { deflate(&mut stream, Z_FINISH) };
        let produced = chunk.len() - stream.avail_out as usize;
        out.extend_from_slice(&chunk[..produced]);
        if rc == Z_STREAM_END {
            break;
        }
        if rc != 0 {
            unsafe { deflateEnd(&mut stream) };
            return Err(format!("deflate failed: {rc}"));
        }
        if stream.avail_out != 0 {
            // No progress possible without more input.
            break;
        }
    }
    unsafe { deflateEnd(&mut stream) };
    // GNU gzfile writes OS byte 3 (Unix); system zlib may emit 0. The
    // gzip header is 10 bytes when FLG=0, and we never set FNAME/FCOMMENT.
    if out.len() >= 10 && out[3] == 0 {
        out[9] = 0x03;
    }
    Ok(out)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn gzip_compress(_data: &[u8], _level: c_int) -> Result<Vec<u8>, String> {
    Err("gzip compression requires the native zlib library".to_string())
}
