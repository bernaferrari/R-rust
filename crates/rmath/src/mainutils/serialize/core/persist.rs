use super::*;

// ---------------------------------------------------------------------------
// Reference index packing/unpacking
// ---------------------------------------------------------------------------

pub unsafe fn OutRefIndex(writer: &mut BinaryWriter, i: c_int) {
    if i > MAX_PACKED_INDEX {
        writer.write_i32(REFSXP);
        writer.write_i32(i);
    } else {
        writer.write_i32((i << 8) | REFSXP);
    }
}

pub fn InRefIndex(flags: c_int, reader: &mut BinaryReader) -> Result<c_int, String> {
    let i = flags >> 8;
    if i == 0 { reader.read_i32() } else { Ok(i) }
}

// ---------------------------------------------------------------------------
// Version decoding
// ---------------------------------------------------------------------------

pub unsafe fn DecodeVersion(packed: c_int, v: *mut c_int, p: *mut c_int, s: *mut c_int) {
    unsafe {
        if !v.is_null() {
            *v = packed / 65536;
        }
        let mut rem = packed % 65536;
        if !p.is_null() {
            *p = rem / 256;
        }
        rem %= 256;
        if !s.is_null() {
            *s = rem;
        }
    }
}

// ---------------------------------------------------------------------------
// Attribute / tag presence helpers
// ---------------------------------------------------------------------------

#[inline]
pub(crate) unsafe fn sexp_has_attributes(s: SEXP) -> bool {
    unsafe {
        let attrib = ATTRIB(s);
        !attrib.is_null() && attrib != R_NilValue()
    }
}

#[inline]
pub(crate) unsafe fn sexp_has_tag(s: SEXP) -> bool {
    unsafe {
        let tag = TAG(s);
        !tag.is_null() && tag != R_NilValue()
    }
}

// Match GNU's structural package/namespace predicates, plus namespaces
// represented by this runtime's package cache.
pub(crate) unsafe fn persistent_package_or_namespace(s: SEXP) -> bool {
    unsafe {
        let name = crate::eval::attrib_core::getAttrib(s, Rf_install(c"name".as_ptr()));
        if TYPEOF(name) == SEXPTYPE::STRSXP
            && XLENGTH(name) > 0
            && CStr::from_ptr(CHAR(STRING_ELT(name, 0)))
                .to_bytes()
                .starts_with(b"package:")
        {
            return true;
        }
        let info = R_findVarInFrame(s, Rf_install(c".__NAMESPACE__.".as_ptr()));
        if TYPEOF(info) == SEXPTYPE::ENVSXP {
            let _info_guard = protect(info);
            let spec = R_findVarInFrame(info, Rf_install(c"spec".as_ptr()));
            if TYPEOF(spec) == SEXPTYPE::STRSXP && XLENGTH(spec) > 0 {
                return true;
            }
        }
        with_required_current_instance(|instance| {
            (*instance)
                .package_namespace_cache
                .values()
                .any(|(_, env)| *env == s)
        })
    }
}

pub(crate) unsafe fn persistent_hook_eligible(s: SEXP) -> bool {
    unsafe {
        match TYPEOF(s) {
            t if t == SEXPTYPE::EXTPTRSXP || t == SEXPTYPE::WEAKREFSXP => true,
            t if t == SEXPTYPE::ENVSXP => {
                s != R_GlobalEnv()
                    && s != R_BaseEnv()
                    && s != R_EmptyEnv()
                    && !persistent_package_or_namespace(s)
            }
            _ => false,
        }
    }
}

pub(crate) unsafe fn persistent_name(s: SEXP, hook: SEXP) -> SEXP {
    unsafe { invoke_persist_hook(s, None, hook) }
}

pub(crate) unsafe fn invoke_persist_hook(
    s: SEXP,
    hook_func: Option<unsafe extern "C" fn(SEXP, SEXP) -> SEXP>,
    hook_data: SEXP,
) -> SEXP {
    unsafe {
        if !persistent_hook_eligible(s) {
            return R_NilValue();
        }
        let name = if let Some(func) = hook_func {
            func(s, hook_data)
        } else if hook_data.is_null() || hook_data == R_NilValue() {
            return R_NilValue();
        } else {
            CallHook(s, hook_data)
        };
        if name == R_NilValue() || name.is_null() {
            return R_NilValue();
        }
        if TYPEOF(name) != SEXPTYPE::STRSXP || XLENGTH(name) == 0 {
            error("persistent hook must return a nonempty character vector");
        }
        name
    }
}

