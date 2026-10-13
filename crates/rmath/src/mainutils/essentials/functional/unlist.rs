//! unlist() family.

use super::*;

/// R's `unlist(x)` — flatten nested list to a vector.
pub unsafe fn do_unlist(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            c"unlist".as_ptr(),
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            return ans;
        }
        let mut x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(x) == SEXPTYPE::LISTSXP {
            x = crate::mainutils::coerce::coerceVector(x, SEXPTYPE::VECSXP.as_c_int());
        }
        let _pair = protect(x);
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return x;
        }
        let recursive = logical_arg_by_name_or_position(args, "recursive", 1)
            .or_else(|| logical_from_raw_arg(args, 1))
            .unwrap_or(true);
        let use_names = logical_arg_by_name_or_position(args, "use.names", 2)
            .or_else(|| logical_from_raw_arg(args, 2))
            .unwrap_or(true);
        let mut entries = Vec::new();
        collect_unlist_entries(x, UnlistName::Absent, recursive, use_names, &mut entries);

        let result_type = unlist_result_type(&entries);
        let total = entries.len() as R_xlen_t;
        // GNU AnswerType still records the child type when every element
        // has length 0. unlist(list(list(), list()), recursive=FALSE) is
        // list(), unlist(list(integer(0))) is integer(0), and only an
        // all-NULL walk stays NULL.
        if entries.is_empty() {
            return unlist_empty_result(x, recursive, use_names, call);
        }

        let result = Rf_allocVector3(result_type, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (idx, entry) in entries.iter().enumerate() {
            match result_type {
                t if t == SEXPTYPE::STRSXP => {
                    let charsxp = if matches!(entry.value, UnlistValue::NaString) {
                        crate::sexp::globals::R_NaString()
                    } else {
                        let cstr = CString::new(entry.value.as_string()).unwrap_or_default();
                        crate::sexp::constructors::Rf_mkChar(cstr.as_ptr())
                    };
                    if !charsxp.is_null() {
                        crate::sexp::accessors::SET_STRING_ELT(result, idx as R_xlen_t, charsxp);
                    }
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    *COMPLEX(result).add(idx) = entry.value.as_complex();
                }
                t if t == SEXPTYPE::REALSXP => {
                    *REAL(result).add(idx) = entry.value.as_real();
                }
                t if t == SEXPTYPE::VECSXP => {
                    SET_VECTOR_ELT(result, idx as R_xlen_t, entry.value.as_sexp());
                }
                t if t == SEXPTYPE::LGLSXP => {
                    *LOGICAL(result).add(idx) = entry.value.as_logical();
                }
                t if t == SEXPTYPE::RAWSXP => {
                    *RAW(result).add(idx) = match &entry.value {
                        UnlistValue::Raw(byte) => *byte,
                        other => other.as_integer() as u8,
                    };
                }
                _ => {
                    *INTEGER(result).add(idx) = entry.value.as_integer();
                }
            }
        }

        if use_names
            && entries
                .iter()
                .any(|entry| !matches!(entry.name, UnlistName::Absent))
        {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, total);
            if !names.is_null() {
                let _names_guard = protect(names);
                for (idx, entry) in entries.iter().enumerate() {
                    let charsxp = match &entry.name {
                        UnlistName::Na => crate::sexp::globals::R_NaString(),
                        UnlistName::Value(s) => {
                            let cstr = CString::new(s.as_str()).unwrap_or_default();
                            Rf_mkChar(cstr.as_ptr())
                        }
                        UnlistName::Absent => Rf_mkChar(c"".as_ptr()),
                    };
                    SET_STRING_ELT(names, idx as R_xlen_t, charsxp);
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
        }

        result
    }
}

