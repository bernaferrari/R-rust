use super::*;

/// Handle comparison operators: <, >, <=, >=, ==, !=
pub unsafe fn do_relop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // Relational primitives are members of the Ops group too. GNU R
        // performs S3 group dispatch before the ordinary comparison ladder.
        if let Some(result) = try_group_dispatch(b"Ops\0", call, op, args, rho) {
            return result;
        }
        let op_name = get_op_name(op, call);
        match op_name {
            "<" | ">" | "<=" | ">=" | "==" | "!=" => {
                let a = CAR(args);
                let b = CAR(CDR(args));
                if a.is_null() || b.is_null() {
                    return R_NilValue();
                }
                // Ops.data.frame compares NULL as an empty column and still
                // returns a logical matrix. The scalar NULL shortcut below
                // would drop that dimension.
                if let Some(result) = data_frame_compare(op_name, call, a, b) {
                    return result;
                }
                // stock relop.c: either operand NULL yields logical(0)
                if a == R_NilValue() || b == R_NilValue() {
                    let empty = Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
                    let _empty_guard = protect(empty);
                    let src_a = if a == R_NilValue() { empty } else { a };
                    let src_b = if b == R_NilValue() { empty } else { b };
                    propagate_binary_vector_attributes(empty, src_a, src_b, 0);
                    return empty;
                }
                if let Some(result) = data_frame_compare(op_name, call, a, b) {
                    return result;
                }
                compare_values(op_name, call, a, b)
            }
            _ => R_NilValue(),
        }
    }
}

