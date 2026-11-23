//! Hierarchical clustering utilities: cutree
//! Port of r-source/src/library/stats/src/hclust-utils.c

use super::checked::{checked, current_owner};
use crate::sexp::accessors::{INTEGER_ELT, REAL_ELT, SET_INTEGER_ELT, SET_REAL_ELT};
use std::os::raw::c_int;

use crate::main::coerce::coerceVector;
use crate::main::util_main::nrows;
use crate::sexp::attrib_core::{R_DimSymbol, setAttrib};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::context::r_error;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::object::SexpMut;
use crate::sexp::protect::protect as protect_sexp;

pub fn cutree(merge: SEXP, which: SEXP) -> SEXP {
    let owner = current_owner();
    let merge_input = checked(owner.sexp(merge));
    let which_input = checked(owner.sexp(which));
    let merge = checked(owner.sexp(coerceVector(
        merge_input.as_raw(),
        SEXPTYPE::INTSXP.as_c_int(),
    )));
    checked(owner.require_active());
    let which = checked(owner.sexp(coerceVector(
        which_input.as_raw(),
        SEXPTYPE::INTSXP.as_c_int(),
    )));
    checked(owner.require_active());
    let rows = nrows(merge.as_raw().cast());
    checked(owner.require_active());
    if rows < 0 || merge.len() != 2 * R_xlen_t::from(rows) {
        r_error("invalid 'tree' ('merge' component)");
    }
    let n = rows
        .checked_add(1)
        .unwrap_or_else(|| r_error("too many leaves in 'tree'"));
    let which_len = c_int::try_from(which.len()).unwrap_or_else(|_| r_error("too many cuts"));

    let ans_len = R_xlen_t::from(n) * R_xlen_t::from(which_len);
    let ans = checked(owner.sexp(Rf_allocVector3(SEXPTYPE::INTSXP, ans_len)));
    checked(owner.require_active());
    let dim = checked(owner.sexp(Rf_allocVector3(SEXPTYPE::INTSXP, 2)));
    let mut dim_out = checked(SexpMut::try_from_checked(dim.clone()));
    checked(dim_out.try_set_integer_elt(0, n));
    checked(dim_out.try_set_integer_elt(1, which_len));
    checked(owner.require_active());
    setAttrib(ans.as_raw(), R_DimSymbol(), dim.as_raw());
    let mut out = checked(SexpMut::try_from_checked(ans.clone()));

    // Read each requested cut once, even for an ALTREP provider that can run GC.
    let cut_bytes = (which_len as usize)
        .checked_mul(std::mem::size_of::<c_int>())
        .unwrap_or_else(|| r_error("cannot allocate cutree workspace"));
    let _cut_budget = if cut_bytes == 0 {
        None
    } else {
        Some(
            checked(owner.with_arena(|arena| arena.reserve_workspace(cut_bytes)))
                .unwrap_or_else(|| r_error("cannot allocate cutree workspace")),
        )
    };
    let mut cuts = Vec::new();
    cuts.try_reserve_exact(which_len as usize)
        .unwrap_or_else(|_| r_error("cannot allocate cutree workspace"));
    for i in 0..which.len() {
        cuts.push(checked(which.try_integer_elt(i)));
    }
    let n = n as usize;
    let mut m_nr = vec![0_i32; n + 1];
    let mut z = vec![0; n + 1];
    let column_start = |column: usize| (column as R_xlen_t) * (n as R_xlen_t);

    for k in 1..n {
        let i = (k - 1) as R_xlen_t;
        let m1 = checked(merge.try_integer_elt(i));
        let m2 = checked(merge.try_integer_elt(R_xlen_t::from(rows) + i));
        let valid = |m: c_int| {
            if m < 0 {
                m.unsigned_abs() as usize <= n
            } else {
                m > 0 && (m as usize) < k
            }
        };
        if !valid(m1) || !valid(m2) {
            r_error("invalid 'tree' ('merge' component)");
        }
        if m1 < 0 {
            m_nr[m1.unsigned_abs() as usize] = k as c_int;
        }
        if m2 < 0 {
            m_nr[m2.unsigned_abs() as usize] = k as c_int;
        }
        if m1 > 0 || m2 > 0 {
            for branch in &mut m_nr[1..] {
                if (m1 > 0 && *branch == m1) || (m2 > 0 && *branch == m2) {
                    *branch = k as c_int;
                }
            }
        }

        let mut first_column = None;
        for (column, &cut) in cuts.iter().enumerate() {
            if cut != (n - k) as c_int {
                continue;
            }
            let start = column_start(column);
            if let Some(first) = first_column {
                for i in 0..n as R_xlen_t {
                    let value = checked(ans.try_integer_elt(first + i));
                    checked(out.try_set_integer_elt(start + i, value));
                }
            } else {
                first_column = Some(start);
                z.fill(0);
                let mut nclust = 0;
                for (i, &branch) in m_nr[1..].iter().enumerate() {
                    let label = if branch == 0 {
                        nclust += 1;
                        nclust
                    } else {
                        let branch = branch as usize;
                        if z[branch] == 0 {
                            nclust += 1;
                            z[branch] = nclust;
                        }
                        z[branch]
                    };
                    checked(out.try_set_integer_elt(start + i as R_xlen_t, label));
                }
            }
        }
    }

    for (column, &cut) in cuts.iter().enumerate() {
        if cut == n as c_int {
            let start = column_start(column);
            for i in 0..n {
                checked(out.try_set_integer_elt(start + i as R_xlen_t, (i + 1) as c_int));
            }
        }
    }
    checked(owner.require_active());
    ans.as_raw()
}
pub unsafe extern "C-unwind" fn c_cutree(merge: SEXP, which: SEXP) -> SEXP {
    cutree(merge, which)
}

