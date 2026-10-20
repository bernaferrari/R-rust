use super::*;

pub unsafe fn WriteItemInternal(
    s: SEXP,
    ref_table: &mut WriteHashTable,
    writer: &mut BinaryWriter,
) {
    unsafe {
        let _item_guard = protect(s);
        // GNU serializes unexpanded compact integer/real sequences as
        // ALTREP_SXP (compact_intseq / compact_realseq) under version 3;
        // state is a REALSXP triple (length, origin, step) and info is the
        // class list (name symbol, package symbol, INTSXP sexp type).
        if writer.version >= 3 && TYPEOF(s) != SEXPTYPE::ENVSXP && !s.is_null() {
            if let Some(seq) = crate::sexp::altseq::unexpanded_int(s)
                && let Some((from, step)) = seq.int_origin_step()
                && (step == 1 || step == -1)
            {
                write_compact_sequence(
                    s,
                    ref_table,
                    writer,
                    c"compact_intseq",
                    SEXPTYPE::INTSXP.as_c_int(),
                    seq.len() as c_double,
                    from as c_double,
                    step as c_double,
                );
                return;
            }
            if let Some(seq) = crate::sexp::altseq::unexpanded_real(s) {
                if let Some((from, step)) = seq.real_origin_step() {
                    if step == 1.0 || step == -1.0 {
                        write_compact_sequence(
                            s,
                            ref_table,
                            writer,
                            c"compact_realseq",
                            SEXPTYPE::REALSXP.as_c_int(),
                            seq.len() as c_double,
                            from,
                            step,
                        );
                        return;
                    }
                }
            }
        }
        {
            if crate::sexp::altrep::has_extension_raw(s) {
                let object =
                    crate::sexp::altrep::rooted_raw(s).unwrap_or_else(|e| error(&e.to_string()));
                let _operation = crate::sexp::altrep::serialization_guard(&object)
                    .unwrap_or_else(|e| error(&e.to_string()));
                let copy = crate::sexp::altrep::materialized_copy(&object)
                    .unwrap_or_else(|e| error(&e.to_string()));
                WriteItemInternal(copy.clone().as_raw(), ref_table, writer);
                return;
            }
        }
        let persistent = writer_persistent_name(s, writer);
        if persistent != R_NilValue() {
            let _persistent_guard = protect(persistent);
            ref_table.add(s);
            writer.write_i32(PERSISTSXP);
            writer.write_i32(0);
            let length = c_int::try_from(XLENGTH(persistent))
                .unwrap_or_else(|_| error("persistent name vector is too long"));
            writer.write_i32(length);
            for i in 0..XLENGTH(persistent) {
                WriteItemInternal(STRING_ELT(persistent, i as R_xlen_t), ref_table, writer);
            }
            return;
        }
        // Check for special singletons
        let special = SaveSpecialHook(s);
        if special != 0 {
            writer.write_i32(special);
            return;
        }

        // Check for already-seen reference (symbols, environments, etc.)
        let ref_idx = ref_table.get(s);
        if ref_idx != 0 {
            OutRefIndex(writer, ref_idx);
            return;
        }

        let stype = TYPEOF(s);

        if stype == SEXPTYPE::ENVSXP {
            let mut binding = FRAME(s);
            while !binding.is_null() && binding != R_NilValue() {
                let symbol = TAG(binding);
                if crate::sexp::envir::binding_is_active_raw(s, symbol)
                    || crate::sexp::envir::binding_is_locked_raw(s, symbol)
                {
                    error(
                        "serialization of active or individually locked bindings is not supported",
                    );
                }
                binding = CDR(binding);
            }
            ref_table.add(s);
            writer.write_i32(SEXPTYPE::ENVSXP.as_c_int());
            writer.write_i32(i32::from(crate::sexp::envir::environment_is_locked_raw(s)));
            WriteItemInternal(ENCLOS(s), ref_table, writer);
            WriteItemInternal(FRAME(s), ref_table, writer);
            // Rust bindings are canonical in FRAME; do not serialize the host hash cache.
            WriteItemInternal(R_NilValue(), ref_table, writer);
            WriteItemInternal(ATTRIB(s), ref_table, writer);
            return;
        }

        // Handle SYMSXP
        if stype == SEXPTYPE::SYMSXP {
            ref_table.add(s);
            writer.write_i32(SEXPTYPE::SYMSXP.as_c_int());
            let pname = PRINTNAME(s);
            WriteItemInternal(pname, ref_table, writer);
            return;
        }

        // GNU uses the dotted-pair wire order for promises: environment,
        // cached value, expression. Capture actual fields before hook callbacks;
        // serialization must never force a deferred binding.
        if stype == SEXPTYPE::PROMSXP {
            let owner =
                crate::sexp::owner::OwnerToken::current().unwrap_or_else(|e| error(&e.to_string()));
            let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
            owner
                .require_active()
                .unwrap_or_else(|e| error(&e.to_string()));
            let object = owner.sexp(s).unwrap_or_else(|e| error(&e.to_string()));
            let attributes = object
                .try_attrib()
                .unwrap_or_else(|e| error(&e.to_string()));
            let environment = object.try_prenv().unwrap_or_else(|e| error(&e.to_string()));
            let value = object
                .try_prvalue()
                .unwrap_or_else(|e| error(&e.to_string()));
            let expression = object
                .try_prcode()
                .unwrap_or_else(|e| error(&e.to_string()));
            let info = object.header().sxpinfo;
            let hasattr = i32::from(!attributes.is_nil());
            let hastag = i32::from(!environment.is_nil());
            writer.write_i32(PackFlags(
                stype,
                i32::from(info.gp()),
                i32::from(info.obj()),
                hasattr,
                hastag,
            ));
            if hasattr != 0 {
                WriteItemInternal(attributes.as_raw(), ref_table, writer);
                owner
                    .require_active()
                    .unwrap_or_else(|e| error(&e.to_string()));
            }
            if hastag != 0 {
                WriteItemInternal(environment.as_raw(), ref_table, writer);
                owner
                    .require_active()
                    .unwrap_or_else(|e| error(&e.to_string()));
            }
            WriteItemInternal(value.as_raw(), ref_table, writer);
            owner
                .require_active()
                .unwrap_or_else(|e| error(&e.to_string()));
            WriteItemInternal(expression.as_raw(), ref_table, writer);
            owner
                .require_active()
                .unwrap_or_else(|e| error(&e.to_string()));
            return;
        }

        // Handle LISTSXP
        if stype == SEXPTYPE::LISTSXP {
            let hastag = if sexp_has_tag(s) { 1 } else { 0 };
            let hasattr = if sexp_has_attributes(s) { 1 } else { 0 };
            let flags = PackFlags(stype, LEVELS(s), OBJECT(s), hasattr, hastag);
            writer.write_i32(flags);
            if hasattr != 0 {
                WriteItemInternal(ATTRIB(s), ref_table, writer);
            }
            if hastag != 0 {
                WriteItemInternal(TAG(s), ref_table, writer);
            }
            WriteItemInternal(CAR(s), ref_table, writer);
            WriteItemInternal(CDR(s), ref_table, writer);
            return;
        }

        // Handle LANGSXP
        if stype == SEXPTYPE::LANGSXP {
            let hastag = if sexp_has_tag(s) { 1 } else { 0 };
            let hasattr = if sexp_has_attributes(s) { 1 } else { 0 };
            let flags = PackFlags(stype, LEVELS(s), OBJECT(s), hasattr, hastag);
            writer.write_i32(flags);
            if hasattr != 0 {
                WriteItemInternal(ATTRIB(s), ref_table, writer);
            }
            if hastag != 0 {
                WriteItemInternal(TAG(s), ref_table, writer);
            }
            WriteItemInternal(CAR(s), ref_table, writer);
            WriteItemInternal(CDR(s), ref_table, writer);
            return;
        }

        // Handle CLOSXP
        if stype == SEXPTYPE::CLOSXP {
            let hasattr = if sexp_has_attributes(s) { 1 } else { 0 };
            let flags = PackFlags(stype, LEVELS(s), OBJECT(s), hasattr, 1); // hastag=1 for closures
            writer.write_i32(flags);
            if hasattr != 0 {
                WriteItemInternal(ATTRIB(s), ref_table, writer);
            }
            WriteItemInternal(CLOENV(s), ref_table, writer);
            WriteItemInternal(FORMALS(s), ref_table, writer);
            WriteItemInternal(BODY(s), ref_table, writer);
            return;
        }

        // GNU R stores BCODESXP as a CAR/CDR/TAG node and serializes its
        // versioned threaded instruction vector with WriteBC1.  This runtime
        // currently has a private vector-backed opcode dialect; emitting it
        // as an ordinary vector would create a stream that GNU R could parse
        // but dispatch incorrectly.  Reject it at the boundary until the
        // adapter is complete.
        if stype == SEXPTYPE::BCODESXP {
            if crate::eval::bc_eval::BCODE_IS_GNU(s) {
                writer.write_i32(SEXPTYPE::BCODESXP.as_c_int());
                writer.write_i32(1); // GNU WriteBC repetition table length.
                write_gnu_bc_payload(s, ref_table, writer, 0);
                return;
            }
            error("cannot serialize private bytecode dialect as GNU R BCODESXP");
        }

        // Handle CHARSXP
        if stype == SEXPTYPE::CHARSXP {
            // GNU serializes the full gp field here (ASCII/UTF8/BYTES).
            // Do not use the 2-bit ARGUSED LEVELS() helper.
            let levs = if s.is_null() {
                0
            } else {
                (*s).sxpinfo.gp() as c_int
            };
            let flags = PackFlags(stype, levs, 0, 0, 0);
            writer.write_i32(flags);
            let len = if s == R_NaString() { -1 } else { LENGTH(s) };
            writer.write_i32(len);
            if len >= 0 {
                let char_data = CHAR(s);
                writer.write_string_bytes(char_data, len);
            }
            return;
        }

        // For atomic/vector types, compute flags with attributes
        let hasattr = if sexp_has_attributes(s) { 1 } else { 0 };
        let flags = PackFlags(stype, LEVELS(s), OBJECT(s), hasattr, 0);

        if stype == SEXPTYPE::LGLSXP || stype == SEXPTYPE::INTSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let int_data = INTEGER(s);
            for i in 0..len as isize {
                writer.write_i32(*int_data.offset(i));
            }
        } else if stype == SEXPTYPE::REALSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let real_data = REAL(s);
            for i in 0..len as isize {
                writer.write_f64(*real_data.offset(i));
            }
        } else if stype == SEXPTYPE::CPLXSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let cpx_data = COMPLEX(s);
            for i in 0..len as isize {
                let c = *cpx_data.offset(i);
                writer.write_f64(c.r);
                writer.write_f64(c.i);
            }
        } else if stype == SEXPTYPE::STRSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            for i in 0..len {
                let elt = STRING_ELT(s, i as R_xlen_t);
                WriteItemInternal(elt, ref_table, writer);
            }
        } else if stype == SEXPTYPE::RAWSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            let raw_data = RAW(s);
            for i in 0..len as isize {
                writer.write_byte(*raw_data.offset(i));
            }
        } else if stype == SEXPTYPE::VECSXP || stype == SEXPTYPE::EXPRSXP {
            writer.write_i32(flags);
            let len = XLENGTH(s);
            writer.write_i32(len as c_int);
            for i in 0..len {
                let elt = VECTOR_ELT(s, i as R_xlen_t);
                WriteItemInternal(elt, ref_table, writer);
            }
        } else {
            // Unknown type: write flags + nothing (best effort)
            writer.write_i32(flags);
        }

        // Write attributes at the end for non-CHARSXP types
        if hasattr != 0 {
            WriteItemInternal(ATTRIB(s), ref_table, writer);
        }
    }
}

