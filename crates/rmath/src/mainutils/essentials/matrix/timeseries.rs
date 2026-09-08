//! Time-series construction.
//!
//! `ts()` is defined in the stats package in GNU R, but the port currently
//! starts with a compact built-in environment instead of sourcing stats/R/ts.R.
//! Keep the compatibility surface isolated here until package startup can load
//! the upstream closure directly.

use super::*;

unsafe fn supplied_arg(args: SEXP, name: &str, position: usize) -> Option<SEXP> {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some(name) {
                return Some(CAR(current));
            }
            current = CDR(current);
        }

        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                if positional == position {
                    return Some(CAR(current));
                }
                positional += 1;
            }
            current = CDR(current);
        }
        None
    }
}

unsafe fn time_value(value: SEXP, default: f64) -> f64 {
    unsafe {
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return default;
        }
        real_elt_or_default(value, 0, default)
    }
}

unsafe fn set_ts_matrix_dimnames(result: SEXP, data: SEXP, nseries: R_xlen_t, args: SEXP) {
    unsafe {
        let supplied_names = supplied_arg(args, "names", 7);
        let source_dimnames =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_DimNamesSymbol());
        let source_names = if source_dimnames != R_NilValue()
            && TYPEOF(source_dimnames) == SEXPTYPE::VECSXP
            && XLENGTH(source_dimnames) > 1
        {
            VECTOR_ELT(source_dimnames, 1)
        } else {
            R_NilValue()
        };
        let names = if let Some(names) = supplied_names {
            names
        } else if source_names != R_NilValue() {
            source_names
        } else {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, nseries);
            let _names_guard = protect(names);
            for i in 0..nseries {
                let label = CString::new(format!("Series {}", i + 1)).unwrap_or_default();
                SET_STRING_ELT(names, i, Rf_mkChar(label.as_ptr()));
            }
            names
        };
        let _names_guard = protect(names);
        if names != R_NilValue() && XLENGTH(names) != nseries {
            std::panic::panic_any(RError {
                message: "length of 'dimnames' [2] not equal to array extent".to_string(),
            });
        }

        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dimnames_guard = protect(dimnames);
        SET_VECTOR_ELT(dimnames, 0, R_NilValue());
        SET_VECTOR_ELT(dimnames, 1, names);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dimnames,
        );
    }
}

unsafe fn resize_series(data: SEXP, ndata: R_xlen_t, nseries: R_xlen_t, nobs: R_xlen_t) -> SEXP {
    unsafe {
        if nobs == ndata {
            return crate::mainutils::duplicate::Rf_duplicate(data);
        }

        let result = Rf_allocVector3(TYPEOF(data), nobs * nseries);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for series in 0..nseries {
            for observation in 0..nobs {
                let source_observation = observation % ndata;
                copy_matrix_element(
                    result,
                    series * nobs + observation,
                    data,
                    series * ndata + source_observation,
                );
            }
        }

        if nseries > 1 {
            set_two_dim_attr(result, nobs, nseries);
            let dimnames = crate::sexp::attrib_core::getAttrib(
                data,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
            );
            if dimnames != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    dimnames,
                );
            }
        }
        result
    }
}

unsafe fn default_ts_class(nseries: R_xlen_t) -> SEXP {
    unsafe {
        if nseries == 1 {
            return Rf_mkString(c"ts".as_ptr());
        }
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 4);
        if class.is_null() {
            return R_NilValue();
        }
        for (index, name) in [c"mts", c"ts", c"matrix", c"array"].iter().enumerate() {
            SET_STRING_ELT(class, index as R_xlen_t, Rf_mkChar(name.as_ptr()));
        }
        class
    }
}

