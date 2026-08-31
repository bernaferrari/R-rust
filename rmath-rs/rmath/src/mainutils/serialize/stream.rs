use super::*;

// ---------------------------------------------------------------------------
// Basic output routines (C stream-based, module-private)
// ---------------------------------------------------------------------------

pub unsafe fn OutInteger(stream: R_outpstream_t, i: c_int) {
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

pub unsafe fn OutReal(stream: R_outpstream_t, d: c_double) {
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

pub unsafe fn OutComplex(stream: R_outpstream_t, c: Rcomplex) {
    unsafe {
        OutReal(stream, c.r);
        OutReal(stream, c.i);
    }
}

pub unsafe fn OutByte(stream: R_outpstream_t, i: Rbyte) {
    unsafe {
        if stream.is_null() {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            out_bytes(stream, &i as *const Rbyte as *const c_void, 1);
        }
    }
}

pub unsafe fn OutString(stream: R_outpstream_t, s: *const c_char, length: c_int) {
    unsafe {
        if stream.is_null() || s.is_null() || length <= 0 {
            return;
        }
        if let Some(out_bytes) = (*stream).OutBytes {
            out_bytes(stream, s as *const c_void, length);
        }
    }
}

pub unsafe fn OutFormat(stream: R_outpstream_t) {
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

pub unsafe fn InInteger(stream: R_inpstream_t) -> c_int {
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

pub unsafe fn InReal(stream: R_inpstream_t) -> c_double {
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

pub unsafe fn InComplex(stream: R_inpstream_t) -> Rcomplex {
    unsafe {
        Rcomplex {
            r: InReal(stream),
            i: InReal(stream),
        }
    }
}

pub unsafe fn InString(stream: R_inpstream_t, buf: *mut c_char, length: c_int) {
    unsafe {
        if stream.is_null() || buf.is_null() || length <= 0 {
            return;
        }
        if let Some(in_bytes) = (*stream).InBytes {
            in_bytes(stream, buf as *mut c_void, length);
        }
    }
}

pub unsafe fn InWord(stream: R_inpstream_t, buf: *mut c_char, size: c_int) {
    unsafe {
        // Simplified: just read size bytes
        InString(stream, buf, size);
    }
}

pub unsafe fn InFormat(stream: R_inpstream_t) {
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

pub unsafe fn MakeHashTable() -> SEXP {
    unsafe {
        let vec = Rf_allocVector3(SEXPTYPE::VECSXP, HASHSIZE as R_xlen_t);
        Rf_cons(Rf_ScalarInteger(0), vec)
    }
}

pub unsafe fn HashAdd(obj: SEXP, ht: SEXP) {
    unsafe {
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
}

pub unsafe fn HashGet(item: SEXP, ht: SEXP) -> c_int {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// Persistent name / special hooks
// ---------------------------------------------------------------------------

pub unsafe fn GetPersistentName(stream: R_outpstream_t, s: SEXP) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let Some(hook) = (*stream).OutPersistHookFunc else {
            return R_NilValue();
        };
        let res = hook(s, (*stream).OutPersistHookData);
        if res.is_null() || res == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(res) != SEXPTYPE::STRSXP {
            error("persistent hook must return a character vector");
        }
        res
    }
}

pub unsafe fn PersistentRestore(stream: R_inpstream_t, s: SEXP) -> SEXP {
    unsafe {
        if stream.is_null() {
            error("read error");
        }
        let Some(hook) = (*stream).InPersistHookFunc else {
            error("read error");
        };
        hook(s, (*stream).InPersistHookData)
    }
}

pub unsafe fn SaveSpecialHookItem(item: SEXP) -> c_int {
    unsafe { SaveSpecialHook(item) }
}

// ---------------------------------------------------------------------------
// Length writing
// ---------------------------------------------------------------------------

pub unsafe fn WriteLENGTH(stream: R_outpstream_t, s: SEXP) {
    unsafe {
        OutInteger(stream, LENGTH(s));
    }
}

// ---------------------------------------------------------------------------
// Vector serialization (C stream-based, module-private)
// ---------------------------------------------------------------------------

pub unsafe fn OutStringVec(stream: R_outpstream_t, s: SEXP, ref_table: SEXP) {
    unsafe {
        let len = XLENGTH(s);
        OutInteger(stream, 0);
        WriteLENGTH(stream, s);
        for i in 0..len {
            let elt = STRING_ELT(s, i as R_xlen_t);
            R_WriteItem(elt, stream);
        }
    }
}

pub unsafe fn InStringVec(stream: R_inpstream_t, ref_table: SEXP) -> SEXP {
    unsafe {
        if InInteger(stream) != 0 {
            error("names in persistent strings are not supported yet");
        }
        let len = InInteger(stream);
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
            let flags = InInteger(stream);
            let mut stype = 0;
            UnpackFlags(
                flags,
                &mut stype,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );

            let elt = if stype == REFSXP {
                let idx = if (flags >> 8) == 0 {
                    InInteger(stream)
                } else {
                    flags >> 8
                };
                GetReadRef(local_ref_table, idx)
            } else {
                let charsxp_type: c_int = SEXPTYPE::CHARSXP.into();
                if stype != charsxp_type {
                    error("read error");
                }
                let slen = InInteger(stream);
                let val = if slen < 0 {
                    R_NaString()
                } else if slen == 0 {
                    Rf_mkCharLen(c"".as_ptr(), 0)
                } else {
                    let mut bytes = vec![0u8; slen as usize];
                    InString(stream, bytes.as_mut_ptr() as *mut c_char, slen);
                    Rf_mkCharLen(bytes.as_ptr() as *const c_char, slen)
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

pub unsafe fn OutIntegerVec(stream: R_outpstream_t, s: SEXP, length: R_xlen_t) {
    unsafe {
        let int_data = INTEGER(s);
        for i in 0..length as isize {
            OutInteger(stream, *int_data.offset(i));
        }
    }
}

pub unsafe fn InIntegerVec(stream: R_inpstream_t, obj: SEXP, length: R_xlen_t) {
    unsafe {
        let int_data = INTEGER(obj);
        for i in 0..length as isize {
            *int_data.offset(i) = InInteger(stream);
        }
    }
}

pub unsafe fn OutRealVec(stream: R_outpstream_t, s: SEXP, length: R_xlen_t) {
    unsafe {
        let real_data = REAL(s);
        for i in 0..length as isize {
            OutReal(stream, *real_data.offset(i));
        }
    }
}

pub unsafe fn InRealVec(stream: R_inpstream_t, obj: SEXP, length: R_xlen_t) {
    unsafe {
        let real_data = REAL(obj);
        for i in 0..length as isize {
            *real_data.offset(i) = InReal(stream);
        }
    }
}

pub unsafe fn OutComplexVec(stream: R_outpstream_t, s: SEXP, length: R_xlen_t) {
    unsafe {
        let cpx_data = COMPLEX(s);
        for i in 0..length as isize {
            let c = *cpx_data.offset(i);
            OutComplex(stream, c);
        }
    }
}

pub unsafe fn InComplexVec(stream: R_inpstream_t, obj: SEXP, length: R_xlen_t) {
    unsafe {
        let cpx_data = COMPLEX(obj);
        for i in 0..length as isize {
            *cpx_data.offset(i) = InComplex(stream);
        }
    }
}

// ---------------------------------------------------------------------------
// Bytecode serialization helpers
// ---------------------------------------------------------------------------

pub unsafe fn WriteBC(s: SEXP, ref_table: SEXP, stream: R_outpstream_t) {
    unsafe {
        let _ = ref_table;
        if stream.is_null() {
            return;
        }
        let raw = R_serialize(s, R_NilValue(), R_NilValue(), R_NilValue(), R_NilValue());
        let len = XLENGTH(raw);
        if len < 0 || len > c_int::MAX as R_xlen_t {
            error("write failed");
        }
        OutInteger(stream, len as c_int);
        if len > 0 {
            if let Some(out_bytes) = (*stream).OutBytes {
                out_bytes(stream, RAW(raw) as *const c_void, len as c_int);
            } else if let Some(out_char) = (*stream).OutChar {
                for i in 0..len as usize {
                    out_char(stream, *RAW(raw).add(i) as c_int);
                }
            } else {
                error("write failed");
            }
        }
    }
}

pub unsafe fn ReadBC(ref_table: SEXP, stream: R_inpstream_t) -> SEXP {
    unsafe {
        let _ = ref_table;
        if stream.is_null() {
            error("read error");
        }
        let len = InInteger(stream);
        if len < 0 {
            error("read error");
        }
        let raw = Rf_allocVector3(SEXPTYPE::RAWSXP, len as R_xlen_t);
        if len > 0 {
            if let Some(in_bytes) = (*stream).InBytes {
                in_bytes(stream, RAW(raw) as *mut c_void, len);
            } else if let Some(in_char) = (*stream).InChar {
                for i in 0..len as usize {
                    let ch = in_char(stream);
                    if ch < 0 {
                        error("read error");
                    }
                    *RAW(raw).add(i) = ch as Rbyte;
                }
            } else {
                error("read error");
            }
        }
        R_unserialize(raw, R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// Character conversion and reading
// ---------------------------------------------------------------------------

pub unsafe fn ReadChar(stream: R_inpstream_t, buf: *mut c_char, length: c_int, levs: c_int) {
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

pub unsafe fn MakeReadRefTable() -> SEXP {
    unsafe {
        let data = Rf_allocVector3(SEXPTYPE::VECSXP, INITIAL_REFREAD_TABLE_SIZE as R_xlen_t);
        Rf_cons(data, Rf_ScalarInteger(0))
    }
}

pub unsafe fn GetReadRef(table: SEXP, index: c_int) -> SEXP {
    unsafe {
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
}

pub unsafe fn AddReadRef(table: SEXP, value: SEXP) {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// R_InitSerializeRoutines
// ---------------------------------------------------------------------------

pub unsafe fn R_InitSerializeRoutines() {
    // all routines are statically available.
}
