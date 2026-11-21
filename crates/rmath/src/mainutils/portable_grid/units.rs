use super::*;

pub(crate) fn unit_kind(x: SEXP) -> String {
    let u = field(x, "units");
    let name = string(u, "");
    if name.is_empty() {
        base_error("invalid grid unit names");
    }
    name
}
pub(crate) fn unit_values(x: SEXP) -> Vec<f64> {
    let object = node_factory()
        .wrap(x)
        .unwrap_or_else(|error| base_error(error.to_string()));
    match SEXPTYPE(TYPEOF(x)) {
        SEXPTYPE::REALSXP => (0..XLENGTH(x))
            .map(|i| {
                object
                    .try_real_elt(i)
                    .unwrap_or_else(|error| base_error(error.to_string()))
            })
            .collect(),
        SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => (0..XLENGTH(x))
            .map(|i| {
                let n = if TYPEOF(x) == SEXPTYPE::INTSXP {
                    object.try_integer_elt(i)
                } else {
                    object.try_logical_elt(i)
                }
                .unwrap_or_else(|error| base_error(error.to_string()));
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
/// Checked real scalar; matches `Rf_ScalarReal` on the session heap.
fn scalar_real(value: f64) -> SEXP {
    let factory = node_factory();
    let vector = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, 1)))
        .unwrap_or_else(|error| base_error(error.to_string()));
    let mut vector = crate::sexp::object::SexpMut::try_from_checked(vector)
        .unwrap_or_else(|error| base_error(error.to_string()));
    vector
        .try_set_real_elt(0, value)
        .unwrap_or_else(|error| base_error(error.to_string()));
    vector.freeze().as_raw()
}
/// Checked integer scalar; matches `Rf_ScalarInteger` on the session heap.
pub(crate) fn scalar_integer(value: i32) -> SEXP {
    let factory = node_factory();
    let vector = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap_or_else(|error| base_error(error.to_string()));
    let mut vector = crate::sexp::object::SexpMut::try_from_checked(vector)
        .unwrap_or_else(|error| base_error(error.to_string()));
    vector
        .try_set_integer_elt(0, value)
        .unwrap_or_else(|error| base_error(error.to_string()));
    vector.freeze().as_raw()
}
fn element_at(x: SEXP, i: R_xlen_t) -> SEXP {
    node_factory()
        .wrap(x)
        .and_then(|x| x.try_vector_elt(i))
        .map(|element| element.as_raw())
        .unwrap_or_else(|error| base_error(error.to_string()))
}
fn mutable_vector(x: SEXP) -> crate::sexp::object::SexpMut<'static> {
    node_factory()
        .wrap(x)
        .and_then(crate::sexp::object::SexpMut::try_from_checked)
        .unwrap_or_else(|error| base_error(error.to_string()))
}
fn wrapped(x: SEXP) -> crate::sexp::object::Sexp<'static> {
    node_factory()
        .wrap(x)
        .unwrap_or_else(|error| base_error(error.to_string()))
}
pub(crate) fn unit_result(template: SEXP, values: &[f64]) -> SEXP {
    let value = result(values);
    // SAFETY: audited boundary — shallow structural copy of a live template.
    let obj = unsafe { crate::mainutils::seq::Rf_shallow_duplicate(template) };
    // Preserve attributes, unit names and string/grob data while replacing
    // only the owned numeric coefficient vector.
    let names = attribute(obj, names_symbol());
    let mut object = mutable_vector(obj);
    let value = wrapped(value);
    for i in 0..XLENGTH(obj).min(XLENGTH(names)) {
        if elt_to_string(names, i) == "value" {
            object
                .try_set_vector_elt(i, value.clone())
                .unwrap_or_else(|error| base_error(error.to_string()));
            return obj;
        }
    }
    base_error("invalid grid unit: missing values")
}

