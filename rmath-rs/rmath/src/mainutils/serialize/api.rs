use super::*;

// ---------------------------------------------------------------------------
// R_Serialize -- serialize an R object to a stream (C API)
// ---------------------------------------------------------------------------

pub unsafe fn R_Serialize(s: SEXP, stream: R_outpstream_t) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let out_ref = &mut *stream;
        // Use OutFormat
        match out_ref.type_ {
            R_pstream_format_t::R_pstream_binary_format => {
                if let Some(out_bytes) = out_ref.OutBytes {
                    out_bytes(stream, b"B\n".as_ptr() as *const c_void, 2);
                }
            }
            R_pstream_format_t::R_pstream_xdr_format => {
                if let Some(out_bytes) = out_ref.OutBytes {
                    out_bytes(stream, b"X\n".as_ptr() as *const c_void, 2);
                }
            }
            R_pstream_format_t::R_pstream_ascii_format
            | R_pstream_format_t::R_pstream_asciihex_format => {
                if let Some(out_bytes) = out_ref.OutBytes {
                    out_bytes(stream, b"A\n".as_ptr() as *const c_void, 2);
                }
            }
            _ => {} // intentionally unhandled: unknown serialization format
        }

        // Write version info (version 3)
        let version = out_ref.version;
        let write_i32_to_stream = |val: i32, stream: R_outpstream_t| {
            let bytes = val.to_ne_bytes();
            if let Some(out_bytes) = (*stream).OutBytes {
                out_bytes(stream, bytes.as_ptr() as *const c_void, 4);
            }
        };

        write_i32_to_stream(3, stream); // version
        write_i32_to_stream(R_VERSION, stream); // writer version
        write_i32_to_stream(R_VERSION_350, stream); // min reader version

        // Write native encoding (empty string for our purposes)
        write_i32_to_stream(0, stream); // encoding name length = 0

        // Write the object using a temporary buffer, then stream it out
        let mut writer = BinaryWriter::new();
        let mut ref_table = WriteHashTable::new();
        WriteItemInternal(s, &mut ref_table, &mut writer);

        // Stream out the serialized bytes
        let data = writer.into_vec();
        if !data.is_empty()
            && let Some(out_bytes) = (*stream).OutBytes
        {
            // Write in chunks to avoid c_int overflow
            let mut offset = 0usize;
            while offset < data.len() {
                let chunk_len = std::cmp::min(CHUNK_SIZE, data.len() - offset);
                let chunk_len_int = chunk_len as c_int;
                if chunk_len_int < 0 {
                    break;
                }
                out_bytes(
                    stream,
                    data[offset..].as_ptr() as *const c_void,
                    chunk_len_int,
                );
                offset += chunk_len;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R_Unserialize -- unserialize an R object from a stream (C API)
// ---------------------------------------------------------------------------

pub unsafe fn R_Unserialize(stream: R_inpstream_t) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let bytes = read_stream_bytes_via_inchar(stream);
        if bytes.is_empty() {
            error("read error");
        }
        let raw = raw_from_bytes(&bytes);
        R_unserialize(raw, R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// R_SerializeInfo
// ---------------------------------------------------------------------------

pub unsafe fn R_SerializeInfo(stream: R_inpstream_t) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        InFormat(stream);

        let version = InInteger(stream);
        let anslen = if version == 3 { 5 } else { 4 };
        let writer_version = InInteger(stream);
        let min_reader_version = InInteger(stream);

        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, anslen as R_xlen_t);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, anslen as R_xlen_t);
        let _ans_guard = protect(ans);
        let _names_guard = protect(names);

        SET_STRING_ELT(names, 0, Rf_mkChar(c"version".as_ptr()));
        SET_VECTOR_ELT(ans, 0, Rf_ScalarInteger(version));

        SET_STRING_ELT(names, 1, Rf_mkChar(c"writer_version".as_ptr()));
        let mut vv = 0;
        let mut vp = 0;
        let mut vs = 0;
        DecodeVersion(writer_version, &mut vv, &mut vp, &mut vs);
        let writer_s = format!("{vv}.{vp}.{vs}");
        let writer_c = CString::new(writer_s).unwrap_or_default();
        SET_VECTOR_ELT(ans, 1, Rf_mkString(writer_c.as_ptr()));

        SET_STRING_ELT(names, 2, Rf_mkChar(c"min_reader_version".as_ptr()));
        if min_reader_version < 0 {
            SET_VECTOR_ELT(ans, 2, Rf_ScalarString(R_NaString()));
        } else {
            DecodeVersion(min_reader_version, &mut vv, &mut vp, &mut vs);
            let min_reader_s = format!("{vv}.{vp}.{vs}");
            let min_reader_c = CString::new(min_reader_s).unwrap_or_default();
            SET_VECTOR_ELT(ans, 2, Rf_mkString(min_reader_c.as_ptr()));
        }

        SET_STRING_ELT(names, 3, Rf_mkChar(c"format".as_ptr()));
        match (*stream).type_ {
            R_pstream_format_t::R_pstream_ascii_format
            | R_pstream_format_t::R_pstream_asciihex_format => {
                SET_VECTOR_ELT(ans, 3, Rf_mkString(c"ascii".as_ptr()));
            }
            R_pstream_format_t::R_pstream_binary_format => {
                SET_VECTOR_ELT(ans, 3, Rf_mkString(c"binary".as_ptr()));
            }
            R_pstream_format_t::R_pstream_xdr_format => {
                SET_VECTOR_ELT(ans, 3, Rf_mkString(c"xdr".as_ptr()));
            }
            _ => error("unknown input format"),
        }

        if version == 3 {
            SET_STRING_ELT(names, 4, Rf_mkChar(c"native_encoding".as_ptr()));
            let nelen = InInteger(stream);
            if !(0..=R_CODESET_MAX).contains(&nelen) {
                error("invalid length of encoding name");
            }
            if nelen == 0 {
                SET_VECTOR_ELT(ans, 4, Rf_mkString(c"".as_ptr()));
            } else {
                let mut bytes = vec![0u8; nelen as usize];
                InString(stream, bytes.as_mut_ptr() as *mut c_char, nelen);
                let enc_ch = Rf_mkCharLen(bytes.as_ptr() as *const c_char, nelen);
                SET_VECTOR_ELT(ans, 4, Rf_ScalarString(enc_ch));
            }
        }

        setAttrib(ans, R_NamesSymbol(), names);
        ans
    }
}

