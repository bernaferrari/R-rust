use super::*;

// ---------------------------------------------------------------------------
// Basic output routines (C stream-based, module-private)
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutInteger(stream: R_outpstream_t, i: c_int) {
    // SAFETY: `stream` is a caller-owned output stream; the null check
    // precedes the field read and the callback only reads `bytes`.
    unsafe {
        if stream.is_null() {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            let bytes = i.to_ne_bytes();
            out_bytes(stream, bytes.as_ptr() as *const c_void, 4);
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutReal(stream: R_outpstream_t, d: c_double) {
    // SAFETY: `stream` is a caller-owned output stream; the null check
    // precedes the field read and the callback only reads `bytes`.
    unsafe {
        if stream.is_null() {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            let bytes = d.to_ne_bytes();
            out_bytes(stream, bytes.as_ptr() as *const c_void, 8);
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutComplex(stream: R_outpstream_t, c: Rcomplex) {
    {
        // SAFETY: forwarded raw pointers satisfy `OutReal`'s contract per this function's own `# Safety` requirements.
        unsafe { OutReal(stream, c.r) };
        // SAFETY: forwarded raw pointers satisfy `OutReal`'s contract per this function's own `# Safety` requirements.
        unsafe { OutReal(stream, c.i) };
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutByte(stream: R_outpstream_t, i: Rbyte) {
    // SAFETY: `stream` is a caller-owned output stream; the null check
    // precedes the field read and the callback only reads `i`.
    unsafe {
        if stream.is_null() {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            out_bytes(stream, &i as *const Rbyte as *const c_void, 1);
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutString(stream: R_outpstream_t, s: *const c_char, length: c_int) {
    // SAFETY: `stream` is a caller-owned output stream and `s` a
    // caller-owned buffer; null/length checks precede the callback.
    unsafe {
        if stream.is_null() || s.is_null() || length <= 0 {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            out_bytes(stream, s as *const c_void, length);
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutFormat(stream: R_outpstream_t) {
    // SAFETY: `stream` is a caller-owned output stream; the null check
    // precedes field reads and each callback sees a static tag slice.
    unsafe {
        if stream.is_null() {
            return;
        }
        let out_ref = &mut *stream;
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
    }
}

// ---------------------------------------------------------------------------
// Basic input routines (C stream-based, module-private)
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InInteger(stream: R_inpstream_t) -> c_int {
    // SAFETY: `stream` is a caller-owned input stream; the null check
    // precedes field reads and the callback fills a local buffer.
    unsafe {
        if stream.is_null() {
            return 0;
        }
        let mut bytes = [0u8; 4];
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, bytes.as_mut_ptr() as *mut c_void, 4);
        }
        if (*stream).type_ == R_pstream_format_t::R_pstream_xdr_format {
            c_int::from_be_bytes(bytes)
        } else {
            c_int::from_ne_bytes(bytes)
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InReal(stream: R_inpstream_t) -> c_double {
    // SAFETY: `stream` is a caller-owned input stream; the null check
    // precedes field reads and the callback fills a local buffer.
    unsafe {
        if stream.is_null() {
            return 0.0;
        }
        let mut bytes = [0u8; 8];
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, bytes.as_mut_ptr() as *mut c_void, 8);
        }
        if (*stream).type_ == R_pstream_format_t::R_pstream_xdr_format {
            c_double::from_be_bytes(bytes)
        } else {
            c_double::from_ne_bytes(bytes)
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InComplex(stream: R_inpstream_t) -> Rcomplex {
    {
        Rcomplex {
            // SAFETY: forwarded raw pointers satisfy `InReal`'s contract per this function's own `# Safety` requirements.
            r: unsafe { InReal(stream) },
            // SAFETY: forwarded raw pointers satisfy `InReal`'s contract per this function's own `# Safety` requirements.
            i: unsafe { InReal(stream) },
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InString(stream: R_inpstream_t, buf: *mut c_char, length: c_int) {
    // SAFETY: `stream` is a caller-owned input stream and `buf` a
    // caller-owned buffer of at least `length` bytes; checks precede
    // the callback write.
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, buf as *mut c_void, length);
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InWord(stream: R_inpstream_t, buf: *mut c_char, size: c_int) {
    {
        // Simplified: just read size bytes
        // SAFETY: forwarded raw pointers satisfy `InString`'s contract per this function's own `# Safety` requirements.
        unsafe { InString(stream, buf, size) };
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InFormat(stream: R_inpstream_t) {
    // SAFETY: `stream` is a caller-owned input stream; the null check
    // precedes all field reads/writes, and callbacks only fill local
    // buffers inside this call.
    unsafe {
        if stream.is_null() {
            return;
        }
        let mut buf = [0u8; 2];
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, buf.as_mut_ptr() as *mut c_void, 2);
        } else if let Some(in_char) = (*stream).InChar {
            for b in &mut buf {
                let ch = in_char(stream);
                if ch < 0 {
                    error("read error");
                }
                *b = ch as u8;
            }
        } else {
            error("read error");
        }

        let mut need_third = false;
        let detected = match buf[0] {
            b'A' => R_pstream_format_t::R_pstream_ascii_format,
            b'B' => R_pstream_format_t::R_pstream_binary_format,
            b'X' => R_pstream_format_t::R_pstream_xdr_format,
            b'\n' if buf[1] == b'A' => {
                need_third = true;
                R_pstream_format_t::R_pstream_ascii_format
            }
            _ => error("unknown input format"),
        };

        // Keep the stream position consistent with the C newline hack.
        if need_third {
            let mut one = 0u8;
            if let Some(in_bytes) = (*stream).InBytes {
                in_bytes(stream, &mut one as *mut u8 as *mut c_void, 1);
            } else if let Some(in_char) = (*stream).InChar {
                if in_char(stream) < 0 {
                    error("read error");
                }
            } else {
                error("read error");
            }
        }

        if (*stream).type_ == R_pstream_format_t::R_pstream_any_format {
            (*stream).type_ = detected;
        } else if (*stream).type_ != detected {
            error("input format does not match specified format");
        }
    }
}

// ---------------------------------------------------------------------------
// Hash table for write reference tracking (C SEXP-based, for C API)
// ---------------------------------------------------------------------------

pub fn MakeHashTable() -> SEXP {
    let vec = Rf_allocVector3(SEXPTYPE::VECSXP, HASHSIZE as R_xlen_t);
    Rf_cons(Rf_ScalarInteger(0), vec)
}

pub fn HashAdd(obj: SEXP, ht: SEXP) {
    if ht.is_null() || TYPEOF(ht) != SEXPTYPE::LISTSXP {
        return;
    }
    let buckets = CDR(ht);
    if buckets.is_null() || TYPEOF(buckets) != SEXPTYPE::VECSXP {
        return;
    }
    let bucket_count = XLENGTH(buckets) as usize;
    if bucket_count == 0 {
        return;
    }
    let key = obj as usize;
    let pos = ((key >> 2) % bucket_count) as R_xlen_t;
    let current_count = asInteger(CAR(ht));
    let next_count = current_count.saturating_add(1);
    SETCAR(ht, Rf_ScalarInteger(next_count));

    let entry = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    SET_VECTOR_ELT(entry, 0, obj);
    SET_VECTOR_ELT(entry, 1, Rf_ScalarInteger(next_count));
    let bucket = VECTOR_ELT(buckets, pos);
    let new_bucket = Rf_cons(entry, bucket);
    SET_VECTOR_ELT(buckets, pos, new_bucket);
}

pub fn HashGet(item: SEXP, ht: SEXP) -> c_int {
    if ht.is_null() || TYPEOF(ht) != SEXPTYPE::LISTSXP {
        return 0;
    }
    let buckets = CDR(ht);
    if buckets.is_null() || TYPEOF(buckets) != SEXPTYPE::VECSXP {
        return 0;
    }
    let bucket_count = XLENGTH(buckets) as usize;
    if bucket_count == 0 {
        return 0;
    }
    let key = item as usize;
    let pos = ((key >> 2) % bucket_count) as R_xlen_t;
    let mut node = VECTOR_ELT(buckets, pos);
    while !node.is_null() && TYPEOF(node) == SEXPTYPE::LISTSXP {
        let entry = CAR(node);
        if !entry.is_null() && TYPEOF(entry) == SEXPTYPE::VECSXP && XLENGTH(entry) >= 2 {
            let stored = VECTOR_ELT(entry, 0);
            if stored == item {
                return asInteger(VECTOR_ELT(entry, 1));
            }
        }
        node = CDR(node);
    }
    0
}

// ---------------------------------------------------------------------------
// Persistent name / special hooks
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn GetPersistentName(stream: R_outpstream_t, s: SEXP) -> SEXP {
    // SAFETY: `stream` is a caller-owned output stream; the persist hook
    // receives only its installed hook data and the checked argument.
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let Some(hook) = (*stream).OutPersistHookFunc else {
            return R_NilValue();
        };
        if !persistent_hook_eligible(s) {
            return R_NilValue();
        }
        let res = hook(s, (*stream).OutPersistHookData);
        if res.is_null() || res == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(res) != SEXPTYPE::STRSXP || XLENGTH(res) == 0 {
            error("persistent hook must return a nonempty character vector");
        }
        res
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn PersistentRestore(stream: R_inpstream_t, s: SEXP) -> SEXP {
    // SAFETY: `stream` is a caller-owned input stream; the persist hook
    // receives only its installed hook data and the checked argument.
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let Some(hook) = (*stream).InPersistHookFunc else {
            error("no restore method available");
        };
        hook(s, (*stream).InPersistHookData)
    }
}

pub fn SaveSpecialHookItem(item: SEXP) -> c_int {
    SaveSpecialHook(item)
}

// ---------------------------------------------------------------------------
// Length writing
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn WriteLENGTH(stream: R_outpstream_t, s: SEXP) {
    {
        // SAFETY: forwarded raw pointers satisfy `OutInteger`'s contract per this function's own `# Safety` requirements.
        unsafe { OutInteger(stream, LENGTH(s)) };
    }
}

// ---------------------------------------------------------------------------
// Vector serialization (C stream-based, module-private)
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutStringVec(stream: R_outpstream_t, s: SEXP, ref_table: SEXP) {
    let len = XLENGTH(s);
    // SAFETY: forwarded raw pointers satisfy `OutInteger`'s contract per this function's own `# Safety` requirements.
    unsafe { OutInteger(stream, 0) };
    // SAFETY: forwarded raw pointers satisfy `WriteLENGTH`'s contract per this function's own `# Safety` requirements.
    unsafe { WriteLENGTH(stream, s) };
    for i in 0..len {
        let elt = STRING_ELT(s, i as R_xlen_t);
        // SAFETY: forwarded raw pointers satisfy `R_WriteItem`'s contract per this function's own `# Safety` requirements.
        unsafe { R_WriteItem(elt, stream) };
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InStringVec(stream: R_inpstream_t, ref_table: SEXP) -> SEXP {
    {
        // SAFETY: forwarded raw pointers satisfy `InInteger`'s contract per this function's own `# Safety` requirements.
        if unsafe { InInteger(stream) } != 0 {
            error("names in persistent strings are not supported yet");
        }
        // SAFETY: forwarded raw pointers satisfy `InInteger`'s contract per this function's own `# Safety` requirements.
        let len = unsafe { InInteger(stream) };
        if len < 0 {
            error("read error");
        }
        let s = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
        let _s_guard = protect(s);
        increment_read_item_depth();

        let local_ref_table = if ref_table.is_null() {
            MakeReadRefTable()
        } else {
            ref_table
        };

        for i in 0..len {
            // SAFETY: forwarded raw pointers satisfy `InInteger`'s contract per this function's own `# Safety` requirements.
            let flags = unsafe { InInteger(stream) };
            let mut stype = 0;
            UnpackFlags(flags, Some(&mut stype), None, None, None, None);

            let elt = if stype == REFSXP {
                let idx = if (flags >> 8) == 0 {
                    // SAFETY: forwarded raw pointers satisfy `InInteger`'s contract per this function's own `# Safety` requirements.
                    unsafe { InInteger(stream) }
                } else {
                    flags >> 8
                };
                GetReadRef(local_ref_table, idx)
            } else {
                let charsxp_type: c_int = SEXPTYPE::CHARSXP.into();
                if stype != charsxp_type {
                    error("read error");
                }
                // SAFETY: forwarded raw pointers satisfy `InInteger`'s contract per this function's own `# Safety` requirements.
                let slen = unsafe { InInteger(stream) };
                let val = if slen < 0 {
                    R_NaString()
                } else if slen == 0 {
                    mk_chars(b"")
                } else {
                    let mut bytes = vec![0u8; slen as usize];
                    // SAFETY: forwarded raw pointers satisfy `InString`'s contract per this function's own `# Safety` requirements.
                    unsafe { InString(stream, bytes.as_mut_ptr() as *mut c_char, slen) };
                    mk_chars(&bytes)
                };
                AddReadRef(local_ref_table, val);
                val
            };
            SET_STRING_ELT(s, i as R_xlen_t, elt);
        }

        decrement_read_item_depth();
        s
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutIntegerVec(stream: R_outpstream_t, s: SEXP, length: R_xlen_t) {
    {
        for i in 0..length {
            // SAFETY: forwarded raw pointers satisfy `OutInteger`'s contract per this function's own `# Safety` requirements.
            unsafe { OutInteger(stream, INTEGER_ELT(s, i as c_int)) };
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InIntegerVec(stream: R_inpstream_t, obj: SEXP, length: R_xlen_t) {
    {
        for i in 0..length {
            // SAFETY: forwarded raw pointers satisfy `InInteger`'s contract per this function's own `# Safety` requirements.
            let value = unsafe { InInteger(stream) };
            SET_INTEGER_ELT(obj, i as c_int, value);
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutRealVec(stream: R_outpstream_t, s: SEXP, length: R_xlen_t) {
    {
        for i in 0..length {
            // SAFETY: forwarded raw pointers satisfy `OutReal`'s contract per this function's own `# Safety` requirements.
            unsafe { OutReal(stream, REAL_ELT(s, i as c_int)) };
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InRealVec(stream: R_inpstream_t, obj: SEXP, length: R_xlen_t) {
    {
        for i in 0..length {
            // SAFETY: forwarded raw pointers satisfy `InReal`'s contract per this function's own `# Safety` requirements.
            let value = unsafe { InReal(stream) };
            SET_REAL_ELT(obj, i as c_int, value);
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn OutComplexVec(stream: R_outpstream_t, s: SEXP, length: R_xlen_t) {
    {
        for i in 0..length {
            // SAFETY: forwarded raw pointers satisfy `OutComplex`'s contract per this function's own `# Safety` requirements.
            unsafe { OutComplex(stream, COMPLEX_ELT(s, i as c_int)) };
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn InComplexVec(stream: R_inpstream_t, obj: SEXP, length: R_xlen_t) {
    {
        for i in 0..length {
            // SAFETY: forwarded raw pointers satisfy `InComplex`'s contract per this function's own `# Safety` requirements.
            let value = unsafe { InComplex(stream) };
            SET_COMPLEX_ELT(obj, i as c_int, value);
        }
    }
}

// ---------------------------------------------------------------------------
// Bytecode serialization helpers
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn WriteBC(s: SEXP, ref_table: SEXP, stream: R_outpstream_t) {
    {
        let _ = ref_table;
        if stream.is_null() {
            return;
        }
        let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
        let len = XLENGTH(raw);
        if len < 0 || len > c_int::MAX as R_xlen_t {
            error("write failed");
        }
        // SAFETY: forwarded raw pointers satisfy `OutInteger`'s contract per this function's own `# Safety` requirements.
        unsafe { OutInteger(stream, len as c_int) };
        let bytes = raw_to_bytes(raw);
        // SAFETY: `stream` is the caller-owned output stream, checked for
        // null above; each callback only reads the owned `bytes` buffer.
        unsafe {
            if !bytes.is_empty() {
                if let Some(out_bytes) = (*stream).OutBytes {
                    out_bytes(stream, bytes.as_ptr() as *const c_void, len as c_int);
                } else if let Some(out_char) = (*stream).OutChar {
                    for byte in &bytes {
                        out_char(stream, *byte as c_int);
                    }
                } else {
                    error("write failed");
                }
            }
        }
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn ReadBC(ref_table: SEXP, stream: R_inpstream_t) -> SEXP {
    {
        let _ = ref_table;
        if stream.is_null() {
            error("read error");
        }
        // SAFETY: forwarded raw pointers satisfy `InInteger`'s contract per this function's own `# Safety` requirements.
        let len = unsafe { InInteger(stream) };
        if len < 0 {
            error("read error");
        }
        let raw = Rf_allocVector3(SEXPTYPE::RAWSXP, len as R_xlen_t);
        if len > 0 {
            // SAFETY: `stream` is the caller-owned input stream, checked for
            // null above; callbacks only fill the owned `bytes` buffer.
            let bytes = unsafe {
                if let Some(in_bytes) = (*stream).InBytes {
                    let mut bytes = vec![0u8; len as usize];
                    in_bytes(stream, bytes.as_mut_ptr() as *mut c_void, len);
                    bytes
                } else if let Some(in_char) = (*stream).InChar {
                    let mut bytes = Vec::with_capacity(len as usize);
                    for _ in 0..len {
                        let ch = in_char(stream);
                        if ch < 0 {
                            error("read error");
                        }
                        bytes.push(ch as Rbyte);
                    }
                    bytes
                } else {
                    error("read error")
                }
            };
            for (index, byte) in bytes.iter().enumerate() {
                SET_RAW_ELT(raw, index as c_int, *byte);
            }
        }
        R_unserialize(raw, R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// Character conversion and reading
// ---------------------------------------------------------------------------

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn ReadChar(stream: R_inpstream_t, buf: *mut c_char, length: c_int, levs: c_int) {
    // SAFETY: `buf` is a caller-owned buffer of at least `length + 1`
    // bytes; the checks below precede the terminating-NUL store.
    unsafe {
        InString(stream, buf, length);
        if !buf.is_null() && length >= 0 {
            *buf.add(length as usize) = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Read reference table (C SEXP-based)
// ---------------------------------------------------------------------------

pub fn MakeReadRefTable() -> SEXP {
    let data = Rf_allocVector3(SEXPTYPE::VECSXP, INITIAL_REFREAD_TABLE_SIZE as R_xlen_t);
    Rf_cons(data, Rf_ScalarInteger(0))
}

pub fn GetReadRef(table: SEXP, index: c_int) -> SEXP {
    if table.is_null() || TYPEOF(table) != SEXPTYPE::LISTSXP || index <= 0 {
        error("reference index out of range");
    }
    let data = CAR(table);
    if data.is_null() || TYPEOF(data) != SEXPTYPE::VECSXP {
        error("reference index out of range");
    }
    let used = asInteger(CDR(table));
    if index > used {
        error("reference index out of range");
    }
    VECTOR_ELT(data, (index - 1) as R_xlen_t)
}

pub fn AddReadRef(table: SEXP, value: SEXP) {
    if table.is_null() || TYPEOF(table) != SEXPTYPE::LISTSXP {
        return;
    }
    let mut data = CAR(table);
    if data.is_null() || TYPEOF(data) != SEXPTYPE::VECSXP {
        return;
    }
    let mut used = asInteger(CDR(table));
    if used < 0 {
        used = 0;
    }

    if (used as R_xlen_t) >= XLENGTH(data) {
        let old_len = XLENGTH(data);
        let new_len = std::cmp::max(1, old_len * 2);
        let grown = Rf_allocVector3(SEXPTYPE::VECSXP, new_len);
        for i in 0..old_len {
            SET_VECTOR_ELT(grown, i, VECTOR_ELT(data, i));
        }
        SETCAR(table, grown);
        data = grown;
    }

    SET_VECTOR_ELT(data, used as R_xlen_t, value);
    SETCDR(table, Rf_ScalarInteger(used.saturating_add(1)));
}

// ---------------------------------------------------------------------------
// R_InitSerializeRoutines
// ---------------------------------------------------------------------------

pub fn R_InitSerializeRoutines() {
    // all routines are statically available.
}
