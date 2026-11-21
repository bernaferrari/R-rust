//! Burg's algorithm for AR model estimation
//! Port of r-source/src/library/stats/src/burg.c

use std::os::raw::{c_double, c_int};

use crate::mainutils::native_routines::buffers::{BufferError, NativeBuffer};

use crate::main::coerce::{asInteger, coerceVector};
use crate::mainutils::errors::{Rf_error, Rf_error_cstr};
use crate::sexp::accessors::{LENGTH, SET_VECTOR_ELT};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::object::SexpMut;
use crate::sexp::protect::protect;

fn checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()))
}

fn burg_values(
    x: &[c_double],
    pmax: usize,
    coefs: &mut [c_double],
    var1: &mut [c_double],
    var2: &mut [c_double],
) {
    let n = x.len();
    let mut u = vec![0.0; n];
    let mut v = vec![0.0; n];
    let mut u0 = vec![0.0; n];

    coefs.fill(0.0);

    let mut sum = 0.0;
    for (t, value) in x.iter().rev().enumerate() {
        u[t] = *value;
        v[t] = *value;
    }

    for value in x {
        sum += value * value;
    }
    var1[0] = sum / n as c_double;
    var2[0] = var1[0];

    for p in 1..=pmax {
        sum = 0.0;
        let mut d = 0.0;
        for t in p..n {
            sum += v[t] * u[t - 1];
            d += v[t] * v[t] + u[t - 1] * u[t - 1];
        }
        let phii = 2.0 * sum / d;
        coefs[pmax * (p - 1) + (p - 1)] = phii;
        if p > 1 {
            for j in 1..p {
                coefs[p - 1 + pmax * (j - 1)] =
                    coefs[p - 2 + pmax * (j - 1)] - phii * coefs[p - 2 + pmax * (p - j - 1)];
            }
        }
        u0.copy_from_slice(&u);
        for t in p..n {
            u[t] = u0[t - 1] - phii * v[t];
            v[t] -= phii * u0[t - 1];
        }
        var1[p] = var1[p - 1] * (1.0 - phii * phii);
        let mut d = 0.0;
        for t in p..n {
            d += v[t] * v[t] + u[t] * u[t];
        }
        var2[p] = d / (2.0 * (n - p) as c_double);
    }
}

fn error(msg: &'static [u8]) -> ! {
    Rf_error_cstr(crate::mainutils::errors::cstr_until_nul(msg));

    unreachable!("Rf_error returned");
}

fn non_negative_usize(value: c_int, name: &'static [u8]) -> usize {
    if value < 0 {
        error(name);
    }
    value as usize
}

pub fn Burg(x: SEXP, order: SEXP) -> SEXP {
    checked(crate::sexp::owner::OwnerToken::with_current(|owner| {
        let x = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
        let _x_guard = protect(x);
        let n = LENGTH(x);
        let pmax = asInteger(order);
        let pmax_usize = non_negative_usize(pmax, b"'order.max' must be non-negative\0");
        if n <= pmax {
            error(b"'order.max' must be smaller than the number of observations\0");
        }

        let coefs = Rf_allocVector(SEXPTYPE::REALSXP, pmax * pmax);
        let _coefs_guard = protect(coefs);
        let var1 = Rf_allocVector(SEXPTYPE::REALSXP, pmax + 1);
        let _var1_guard = protect(var1);
        let var2 = Rf_allocVector(SEXPTYPE::REALSXP, pmax + 1);
        let _var2_guard = protect(var2);

        let x_value = checked(owner.sexp(x));
        let x_values: Vec<c_double> = (0..x_value.len())
            .map(|i| checked(x_value.try_real_elt(i)))
            .collect();
        let mut coefs_values = vec![0.0; pmax_usize * pmax_usize];
        let mut var1_values = vec![0.0; pmax_usize + 1];
        let mut var2_values = vec![0.0; pmax_usize + 1];
        burg_values(
            &x_values,
            pmax_usize,
            &mut coefs_values,
            &mut var1_values,
            &mut var2_values,
        );
        let mut coefs_out = checked(SexpMut::try_from_checked(checked(owner.sexp(coefs))));
        for (i, &v) in coefs_values.iter().enumerate() {
            checked(coefs_out.try_set_real_elt(i as R_xlen_t, v));
        }
        let mut var1_out = checked(SexpMut::try_from_checked(checked(owner.sexp(var1))));
        for (i, &v) in var1_values.iter().enumerate() {
            checked(var1_out.try_set_real_elt(i as R_xlen_t, v));
        }
        let mut var2_out = checked(SexpMut::try_from_checked(checked(owner.sexp(var2))));
        for (i, &v) in var2_values.iter().enumerate() {
            checked(var2_out.try_set_real_elt(i as R_xlen_t, v));
        }

        let ans = Rf_allocVector(SEXPTYPE::VECSXP, 3);
        let _ans_guard = protect(ans);

        SET_VECTOR_ELT(ans, 0, coefs);
        SET_VECTOR_ELT(ans, 1, var1);
        SET_VECTOR_ELT(ans, 2, var2);

        ans
    }))
}

