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

        // Write the object using a temporary buffer, then stream it out.
        // GNU R_Serialize calls stream->OutPersistHookFunc for eligible
        // reference objects even when hook data is not an R function.
        // Keep the R-function path when only hook data is a closure.
        let mut writer = BinaryWriter::new();
        let hook_data = out_ref.OutPersistHookData;
        if let Some(func) = out_ref.OutPersistHookFunc {
            writer.set_c_persist_hook(Some(func), hook_data);
        } else if persist_hook_is_r_function(hook_data) {
            writer.set_persist_hook(hook_data);
        }
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
        R_unserialize_from_stream_hooks(
            raw,
            (*stream).InPersistHookFunc,
            (*stream).InPersistHookData,
            None,
        )
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

        let _object_guard = protect(object);
        let _hook_guard = protect(fun);
        let version = if Sversion == R_NilValue() {
            defaultSerializeVersion()
        } else {
            let version = asInteger(Sversion);
            if version != 2 && version != 3 {
                error(&format!("version {version} not supported"));
            }
            version
        };

        let ascii_value = if ascii.is_null() || ascii == R_NilValue() {
            0
        } else {
            asLogical(ascii)
        };
        let ascii_format = ascii_value != 0;
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
        writer.set_ascii_hex(ascii_value == crate::sexp::ffi::NA_LOGICAL);
        writer.set_xdr_body(xdr_format);
        writer.set_persist_hook(fun);

        // Version info
        writer.write_i32(version); // version
        writer.write_i32(R_VERSION); // writer version
        writer.write_i32(if version == 2 {
            R_VERSION_230
        } else {
            R_VERSION_350
        });
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
        if !fun.is_null() && fun != R_NilValue() && TYPEOF(fun) == SEXPTYPE::ENVSXP {
            let owner =
                crate::sexp::owner::OwnerToken::current().unwrap_or_else(|e| error(&e.to_string()));
            let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
            owner
                .require_active()
                .unwrap_or_else(|e| error(&e.to_string()));
            let data = owner
                .sexp(fun)
                .and_then(|s| s.into_owned())
                .unwrap_or_else(|e| error(&e.to_string()));
            R_unserialize_from_stream_hooks(
                icon,
                None,
                R_NilValue(),
                Some(LazyLoadRestore { data }),
            )
        } else {
            R_unserialize_from_stream_hooks(icon, None, fun, None)
        }
    }
}

/// The runtime's lazy-load hook is Rust control flow with an actual owning
/// environment. External C stream callbacks keep their separate ABI contract.
#[derive(Clone)]
pub(super) struct LazyLoadRestore {
    data: crate::sexp::object::Sexp<'static>,
}

impl LazyLoadRestore {
    pub(super) fn restore(&self, names: SEXP) -> Result<SEXP, String> {
        use crate::sexp::{
            object::SexpError,
            owner::{OwnerToken, StoredOwner},
        };
        let authority = StoredOwner::from_value(&self.data).map_err(|e| e.to_string())?;
        authority
            .with_projection(|pointer| {
                // This original operation pin retains cleanup storage through unwind.
                let owner = unsafe { OwnerToken::from_raw(pointer) };
                owner.require_active()?;
                if crate::sexp::memory::is_arena_lent(pointer) {
                    return Err(SexpError::OwnerNotActive);
                }
                let names = owner.sexp(names)?;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    crate::sexp::session::with_instance_active(pointer, || {
                        let raw = persist_restore_inner(names.as_raw(), self.data.as_raw(), owner);
                        owner.require_active()?;
                        owner.sexp(raw)?.into_owned()
                    })
                }));
                // Revoked authority cannot publish a value or a callback panic.
                owner.require_active()?;
                match result {
                    Ok(value) => value.map(|value| value.as_raw()),
                    Err(payload) => std::panic::resume_unwind(payload),
                }
            })
            .map_err(|e| e.to_string())
    }
}

