use super::*;

pub(crate) unsafe fn unit_kind(x: SEXP) -> String {
    unsafe {
        let u = field(x, "units");
        let name = string(u, "");
        if name.is_empty() {
            base_error("invalid grid unit names");
        }
        name
    }
}
pub(crate) unsafe fn unit_values(x: SEXP) -> Vec<f64> {
    unsafe {
        match SEXPTYPE(TYPEOF(x)) {
            SEXPTYPE::REALSXP => (0..XLENGTH(x)).map(|i| *REAL(x).add(i as usize)).collect(),
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => (0..XLENGTH(x))
                .map(|i| {
                    let n = if TYPEOF(x) == SEXPTYPE::INTSXP {
                        *INTEGER(x).add(i as usize)
                    } else {
                        *LOGICAL(x).add(i as usize)
                    };
                    if n == i32::MIN {
                        crate::sexp::ffi::NA_REAL
                    } else {
                        f64::from(n)
                    }
                })
                .collect(),
            _ => base_error("unit arithmetic requires numeric values"),
        }
    }
}
pub(crate) unsafe fn unit_result(template: SEXP, values: &[f64]) -> SEXP {
    unsafe {
        use crate::sexp::protect::protect;
        let _template = protect(template);
        let value = result(values);
        let _value = protect(value);
        let obj = crate::mainutils::seq::Rf_shallow_duplicate(template);
        let _obj = protect(obj);
        // Preserve attributes, unit names and string/grob data while replacing
        // only the owned numeric coefficient vector.
        let names = getAttrib(obj, R_NamesSymbol());
        for i in 0..XLENGTH(obj).min(XLENGTH(names)) {
            if elt_to_string(names, i) == "value" {
                SET_VECTOR_ELT(obj, i, value);
                return obj;
            }
        }
        base_error("invalid grid unit: missing values")
    }
}

// Build the small unit object used by the portable grid R wrappers.  Arithmetic
// units use `units` = the operation name and `data` = a list of scalar units;
// this mirrors grid's unit_v2 expression nodes without making the rest of the
// portable API depend on grid's private representation.
pub(crate) unsafe fn scalar_unit(value: f64, name: &str, data: SEXP) -> SEXP {
    unsafe {
        let _data_arg = crate::sexp::protect::protect(data);
        let out = Rf_allocVector(SEXPTYPE::VECSXP, 3);
        let _out = crate::sexp::protect::protect(out);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, 3);
        let _names = crate::sexp::protect::protect(names);
        for (i, n) in ["value", "units", "data"].iter().enumerate() {
            SET_STRING_ELT(
                names,
                i as i64,
                Rf_mkCharLen(n.as_ptr() as *const _, n.len() as i32),
            );
        }
        setAttrib(out, R_NamesSymbol(), names);
        SET_VECTOR_ELT(out, 0, Rf_ScalarReal(value));
        let unit_name = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        let _unit_name = crate::sexp::protect::protect(unit_name);
        SET_STRING_ELT(
            unit_name,
            0,
            Rf_mkCharLen(name.as_ptr() as *const _, name.len() as i32),
        );
        SET_VECTOR_ELT(out, 1, unit_name);
        if name == ".rport-expression" {
            let elements = Rf_allocVector(SEXPTYPE::VECSXP, 1);
            SET_VECTOR_ELT(elements, 0, data);
            SET_VECTOR_ELT(out, 2, elements);
        } else {
            SET_VECTOR_ELT(out, 2, data);
        }
        let class = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        let _class = crate::sexp::protect::protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"unit".as_ptr()));
        setAttrib(out, R_ClassSymbol(), class);
        out
    }
}