/// Compare two non-data-frame values through the ordinary relational ladder.
///
/// `Ops.data.frame` applies the primitive separately to each column, so keeping
/// this path factored lets data-frame dispatch reuse Date/factor/character and
/// numeric behavior instead of growing a second, subtly different comparator.
pub(crate) unsafe fn compare_values(op_name: &str, call: SEXP, a: SEXP, b: SEXP) -> SEXP {
    unsafe {
        if let Some(result) = numeric_version_comparison(op_name, a, b) {
            return result;
        }
        if let Some(result) = date_binary_comparison(op_name, a, b) {
            return result;
        }

        let a = posixlt_as_posixct_operand(call, a);
        let b = posixlt_as_posixct_operand(call, b);
        if let Some(result) = posixct_binary_comparison(op_name, a, b) {
            return result;
        }

        if let Some(result) = difftime_binary_comparison(op_name, a, b) {
            return result;
        }
        if let Some(result) = ordered_factor_compare(op_name, a, b) {
            return result;
        }
        if let Some(result) = unordered_factor_compare(op_name, call, a, b) {
            return result;
        }
        // stock relop.c: symbols and calls deparse to strings first
        let a = if TYPEOF(a) == SEXPTYPE::SYMSXP || TYPEOF(a) == SEXPTYPE::LANGSXP {
            crate::mainutils::deparse::deparse1line(a, false)
        } else {
            a
        };
        let b = if TYPEOF(b) == SEXPTYPE::SYMSXP || TYPEOF(b) == SEXPTYPE::LANGSXP {
            crate::mainutils::deparse::deparse1line(b, false)
        } else {
            b
        };
        let relop_ok = |s: SEXP| {
            let t = TYPEOF(s);
            t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::CPLXSXP
                || t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP
                || t == SEXPTYPE::VECSXP
                || t == SEXPTYPE::EXPRSXP
        };
        if !relop_ok(a) || !relop_ok(b) {
            arithmetic_error(format!(
                "comparison ({op_name}) is possible only for atomic and list types"
            ));
        }
        // stock coercion ladder: string involvement compares as
        // character, then complex, then raw/numeric coercion.
        let a_str = TYPEOF(a) == SEXPTYPE::STRSXP;
        let b_str = TYPEOF(b) == SEXPTYPE::STRSXP;
        if a_str || b_str {
            if matches!(SEXPTYPE(TYPEOF(a)), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP)
                || matches!(SEXPTYPE(TYPEOF(b)), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP)
            {
                return super::list_character::compare(op_name, a, b)
                    .unwrap_or_else(|error| arithmetic_error(error));
            }
            let a = if a_str {
                a
            } else {
                relop_operand_to_character(a)
            };
            let b = if b_str {
                b
            } else {
                relop_operand_to_character(b)
            };
            return character_compare(op_name, a, b);
        }
        if TYPEOF(a) == SEXPTYPE::CPLXSXP || TYPEOF(b) == SEXPTYPE::CPLXSXP {
            return complex_relop(op_name, a, b);
        }
        if (TYPEOF(a) == SEXPTYPE::INTSXP
            || TYPEOF(a) == SEXPTYPE::REALSXP
            || TYPEOF(a) == SEXPTYPE::LGLSXP)
            && (TYPEOF(b) == SEXPTYPE::INTSXP
                || TYPEOF(b) == SEXPTYPE::REALSXP
                || TYPEOF(b) == SEXPTYPE::LGLSXP)
        {
            return binary_compare(op_name, a, b);
        }
        if TYPEOF(a) == SEXPTYPE::REALSXP || TYPEOF(b) == SEXPTYPE::REALSXP {
            let a = crate::mainutils::coerce::coerceVector(a, SEXPTYPE::REALSXP.into());
            let _a = protect(a);
            let b = crate::mainutils::coerce::coerceVector(b, SEXPTYPE::REALSXP.into());
            let _b = protect(b);
            return binary_compare(op_name, a, b);
        }
        if TYPEOF(a) == SEXPTYPE::INTSXP || TYPEOF(b) == SEXPTYPE::INTSXP {
            let a = crate::mainutils::coerce::coerceVector(a, SEXPTYPE::INTSXP.into());
            let _a = protect(a);
            let b = crate::mainutils::coerce::coerceVector(b, SEXPTYPE::INTSXP.into());
            let _b = protect(b);
            return binary_compare(op_name, a, b);
        }
        if TYPEOF(a) == SEXPTYPE::LGLSXP || TYPEOF(b) == SEXPTYPE::LGLSXP {
            let a = crate::mainutils::coerce::coerceVector(a, SEXPTYPE::LGLSXP.into());
            let _a = protect(a);
            let b = crate::mainutils::coerce::coerceVector(b, SEXPTYPE::LGLSXP.into());
            let _b = protect(b);
            return binary_compare(op_name, a, b);
        }
        if TYPEOF(a) == SEXPTYPE::RAWSXP || TYPEOF(b) == SEXPTYPE::RAWSXP {
            let a = crate::mainutils::coerce::coerceVector(a, SEXPTYPE::REALSXP.into());
            let _a = protect(a);
            let b = crate::mainutils::coerce::coerceVector(b, SEXPTYPE::REALSXP.into());
            let _b = protect(b);
            return binary_compare(op_name, a, b);
        }
        arithmetic_error("comparison of these types is not implemented");
    }
}