/// A failed reconstruction must not leave its provisional environment in the
/// shared reference cache. Cleanup changes only this original, exact binding;
/// it neither looks up an ambient owner nor invokes active binding callbacks.
struct PendingPersistEnvironment<'s> {
    cache: crate::sexp::object::Sexp<'s>,
    symbol: crate::sexp::object::Sexp<'s>,
    environment: crate::sexp::object::Sexp<'s>,
    owner: crate::sexp::owner::OwnerToken<'s>,
    _pin: Option<crate::sexp::owner::OwnerPin>,
    committed: bool,
}
impl Drop for PendingPersistEnvironment<'_> {
    fn drop(&mut self) {
        use crate::sexp::{
            ffi::{EdgeField, NodeBody},
            heap::ResolvedLink,
        };
        if self.committed {
            return;
        }
        let Ok(cache) = self.cache.allocation() else {
            return;
        };
        let heap = cache.heap_identity();
        let Some(symbol) = self.symbol.allocation().ok().and_then(|n| n.link()) else {
            return;
        };
        let Some(environment) = self.environment.allocation().ok().and_then(|n| n.link()) else {
            return;
        };
        let Some(header) = heap.node_snapshot(cache) else {
            return;
        };
        let NodeBody::Environment(body) = header.data else {
            return;
        };
        let mut current = body.frame;
        let mut previous = None;
        let mut seen = std::collections::HashSet::new();
        while seen.insert(current) {
            let Some(ResolvedLink::Node { allocation, .. }) = heap.resolve_link(current) else {
                return;
            };
            let Some(header) = heap.node_snapshot(&allocation) else {
                return;
            };
            let NodeBody::List(cell) = header.data else {
                return;
            };
            if cell.tagval == symbol && cell.carval == environment {
                let (parent, field) = previous
                    .map_or((cache.clone(), EdgeField::EnvironmentFrame), |p| {
                        (p, EdgeField::ListCdr)
                    });
                let Some(mut header) = heap.node_snapshot(&parent) else {
                    return;
                };
                let Some(next) = heap.projection_of_link(cell.cdrval) else {
                    return;
                };
                let Some(parent_pointer) = heap.node_projection(&parent) else {
                    return;
                };
                // Both canonical nodes and physical runtime are retained; this
                // barrier is callback-free and remains valid after revocation.
                if !unsafe {
                    crate::sexp::gengc::write_barrier_in(self.owner.as_ptr(), parent_pointer, next)
                } {
                    return;
                }
                if header.set_edge(field, cell.cdrval).is_some() {
                    heap.replace_node(&parent, header);
                }
                return;
            }
            previous = Some(allocation);
            current = cell.cdrval;
        }
    }
}