// ---------------------------------------------------------------------------
// R_ReadItem / R_WriteItem (C stream API)
// ---------------------------------------------------------------------------

pub unsafe fn R_ReadItem(stream: R_inpstream_t) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let bytes = read_stream_bytes_via_inchar(stream);
        if bytes.is_empty() {
            error("read error");
        }
        let mut reader = BinaryReader::new(&bytes);
        let mut ref_table = ReadRefTable::new();
        match ReadItemInternal(&mut reader, &mut ref_table) {
            Ok(v) => v,
            Err(_) => error("read error"),
        }
    }
}

pub unsafe fn R_WriteItem(s: SEXP, stream: R_outpstream_t) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let mut writer = BinaryWriter::new();
        let mut ref_table = WriteHashTable::new();
        WriteItemInternal(s, &mut ref_table, &mut writer);
        let data = writer.into_vec();
        if !data.is_empty()
            && let Some(out_bytes) = (*stream).OutBytes
        {
            let mut offset = 0usize;
            while offset < data.len() {
                let chunk_len = std::cmp::min(CHUNK_SIZE, data.len() - offset);
                let chunk_len_int = chunk_len as c_int;
                if chunk_len_int < 0 {
                    break;
                }
                out_bytes(
                    stream,
                    data[offset..].as_ptr() as *const c_void,
                    chunk_len_int,
                );
                offset += chunk_len;
            }
        }
    }
}