pub(crate) unsafe fn unit_element(x: SEXP, i: usize) -> SEXP {
    unsafe {
        let values = field(x, "value");
        let names = field(x, "units");
        let data = field(x, "data");
        let value = if XLENGTH(values) == 0 {
            0.
        } else if TYPEOF(values) == SEXPTYPE::REALSXP {
            *REAL(values).add(i % XLENGTH(values) as usize)
        } else if TYPEOF(values) == SEXPTYPE::INTSXP {
            *INTEGER(values).add(i % XLENGTH(values) as usize) as f64
        } else {
            base_error("invalid grid unit values");
        };
        let name = if XLENGTH(names) == 0 {
            "npc".into()
        } else {
            elt_to_string(names, (i as i64) % XLENGTH(names))
        };
        let datum = if matches!(name.as_str(), "sum" | "min" | "max") {
            data
        } else if data == R_NilValue() || XLENGTH(data) == 0 {
            R_NilValue()
        } else if TYPEOF(data) == SEXPTYPE::STRSXP {
            let d = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let _d = crate::sexp::protect::protect(d);
            SET_STRING_ELT(d, 0, STRING_ELT(data, (i as i64) % XLENGTH(data)));
            d
        } else {
            VECTOR_ELT(data, (i as i64) % XLENGTH(data))
        };
        scalar_unit(value, &name, datum)
    }
}
pub(crate) unsafe fn unit_name_at(x: SEXP, i: usize) -> String {
    unsafe {
        let names = field(x, "units");
        if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) == 0 {
            base_error("invalid grid unit names");
        }
        elt_to_string(names, (i as i64) % XLENGTH(names))
    }
}
pub(crate) unsafe fn unit_data_at(x: SEXP, i: usize) -> SEXP {
    unsafe {
        if matches!(unit_name_at(x, i).as_str(), "sum" | "min" | "max") {
            return x;
        }
        let data = field(x, "data");
        if data == R_NilValue() || XLENGTH(data) == 0 {
            return R_NilValue();
        }
        if TYPEOF(data) == SEXPTYPE::VECSXP {
            VECTOR_ELT(data, (i as i64) % XLENGTH(data))
        } else {
            data
        }
    }
}

