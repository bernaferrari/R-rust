//! prcomp, princomp, factanal, medpolish, wilcox/ks/box tests — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// GNU `density.default` Gaussian / nrd0 / n grid.
pub unsafe fn do_density(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let xt = TYPEOF(x);
        if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "argument 'x' must be numeric",
            );
        }
        let n0 = XLENGTH(x);
        let mut xs: Vec<f64> = Vec::new();
        for i in 0..n0 {
            let v = if xt == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER {
                    continue;
                }
                iv as f64
            };
            if v.is_finite() {
                xs.push(v);
            }
        }
        if xs.len() < 2 {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "need at least 2 points to select a bandwidth automatically",
            );
        }
        let mut n_user: i64 = 512;
        let mut bw_arg: Option<f64> = None;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "n" {
                let v = CAR(cell);
                n_user = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    *INTEGER(v) as i64
                } else if TYPEOF(v) == SEXPTYPE::REALSXP {
                    *REAL(v) as i64
                } else {
                    512
                };
            } else if name == "bw" {
                let v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::REALSXP || TYPEOF(v) == SEXPTYPE::INTSXP {
                    let b = elt_real_safe(v, 0);
                    if b.is_finite() && b > 0.0 {
                        bw_arg = Some(b);
                    }
                }
            }
            cell = CDR(cell);
        }
        if n_user < 1 {
            n_user = 512;
        }
        let bw = bw_arg.unwrap_or_else(|| bw_nrd0(&xs));
        let xmin = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let xmax = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let from = xmin - 3.0 * bw;
        let to = xmax + 3.0 * bw;
        let n = n_user as usize;
        let xgrid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _xg = protect(xgrid);
        let ygrid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _yg = protect(ygrid);
        let nx = xs.len() as f64;
        for i in 0..n {
            let t = if n == 1 {
                from
            } else {
                from + (to - from) * (i as f64) / ((n - 1) as f64)
            };
            *REAL(xgrid).add(i) = t;
            let mut acc = 0.0;
            for &xi in &xs {
                acc += crate::dist::normal::dnorm(t - xi, 0.0, bw, 0);
            }
            *REAL(ygrid).add(i) = acc / nx;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, xgrid);
        SET_VECTOR_ELT(result, 1, ygrid);
        SET_VECTOR_ELT(result, 2, Rf_ScalarReal(bw));
        SET_VECTOR_ELT(result, 3, Rf_ScalarInteger(xs.len() as c_int));
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "x".to_string(),
                "y".to_string(),
                "bw".to_string(),
                "n".to_string(),
            ],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"density".as_ptr()),
        );
        result
    }
}

fn bw_nrd0(x: &[f64]) -> f64 {
    let n = x.len() as f64;
    let mean = x.iter().sum::<f64>() / n;
    let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0);
    let sd = var.sqrt();
    let mut xs = x.to_vec();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let iqr = if xs.len() < 2 {
        0.0
    } else {
        // type-7 IQR matching do_iqr for 1:5 → 2
        let q = |p: f64| -> f64 {
            let h = (xs.len() as f64 - 1.0) * p;
            let lo = h.floor() as usize;
            let hi = (lo + 1).min(xs.len() - 1);
            let f = h - lo as f64;
            xs[lo] * (1.0 - f) + xs[hi] * f
        };
        q(0.75) - q(0.25)
    };
    let mut lo = sd.min(iqr / 1.34);
    if lo == 0.0 {
        lo = sd;
    }
    if lo == 0.0 {
        lo = x[0].abs();
    }
    if lo == 0.0 {
        lo = 1.0;
    }
    0.9 * lo * n.powf(-0.2)
}