pub unsafe fn read_stream_bytes_via_inchar(stream: R_inpstream_t) -> Vec<u8> {
    unsafe {
        if stream.is_null() {
            return Vec::new();
        }
        let Some(in_char) = (*stream).InChar else {
            return Vec::new();
        };
        let mut out = Vec::new();
        loop {
            let ch = in_char(stream);
            if ch < 0 {
                break;
            }
            out.push(ch as u8);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Stream initializers
// ---------------------------------------------------------------------------

pub unsafe fn R_InitInPStream(
    stream: R_inpstream_t,
    data: R_pstream_data_t,
    type_: R_pstream_format_t,
    inchar: Option<unsafe extern "C" fn(R_inpstream_t) -> c_int>,
    inbytes: Option<unsafe extern "C" fn(R_inpstream_t, *mut c_void, c_int)>,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        if stream.is_null() {
            return;
        }
        (*stream).data = data;
        (*stream).type_ = type_;
        (*stream).InChar = inchar;
        (*stream).InBytes = inbytes;
        (*stream).InPersistHookFunc = phook;
        (*stream).InPersistHookData = pdata;
        (*stream).native_encoding[0] = 0;
        (*stream).nat2nat_obj = ptr::null_mut();
        (*stream).nat2utf8_obj = ptr::null_mut();
    }
}

pub unsafe fn R_InitOutPStream(
    stream: R_outpstream_t,
    data: R_pstream_data_t,
    type_: R_pstream_format_t,
    version: c_int,
    outchar: Option<unsafe extern "C" fn(R_outpstream_t, c_int)>,
    outbytes: Option<unsafe extern "C" fn(R_outpstream_t, *const c_void, c_int)>,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        if stream.is_null() {
            return;
        }
        let ver = if version != 0 {
            version
        } else {
            R_DEFAULT_SERIALIZE_VERSION
        };
        (*stream).data = data;
        (*stream).type_ = type_;
        (*stream).version = ver;
        (*stream).OutChar = outchar;
        (*stream).OutBytes = outbytes;
        (*stream).OutPersistHookFunc = phook;
        (*stream).OutPersistHookData = pdata;
    }
}

pub unsafe fn R_InitFileOutPStream(
    stream: R_outpstream_t,
    fp: *mut c_void,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitOutPStream(
            stream,
            fp,
            type_,
            version,
            Some(OutCharFile),
            Some(OutBytesFile),
            phook,
            pdata,
        );
    }
}

pub unsafe fn R_InitFileInPStream(
    stream: R_inpstream_t,
    fp: *mut c_void,
    type_: R_pstream_format_t,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitInPStream(
            stream,
            fp,
            type_,
            Some(InCharFile),
            Some(InBytesFile),
            phook,
            pdata,
        );
    }
}

pub unsafe fn R_InitConnOutPStream(
    stream: R_outpstream_t,
    con: *mut c_void,
    type_: R_pstream_format_t,
    version: c_int,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitOutPStream(
            stream,
            con,
            type_,
            version,
            Some(OutCharFile),
            Some(OutBytesFile),
            phook,
            pdata,
        );
    }
}

pub unsafe fn R_InitConnInPStream(
    stream: R_inpstream_t,
    con: *mut c_void,
    type_: R_pstream_format_t,
    phook: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    pdata: SEXP,
) {
    unsafe {
        R_InitInPStream(
            stream,
            con,
            type_,
            Some(InCharFile),
            Some(InBytesFile),
            phook,
            pdata,
        );
    }
}

// ---------------------------------------------------------------------------
// R_serialize / R_unserialize (R-level entry points, memory-based)
// ---------------------------------------------------------------------------

/// Serialize an R object to a raw vector (when icon is R_NilValue).
/// This is the main entry point for `serialize()` in R.
pub unsafe fn R_serialize(
    object: SEXP,
    icon: SEXP,
    ascii: SEXP,
    Sversion: SEXP,
    fun: SEXP,
) -> SEXP {
    unsafe { R_serialize_with_xdr(object, icon, ascii, R_NilValue(), Sversion, fun) }
}

