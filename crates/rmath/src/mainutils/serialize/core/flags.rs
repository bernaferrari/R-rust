use super::*;

// ---------------------------------------------------------------------------
// Flag packing/unpacking
// ---------------------------------------------------------------------------

pub unsafe fn PackFlags(
    type_: c_int,
    levs: c_int,
    isobj: c_int,
    hasattr: c_int,
    hastag: c_int,
) -> c_int {
    let mut levs = levs;
    if type_ == SEXPTYPE::CHARSXP.as_c_int() {
        // GNU: levs &= ~(CACHED_MASK | HASHASH_MASK)
        const CACHED_MASK: c_int = 1 << 5;
        const HASHASH_MASK: c_int = 1 << 0;
        levs &= !(CACHED_MASK | HASHASH_MASK);
    }
    match type_ {
        kind if kind == SEXPTYPE::LGLSXP.as_c_int()
            || kind == SEXPTYPE::INTSXP.as_c_int()
            || kind == SEXPTYPE::REALSXP.as_c_int()
            || kind == SEXPTYPE::CPLXSXP.as_c_int()
            || kind == SEXPTYPE::STRSXP.as_c_int()
            || kind == SEXPTYPE::VECSXP.as_c_int()
            || kind == SEXPTYPE::EXPRSXP.as_c_int()
            || kind == SEXPTYPE::RAWSXP.as_c_int() =>
        {
            levs &= !GROWABLE_MASK;
        }
        _ => {}
    }
    let mut val = type_ | (levs << 12);
    if isobj != 0 {
        val |= IS_OBJECT_BIT_MASK;
    }
    if hasattr != 0 {
        val |= HAS_ATTR_BIT_MASK;
    }
    if hastag != 0 {
        val |= HAS_TAG_BIT_MASK;
    }
    val
}

pub unsafe fn UnpackFlags(
    flags: c_int,
    ptype: *mut c_int,
    plevs: *mut c_int,
    pisobj: *mut c_int,
    phasattr: *mut c_int,
    phastag: *mut c_int,
) {
    unsafe {
        if !ptype.is_null() {
            *ptype = flags & DECODE_TYPE_MASK;
        }
        if !plevs.is_null() {
            *plevs = flags >> 12;
        }
        if !pisobj.is_null() {
            *pisobj = if flags & IS_OBJECT_BIT_MASK != 0 {
                1
            } else {
                0
            };
        }
        if !phasattr.is_null() {
            *phasattr = if flags & HAS_ATTR_BIT_MASK != 0 { 1 } else { 0 };
        }
        if !phastag.is_null() {
            *phastag = if flags & HAS_TAG_BIT_MASK != 0 { 1 } else { 0 };
        }
    }
}

/// Restore GNU serialized gp bits after `SETLEVELS`.
///
/// Our `SETLEVELS` only keeps JIT bits (gp 0..1). GNU writes the full
/// LEVELS word, including S4 (gp bit 4). Methods lazy-load stores
/// standardGeneric / MethodDefinition as CLOSXP with that bit set.
pub(crate) unsafe fn restore_serialized_gp(s: SEXP, levs: c_int, isobj: c_int) {
    unsafe {
        SETLEVELS(s, levs);
        if isobj != 0 {
            SET_OBJECT(s, 1);
        }
        if (levs & crate::sexp::accessors::S4_OBJECT_MASK as c_int) != 0 {
            SET_S4_OBJECT(s);
        }
    }
}

pub(crate) fn altrep_symbol_name(sym: SEXP) -> String {
    unsafe {
        if sym.is_null() || TYPEOF(sym) != SEXPTYPE::SYMSXP {
            return String::new();
        }
        let raw = CHAR(PRINTNAME(sym));
        if raw.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned()
    }
}

pub(crate) unsafe fn altrep_state_triple(state: SEXP) -> Result<(f64, f64, f64), String> {
    unsafe {
        if state.is_null() {
            return Err("invalid ALTREP compact sequence state".into());
        }
        if TYPEOF(state) == SEXPTYPE::REALSXP && XLENGTH(state) >= 3 {
            let p = REAL(state);
            return Ok((*p, *p.add(1), *p.add(2)));
        }
        if TYPEOF(state) == SEXPTYPE::INTSXP && XLENGTH(state) >= 3 {
            let p = INTEGER(state);
            return Ok((*p as f64, *p.add(1) as f64, *p.add(2) as f64));
        }
        Err("invalid ALTREP compact sequence state".into())
    }
}

pub(crate) unsafe fn expand_compact_intseq(state: SEXP) -> Result<SEXP, String> {
    unsafe {
        let (n, first, incr) = altrep_state_triple(state)?;
        if n < 0.0 || n > i32::MAX as f64 {
            return Err("invalid compact integer sequence length".into());
        }
        let len = n as R_xlen_t;
        let inc = incr as i32;
        if inc != 1 && inc != -1 {
            return Err(format!(
                "compact sequences with increment {inc} not supported yet"
            ));
        }
        let s = Rf_allocVector3(SEXPTYPE::INTSXP, len);
        let _s = protect(s);
        let data = INTEGER(s);
        let n1 = first as i32;
        for i in 0..len as isize {
            *data.offset(i) = n1 + inc * i as i32;
        }
        Ok(s)
    }
}