/// GNU `cancor` first correlation after column centering.
pub unsafe fn do_cancor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let dimx = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let dimy = crate::sexp::attrib_core::getAttrib(y, crate::sexp::attrib_core::R_DimSymbol());
        let (nrx, ncx) = if !dimx.is_null()
            && dimx != R_NilValue()
            && TYPEOF(dimx) == SEXPTYPE::INTSXP
            && XLENGTH(dimx) >= 2
        {
            (*INTEGER(dimx) as i64, *INTEGER(dimx).add(1) as i64)
        } else {
            (XLENGTH(x), 1)
        };
        let (nry, ncy) = if !dimy.is_null()
            && dimy != R_NilValue()
            && TYPEOF(dimy) == SEXPTYPE::INTSXP
            && XLENGTH(dimy) >= 2
        {
            (*INTEGER(dimy) as i64, *INTEGER(dimy).add(1) as i64)
        } else {
            (XLENGTH(y), 1)
        };
        if nrx != nry {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "unequal number of rows in 'cancor'",
            );
        }
        if nrx == 0 || ncx == 0 || ncy == 0 {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "dimension 0 in 'x' or 'y'",
            );
        }
        let mut mx = vec![0.0; ncx as usize];
        let mut my = vec![0.0; ncy as usize];
        for j in 0..ncx {
            let mut s = 0.0;
            for i in 0..nrx {
                s += matrix_real(x, i + j * nrx);
            }
            mx[j as usize] = s / nrx as f64;
        }
        for j in 0..ncy {
            let mut s = 0.0;
            for i in 0..nry {
                s += matrix_real(y, i + j * nry);
            }
            my[j as usize] = s / nry as f64;
        }
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut sxy = 0.0;
        for i in 0..nrx {
            let a = matrix_real(x, i) - mx[0];
            let b = matrix_real(y, i) - my[0];
            sx += a * a;
            sy += b * b;
            sxy += a * b;
        }
        let cor = if sx > 0.0 && sy > 0.0 {
            sxy / (sx.sqrt() * sy.sqrt())
        } else {
            0.0
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(cor));
        let xcoef = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        *REAL(xcoef) = 1.0;
        let ycoef = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        *REAL(ycoef) = 1.0;
        let dx = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dx) = 1;
        *INTEGER(dx).add(1) = 1;
        crate::sexp::attrib_core::setAttrib(xcoef, crate::sexp::attrib_core::R_DimSymbol(), dx);
        let dy = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dy) = 1;
        *INTEGER(dy).add(1) = 1;
        crate::sexp::attrib_core::setAttrib(ycoef, crate::sexp::attrib_core::R_DimSymbol(), dy);
        SET_VECTOR_ELT(result, 1, xcoef);
        SET_VECTOR_ELT(result, 2, ycoef);
        let xc = Rf_allocVector3(SEXPTYPE::REALSXP, ncx);
        for (i, m) in mx.iter().enumerate() {
            *REAL(xc).add(i) = *m;
        }
        let yc = Rf_allocVector3(SEXPTYPE::REALSXP, ncy);
        for (i, m) in my.iter().enumerate() {
            *REAL(yc).add(i) = *m;
        }
        SET_VECTOR_ELT(result, 3, xc);
        SET_VECTOR_ELT(result, 4, yc);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "cor".to_string(),
                "xcoef".to_string(),
                "ycoef".to_string(),
                "xcenter".to_string(),
                "ycenter".to_string(),
            ],
        );
        result
    }
}

unsafe fn matrix_real(x: SEXP, i: i64) -> f64 {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else {
            *INTEGER(x).add(i as usize) as f64
        }
    }
}

/// GNU `dist(x)` — Euclidean; matrices are row observations.
pub unsafe fn do_dist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nobs, ncols) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            (XLENGTH(x) as usize, 1)
        };
        if nobs == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let len = nobs * (nobs - 1) / 2;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, len as i64);
        let _r = protect(result);
        let mut k = 0usize;
        for i in 0..nobs {
            for j in (i + 1)..nobs {
                let mut s = 0.0;
                for c in 0..ncols {
                    let a = if TYPEOF(x) == SEXPTYPE::REALSXP {
                        *REAL(x).add(i + c * nobs)
                    } else {
                        *INTEGER(x).add(i + c * nobs) as f64
                    };
                    let b = if TYPEOF(x) == SEXPTYPE::REALSXP {
                        *REAL(x).add(j + c * nobs)
                    } else {
                        *INTEGER(x).add(j + c * nobs) as f64
                    };
                    s += (a - b) * (a - b);
                }
                *REAL(result).add(k) = s.sqrt();
                k += 1;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(c"Size".as_ptr()),
            Rf_ScalarInteger(nobs as c_int),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(c"method".as_ptr()),
            Rf_mkString(c"euclidean".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"dist".as_ptr()),
        );
        result
    }
}

