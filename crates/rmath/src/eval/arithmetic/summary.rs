use super::*;

/// Handle summary functions: sum, min, max, prod, range.
/// These accept multiple arguments and aggregate across all elements.
pub unsafe fn do_summary(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(result) = try_group_dispatch(b"Summary\0", call, op, args, rho) {
            return result;
        }
        let summary_op = SummaryOp::from_name(get_op_name(op, call));
        #[cfg(feature = "renderplot-device")]
        if let Some(result) =
            crate::mainutils::portable_grid::unit_summary(get_op_name(op, call), args)
        {
            return result;
        }
        let na_rm = parse_summary_na_rm(args);
        let finite = parse_summary_finite(args);
        let first = first_summary_data_arg(args);
        let first_is_lt = crate::mainutils::essentials::sexp_has_class(first, "POSIXlt")
            && TYPEOF(first) == SEXPTYPE::VECSXP;
        let args = if first_is_lt {
            coerce_summary_posixlt_args(call, op, args, rho)
        } else {
            args
        };
        let _args = protect(args);
        let shape = scan_summary_shape(args, summary_op);
        let first = first_summary_data_arg(args);

        let result = match summary_op {
            SummaryOp::Sum => eval_sum(args, shape, na_rm),
            SummaryOp::Prod => eval_prod(args, shape, na_rm),
            SummaryOp::Min => eval_minmax(args, shape, na_rm, finite, SummaryOp::Min),
            SummaryOp::Max => eval_minmax(args, shape, na_rm, finite, SummaryOp::Max),
            SummaryOp::Range => eval_range(args, shape, na_rm, finite),
        };
        let _r = protect(result);

        if matches!(
            summary_op,
            SummaryOp::Min | SummaryOp::Max | SummaryOp::Range
        ) {
            restore_datetime_summary_class(first, result);
            if first_is_lt {
                return crate::mainutils::datetime::do_as_POSIXlt(
                    call,
                    op,
                    Rf_cons(result, R_NilValue()),
                    rho,
                );
            }
        }
        result
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SummaryOp {
    Sum,
    Min,
    Max,
    Prod,
    Range,
}

impl SummaryOp {
    pub(crate) fn from_name(name: &str) -> Self {
        match name {
            "sum" => Self::Sum,
            "min" => Self::Min,
            "max" => Self::Max,
            "prod" => Self::Prod,
            "range" => Self::Range,
            _ => summary_error(format!("unsupported summary primitive '{name}'")),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct SummaryShape {
    saw_real: bool,
    saw_complex: bool,
    saw_string: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissingKind {
    NaN,
    NA,
}

pub(crate) fn merge_missing(
    current: Option<MissingKind>,
    next: MissingKind,
) -> Option<MissingKind> {
    match (current, next) {
        (Some(MissingKind::NA), _) | (_, MissingKind::NA) => Some(MissingKind::NA),
        _ => Some(MissingKind::NaN),
    }
}

pub(crate) fn real_missing(value: f64) -> Option<MissingKind> {
    if !value.is_nan() {
        None
    } else if value.to_bits() == R_NA_BIT_PATTERN {
        Some(MissingKind::NA)
    } else {
        Some(MissingKind::NaN)
    }
}

pub(crate) fn missing_real(kind: MissingKind) -> f64 {
    match kind {
        MissingKind::NA => NA_REAL,
        MissingKind::NaN => f64::NAN,
    }
}

pub(crate) fn complex_missing(value: Rcomplex) -> bool {
    value.r.is_nan() || value.i.is_nan()
}

pub(crate) unsafe fn parse_summary_na_rm(args: SEXP) -> bool {
    unsafe {
        let mut na_rm = false;
        let mut seen = false;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") {
                if seen {
                    summary_error("formal argument \"na.rm\" matched by multiple actual arguments");
                }
                seen = true;
                na_rm = summary_logical_arg(CAR(current));
            }
            current = CDR(current);
        }
        na_rm
    }
}

pub(crate) unsafe fn parse_summary_finite(args: SEXP) -> bool {
    unsafe {
        let mut finite = false;
        let mut seen = false;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "finite") {
                if seen {
                    summary_error(
                        "formal argument \"finite\" matched by multiple actual arguments",
                    );
                }
                seen = true;
                finite = summary_logical_arg(CAR(current));
            }
            current = CDR(current);
        }
        finite
    }
}

pub(crate) unsafe fn summary_logical_arg(x: SEXP) -> bool {
    unsafe {
        let Some(arg) = NumericVector::from_raw(x) else {
            summary_error("invalid 'na.rm' value");
        };
        if arg.clone().len() == 0 {
            summary_error("invalid 'na.rm' value");
        }
        match arg.clone().typeof_() {
            SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => {
                let value = arg.int_at(0);
                if value == NA_INTEGER {
                    summary_error("invalid 'na.rm' value");
                }
                value != 0
            }
            SEXPTYPE::REALSXP => {
                let value = arg.real_at(0);
                if value.is_nan() {
                    summary_error("invalid 'na.rm' value");
                }
                value != 0.0
            }
            _ => summary_error("invalid 'na.rm' value"),
        }
    }
}

pub(crate) unsafe fn scan_summary_shape(args: SEXP, op: SummaryOp) -> SummaryShape {
    unsafe {
        let mut shape = SummaryShape::default();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") || tag_name_is(TAG(current), "finite") {
                current = CDR(current);
                continue;
            }

            let value = CAR(current);
            if value.is_null() || value == R_NilValue() {
                current = CDR(current);
                continue;
            }
            let value_type = Sexp::from_raw(value)
                .map(|value| value.typeof_())
                .unwrap_or(SEXPTYPE::NILSXP);
            match value_type {
                t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {}
                t if t == SEXPTYPE::REALSXP => shape.saw_real = true,
                t if t == SEXPTYPE::CPLXSXP => {
                    if matches!(op, SummaryOp::Min | SummaryOp::Max | SummaryOp::Range) {
                        summary_error("invalid 'type' (complex) of argument");
                    }
                    shape.saw_complex = true;
                }
                t if t == SEXPTYPE::STRSXP => {
                    if matches!(op, SummaryOp::Min | SummaryOp::Max) {
                        shape.saw_string = true;
                    } else {
                        summary_error("invalid 'type' (character) of argument");
                    }
                }
                _ => summary_error("invalid 'type' of argument"),
            }
            current = CDR(current);
        }
        shape
    }
}