pub(crate) unsafe fn expand_compact_realseq(state: SEXP) -> Result<SEXP, String> {
    unsafe {
        let (n, first, incr) = altrep_state_triple(state)?;
        if n < 0.0 || n > i32::MAX as f64 {
            return Err("invalid compact real sequence length".into());
        }
        if incr != 1.0 && incr != -1.0 {
            return Err(format!(
                "compact sequences with increment {incr} not supported yet"
            ));
        }
        let len = n as R_xlen_t;
        let s = Rf_allocVector3(SEXPTYPE::REALSXP, len);
        let _s = protect(s);
        let data = REAL(s);
        for i in 0..len as isize {
            *data.offset(i) = first + incr * i as f64;
        }
        Ok(s)
    }
}

pub(crate) unsafe fn expand_deferred_string(state: SEXP) -> Result<SEXP, String> {
    unsafe {
        if state.is_null() || TYPEOF(state) != SEXPTYPE::LISTSXP {
            return Err("invalid deferred string state".into());
        }
        let arg = CAR(state);
        if arg.is_null() || arg == R_NilValue() {
            return Err("invalid deferred string argument".into());
        }
        let n = XLENGTH(arg);
        let s = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _s = protect(s);
        match TYPEOF(arg) {
            t if t == SEXPTYPE::INTSXP => {
                let data = INTEGER(arg);
                for i in 0..n as isize {
                    let v = *data.offset(i);
                    if v == crate::sexp::NA_INTEGER {
                        SET_STRING_ELT(s, i as R_xlen_t, crate::sexp::globals::R_NaString());
                    } else {
                        let text = v.to_string();
                        SET_STRING_ELT(
                            s,
                            i as R_xlen_t,
                            Rf_mkCharLen(text.as_ptr() as *const c_char, text.len() as c_int),
                        );
                    }
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let data = REAL(arg);
                for i in 0..n as isize {
                    let v = *data.offset(i);
                    if crate::sexp::ffi::is_na_real(v) {
                        SET_STRING_ELT(s, i as R_xlen_t, crate::sexp::globals::R_NaString());
                    } else if v.is_nan() {
                        SET_STRING_ELT(
                            s,
                            i as R_xlen_t,
                            Rf_mkCharLen(b"NaN".as_ptr() as *const c_char, 3),
                        );
                    } else {
                        let text = if v.fract() == 0.0 && v.abs() < 1e15 {
                            format!("{}", v as i64)
                        } else {
                            format!("{v}")
                        };
                        SET_STRING_ELT(
                            s,
                            i as R_xlen_t,
                            Rf_mkCharLen(text.as_ptr() as *const c_char, text.len() as c_int),
                        );
                    }
                }
            }
            _ => return Err("unsupported type for deferred string coercion".into()),
        }
        Ok(s)
    }
}

pub(crate) unsafe fn altrep_unserialize_ex(
    info: SEXP,
    state: SEXP,
    attr: SEXP,
    isobj: c_int,
    levs: c_int,
) -> Result<SEXP, String> {
    unsafe {
        if info.is_null() || TYPEOF(info) != SEXPTYPE::LISTSXP {
            return Err("invalid ALTREP class info".into());
        }
        let class_name = altrep_symbol_name(CAR(info));
        let type_cell = CADDR(info);
        let typ = if !type_cell.is_null()
            && TYPEOF(type_cell) == SEXPTYPE::INTSXP
            && XLENGTH(type_cell) >= 1
        {
            *INTEGER(type_cell)
        } else {
            return Err("invalid ALTREP class type".into());
        };
        let val = match class_name.as_str() {
            "compact_intseq" => expand_compact_intseq(state)?,
            "compact_realseq" => expand_compact_realseq(state)?,
            "deferred_string" => expand_deferred_string(state)?,

            "wrap_integer" | "wrap_real" | "wrap_logical" | "wrap_string" | "wrap_complex"
            | "wrap_raw" | "wrap_list" => {
                if TYPEOF(state) != SEXPTYPE::LISTSXP {
                    return Err("invalid ALTREP wrapper state".into());
                }
                CAR(state)
            }
            _ => {
                let sexp_type = SEXPTYPE(typ);
                if matches!(
                    sexp_type,
                    SEXPTYPE::LGLSXP
                        | SEXPTYPE::INTSXP
                        | SEXPTYPE::REALSXP
                        | SEXPTYPE::CPLXSXP
                        | SEXPTYPE::STRSXP
                        | SEXPTYPE::RAWSXP
                        | SEXPTYPE::VECSXP
                        | SEXPTYPE::EXPRSXP
                ) {
                    Rf_allocVector3(sexp_type, 0)
                } else {
                    return Err("cannot unserialize this ALTREP object".into());
                }
            }
        };
        let _val = protect(val);
        SET_ATTRIB(val, attr);
        restore_serialized_gp(val, levs, isobj);
        Ok(val)
    }
}