pub(super) unsafe fn logical_from_raw_arg(args: SEXP, position: usize) -> Option<bool> {
    unsafe {
        let mut current = args;
        for _ in 0..position {
            if current.is_null() || current == R_NilValue() {
                return None;
            }
            current = CDR(current);
        }
        if current.is_null() || current == R_NilValue() {
            return None;
        }
        let value = CAR(current);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::LGLSXP || TYPEOF(value) == SEXPTYPE::INTSXP {
            *INTEGER(value)
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if ISNAN(value) {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        (raw != NA_INTEGER).then_some(raw != 0)
    }
}

pub(super) struct UnlistEntry {
    value: UnlistValue,
    name: UnlistName,
}

#[derive(Clone)]
pub(super) enum UnlistName {
    Absent,
    Na,
    Value(String),
}

pub(super) enum UnlistValue {
    Logical(i32),
    Integer(i32),
    Real(f64),
    Complex(Rcomplex),
    String(String),
    Raw(u8),
    NaString,
    Object(SEXP),
    /// Atomic element still owned by the input vector — materialize at fill.
    Element {
        parent: SEXP,
        index: R_xlen_t,
    },
}

impl UnlistValue {
    fn as_integer(&self) -> i32 {
        match self {
            Self::Logical(value) | Self::Integer(value) => *value,
            Self::Real(value) => {
                if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() {
                    NA_INTEGER
                } else {
                    *value as i32
                }
            }
            Self::Raw(value) => *value as i32,
            Self::Complex(_)
            | Self::String(_)
            | Self::NaString
            | Self::Object(_)
            | Self::Element { .. } => NA_INTEGER,
        }
    }

    fn as_logical(&self) -> i32 {
        self.as_integer()
    }

    fn as_real(&self) -> f64 {
        match self {
            Self::Logical(value) | Self::Integer(value) => {
                if *value == NA_INTEGER {
                    NA_REAL
                } else {
                    *value as f64
                }
            }
            Self::Real(value) => *value,
            Self::Complex(value) => value.r,
            Self::Raw(value) => *value as f64,
            Self::String(_) | Self::NaString | Self::Object(_) | Self::Element { .. } => NA_REAL,
        }
    }

    fn as_complex(&self) -> Rcomplex {
        match self {
            Self::Logical(value) | Self::Integer(value) => Rcomplex {
                r: if *value == NA_INTEGER {
                    NA_REAL
                } else {
                    *value as f64
                },
                i: 0.0,
            },
            Self::Real(value) => Rcomplex { r: *value, i: 0.0 },
            Self::Complex(value) => *value,
            Self::Raw(value) => Rcomplex {
                r: *value as f64,
                i: 0.0,
            },
            Self::String(_) | Self::NaString | Self::Object(_) | Self::Element { .. } => Rcomplex {
                r: NA_REAL,
                i: NA_REAL,
            },
        }
    }

    fn as_string(&self) -> String {
        match self {
            Self::Logical(value) => match *value {
                TRUE => "TRUE".to_string(),
                FALSE => "FALSE".to_string(),
                _ => "NA".to_string(),
            },
            Self::Integer(value) => {
                if *value == NA_INTEGER {
                    "NA".to_string()
                } else {
                    value.to_string()
                }
            }
            Self::Real(value) => {
                if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() {
                    "NA".to_string()
                } else {
                    value.to_string()
                }
            }
            Self::Complex(value) => format!(
                "{}{}{}i",
                value.r,
                if value.i < 0.0 { "" } else { "+" },
                value.i
            ),
            Self::String(value) => value.clone(),
            Self::NaString => "NA".to_string(),
            Self::Object(value) => elt_to_string(*value, 0),
            Self::Raw(value) => format!("{value:02x}"),
            Self::Element { parent, index } => unsafe { elt_to_string(*parent, *index) },
        }
    }

    fn as_sexp(&self) -> SEXP {
        match self {
            Self::Object(value) => *value,
            Self::Element { parent, index } => unsafe { unlist_scalar_element(*parent, *index) },
            _ => unsafe { R_NilValue() },
        }
    }
}

pub(super) unsafe fn unlist_empty_result(
    x: SEXP,
    recursive: bool,
    use_names: bool,
    call: SEXP,
) -> SEXP {
    unsafe {
        let mut data = crate::mainutils::bind::BindData {
            ans_flags: 0,
            ans_ptr: std::ptr::null_mut(),
            ans_length: 0,
            ans_names: std::ptr::null_mut(),
            ans_nnames: 0,
        };
        if TYPEOF(x) == SEXPTYPE::VECSXP || TYPEOF(x) == SEXPTYPE::EXPRSXP {
            for i in 0..XLENGTH(x) {
                crate::mainutils::bind::AnswerType(
                    VECTOR_ELT(x, i),
                    recursive,
                    use_names,
                    &mut data,
                    call,
                );
            }
        }
        if data.ans_flags == 0 {
            return R_NilValue();
        }
        Rf_allocVector3(crate::mainutils::bind::ans_flags_to_mode(data.ans_flags), 0)
    }
}

pub(super) fn unlist_result_type(entries: &[UnlistEntry]) -> SEXPTYPE {
    if entries.iter().any(|entry| {
        matches!(
            entry.value,
            UnlistValue::Object(_) | UnlistValue::Element { .. }
        )
    }) {
        SEXPTYPE::VECSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::String(_) | UnlistValue::NaString))
    {
        SEXPTYPE::STRSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Complex(_)))
    {
        SEXPTYPE::CPLXSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Real(_)))
    {
        SEXPTYPE::REALSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Integer(_)))
    {
        SEXPTYPE::INTSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Logical(_)))
    {
        SEXPTYPE::LGLSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Raw(_)))
    {
        SEXPTYPE::RAWSXP
    } else {
        SEXPTYPE::INTSXP
    }
}