pub(crate) unsafe fn eval_sum(args: SEXP, shape: SummaryShape, na_rm: bool) -> SEXP {
    unsafe {
        let mut int_total: i64 = 0;
        let mut real_total = 0.0;
        let mut complex_total = Rcomplex { r: 0.0, i: 0.0 };
        let mut missing = None;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") || tag_name_is(TAG(current), "finite") {
                current = CDR(current);
                continue;
            }
            let value = CAR(current);

            match Sexp::from_raw(value).map(|s| s.typeof_()) {
                Some(SEXPTYPE::LGLSXP) | Some(SEXPTYPE::INTSXP) => {
                    let vector = NumericVector::from_raw(value).expect("integer-like vector");
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().int_at(i);
                        if item == NA_INTEGER {
                            if !na_rm {
                                missing = merge_missing(missing, MissingKind::NA);
                            }
                            continue;
                        }
                        int_total += item as i64;
                        complex_total.r += item as f64;
                    }
                }
                Some(SEXPTYPE::REALSXP) => {
                    let vector = NumericVector::from_raw(value).expect("real vector");
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().real_at(i);
                        if let Some(kind) = real_missing(item) {
                            if !na_rm {
                                missing = merge_missing(missing, kind);
                            }
                            continue;
                        }
                        real_total += item;
                        complex_total.r += item;
                    }
                }
                Some(SEXPTYPE::CPLXSXP) => {
                    let vector = Sexp::from_raw_unchecked(value);
                    for (i, item) in vector.iter_complex().enumerate() {
                        poll_vector_cancellation(i as R_xlen_t);
                        if na_rm && complex_missing(item) {
                            continue;
                        }
                        if item.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                            || complex_total.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                        {
                            complex_total.r = NA_REAL;
                        } else {
                            complex_total.r += item.r;
                        }
                        if item.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                            || complex_total.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                        {
                            complex_total.i = NA_REAL;
                        } else {
                            complex_total.i += item.i;
                        }
                    }
                }
                _ => {}
            }
            current = CDR(current);
        }

        if shape.saw_complex {
            return Rf_ScalarComplex(complex_total);
        }
        if shape.saw_real {
            if let Some(kind) = missing {
                return Rf_ScalarReal(missing_real(kind));
            }
            return Rf_ScalarReal((int_total as f64) + real_total);
        }
        if missing.is_some() {
            return Rf_ScalarInteger(NA_INTEGER);
        }
        if int_total > i32::MAX as i64 || int_total < i32::MIN as i64 {
            return Rf_ScalarReal(int_total as f64);
        }
        Rf_ScalarInteger(int_total as i32)
    }
}

