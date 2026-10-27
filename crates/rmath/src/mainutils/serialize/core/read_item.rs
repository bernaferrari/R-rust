use super::*;

// ---------------------------------------------------------------------------
// Internal ReadItem (recursive, reads from BinaryReader)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(crate) enum ReadContext {
    Data,
    ClosureBody,
    PromiseExpression,
}

pub unsafe fn ReadItemInternal(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
) -> Result<SEXP, String> {
    unsafe { read_item_in_context(reader, ref_table, ReadContext::Data) }
}

pub(crate) unsafe fn read_item_in_context(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    context: ReadContext,
) -> Result<SEXP, String> {
    // Bound native stack usage independently of the R evaluation stack.
    // This reader is private; a panic discards it at the session boundary.
    if reader.item_depth >= 128 {
        return Err("read error: serialized object nesting exceeds 128 levels".into());
    }
    reader.item_depth += 1;
    let result = unsafe { read_item_body(reader, ref_table, context) };
    reader.item_depth -= 1;
    result
}

/// These two values describe the port's imported bytecode representation,
/// not user constants. Share their owned original-session cells rather than
/// allocating two additional R nodes for every imported closure.
type ImportedMetadata = [crate::sexp::object::Sexp<'static>; 2];

unsafe fn imported_bytecode_metadata() -> Result<Option<ImportedMetadata>, String> {
    use crate::sexp::{instance::RuntimeValue, object::SexpError, owner::OwnerToken};
    let owner = unsafe { OwnerToken::current() }.map_err(|e| e.to_string())?;
    if owner.weak_owner().is_none() {
        return Ok(None); // Explicitly borrowed native fixtures retain their old path.
    }
    let _pin = owner.pin().map_err(|e| e.to_string())?;
    let pointer = owner.as_ptr();
    // No state loan crosses allocation, collection, or provider callbacks.
    let cached = unsafe {
        (*pointer)
            .serialize_state
            .imported_bytecode_metadata
            .clone()
    };
    if let [Some(marker), Some(hint)] = cached.map(|value| value.owned()) {
        return Ok(Some([marker, hint]));
    }
    let factory = owner.node_factory();
    let make = |number| {
        let value = factory
            .allocate(|arena| {
                crate::sexp::builder::scalar_integer_in(arena, number).map(|value| value.as_raw())
            })?
            .into_owned()?;
        let node = value.allocation()?;
        let heap = node.heap_identity();
        let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
        header.sxpinfo.set_named(3);
        heap.replace_node(node, header)
            .ok_or(SexpError::StaleAllocation)?;
        Ok::<_, SexpError>(value)
    };
    let marker = make(crate::eval::bytecode::GNU_BC_DIALECT_MARKER).map_err(|e| e.to_string())?;
    let hint = make(4).map_err(|e| e.to_string())?;
    owner.require_active().map_err(|e| e.to_string())?;
    // Allocation callbacks may have initialized this same owner's cache.
    // Preserve that publication and release our redundant temporary cells.
    let published = unsafe {
        (*pointer)
            .serialize_state
            .imported_bytecode_metadata
            .clone()
    };
    if let [Some(marker), Some(hint)] = published.map(|value| value.owned()) {
        return Ok(Some([marker, hint]));
    }
    unsafe {
        (*pointer).serialize_state.imported_bytecode_metadata = [
            RuntimeValue::from_owned(marker.clone()),
            RuntimeValue::from_owned(hint.clone()),
        ];
    }
    Ok(Some([marker, hint]))
}

// Read GNU compiler constants completely and retain the instruction stream in
// an explicitly tagged internal BCODESXP. Standalone GNU bytecode remains
// rejected below; this path is reached only for executable closure bodies or promise expressions.
pub(crate) unsafe fn read_bc_source(
    reader: &mut BinaryReader,
    refs: &mut ReadRefTable,
    reps: SEXP,
) -> Result<SEXP, String> {
    if reader.item_depth >= 128 {
        return Err("read error: bytecode nesting exceeds 128 levels".into());
    }
    reader.item_depth += 1;
    reader.bc_source_depth += 1;
    let result = (|| unsafe {
        let code = ReadItemInternal(reader, refs)?;
        let _code = protect(code);
        if TYPEOF(code) != SEXPTYPE::INTSXP {
            return Err("GNU bytecode instructions must be integers".into());
        }
        let n = XLENGTH(code) as usize;
        let words = if n == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(INTEGER(code), n)
        };
        crate::eval::bytecode::validate_gnu_bytecode_stream(words)?;
        let count = reader.read_object_vector_length()?;
        if count == 0 {
            return Err("GNU compiled closure has no source expression".into());
        }
        let stream_owner = if reader.pull.is_some() {
            Some(crate::sexp::owner::OwnerToken::current().map_err(|e| e.to_string())?)
        } else {
            None
        };
        let _stream_pin = stream_owner
            .map(|owner| owner.pin())
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut staged = stream_owner
            .map(super::super::stream_values::StreamValues::for_owner)
            .transpose()?;
        let constants = if staged.is_none() {
            Rf_allocVector(SEXPTYPE::VECSXP, count)
        } else {
            R_NilValue()
        };
        let _constants = protect(constants);
        for i in 0..count {
            let kind = reader.read_i32()?;
            let value = if kind == SEXPTYPE::BCODESXP.as_c_int() {
                read_bc_source(reader, refs, reps)?
            } else {
                read_bc_language(kind, reader, refs, reps)?
            };
            if let Some(values) = &mut staged {
                let owner = stream_owner.unwrap();
                owner.require_active().map_err(|e| e.to_string())?;
                values.push(
                    owner
                        .sexp(value)
                        .and_then(|v| v.into_owned())
                        .map_err(|e| e.to_string())?,
                )?;
            } else {
                SET_VECTOR_ELT(constants, i as R_xlen_t, value);
            }
        }
        let owned_constants = staged
            .map(|values| values.finish(stream_owner.unwrap(), SEXPTYPE::VECSXP))
            .transpose()?;
        let constants = owned_constants
            .as_ref()
            .map_or(constants, |value| value.as_raw());
        // Keep source fallback for every validated GNU stream outside the
        // bounded adapter. This avoids treating a private opcode collision as
        // GNU execution and preserves the existing interpreted behavior.
        if !crate::eval::bytecode::validate_gnu_adapter_with_constants(words, constants)? {
            return Ok(VECTOR_ELT(constants, 0));
        }

        // GNU keeps its source in constant zero. Share that original graph
        // rather than duplicating every imported closure's syntax tree.
        // The sticky shared mark makes public body/subset edits copy before
        // writing, including expressions also referenced by other constants.
        let source = VECTOR_ELT(constants, 0);
        SET_NAMED(source, 3);
        let _source = protect(source);
        let metadata = imported_bytecode_metadata()?;
        let marker = metadata.as_ref().map_or_else(
            || Rf_ScalarInteger(crate::eval::bytecode::GNU_BC_DIALECT_MARKER),
            |values| values[0].as_raw(),
        );
        let _marker = protect(marker);
        let stack_hint = metadata
            .as_ref()
            .map_or_else(|| Rf_ScalarInteger(4), |values| values[1].as_raw());
        let _stack_hint = protect(stack_hint);
        let bcode = Rf_allocVector(SEXPTYPE::BCODESXP, 5);
        let _bcode = protect(bcode);
        SET_VECTOR_ELT(bcode, 0, code);
        SET_VECTOR_ELT(bcode, 1, constants);
        SET_VECTOR_ELT(bcode, 2, stack_hint);
        SET_VECTOR_ELT(bcode, 3, marker);
        SET_VECTOR_ELT(bcode, 4, source);
        Ok(bcode)
    })();
    reader.item_depth -= 1;
    reader.bc_source_depth -= 1;
    result
}