/// Port the comparison branch of GNU R's `Ops.data.frame`.
///
/// Atomic non-scalar operands are laid out column-major across the frame;
/// lists supply one operand per column; scalars are broadcast. Comparisons
/// return a logical matrix rather than leaking the frame's underlying VECSXP.
pub(crate) unsafe fn data_frame_compare(
    op_name: &str,
    call: SEXP,
    lhs: SEXP,
    rhs: SEXP,
) -> Option<SEXP> {
    unsafe {
        let lhs_frame = has_class(lhs, "data.frame") && TYPEOF(lhs) == SEXPTYPE::VECSXP;
        let rhs_frame = has_class(rhs, "data.frame") && TYPEOF(rhs) == SEXPTYPE::VECSXP;
        if !lhs_frame && !rhs_frame {
            return None;
        }

        let frame = if lhs_frame { lhs } else { rhs };
        let ncol = XLENGTH(frame);
        let nrow = crate::mainutils::essentials::data_frame_row_count(frame);

        if lhs_frame
            && rhs_frame
            && (XLENGTH(lhs) != XLENGTH(rhs)
                || crate::mainutils::essentials::data_frame_row_count(lhs)
                    != crate::mainutils::essentials::data_frame_row_count(rhs))
        {
            arithmetic_error(format!(
                "'{op_name}' only defined for equally-sized data frames"
            ));
        }

        validate_data_frame_operand(lhs, lhs_frame, ncol);
        validate_data_frame_operand(rhs, rhs_frame, ncol);

        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, nrow.saturating_mul(ncol));
        let _result_guard = protect(result);
        for column in 0..ncol {
            let (left, _left_guard) = data_frame_column_operand(lhs, lhs_frame, column, nrow);
            let (right, _right_guard) = data_frame_column_operand(rhs, rhs_frame, column, nrow);
            let compared = compare_values(op_name, call, left, right);
            let _compared_guard = protect(compared);
            if TYPEOF(compared) != SEXPTYPE::LGLSXP || XLENGTH(compared) != nrow {
                arithmetic_error("dimension mismatch in data frame comparison");
            }
            for row in 0..nrow {
                SET_LOGICAL_ELT(
                    result,
                    (column * nrow + row) as i32,
                    LOGICAL_ELT(compared, row as i32),
                );
            }
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _dim_guard = protect(dim);
        SET_INTEGER_ELT(dim, 0, nrow as i32);
        SET_INTEGER_ELT(dim, 1, ncol as i32);
        setAttrib(result, R_DimSymbol(), dim);

        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dimnames_guard = protect(dimnames);
        SET_VECTOR_ELT(dimnames, 0, explicit_data_frame_row_names(frame));
        SET_VECTOR_ELT(dimnames, 1, getAttrib(frame, R_NamesSymbol()));
        setAttrib(result, R_DimNamesSymbol(), dimnames);
        Some(result)
    }
}
pub(crate) unsafe fn data_frame_arith(op: &str, sa: SEXP, sb: SEXP) -> Option<SEXP> {
    unsafe {
        let lhs_frame = has_class(sa, "data.frame") && TYPEOF(sa) == SEXPTYPE::VECSXP;
        let rhs_frame = has_class(sb, "data.frame") && TYPEOF(sb) == SEXPTYPE::VECSXP;
        if !lhs_frame && !rhs_frame {
            return None;
        }
        let frame = if lhs_frame { sa } else { sb };
        let ncol = XLENGTH(frame);
        let out = Rf_allocVector3(SEXPTYPE::VECSXP, ncol);
        let _g = protect(out);
        for i in 0..ncol {
            let left = if lhs_frame { VECTOR_ELT(sa, i) } else { sa };
            let right = if rhs_frame { VECTOR_ELT(sb, i) } else { sb };
            let col = real_binary(op, left, right);
            SET_VECTOR_ELT(out, i, col);
        }
        let names = getAttrib(frame, R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            setAttrib(out, R_NamesSymbol(), names);
        }
        let rows = getAttrib(frame, crate::sexp::attrib_core::R_RowNamesSymbol());
        if !rows.is_null() && rows != R_NilValue() {
            setAttrib(out, crate::sexp::attrib_core::R_RowNamesSymbol(), rows);
        }
        setAttrib(
            out,
            crate::sexp::attrib_core::R_ClassSymbol(),
            crate::sexp::constructors::Rf_mkString(c"data.frame".as_ptr()),
        );
        Some(out)
    }
}

pub(crate) unsafe fn validate_data_frame_operand(value: SEXP, is_frame: bool, ncol: R_xlen_t) {
    unsafe {
        if is_frame
            || value == R_NilValue()
            || TYPEOF(value) == SEXPTYPE::NILSXP
            || TYPEOF(value) != SEXPTYPE::VECSXP
        {
            return;
        }
        let len = XLENGTH(value);
        if len > 1 && len != ncol {
            arithmetic_error(format!("list of length {len} not meaningful"));
        }
        if len == 0 {
            arithmetic_error("subscript out of bounds");
        }
    }
}