pub(super) unsafe fn unlist_scalar_element(x: SEXP, index: R_xlen_t) -> SEXP {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(*LOGICAL(x).add(index as usize)),
            t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(*INTEGER(x).add(index as usize)),
            t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(*REAL(x).add(index as usize)),
            t if t == SEXPTYPE::CPLXSXP => {
                let value = *COMPLEX(x).add(index as usize);
                crate::sexp::constructors::Rf_ScalarComplex(value)
            }
            t if t == SEXPTYPE::STRSXP => {
                let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                SET_STRING_ELT(out, 0, STRING_ELT(x, index));
                out
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => VECTOR_ELT(x, index),
            _ => x,
        }
    }
}

pub(super) unsafe fn collect_unlist_entries(
    x: SEXP,
    prefix: UnlistName,

    recursive: bool,
    use_names: bool,
    out: &mut Vec<UnlistEntry>,
) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }
        if TYPEOF(x) == SEXPTYPE::VECSXP || TYPEOF(x) == SEXPTYPE::EXPRSXP {
            let names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            let n = XLENGTH(x);
            if !recursive {
                let any_list = (0..n).any(|i| {
                    let child = VECTOR_ELT(x, i);
                    TYPEOF(child) == SEXPTYPE::VECSXP || TYPEOF(child) == SEXPTYPE::EXPRSXP
                });
                if any_list || !matches!(prefix, UnlistName::Absent) {
                    for i in 0..n {
                        let child = VECTOR_ELT(x, i);
                        let child_name = if use_names {
                            unlist_element_name(&prefix, names, i, n)
                        } else {
                            UnlistName::Absent
                        };
                        if TYPEOF(child) == SEXPTYPE::VECSXP || TYPEOF(child) == SEXPTYPE::EXPRSXP {
                            let child_names = crate::sexp::attrib_core::getAttrib(
                                child,
                                crate::sexp::attrib_core::R_NamesSymbol(),
                            );
                            let child_n = XLENGTH(child);
                            for j in 0..child_n {
                                let leaf_name = if use_names {
                                    unlist_element_name(&child_name, child_names, j, child_n)
                                } else {
                                    UnlistName::Absent
                                };
                                out.push(UnlistEntry {
                                    value: UnlistValue::Object(VECTOR_ELT(child, j)),
                                    name: leaf_name,
                                });
                            }
                        } else {
                            let child_n = XLENGTH(child);
                            let child_names = crate::sexp::attrib_core::getAttrib(
                                child,
                                crate::sexp::attrib_core::R_NamesSymbol(),
                            );
                            for j in 0..child_n {
                                let leaf_name = if use_names {
                                    unlist_element_name(&child_name, child_names, j, child_n)
                                } else {
                                    UnlistName::Absent
                                };
                                out.push(UnlistEntry {
                                    value: UnlistValue::Element {
                                        parent: child,
                                        index: j,
                                    },
                                    name: leaf_name,
                                });
                            }
                        }
                    }
                    return;
                }
            }
            for i in 0..n {
                let child_name = if use_names {
                    unlist_element_name(&prefix, names, i, n)
                } else {
                    UnlistName::Absent
                };
                collect_unlist_entries(VECTOR_ELT(x, i), child_name, recursive, use_names, out);
            }
            return;
        }
        if TYPEOF(x) == SEXPTYPE::LANGSXP
            || TYPEOF(x) == SEXPTYPE::SYMSXP
            || TYPEOF(x) == SEXPTYPE::CLOSXP
            || TYPEOF(x) == SEXPTYPE::ENVSXP
        {
            out.push(UnlistEntry {
                value: UnlistValue::Object(x),
                name: prefix,
            });
            return;
        }

        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        for i in 0..XLENGTH(x) {
            let name = if use_names {
                unlist_element_name(&prefix, names, i, XLENGTH(x))
            } else {
                UnlistName::Absent
            };

            let value = match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP => UnlistValue::Logical(*LOGICAL(x).add(i as usize)),
                t if t == SEXPTYPE::INTSXP => UnlistValue::Integer(*INTEGER(x).add(i as usize)),
                t if t == SEXPTYPE::REALSXP => UnlistValue::Real(*REAL(x).add(i as usize)),
                t if t == SEXPTYPE::CPLXSXP => UnlistValue::Complex(*COMPLEX(x).add(i as usize)),
                t if t == SEXPTYPE::RAWSXP => UnlistValue::Raw(*RAW(x).add(i as usize)),
                t if t == SEXPTYPE::STRSXP => {
                    let string = STRING_ELT(x, i);
                    if string.is_null() || string == crate::sexp::globals::R_NaString() {
                        UnlistValue::NaString
                    } else {
                        UnlistValue::String(
                            CStr::from_ptr(CHAR(string)).to_string_lossy().into_owned(),
                        )
                    }
                }
                _ => UnlistValue::String(elt_to_string(x, i)),
            };
            out.push(UnlistEntry { value, name });
        }
    }
}