fn list_elt(list: SEXP, name: &str) -> SEXP {
    use crate::sexp::accessors::{STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH};
    use crate::sexp::globals::R_NilValue;
    if list.is_null() || list == R_NilValue() || TYPEOF(list) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let names =
        crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
    if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
        return R_NilValue();
    }
    for i in 0..XLENGTH(names) {
        let s = STRING_ELT(names, i);
        if s.is_null() {
            continue;
        }
        let raw = s;
        if raw.is_null() {
            continue;
        }
        if crate::library::stats::checked::char_string(raw) == name {
            return VECTOR_ELT(list, i);
        }
    }
    R_NilValue()
}

/// GNU `cutree(tree, k)` / `cutree(tree, h)`.
pub fn do_cutree(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::accessors::CHAR;
    use crate::sexp::accessors::PRINTNAME;
    use crate::sexp::accessors::{CAR, CDR, INTEGER, REAL, SET_VECTOR_ELT, TAG, TYPEOF, XLENGTH};
    use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector3};
    use crate::sexp::ffi::NA_REAL;
    use crate::sexp::globals::R_NilValue;

    let mut tree = R_NilValue();
    let mut k = NA_REAL;
    let mut h = NA_REAL;
    let mut have_k = false;
    let mut have_h = false;
    let mut cell = args;
    let mut pos = 0;
    while !cell.is_null() && cell != R_NilValue() {
        let v = CAR(cell);
        let tag = TAG(cell);
        let name = if !tag.is_null() && tag != R_NilValue() {
            crate::library::stats::checked::char_string(PRINTNAME(tag))
        } else {
            String::new()
        };
        match name.as_str() {
            "tree" => tree = v,
            "k" => {
                if !v.is_null() && v != R_NilValue() {
                    k = if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                        INTEGER_ELT(v, 0) as f64
                    } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                        REAL_ELT(v, 0)
                    } else {
                        NA_REAL
                    };
                    have_k = k.is_finite();
                }
            }
            "h" => {
                if !v.is_null() && v != R_NilValue() {
                    h = if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                        REAL_ELT(v, 0)
                    } else if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                        INTEGER_ELT(v, 0) as f64
                    } else {
                        NA_REAL
                    };
                    have_h = h.is_finite();
                }
            }
            _ if name.is_empty() => {
                if pos == 0 {
                    tree = v;
                } else if pos == 1 && !have_k && !have_h {
                    if !v.is_null() && v != R_NilValue() {
                        k = if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                            INTEGER_ELT(v, 0) as f64
                        } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                            REAL_ELT(v, 0)
                        } else {
                            NA_REAL
                        };
                        have_k = k.is_finite();
                    }
                }
                pos += 1;
            }
            _ => {}
        }
        cell = CDR(cell);
    }
    let merge = list_elt(tree, "merge");
    if merge.is_null() || merge == R_NilValue() {
        return R_NilValue();
    }
    let n = crate::library::stats::checked::nrows(merge) + 1;
    let mut which_k = if have_k { k.round() as i32 } else { 0 };
    if !have_k && have_h {
        let height = list_elt(tree, "height");
        let nmerge = n - 1;
        let mut first = nmerge + 1; // Inf
        if !height.is_null() && height != R_NilValue() {
            for i in 0..nmerge {
                let hi = if TYPEOF(height) == SEXPTYPE::REALSXP {
                    REAL_ELT(height, i as i32)
                } else {
                    INTEGER_ELT(height, i as i32) as f64
                };
                if hi > h {
                    first = i + 1;
                    break;
                }
            }
        }
        which_k = n + 1 - first;
    }
    if which_k < 1 {
        which_k = 1;
    }
    if which_k > n {
        which_k = n;
    }
    let ksexp = Rf_ScalarInteger(which_k);
    let _ks = protect_sexp(ksexp);
    let ans = cutree(merge, ksexp);
    let _ans = protect_sexp(ans);
    let out = Rf_allocVector3(SEXPTYPE::INTSXP, n as i64);
    let _o = protect_sexp(out);
    for i in 0..n {
        SET_INTEGER_ELT(out, i as i32, INTEGER_ELT(ans, i as i32));
    }
    let _ = SET_VECTOR_ELT;
    out
}