pub(crate) unsafe fn data_frame_column_operand(
    value: SEXP,
    is_frame: bool,
    column: R_xlen_t,
    nrow: R_xlen_t,
) -> (SEXP, Option<crate::sexp::protect::ProtectGuard<'static>>) {
    unsafe {
        if is_frame {
            return (VECTOR_ELT(value, column), None);
        }
        if value == R_NilValue() || TYPEOF(value) == SEXPTYPE::NILSXP {
            let empty = Rf_allocVector3(SEXPTYPE::LGLSXP, nrow);
            let guard = protect(empty);
            return (empty, Some(guard));
        }
        if TYPEOF(value) == SEXPTYPE::VECSXP {
            let index = if XLENGTH(value) == 1 { 0 } else { column };
            return (VECTOR_ELT(value, index), None);
        }
        if XLENGTH(value) <= 1 {
            return (value, None);
        }

        let segment = recycled_atomic_segment(value, column * nrow, nrow);
        let guard = protect(segment);
        (segment, Some(guard))
    }
}

pub(crate) unsafe fn recycled_atomic_segment(value: SEXP, offset: R_xlen_t, len: R_xlen_t) -> SEXP {
    unsafe {
        let source_len = XLENGTH(value);
        let result = Rf_allocVector3(TYPEOF(value), len);
        if source_len == 0 {
            return result;
        }
        for i in 0..len {
            let source = ((offset + i) % source_len) as i32;
            let target = i as i32;
            match TYPEOF(value) {
                t if t == SEXPTYPE::LGLSXP => {
                    SET_LOGICAL_ELT(result, target, LOGICAL_ELT(value, source))
                }
                t if t == SEXPTYPE::INTSXP => {
                    SET_INTEGER_ELT(result, target, INTEGER_ELT(value, source))
                }
                t if t == SEXPTYPE::REALSXP => {
                    SET_REAL_ELT(result, target, REAL_ELT(value, source))
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    SET_COMPLEX_ELT(result, target, COMPLEX_ELT(value, source))
                }
                t if t == SEXPTYPE::STRSXP => {
                    SET_STRING_ELT(result, i, STRING_ELT(value, source as R_xlen_t))
                }
                t if t == SEXPTYPE::RAWSXP => SET_RAW_ELT(result, target, RAW_ELT(value, source)),
                _ => arithmetic_error("comparison of these types is not implemented"),
            }
        }
        result
    }
}

pub(crate) unsafe fn explicit_data_frame_row_names(frame: SEXP) -> SEXP {
    unsafe {
        // GNU Ops.data.frame keeps `rn <- NULL` unless
        // `.row_names_info(e) > 0`: compact `c(NA, -n)` automatic row
        // names stay NULL. `getAttrib` materializes them to `1:n`, so
        // read the raw stored attribute like `row.names.stored` does.
        let row_names = crate::mainutils::essentials::row_names_stored(frame);
        let compact = !row_names.is_null()
            && row_names != R_NilValue()
            && TYPEOF(row_names) == SEXPTYPE::INTSXP
            && XLENGTH(row_names) == 2
            && INTEGER_ELT(row_names, 0) == NA_INTEGER
            && INTEGER_ELT(row_names, 1) < 0;
        if compact { R_NilValue() } else { row_names }
    }
}

/// Unary plus on a logical is an integer. Integer and real unary plus
/// return the object unchanged.
pub(crate) unsafe fn unary_plus_logical(x: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        let _guard = protect(result);
        for i in 0..n {
            SET_INTEGER_ELT(result, i as i32, LOGICAL_ELT(x, i as i32));
        }
        propagate_unary_vector_attributes(result, x, n);
        if has_class(x, "ts") || has_class(x, "mts") {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                R_NilValue(),
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_TspSymbol(),
                R_NilValue(),
            );
        }
        result
    }
}