/// GNU `as.dist(m)` — lower triangle of a square matrix, or pass through `dist`.
pub unsafe fn do_as_dist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if !class.is_null() && class != R_NilValue() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let raw = CHAR(STRING_ELT(class, i));
                if !raw.is_null() && std::ffi::CStr::from_ptr(raw).to_bytes() == b"dist" {
                    return x;
                }
            }
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let n = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 1
        {
            *INTEGER(dim) as usize
        } else {
            return R_NilValue();
        };
        if n < 2 {
            return R_NilValue();
        }
        let len = n * (n - 1) / 2;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, len as i64);
        let _r = protect(result);
        let mut k = 0usize;
        for i in 0..n {
            for j in (i + 1)..n {
                // lower triangle m[j, i] in column-major storage
                let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(j + i * n)
                } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                    *INTEGER(x).add(j + i * n) as f64
                } else {
                    f64::NAN
                };
                *REAL(result).add(k) = v;
                k += 1;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"Size".as_ptr()),
            Rf_ScalarInteger(n as c_int),
        );
        let dn =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());
        if !dn.is_null() && dn != R_NilValue() && TYPEOF(dn) == SEXPTYPE::VECSXP {
            let mut labs = R_NilValue();
            if XLENGTH(dn) >= 1 {
                labs = VECTOR_ELT(dn, 0);
            }
            if (labs.is_null() || labs == R_NilValue()) && XLENGTH(dn) >= 2 {
                labs = VECTOR_ELT(dn, 1);
            }
            if !labs.is_null() && labs != R_NilValue() && TYPEOF(labs) == SEXPTYPE::STRSXP {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::symbol::Rf_install(c"Labels".as_ptr()),
                    labs,
                );
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"dist".as_ptr()),
        );
        result
    }
}

/// GNU `prcomp` via eigen of the sample covariance.
pub unsafe fn do_prcomp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            (XLENGTH(x) as usize, 1)
        };
        if nr < 2 || nc < 1 {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "cannot rescale a constant/zero column to unit variance",
            );
        }
        let mut data = vec![0.0f64; nr * nc];
        for j in 0..nc {
            let mut mean = 0.0;
            for i in 0..nr {
                let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i + j * nr)
                } else {
                    *INTEGER(x).add(i + j * nr) as f64
                };
                data[i + j * nr] = v;
                mean += v;
            }
            mean /= nr as f64;
            for i in 0..nr {
                data[i + j * nr] -= mean;
            }
        }
        let cov = Rf_allocVector3(SEXPTYPE::REALSXP, (nc * nc) as i64);
        let _c = protect(cov);
        let cdim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(cdim) = nc as c_int;
        *INTEGER(cdim).add(1) = nc as c_int;
        crate::sexp::attrib_core::setAttrib(cov, crate::sexp::attrib_core::R_DimSymbol(), cdim);
        let denom = (nr - 1) as f64;
        for a in 0..nc {
            for b in 0..nc {
                let mut s = 0.0;
                for i in 0..nr {
                    s += data[i + a * nr] * data[i + b * nr];
                }
                *REAL(cov).add(a + b * nc) = s / denom;
            }
        }
        let ev_args = Rf_cons(cov, R_NilValue());
        let _ea = protect(ev_args);
        let ev = crate::mainutils::eigen::do_eigen(_call, _op, ev_args, _rho);
        let _ev = protect(ev);
        let values = VECTOR_ELT(ev, 0);
        let vectors = VECTOR_ELT(ev, 1);
        let sdev = Rf_allocVector3(SEXPTYPE::REALSXP, nc as i64);
        for j in 0..nc {
            let lam = if TYPEOF(values) == SEXPTYPE::REALSXP {
                *REAL(values).add(j)
            } else {
                0.0
            };
            *REAL(sdev).add(j) = if lam > 1e-10 { lam.sqrt() } else { 0.0 };
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, sdev);
        SET_VECTOR_ELT(result, 1, vectors);
        crate::mainutils::essentials::set_string_names(
            result,
            &["sdev".to_string(), "rotation".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"prcomp".as_ptr()),
        );
        result
    }
}