/// `.Fortran(C_eureka, lr, r, g, f, var, a)` over admitted buffers.
pub(crate) fn eureka_owned(buffers: &mut [NativeBuffer]) -> Result<(), BufferError> {
    use NativeBuffer::{Integer, Real};
    let [Integer(lr), Real(r), Real(g), Real(f), Real(var), Real(a)] = buffers else {
        return Err(BufferError::new("invalid eureka buffers"));
    };
    let lr = lr[0] as usize;
    if lr != 0 {
        eureka(lr, r, g, f, var, a);
    }
    Ok(())
}

fn eureka(lr: usize, r: &[f64], g: &[f64], f: &mut [f64], varp: &mut [f64], a: &mut [f64]) {
    let at = |row: usize, col: usize| (col - 1) * lr + (row - 1);
    let mut v = r[0];
    let mut d = r[1];
    a[0] = 1.0;
    f[0] = g[1] / v;
    let mut q = f[0] * r[1];
    varp[0] = (1.0 - f[0] * f[0]) * r[0];
    if lr == 1 {
        return;
    }
    for l in 2..=lr {
        a[l - 1] = -d / v;
        if l > 2 {
            let l1 = (l - 2) / 2;
            let l2 = l1 + 1;
            for j in 2..=l2 {
                let hold = a[j - 1];
                let k = l - j + 1;
                a[j - 1] = a[j - 1] + a[l - 1] * a[k - 1];
                a[k - 1] = a[k - 1] + a[l - 1] * hold;
            }
            if 2 * l1 != l - 2 {
                a[l2] *= 1.0 + a[l - 1];
            }
        }
        v += a[l - 1] * d;
        f[at(l, l)] = (g[l] - q) / v;
        for j in 1..l {
            f[at(l, j)] = f[at(l - 1, j)] + f[at(l, l)] * a[l - j];
        }
        varp[l - 1] = varp[l - 2] * (1.0 - f[at(l, l)] * f[at(l, l)]);
        if l == lr {
            return;
        }
        d = 0.0;
        q = 0.0;
        for i in 1..=l {
            let k = l - i + 2;
            d += a[i - 1] * r[k - 1];
            q += f[at(l, i)] * r[k - 1];
        }
    }
}

#[cfg(test)]
mod eureka_tests {
    use super::eureka;

    #[test]
    fn eureka_second_order_preserves_column_major_output() {
        let mut coefficients = [-1.0; 4];
        let mut variance = [0.0; 2];
        let mut work = [0.0; 2];
        eureka(
            2,
            &[4.0, 2.0, 1.0],
            &[0.0, 2.0, 1.0],
            &mut coefficients,
            &mut variance,
            &mut work,
        );
        assert_eq!(coefficients, [0.5, 0.5, -1.0, 0.0]);
        assert_eq!(variance, [3.0, 3.0]);
    }
}