pub unsafe extern "C-unwind" fn c_mono_fc_m(m: SEXP, sx: SEXP) -> SEXP {
    let n = crate::sexp::accessors::XLENGTH(m);
    if n < 2 {
        crate::main::errors::Rf_error_cstr(c"length(m) must be at least two");
    }
    if crate::sexp::accessors::TYPEOF(sx) != SEXPTYPE::REALSXP
        || crate::sexp::accessors::XLENGTH(sx) != n - 1
    {
        crate::main::errors::Rf_error_cstr(
            c"Argument Sx must be numeric vector one shorter than m[]",
        );
    }
    let val = crate::mainutils::seq::Rf_duplicate(m);
    let _g = protect_sexp(val);
    for k in 0..(n - 1) as usize {
        let sk = REAL_ELT(sx, k as i32);
        if sk == 0.0 {
            SET_REAL_ELT(val, k as i32, 0.0);
            SET_REAL_ELT(val, (k + 1) as i32, 0.0);
        } else {
            let alpha = REAL_ELT(val, k as i32) / sk;
            let beta = REAL_ELT(val, (k + 1) as i32) / sk;
            let a2b3 = 2.0 * alpha + beta - 3.0;
            let ab23 = alpha + 2.0 * beta - 3.0;
            if a2b3 > 0.0 && ab23 > 0.0 && alpha * (a2b3 + ab23) < a2b3 * a2b3 {
                let tau_s = 3.0 * sk / (alpha * alpha + beta * beta).sqrt();
                SET_REAL_ELT(val, k as i32, tau_s * alpha);
                SET_REAL_ELT(val, (k + 1) as i32, tau_s * beta);
            }
        }
    }
    val
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::altrep::{
        AltrepBuilder, AltrepClass, AltrepContext, AltrepElement, is_materialized,
    };
    use crate::sexp::{object::SexpResult, session::RSession};
    use std::{cell::Cell, rc::Rc};

    struct CollectingCuts(Rc<Cell<usize>>);

    impl AltrepClass for CollectingCuts {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::INTSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
            Ok(3)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            i: R_xlen_t,
        ) -> SexpResult<AltrepElement<'s>> {
            self.0.set(self.0.get() + 1);
            context.gc()?;
            Ok(AltrepElement::Integer([2, 1, 1][i as usize]))
        }
    }

    #[test]
    fn cutree_keeps_collecting_altrep_cuts_lazy() {
        let session = RSession::new_for_gc_tests();
        let reads = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class("collecting-cutree-cuts", CollectingCuts(reads.clone()))
            .unwrap();
        let cuts = AltrepBuilder::new(class).build().unwrap();
        session.with_active(|| {
            let merge = checked(current_owner().sexp(Rf_allocVector3(SEXPTYPE::INTSXP, 2)));
            let mut out = checked(SexpMut::try_from_checked(merge.clone()));
            checked(out.try_set_integer_elt(0, -1));
            checked(out.try_set_integer_elt(1, -2));
            let dims = checked(current_owner().sexp(Rf_allocVector3(SEXPTYPE::INTSXP, 2)));
            let mut out = checked(SexpMut::try_from_checked(dims.clone()));
            checked(out.try_set_integer_elt(0, 1));
            checked(out.try_set_integer_elt(1, 2));
            setAttrib(merge.as_raw(), R_DimSymbol(), dims.as_raw());
            let result = session.sexp(cutree(merge.as_raw(), cuts.as_raw())).unwrap();
            assert_eq!(
                super::super::checked::int_vec(result.as_raw()),
                [1, 2, 1, 1, 1, 1]
            );
            assert!(!is_materialized(&cuts));
        });
        assert_eq!(reads.get(), 3);
    }
}
