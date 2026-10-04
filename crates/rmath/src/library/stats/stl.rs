//! Port of r-source/src/library/stats/src/stl.c.

use std::os::raw::c_int;

use crate::sexp::ffi::SEXP;

pub(crate) mod kernel;

fn nextodd(x: f64) -> c_int {
    let r = x.round() as c_int;
    if r % 2 == 0 { r + 1 } else { r }
}

/// GNU `stl(x, s.window)` for a tsp series.
pub unsafe fn do_stl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{
            CAR, CDR, INTEGER, REAL, SET_STRING_ELT, SET_VECTOR_ELT, TAG, TYPEOF, XLENGTH,
        };
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkString};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::protect;
        use crate::sexp::symbol::Rf_install;
        let x0 = CAR(args);
        let mut sw = 7i32;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::PRINTNAME(tag),
                ))
                .to_string_lossy()
                .into_owned()
            } else {
                String::new()
            };
            if name == "s.window" {
                let v = CAR(cell);
                sw = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    *INTEGER(v)
                } else {
                    *REAL(v) as c_int
                };
            }
            cell = CDR(cell);
        }
        let n = XLENGTH(x0) as c_int;
        let tsp = crate::sexp::attrib_core::getAttrib(x0, Rf_install(c"tsp".as_ptr()));
        let period = if !tsp.is_null()
            && tsp != R_NilValue()
            && TYPEOF(tsp) == SEXPTYPE::REALSXP
            && XLENGTH(tsp) >= 3
        {
            *REAL(tsp).add(2) as c_int
        } else {
            12
        };
        if period < 2 || n <= 2 * period {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "series is not periodic or has less than two periods",
            );
        }
        let mut ns = nextodd(sw as f64).max(3);
        if ns % 2 == 0 {
            ns += 1;
        }
        let mut nt = nextodd((1.5 * period as f64 / (1.0 - 1.5 / ns as f64)).ceil());
        nt = nt.max(3);
        let nl = nextodd(period as f64).max(3);
        let isdeg: c_int = 0;
        let itdeg: c_int = 1;
        let ildeg: c_int = 1;
        let nsjump = (ns / 10).max(1);
        let ntjump = (nt / 10).max(1);
        let nljump = (nl / 10).max(1);
        let ni: c_int = 2;
        let no: c_int = 0;
        let mut y = vec![0.0f64; n as usize];
        for i in 0..n as usize {
            y[i] = if TYPEOF(x0) == SEXPTYPE::REALSXP {
                *REAL(x0).add(i)
            } else {
                *INTEGER(x0).add(i) as f64
            };
        }
        let mut rw = vec![1.0f64; n as usize];
        let mut season = vec![0.0f64; n as usize];
        let mut trend = vec![0.0f64; n as usize];
        kernel::filter(
            &y,
            kernel::Parameters::new(
                n as usize,
                period,
                [ns, nt, nl],
                [isdeg, itdeg, ildeg],
                [nsjump, ntjump, nljump],
                ni,
                no,
            ),
            kernel::Output {
                weights: &mut rw,
                season: &mut season,
                trend: &mut trend,
            },
        )
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let ts = Rf_allocVector3(SEXPTYPE::REALSXP, (n as usize * 3) as i64);
        let _ts = protect(ts);
        for i in 0..n as usize {
            *REAL(ts).add(i) = season[i];
            *REAL(ts).add(i + n as usize) = trend[i];
            *REAL(ts).add(i + 2 * n as usize) = y[i] - season[i] - trend[i];
        }
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dim) = n;
        *INTEGER(dim).add(1) = 3;
        crate::sexp::attrib_core::setAttrib(ts, crate::sexp::attrib_core::R_DimSymbol(), dim);
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        SET_STRING_ELT(
            cn,
            0,
            crate::sexp::constructors::Rf_mkChar(c"seasonal".as_ptr()),
        );
        SET_STRING_ELT(
            cn,
            1,
            crate::sexp::constructors::Rf_mkChar(c"trend".as_ptr()),
        );
        SET_STRING_ELT(
            cn,
            2,
            crate::sexp::constructors::Rf_mkChar(c"remainder".as_ptr()),
        );
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        SET_VECTOR_ELT(dn, 0, R_NilValue());
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(ts, crate::sexp::attrib_core::R_DimNamesSymbol(), dn);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, ts);
        crate::mainutils::essentials::set_string_names(result, &["time.series".to_string()]);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"stl".as_ptr()),
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use super::kernel;

    #[test]
    fn stl_decomposes_additive_series_and_sets_default_weights() {
        let y: Vec<f64> = (0..24)
            .map(|i| 10.0 + i as f64 * 0.25 + [1.0, -1.0, 0.5, -0.5][i % 4])
            .collect();
        let n = y.len() as i32;
        let np = 4;
        let ns = 7;
        let nt = 9;
        let nl = 7;
        let isdeg = 1;
        let itdeg = 1;
        let ildeg = 1;
        let nsjump = 1;
        let ntjump = 1;
        let nljump = 1;
        let ni = 2;
        let no = 0;
        let mut rw = vec![0.0; y.len()];
        let mut season = vec![0.0; y.len()];
        let mut trend = vec![0.0; y.len()];

        kernel::filter(
            &y,
            kernel::Parameters::new(
                n as usize,
                np,
                [ns, nt, nl],
                [isdeg, itdeg, ildeg],
                [nsjump, ntjump, nljump],
                ni,
                no,
            ),
            kernel::Output {
                weights: &mut rw,
                season: &mut season,
                trend: &mut trend,
            },
        )
        .unwrap();

        assert!(rw.iter().all(|w| (*w - 1.0).abs() < 1e-12));
        assert!(season.iter().all(|x| x.is_finite()));
        assert!(trend.iter().all(|x| x.is_finite()));
        let max_residual = y
            .iter()
            .zip(season.iter().zip(&trend))
            .map(|(y, (s, t))| (y - s - t).abs())
            .fold(0.0, f64::max);
        assert!(max_residual < 2.5, "max residual was {max_residual}");
    }

    #[test]
    fn stl_normalizes_small_even_spans_and_period() {
        let y: Vec<f64> = (0..12).map(|i| i as f64).collect();
        let n = y.len() as i32;
        let np = 1;
        let ns = 2;
        let nt = 2;
        let nl = 2;
        let degree = 0;
        let jump = 1;
        let ni = 1;
        let no = 0;
        let mut rw = vec![0.0; y.len()];
        let mut season = vec![0.0; y.len()];
        let mut trend = vec![0.0; y.len()];

        kernel::filter(
            &y,
            kernel::Parameters::new(
                n as usize,
                np,
                [ns, nt, nl],
                [degree, degree, degree],
                [jump, jump, jump],
                ni,
                no,
            ),
            kernel::Output {
                weights: &mut rw,
                season: &mut season,
                trend: &mut trend,
            },
        )
        .unwrap();

        assert!(season.iter().all(|x| x.is_finite()));
        assert!(trend.iter().all(|x| x.is_finite()));
        assert!(rw.iter().all(|w| (*w - 1.0).abs() < 1e-12));
    }
}
