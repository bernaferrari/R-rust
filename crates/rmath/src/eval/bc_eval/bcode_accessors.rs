//! BCODE_* accessors and constant-pool snapshot helpers — extracted verbatim from eval/bc_eval.rs

use super::*;

// ---------------------------------------------------------------------------
// BCODESXP accessors
// ---------------------------------------------------------------------------

/// Get the bytecode instruction array from a BCODESXP.
pub unsafe fn BCODE_CODE(x: SEXP) -> *const c_int {
    unsafe {
        if x.is_null() {
            return ptr::null();
        }
        // The code is stored as the first element of the vector
        let v = crate::sexp::accessors::VECTOR_ELT(x, 0);
        if v.is_null() {
            return ptr::null();
        }
        INTEGER(v) as *const c_int
    }
}

/// Get the constant pool from a BCODESXP.
pub unsafe fn BCODE_CONSTS(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return R_NilValue();
        }
        // Constants are the second element
        crate::sexp::accessors::VECTOR_ELT(x, 1)
    }
}

/// Whether this BCODESXP was built by the GNU R deserialization adapter.
/// The marker is an owned internal payload slot, never a guessed opcode.
pub unsafe fn BCODE_IS_GNU(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::BCODESXP || (*x).data.vector().length < 4 {
            return false;
        }
        let marker = VECTOR_ELT(x, 3);
        !marker.is_null()
            && TYPEOF(marker) == SEXPTYPE::INTSXP
            && LENGTH(marker) == 1
            && *INTEGER(marker) == super::super::bytecode::GNU_BC_DIALECT_MARKER
    }
}

/// Get the source expression stored in bytecode constant slot 0.
pub unsafe fn BCODE_EXPR(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::BCODESXP {
            return R_NilValue();
        }
        if BCODE_IS_GNU(x) && (*x).data.vector().length >= 5 {
            let source = VECTOR_ELT(x, 4);
            if !source.is_null() {
                return source;
            }
        }
        let consts = BCODE_CONSTS(x);
        if consts.is_null() || TYPEOF(consts) != SEXPTYPE::VECSXP || LENGTH(consts) == 0 {
            return R_NilValue();
        }
        VECTOR_ELT(consts, 0)
    }
}

pub(crate) fn gnu_plain_numeric(x: SEXP) -> bool {
    unsafe {
        let ty = TYPEOF(x);
        ty == SEXPTYPE::LGLSXP || ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::REALSXP
    }
}

/// Non-object `LOGBASE` follows GNU `math2`: a zero-length numeric side
/// yields `numeric(0)`, with attributes taken from `x` only when `x` is
/// empty. Objects and complex values keep the `do_math1` path.
pub(crate) unsafe fn gnu_logbase_empty_result(value: SEXP, base: SEXP) -> Option<SEXP> {
    unsafe {
        if crate::sexp::accessors::OBJECT(value) != 0 || crate::sexp::accessors::OBJECT(base) != 0 {
            return None;
        }
        let n_value = crate::sexp::accessors::XLENGTH(value);
        let n_base = crate::sexp::accessors::XLENGTH(base);
        if n_value != 0 && n_base != 0 {
            return None;
        }
        if TYPEOF(value) == SEXPTYPE::CPLXSXP || TYPEOF(base) == SEXPTYPE::CPLXSXP {
            return None;
        }
        if !gnu_plain_numeric(value) || !gnu_plain_numeric(base) {
            bc_error("non-numeric argument to mathematical function");
        }
        let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        let _empty = own_operand(empty);
        if n_value == 0 {
            crate::mainutils::coerce::SHALLOW_DUPLICATE_ATTRIB(empty, value);
        }
        Some(empty)
    }
}

/// GNU `math1` / `do_math1` always return real for `log`, `floor`, and
/// `ceiling`. The shared dispatcher may narrow an integer or logical input
/// to `INTSXP` when every image fits in an int, including length 0.
pub(crate) unsafe fn gnu_math1_real_result(result: SEXP) -> SEXP {
    unsafe {
        if result.is_null() || result == R_NilValue() || TYPEOF(result) != SEXPTYPE::INTSXP {
            return result;
        }
        let _keep = own_operand(result);
        let n = crate::sexp::accessors::XLENGTH(result);
        let out = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _out = own_operand(out);
        if n > 0 {
            let src = INTEGER(result);
            let dst = REAL(out);
            for i in 0..n as usize {
                let iv = *src.add(i);
                *dst.add(i) = if iv == crate::sexp::ffi::NA_INTEGER {
                    crate::sexp::ffi::NA_REAL
                } else {
                    iv as f64
                };
            }
        }
        crate::mainutils::coerce::SHALLOW_DUPLICATE_ATTRIB(out, result);
        out
    }
}

pub(crate) fn owned_constant_snapshot(pool: &Sexp<'static>) -> Vec<Sexp<'static>> {
    if pool.typeof_() != SEXPTYPE::VECSXP && pool.typeof_() != SEXPTYPE::EXPRSXP {
        bc_error("bytecode requires a vector constant pool");
    }
    let length = usize::try_from(pool.len())
        .unwrap_or_else(|_| bc_error("invalid bytecode constant length"));
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .unwrap_or_else(|_| bc_error("cannot reserve bytecode constants"));
    for index in 0..length {
        values.push(
            pool.try_vector_elt(index as i64)
                .and_then(Sexp::into_owned)
                .unwrap_or_else(|error| bc_error(error.to_string())),
        );
    }
    values
}

pub(crate) fn owned_constant_at(constants: &[Sexp<'static>], index: i64, context: &str) -> SEXP {
    usize::try_from(index)
        .ok()
        .and_then(|index| constants.get(index))
        .map(Sexp::as_raw)
        .unwrap_or_else(|| {
            bc_error(format!(
                "{context} constant index {index} out of range for pool length {}",
                constants.len()
            ))
        })
}

pub(crate) fn owned_instruction_words(code: &Sexp<'static>) -> Vec<c_int> {
    if code.typeof_() != SEXPTYPE::INTSXP {
        bc_error("bytecode instruction stream must be integer");
    }
    let length = usize::try_from(code.len())
        .unwrap_or_else(|_| bc_error("invalid bytecode instruction length"));
    let mut words = Vec::new();
    words
        .try_reserve_exact(length)
        .unwrap_or_else(|_| bc_error("cannot reserve bytecode instruction stream"));
    for index in 0..length {
        words.push(
            code.integer_elt(index as i64)
                .unwrap_or_else(|| bc_error("missing bytecode instruction")),
        );
    }
    words
}
