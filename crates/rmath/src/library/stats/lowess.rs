//! Lowess (locally weighted scatterplot smoothing)
//! Port of r-source/src/library/stats/src/lowess.c

use std::os::raw::{c_double, c_int};

use crate::main::coerce::{asInteger, asReal};
use crate::main::errors::Rf_error;
use crate::main::sort::rPsort;
use crate::sexp::accessors::{LENGTH, REAL, TYPEOF};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::NA_INTEGER;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

#[inline]
fn fsquare(x: c_double) -> c_double {
    x * x
}

#[inline]
fn fcube(x: c_double) -> c_double {
    x * x * x
}

#[inline]
fn fmax2(a: c_double, b: c_double) -> c_double {
    if a > b { a } else { b }
}

#[inline]
fn imin2(a: c_int, b: c_int) -> c_int {
    if a < b { a } else { b }
}

#[inline]
fn imax2(a: c_int, b: c_int) -> c_int {
    if a > b { a } else { b }
}

unsafe fn lowest(
    x: *mut c_double,
    y: *mut c_double,
    n: c_int,
    xs: *const c_double,
    ys: *mut c_double,
    nleft: c_int,
    nright: c_int,
    w: *mut c_double,
    userw: bool,
    rw: *const c_double,
    ok: *mut bool,
) {
    unsafe {
        let mut nrt: c_int;
        let mut j: c_int;
        let mut a: c_double;
        let mut b: c_double;
        let mut c: c_double;
        let mut h: c_double;
        let mut h1: c_double;
        let mut h9: c_double;
        let mut r: c_double;
        let range: c_double;

        range = *x.add((n - 1) as usize) - *x.add(0);
        h = fmax2(
            *xs - *x.add((nleft - 1) as usize),
            *x.add((nright - 1) as usize) - *xs,
        );
        h9 = 0.999 * h;
        h1 = 0.001 * h;

        // sum of weights
        a = 0.0;
        j = nleft;
        while j <= n {
            *w.add((j - 1) as usize) = 0.0;
            r = (*x.add((j - 1) as usize) - *xs).abs();
            if r <= h9 {
                if r <= h1 {
                    *w.add((j - 1) as usize) = 1.0;
                } else {
                    *w.add((j - 1) as usize) = fcube(1.0 - fcube(r / h));
                }
                if userw {
                    *w.add((j - 1) as usize) *= *rw.add((j - 1) as usize);
                }
                a += *w.add((j - 1) as usize);
            } else if *x.add((j - 1) as usize) > *xs {
                break;
            }
            j += 1;
        }

        nrt = j - 1;
        if a <= 0.0 {
            *ok = false;
        } else {
            *ok = true;

            // weighted least squares
            // make sum of w[j] == 1
            j = nleft;
            while j <= nrt {
                *w.add((j - 1) as usize) /= a;
                j += 1;
            }
            if h > 0.0 {
                a = 0.0;

                // use linear fit
                // weighted center of x values
                j = nleft;
                while j <= nrt {
                    a += *w.add((j - 1) as usize) * *x.add((j - 1) as usize);
                    j += 1;
                }
                b = *xs - a;
                c = 0.0;
                j = nleft;
                while j <= nrt {
                    c += *w.add((j - 1) as usize) * fsquare(*x.add((j - 1) as usize) - a);
                    j += 1;
                }
                if c.sqrt() > 0.001 * range {
                    b /= c;

                    // points are spread out enough to compute slope
                    j = nleft;
                    while j <= nrt {
                        *w.add((j - 1) as usize) *= b * (*x.add((j - 1) as usize) - a) + 1.0;
                        j += 1;
                    }
                }
            }
            *ys = 0.0;
            j = nleft;
            while j <= nrt {
                *ys += *w.add((j - 1) as usize) * *y.add((j - 1) as usize);
                j += 1;
            }
        }
    }
}