unsafe fn persist_restore_inner(
    names: SEXP,
    data: SEXP,
    owner: crate::sexp::owner::OwnerToken<'_>,
) -> SEXP {
    unsafe {
        if data.is_null() || TYPEOF(data) != SEXPTYPE::ENVSXP {
            return R_NilValue();
        }
        let name = if TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) > 0 {
            let raw = CHAR(STRING_ELT(names, 0));
            if raw.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned()
            }
        } else {
            String::new()
        };
        if name.is_empty() {
            return R_NilValue();
        }
        let Ok(cname) = std::ffi::CString::new(name.clone()) else {
            return R_NilValue();
        };
        let name_sym = Rf_install(cname.as_ptr());

        let cache_sym = Rf_install(c"cache".as_ptr());
        let cache = R_findVarInFrame(data, cache_sym);
        if !cache.is_null() && cache != R_UnboundValue() && TYPEOF(cache) == SEXPTYPE::ENVSXP {
            let hit = R_findVarInFrame(cache, name_sym);
            if !hit.is_null() && hit != R_UnboundValue() {
                return hit;
            }
        }
        // GNU package/namespace persist: reuse the live namespace so
        // lazy-loaded closures keep `standardGeneric` and other ns bindings.
        let package = name.strip_prefix("package:").unwrap_or(name.as_str());
        if let Some(live) = crate::mainutils::essentials::cached_namespace_by_name(package) {
            if !cache.is_null() && cache != R_UnboundValue() && TYPEOF(cache) == SEXPTYPE::ENVSXP {
                crate::sexp::envir::defineVar(name_sym, live, cache);
            }
            return live;
        }

        let factory = owner.node_factory();
        let environment = factory
            .wrap(crate::sexp::memory_ext::NewEnvironment(
                R_NilValue(),
                R_EmptyEnv(),
                R_NilValue(),
            ))
            .unwrap_or_else(|e| error(&e.to_string()));
        let env_raw = environment.as_raw();
        let mut pending =
            if !cache.is_null() && cache != R_UnboundValue() && TYPEOF(cache) == SEXPTYPE::ENVSXP {
                let cache = factory
                    .wrap(cache)
                    .unwrap_or_else(|e| error(&e.to_string()));
                let symbol = factory
                    .wrap(name_sym)
                    .unwrap_or_else(|e| error(&e.to_string()));
                Some(PendingPersistEnvironment {
                    cache,
                    symbol,
                    environment: environment.clone(),
                    owner,
                    _pin: owner.pin().unwrap_or_else(|e| error(&e.to_string())),
                    committed: false,
                })
            } else {
                None
            };
        if let Some(pending) = &pending {
            crate::sexp::envir::define_var_safe(
                pending.symbol.clone(),
                environment.clone(),
                pending.cache.clone(),
            );
        }
        let env = env_raw;
        let refs_sym = Rf_install(c"refs".as_ptr());
        let refs = R_findVarInFrame(data, refs_sym);
        let mut key = R_NilValue();
        if !refs.is_null() && refs != R_UnboundValue() {
            if TYPEOF(refs) == SEXPTYPE::ENVSXP {
                key = R_findVarInFrame(refs, name_sym);
            } else if TYPEOF(refs) == SEXPTYPE::VECSXP {
                let nm = crate::sexp::attrib_core::getAttrib(
                    refs,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                if TYPEOF(nm) == SEXPTYPE::STRSXP {
                    for i in 0..XLENGTH(nm) {
                        let raw = CHAR(STRING_ELT(nm, i));
                        if !raw.is_null() && std::ffi::CStr::from_ptr(raw).to_string_lossy() == name
                        {
                            key = VECTOR_ELT(refs, i);
                            break;
                        }
                    }
                }
            }
        }
        if key.is_null() || key == R_UnboundValue() || key == R_NilValue() {
            error(&format!("lazy-load reference '{name}' has no payload"));
        }
        if TYPEOF(key) == SEXPTYPE::VECSXP {
            let kn =
                crate::sexp::attrib_core::getAttrib(key, crate::sexp::attrib_core::R_NamesSymbol());
            if TYPEOF(kn) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(kn) {
                    let raw = CHAR(STRING_ELT(kn, i));
                    if !raw.is_null()
                        && std::ffi::CStr::from_ptr(raw).to_string_lossy() == "eagerKey"
                    {
                        key = VECTOR_ELT(key, i);
                        break;
                    }
                }
            }
        }
        let datafile = R_findVarInFrame(data, Rf_install(c"datafile".as_ptr()));
        let compressed = R_findVarInFrame(data, Rf_install(c"compressed".as_ptr()));
        if datafile.is_null()
            || datafile == R_UnboundValue()
            || compressed.is_null()
            || compressed == R_UnboundValue()
        {
            error("lazy-load reference lacks its database path or compression mode");
        }
        let fields = [key, datafile, compressed, data]
            .map(|raw| factory.wrap(raw).unwrap_or_else(|e| error(&e.to_string())));
        let mut args = factory.nil();
        for field in fields.iter().rev() {
            args = factory
                .pairlist_cell(field, &args, &factory.nil())
                .unwrap_or_else(|e| error(&e.to_string()));
        }
        let fetched = factory
            .wrap(do_lazyLoadDBfetch(
                R_NilValue(),
                R_NilValue(),
                args.as_raw(),
                R_NilValue(),
            ))
            .unwrap_or_else(|e| error(&e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let fetched_raw = fetched.as_raw();
        if TYPEOF(fetched_raw) != SEXPTYPE::VECSXP {
            error("invalid lazy-load environment payload");
        }
        let fetched = fetched_raw;
        if TYPEOF(fetched) == SEXPTYPE::VECSXP {
            let fnames = crate::sexp::attrib_core::getAttrib(
                fetched,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if TYPEOF(fnames) != SEXPTYPE::STRSXP {
                error("lazy-load environment payload has no field names");
            }
            let mut fields = Vec::new();
            for i in 0..XLENGTH(fnames) {
                let label = factory
                    .wrap(STRING_ELT(fnames, i))
                    .unwrap_or_else(|e| error(&e.to_string()))
                    .as_string()
                    .unwrap_or_default();
                let value = factory
                    .wrap(VECTOR_ELT(fetched, i))
                    .unwrap_or_else(|e| error(&e.to_string()));
                fields.push((label, value));
            }
            {
                for (label, value) in fields {
                    let elt = value.as_raw();
                    match label.as_str() {
                        "enclos" => {
                            if TYPEOF(elt) != SEXPTYPE::ENVSXP {
                                error("invalid lazy-load environment enclosure");
                            }
                            crate::sexp::accessors::SET_ENCLOS(env, elt);
                        }
                        "bindings" => {
                            if TYPEOF(elt) != SEXPTYPE::VECSXP {
                                error("invalid lazy-load environment bindings");
                            }
                            let tail = factory
                                .pairlist_cell(&environment, &factory.nil(), &factory.nil())
                                .unwrap_or_else(|e| error(&e.to_string()));
                            let call = factory
                                .pairlist_cell(&value, &tail, &factory.nil())
                                .unwrap_or_else(|e| error(&e.to_string()));
                            crate::mainutils::essentials::do_list2env(
                                R_NilValue(),
                                R_NilValue(),
                                call.as_raw(),
                                R_NilValue(),
                            );
                        }
                        "attributes" if elt != R_NilValue() => {
                            crate::sexp::accessors::SET_ATTRIB(env, elt);
                        }
                        "locked"
                            if TYPEOF(elt) == SEXPTYPE::LGLSXP
                                && XLENGTH(elt) > 0
                                && value.try_logical_elt(0).unwrap_or(0) != 0 =>
                        {
                            crate::sexp::envir::lock_environment_raw(env);
                        }
                        _ => {}
                    }
                    owner
                        .require_active()
                        .unwrap_or_else(|e| error(&e.to_string()));
                }
            }
        }
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        if let Some(pending) = pending.as_mut() {
            pending.committed = true;
        }
        env
    }
}

unsafe fn persist_hook_is_r_function(hook: SEXP) -> bool {
    unsafe {
        !hook.is_null()
            && hook != R_NilValue()
            && (TYPEOF(hook) == SEXPTYPE::CLOSXP
                || TYPEOF(hook) == SEXPTYPE::BUILTINSXP
                || TYPEOF(hook) == SEXPTYPE::SPECIALSXP)
    }
}

unsafe fn R_unserialize_from_stream_hooks(
    icon: SEXP,
    hook_func: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    hook_data: SEXP,
    lazy_restore: Option<LazyLoadRestore>,
) -> SEXP {
    unsafe {
        if icon.is_null() {
            error("read error");
        }

        let owner =
            crate::sexp::owner::OwnerToken::current().unwrap_or_else(|e| error(&e.to_string()));
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let input = owner.sexp(icon).unwrap_or_else(|e| error(&e.to_string()));
        let _hook = if hook_data.is_null() {
            None
        } else {
            Some(
                owner
                    .sexp(hook_data)
                    .unwrap_or_else(|e| error(&e.to_string())),
            )
        };
        // Must be RAWSXP
        let stype = TYPEOF(icon);
        if stype != SEXPTYPE::RAWSXP {
            error("not a proper raw vector");
        }

        if ALTREP(input.as_raw()) != 0 {
            RAW(input.as_raw());
        }
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        let header = input.header();
        let len = usize::try_from(input.len()).unwrap_or_else(|_| error("read error"));
        if len == 0 {
            error("read error");
        }
        let lease = header
            .payload_lease()
            .unwrap_or_else(|| error("raw input has no initialized payload"));
        // Copy canonical cells before any restoration callback. The reader
        // cannot alias an R payload that reentrant code may resize or mutate.
        let data: Vec<u8> = (0..len)
            .map(|i| {
                lease
                    .byte_elt(i)
                    .unwrap_or_else(|| error("invalid raw input payload"))
            })
            .collect();
        let data = data.as_slice();

        // GNU serialize.c: gzip-compressed RDS starts with 1f 8b.
        let decompressed;
        let payload: &[u8] = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
            let mut decoder = GzDecoder::new(data);
            let mut out = Vec::new();
            if decoder.read_to_end(&mut out).is_err() || out.is_empty() {
                error("read error");
            }
            decompressed = out;
            &decompressed
        } else {
            data
        };

        let mut reader = BinaryReader::new(payload);
        reader.lazy_restore = lazy_restore;
        if hook_func.is_some() {
            reader.set_c_persist_hook(hook_func, hook_data);
        } else {
            reader.set_persist_hook(hook_data);
        }

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
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ReadItemInternal(&mut reader, &mut ref_table)
        }));
        owner
            .require_active()
            .unwrap_or_else(|e| error(&e.to_string()));
        match outcome {
            Ok(Ok(s)) => owner
                .sexp(s)
                .unwrap_or_else(|e| error(&e.to_string()))
                .as_raw(),
            Ok(Err(message)) => error(&message),
            Err(payload) => std::panic::resume_unwind(payload),
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
        let has_object = arg_present_by_name_or_position(args, "object", 0);
        if !has_object || object.is_null() || object == R_MissingArg() {
            error("argument \"object\" is missing, with no default");
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
        let hook = arg_by_name_or_position(args, "refhook", 5);
        let raw = R_serialize_with_xdr(object, R_NilValue(), ascii, xdr, version, hook);
        if TYPEOF(conn) == SEXPTYPE::INTSXP {
            let n = XLENGTH(raw) as usize;
            let bytes = std::slice::from_raw_parts(RAW(raw), n);
            crate::mainutils::connections::connection_write_bytes(*INTEGER(conn), bytes);
            return R_NilValue();
        }
        raw
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

        let object = CAR(args);
        let conn = CADR(args);
        let raw = R_serialize_with_xdr(
            object,
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
        );
        if conn.is_null() || conn == R_NilValue() || TYPEOF(conn) != SEXPTYPE::INTSXP {
            return raw;
        }
        let index = *INTEGER(conn) as usize;
        let guard = crate::mainutils::connections::get_connection(index);
        let path = guard[index].as_ref().unwrap().description.clone();
        let n = XLENGTH(raw) as usize;
        let bytes = std::slice::from_raw_parts(RAW(raw), n);
        if std::fs::write(&path, bytes).is_err() {
            error("cannot write to connection");
        }
        R_NilValue()
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

        let conn = CAR(args);
        let hook = arg_by_name_or_position(args, "refhook", 1);
        if !conn.is_null() && TYPEOF(conn) == SEXPTYPE::RAWSXP {
            return R_unserialize(conn, hook);
        }
        if !conn.is_null()
            && TYPEOF(conn) == SEXPTYPE::INTSXP
            && XLENGTH(conn) == 1
            && crate::mainutils::objects::inherits2(conn, c"connection".as_ptr()) != 0
        {
            let bytes = crate::mainutils::connections::connection_read_all(*INTEGER(conn));
            let raw = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as i64);
            if !bytes.is_empty() {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw), bytes.len());
            }
            return R_unserialize(raw, hook);
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
        // Portable pinned data uses the same original lazy-load stream, while
        // retaining all selected operands through provider and reader callbacks.
        // Inspection of ordinary file vectors invokes no provider accessor.
        let owner =
            crate::sexp::owner::OwnerToken::current().unwrap_or_else(|e| error(&e.to_string()));
        let file_owned = owner
            .sexp(file)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|e| error(&e.to_string()));
        if let Some(image) = crate::library::portable_package::database(&file_owned)
            .unwrap_or_else(|failure| error(&failure.to_string()))
        {
            let own = |value| {
                owner
                    .sexp(value)
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .unwrap_or_else(|failure| error(&failure.to_string()))
            };
            return image
                .fetch(own(key), file_owned, own(compsxp), own(hook))
                .unwrap_or_else(|message| error(&message))
                .as_raw();
        }
        if crate::library::datasets::is_database(&file_owned)
            .unwrap_or_else(|e| error(&e.to_string()))
        {
            let own = |value| {
                owner
                    .sexp(value)
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .unwrap_or_else(|e| error(&e.to_string()))
            };
            return crate::library::datasets::fetch(own(key), file_owned, own(compsxp), own(hook))
                .unwrap_or_else(|message| error(&message))
                .as_raw();
        }
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

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod persistence_tests;
