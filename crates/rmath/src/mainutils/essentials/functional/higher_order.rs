//! Compact/keep/discard/append/flatten higher-order helpers.

use super::*;

/// R's `is.atomic(x)` — TRUE for non-recursive types (not list, pairlist, etc.).
pub fn do_is_atomic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let is_atomic = t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::CHARSXP;
    Rf_ScalarLogical(if is_atomic { TRUE } else { FALSE })
}

/// R's `is.recursive(x)` — TRUE for recursive types (list, pairlist, language, etc.).
pub fn do_is_recursive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let is_rec = t == SEXPTYPE::VECSXP
        || t == SEXPTYPE::LISTSXP
        || t == SEXPTYPE::LANGSXP
        || t == SEXPTYPE::CLOSXP
        || t == SEXPTYPE::BUILTINSXP
        || t == SEXPTYPE::SPECIALSXP
        || t == SEXPTYPE::ENVSXP
        || t == SEXPTYPE::EXPRSXP;
    Rf_ScalarLogical(if is_rec { TRUE } else { FALSE })
}

/// R's `is.object(x)` — TRUE if x has a "class" attribute.
pub fn do_is_object(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let class = crate::sexp::attrib_core::getAttrib(x, install_str("class"));
    Rf_ScalarLogical(if !class.is_null() && class != R_NilValue() {
        TRUE
    } else {
        FALSE
    })
}

// ---------------------------------------------------------------------------
// List operations
// ---------------------------------------------------------------------------






// ---------------------------------------------------------------------------
// Complete list/data.frame — checking
// ---------------------------------------------------------------------------

/// R's `is.data.frame(x)` — check if x has "data.frame" class.
pub fn do_is_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let class = crate::sexp::attrib_core::getAttrib(x, install_str("class"));
    if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP && XLENGTH(class) > 0 {
        let cls = elt_to_string(class, 0);
        return Rf_ScalarLogical(if cls == "data.frame" { TRUE } else { FALSE });
    }
    Rf_ScalarLogical(FALSE)
}

// ---------------------------------------------------------------------------
// Complete list operations — modifyList, splice, flatten, split, melt, cast
// ---------------------------------------------------------------------------

/// R's `modifyList(old, new)` — merge new into old (simplified: shallow merge).
pub fn do_modify_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let old = CAR(args);
    let new_list = CAR(CDR(args));
    if old.is_null() || old == R_NilValue() {
        return new_list;
    }
    if new_list.is_null() || new_list == R_NilValue() {
        return old;
    }
    // Simplified: if both are VECSXP, return new_list (shallow overlay)
    let t_old = TYPEOF(old);
    let t_new = TYPEOF(new_list);
    if t_old == SEXPTYPE::VECSXP && t_new == SEXPTYPE::VECSXP {
        // Return a copy of old with elements from new overlaid
        let n_old = XLENGTH(old);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n_old);
        if result.is_null() {
            return new_list;
        }
        let _p = protect(result);
        for i in 0..n_old {
            let elem = VECTOR_ELT(old, i);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i, elem);
        }
        // Overlay elements from new (simplified: by index)
        let n_new = XLENGTH(new_list);
        for i in 0..n_new.min(n_old) {
            let elem = VECTOR_ELT(new_list, i);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i, elem);
        }
        return result;
    }
    new_list
}