unsafe fn clowess(
    x: *const c_double,
    y: *const c_double,
    n: c_int,
    f: c_double,
    nsteps: c_int,
    delta: c_double,
    ys: *mut c_double,
    rw: *mut c_double,
    res: *mut c_double,
) {
    unsafe {
        if n < 2 {
            *ys = *y;
            return;
        }

        let ns = imax2(2, imin2(n, (f * n as c_double + 1e-7) as c_int));

        let mut iter: c_int = 1;
        while iter <= nsteps + 1 {
            let mut nleft: c_int = 1;
            let mut nright: c_int = ns;
            let mut last: c_int = 0;
            let mut i: c_int = 1;

            loop {
                if nright < n {
                    let d1 = *x.add((i - 1) as usize) - *x.add((nleft - 1) as usize);
                    let d2 = *x.add(nright as usize) - *x.add((i - 1) as usize);

                    if d1 > d2 {
                        nleft += 1;
                        nright += 1;
                        continue;
                    }
                }

                let mut ok = false;
                lowest(
                    x as *mut c_double,
                    y as *mut c_double,
                    n,
                    &*x.add((i - 1) as usize),
                    ys.add((i - 1) as usize),
                    nleft,
                    nright,
                    res,
                    iter > 1,
                    rw,
                    &mut ok,
                );
                if !ok {
                    *ys.add((i - 1) as usize) = *y.add((i - 1) as usize);
                }

                if last < i - 1 {
                    let denom = *x.add((i - 1) as usize) - *x.add((last - 1) as usize);
                    let mut j = last + 1;
                    while j < i {
                        let alpha =
                            (*x.add((j - 1) as usize) - *x.add((last - 1) as usize)) / denom;
                        *ys.add((j - 1) as usize) = alpha * *ys.add((i - 1) as usize)
                            + (1.0 - alpha) * *ys.add((last - 1) as usize);
                        j += 1;
                    }
                }

                last = i;

                let cut = *x.add((last - 1) as usize) + delta;
                i = last + 1;
                while i <= n {
                    if *x.add((i - 1) as usize) > cut {
                        break;
                    }
                    if *x.add((i - 1) as usize) == *x.add((last - 1) as usize) {
                        *ys.add((i - 1) as usize) = *ys.add((last - 1) as usize);
                        last = i;
                    }
                    i += 1;
                }
                i = imax2(last + 1, i - 1);
                if last >= n {
                    break;
                }
            }

            let mut i: c_int = 0;
            while i < n {
                *res.add(i as usize) = *y.add(i as usize) - *ys.add(i as usize);
                i += 1;
            }

            let mut sc: c_double = 0.0;
            let mut i: c_int = 0;
            while i < n {
                sc += (*res.add(i as usize)).abs();
                i += 1;
            }
            sc /= n as c_double;

            if iter > nsteps {
                break;
            }

            let mut i: c_int = 0;
            while i < n {
                *rw.add(i as usize) = (*res.add(i as usize)).abs();
                i += 1;
            }

            let m1 = n / 2;
            rPsort(rw, n, m1);
            let cmad = if n % 2 == 0 {
                let m2 = n - m1 - 1;
                rPsort(rw, n, m2);
                3.0 * (*rw.add(m1 as usize) + *rw.add(m2 as usize))
            } else {
                6.0 * *rw.add(m1 as usize)
            };

            if cmad < 1e-7 * sc {
                break;
            }
            let c9 = 0.999 * cmad;
            let c1 = 0.001 * cmad;
            let mut i: c_int = 0;
            while i < n {
                let r = (*res.add(i as usize)).abs();
                if r <= c1 {
                    *rw.add(i as usize) = 1.0;
                } else if r <= c9 {
                    *rw.add(i as usize) = fsquare(1.0 - fsquare(r / cmad));
                } else {
                    *rw.add(i as usize) = 0.0;
                }
                i += 1;
            }
            iter += 1;
        }
    }
}

