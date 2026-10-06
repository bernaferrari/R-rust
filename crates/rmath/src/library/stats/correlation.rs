//! Public GNU `C_cor` admission, missing-row policy and result structure.
//! Based on r-source r90451 src/library/stats/src/cov.c. The original R
//! wrapper performs Spearman ranking before entering this native descriptor.

use crate::sexp::{accessors::*, constructors::*, ffi::*, globals::*, protect::protect};

#[derive(Clone, Copy, PartialEq)]
enum MissingRows {
    All,
    Complete,
    Pairwise,
    Everything,
    OrComplete,
}

impl MissingRows {
    fn from_r(value: SEXP) -> Self {
        match unsafe { crate::mainutils::coerce::asInteger(value) } {
            1 => Self::All,
            2 => Self::Complete,
            3 => Self::Pairwise,
            4 => Self::Everything,
            5 => Self::OrComplete,
            _ => crate::mainutils::essentials::base_error("invalid 'use' (computational method)"),
        }
    }
}

struct Columns {
    values: Vec<f64>,
    rows: usize,
    count: usize,
    matrix: bool,
}

impl Columns {
    unsafe fn read(value: SEXP, owner: crate::sexp::owner::OwnerToken<'_>) -> Self {
        unsafe {
            let dim = crate::attrib_core::getAttrib(value, crate::attrib_core::R_DimSymbol());
            let matrix = TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2;
            let (rows, count) = if matrix {
                (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
            } else {
                (XLENGTH(value) as usize, 1)
            };
            // Copy while the converted vector is protected. No native span is
            // retained across result allocation or a warning callback.
            let view = owner.sexp(value).unwrap_or_else(|error| {
                crate::mainutils::essentials::base_error(error.to_string())
            });
            let values = (0..view.len())
                .map(|index| view.try_real_elt(index))
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_else(|error| {
                    crate::mainutils::essentials::base_error(error.to_string())
                });
            if rows.checked_mul(count) != Some(values.len()) {
                crate::mainutils::essentials::base_error("incompatible dimensions");
            }
            Self {
                values,
                rows,
                count,
                matrix,
            }
        }
    }

    fn column(&self, index: usize) -> &[f64] {
        &self.values[index * self.rows..(index + 1) * self.rows]
    }
}

/// The typed .Call descriptor retains its caller's runtime admission. Original
/// arguments and both converted vectors stay protected through publication and
/// condition delivery; numerical work uses independent copied values.
pub(super) unsafe fn cor(x: SEXP, y: SEXP, na_method: SEXP, kendall: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::mainutils::essentials::base_error(error.to_string()));
        if x.is_null() || x == R_NilValue() {
            crate::mainutils::essentials::base_error("'x' is NULL");
        }
        super::random::reject_var_on_factor(x);
        super::random::reject_var_on_factor(y);
        let x = crate::mainutils::coerce::coerceVector(x, SEXPTYPE::REALSXP.into());
        let _x = protect(x);
        let y_null = y.is_null() || y == R_NilValue();
        let y = if y_null {
            x
        } else {
            crate::mainutils::coerce::coerceVector(y, SEXPTYPE::REALSXP.into())
        };
        let _y = protect(y);
        let xs = Columns::read(x, owner);
        let ys = if y_null {
            None
        } else {
            Some(Columns::read(y, owner))
        };
        let ys = ys.as_ref().unwrap_or(&xs);
        if xs.rows != ys.rows {
            crate::mainutils::essentials::base_error("incompatible dimensions");
        }
        let policy = MissingRows::from_r(na_method);
        let kendall = crate::mainutils::coerce::asBool(kendall);
        let (values, zero_sd) = calculate(&xs, ys, y_null, policy, kendall != 0);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as i64);
        let _ans = protect(ans);
        for (index, value) in values.into_iter().enumerate() {
            SET_REAL_ELT(ans, index as i32, value);
        }
        if xs.matrix || ys.matrix {
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let _dim = protect(dim);
            SET_INTEGER_ELT(dim, 0, xs.count as i32);
            SET_INTEGER_ELT(dim, 1, ys.count as i32);
            crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimSymbol(), dim);
            let names = |value| {
                let dn =
                    crate::attrib_core::getAttrib(value, crate::attrib_core::R_DimNamesSymbol());
                if TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 2 {
                    VECTOR_ELT(dn, 1)
                } else {
                    R_NilValue()
                }
            };
            let xn = names(x);
            let yn = names(y);
            if xn != R_NilValue() || yn != R_NilValue() {
                let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
                let _dn = protect(dn);
                SET_VECTOR_ELT(dn, 0, crate::mainutils::duplicate::duplicate(xn));
                SET_VECTOR_ELT(dn, 1, crate::mainutils::duplicate::duplicate(yn));
                crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimNamesSymbol(), dn);
            }
        }
        if zero_sd {
            crate::mainutils::errors::Rf_warning1(c"the standard deviation is zero".as_ptr());
        }
        ans
    }
}