pub(crate) unsafe fn writer_persistent_name(s: SEXP, writer: &BinaryWriter) -> SEXP {
    unsafe {
        if writer.persist_hook_func.is_some() {
            invoke_persist_hook(s, writer.persist_hook_func, writer.persist_hook_data)
        } else {
            persistent_name(s, writer.persist_hook)
        }
    }
}

// ---------------------------------------------------------------------------
// SaveSpecialHook -- detect special singleton values
// ---------------------------------------------------------------------------

pub unsafe fn SaveSpecialHook(item: SEXP) -> c_int {
    unsafe {
        if item.is_null() {
            return NILVALUE_SXP;
        }
        if TYPEOF(item) == SEXPTYPE::NILSXP {
            return NILVALUE_SXP;
        }
        if item == R_GlobalEnv() {
            return GLOBALENV_SXP;
        }
        if item == R_UnboundValue() {
            return UNBOUNDVALUE_SXP;
        }
        if item == R_MissingArg() {
            return MISSINGARG_SXP;
        }
        if item == R_EmptyEnv() {
            return EMPTYENV_SXP;
        }
        if item == R_BaseEnv() {
            return BASEENV_SXP;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Internal WriteItem (recursive, writes to BinaryWriter)
// ---------------------------------------------------------------------------

// Nested GNU bytecode constants use WriteBC1, without an extra object tag or
// repetition-table header. A single outer WriteBC owns the repetition table.
pub(crate) unsafe fn write_gnu_bc_payload(
    s: SEXP,
    ref_table: &mut WriteHashTable,
    writer: &mut BinaryWriter,
    depth: usize,
) {
    unsafe {
        if depth >= 128 {
            error("GNU bytecode nesting exceeds 128 levels");
        }
        if !crate::eval::bc_eval::BCODE_IS_GNU(s) {
            error("cannot serialize private bytecode as a GNU constant");
        }
        let code = VECTOR_ELT(s, 0);
        let constants = crate::eval::bc_eval::BCODE_CONSTS(s);
        if (*s).data.vector().length != 5
            || (!ATTRIB(s).is_null() && ATTRIB(s) != R_NilValue())
            || code.is_null()
            || TYPEOF(code) != SEXPTYPE::INTSXP
            || constants.is_null()
            || TYPEOF(constants) != SEXPTYPE::VECSXP
        {
            error("invalid GNU constant bytecode payload");
        }
        let code_len = XLENGTH(code) as usize;
        let code_ptr = INTEGER(code);
        if code_ptr.is_null() {
            error("GNU bytecode instruction stream has a null data pointer");
        }
        let words = std::slice::from_raw_parts(code_ptr, code_len);
        let constant_count = XLENGTH(constants) as usize;
        if constant_count > i32::MAX as usize {
            error("GNU bytecode constant pool is too large to serialize");
        }
        match crate::eval::bytecode::validate_gnu_adapter_with_constants(words, constants) {
            Ok(true) => {}
            Ok(false) => error("unsupported GNU bytecode stream cannot be serialized"),
            Err(message) => error(&message),
        }
        WriteItemInternal(VECTOR_ELT(s, 0), ref_table, writer);
        writer.write_i32(constant_count as i32);
        for i in 0..XLENGTH(constants) {
            let value = VECTOR_ELT(constants, i);
            write_bc_language(value, ref_table, writer, depth + 1);
        }
    }
}

pub(crate) unsafe fn write_bc_language(
    value: SEXP,
    ref_table: &mut WriteHashTable,
    writer: &mut BinaryWriter,
    depth: usize,
) {
    unsafe {
        if depth >= 128 {
            error("GNU bytecode language nesting exceeds 128 levels");
        }
        let kind = TYPEOF(value);
        if kind == SEXPTYPE::BCODESXP {
            writer.write_i32(kind);
            write_gnu_bc_payload(value, ref_table, writer, depth + 1);
            return;
        }
        if kind != SEXPTYPE::LANGSXP && kind != SEXPTYPE::LISTSXP {
            writer.write_i32(kind);
            WriteItemInternal(value, ref_table, writer);
            return;
        }

        let attributes = ATTRIB(value);
        let plain = attributes.is_null() || attributes == R_NilValue();
        writer.write_i32(if kind == SEXPTYPE::LANGSXP {
            if plain { 6 } else { 240 }
        } else if plain {
            2
        } else {
            239
        });
        if !attributes.is_null() && attributes != R_NilValue() {
            WriteItemInternal(attributes, ref_table, writer);
        }
        WriteItemInternal(TAG(value), ref_table, writer);
        write_bc_language(CAR(value), ref_table, writer, depth + 1);
        write_bc_language(CDR(value), ref_table, writer, depth + 1);
    }
}