pub(crate) unsafe fn read_bc_language(
    kind: i32,
    reader: &mut BinaryReader,
    refs: &mut ReadRefTable,
    reps: SEXP,
) -> Result<SEXP, String> {
    if reader.item_depth >= 128 {
        return Err("read error: bytecode language nesting exceeds 128 levels".into());
    }
    reader.item_depth += 1;
    let result = (|| unsafe {
        if kind == 243 {
            // BCREPREF
            let index = reader.read_i32()?;
            if index < 0 || index as R_xlen_t >= XLENGTH(reps) {
                return Err("invalid bytecode repetition reference".into());
            }
            let value = VECTOR_ELT(reps, index as R_xlen_t);
            if value == R_NilValue() {
                return Err("undefined bytecode repetition reference".into());
            }
            return Ok(value);
        }
        if ![244, 6, 2, 240, 239].contains(&kind) {
            return ReadItemInternal(reader, refs);
        }
        let (position, kind) = if kind == 244 {
            // BCREPDEF
            let pos = reader.read_i32()?;
            if pos < 0 || pos as R_xlen_t >= XLENGTH(reps) {
                return Err("invalid bytecode repetition definition".into());
            }
            if VECTOR_ELT(reps, pos as R_xlen_t) != R_NilValue() {
                return Err("duplicate bytecode repetition definition".into());
            }
            (Some(pos), reader.read_i32()?)
        } else {
            (None, kind)
        };
        let (node_type, attributes) = match kind {
            6 => (SEXPTYPE::LANGSXP, false),
            2 => (SEXPTYPE::LISTSXP, false),
            240 => (SEXPTYPE::LANGSXP, true),
            239 => (SEXPTYPE::LISTSXP, true),
            _ => return Err("invalid bytecode language node type".into()),
        };
        let node = allocSExp(node_type);
        let _node = protect(node);
        if let Some(pos) = position {
            SET_VECTOR_ELT(reps, pos as R_xlen_t, node);
        }
        if attributes {
            SET_ATTRIB(node, ReadItemInternal(reader, refs)?);
        }
        SETTAG(node, ReadItemInternal(reader, refs)?);
        let car_kind = reader.read_i32()?;
        SETCAR(node, read_bc_language(car_kind, reader, refs, reps)?);
        let cdr_kind = reader.read_i32()?;
        SETCDR(node, read_bc_language(cdr_kind, reader, refs, reps)?);
        Ok(node)
    })();
    reader.item_depth -= 1;
    result
}
pub(crate) unsafe fn read_packed_string_vec(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
) -> Result<SEXP, String> {
    unsafe {
        let names_flag = reader.read_i32()?;
        if names_flag != 0 {
            return Err("names in persistent strings are not supported".into());
        }
        let len = reader.read_vector_length(4)?;
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
        let _names = protect(names);
        for i in 0..len {
            let value = ReadItemInternal(reader, ref_table)?;
            if TYPEOF(value) != SEXPTYPE::CHARSXP {
                return Err("persistent names must contain strings".into());
            }
            SET_STRING_ELT(names, i as R_xlen_t, value);
        }
        Ok(names)
    }
}