pub unsafe fn lowess(x: SEXP, y: SEXP, sf: SEXP, siter: SEXP, sdelta: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) != SEXPTYPE::REALSXP || TYPEOF(y) != SEXPTYPE::REALSXP {
            Rf_error(b"invalid input\0".as_ptr() as *const _);
        }
        let nx = LENGTH(x);
        if nx == NA_INTEGER || nx == 0 {
            Rf_error(b"invalid input\0".as_ptr() as *const _);
        }
        let f = asReal(sf);
        if !f.is_finite() || f <= 0.0 {
            Rf_error(b"'f' must be finite and > 0\0".as_ptr() as *const _);
        }
        let iter = asInteger(siter);
        if iter == NA_INTEGER || iter < 0 {
            Rf_error(b"'iter' must be finite and >= 0\0".as_ptr() as *const _);
        }
        let delta = asReal(sdelta);
        if !delta.is_finite() || delta < 0.0 {
            Rf_error(b"'delta' must be finite and > 0\0".as_ptr() as *const _);
        }

        let ans = Rf_allocVector(SEXPTYPE::REALSXP, nx);
        let _ans_guard = protect(ans);
        let mut rw = vec![0.0f64; nx as usize];
        let mut res = vec![0.0f64; nx as usize];
        clowess(
            REAL(x),
            REAL(y),
            nx,
            f,
            iter,
            delta,
            REAL(ans),
            rw.as_mut_ptr(),
            res.as_mut_ptr(),
        );
        ans
    }
}
pub unsafe extern "C-unwind" fn c_lowess(
    x: SEXP,
    y: SEXP,
    f: SEXP,
    iter: SEXP,
    delta: SEXP,
) -> SEXP {
    unsafe { lowess(x, y, f, iter, delta) }
}

/// GNU `lowess(x, y=NULL, f=2/3, iter=3, delta=0.01*diff(range(x)))`.
pub unsafe fn do_lowess(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, INTEGER, SET_VECTOR_ELT, SETTAG};
        use crate::sexp::constructors::{
            Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector3, Rf_cons,
        };
        use crate::sexp::ffi::R_xlen_t;
        use crate::sexp::globals::{R_MissingArg, R_NilValue};
        use crate::sexp::symbol::Rf_install;
        use std::ffi::CString;
        let mut formals = R_NilValue();
        for name in ["delta", "iter", "f", "y", "x"] {
            let cell = Rf_cons(R_MissingArg(), formals);
            SETTAG(
                cell,
                Rf_install(CString::new(name).unwrap_or_default().as_ptr()),
            );
            formals = cell;
        }
        let _formals = protect(formals);
        let matched = crate::mainutils::match_mod::matchArgs_RC(formals, args, call);
        let _matched = protect(matched);
        let mut slots = [R_MissingArg(); 5];
        let mut cell = matched;
        let mut i = 0;
        while !cell.is_null() && cell != R_NilValue() && i < 5 {
            slots[i] = CAR(cell);
            cell = CDR(cell);
            i += 1;
        }
        let mut x0 = slots[0];
        let mut y0 = slots[1];
        let absent = |s: SEXP| s.is_null() || s == R_NilValue() || s == R_MissingArg();
        if absent(x0) {
            crate::mainutils::errors::errorcall_str(
                call,
                "argument \"x\" is missing, with no default",
            );
        }
        if absent(y0) {
            let n = crate::sexp::accessors::XLENGTH(x0);
            let seq = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            let _s = protect(seq);
            for j in 0..n {
                *REAL(seq).add(j as usize) = (j + 1) as f64;
            }
            y0 = x0;
            x0 = seq;
        }
        let n = crate::sexp::accessors::XLENGTH(x0);
        let f = if absent(slots[2]) {
            2.0 / 3.0
        } else {
            asReal(slots[2])
        };
        let iter = if absent(slots[3]) {
            3
        } else {
            asInteger(slots[3])
        };
        let xd = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _xd = protect(xd);
        let yd = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _yd = protect(yd);
        let as_f64 = |v: SEXP, i: R_xlen_t| -> f64 {
            if TYPEOF(v) == SEXPTYPE::REALSXP {
                *REAL(v).add(i as usize)
            } else if TYPEOF(v) == SEXPTYPE::INTSXP || TYPEOF(v) == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(v).add(i as usize);
                if iv == NA_INTEGER {
                    f64::NAN
                } else {
                    iv as f64
                }
            } else {
                f64::NAN
            }
        };
        let mut xmin = f64::INFINITY;
        let mut xmax = f64::NEG_INFINITY;
        for j in 0..n {
            let xv = as_f64(x0, j);
            *REAL(xd).add(j as usize) = xv;
            *REAL(yd).add(j as usize) = as_f64(y0, j);
            if xv.is_finite() {
                xmin = xmin.min(xv);
                xmax = xmax.max(xv);
            }
        }
        let delta = if absent(slots[4]) {
            0.01 * (xmax - xmin)
        } else {
            asReal(slots[4])
        };
        let sf = Rf_ScalarReal(f);
        let _sf = protect(sf);
        let siter = Rf_ScalarInteger(iter);
        let _si = protect(siter);
        let sdelta = Rf_ScalarReal(delta);
        let _sd = protect(sdelta);
        let ys = lowess(xd, yd, sf, siter, sdelta);
        let _ys = protect(ys);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, xd);
        SET_VECTOR_ELT(result, 1, ys);
        crate::mainutils::essentials::set_string_names(result, &["x".to_string(), "y".to_string()]);
        result
    }
}