pub unsafe fn R_serialize_with_xdr(
    object: SEXP,
    icon: SEXP,
    ascii: SEXP,
    xdr: SEXP,
    Sversion: SEXP,
    fun: SEXP,
) -> SEXP {
    unsafe {
        if object.is_null() {
            error("read error");
        }

        let version = if Sversion == R_NilValue() {
            defaultSerializeVersion()
        } else {
            let version = asInteger(Sversion);
            if version != 2 && version != 3 {
                error(&format!("version {version} not supported"));
            }
            version
        };

        let ascii_format = !ascii.is_null() && ascii != R_NilValue() && asLogical(ascii) != 0;
        let xdr_format =
            !ascii_format && (xdr.is_null() || xdr == R_NilValue() || asLogical(xdr) != 0);

        // Build the header
        let mut writer = BinaryWriter::new();
        writer.write_byte(if ascii_format {
            b'A'
        } else if xdr_format {
            b'X'
        } else {
            b'B'
        });
        writer.write_byte(b'\n');
        writer.set_ascii_body(ascii_format);
        writer.set_xdr_body(xdr_format);

        // Version info
        writer.write_i32(version); // version
        writer.write_i32(R_VERSION); // writer version
        writer.write_i32(R_VERSION_350); // min reader version
        if version == 3 {
            writer.write_i32(0); // native encoding length
        }

        // Serialize the object
        let mut ref_table = WriteHashTable::new();
        WriteItemInternal(object, &mut ref_table, &mut writer);

        // Create RAWSXP from the serialized bytes
        let data = writer.into_vec();
        let raw = Rf_allocVector3(SEXPTYPE::RAWSXP, data.len() as R_xlen_t);
        if !raw.is_null() && !data.is_empty() {
            let raw_ptr = RAW(raw);
            ptr::copy_nonoverlapping(data.as_ptr(), raw_ptr, data.len());
        }
        raw
    }
}

/// Unserialize an R object from a raw vector.
pub unsafe fn R_unserialize(icon: SEXP, fun: SEXP) -> SEXP {
    unsafe {
        if icon.is_null() {
            error("read error");
        }

        // Must be RAWSXP
        let stype = TYPEOF(icon);
        if stype != SEXPTYPE::RAWSXP {
            error("not a proper raw vector");
        }

        let len = XLENGTH(icon) as usize;
        if len == 0 {
            error("read error");
        }

        let raw_ptr = RAW(icon);
        let data = slice::from_raw_parts(raw_ptr, len);

        let mut reader = BinaryReader::new(data);

        // Read format header: two bytes (`A\n`, `B\n`, or `X\n`).
        let fmt1 = reader.read_byte().unwrap_or(0);
        let fmt2 = reader.read_byte().unwrap_or(0);
        if (fmt1 != b'A' && fmt1 != b'B' && fmt1 != b'X') || fmt2 != b'\n' {
            error("unknown input format");
        }
        reader.set_ascii_body(fmt1 == b'A');
        reader.set_xdr_body(fmt1 == b'X');

        // Read version
        let version = reader.read_i32().unwrap_or(0);
        if version != 2 && version != 3 {
            error("version not supported");
        }

        // Read writer_version and min_reader_version
        let _writer_version = reader.read_i32().unwrap_or(0);
        let _min_reader_version = reader.read_i32().unwrap_or(0);

        // Read native encoding length (version 3)
        if version == 3 {
            let nelen = reader.read_i32().unwrap_or(0);
            if nelen > 0 {
                // Skip encoding bytes
                let _ = reader.read_bytes(nelen as usize);
            }
        }

        // Read the object
        let mut ref_table = ReadRefTable::new();
        match ReadItemInternal(&mut reader, &mut ref_table) {
            Ok(s) => s,
            Err(_) => error("read error"),
        }
    }
}

// ---------------------------------------------------------------------------
// R_serializeb
// ---------------------------------------------------------------------------

pub unsafe fn R_serializeb(object: SEXP, icon: SEXP, xdr: SEXP, Sversion: SEXP, fun: SEXP) -> SEXP {
    unsafe { R_serialize_with_xdr(object, icon, R_NilValue(), xdr, Sversion, fun) }
}

// ---------------------------------------------------------------------------
// do_serialize (dispatch for serialize/unserialize builtins)
// ---------------------------------------------------------------------------