/// Unary minus — negate each element of a numeric vector.
pub(crate) unsafe fn unary_minus(x: SEXP) -> SEXP {
    unsafe {
        let Some(input) = NumericVector::from_raw(x) else {
            return R_NilValue();
        };
        let n = input.clone().len();
        let result_type = if input.clone().typeof_() == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        };
        let result_raw = Rf_allocVector3(result_type, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        if result_type == SEXPTYPE::REALSXP {
            for i in 0..n {
                result_mut.set_real_elt(i, -input.clone().real_at(i));
            }
        } else {
            for i in 0..n {
                let value = input.clone().int_at(i);
                let negated = if value == NA_INTEGER {
                    NA_INTEGER
                } else {
                    -value
                };
                result_mut.set_integer_elt(i, negated);
            }
        }
        let _ = result_mut.freeze();
        if TYPEOF(x) == SEXPTYPE::LGLSXP {
            propagate_unary_vector_attributes(result_raw, x, n);
            if has_class(x, "ts") || has_class(x, "mts") {
                crate::sexp::attrib_core::setAttrib(
                    result_raw,
                    crate::sexp::attrib_core::R_ClassSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result_raw,
                    crate::sexp::attrib_core::R_TspSymbol(),
                    R_NilValue(),
                );
            }
        } else {
            copy_all_attrib(result_raw, x);
        }
        result_raw
    }
}

pub(crate) unsafe fn ordered_factor_compare(op: &str, sa: SEXP, sb: SEXP) -> Option<SEXP> {
    unsafe {
        let a_ordered = has_class(sa, "ordered");
        let b_ordered = has_class(sb, "ordered");
        if !a_ordered && !b_ordered {
            return None;
        }

        let levels_owner = if a_ordered { sa } else { sb };
        let levels = factor_levels(levels_owner)?;
        if (a_ordered && !factor_levels_match(sa, &levels))
            || (b_ordered && !factor_levels_match(sb, &levels))
        {
            std::panic::panic_any(RError {
                message: "level sets of factors are different".to_string(),
            });
        }

        let Some(a) = Sexp::from_raw(sa) else {
            return Some(R_NilValue());
        };
        let Some(b) = Sexp::from_raw(sb) else {
            return Some(R_NilValue());
        };
        let a_len = a.len();
        let b_len = b.len();
        let n = match (a_len, b_len) {
            (0, _) | (_, 0) => 0,
            _ => a_len.max(b_len),
        };
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return Some(R_NilValue());
        };
        let _result_guard = protect(result_raw);
        warn_if_non_multiple_recycling(a_len, b_len);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..n {
            let lhs = ordered_operand_code(sa, i % a_len, &levels);
            let rhs = ordered_operand_code(sb, i % b_len, &levels);
            result_mut.set_logical_elt(i, ordered_code_compare(op, lhs, rhs));
        }
        let _ = result_mut.freeze();
        Some(result_raw)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum OrderedOperand {
    Code(i32),
    Missing,
    UnknownLevel,
}

pub(crate) fn ordered_code_compare(op: &str, lhs: OrderedOperand, rhs: OrderedOperand) -> i32 {
    match (lhs, rhs) {
        (OrderedOperand::Missing, _) | (_, OrderedOperand::Missing) => NA_LOGICAL,
        (OrderedOperand::UnknownLevel, _) | (_, OrderedOperand::UnknownLevel) => match op {
            "==" => FALSE,
            "!=" => TRUE,
            _ => NA_LOGICAL,
        },
        (OrderedOperand::Code(lhs), OrderedOperand::Code(rhs)) => match op {
            "<" if lhs < rhs => TRUE,
            ">" if lhs > rhs => TRUE,
            "<=" if lhs <= rhs => TRUE,
            ">=" if lhs >= rhs => TRUE,
            "==" if lhs == rhs => TRUE,
            "!=" if lhs != rhs => TRUE,
            _ => FALSE,
        },
    }
}

pub(crate) unsafe fn ordered_operand_code(
    value: SEXP,
    index: R_xlen_t,
    levels: &[String],
) -> OrderedOperand {
    unsafe {
        if has_class(value, "ordered") {
            let code = INTEGER_ELT(value, index as i32);
            if code == NA_INTEGER || code <= 0 || code as usize > levels.len() {
                OrderedOperand::Missing
            } else {
                OrderedOperand::Code(code)
            }
        } else if TYPEOF(value) == SEXPTYPE::STRSXP {
            let charsxp = STRING_ELT(value, index);
            let Some(label) = charsxp_to_string(charsxp) else {
                return OrderedOperand::Missing;
            };
            levels
                .iter()
                .position(|level| level == &label)
                .map(|position| OrderedOperand::Code(position as i32 + 1))
                .unwrap_or(OrderedOperand::UnknownLevel)
        } else {
            OrderedOperand::UnknownLevel
        }
    }
}

pub(crate) unsafe fn factor_levels_match(value: SEXP, expected: &[String]) -> bool {
    unsafe { factor_levels(value).is_some_and(|levels| levels == expected) }
}

pub(crate) unsafe fn factor_levels(value: SEXP) -> Option<Vec<String>> {
    unsafe {
        let levels = getAttrib(value, Rf_install(c"levels".as_ptr()));
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        let mut out = Vec::with_capacity(LENGTH(levels) as usize);
        for i in 0..LENGTH(levels) as R_xlen_t {
            out.push(charsxp_to_string(STRING_ELT(levels, i))?);
        }
        Some(out)
    }
}

pub(crate) unsafe fn has_class(value: SEXP, class_name: &str) -> bool {
    unsafe {
        let class = getAttrib(value, Rf_install(c"class".as_ptr()));
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..LENGTH(class) as R_xlen_t)
            .any(|i| charsxp_to_string(STRING_ELT(class, i)).as_deref() == Some(class_name))
    }
}