pub(crate) fn first_string_elt(names: SEXP) -> String {
    unsafe {
        if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) < 1 {
            return String::new();
        }
        let raw = CHAR(STRING_ELT(names, 0));
        if raw.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned()
    }
}

pub(crate) unsafe fn read_promise(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    hasattr: i32,
    hastag: i32,
    levs: i32,
    isobj: i32,
) -> Result<SEXP, String> {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current().map_err(|e| e.to_string())?;
        let _pin = owner.pin().map_err(|e| e.to_string())?;
        owner.require_active().map_err(|e| e.to_string())?;
        let factory = owner.node_factory();
        let attributes = if hasattr != 0 {
            factory
                .wrap(ReadItemInternal(reader, ref_table)?)
                .map_err(|e| e.to_string())?
        } else {
            factory.nil()
        };
        owner.require_active().map_err(|e| e.to_string())?;
        let environment = if hastag != 0 {
            factory
                .wrap(ReadItemInternal(reader, ref_table)?)
                .map_err(|e| e.to_string())?
        } else {
            factory.nil()
        };
        if !matches!(environment.typeof_(), SEXPTYPE::NILSXP | SEXPTYPE::ENVSXP) {
            return Err("invalid promise environment".into());
        }
        owner.require_active().map_err(|e| e.to_string())?;
        let value = factory
            .wrap(ReadItemInternal(reader, ref_table)?)
            .map_err(|e| e.to_string())?;
        owner.require_active().map_err(|e| e.to_string())?;
        let expression = factory
            .wrap(read_item_in_context(
                reader,
                ref_table,
                ReadContext::PromiseExpression,
            )?)
            .map_err(|e| e.to_string())?;
        owner.require_active().map_err(|e| e.to_string())?;
        let attributes_link = factory.link(&attributes).map_err(|e| e.to_string())?;
        let body = crate::sexp::ffi::NodeBody::Promise(crate::sexp::ffi::Promsxp {
            value: factory.link(&value).map_err(|e| e.to_string())?,
            expr: factory.link(&expression).map_err(|e| e.to_string())?,
            env: factory.link(&environment).map_err(|e| e.to_string())?,
        });
        // Seal every field while the original arena is lent. The producer
        // roots the complete node before deferred allocation/GC callbacks.
        let promise = factory
            .allocate(|arena| {
                let pointer = arena.alloc_node(SEXPTYPE::PROMSXP);
                let node = arena.node_token(pointer)?;
                let heap = arena.heap_identity();
                let mut header = heap.node_snapshot(&node)?;
                header.data = body;
                header.attrib = attributes_link;
                header.sxpinfo.set_gp(levs as u16);
                header.sxpinfo.set_obj(isobj != 0);
                heap.replace_node(&node, header)?;
                Some(pointer)
            })
            .map_err(|e| e.to_string())?;
        owner.require_active().map_err(|e| e.to_string())?;
        Ok(promise.as_raw())
    }
}