pub unsafe fn do_serialize(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            error("argument \"connection\" is missing, with no default");
        }

        // serialize(object, connection, ascii, xdr, version, refhook)
        let object = arg_by_name_or_position(args, "object", 0);
        let conn = arg_by_name_or_position(args, "connection", 1);
        if object.is_null() || object == R_NilValue() {
            error("argument \"connection\" is missing, with no default");
        }
        let has_conn = arg_present_by_name_or_position(args, "connection", 1);
        if !has_conn || conn.is_null() || conn == R_MissingArg() {
            error("argument \"connection\" is missing, with no default");
        }
        let mut ascii = arg_by_name_or_position(args, "ascii", 2);
        let mut version = arg_by_name_or_position(args, "version", 4);
        if version == R_NilValue()
            && !ascii.is_null()
            && ascii != R_NilValue()
            && TYPEOF(ascii) != SEXPTYPE::LGLSXP
            && scalar_integer_value(ascii).is_some()
            && optional_arg(args, 3) == R_NilValue()
        {
            version = ascii;
            ascii = R_NilValue();
        }

        let xdr = arg_by_name_or_position(args, "xdr", 3);
        R_serialize_with_xdr(object, R_NilValue(), ascii, xdr, version, R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// do_serializeToConn
// ---------------------------------------------------------------------------

pub unsafe fn do_serializeToConn(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            error("wrong number of arguments");
        }

        // serializeToConn(object, connection, ascii)
        let object = CAR(args);
        let _conn = CADR(args);

        // Serialize to a raw vector, then the connection layer would write it
        //  do the serialization
        R_serialize_with_xdr(
            object,
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
        )
    }
}

// ---------------------------------------------------------------------------
// do_unserializeFromConn
// ---------------------------------------------------------------------------

pub unsafe fn do_unserializeFromConn(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        if args.is_null() || args == R_NilValue() {
            error("wrong number of arguments");
        }

        // unserializeFromConn(connection, hook)
        let conn = CAR(args);
        // If the first argument is a raw vector, use R_unserialize directly
        if !conn.is_null() && TYPEOF(conn) == SEXPTYPE::RAWSXP {
            return R_unserialize(conn, R_NilValue());
        }

        error("'connection' must be a connection");
    }
}

// ---------------------------------------------------------------------------
// Lazy-load database functions
// ---------------------------------------------------------------------------

pub unsafe fn do_lazyLoadDBflush(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        let file = require_arg(args, 0);
        let _ = sexp_to_path(file);
        clear_lazy_load_cache();
        R_NilValue()
    }
}

pub unsafe fn do_lazyLoadDBfetch(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        let key = require_arg(args, 0);
        let file = require_arg(args, 1);
        let compsxp = require_arg(args, 2);
        let hook = require_arg(args, 3);
        let compressed = asInteger(compsxp);

        let mut err: Rboolean = 0;
        let mut raw = readRawFromFile(file, key);
        let mut raw_guard = protect(raw);
        if compressed == 3 {
            let next = R_decompress3(raw, &mut err);
            raw = next;
            raw_guard = protect(raw);
        } else if compressed == 2 {
            let next = R_decompress2(raw, &mut err);
            raw = next;
            raw_guard = protect(raw);
        } else if compressed != 0 {
            let next = R_decompress1(raw, &mut err);
            raw = next;
            raw_guard = protect(raw);
        }

        if err != 0 {
            let file_name = sexp_to_path(file);
            error(&format!(
                "lazy-load database '{}' is corrupt",
                file_name.display()
            ));
        }

        let mut val = R_unserialize(raw, hook);
        let mut val_guard = protect(val);
        if TYPEOF(val) == SEXPTYPE::PROMSXP {
            val = Rf_eval(val, R_GlobalEnv());
            val_guard = protect(val);
            if !val.is_null() {
                SET_NAMED(val, 2);
            }
        }
        let _ = (raw_guard, val_guard);
        val
    }
}

pub unsafe fn do_getVarsFromFrame(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        let vars = require_arg(args, 0);
        let env = require_arg(args, 1);
        let forcesxp = require_arg(args, 2);
        R_getVarsFromFrame(vars, env, forcesxp)
    }
}

pub unsafe fn do_lazyLoadDBinsertValue(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, env);
        let value = require_arg(args, 0);
        let file = require_arg(args, 1);
        let ascii = require_arg(args, 2);
        let compsxp = require_arg(args, 3);
        let hook = require_arg(args, 4);
        R_lazyLoadDBinsertValue(value, file, ascii, compsxp, hook)
    }
}