pub(crate) unsafe fn charsxp_to_string(charsxp: SEXP) -> Option<String> {
    unsafe {
        if charsxp.is_null() || charsxp == R_NaString() {
            return None;
        }
        Sexp::from_raw(charsxp).and_then(|s| s.try_as_string().ok())
    }
}

pub(crate) unsafe fn character_compare(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        let Some(a) = Sexp::from_raw(sa) else {
            return R_NilValue();
        };
        let Some(b) = Sexp::from_raw(sb) else {
            return R_NilValue();
        };
        let a_len = a.len();
        let b_len = b.len();
        let n = match (a_len, b_len) {
            (0, _) | (_, 0) => 0,
            _ => a_len.max(b_len),
        };
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        warn_if_non_multiple_recycling(a_len, b_len);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);

        for i in 0..n {
            let ai = i % a_len;
            let bi = i % b_len;
            let ac = STRING_ELT(sa, ai);
            let bc = STRING_ELT(sb, bi);
            let value = if ac.is_null() || bc.is_null() || ac == R_NaString() || bc == R_NaString()
            {
                NA_LOGICAL
            } else if op == "==" {
                if crate::mainutils::relop::Seql(ac, bc) != 0 {
                    TRUE
                } else {
                    FALSE
                }
            } else if op == "!=" {
                if crate::mainutils::relop::Seql(ac, bc) != 0 {
                    FALSE
                } else {
                    TRUE
                }
            } else {
                let av = crate::sexp::accessors::charsxp_as_utf8(ac);
                let bv = crate::sexp::accessors::charsxp_as_utf8(bc);
                let a = String::from_utf8_lossy(&av);
                let b = String::from_utf8_lossy(&bv);
                if compare_strings(op, a.as_ref(), b.as_ref()) {
                    TRUE
                } else {
                    FALSE
                }
            };
            result_mut.set_logical_elt(i, value);
        }
        let _ = result_mut.freeze();

        propagate_binary_vector_attributes(result_raw, sa, sb, n);
        result_raw
    }
}

pub(crate) fn compare_strings(op: &str, a: &str, b: &str) -> bool {
    match op {
        "<" => a < b,
        ">" => a > b,
        "<=" => a <= b,
        ">=" => a >= b,
        "==" => a == b,
        "!=" => a != b,
        _ => false,
    }
}
