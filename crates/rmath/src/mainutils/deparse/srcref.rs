#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// src2buff1 — deparse one source reference to buffer
// ---------------------------------------------------------------------------

/// Deparse one source reference to the buffer.
///
/// Unimplemented: requires R_AsCharacterSymbol and eval infrastructure.
pub unsafe fn src2buff1(_srcref: SEXP, _d: *mut LocalParseData) {
    // requires eval/R_AsCharacterSymbol
}

// ---------------------------------------------------------------------------
// src2buff — deparse source element k to buffer
// ---------------------------------------------------------------------------

/// Deparse source element k to buffer if possible. Returns false on failure.
pub unsafe fn src2buff(sv: SEXP, k: c_int, d: *mut LocalParseData) -> bool {
    unsafe {
        if !sv.is_null() && TYPEOF(sv) == SEXPTYPE::VECSXP && LENGTH(sv) > k {
            let t = VECTOR_ELT(sv, k as R_xlen_t);
            if !isNull(t) {
                src2buff1(t, d);
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