// Build the small unit object used by the portable grid R wrappers.  Arithmetic
// units use `units` = the operation name and `data` = a list of scalar units;
// this mirrors grid's unit_v2 expression nodes without making the rest of the
// portable API depend on grid's private representation.
pub(crate) fn scalar_unit(value: f64, name: &str, data: SEXP) -> SEXP {
    let factory = node_factory();
    let out = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 3)))
        .unwrap_or_else(|error| base_error(error.to_string()));
    let mut out = crate::sexp::object::SexpMut::try_from_checked(out)
        .unwrap_or_else(|error| base_error(error.to_string()));
    let names = factory
        .strings(&["value", "units", "data"])
        .unwrap_or_else(|error| base_error(error.to_string()));
    set_attribute(out.as_raw(), names_symbol(), names.as_raw());
    out.try_set_vector_elt(0, wrapped(scalar_real(value)))
        .unwrap_or_else(|error| base_error(error.to_string()));
    let unit_name = factory
        .strings(&[name])
        .unwrap_or_else(|error| base_error(error.to_string()));
    out.try_set_vector_elt(1, unit_name)
        .unwrap_or_else(|error| base_error(error.to_string()));
    if name == ".rport-expression" {
        let elements = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 1)))
            .unwrap_or_else(|error| base_error(error.to_string()));
        let mut elements = crate::sexp::object::SexpMut::try_from_checked(elements)
            .unwrap_or_else(|error| base_error(error.to_string()));
        elements
            .try_set_vector_elt(0, wrapped(data))
            .unwrap_or_else(|error| base_error(error.to_string()));
        out.try_set_vector_elt(2, elements.freeze())
            .unwrap_or_else(|error| base_error(error.to_string()));
    } else {
        out.try_set_vector_elt(2, wrapped(data))
            .unwrap_or_else(|error| base_error(error.to_string()));
    }
    let class = factory
        .strings(&["unit"])
        .unwrap_or_else(|error| base_error(error.to_string()));
    set_attribute(out.as_raw(), class_symbol(), class.as_raw());
    out.freeze().as_raw()
}

pub(crate) fn unit_element(x: SEXP, i: usize) -> SEXP {
    let values = field(x, "value");
    let names = field(x, "units");
    let data = field(x, "data");
    let value = if XLENGTH(values) == 0 {
        0.
    } else {
        let vector = wrapped(values);
        match SEXPTYPE(TYPEOF(values)) {
            SEXPTYPE::REALSXP => vector.try_real_elt((i as i64) % XLENGTH(values)),
            SEXPTYPE::INTSXP => vector
                .try_integer_elt((i as i64) % XLENGTH(values))
                .map(f64::from),
            _ => base_error("invalid grid unit values"),
        }
        .unwrap_or_else(|error| base_error(error.to_string()))
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
        let element = wrapped(data)
            .try_string_elt((i as i64) % XLENGTH(data))
            .unwrap_or_else(|error| base_error(error.to_string()));
        let factory = node_factory();
        let d = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, 1)))
            .unwrap_or_else(|error| base_error(error.to_string()));
        let mut d = crate::sexp::object::SexpMut::try_from_checked(d)
            .unwrap_or_else(|error| base_error(error.to_string()));
        d.try_set_string_elt(0, element)
            .unwrap_or_else(|error| base_error(error.to_string()));
        d.freeze().as_raw()
    } else {
        element_at(data, (i as i64) % XLENGTH(data))
    };
    scalar_unit(value, &name, datum)
}
pub(crate) fn unit_name_at(x: SEXP, i: usize) -> String {
    let names = field(x, "units");
    if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) == 0 {
        base_error("invalid grid unit names");
    }
    elt_to_string(names, (i as i64) % XLENGTH(names))
}
pub(crate) fn unit_data_at(x: SEXP, i: usize) -> SEXP {
    if matches!(unit_name_at(x, i).as_str(), "sum" | "min" | "max") {
        return x;
    }
    let data = field(x, "data");
    if data == R_NilValue() || XLENGTH(data) == 0 {
        return R_NilValue();
    }
    if TYPEOF(data) == SEXPTYPE::VECSXP {
        element_at(data, (i as i64) % XLENGTH(data))
    } else {
        data
    }
}