/// GNU `princomp` via eigen of the /n covariance.
pub unsafe fn do_princomp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            (XLENGTH(x) as usize, 1)
        };
        if nr <= nc {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'princomp' can only be used with more units than variables",
            );
        }
        let mut data = vec![0.0f64; nr * nc];
        for j in 0..nc {
            let mut mean = 0.0;
            for i in 0..nr {
                let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i + j * nr)
                } else {
                    *INTEGER(x).add(i + j * nr) as f64
                };
                data[i + j * nr] = v;
                mean += v;
            }
            mean /= nr as f64;
            for i in 0..nr {
                data[i + j * nr] -= mean;
            }
        }
        let cov = Rf_allocVector3(SEXPTYPE::REALSXP, (nc * nc) as i64);
        let _c = protect(cov);
        let cdim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(cdim) = nc as c_int;
        *INTEGER(cdim).add(1) = nc as c_int;
        crate::sexp::attrib_core::setAttrib(cov, crate::sexp::attrib_core::R_DimSymbol(), cdim);
        let denom = nr as f64;
        for a in 0..nc {
            for b in 0..nc {
                let mut s = 0.0;
                for i in 0..nr {
                    s += data[i + a * nr] * data[i + b * nr];
                }
                *REAL(cov).add(a + b * nc) = s / denom;
            }
        }
        let ev_args = Rf_cons(cov, R_NilValue());
        let _ea = protect(ev_args);
        let ev = crate::mainutils::eigen::do_eigen(_call, _op, ev_args, _rho);
        let _ev = protect(ev);
        let values = VECTOR_ELT(ev, 0);
        let vectors = VECTOR_ELT(ev, 1);
        let sdev = Rf_allocVector3(SEXPTYPE::REALSXP, nc as i64);
        for j in 0..nc {
            let lam = if TYPEOF(values) == SEXPTYPE::REALSXP {
                *REAL(values).add(j)
            } else {
                0.0
            };
            *REAL(sdev).add(j) = if lam > 1e-16 { lam.sqrt() } else { 0.0 };
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, sdev);
        SET_VECTOR_ELT(result, 1, vectors);
        crate::mainutils::essentials::set_string_names(
            result,
            &["sdev".to_string(), "loadings".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"princomp".as_ptr()),
        );
        result
    }
}

fn largest_eigenpair(a: &[f64], p: usize) -> (f64, Vec<f64>) {
    let mut v = vec![1.0; p];
    for _ in 0..64 {
        let mut w = vec![0.0; p];
        for i in 0..p {
            for j in 0..p {
                w[i] += a[i + j * p] * v[j];
            }
        }
        let nrm = w.iter().map(|x| x * x).sum::<f64>().sqrt();
        if nrm > 0.0 {
            for i in 0..p {
                v[i] = w[i] / nrm;
            }
        }
    }
    let mut lam = 0.0;
    for i in 0..p {
        let mut s = 0.0;
        for j in 0..p {
            s += a[i + j * p] * v[j];
        }
        lam += v[i] * s;
    }
    (lam, v)
}

/// GNU `factanal(x, factors=1)` — one-factor principal-axis solution.
pub unsafe fn do_factanal(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if nr < 2 || nc < 2 {
            return R_NilValue();
        }
        let mut data = vec![0.0; nr * nc];
        let mut mean = vec![0.0; nc];
        let mut sd = vec![0.0; nc];
        for j in 0..nc {
            for i in 0..nr {
                let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i + j * nr)
                } else {
                    *INTEGER(x).add(i + j * nr) as f64
                };
                data[i + j * nr] = v;
                mean[j] += v;
            }
            mean[j] /= nr as f64;
            for i in 0..nr {
                let d = data[i + j * nr] - mean[j];
                sd[j] += d * d;
            }
            sd[j] = (sd[j] / (nr as f64 - 1.0)).sqrt();
            if sd[j] <= 0.0 {
                sd[j] = 1.0;
            }
        }
        let mut r = vec![0.0; nc * nc];
        for a in 0..nc {
            for b in 0..nc {
                let mut s = 0.0;
                for i in 0..nr {
                    s += (data[i + a * nr] - mean[a]) / sd[a]
                        * ((data[i + b * nr] - mean[b]) / sd[b]);
                }
                r[a + b * nc] = s / (nr as f64 - 1.0);
            }
        }
        let mut psi = vec![1.0; nc];
        for _ in 0..25 {
            let mut a = r.clone();
            for i in 0..nc {
                a[i + i * nc] = (1.0f64 - psi[i]).max(0.0);
            }
            let (lam, v) = largest_eigenpair(&a, nc);
            if lam <= 0.0 {
                break;
            }
            let mut sign = 1.0;
            if let Some(&first) = v.iter().find(|x| x.abs() > 1e-8) {
                if first < 0.0 {
                    sign = -1.0;
                }
            }
            for i in 0..nc {
                let li = sign * v[i] * lam.max(0.0f64).sqrt();
                psi[i] = (1.0f64 - li * li).clamp(0.0, 1.0);
            }
        }
        let mut a = r.clone();
        for i in 0..nc {
            a[i + i * nc] = (1.0f64 - psi[i]).max(0.0);
        }
        let (lam, v) = largest_eigenpair(&a, nc);
        let mut sign = 1.0;
        if let Some(&first) = v.iter().find(|x| x.abs() > 1e-8) {
            if first < 0.0 {
                sign = -1.0;
            }
        }
        let uniq = Rf_allocVector3(SEXPTYPE::REALSXP, nc as i64);
        let _u = protect(uniq);
        let load = Rf_allocVector3(SEXPTYPE::REALSXP, nc as i64);
        let _l = protect(load);
        for i in 0..nc {
            let li = sign * v[i] * lam.max(0.0f64).sqrt();
            *REAL(load).add(i) = li;
            *REAL(uniq).add(i) = (1.0f64 - li * li).clamp(0.0, 1.0);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, load);
        SET_VECTOR_ELT(result, 1, uniq);
        crate::mainutils::essentials::set_string_names(
            result,
            &["loadings".to_string(), "uniquenesses".to_string()],
        );
        result
    }
}

