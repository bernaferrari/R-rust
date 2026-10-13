//! tsp series accessors: cycle/time/as.ts/window/lag/start/end/frequency

use super::*;

unsafe fn ts_start_freq(x: SEXP) -> (f64, f64) {
    unsafe {
        let tsp = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
        );
        if !tsp.is_null() && TYPEOF(tsp) == SEXPTYPE::REALSXP && XLENGTH(tsp) >= 3 {
            (*REAL(tsp), *REAL(tsp).add(2))
        } else {
            (1.0, 1.0)
        }
    }
}

/// GNU `cycle(ts)`.
pub unsafe fn do_cycle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x);
        let (start, freq) = ts_start_freq(x);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _a = protect(ans);
        let start_cycle = ((start - 1.0) * freq).round();
        for i in 0..n as usize {
            let cyc = (start_cycle + i as f64).rem_euclid(freq) + 1.0;
            *REAL(ans).add(i) = cyc;
        }
        ans
    }
}

/// GNU `time(ts)`.
pub unsafe fn do_time(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x);
        let (start, freq) = ts_start_freq(x);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _a = protect(ans);
        for i in 0..n as usize {
            *REAL(ans).add(i) = start + i as f64 / freq;
        }
        ans
    }
}

/// GNU `as.ts(x)`.
pub unsafe fn do_as_ts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'ts' object must have one or more observations",
            );
        }
        let n = XLENGTH(x);
        if n == 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'ts' object must have one or more observations",
            );
        }
        let existing = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
        );
        if !existing.is_null() && existing != R_NilValue() {
            return x;
        }
        let tsp = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _t = protect(tsp);
        *REAL(tsp) = 1.0;
        *REAL(tsp).add(1) = n as f64;
        *REAL(tsp).add(2) = 1.0;
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
            tsp,
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ts".as_ptr()),
        );
        x
    }
}

/// GNU `plot.ts(x)` — `as.ts(x)` then plot.
pub unsafe fn do_plot_ts(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let as_args = Rf_cons(x, R_NilValue());
        let _aa = protect(as_args);
        let _ = do_as_ts(call, op, as_args, rho);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `ts.plot(...)` — `ts.union(...)` then `plot.ts`.
pub unsafe fn do_ts_plot(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let sers = crate::mainutils::essentials::do_ts_union(call, op, args, rho);
        let _s = protect(sers);
        let plot_args = Rf_cons(sers, R_NilValue());
        let _pa = protect(plot_args);
        do_plot_ts(call, op, plot_args, rho)
    }
}

/// GNU `hasTsp(x)` — ensure a `tsp` attribute, do not set class.
pub unsafe fn do_has_tsp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let existing = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
        );
        if !existing.is_null() && existing != R_NilValue() {
            return x;
        }
        let n = XLENGTH(x);
        let tsp = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _t = protect(tsp);
        *REAL(tsp) = 1.0;
        *REAL(tsp).add(1) = n as f64;
        *REAL(tsp).add(2) = 1.0;
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
            tsp,
        );
        x
    }
}

/// GNU `window(ts, start, end)`.
pub unsafe fn do_window(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let start = CAR(CDR(args));
        let end = CAR(CDR(CDR(args)));
        let (t0, freq) = ts_start_freq(x);
        let s = if TYPEOF(start) == SEXPTYPE::REALSXP {
            *REAL(start)
        } else {
            *INTEGER(start) as f64
        };
        let e = if TYPEOF(end) == SEXPTYPE::REALSXP {
            *REAL(end)
        } else {
            *INTEGER(end) as f64
        };
        let i0 = ((s - t0) * freq).round() as i64;
        let i1 = ((e - t0) * freq).round() as i64;
        let n = XLENGTH(x);
        let lo = i0.max(0);
        let hi = i1.min(n - 1);
        let out_n = (hi - lo + 1).max(0);
        let ty = TYPEOF(x);
        let ans = Rf_allocVector3(
            if ty == SEXPTYPE::INTSXP {
                SEXPTYPE::INTSXP
            } else {
                SEXPTYPE::REALSXP
            },
            out_n,
        );
        let _a = protect(ans);
        for i in 0..out_n as usize {
            let src = (lo as usize) + i;
            if ty == SEXPTYPE::INTSXP {
                *INTEGER(ans).add(i) = *INTEGER(x).add(src);
            } else {
                *REAL(ans).add(i) = *REAL(x).add(src);
            }
        }
        ans
    }
}

/// GNU `aggregate.ts(x, nfrequency=1, FUN=sum)` — block sums.
pub unsafe fn do_aggregate_ts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let (t0, freq) = ts_start_freq(x);
        let nfreq_s = CAR(CDR(args));
        let nfreq = if nfreq_s.is_null()
            || nfreq_s == R_NilValue()
            || nfreq_s == crate::sexp::globals::R_MissingArg()
        {
            1.0
        } else if TYPEOF(nfreq_s) == SEXPTYPE::REALSXP {
            *REAL(nfreq_s)
        } else {
            *INTEGER(nfreq_s) as f64
        };
        let len = (freq / nfreq).round() as usize;
        if len == 0 {
            return x;
        }
        let n = XLENGTH(x) as usize;
        let nout = n / len;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nout as i64);
        let _r = protect(result);
        for i in 0..nout {
            let mut s = 0.0;
            for j in 0..len {
                let idx = i * len + j;
                s += if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(idx)
                } else {
                    *INTEGER(x).add(idx) as f64
                };
            }
            *REAL(result).add(i) = s;
        }
        let tsp = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _t = protect(tsp);
        *REAL(tsp) = t0;
        *REAL(tsp).add(1) = t0 + (nout as f64 - 1.0) / nfreq;
        *REAL(tsp).add(2) = nfreq;
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
            tsp,
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ts".as_ptr()),
        );
        result
    }
}

