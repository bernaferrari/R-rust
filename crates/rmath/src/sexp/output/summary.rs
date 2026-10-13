//! output — extracted verbatim from output.rs.

use super::*;

pub(crate) fn summary_default_digits() -> i32 {
    unsafe { 3.max(crate::mainutils::options::GetOptionDigits() - 3) }
}

pub(crate) fn with_summary_default_digits<T>(f: impl FnOnce() -> T) -> T {
    unsafe {
        let digits = summary_default_digits();
        let old = crate::mainutils::format::format_get_R_print();
        let previous =
            crate::mainutils::format::format_set_R_print(crate::mainutils::format::RPrint {
                digits,
                scipen: old.scipen,
                na_width: old.na_width,
                na_width_noquote: old.na_width_noquote,
            });
        let rendered = f();
        crate::mainutils::format::format_set_R_print(previous);
        rendered
    }
}

pub(crate) fn format_named_summary_reals(x: Sexp<'_>, names: &[String]) -> Option<Vec<String>> {
    unsafe {
        if x.typeof_() != SEXPTYPE::REALSXP {
            return None;
        }
        let slice: Vec<_> = x.iter_real().collect();
        if slice.len() != names.len() {
            return None;
        }
        let mut finite = Vec::new();
        let mut nas_at = None;
        for (i, name) in names.iter().enumerate() {
            if name == "NAs" {
                nas_at = Some(i);
            } else {
                finite.push(i);
            }
        }
        let n = finite.len() as R_xlen_t;
        let tmp = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if tmp.is_null() {
            return None;
        }
        let _tmp = crate::sexp::protect::protect(tmp);
        for (j, &i) in finite.iter().enumerate() {
            *crate::sexp::accessors::REAL(tmp).add(j) = slice[i];
        }
        Some(with_summary_default_digits(|| {
            let mut w = 0;
            let mut d = 0;
            let mut e = 0;
            crate::mainutils::format::formatRealS(tmp, n, &mut w, &mut d, &mut e, 0);
            let mut values = vec![String::new(); names.len()];
            for (j, &i) in finite.iter().enumerate() {
                values[i] = encode_cstr(crate::mainutils::printutils::EncodeReal0(
                    crate::sexp::accessors::REAL_ELT(tmp, j as std::os::raw::c_int),
                    w,
                    d,
                    e,
                    OUT_DEC,
                ));
            }
            if let Some(i) = nas_at {
                values[i] = format!("{}", slice[i] as i64);
            }
            values
        }))
    }
}

pub(crate) fn format_summary_default_unnamed_numeric(x: Sexp<'_>) -> String {
    with_summary_default_digits(|| unsafe { format_vector_stock(x, false) })
}

pub(crate) fn format_summary_via_format(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let sym = crate::sexp::symbol::Rf_install(c"format".as_ptr());
        let fun = crate::sexp::envir::findFun(sym, crate::sexp::globals::R_GlobalEnv());
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            return None;
        }
        let call = crate::sexp::constructors::Rf_lang2(fun, x.as_raw());
        let _call = crate::sexp::protect::protect(call);
        let res = crate::eval::eval::Rf_eval(call, crate::sexp::globals::R_GlobalEnv());
        if res.is_null() || crate::sexp::accessors::TYPEOF(res) != SEXPTYPE::STRSXP {
            return None;
        }
        let n = crate::sexp::accessors::XLENGTH(res);
        let mut values = Vec::with_capacity(n as usize);
        for i in 0..n {
            let charsxp = crate::sexp::accessors::STRING_ELT(res, i);
            let bytes = crate::sexp::accessors::charsxp_as_utf8(charsxp);
            values.push(String::from_utf8_lossy(&bytes).trim().to_string());
        }
        Some(values)
    }
}

pub(crate) fn format_summary_default(x: Sexp<'_>) -> Option<String> {
    if !has_class(x.clone(), "summaryDefault") || !has_class(x.clone(), "table") {
        return None;
    }
    let Some(names) = table_names(x.clone()) else {
        if x.clone().typeof_() != SEXPTYPE::REALSXP {
            return None;
        }
        return Some(format_summary_default_unnamed_numeric(x));
    };
    let values: Vec<String> = match x.clone().typeof_() {
        SEXPTYPE::REALSXP => format_summary_via_format(x.clone())
            .or_else(|| format_named_summary_reals(x.clone(), &names))?,

        SEXPTYPE::INTSXP => (0..x.clone().len())
            .map(|i| {
                if matches!(
                    names.get(i as usize).map(String::as_str),
                    Some("Min.nchar" | "Max.nchar")
                ) && format_integer_element(x.clone(), i) == "NA"
                {
                    String::new()
                } else {
                    format_integer_element(x.clone(), i)
                }
            })
            .collect(),
        SEXPTYPE::STRSXP => (0..x.clone().len())
            .map(|i| match string_element_text(x.clone(), i) {
                Some(Some(value)) => value.to_string(),
                Some(None) | None => "NA".to_string(),
            })
            .collect(),
        _ => return None,
    };
    let min_width = match x.typeof_() {
        SEXPTYPE::REALSXP | SEXPTYPE::STRSXP => 7,
        SEXPTYPE::INTSXP => 8,
        _ => 0,
    };
    let widths: Vec<usize> = names
        .iter()
        .zip(&values)
        .map(|(name, value)| name.len().max(value.len()).max(min_width))
        .collect();
    let name_line = names
        .iter()
        .zip(&widths)
        .map(|(name, width)| format!("{name:>width$}"))
        .collect::<Vec<_>>()
        .join(" ");
    let value_line = values
        .iter()
        .zip(&widths)
        .map(|(value, width)| format!("{value:>width$}"))
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!("{name_line} \n{value_line} "))
}