pub(crate) unsafe fn eval_prod(args: SEXP, shape: SummaryShape, na_rm: bool) -> SEXP {
    unsafe {
        let mut real_total = 1.0;
        let mut complex_total = Rcomplex { r: 1.0, i: 0.0 };
        let mut missing = None;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") || tag_name_is(TAG(current), "finite") {
                current = CDR(current);
                continue;
            }

            let value = CAR(current);
            match Sexp::from_raw(value).map(|s| s.typeof_()) {
                Some(SEXPTYPE::LGLSXP) | Some(SEXPTYPE::INTSXP) => {
                    let vector = NumericVector::from_raw(value).expect("integer-like vector");
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().int_at(i);
                        if item == NA_INTEGER {
                            if !na_rm {
                                missing = merge_missing(missing, MissingKind::NA);
                            }
                            continue;
                        }
                        let item = item as f64;
                        real_total *= item;
                        complex_total.r *= item;
                        complex_total.i *= item;
                    }
                }
                Some(SEXPTYPE::REALSXP) => {
                    let vector = NumericVector::from_raw(value).expect("real vector");
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().real_at(i);
                        if let Some(kind) = real_missing(item) {
                            if !na_rm {
                                missing = merge_missing(missing, kind);
                            }
                            continue;
                        }
                        real_total *= item;
                        complex_total.r *= item;
                        complex_total.i *= item;
                    }
                }
                Some(SEXPTYPE::CPLXSXP) => {
                    let vector = Sexp::from_raw_unchecked(value);
                    for (i, item) in vector.iter_complex().enumerate() {
                        poll_vector_cancellation(i as R_xlen_t);
                        if complex_missing(item) {
                            if !na_rm {
                                missing = merge_missing(missing, MissingKind::NA);
                            }
                            continue;
                        }
                        let old = complex_total;
                        complex_total.r = old.r * item.r - old.i * item.i;
                        complex_total.i = old.r * item.i + old.i * item.r;
                    }
                }
                _ => {}
            }
            current = CDR(current);
        }

        if shape.saw_complex {
            if missing.is_some() {
                return Rf_ScalarComplex(Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                });
            }
            Rf_ScalarComplex(complex_total)
        } else if let Some(kind) = missing {
            Rf_ScalarReal(missing_real(kind))
        } else {
            Rf_ScalarReal(real_total)
        }
    }
}