/// GNU `window(x, start, end) <- value` — replace a freq-1 window.
pub unsafe fn do_window_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let start = CAR(CDR(args));
        let end = CAR(CDR(CDR(args)));
        let value = CAR(CDR(CDR(CDR(args))));
        let (t0, freq) = ts_start_freq(x);
        let s = if TYPEOF(start) == SEXPTYPE::REALSXP {
            *REAL(start)
        } else {
            *INTEGER(start) as f64
        };
        let e = if TYPEOF(end) == SEXPTYPE::REALSXP {
            *REAL(end)
        } else {
            *INTEGER(end) as f64
        };
        let val = if TYPEOF(value) == SEXPTYPE::REALSXP {
            *REAL(value)
        } else {
            *INTEGER(value) as f64
        };
        let n = XLENGTH(x) as usize;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _r = protect(result);
        for i in 0..n {
            let t = t0 + i as f64 / freq;
            let src = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
            *REAL(result).add(i) = if t + 1e-9 >= s && t <= e + 1e-9 {
                val
            } else {
                src
            };
        }
        let tsp = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
        );
        if !tsp.is_null() && tsp != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
                tsp,
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ts".as_ptr()),
        );
        result
    }
}

/// GNU `lag(ts, k)` shifts tsp, keeps values.
pub unsafe fn do_lag(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let k_s = CAR(CDR(args));
        let k = if TYPEOF(k_s) == SEXPTYPE::REALSXP {
            *REAL(k_s)
        } else {
            *INTEGER(k_s) as f64
        };
        let (start, freq) = ts_start_freq(x);
        let n = XLENGTH(x) as f64;
        let tsp = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _t = protect(tsp);
        *REAL(tsp) = start - k / freq;
        *REAL(tsp).add(1) = start - k / freq + (n - 1.0) / freq;
        *REAL(tsp).add(2) = freq;
        let ans = if TYPEOF(x) == SEXPTYPE::INTSXP {
            let a = Rf_allocVector3(SEXPTYPE::INTSXP, n as i64);
            for i in 0..n as usize {
                *INTEGER(a).add(i) = *INTEGER(x).add(i);
            }
            a
        } else {
            let a = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            for i in 0..n as usize {
                *REAL(a).add(i) = *REAL(x).add(i);
            }
            a
        };
        let _a = protect(ans);
        crate::sexp::attrib_core::setAttrib(
            ans,
            crate::sexp::symbol::Rf_install(c"tsp".as_ptr()),
            tsp,
        );
        crate::sexp::attrib_core::setAttrib(
            ans,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"ts".as_ptr()),
        );
        ans
    }
}

/// GNU `frequency(ts)`.
pub unsafe fn do_frequency(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let (_s, freq) = ts_start_freq(CAR(args));
        Rf_ScalarReal(freq)
    }
}

/// GNU `deltat(ts)`.
pub unsafe fn do_deltat(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let (_s, freq) = ts_start_freq(CAR(args));
        Rf_ScalarReal(1.0 / freq)
    }
}

unsafe fn time_to_ycyc(t: f64, freq: f64) -> (f64, f64) {
    let mut year = t.floor();
    let mut cyc = ((t - year) * freq).round() + 1.0;
    if cyc > freq {
        year += 1.0;
        cyc = 1.0;
    }
    if cyc < 1.0 {
        year -= 1.0;
        cyc = freq;
    }
    (year, cyc)
}

/// GNU `start(ts)` as c(year, cycle).
pub unsafe fn do_start(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let (start, freq) = ts_start_freq(x);
        let (y, c) = time_to_ycyc(start, freq);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _a = protect(ans);
        *REAL(ans) = y;
        *REAL(ans).add(1) = c;
        ans
    }
}

/// GNU `end(ts)` as c(year, cycle).
pub unsafe fn do_end(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x);
        let (start, freq) = ts_start_freq(x);
        let t = start + (n as f64 - 1.0) / freq;
        let (y, c) = time_to_ycyc(t, freq);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _a = protect(ans);
        *REAL(ans) = y;
        *REAL(ans).add(1) = c;
        ans
    }
}

/// GNU `is.ts(x)`.
pub unsafe fn do_is_ts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        let mut ok = false;
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i)));
                if s.to_bytes() == b"ts" {
                    ok = true;
                    break;
                }
            }
        }
        Rf_ScalarLogical(if ok { 1 } else { 0 })
    }
}

/// GNU `is.mts(x)` — ts matrix with class `mts`.
pub unsafe fn do_is_mts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        let mut has_ts = false;
        let mut has_mts = false;
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(class) {
                let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(class, i)));
                if s.to_bytes() == b"ts" {
                    has_ts = true;
                }
                if s.to_bytes() == b"mts" {
                    has_mts = true;
                }
            }
        }
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let is_mat = !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2;
        Rf_ScalarLogical(if has_ts && has_mts && is_mat { 1 } else { 0 })
    }
}