/// GNU default/fixed-span supersmoother through owning inputs and checked slices.
pub unsafe fn do_supsmu(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::{
        R_xlen_t, Sexp, SexpMut,
        owner::{OwnerToken, RuntimeAccess, with_runtime},
    };
    let fail = |error: String| -> ! { crate::sexp::context::r_error(error) };
    // SAFETY: this translated entry is activated by the original runtime.
    let owner = unsafe { OwnerToken::current() }
        .map_err(|e| e.to_string())
        .unwrap_or_else(|e| fail(e));
    let original = owner
        .weak_owner()
        .ok_or_else(|| "supersmoother requires a managed runtime".to_owned())
        .unwrap_or_else(|e| fail(e));
    let args = owner
        .sexp(args)
        .and_then(Sexp::into_owned)
        .map_err(|e| e.to_string())
        .unwrap_or_else(|e| fail(e));
    let result = with_runtime(&original, |access| -> Result<SEXP, String> {
        let domain = access.domain();
        let names = ["x", "y", "wt", "span", "periodic", "bass", "trace"];
        let mut slots: Vec<Option<Sexp<'static>>> = (0..7).map(|_| None).collect();
        let mut cell = args;
        let mut seen = std::collections::HashSet::new();
        let mut actual = Vec::new();
        // Capture owning values before matching or any numeric provider.
        while !cell.is_nil() {
            if !seen.insert(cell.as_raw().addr()) {
                return Err("cyclic supersmoother arguments".into());
            }
            let value = cell
                .try_car()
                .and_then(Sexp::into_owned)
                .map_err(|e| e.to_string())?;
            let tag = cell.try_tag().map_err(|e| e.to_string())?;
            let name = if tag.is_nil() {
                None
            } else {
                Some(
                    tag.try_printname()
                        .and_then(|n| n.try_as_string())
                        .map_err(|e| e.to_string())?,
                )
            };
            actual.push((name, value));
            cell = cell
                .try_cdr()
                .and_then(Sexp::into_owned)
                .map_err(|e| e.to_string())?;
        }
        // GNU closure matching: all exact names, then partial names, then positionals.
        // Missing values still reserve the matched formal, while requesting its default.
        let mut assigned = [false; 7];
        let mut matches = vec![None; actual.len()];
        for (i, (name, _)) in actual.iter().enumerate() {
            if let Some(name) = name {
                if let Some(index) = names.iter().position(|&n| n == name) {
                    if assigned[index] {
                        return Err("duplicate supersmoother argument".into());
                    }
                    assigned[index] = true;
                    matches[i] = Some(index);
                }
            }
        }
        for (i, (name, _)) in actual.iter().enumerate() {
            if matches[i].is_none() {
                if let Some(name) = name {
                    let candidates: Vec<_> = names
                        .iter()
                        .enumerate()
                        .filter(|(index, n)| !assigned[*index] && n.starts_with(name))
                        .map(|(index, _)| index)
                        .collect();
                    if candidates.len() != 1 {
                        return Err("unknown or ambiguous supersmoother argument".into());
                    }
                    assigned[candidates[0]] = true;
                    matches[i] = Some(candidates[0]);
                }
            }
        }
        let mut positional = 0;
        for (i, (name, _)) in actual.iter().enumerate() {
            if name.is_none() {
                while positional < 7 && assigned[positional] {
                    positional += 1;
                }
                if positional >= 7 {
                    return Err("unused supersmoother argument".into());
                }
                assigned[positional] = true;
                matches[i] = Some(positional);
                positional += 1;
            }
        }
        for ((_, value), index) in actual.into_iter().zip(matches) {
            let index = index.ok_or("unmatched supersmoother argument")?;
            slots[index] = if value.as_raw() == domain.missing().as_raw() {
                None
            } else {
                Some(value)
            };
        }
        access.require_active().map_err(|e| e.to_string())?;
        fn numeric(v: &Sexp<'_>, access: &RuntimeAccess) -> Result<Vec<f64>, String> {
            let kind = v.typeof_();
            if !matches!(
                kind,
                SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
            ) {
                return Err("supersmoother requires numeric observations".into());
            }
            let len = usize::try_from(v.len()).map_err(|_| "invalid supersmoother length")?;
            access.require_active().map_err(|e| e.to_string())?;
            let mut result = Vec::new();
            result
                .try_reserve_exact(len)
                .map_err(|_| "supersmoother allocation failed")?;
            for i in 0..len {
                let i = i as R_xlen_t;
                let x = match kind {
                    SEXPTYPE::REALSXP => v.try_real_elt(i).map_err(|e| e.to_string())?,
                    SEXPTYPE::INTSXP => {
                        let x = v.try_integer_elt(i).map_err(|e| e.to_string())?;
                        if x == NA_INTEGER {
                            f64::NAN
                        } else {
                            f64::from(x)
                        }
                    }
                    _ => {
                        let x = v.try_logical_elt(i).map_err(|e| e.to_string())?;
                        if x == NA_INTEGER {
                            f64::NAN
                        } else {
                            f64::from(x)
                        }
                    }
                };
                access.require_active().map_err(|e| e.to_string())?;
                result.push(x);
            }
            Ok(result)
        }
        let scalar = |index: usize, default: f64| -> Result<f64, String> {
            let x = if let Some(v) = &slots[index] {
                v.try_as_f64().map_err(|e| e.to_string())?
            } else {
                default
            };
            access.require_active().map_err(|e| e.to_string())?;
            Ok(x)
        };
        fn span_text(v: &Sexp<'_>, access: &RuntimeAccess) -> Result<String, String> {
            let text = v.try_string_value_elt(0).map_err(|e| e.to_string())?;
            access.require_active().map_err(|e| e.to_string())?;
            text.ok_or_else(|| "missing value where TRUE/FALSE needed".into())
        }
        let mut character_span = None;
        let span = if let Some(v) = &slots[3] {
            // The public GNU wrapper first uses span == "cv" in an if condition.
            // It must reject a non-scalar condition before reading its first element.
            let length = v.len();
            access.require_active().map_err(|e| e.to_string())?;
            if length == 0 {
                return Err("argument is of length zero".into());
            }
            if length != 1 {
                return Err("the condition has length > 1".into());
            }
            if v.typeof_() == SEXPTYPE::STRSXP {
                if span_text(v, access)? == "cv" {
                    0.
                } else {
                    // Keep the port's character-comparison semantics, and GNU's
                    // short-circuit provider order, before numeric coercion.
                    if span_text(v, access)?.as_str() < "0" || span_text(v, access)?.as_str() > "1"
                    {
                        return Err("'span' must be between 0 and 1.".into());
                    }
                    character_span = Some(v.clone());
                    0.
                }
            } else {
                if v.typeof_() == SEXPTYPE::CPLXSXP {
                    return Err("invalid comparison with complex values".into());
                }
                let value = scalar(3, 0.)?;
                let missing = value.is_nan()
                    || matches!(v.typeof_(), SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP)
                        && value == f64::from(NA_INTEGER);
                if missing {
                    return Err("missing value where TRUE/FALSE needed".into());
                }
                value
            }
        } else {
            0.
        };
        if !(0. ..=1.).contains(&span) {
            return Err("'span' must be between 0 and 1.".into());
        }
        let x = numeric(slots[0].as_ref().ok_or("missing supersmoother x")?, access)?;
        let yv = slots[1].as_ref().ok_or("missing supersmoother y")?;
        if !matches!(yv.typeof_(), SEXPTYPE::REALSXP | SEXPTYPE::INTSXP) {
            return Err("'y' must be numeric vector".into());
        }
        let y = numeric(yv, access)?;
        if y.is_empty() {
            return Err("'y' must be numeric vector".into());
        }
        if x.len() != y.len() {
            return Err("number of observations in 'x' and 'y' must match.".into());
        }
        let weights = if let Some(w) = &slots[2] {
            numeric(w, access)?
        } else {
            let mut w = Vec::new();
            w.try_reserve_exact(y.len())
                .map_err(|_| "supersmoother weight allocation failed")?;
            w.resize(y.len(), 1.);
            w
        };
        if weights.len() != y.len() {
            return Err("number of weights must match number of observations.".into());
        }
        let periodic = scalar(4, 0.)?;
        if !periodic.is_finite() {
            return Err("invalid periodic flag".into());
        }
        let periodic = periodic != 0.;
        let alpha = scalar(5, 0.)?;
        let trace = scalar(6, 0.)?;
        if !trace.is_finite() {
            return Err("invalid trace flag".into());
        }
        if trace != 0. {
            return Err("supersmoother trace output is not implemented".into());
        }
        if periodic && x.iter().any(|x| !x.is_finite() || *x < 0. || *x > 1.) {
            return Err("'x' must be between 0 and 1 for periodic smooth".into());
        }
        let mut order: Vec<_> = (0..y.len())
            .filter(|&i| (x[i] + y[i] + weights[i]).is_finite())
            .collect();
        if order.is_empty() {
            return Err("no finite observations".into());
        }
        order.sort_by(|&a, &b| {
            x[a].partial_cmp(&x[b])
                .unwrap()
                .then_with(|| y[a].partial_cmp(&y[b]).unwrap())
        });
        if order.len() != y.len() {
            let deleted = y.len() - order.len();
            let text = if deleted == 1 {
                format!("{deleted} observation with NA, NaN or Inf deleted")
            } else {
                format!("{deleted} observations with NAs, NaNs and/or Infs deleted")
            };
            let message = std::ffi::CString::new(text).unwrap();
            access
                .with_native(|_| {
                    unsafe {
                        crate::mainutils::errors::Rf_warning1(message.as_ptr());
                    }
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
        }
        // GNU performs as.double(span) only after observation validation and
        // deletion warnings. Keep the original input rooted through this last
        // provider read and any coercion warning callback.
        let span = if let Some(v) = character_span {
            let text = v.try_string_value_elt(0).map_err(|e| e.to_string())?;
            access.require_active().map_err(|e| e.to_string())?;
            let text = text.ok_or("NA/NaN/Inf in foreign function call (arg 6)")?;
            let text = std::ffi::CString::new(text).map_err(|e| e.to_string())?;
            let value = access
                .with_native(|_| {
                    // SAFETY: the canonical parser receives a live owned CString;
                    // its end pointer stays within that same NUL-terminated buffer.
                    let (value, complete) = unsafe {
                        let mut end = std::ptr::null_mut();
                        let value = crate::mainutils::util_main::R_strtod(text.as_ptr(), &mut end);
                        let tail = std::ffi::CStr::from_ptr(end).to_bytes();
                        (value, tail.iter().all(u8::is_ascii_whitespace))
                    };
                    if !complete {
                        unsafe {
                            crate::mainutils::errors::Rf_warning1(
                                c"NAs introduced by coercion".as_ptr(),
                            );
                        }
                        Ok(f64::NAN)
                    } else {
                        Ok(value)
                    }
                })
                .map_err(|e| e.to_string())?;
            if !value.is_finite() {
                return Err("NA/NaN/Inf in foreign function call (arg 6)".into());
            }
            value
        } else {
            span
        };
        let xo: Vec<_> = order.iter().map(|&i| x[i]).collect();
        let yo: Vec<_> = order.iter().map(|&i| y[i]).collect();
        let wo: Vec<_> = order.iter().map(|&i| weights[i]).collect();
        let n = order.len();
        let width = n.checked_mul(7).ok_or("supersmoother workspace overflow")?;
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(width)
            .map_err(|_| "supersmoother workspace allocation failed")?;
        scratch.resize(width, 0.);
        let mut smooth = Vec::new();
        smooth
            .try_reserve_exact(n)
            .map_err(|_| "supersmoother output allocation failed")?;
        smooth.resize(n, 0.);
        let mut edf = [0.];
        super::supsmu::filter(
            super::supsmu::Input {
                x: &xo,
                y: &yo,
                weights: &wo,
            },
            super::supsmu::Parameters {
                n,
                periodic: if periodic { 2 } else { 1 },
                span,
                alpha,
            },
            super::supsmu::Output {
                smoothed: &mut smooth,
                scratch: &mut scratch,
                edf: &mut edf,
            },
        )
        .map_err(|e| e.to_string())?;
        let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
        let allocate = |kind, len| {
            allocator
                .allocate(|arena| arena.alloc_vector_sexp(kind, len).map(|v| v.as_raw()))
                .and_then(Sexp::into_owned)
                .map_err(|e| e.to_string())
        };
        let keep: Vec<_> = (0..n).filter(|&i| i == 0 || xo[i] != xo[i - 1]).collect();
        let mut xout =
            SexpMut::try_from_checked(allocate(SEXPTYPE::REALSXP, keep.len() as R_xlen_t)?)
                .map_err(|e| e.to_string())?;
        let mut yout =
            SexpMut::try_from_checked(allocate(SEXPTYPE::REALSXP, keep.len() as R_xlen_t)?)
                .map_err(|e| e.to_string())?;
        for (i, &j) in keep.iter().enumerate() {
            xout.try_set_real_elt(i as R_xlen_t, xo[j])
                .map_err(|e| e.to_string())?;
            yout.try_set_real_elt(i as R_xlen_t, smooth[j])
                .map_err(|e| e.to_string())?;
        }
        let mut result =
            SexpMut::try_from_checked(allocate(SEXPTYPE::VECSXP, 2)?).map_err(|e| e.to_string())?;
        result
            .try_set_vector_elt(0, xout.freeze())
            .map_err(|e| e.to_string())?;
        result
            .try_set_vector_elt(1, yout.freeze())
            .map_err(|e| e.to_string())?;
        let result = result.freeze();
        let names = allocator
            .strings(&["x", "y"])
            .and_then(Sexp::into_owned)
            .map_err(|e| e.to_string())?;
        access
            .with_native(|_| {
                unsafe {
                    crate::sexp::attrib_core::setAttrib(
                        result.as_raw(),
                        crate::sexp::attrib_core::R_NamesSymbol(),
                        names.as_raw(),
                    );
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        access.require_active().map_err(|e| e.to_string())?;
        Ok(result.as_raw())
    })
    .map_err(|e| e.to_string())
    .unwrap_or_else(|e| fail(e));
    result.unwrap_or_else(|e| fail(e))
}