pub(crate) unsafe fn eval_minmax_string(args: SEXP, na_rm: bool, op: SummaryOp) -> SEXP {
    unsafe {
        let mut best: Option<String> = None;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") || tag_name_is(TAG(current), "finite") {
                current = CDR(current);
                continue;
            }
            let value = CAR(current);
            let t = TYPEOF(value);
            let n = XLENGTH(value);
            if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP {
                let mut extreme: Option<f64> = None;
                for i in 0..n {
                    let v = if t == SEXPTYPE::REALSXP {
                        REAL_ELT(value, i as i32)
                    } else {
                        let iv = INTEGER_ELT(value, i as i32);
                        if iv == NA_INTEGER {
                            if na_rm {
                                continue;
                            }
                            let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                            SET_STRING_ELT(out, 0, crate::sexp::globals::R_NaString());
                            return out;
                        }
                        iv as f64
                    };
                    if t == SEXPTYPE::REALSXP && v.is_nan() {
                        if na_rm {
                            continue;
                        }
                        let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                        SET_STRING_ELT(out, 0, crate::sexp::globals::R_NaString());
                        return out;
                    }
                    extreme = Some(match extreme {
                        None => v,
                        Some(prev) => {
                            if op == SummaryOp::Min {
                                prev.min(v)
                            } else {
                                prev.max(v)
                            }
                        }
                    });
                }
                if let Some(v) = extreme {
                    let text = if t == SEXPTYPE::REALSXP && v.fract() != 0.0 {
                        format!("{v}")
                    } else {
                        format!("{}", v as i64)
                    };
                    best = Some(match best {
                        None => text,
                        Some(prev) => {
                            if (op == SummaryOp::Min && text < prev)
                                || (op == SummaryOp::Max && text > prev)
                            {
                                text
                            } else {
                                prev
                            }
                        }
                    });
                }
                current = CDR(current);
                continue;
            }
            for i in 0..n {
                if t != SEXPTYPE::STRSXP {
                    continue;
                }
                let s = STRING_ELT(value, i);
                let text = if s.is_null() || s == crate::sexp::globals::R_NaString() {
                    if na_rm {
                        continue;
                    }
                    let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                    SET_STRING_ELT(out, 0, crate::sexp::globals::R_NaString());
                    return out;
                } else {
                    std::ffi::CStr::from_ptr(CHAR(s))
                        .to_string_lossy()
                        .into_owned()
                };
                best = Some(match best {
                    None => text,
                    Some(prev) => {
                        if (op == SummaryOp::Min && text < prev)
                            || (op == SummaryOp::Max && text > prev)
                        {
                            text
                        } else {
                            prev
                        }
                    }
                });
            }
            current = CDR(current);
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if let Some(text) = best {
            let cstr = std::ffi::CString::new(text).unwrap_or_default();
            SET_STRING_ELT(out, 0, Rf_mkChar(cstr.as_ptr()));
        } else {
            SET_STRING_ELT(out, 0, crate::sexp::globals::R_NaString());
        }
        out
    }
}

