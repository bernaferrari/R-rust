use super::*;


// ---------------------------------------------------------------------------
// Compression functions
// ---------------------------------------------------------------------------

pub unsafe fn R_compress1(inp: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_compress1 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen > u32::MAX as usize {
            error("raw vector too large to compress");
        }
        let input = slice::from_raw_parts(RAW(inp), inlen);
        let payload = match zlib_compress(input) {
            Ok(data) => data,
            Err(err) => error(&format!("internal error in R_compress1: {err}")),
        };
        raw_from_bytes(&build_compressed_blob(inlen, None, &payload))
    }
}

pub unsafe fn R_decompress1(inp: SEXP, err: *mut Rboolean) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_decompress1 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen < 4 {
            mark_decompress_error(err);
            return R_NilValue();
        }
        let data = slice::from_raw_parts(RAW(inp), inlen);
        let outlen = match parse_swapped_len_prefix(data) {
            Some(v) => v,
            None => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        match zlib_decompress_exact(&data[4..], outlen) {
            Ok(decoded) => raw_from_bytes(&decoded),
            Err(_) => {
                mark_decompress_error(err);
                R_NilValue()
            }
        }
    }
}

pub unsafe fn R_compress2(inp: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_compress2 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen > u32::MAX as usize {
            error("raw vector too large to compress");
        }
        let input = slice::from_raw_parts(RAW(inp), inlen);
        let (marker, payload) = match bzip2_compress(input) {
            Ok(compressed) if compressed.len() <= inlen => (b'2', compressed),
            Ok(_) => (b'0', input.to_vec()),
            Err(err) => error(&format!("internal error in R_compress2: {err}")),
        };
        raw_from_bytes(&build_compressed_blob(inlen, Some(marker), &payload))
    }
}

pub unsafe fn R_decompress2(inp: SEXP, err: *mut Rboolean) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_decompress2 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen < 5 {
            mark_decompress_error(err);
            return R_NilValue();
        }
        let data = slice::from_raw_parts(RAW(inp), inlen);
        let outlen = match parse_swapped_len_prefix(data) {
            Some(v) => v,
            None => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        let decoded = match data[4] {
            b'2' => bzip2_decompress_exact(&data[5..], outlen),
            b'1' => zlib_decompress_exact(&data[5..], outlen),
            b'0' => {
                if data.len() < 5 + outlen {
                    mark_decompress_error(err);
                    return R_NilValue();
                }
                Ok(data[5..5 + outlen].to_vec())
            }
            _ => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        match decoded {
            Ok(out) => raw_from_bytes(&out),
            Err(_) => {
                mark_decompress_error(err);
                R_NilValue()
            }
        }
    }
}

pub unsafe fn R_compress3(inp: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_compress3 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen > u32::MAX as usize {
            error("raw vector too large to compress");
        }
        let input = slice::from_raw_parts(RAW(inp), inlen);
        let (marker, payload) = match lzma2_raw_encode(input, inlen + 5) {
            Ok(compressed) => (b'Z', compressed),
            Err(_) => (b'0', input.to_vec()),
        };
        raw_from_bytes(&build_compressed_blob(inlen, Some(marker), &payload))
    }
}

pub unsafe fn R_decompress3(inp: SEXP, err: *mut Rboolean) -> SEXP {
    unsafe {
        if TYPEOF(inp) != SEXPTYPE::RAWSXP {
            error("R_decompress3 requires a raw vector");
        }
        let inlen = XLENGTH(inp) as usize;
        if inlen < 5 {
            mark_decompress_error(err);
            return R_NilValue();
        }
        let data = slice::from_raw_parts(RAW(inp), inlen);
        let outlen = match parse_swapped_len_prefix(data) {
            Some(v) => v,
            None => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        let decoded = match data[4] {
            b'Z' => lzma2_raw_decode(&data[5..], outlen).map_err(|_| ()),
            b'2' => bzip2_decompress_exact(&data[5..], outlen).map_err(|_| ()),
            b'1' => zlib_decompress_exact(&data[5..], outlen).map_err(|_| ()),
            b'0' => {
                if data.len() < 5 + outlen {
                    mark_decompress_error(err);
                    return R_NilValue();
                }
                Ok(data[5..5 + outlen].to_vec())
            }
            _ => {
                mark_decompress_error(err);
                return R_NilValue();
            }
        };
        match decoded {
            Ok(out) => raw_from_bytes(&out),
            Err(_) => {
                mark_decompress_error(err);
                R_NilValue()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R_snprintf utility
// ---------------------------------------------------------------------------

pub unsafe fn Rsnprintf(buf: *mut c_char, size: usize, _format: *const c_char) -> c_int {
    unsafe {
        if !buf.is_null() && size > 0 {
            *buf = 0;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// InStringStream helper
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct R_instring_stream_st {
    pub last: c_int,
    pub stream: R_inpstream_t,
}

pub type R_instring_stream_t = *mut R_instring_stream_st;

pub unsafe fn InitInStringStream(s: R_instring_stream_t, stream: R_inpstream_t) {
    unsafe {
        if !s.is_null() {
            (*s).last = 0;
            (*s).stream = stream;
        }
    }
}

pub unsafe fn GetChar(s: R_instring_stream_t) -> c_int {
    -1
}

pub unsafe fn UngetChar(s: R_instring_stream_t, c: c_int) {
    unsafe {
        if !s.is_null() {
            (*s).last = c;
        }
    }
}