/// Keep the recursive entry small: the unchanged depth limit is meaningful
/// only when a node does not reserve every unrelated payload branch's locals.
#[derive(Clone, Copy)]
pub(crate) struct ItemFlags {
    stype: c_int,
    levs: c_int,
    isobj: c_int,
    hasattr: c_int,
    hastag: c_int,
}

#[inline(never)]
pub(crate) unsafe fn read_item_body(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    context: ReadContext,
) -> Result<SEXP, String> {
    unsafe {
        let packed = reader.read_i32()?;
        let mut flags = ItemFlags {
            stype: 0,
            levs: 0,
            isobj: 0,
            hasattr: 0,
            hastag: 0,
        };
        UnpackFlags(
            packed,
            &mut flags.stype,
            &mut flags.levs,
            &mut flags.isobj,
            &mut flags.hasattr,
            &mut flags.hastag,
        );
        match SEXPTYPE(flags.stype) {
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => read_list_item(reader, ref_table, flags),
            SEXPTYPE::CLOSXP => read_closure_item(reader, ref_table, flags),
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => read_references_item(reader, ref_table, flags),
            _ => read_remaining_item(reader, ref_table, packed, flags, context),
        }
    }
}

#[inline(never)]
pub(crate) unsafe fn read_list_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    flags: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        stype,
        levs,
        isobj,
        hasattr,
        hastag,
    } = flags;
    unsafe {
        let s = allocSExp(SEXPTYPE(stype));
        let _s_guard = protect(s);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        if hastag != 0 {
            let tag = ReadItemInternal(reader, ref_table)?;
            SETTAG(s, tag);
        }
        let car = ReadItemInternal(reader, ref_table)?;
        SETCAR(s, car);
        let cdr = ReadItemInternal(reader, ref_table)?;
        SETCDR(s, cdr);
        restore_serialized_gp(s, levs, isobj);
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_closure_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    flags: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        levs,
        isobj,
        hasattr,
        ..
    } = flags;
    unsafe {
        let s = allocSExp(SEXPTYPE::CLOSXP);
        let _s_guard = protect(s);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        let cloenv = ReadItemInternal(reader, ref_table)?;
        SET_CLOENV(s, cloenv);
        let formals = ReadItemInternal(reader, ref_table)?;
        SET_FORMALS(s, formals);
        let body = read_item_in_context(reader, ref_table, ReadContext::ClosureBody)?;
        SET_BODY(s, body);
        restore_serialized_gp(s, levs, isobj);
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_references_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    flags: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        stype,
        levs,
        isobj,
        hasattr,
        ..
    } = flags;
    unsafe {
        let len = reader.read_object_vector_length()?;
        let staged = if reader.pull.is_some() {
            let owner = crate::sexp::owner::OwnerToken::current().map_err(|e| e.to_string())?;
            let _pin = owner.pin().map_err(|e| e.to_string())?;
            let reservation = owner
                .with_arena(|arena| arena.reserve_workspace(0))
                .map_err(|e| e.to_string())?
                .ok_or("unserialize exceeds session memory limit")?;
            let mut values = super::super::stream_values::StreamValues::new(reservation);
            for _ in 0..len {
                let value = ReadItemInternal(reader, ref_table)?;
                owner.require_active().map_err(|e| e.to_string())?;
                values.push(
                    owner
                        .sexp(value)
                        .and_then(|s| s.into_owned())
                        .map_err(|e| e.to_string())?,
                )?;
            }
            Some(values)
        } else {
            None
        };
        let s = Rf_allocVector3(stype, len as R_xlen_t);
        let _s_guard = protect(s);
        if let Some(values) = &staged {
            for (i, value) in values.iter().enumerate() {
                SET_VECTOR_ELT(s, i as R_xlen_t, value.as_raw());
            }
        } else {
            for i in 0..len {
                let elt = ReadItemInternal(reader, ref_table)?;
                SET_VECTOR_ELT(s, i as R_xlen_t, elt);
            }
        }
        restore_serialized_gp(s, levs, isobj);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_remaining_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    packed: c_int,
    item: ItemFlags,
    context: ReadContext,
) -> Result<SEXP, String> {
    let ItemFlags {
        stype,
        levs,
        isobj,
        hasattr,
        hastag,
    } = item;
    let flags = packed;
    unsafe {
        if stype == PERSISTSXP {
            read_persistent_item(reader, ref_table, item)
        } else if stype == NILVALUE_SXP {
            Ok(R_NilValue())
        } else if stype == GLOBALENV_SXP {
            Ok(R_GlobalEnv())
        } else if stype == UNBOUNDVALUE_SXP {
            Ok(R_UnboundValue())
        } else if stype == MISSINGARG_SXP {
            Ok(R_MissingArg())
        } else if stype == EMPTYENV_SXP {
            Ok(R_EmptyEnv())
        } else if stype == BASEENV_SXP {
            Ok(R_BaseEnv())
        } else if stype == BASENAMESPACE_SXP {
            Ok(crate::sexp::envir::R_BaseNamespace())
        } else if stype == NAMESPACESXP || stype == PACKAGESXP {
            read_namespace_item(reader, ref_table, item)
        } else if stype == REFSXP {
            let idx = InRefIndex(flags, reader)?;
            ref_table.get(idx)
        } else if stype == SEXPTYPE::ENVSXP {
            read_environment_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::SYMSXP {
            read_symbol_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::PROMSXP {
            read_promise(reader, ref_table, hasattr, hastag, levs, isobj)
        } else if stype == SEXPTYPE::CHARSXP {
            read_character_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::LGLSXP || stype == SEXPTYPE::INTSXP {
            read_integer_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::REALSXP {
            read_real_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::CPLXSXP {
            read_complex_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::STRSXP {
            read_strings_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::RAWSXP {
            read_raw_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::BCODESXP
            && (!matches!(context, ReadContext::Data) || reader.bc_source_depth > 0)
        {
            read_bytecode_source_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::BCODESXP {
            read_bytecode_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::SPECIALSXP || stype == SEXPTYPE::BUILTINSXP {
            read_primitive_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::EXTPTRSXP {
            read_external_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::WEAKREFSXP {
            read_weakref_item(reader, ref_table, item)
        } else if stype == SEXPTYPE::S4SXP {
            read_s4_item(reader, ref_table, item)
        } else if stype == ALTREP_SXP {
            read_altrep_item(reader, ref_table, item)
        } else {
            Err(format!("ReadItem: unknown type {}", stype))
        }
    }
}

#[inline(never)]
pub(crate) unsafe fn read_weakref_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        levs,
        isobj,
        hasattr,
        ..
    } = item;
    unsafe {
        let s = crate::mainutils::memory_main::R_MakeWeakRef(
            R_NilValue(),
            R_NilValue(),
            R_NilValue(),
            0,
        );
        let _s_guard = protect(s);
        ref_table.add(s);
        restore_serialized_gp(s, levs, isobj);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_persistent_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let _ = item;
    unsafe {
        let names_flag = reader.read_i32()?;
        if names_flag != 0 {
            return Err("names in persistent strings are not supported".into());
        }
        let len = reader.read_vector_length(4)?;
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
        let _names_guard = protect(names);
        for i in 0..len {
            let value = ReadItemInternal(reader, ref_table)?;
            if TYPEOF(value) != SEXPTYPE::CHARSXP {
                return Err("persistent names must contain strings".into());
            }
            SET_STRING_ELT(names, i as R_xlen_t, value);
        }
        // Public hooks run for every record. Only the private lazy-load path
        // retains a cache, including admitted original bytes for each key.
        let cache_key = if reader.lazy_restore.is_some() {
            let mut key = super::super::owned_input::InputBytes::new(ref_table.reserve(0)?, 0)
                .map_err(|e| e.to_string())?;
            for i in 0..len {
                if i > 0 {
                    key.push(0).map_err(|e| e.to_string())?;
                }
                let raw = CHAR(STRING_ELT(names, i as R_xlen_t));
                if !raw.is_null() {
                    std::io::Write::write_all(&mut key, std::ffi::CStr::from_ptr(raw).to_bytes())
                        .map_err(|e| e.to_string())?;
                }
            }
            if let Some(cached) = reader.persist_cache.as_ref().and_then(|entries| {
                entries
                    .iter()
                    .find(|(existing, _)| **existing == *key)
                    .map(|(_, value)| *value)
            }) {
                ref_table.add(cached);
                return Ok(cached);
            }
            Some(key)
        } else {
            None
        };
        let restored = if let Some(restore) = reader.lazy_restore.clone() {
            restore.restore(names)?
        } else if let Some(func) = reader.persist_hook_func {
            func(names, reader.persist_hook_data)
        } else if reader.persist_hook.is_null() || reader.persist_hook == R_NilValue() {
            return Err("no restore method available".into());
        } else {
            CallHook(names, reader.persist_hook)
        };
        let _restored_guard = protect(restored);
        ref_table.add(restored);
        if let Some(key) = cache_key {
            if reader.persist_cache.is_none() {
                reader.persist_cache = Some(ref_table.workspace()?);
            }
            reader
                .persist_cache
                .as_mut()
                .unwrap()
                .push((key, restored))?;
        }
        return Ok(restored);
    }
}

#[inline(never)]
pub(crate) unsafe fn read_namespace_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let _ = item;
    unsafe {
        let names = read_packed_string_vec(reader, ref_table)?;
        let pkg = first_string_elt(names);
        let env = if pkg.is_empty() || pkg == "base" {
            R_BaseEnv()
        } else {
            crate::mainutils::essentials::load_package_namespace_by_name(&pkg).unwrap_or_else(
                |_| {
                    crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_BaseEnv(), R_NilValue())
                },
            )
        };
        ref_table.add(env);
        Ok(env)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_environment_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let _ = item;
    unsafe {
        let locked = reader.read_i32()?;
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_BaseEnv(), R_NilValue());
        let _env = protect(env);
        ref_table.add(env); // Cyclic environments must resolve before their bindings are read.
        let parent = ReadItemInternal(reader, ref_table)?;
        if parent != R_NilValue() && TYPEOF(parent) != SEXPTYPE::ENVSXP {
            return Err("invalid environment enclosure".into());
        }
        SET_ENCLOS(
            env,
            if parent == R_NilValue() {
                R_BaseEnv()
            } else {
                parent
            },
        );
        let frame = ReadItemInternal(reader, ref_table)?;
        let _frame = protect(frame);
        let hash = ReadItemInternal(reader, ref_table)?;
        let _hash = protect(hash);
        let attributes = ReadItemInternal(reader, ref_table)?;
        SET_ATTRIB(env, attributes);
        let mut chains = vec![frame];
        if hash != R_NilValue() {
            if TYPEOF(hash) != SEXPTYPE::VECSXP {
                return Err("invalid environment hash table".into());
            }
            crate::sexp::env_hash::promote_to_hash_table(env);
            for i in 0..XLENGTH(hash) {
                chains.push(VECTOR_ELT(hash, i));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for mut cell in chains {
            while !cell.is_null() && cell != R_NilValue() {
                if TYPEOF(cell) != SEXPTYPE::LISTSXP || !seen.insert(cell as usize) {
                    return Err("invalid or cyclic environment binding list".into());
                }
                // GNU Defn.h stores active/locked binding flags in gp15/gp14.
                // Reject rather than silently turn these into ordinary bindings.
                if LEVELS(cell) & ((1 << 15) | (1 << 14)) != 0 {
                    return Err(
                        "restoration of active or individually locked bindings is not supported"
                            .into(),
                    );
                }
                let symbol = TAG(cell);
                if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                    return Err("invalid environment binding name".into());
                }
                crate::sexp::envir::defineVar(symbol, CAR(cell), env);
                cell = CDR(cell);
            }
        }
        if attributes != R_NilValue()
            && crate::eval::attrib_core::getAttrib(env, crate::eval::attrib_core::R_ClassSymbol())
                != R_NilValue()
        {
            SET_OBJECT(env, 1);
        }
        if locked != 0 {
            crate::sexp::envir::lock_environment_raw(env);
        }
        Ok(env)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_symbol_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let _ = item;
    unsafe {
        let pname = ReadItemInternal(reader, ref_table)?;
        let sym = Rf_install(CHAR(pname));
        ref_table.add(sym);
        Ok(sym)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_character_item(
    reader: &mut BinaryReader,
    _ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let _ = item;
    unsafe {
        let len = reader.read_i32()?;
        if len < 0 {
            Ok(R_NaString())
        } else if len == 0 {
            let s = Rf_mkCharLen(b"\0" as *const u8 as *const c_char, 0);
            Ok(s)
        } else {
            let bytes =
                reader.read_string_bytes(len as usize, |length| _ref_table.reserve(length))?;
            if let Some(name) = crate::sexp::symbol::interned_ascii_character(&bytes) {
                return Ok(name);
            }
            let s = Rf_mkCharLen(bytes.as_ptr() as *const c_char, len);
            Ok(s)
        }
    }
}

#[inline(never)]
pub(crate) unsafe fn read_integer_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        stype,
        levs,
        isobj,
        hasattr,
        ..
    } = item;
    unsafe {
        let len = reader.read_vector_length(4)?;
        // Plain logical scalars may share GNU's immutable singleton bank.
        // Serialized metadata and noncanonical integer representations still
        // require their own vector and must survive an exact wire round trip.
        if stype == SEXPTYPE::LGLSXP && len == 1 && levs == 0 && isobj == 0 && hasattr == 0 {
            let value = reader.read_i32()?;
            if matches!(value, 0 | 1 | crate::sexp::ffi::NA_LOGICAL) {
                return Ok(Rf_ScalarLogical(value));
            }
            let s = Rf_allocVector3(stype, 1);
            SET_LOGICAL_ELT(s, 0, value);
            return Ok(s);
        }
        let s = Rf_allocVector3(stype, len as R_xlen_t);
        let _s_guard = protect(s);
        let int_data = INTEGER(s);
        for i in 0..len as isize {
            *int_data.offset(i) = reader.read_i32()?;
        }
        restore_serialized_gp(s, levs, isobj);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_real_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        stype,
        levs,
        isobj,
        hasattr,
        ..
    } = item;
    unsafe {
        let len = reader.read_vector_length(8)?;
        let s = Rf_allocVector3(stype, len as R_xlen_t);
        let _s_guard = protect(s);
        let real_data = REAL(s);
        for i in 0..len as isize {
            *real_data.offset(i) = reader.read_f64()?;
        }
        restore_serialized_gp(s, levs, isobj);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_complex_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        stype,
        levs,
        isobj,
        hasattr,
        ..
    } = item;
    unsafe {
        let len = reader.read_vector_length(16)?;
        let s = Rf_allocVector3(stype, len as R_xlen_t);
        let _s_guard = protect(s);
        let cpx_data = COMPLEX(s);
        for i in 0..len as isize {
            let r = reader.read_f64()?;
            let im = reader.read_f64()?;
            *cpx_data.offset(i) = Rcomplex { r, i: im };
        }
        restore_serialized_gp(s, levs, isobj);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_strings_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        levs,
        isobj,
        hasattr,
        ..
    } = item;
    unsafe {
        let len = reader.read_vector_length(4)?;
        if reader.bc_source_depth > 0 && len == 1 && levs == 0 && isobj == 0 && hasattr == 0 {
            let character = ReadItemInternal(reader, ref_table)?;
            if TYPEOF(character) != SEXPTYPE::CHARSXP {
                return Err("invalid character scalar in GNU bytecode".into());
            }
            let _character = protect(character);
            let owner = crate::sexp::owner::OwnerToken::current().map_err(|e| e.to_string())?;
            let value = owner
                .with_arena(|arena| arena.immutable_bytecode_string(character))
                .map_err(|e| e.to_string())?;
            if value.is_null() {
                return Err("cannot allocate GNU bytecode string literal".into());
            }
            return Ok(value);
        }
        let s = Rf_allocVector3(SEXPTYPE::STRSXP, len as R_xlen_t);
        let _s_guard = protect(s);
        for i in 0..len {
            let elt = ReadItemInternal(reader, ref_table)?;
            SET_STRING_ELT(s, i as R_xlen_t, elt);
        }
        restore_serialized_gp(s, levs, isobj);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_raw_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        levs,
        isobj,
        hasattr,
        ..
    } = item;
    unsafe {
        let len = reader.read_vector_length(1)?;
        let s = Rf_allocVector3(SEXPTYPE::RAWSXP, len as R_xlen_t);
        let _s_guard = protect(s);
        let raw_data = RAW(s);
        for i in 0..len as isize {
            *raw_data.offset(i) = reader.read_byte()?;
        }
        restore_serialized_gp(s, levs, isobj);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_bytecode_source_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let _ = item;
    unsafe {
        let repeated = reader.read_vector_length(4)?;
        let reps = Rf_allocVector(SEXPTYPE::VECSXP, repeated);
        let _reps = protect(reps);
        for i in 0..repeated {
            SET_VECTOR_ELT(reps, i as R_xlen_t, R_NilValue());
        }
        read_bc_source(reader, ref_table, reps)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_bytecode_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let _ = item;
    unsafe {
        let repeated = reader.read_i32()?;
        if repeated < 1 {
            return Err("GNU R BCODESXP has an invalid repetition table length".into());
        }
        let code = ReadItemInternal(reader, ref_table)?;
        if TYPEOF(code) != SEXPTYPE::INTSXP {
            return Err("GNU R BCODESXP instruction stream is not an integer vector".into());
        }
        let code_len = XLENGTH(code) as usize;
        let code_ptr = INTEGER(code);
        if code_len > 0 && code_ptr.is_null() {
            return Err("GNU R BCODESXP instruction stream has a null data pointer".into());
        }
        let code_slice = if code_len == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(code_ptr, code_len)
        };
        crate::eval::bytecode::validate_gnu_bytecode_stream(code_slice)?;
        Err("GNU R BCODESXP is well-framed but execution adapter is unavailable".into())
    }
}

#[inline(never)]
pub(crate) unsafe fn read_primitive_item(
    reader: &mut BinaryReader,
    _ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let _ = item;
    unsafe {
        // GNU serialize.c: OutInteger(strlen(PRIMNAME)); OutString(name, len)
        let length = reader.read_i32()?;
        if length < 0 {
            return Err("invalid primitive name length".into());
        }
        let bytes =
            reader.read_string_bytes(length as usize, |length| _ref_table.reserve(length))?;
        let mut cbuf = bytes;
        cbuf.push(0).map_err(|e| e.to_string())?;
        let prim = crate::mainutils::names::R_Primitive(cbuf.as_ptr() as *const c_char);
        if prim.is_null() || prim == R_NilValue() {
            Ok(R_NilValue())
        } else {
            Ok(prim)
        }
    }
}

#[inline(never)]
pub(crate) unsafe fn read_external_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        levs,
        isobj,
        hasattr,
        ..
    } = item;
    unsafe {
        // GNU serialize.c: HashAdd; WriteItem(PROT); WriteItem(TAG); addr is not serialized.
        let s = allocSExp(SEXPTYPE::EXTPTRSXP);
        let _s_guard = protect(s);
        ref_table.add(s);
        crate::mainutils::memory_main::R_SetExternalPtrAddr(s, std::ptr::null_mut());
        let prot = ReadItemInternal(reader, ref_table)?;
        crate::mainutils::memory_main::R_SetExternalPtrProtected(s, prot);
        let tag = ReadItemInternal(reader, ref_table)?;
        crate::mainutils::memory_main::R_SetExternalPtrTag(s, tag);
        restore_serialized_gp(s, levs, isobj);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_s4_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags {
        levs,
        isobj,
        hasattr,
        ..
    } = item;
    unsafe {
        // GNU serialize.c: S4SXP is attributes-only; allocS4Object + InAttrib.
        let s = allocSExp(SEXPTYPE::S4SXP);
        let _s_guard = protect(s);
        SET_S4_OBJECT(s);
        restore_serialized_gp(s, levs, isobj);
        if hasattr != 0 {
            let attr = ReadItemInternal(reader, ref_table)?;
            SET_ATTRIB(s, attr);
        }
        Ok(s)
    }
}

#[inline(never)]
pub(crate) unsafe fn read_altrep_item(
    reader: &mut BinaryReader,
    ref_table: &mut ReadRefTable,
    item: ItemFlags,
) -> Result<SEXP, String> {
    let ItemFlags { levs, isobj, .. } = item;
    unsafe {
        let info = ReadItemInternal(reader, ref_table)?;
        let _info = protect(info);
        let state = ReadItemInternal(reader, ref_table)?;
        let _state = protect(state);
        let attr = ReadItemInternal(reader, ref_table)?;
        let _attr = protect(attr);
        altrep_unserialize_ex(info, state, attr, isobj, levs)
    }
}

// ---------------------------------------------------------------------------
// defaultSerializeVersion
// ---------------------------------------------------------------------------

pub unsafe fn defaultSerializeVersion() -> c_int {
    R_DEFAULT_SERIALIZE_VERSION
}