pub(crate) unsafe fn eval_minmax(
    args: SEXP,
    shape: SummaryShape,
    na_rm: bool,
    finite: bool,
    op: SummaryOp,
) -> SEXP {
    unsafe {
        if shape.saw_string {
            return eval_minmax_string(args, na_rm, op);
        }
        let mut seen = false;
        let mut int_best = if op == SummaryOp::Min {
            i32::MAX
        } else {
            i32::MIN
        };
        let mut real_best = if op == SummaryOp::Min {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
        let mut saw_string = false;
        let mut scan = args;
        while !scan.is_null() && scan != R_NilValue() {
            if !tag_name_is(TAG(scan), "na.rm") && !tag_name_is(TAG(scan), "finite") {
                if TYPEOF(CAR(scan)) == SEXPTYPE::STRSXP {
                    saw_string = true;
                    break;
                }
            }
            scan = CDR(scan);
        }
        if saw_string {
            return eval_minmax_string(args, na_rm, op);
        }
        let mut missing = None;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            if tag_name_is(TAG(current), "na.rm") || tag_name_is(TAG(current), "finite") {
                current = CDR(current);
                continue;
            }
            let value = CAR(current);
            match Sexp::from_raw(value).map(|s| s.typeof_()) {
                Some(SEXPTYPE::LGLSXP) | Some(SEXPTYPE::INTSXP) => {
                    let vector = NumericVector::from_raw(value).expect("integer-like vector");
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().int_at(i);
                        if item == NA_INTEGER {
                            if !finite && !na_rm {
                                missing = merge_missing(missing, MissingKind::NA);
                            }
                            continue;
                        }
                        seen = true;
                        if op == SummaryOp::Min {
                            int_best = int_best.min(item);
                            real_best = real_best.min(item as f64);
                        } else {
                            int_best = int_best.max(item);
                            real_best = real_best.max(item as f64);
                        }
                    }
                }
                Some(SEXPTYPE::REALSXP) => {
                    let vector = NumericVector::from_raw(value).expect("real vector");
                    for i in 0..vector.clone().len() {
                        poll_vector_cancellation(i);
                        let item = vector.clone().real_at(i);
                        if finite && !item.is_finite() {
                            continue;
                        }
                        if let Some(kind) = real_missing(item) {
                            if !na_rm {
                                missing = merge_missing(missing, kind);
                            }
                            continue;
                        }
                        seen = true;
                        if op == SummaryOp::Min {
                            real_best = real_best.min(item);
                        } else {
                            real_best = real_best.max(item);
                        }
                    }
                }
                _ => {}
            }
            current = CDR(current);
        }

        if let Some(kind) = missing {
            if shape.saw_real {
                return Rf_ScalarReal(missing_real(kind));
            }
            return Rf_ScalarInteger(NA_INTEGER);
        }
        if !seen {
            warn_empty_minmax(op);
            return Rf_ScalarReal(if op == SummaryOp::Min {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            });
        }
        if shape.saw_real {
            Rf_ScalarReal(real_best)
        } else {
            Rf_ScalarInteger(int_best)
        }
    }
}

pub(crate) unsafe fn eval_range(
    args: SEXP,
    shape: SummaryShape,
    na_rm: bool,
    finite: bool,
) -> SEXP {
    unsafe {
        let min = eval_minmax(args, shape, na_rm, finite, SummaryOp::Min);
        let max = eval_minmax(args, shape, na_rm, finite, SummaryOp::Max);

        let min_value = Sexp::from_raw_unchecked(min);
        let max_value = Sexp::from_raw_unchecked(max);
        let result_type = if TYPEOF(min) == SEXPTYPE::REALSXP || TYPEOF(max) == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        };
        let result = Rf_allocVector3(result_type, 2);
        let result_view = Sexp::from_raw_unchecked(result);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result_view);
        match result_type {
            SEXPTYPE::REALSXP => {
                result_mut.set_real_elt(
                    0,
                    min_value
                        .clone()
                        .real_elt(0)
                        .or_else(|| min_value.clone().integer_elt(0).map(f64::from))
                        .unwrap_or(NA_REAL),
                );
                result_mut.set_real_elt(
                    1,
                    max_value
                        .clone()
                        .real_elt(0)
                        .or_else(|| max_value.clone().integer_elt(0).map(f64::from))
                        .unwrap_or(NA_REAL),
                );
            }
            SEXPTYPE::INTSXP => {
                result_mut
                    .set_integer_elt(0, min_value.clone().integer_elt(0).unwrap_or(NA_INTEGER));
                result_mut
                    .set_integer_elt(1, max_value.clone().integer_elt(0).unwrap_or(NA_INTEGER));
            }
            _ => {}
        }
        let _ = result_mut.freeze();
        result
    }
}

pub(crate) fn warn_empty_minmax(op: SummaryOp) {
    match op {
        SummaryOp::Min => {
            warn_simple("no non-missing arguments to min; returning Inf");
        }
        SummaryOp::Max => {
            warn_simple("no non-missing arguments to max; returning -Inf");
        }
        _ => {}
    }
}