pub(super) unsafe fn unlist_element_name(
    prefix: &UnlistName,
    names: SEXP,
    index: R_xlen_t,
    len: R_xlen_t,
) -> UnlistName {
    unsafe {
        let own = if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && index < XLENGTH(names)
        {
            let elt = STRING_ELT(names, index);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                UnlistName::Na
            } else {
                let value = CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned();
                if value.is_empty() {
                    UnlistName::Absent
                } else {
                    UnlistName::Value(value)
                }
            }
        } else {
            UnlistName::Absent
        };

        match (prefix, own) {
            (UnlistName::Value(prefix), UnlistName::Value(own)) => {
                UnlistName::Value(format!("{prefix}.{own}"))
            }
            (UnlistName::Absent, own) => own,
            (UnlistName::Na, UnlistName::Value(own)) => UnlistName::Value(format!("NA.{own}")),
            (UnlistName::Na, UnlistName::Absent) if len > 1 => {
                UnlistName::Value(format!("NA{}", index + 1))
            }
            (UnlistName::Na, _) => UnlistName::Na,
            (UnlistName::Value(prefix), UnlistName::Na) => {
                UnlistName::Value(format!("{prefix}.NA"))
            }
            (UnlistName::Value(prefix), UnlistName::Absent) if len > 1 => {
                UnlistName::Value(format!("{}{}", prefix, index + 1))
            }
            (UnlistName::Value(prefix), UnlistName::Absent) => UnlistName::Value(prefix.clone()),
        }
    }
}