pub(crate) unsafe fn expression_unit(op: &str, terms: &[SEXP]) -> SEXP {
    unsafe {
        let _terms: Vec<_> = terms
            .iter()
            .map(|term| crate::sexp::protect::protect(*term))
            .collect();
        let data = Rf_allocVector(SEXPTYPE::VECSXP, terms.len() as i32);
        let _data = crate::sexp::protect::protect(data);
        for (i, term) in terms.iter().enumerate() {
            SET_VECTOR_ELT(data, i as i64, *term);
        }
        scalar_unit(1., op, data)
    }
}
pub(crate) enum UnitTerm {
    Scalar(f64, String, SEXP),
    Expr(SEXP),
}
pub(crate) unsafe fn unit_terms_result(template: SEXP, terms: &[UnitTerm]) -> SEXP {
    unsafe {
        let _template = crate::sexp::protect::protect(template);
        let _exprs: Vec<_> = terms
            .iter()
            .map(|term| match term {
                UnitTerm::Expr(e) | UnitTerm::Scalar(_, _, e) => crate::sexp::protect::protect(*e),
            })
            .collect();
        let values = Rf_allocVector(SEXPTYPE::REALSXP, terms.len() as i32);
        let _values = crate::sexp::protect::protect(values);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, terms.len() as i32);
        let _names = crate::sexp::protect::protect(names);
        let data = Rf_allocVector(SEXPTYPE::VECSXP, terms.len() as i32);
        let _data = crate::sexp::protect::protect(data);
        let mut any_data = false;
        for (i, term) in terms.iter().enumerate() {
            let (v, n, d) = match term {
                UnitTerm::Scalar(v, n, d) if matches!(n.as_str(), "sum" | "min" | "max") => {
                    (*v, ".rport-expression", *d)
                }
                UnitTerm::Scalar(v, n, d) => (*v, n.as_str(), *d),
                UnitTerm::Expr(e) => (1., ".rport-expression", *e),
            };
            *REAL(values).add(i) = v;
            SET_STRING_ELT(
                names,
                i as i64,
                Rf_mkCharLen(n.as_ptr() as *const _, n.len() as i32),
            );
            SET_VECTOR_ELT(data, i as i64, d);
            any_data |= d != R_NilValue();
        }
        let obj = crate::mainutils::seq::Rf_shallow_duplicate(template);
        let nms = getAttrib(obj, R_NamesSymbol());
        for i in 0..XLENGTH(obj).min(XLENGTH(nms)) {
            match elt_to_string(nms, i).as_str() {
                "value" => SET_VECTOR_ELT(obj, i, values),
                "units" => SET_VECTOR_ELT(obj, i, names),
                "data" => SET_VECTOR_ELT(obj, i, if any_data { data } else { R_NilValue() }),
                _ => {}
            }
        }
        obj
    }
}
pub unsafe fn unit_binary(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let au = crate::mainutils::essentials::sexp_has_class(a, "unit");
        let bu = crate::mainutils::essentials::sexp_has_class(b, "unit");
        if !au && !bu {
            return None;
        }
        let unary = b == R_NilValue();
        match op {
            "+" | "-" if au && (bu || unary) => {}
            "*" if au != bu && !unary => {}
            "/" if au && !bu && !unary => {}
            _ => base_error("invalid unit arithmetic operands"),
        }
        let template = if au { a } else { b };
        let mut expr_roots = Vec::new();
        let av = unit_values(if au { field(a, "value") } else { a });
        let out: Vec<UnitTerm> = if unary {
            av.into_iter()
                .enumerate()
                .map(|(i, v)| {
                    UnitTerm::Scalar(
                        if op == "-" { -v } else { v },
                        unit_name_at(template, i),
                        unit_data_at(template, i),
                    )
                })
                .collect()
        } else {
            let bv = unit_values(if bu { field(b, "value") } else { b });
            let n = if av.is_empty() || bv.is_empty() {
                0
            } else {
                av.len().max(bv.len())
            };
            (0..n)
                .map(|i| {
                    let x = av[i % av.len()];
                    let y = bv[i % bv.len()];
                    if au && bu {
                        let an = elt_to_string(
                            field(a, "units"),
                            (i as i64) % XLENGTH(field(a, "units")),
                        );
                        let bn = elt_to_string(
                            field(b, "units"),
                            (i as i64) % XLENGTH(field(b, "units")),
                        );
                        if an != bn
                            || field(a, "data") != R_NilValue()
                            || field(b, "data") != R_NilValue()
                        {
                            let left = unit_element(a, i);
                            let _left = crate::sexp::protect::protect(left);
                            let mut right = unit_element(b, i);
                            let _right = crate::sexp::protect::protect(right);
                            if op == "-" {
                                let right_data = field(right, "data");
                                right = scalar_unit(-y, &bn, right_data);
                            }
                            let _right_adjusted = crate::sexp::protect::protect(right);
                            let expr = expression_unit("sum", &[left, right]);
                            expr_roots.push(crate::sexp::protect::protect(expr));
                            return UnitTerm::Expr(expr);
                        }
                    }
                    UnitTerm::Scalar(
                        match op {
                            "+" => x + y,
                            "-" => x - y,
                            "*" => x * y,
                            "/" => x / y,
                            _ => unreachable!(),
                        },
                        if au {
                            unit_name_at(a, i)
                        } else {
                            unit_name_at(b, i)
                        },
                        if au && !bu {
                            unit_data_at(a, i)
                        } else if bu && !au {
                            unit_data_at(b, i)
                        } else {
                            R_NilValue()
                        },
                    )
                })
                .collect::<Vec<_>>()
        };
        Some(unit_terms_result(template, &out))
    }
}
pub unsafe fn unit_summary(op: &str, args: SEXP) -> Option<SEXP> {
    unsafe {
        let na_tag = crate::sexp::symbol::Rf_install(c"na.rm".as_ptr());
        let mut p = args;
        let mut template = R_NilValue();
        while p != R_NilValue() {
            if TAG(p) != na_tag && crate::mainutils::essentials::sexp_has_class(CAR(p), "unit") {
                template = CAR(p);
                break;
            }
            p = CDR(p);
        }
        if template == R_NilValue() {
            return None;
        }
        if !matches!(op, "sum" | "min" | "max") {
            base_error("unit summary is not supported");
        }
        let mut values = Vec::new();
        let mut expression_terms = Vec::new();
        let mut expression_roots = Vec::new();
        let mut mixed = false;
        let first_kind = unit_kind(template);
        p = args;
        while p != R_NilValue() {
            if TAG(p) != na_tag {
                let x = CAR(p);
                if !crate::mainutils::essentials::sexp_has_class(x, "unit") {
                    base_error("unit summary requires matching units");
                }
                mixed |= unit_kind(x) != first_kind || field(x, "data") != R_NilValue();
                for i in 0..XLENGTH(field(x, "value")) {
                    mixed |= unit_name_at(x, i as usize) != first_kind;
                    let term = unit_element(x, i as usize);
                    expression_roots.push(crate::sexp::protect::protect(term));
                    expression_terms.push(term);
                }
                values.extend(unit_values(field(x, "value")));
            }
            p = CDR(p);
        }
        // GNU R grid propagates missing coefficients even with na.rm=TRUE.
        let v = if values
            .iter()
            .any(|v| v.to_bits() == crate::sexp::ffi::NA_REAL.to_bits())
        {
            crate::sexp::ffi::NA_REAL
        } else if values.iter().any(|v| v.is_nan()) {
            f64::NAN
        } else {
            match op {
                "sum" => values.iter().sum(),
                "min" => values.iter().copied().fold(f64::INFINITY, f64::min),
                "max" => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                _ => unreachable!(),
            }
        };
        if mixed {
            Some(expression_unit(op, &expression_terms))
        } else {
            Some(unit_result(template, &[v]))
        }
    }
}