pub(crate) fn summary_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

pub(crate) fn tag_name_is(tag: SEXP, expected: &str) -> bool {
    unsafe {
        if tag.is_null() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return false;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return false;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            return false;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .is_ok_and(|name| name == expected)
    }
}

pub(crate) unsafe fn logical_arg_is_true(x: SEXP) -> bool {
    unsafe {
        let Some(arg) = NumericVector::from_raw(x) else {
            return false;
        };
        unsafe fn logical_arg_is_true(x: SEXP) -> bool {
            return false;
        }
        match arg.clone().typeof_() {
            SEXPTYPE::LGLSXP => arg.int_at(0) == TRUE,
            SEXPTYPE::INTSXP => {
                let value = arg.int_at(0);
                value != 0 && value != NA_INTEGER
            }
            SEXPTYPE::REALSXP => {
                let value = arg.real_at(0);
                value != 0.0 && value.to_bits() != R_NA_BIT_PATTERN && !value.is_nan()
            }
            _ => false,
        }
    }
}

/// Handle `mean(x, na.rm = FALSE)` for atomic numeric vectors.
///
/// Full R reaches this through `mean.default`; the embedded evaluator keeps it
/// as a primitive while preserving the C-level numeric behavior.
pub unsafe fn do_mean(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x = R_NilValue();
        let mut na_rm = false;

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if tag_name_is(TAG(current), "na.rm") {
                na_rm = logical_arg_is_true(arg);
            } else if x == R_NilValue() {
                x = arg;
            }
            current = CDR(current);
        }

        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(f64::NAN);
        }

        let Some(vector) = NumericVector::from_raw(x) else {
            return Rf_ScalarReal(f64::NAN);
        };

        let mut total = 0.0;
        let mut count = 0usize;
        for i in 0..vector.clone().len() {
            let value = vector.clone().real_at(i);
            let is_na = value.to_bits() == R_NA_BIT_PATTERN;
            let is_nan = value.is_nan() && !is_na;
            if is_na || is_nan {
                if na_rm {
                    continue;
                }
                return Rf_ScalarReal(if is_na { NA_REAL } else { f64::NAN });
            }
            total += value;
            count += 1;
        }

        if count == 0 {
            Rf_ScalarReal(f64::NAN)
        } else {
            Rf_ScalarReal(total / count as f64)
        }
    }
}

/// Handle type-checking functions: is.numeric, is.integer, is.double,
/// is.complex, is.logical, is.character, is.null, is.raw.
pub unsafe fn do_is_type(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let op_name = get_op_name(op, call);
        if op_name == "is.numeric" {
            let mut ans = R_NilValue();
            if crate::eval::dispatch::DispatchOrEval(
                call,
                op,
                c"is.numeric".as_ptr(),
                args,
                rho,
                &mut ans,
                0,
                1,
            ) != 0
            {
                return ans;
            }
        }
        let x = CAR(args);

        if x.is_null() || x == R_NilValue() {
            if op_name == "is.null" {
                return Rf_ScalarLogical(TRUE);
            }
            return Rf_ScalarLogical(FALSE);
        }
        let Some(x) = Sexp::from_raw(x) else {
            return Rf_ScalarLogical(FALSE);
        };
        if op_name == "is.numeric" {
            let raw = CAR(args);
            if crate::mainutils::objects::inherits2(raw, c"Date".as_ptr()) != 0
                || crate::mainutils::objects::inherits2(raw, c"POSIXt".as_ptr()) != 0
            {
                return Rf_ScalarLogical(FALSE);
            }
        }
        let t = x.typeof_();
        let result = match op_name {
            "is.numeric" => crate::mainutils::coerce::is_numeric_safe(x.clone()) != 0,
            "is.integer" => crate::mainutils::coerce::is_type_safe(x.clone(), 13).unwrap_or(0) != 0,
            "is.double" => t == SEXPTYPE::REALSXP,
            "is.complex" => t == SEXPTYPE::CPLXSXP,
            "is.logical" => t == SEXPTYPE::LGLSXP,
            "is.character" => t == SEXPTYPE::STRSXP,
            "is.null" => false,
            "is.raw" => t == SEXPTYPE::RAWSXP,
            _ => false,
        };
        Rf_ScalarLogical(if result { TRUE } else { FALSE })
    }
}