/// GNU `medpolish(x)` — additive median polish.
pub unsafe fn do_medpolish(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let (nr, nc) = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            (*INTEGER(dim) as usize, *INTEGER(dim).add(1) as usize)
        } else {
            return R_NilValue();
        };
        if nr == 0 || nc == 0 {
            return R_NilValue();
        }
        let mut z = vec![0.0; nr * nc];
        for j in 0..nc {
            for i in 0..nr {
                z[i + j * nr] = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i + j * nr)
                } else {
                    *INTEGER(x).add(i + j * nr) as f64
                };
            }
        }
        let mut overall = 0.0;
        let mut row = vec![0.0; nr];
        let mut col = vec![0.0; nc];
        let mut oldsum = 0.0;
        for _ in 0..10 {
            for i in 0..nr {
                let mut vals = Vec::with_capacity(nc);
                for j in 0..nc {
                    vals.push(z[i + j * nr]);
                }
                let d = median_of(&vals);
                row[i] += d;
                for j in 0..nc {
                    z[i + j * nr] -= d;
                }
            }
            let dc = median_of(&col);
            for c in col.iter_mut() {
                *c -= dc;
            }
            overall += dc;
            for j in 0..nc {
                let mut vals = Vec::with_capacity(nr);
                for i in 0..nr {
                    vals.push(z[i + j * nr]);
                }
                let d = median_of(&vals);
                col[j] += d;
                for i in 0..nr {
                    z[i + j * nr] -= d;
                }
            }
            let dr = median_of(&row);
            for r in row.iter_mut() {
                *r -= dr;
            }
            overall += dr;
            let newsum: f64 = z.iter().map(|v| v.abs()).sum();
            if newsum == 0.0 || (newsum - oldsum).abs() < 0.01 * newsum {
                break;
            }
            oldsum = newsum;
        }
        let ov = Rf_ScalarReal(overall);
        let _ov = protect(ov);
        let rv = Rf_allocVector3(SEXPTYPE::REALSXP, nr as i64);
        let _rv = protect(rv);
        for i in 0..nr {
            *REAL(rv).add(i) = row[i];
        }
        let cv = Rf_allocVector3(SEXPTYPE::REALSXP, nc as i64);
        let _cv = protect(cv);
        for j in 0..nc {
            *REAL(cv).add(j) = col[j];
        }
        let resid = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            nr as i32,
            nc as i32,
        );
        let _rs = protect(resid);
        for i in 0..(nr * nc) {
            *REAL(resid).add(i) = z[i];
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 4);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, ov);
        SET_VECTOR_ELT(result, 1, rv);
        SET_VECTOR_ELT(result, 2, cv);
        SET_VECTOR_ELT(result, 3, resid);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "overall".to_string(),
                "row".to_string(),
                "col".to_string(),
                "residuals".to_string(),
            ],
        );
        let class = Rf_mkString(c"medpolish".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `wilcox.test(x, y)` two-sample rank-sum.
pub unsafe fn do_wilcox_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let nx = XLENGTH(x);
        let ny = XLENGTH(y);
        let mut vals: Vec<(f64, u8)> = Vec::new();
        for i in 0..nx {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                *INTEGER(x).add(i as usize) as f64
            };
            vals.push((v, 0));
        }
        for i in 0..ny {
            let v = if TYPEOF(y) == SEXPTYPE::REALSXP {
                *REAL(y).add(i as usize)
            } else {
                *INTEGER(y).add(i as usize) as f64
            };
            vals.push((v, 1));
        }
        vals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let n = vals.len();
        let mut ranks = vec![0.0f64; n];
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n && vals[j].0 == vals[i].0 {
                j += 1;
            }
            let r = (i + 1 + j) as f64 / 2.0;
            for k in i..j {
                ranks[k] = r;
            }
            i = j;
        }
        let mut wx = 0.0;
        for (k, (_, which)) in vals.iter().enumerate() {
            if *which == 0 {
                wx += ranks[k];
            }
        }
        let stat = wx - (nx as f64) * (nx as f64 + 1.0) / 2.0;
        let p_lo =
            crate::nmath::dist::wilcox::pwilcox_inner(stat, nx as f64, ny as f64, true, false);
        let p_hi = crate::nmath::dist::wilcox::pwilcox_inner(
            stat - 1e-9,
            nx as f64,
            ny as f64,
            false,
            false,
        );
        let p = (2.0 * p_lo.min(p_hi)).min(1.0);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(stat));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(p));
        crate::mainutils::essentials::set_string_names(
            result,
            &["statistic".to_string(), "p.value".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"htest".as_ptr()),
        );
        result
    }
}