/// GNU R's `stats::ts()` constructor.
///
/// This implements the general numeric/vector and matrix construction surface:
/// named or positional start/end/frequency/deltat arguments, calendar-style
/// two-component endpoints, recycling/truncation to the requested span, and
/// the canonical `tsp` and class attributes.
pub unsafe fn do_ts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let data = supplied_arg(args, "data", 0).unwrap_or_else(|| Rf_ScalarReal(NA_REAL));
        let _data_guard = protect(data);
        let dim =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_DimSymbol());
        let (ndata, nseries) = if dim != R_NilValue() && XLENGTH(dim) == 2 {
            (
                real_elt_or_default(dim, 0, 0.0) as R_xlen_t,
                real_elt_or_default(dim, 1, 0.0) as R_xlen_t,
            )
        } else {
            (XLENGTH(data), 1)
        };
        if ndata == 0 {
            std::panic::panic_any(RError {
                message: "'ts' object must have one or more observations".to_string(),
            });
        }

        let frequency_arg = supplied_arg(args, "frequency", 3);
        let deltat_arg = supplied_arg(args, "deltat", 4);
        let mut frequency = match (frequency_arg, deltat_arg) {
            (Some(value), _) => real_elt_or_default(value, 0, 1.0),
            (None, Some(value)) => 1.0 / real_elt_or_default(value, 0, 1.0),
            (None, None) => 1.0,
        };
        if !frequency.is_finite() || frequency <= 0.0 {
            std::panic::panic_any(RError {
                message: "invalid 'frequency' argument".to_string(),
            });
        }
        let ts_eps = supplied_arg(args, "ts.eps", 5)
            .map(|value| real_elt_or_default(value, 0, 1e-5))
            .unwrap_or(1e-5);
        let distance_to_integer = (frequency - frequency.round()).abs();
        if frequency > 1.0 && distance_to_integer > 0.0 && distance_to_integer < ts_eps {
            frequency = frequency.round();
        }

        let start_arg = supplied_arg(args, "start", 1);
        let end_arg = supplied_arg(args, "end", 2);
        let mut start = start_arg.map_or(1.0, |value| time_value(value, 1.0));
        let mut end = end_arg.map_or(f64::NAN, |value| time_value(value, f64::NAN));
        // Calendar-style c(year, period) endpoints use the selected frequency.
        if let Some(value) = start_arg
            && XLENGTH(value) > 1
        {
            start = real_elt_or_default(value, 0, 1.0)
                + (real_elt_or_default(value, 1, 1.0) - 1.0) / frequency;
        }
        if let Some(value) = end_arg
            && XLENGTH(value) > 1
        {
            end = real_elt_or_default(value, 0, 1.0)
                + (real_elt_or_default(value, 1, 1.0) - 1.0) / frequency;
        }
        if end_arg.is_none() {
            end = start + (ndata - 1) as f64 / frequency;
        } else if start_arg.is_none() {
            start = end - (ndata - 1) as f64 / frequency;
        }
        if !start.is_finite() || !end.is_finite() || start > end {
            std::panic::panic_any(RError {
                message: "'start' cannot be after 'end'".to_string(),
            });
        }

        let cycles = (end - start) * frequency;
        let rounded_cycles = cycles.round();
        if (rounded_cycles - cycles).abs() > 1e-5 * cycles.max(1.0) {
            std::panic::panic_any(RError {
                message: "'end' must be a whole number of cycles after 'start'".to_string(),
            });
        }
        let nobs = (cycles + 1.01).floor() as R_xlen_t;
        let result = resize_series(data, ndata, nseries, nobs);
        if result == R_NilValue() {
            return result;
        }
        let _result_guard = protect(result);
        if nseries > 1 {
            set_ts_matrix_dimnames(result, data, nseries, args);
        }

        let tsp = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _tsp_guard = protect(tsp);
        *REAL(tsp) = start;
        *REAL(tsp).add(1) = end;
        *REAL(tsp).add(2) = frequency;
        crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_TspSymbol(), tsp);

        let class_arg = supplied_arg(args, "class", 6);
        let class = class_arg.unwrap_or_else(|| default_ts_class(nseries));
        let _class_guard = protect(class);
        let class_is_none = TYPEOF(class) == SEXPTYPE::STRSXP
            && XLENGTH(class) > 0
            && CStr::from_ptr(CHAR(STRING_ELT(class, 0))).to_bytes() == b"none";
        if class != R_NilValue() && !class_is_none {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use crate::sexp::ffi::TRUE;
    use crate::sexp::session::RSession;

    #[test]
    fn ts_builds_calendar_tsp_and_class() {
        let mut session = RSession::new();
        let (result, output, visible) = session
            .eval_code_with_output_capture("z <- ts(1:10, frequency = 4, start = c(1959, 2))");
        result.expect("ts construction should evaluate");
        assert!(output.stdout.is_empty());
        assert!(!visible);

        let (tsp, _, _) = session.eval_code_with_output_capture("tsp(z)");
        let tsp = tsp.expect("tsp should be readable");
        assert_eq!(tsp.clone().real_elt(0), Some(1959.25));
        assert_eq!(tsp.clone().real_elt(1), Some(1961.5));
        assert_eq!(tsp.real_elt(2), Some(4.0));

        let (class, _, _) = session.eval_code_with_output_capture("identical(class(z), \"ts\")");
        assert_eq!(
            class.expect("class should be readable").logical_elt(0),
            Some(TRUE)
        );

        let (values, _, _) = session.eval_code_with_output_capture("as.integer(z)");
        let values = values.expect("series values should be readable");
        assert_eq!(values.clone().integer_elt(0), Some(1));
        assert_eq!(values.integer_elt(9), Some(10));
    }

    #[test]
    fn ts_honors_end_deltat_and_recycles_data() {
        let mut session = RSession::new();
        let (result, output, visible) = session.eval_code_with_output_capture(
            "z <- ts(1:2, start = 2, end = 3, deltat = 0.5); \
             identical(unclass(z), structure(c(1L, 2L, 1L), tsp = c(2, 3, 2)))",
        );
        let result = result.expect("ts construction should recycle data");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(output.stdout.is_empty());
        assert!(visible);
    }

    #[test]
    fn ts_matrix_uses_mts_class_and_series_names() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_code_with_output_capture(
            "z <- ts(matrix(1:6, 3, 2), names = c(\"left\", \"right\")); \
             identical(class(z), c(\"mts\", \"ts\", \"matrix\", \"array\")) && \
             identical(dimnames(z), list(NULL, c(\"left\", \"right\"))) && \
             identical(tsp(z), c(1, 3, 1))",
        );
        assert_eq!(
            result
                .expect("matrix ts construction should evaluate")
                .logical_elt(0),
            Some(TRUE)
        );
    }

    #[test]
    fn pinned_structure_driver_advances_through_ts_comparison() {
        let source = include_str!("../../../../../../tests/upstream-r/vendor/structure.R");
        let through_ts = source
            .split("## levels <-> .Label")
            .next()
            .expect("the pinned structure driver retains its levels section");
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(through_ts);
        result.expect("the unmodified upstream prefix through ts() should pass");
        assert!(
            output
                .stdout
                .contains("$class\n[1] \"ts\"\n\nstructure(1:10"),
            "the auto-printed attributes(z) list must retain stock's final separator before the following deparse output; got:\n{}",
            output.stdout
        );
    }
}