fn calculate(
    x: &Columns,
    y: &Columns,
    self_matrix: bool,
    policy: MissingRows,
    kendall: bool,
) -> (Vec<f64>, bool) {
    if x.values.is_empty() {
        match policy {
            MissingRows::Complete => {
                crate::mainutils::essentials::base_error("no complete element pairs")
            }
            MissingRows::All | MissingRows::Pairwise => {
                crate::mainutils::essentials::base_error("'x' is empty")
            }
            _ => {}
        }
    }
    let complete: Vec<bool> = (0..x.rows)
        .map(|r| {
            (0..x.count).all(|c| !x.column(c)[r].is_nan())
                && (0..y.count).all(|c| !y.column(c)[r].is_nan())
        })
        .collect();
    if policy == MissingRows::All && complete.iter().any(|ok| !ok) {
        crate::mainutils::essentials::base_error("missing observations in cov/cor");
    }
    if matches!(policy, MissingRows::All | MissingRows::Complete) && !complete.iter().any(|ok| *ok)
    {
        crate::mainutils::essentials::base_error("no complete element pairs");
    }
    let length = x.count.checked_mul(y.count).unwrap_or_else(|| {
        crate::mainutils::essentials::base_error("correlation matrix is too large")
    });
    let mut ans = vec![NA_REAL; length];
    let mut zero_sd = false;
    for j in 0..y.count {
        for i in 0..x.count {
            let xx = x.column(i);
            let yy = y.column(j);
            let rows: Vec<usize> = (0..x.rows)
                .filter(|&r| match policy {
                    MissingRows::Pairwise => !xx[r].is_nan() && !yy[r].is_nan(),
                    MissingRows::Everything => true,
                    _ => complete[r],
                })
                .collect();
            if rows.len() < 2 {
                continue;
            }
            // GNU's non-pairwise self-matrix path sets each diagonal to one,
            // including constant/NA columns, after its n>1 admission.
            if self_matrix && policy != MissingRows::Pairwise && i == j {
                ans[i + j * x.count] = 1.0;
                continue;
            }
            if policy == MissingRows::Everything
                && rows.iter().any(|&r| xx[r].is_nan() || yy[r].is_nan())
            {
                continue;
            }
            let (value, zero) = pair(xx, yy, &rows, kendall, policy == MissingRows::Pairwise);
            ans[i + j * x.count] = value;
            zero_sd |= zero;
        }
    }
    (ans, zero_sd)
}

fn pair(x: &[f64], y: &[f64], rows: &[usize], kendall: bool, pairwise: bool) -> (f64, bool) {
    let (mut sum, mut vx, mut vy) = (0.0, 0.0, 0.0);
    if kendall {
        let sign = |a: f64, b: f64| {
            if a > b {
                1.0
            } else if a < b {
                -1.0
            } else {
                0.0
            }
        };
        for (k, &r) in rows.iter().enumerate() {
            for &s in &rows[..k] {
                let dx = sign(x[r], x[s]);
                let dy = sign(y[r], y[s]);
                sum += dx * dy;
                vx += dx * dx;
                vy += dy * dy;
            }
        }
    } else {
        let mean = |data: &[f64]| {
            let mut m = rows.iter().map(|&r| data[r]).sum::<f64>() / rows.len() as f64;
            // GNU complete/everything mean corrects finite first-pass means.
            if !pairwise && m.is_finite() {
                m += rows.iter().map(|&r| data[r] - m).sum::<f64>() / rows.len() as f64;
            }
            m
        };
        let mx = mean(x);
        let my = mean(y);
        for &r in rows {
            let dx = x[r] - mx;
            let dy = y[r] - my;
            sum += dx * dy;
            vx += dx * dx;
            vy += dy * dy;
        }
        let n1 = (rows.len() - 1) as f64;
        sum /= n1;
        vx /= n1;
        vy /= n1;
    }
    if vx == 0.0 || vy == 0.0 {
        (NA_REAL, true)
    } else {
        ((sum / (vx.sqrt() * vy.sqrt())).clamp(-1.0, 1.0), false)
    }
}

#[cfg(test)]
mod tests {
    use crate::sexp::{RSession, SEXPTYPE, Sexp, SexpMut};

    fn integers(session: &RSession, values: &[i32]) -> Sexp<'static> {
        let value = session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| {
                arena
                    .alloc_vector_sexp(SEXPTYPE::INTSXP, values.len() as i64)
                    .map(|value| value.as_raw())
            })
            .unwrap()
            .into_owned()
            .unwrap();
        let mut value = SexpMut::try_from_checked(value).unwrap();
        for (index, &element) in values.iter().enumerate() {
            value.try_set_integer_elt(index as i64, element).unwrap();
        }
        value.freeze()
    }

    #[test]
    fn native_cor_keeps_both_coercions_live_through_torture_publication() {
        let mut session = RSession::new_for_gc_tests();
        session
            .eval_code_with_output_capture("gctorture(TRUE)")
            .0
            .unwrap();
        let x = integers(&session, &[1, 2, 3]);
        let y = integers(&session, &[2, 4, 6]);
        let mode = integers(&session, &[4]);
        let kendall = integers(&session, &[0]);
        let result = session.with_active(|| unsafe {
            let raw = super::cor(x.as_raw(), y.as_raw(), mode.as_raw(), kendall.as_raw());
            session
                .owner_token()
                .unwrap()
                .sexp(raw)
                .unwrap()
                .into_owned()
                .unwrap()
        });
        drop((x, y, mode, kendall));
        session.owner_token().unwrap().full_gc().unwrap();
        assert_eq!(result.try_real_elt(0), Ok(1.0));
        session
            .eval_code_with_output_capture("gctorture(FALSE)")
            .0
            .unwrap();
    }
}