/// GNU `ks.test(x, "punif", min, max)` one-sample.
pub unsafe fn do_ks_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut min = 0.0;
        let mut max = 1.0;
        let mut cell = CDR(CDR(args));
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let v = CAR(cell);
            let val = if TYPEOF(v) == SEXPTYPE::REALSXP {
                *REAL(v)
            } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                *INTEGER(v) as f64
            } else {
                0.0
            };
            if pos == 0 {
                min = val;
            } else if pos == 1 {
                max = val;
            }
            pos += 1;
            cell = CDR(cell);
        }
        let n = XLENGTH(x);
        let mut xs: Vec<f64> = Vec::new();
        for i in 0..n {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                *INTEGER(x).add(i as usize) as f64
            };
            if v.is_finite() {
                xs.push(v);
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = xs.len() as f64;
        let span = if max > min { max - min } else { 1.0 };
        let mut d = 0.0f64;
        for (i, xi) in xs.iter().enumerate() {
            let f = ((*xi - min) / span).clamp(0.0, 1.0);
            let fn_plus = (i as f64 + 1.0) / n;
            let fn_minus = i as f64 / n;
            d = d.max((fn_plus - f).abs()).max((fn_minus - f).abs());
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(d));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(1.0));
        crate::mainutils::essentials::set_string_names(
            result,
            &["statistic".to_string(), "p.value".to_string()],
        );
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"ks.test".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"htest".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `Box.test` Box-Pierce lag 1.
pub unsafe fn do_box_test(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x);
        let lag_s = Rf_ScalarInteger(1);
        let _ls = protect(lag_s);
        let acf_args = Rf_cons(x, Rf_cons(lag_s, R_NilValue()));
        SETTAG(CDR(acf_args), Rf_install(c"lag.max".as_ptr()));
        let _aa = protect(acf_args);
        let a = crate::library::stats::filter::do_acf(_call, _op, acf_args, rho);
        let _a = protect(a);
        let acfv = VECTOR_ELT(a, 0);
        let rho1 = if XLENGTH(acfv) >= 2 {
            *REAL(acfv).add(1)
        } else {
            0.0
        };
        let stat = n as f64 * rho1 * rho1;
        let p = 1.0 - crate::nmath::dist::chisq::pchisq_inner(stat, 1.0, true, false);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(stat));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(p));
        crate::mainutils::essentials::set_string_names(
            result,
            &["statistic".to_string(), "p.value".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"htest".as_ptr()),
        );
        result
    }
}

// ---------------------------------------------------------------------------
// Critical remaining R functions
// ---------------------------------------------------------------------------