/// Emit one GNU ALTREP_SXP compact-sequence record: class info list, state
/// triple, and the object's attributes.
unsafe fn write_compact_sequence(
    s: SEXP,
    ref_table: &mut WriteHashTable,
    writer: &mut BinaryWriter,
    class_name: &CStr,
    sexp_type: c_int,
    length: c_double,
    origin: c_double,
    step: c_double,
) {
    unsafe {
        let name_sym = Rf_install(class_name.as_ptr());
        let _name = protect(name_sym);
        let pkg_sym = Rf_install(c"base".as_ptr());
        let _pkg = protect(pkg_sym);
        let type_vec = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        let _tv = protect(type_vec);
        if !type_vec.is_null() {
            *INTEGER(type_vec) = sexp_type;
        }
        let info_tail = Rf_cons(type_vec, R_NilValue());
        let _it = protect(info_tail);
        let info_mid = Rf_cons(pkg_sym, info_tail);
        let _im = protect(info_mid);
        let info = Rf_cons(name_sym, info_mid);
        let _info = protect(info);
        let state = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _st = protect(state);
        if !state.is_null() {
            let p = REAL(state);
            *p = length;
            *p.add(1) = origin;
            *p.add(2) = step;
        }
        writer.write_i32(PackFlags(ALTREP_SXP, LEVELS(s), OBJECT(s), 0, 0));
        WriteItemInternal(info, ref_table, writer);
        WriteItemInternal(state, ref_table, writer);
        WriteItemInternal(ATTRIB(s), ref_table, writer);
    }
}