pub(crate) fn expression_unit(op: &str, terms: &[SEXP]) -> SEXP {
    let factory = node_factory();
    let data = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, terms.len() as i64)))
        .unwrap_or_else(|error| base_error(error.to_string()));
    let mut data = crate::sexp::object::SexpMut::try_from_checked(data)
        .unwrap_or_else(|error| base_error(error.to_string()));
    for (i, term) in terms.iter().enumerate() {
        data.try_set_vector_elt(i as i64, wrapped(*term))
            .unwrap_or_else(|error| base_error(error.to_string()));
    }
    scalar_unit(1., op, data.freeze().as_raw())
}
pub(crate) enum UnitTerm {
    Scalar(f64, String, SEXP),
    Expr(SEXP),
}
pub(crate) fn unit_terms_result(template: SEXP, terms: &[UnitTerm]) -> SEXP {
    let factory = node_factory();
    let values = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, terms.len() as i64)))
        .unwrap_or_else(|error| base_error(error.to_string()));
    let mut values = crate::sexp::object::SexpMut::try_from_checked(values)
        .unwrap_or_else(|error| base_error(error.to_string()));
    let names = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, terms.len() as i64)))
        .unwrap_or_else(|error| base_error(error.to_string()));
    let mut names = crate::sexp::object::SexpMut::try_from_checked(names)
        .unwrap_or_else(|error| base_error(error.to_string()));
    let data = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, terms.len() as i64)))
        .unwrap_or_else(|error| base_error(error.to_string()));
    let mut data = crate::sexp::object::SexpMut::try_from_checked(data)
        .unwrap_or_else(|error| base_error(error.to_string()));
    let mut any_data = false;
    for (i, term) in terms.iter().enumerate() {
        let (v, n, d) = match term {
            UnitTerm::Scalar(v, n, d) if matches!(n.as_str(), "sum" | "min" | "max") => {
                (*v, ".rport-expression", *d)
            }
            UnitTerm::Scalar(v, n, d) => (*v, n.as_str(), *d),
            UnitTerm::Expr(e) => (1., ".rport-expression", *e),
        };
        values
            .try_set_real_elt(i as i64, v)
            .unwrap_or_else(|error| base_error(error.to_string()));
        let name_chars = factory
            .character(n)
            .unwrap_or_else(|error| base_error(error.to_string()));
        names
            .try_set_string_elt(i as i64, name_chars)
            .unwrap_or_else(|error| base_error(error.to_string()));
        data.try_set_vector_elt(i as i64, wrapped(d))
            .unwrap_or_else(|error| base_error(error.to_string()));
        any_data |= d != R_NilValue();
    }
    let names = names.freeze();
    let data = data.freeze();
    let values = values.freeze();
    // SAFETY: audited boundary — shallow structural copy of a live template.
    let obj = unsafe { crate::mainutils::seq::Rf_shallow_duplicate(template) };
    let nms = attribute(obj, names_symbol());
    let mut object = mutable_vector(obj);
    for i in 0..XLENGTH(obj).min(XLENGTH(nms)) {
        let replacement = match elt_to_string(nms, i).as_str() {
            "value" => Some(values.clone()),
            "units" => Some(names.clone()),
            "data" => Some(if any_data {
                data.clone()
            } else {
                factory.nil()
            }),
            _ => None,
        };
        if let Some(replacement) = replacement {
            object
                .try_set_vector_elt(i, replacement)
                .unwrap_or_else(|error| base_error(error.to_string()));
        }
    }
    obj
}
pub fn unit_binary(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    let au = has_class(a, "unit");
    let bu = has_class(b, "unit");
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
                    let an =
                        elt_to_string(field(a, "units"), (i as i64) % XLENGTH(field(a, "units")));
                    let bn =
                        elt_to_string(field(b, "units"), (i as i64) % XLENGTH(field(b, "units")));
                    if an != bn
                        || field(a, "data") != R_NilValue()
                        || field(b, "data") != R_NilValue()
                    {
                        // SAFETY: audited boundary — raw temporaries must stay
                        // protected across later allocations.
                        unsafe {
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
pub fn unit_summary(op: &str, args: SEXP) -> Option<SEXP> {
    // SAFETY: audited boundary — interns a fixed symbol under the active runtime.
    let na_tag = unsafe { crate::sexp::symbol::Rf_install(c"na.rm".as_ptr()) };
    let mut p = args;
    let mut template = R_NilValue();
    while p != R_NilValue() {
        if TAG(p) != na_tag && has_class(CAR(p), "unit") {
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
            if !has_class(x, "unit") {
                base_error("unit summary requires matching units");
            }
            mixed |= unit_kind(x) != first_kind || field(x, "data") != R_NilValue();
            for i in 0..XLENGTH(field(x, "value")) {
                mixed |= unit_name_at(x, i as usize) != first_kind;
                let term = unit_element(x, i as usize);
                // SAFETY: audited boundary — raw temporaries must stay
                // protected across later allocations.
                expression_roots.push(unsafe { crate::sexp::protect::protect(term) });
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