// ---------------------------------------------------------------------------
// Operator name extraction
// ---------------------------------------------------------------------------

pub(crate) unsafe fn get_op_name(op: SEXP, call: SEXP) -> &'static str {
    unsafe {
        // Use descriptor identity, including session-local portable bindings,
        // so aliases and NextMethod's synthetic calls keep the same operation.
        if let Some(descriptor) = crate::eval::primitive::PrimitiveDescriptor::from_raw(op) {
            return descriptor.name;
        }
        let portable = Sexp::from_raw(op).and_then(crate::eval::primitive::portable_primitive_name);
        let name = if let Some(ref name) = portable {
            name.as_str()
        } else {
            if call.is_null() || TYPEOF(CAR(call)) != SEXPTYPE::SYMSXP {
                return "";
            }
            let s = CHAR(crate::sexp::accessors::PRINTNAME(CAR(call)));
            if s.is_null() {
                return "";
            }
            std::ffi::CStr::from_ptr(s).to_str().unwrap_or("")
        };
        match name {
            "+" => "+",
            "-" => "-",
            "*" => "*",
            "/" => "/",
            "^" => "^",
            "%%" => "%%",
            "%/%" => "%/%",
            ":" => ":",
            "<" => "<",
            ">" => ">",
            "<=" => "<=",
            ">=" => ">=",
            "==" => "==",
            "!=" => "!=",
            "!" => "!",
            "abs" => "abs",
            "sqrt" => "sqrt",
            "log" => "log",
            "log2" => "log2",
            "log10" => "log10",
            "exp" => "exp",
            "sinh" => "sinh",
            "cosh" => "cosh",
            "tanh" => "tanh",
            "ceiling" => "ceiling",
            "floor" => "floor",
            "trunc" => "trunc",
            "round" => "round",
            "sign" => "sign",
            "length" => "length",
            "sum" => "sum",
            "mean" => "mean",
            "min" => "min",
            "max" => "max",
            "prod" => "prod",
            "range" => "range",
            "is.numeric" => "is.numeric",
            "is.integer" => "is.integer",
            "is.double" => "is.double",
            "is.complex" => "is.complex",
            "is.logical" => "is.logical",
            "is.character" => "is.character",
            "is.null" => "is.null",
            "is.raw" => "is.raw",
            _ => "",
        }
    }
}

// ---------------------------------------------------------------------------
// Registration functions
// ---------------------------------------------------------------------------

pub unsafe fn register_special_forms(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;
        use crate::sexp::constructors::Rf_cons;

        let special_forms = [
            "<-",
            "<<-",
            "=",
            "if",
            "{",
            "(",
            "function",
            "while",
            "for",
            "repeat",
            "break",
            "next",
            "return",
            "switch",
            "on.exit",
            "$",
            "$<-",
            "[",
            "[<-",
            "[[",
            "[[<-",
            "@",
            "@<-",
            "&&",
            "||",
            "~",
            ":",
            ".Internal",
            "expression",
            "call",
            "Exec",
            "Tailcall",
        ];

        let frame = crate::sexp::accessors::FRAME(env);
        let mut chain = frame;
        for op_name in special_forms {
            let prim =
                crate::eval::primitive::make_primitive_binding(op_name, SEXPTYPE::SPECIALSXP);
            let sym = Rf_install(CString::new(op_name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(prim, chain);
            SETTAG(cell, sym);
            chain = cell;
        }
        SET_FRAME(env, chain);
    }
}
