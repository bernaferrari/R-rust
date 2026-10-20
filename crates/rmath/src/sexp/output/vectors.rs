//! output — extracted verbatim from output.rs.

use super::*;

// ---------------------------------------------------------------------------
// Faithful ports of stock printvector.c: printVector / printNamedVector.
//
// Field widths come from format.rs (formatRealS & co.) and each element is
// encoded through the printutils Encode* primitives at the COMMON width, so
// all elements of a vector share one number of decimals and one field width,
// exactly like stock. Lines wrap at options("width") with "[i]" index labels.
// ---------------------------------------------------------------------------

pub(crate) fn factor_levels(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if !string_vector_contains(class, "factor") {
            return None;
        }

        let levels = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_LevelsSymbol(),
        );
        string_vector_labels(levels).filter(|levels| !levels.is_empty())
    }
}

pub(crate) unsafe fn string_vector_contains(x: SEXP, needle: &str) -> bool {
    unsafe { string_vector_values(x) }
        .map(|values| values.iter().any(|value| value == needle))
        .unwrap_or(false)
}

pub(crate) unsafe fn string_vector_values(x: SEXP) -> Option<Vec<String>> {
    let sexp = unsafe { Sexp::from_raw(x) }?;
    if sexp.clone().typeof_() != SEXPTYPE::STRSXP {
        return None;
    }
    let mut values = Vec::with_capacity(sexp.clone().len() as usize);
    for i in 0..sexp.clone().len() {
        values.push(string_element_text(sexp.clone(), i).flatten()?.to_string());
    }
    Some(values)
}

pub(crate) unsafe fn string_vector_labels(x: SEXP) -> Option<Vec<String>> {
    let sexp = unsafe { Sexp::from_raw(x) }?;
    if sexp.clone().typeof_() != SEXPTYPE::STRSXP {
        return None;
    }
    let mut values = Vec::with_capacity(sexp.clone().len() as usize);
    for i in 0..sexp.clone().len() {
        values.push(match string_element_text(sexp.clone(), i) {
            Some(Some(value)) => value.to_string(),
            Some(None) | None => "<NA>".to_string(),
        });
    }
    Some(values)
}

pub(crate) fn vector_print_names(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let names = string_vector_labels(names)?;
        if names.len() != x.len() as usize || names.iter().all(|name| name.is_empty()) {
            None
        } else {
            Some(names)
        }
    }
}

pub(crate) fn has_names_attribute(x: Sexp<'_>) -> bool {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        Sexp::from_raw(names).is_some_and(|names| names.typeof_() == SEXPTYPE::STRSXP)
    }
}

pub(crate) fn format_named_atomic_vector(x: Sexp<'_>, values: Vec<String>) -> Option<String> {
    let mut names = vector_print_names(x)?;
    let limit = values.len();
    names.truncate(limit);
    Some(format_named_values(&names, &values))
}

pub(crate) fn string_element_text(x: Sexp<'_>, i: R_xlen_t) -> Option<Option<String>> {
    x.string_value_elt(i)
}

pub(crate) fn format_string_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    format_string_element_maybe_quoted(x, i, true)
}

#[derive(Clone, Default)]
pub(crate) struct PrintedString {
    pub(crate) text: String,
    pub(crate) width: usize,
}
pub(crate) fn printed_string_element(x: Sexp<'_>, i: R_xlen_t, quote: bool) -> PrintedString {
    let text = match x.try_string_value_elt(i) {
        Ok(Some(value)) if quote => format!("\"{}\"", escape_printed_string(&value)),
        Ok(Some(value)) => escape_printed_text(&value, false),
        Err(crate::sexp::object::SexpError::InvalidUtf8) => {
            if let Ok(value) = x.try_string_elt(i) {
                // SAFETY: the checked element and parent retain the sealed
                // character through these copy-only encoders. No R callback
                // occurs while borrowing their per-runtime text buffer.
                unsafe {
                    let q = if quote { b'"' as i32 } else { 0 };
                    let p = crate::mainutils::printutils::EncodeString(
                        value.as_raw(),
                        0,
                        q,
                        crate::mainutils::printutils::Rprt_adj::none,
                    );
                    if let Ok(text) = std::ffi::CStr::from_ptr(p).to_str() {
                        return PrintedString {
                            text: text.to_owned(),
                            width: crate::mainutils::format::string_element_width(value.as_raw(), q)
                                .max(0) as usize,
                        };
                    }
                }
            }
            if quote { "NA" } else { "<NA>" }.to_owned()
        }
        Ok(None) | Err(_) => if quote { "NA" } else { "<NA>" }.to_owned(),
    };
    PrintedString {
        width: text
            .chars()
            .map(crate::mainutils::unicode_width::columns)
            .sum(),
        text,
    }
}
pub(crate) fn format_string_element_maybe_quoted(x: Sexp<'_>, i: R_xlen_t, quote: bool) -> String {
    printed_string_element(x, i, quote).text
}

pub(crate) fn escape_printed_string(value: &str) -> String {
    escape_printed_text(value, true)
}

pub(crate) fn escape_printed_text(value: &str, quote: bool) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' if quote => output.push_str("\\\""),
            '\u{7}' => output.push_str("\\a"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{b}' => output.push_str("\\v"),
            ch if ch.is_ascii_control() => write!(output, "\\{:03o}", ch as u32).unwrap(),
            ch => output.push(ch),
        }
    }
    output
}

pub(crate) fn format_string_vector(x: Sexp<'_>) -> String {
    if x.clone().len() == 0 {
        return "character(0)".to_string();
    }
    let vals: Vec<String> = (0..x.clone().len().min(10))
        .map(|i| format_string_element(x.clone(), i))
        .collect();
    let suffix = if x.len() > 10 { " ..." } else { "" };
    format!("[1] {}{}", vals.join(" "), suffix)
}

pub(crate) fn format_factor(x: Sexp<'_>) -> Option<String> {
    let levels = factor_levels(x.clone())?;
    let mut vals: Vec<String> = x
        .clone()
        .iter_integer()
        .take(10)
        .map(|code| {
            if code == NA_INTEGER {
                "<NA>".to_string()
            } else {
                levels
                    .get((code - 1) as usize)
                    .cloned()
                    .unwrap_or_else(|| code.to_string())
            }
        })
        .collect();
    let width = vals.iter().map(|s| s.chars().count()).max().unwrap_or(0);
    for val in &mut vals {
        *val = format!("{val:<width$}");
    }
    let suffix = if x.len() > 10 { " ..." } else { "" };
    Some(format!(
        "[1] {}{}\nLevels: {}",
        vals.join(" "),
        suffix,
        levels.join(" ")
    ))
}

pub(crate) fn table_names(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if !string_vector_contains(class, "table") {
            return None;
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if let Some(labels) =
            string_vector_values(names).filter(|names| names.len() == x.len() as usize)
        {
            return Some(labels);
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        if dimnames.is_null() || TYPEOF(dimnames) != SEXPTYPE::VECSXP || XLENGTH(dimnames) < 1 {
            return None;
        }
        string_vector_values(VECTOR_ELT(dimnames, 0))
            .filter(|names| names.len() == x.len() as usize)
    }
}

pub(crate) fn table_title(x: Sexp<'_>) -> Option<String> {
    unsafe {
        let title = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::symbol::Rf_install(c"table.name".as_ptr()),
        );
        let title = Sexp::from_raw(title)?;
        if title.clone().typeof_() != SEXPTYPE::STRSXP || title.clone().len() == 0 {
            return None;
        }
        string_element_text(title, 0).flatten()
    }
}

pub(crate) fn format_table(x: Sexp<'_>) -> Option<String> {
    let names = table_names(x.clone())?;
    let values: Vec<String> = match x.clone().typeof_() {
        SEXPTYPE::INTSXP => (0..x.clone().len())
            .map(|i| format_integer_element(x.clone(), i))
            .collect(),
        SEXPTYPE::REALSXP => (0..x.clone().len())
            .map(|i| format_real_element(x.clone(), i))
            .collect(),
        _ => return None,
    };
    if let Some(title) = table_title(x) {
        let widths: Vec<usize> = names
            .iter()
            .zip(&values)
            .map(|(name, value)| name.len().max(value.len()).max(4))
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
        Some(format!("{title}\n{name_line}\n{value_line}"))
    } else {
        let width = names
            .iter()
            .zip(&values)
            .map(|(name, value)| name.len().max(value.len()))
            .max()
            .unwrap_or(1);
        let name_line = names
            .iter()
            .map(|name| format!("{name:>width$}"))
            .collect::<Vec<_>>()
            .join(" ");
        let value_line = values
            .iter()
            .map(|value| format!("{value:>width$}"))
            .collect::<Vec<_>>()
            .join(" ");
        // GNU print.array of a 1-d table keeps a trailing column space.
        Some(format!("\n{name_line} \n{value_line} "))
    }
}

pub(crate) const OUT_DEC: *const std::os::raw::c_char =
    b".\0".as_ptr() as *const std::os::raw::c_char;

pub(crate) unsafe fn encode_cstr(p: *const std::os::raw::c_char) -> String {
    // SAFETY: this unsafe function requires `p` to be null or a live,
    // NUL-terminated C string for the duration of the conversion.
    unsafe {
        if p.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// (R_print.width, R_print.gap, R_print.max). Width comes from the
/// active `print.default` data so `print(x, width = 120)` wraps like GNU.
pub(crate) fn vector_print_settings() -> (std::os::raw::c_int, std::os::raw::c_int, i64) {
    unsafe {
        let width = crate::mainutils::print::get_R_print_data().width;
        let max = print_max_cells();
        let gap = crate::mainutils::print::get_R_print_data().gap;
        (width, gap, max as i64)
    }
}

/// Stock VectorIndex: right-justify "[i]" in `labwidth` columns.
pub(crate) fn vector_index(i: R_xlen_t, labwidth: usize) -> String {
    let label = format!("[{i}]");
    // GNU uses a signed %*s padding width. At max=0 its two-column
    // labwidth is narrower than [1], so the negative width emits one space.
    format!("{}{}", " ".repeat(labwidth.abs_diff(label.len())), label)
}

/// The stock type-specific field width `w` (before the gap is added by the
/// callers that add it), computed over the first `n` elements.
pub(crate) unsafe fn type_field_width(
    raw: SEXP,
    tp: SEXPTYPE,
    n: R_xlen_t,
    quote: bool,
) -> (
    std::os::raw::c_int,
    std::os::raw::c_int,
    std::os::raw::c_int,
) {
    // SAFETY: callers provide a live atomic-vector SEXP whose runtime tag is
    // `tp`; the formatting routines only read it and write to stack locals.
    unsafe {
        // (w, d, e); only real/complex use d/e.
        let _ = quote;
        match tp {
            SEXPTYPE::LGLSXP => {
                let mut w = 0;
                crate::mainutils::format::formatLogicalS(raw, n, &mut w);
                (w, 0, 0)
            }
            SEXPTYPE::INTSXP => {
                let mut w = 0;
                crate::mainutils::format::formatIntegerS(raw, n, &mut w);
                (w, 0, 0)
            }
            SEXPTYPE::REALSXP => {
                let mut w = 0;
                let mut d = 0;
                let mut e = 0;
                crate::mainutils::format::formatRealS(raw, n, &mut w, &mut d, &mut e, 0);
                (w, d, e)
            }
            SEXPTYPE::CPLXSXP => {
                let mut wr = 0;
                let mut dr = 0;
                let mut er = 0;
                let mut wi = 0;
                let mut di = 0;
                let mut ei = 0;
                crate::mainutils::format::formatComplexS(
                    raw, n, &mut wr, &mut dr, &mut er, &mut wi, &mut di, &mut ei, 0,
                );
                (wr + wi + 2, dr, er)
            }
            SEXPTYPE::STRSXP => {
                let mut w = 0;
                // Stock formatString measures against the actual quote
                // character so embedded quotes render as \" (two columns).
                let quote_ch = if quote {
                    b'"' as std::os::raw::c_int
                } else {
                    0
                };
                crate::mainutils::format::formatStringS(raw, n, &mut w, quote_ch);
                (w, 0, 0)
            }
            SEXPTYPE::RAWSXP => {
                let mut w = 0;
                crate::mainutils::format::formatRawS(raw, n, &mut w);
                (w, 0, 0)
            }
            _ => (0, 0, 0),
        }
    }
}

/// formatComplexS results, computed once per vector.
#[derive(Clone, Copy)]
pub(crate) struct ComplexFmt {
    wr: std::os::raw::c_int,
    dr: std::os::raw::c_int,
    er: std::os::raw::c_int,
    wi: std::os::raw::c_int,
    di: std::os::raw::c_int,
    ei: std::os::raw::c_int,
}

pub(crate) unsafe fn complex_fmt(raw: SEXP, n: R_xlen_t) -> ComplexFmt {
    // SAFETY: callers guarantee that `raw` is a live CPLXSXP with at least
    // `n` elements; all output pointers refer to initialized stack locals.
    unsafe {
        let (mut wr, mut dr, mut er, mut wi, mut di, mut ei) = (0, 0, 0, 0, 0, 0);
        crate::mainutils::format::formatComplexS(
            raw, n, &mut wr, &mut dr, &mut er, &mut wi, &mut di, &mut ei, 0,
        );
        ComplexFmt {
            wr,
            dr,
            er,
            wi,
            di,
            ei,
        }
    }
}

pub(crate) fn part_is_na(v: f64) -> bool {
    crate::sexp::ffi::is_na_real(v)
}

pub(crate) fn part_is_nan(v: f64) -> bool {
    v.is_nan()
}

/// Encode element `i` at the common width, per stock print*Vector tight loops.
pub(crate) unsafe fn encode_element_at(
    raw: SEXP,
    tp: SEXPTYPE,
    i: R_xlen_t,
    w: std::os::raw::c_int,
    d: std::os::raw::c_int,
    e: std::os::raw::c_int,
    quote: bool,
    gap: std::os::raw::c_int,
    cfmt: ComplexFmt,
) -> String {
    // SAFETY: `encode_element_adj` inherits this function's live-SEXP,
    // matching-tag, and in-bounds-index requirements.
    unsafe {
        encode_element_adj(
            raw,
            tp,
            i,
            w,
            d,
            e,
            quote,
            gap,
            cfmt,
            crate::mainutils::printutils::Rprt_adj::right,
        )
    }
}

/// Like encode_element_at, with an explicit justification for STRSXP
/// elements: stock printVector left-adjusts (R_print.right = FALSE by
/// default), printNamedVector right-adjusts (Rprt_adj_right).
pub(crate) unsafe fn encode_element_adj(
    raw: SEXP,
    tp: SEXPTYPE,
    i: R_xlen_t,
    w: std::os::raw::c_int,
    d: std::os::raw::c_int,
    e: std::os::raw::c_int,
    quote: bool,
    gap: std::os::raw::c_int,
    cfmt: ComplexFmt,
    str_adj: crate::mainutils::printutils::Rprt_adj,
) -> String {
    // SAFETY: callers guarantee `raw` is live, `tp` is its actual runtime
    // tag, and `i` is in bounds; formatter-returned C strings are borrowed
    // only until the next formatter call and are copied immediately.
    unsafe {
        let i32i = i as std::os::raw::c_int;
        match tp {
            SEXPTYPE::LGLSXP => encode_cstr(crate::mainutils::printutils::EncodeLogical(
                crate::sexp::accessors::LOGICAL_ELT(raw, i32i),
                w,
            )),
            SEXPTYPE::INTSXP => encode_cstr(crate::mainutils::printutils::EncodeInteger(
                crate::sexp::accessors::INTEGER_ELT(raw, i32i),
                w,
            )),
            SEXPTYPE::REALSXP => encode_cstr(crate::mainutils::printutils::EncodeReal0(
                crate::sexp::accessors::REAL_ELT(raw, i32i),
                w,
                d,
                e,
                OUT_DEC,
            )),
            SEXPTYPE::CPLXSXP => {
                let c = crate::sexp::accessors::COMPLEX_ELT(raw, i32i);
                if part_is_na(c.r) || part_is_na(c.i) {
                    // stock: NA parts render as NA over the total width
                    encode_cstr(crate::mainutils::printutils::EncodeReal0(
                        crate::sexp::ffi::NA_REAL,
                        w,
                        0,
                        0,
                        OUT_DEC,
                    ))
                } else {
                    encode_cstr(crate::mainutils::printutils::EncodeComplex(
                        c,
                        cfmt.wr + gap,
                        cfmt.dr,
                        cfmt.er,
                        cfmt.wi,
                        cfmt.di,
                        cfmt.ei,
                        OUT_DEC,
                    ))
                }
            }
            SEXPTYPE::STRSXP => {
                let quote_ch = if quote {
                    b'"' as std::os::raw::c_int
                } else {
                    0
                };
                encode_cstr(crate::mainutils::printutils::EncodeString(
                    crate::sexp::accessors::STRING_ELT(raw, i),
                    w,
                    quote_ch,
                    str_adj,
                ))
            }
            SEXPTYPE::RAWSXP => encode_cstr(crate::mainutils::printutils::EncodeRaw(
                crate::sexp::accessors::RAW_ELT(raw, i32i),
                std::ptr::null(),
            )),
            _ => String::new(),
        }
    }
}

/// Stock printVector: unnamed vector with "[i]" index labels, wrapping at
/// options("width"). Every element occupies the common field width.
pub(crate) unsafe fn print_vector_stock(x: Sexp, quote: bool, n_pr: R_xlen_t) -> String {
    // SAFETY: `x` is a rooted live SEXP and `n_pr` is bounded by its length;
    // all raw access remains read-only for the duration of this call.
    unsafe {
        if x.typeof_() == SEXPTYPE::REALSXP {
            crate::mainutils::printvector::warn_illegal_outdec();
        }
        let raw = x.clone().as_raw();
        let tp = x.typeof_();
        let (print_width, gap, _max) = vector_print_settings();
        let (mut w, d, e) = type_field_width(raw, tp, n_pr, quote);
        let mut out = String::new();
        let labwidth = crate::mainutils::printutils::IndexWidth_xlen(n_pr) + 2;
        let mut width = labwidth;
        out.push_str(&vector_index(1, labwidth as usize));
        if !matches!(tp, SEXPTYPE::STRSXP | SEXPTYPE::RAWSXP) {
            w += gap;
        }
        let is_str = matches!(tp, SEXPTYPE::STRSXP);
        let gap_prefixed = is_str || tp == SEXPTYPE::RAWSXP;
        let cfmt = if tp == SEXPTYPE::CPLXSXP {
            complex_fmt(raw, n_pr)
        } else {
            ComplexFmt {
                wr: 0,
                dr: 0,
                er: 0,
                wi: 0,
                di: 0,
                ei: 0,
            }
        };
        for i in 0..n_pr {
            if i > 0 {
                // stock wrap conditions: char adds the gap to the check and to
                // the accumulated width; raw prefixes the gap but counts only w.
                let wrap = if is_str {
                    width + w + gap > print_width
                } else {
                    width + w > print_width
                };
                if wrap {
                    out.push('\n');
                    out.push_str(&vector_index(i + 1, labwidth as usize));
                    width = labwidth;
                }
            }
            if gap_prefixed {
                out.push_str(&" ".repeat(gap as usize));
            }
            out.push_str(&encode_element_adj(
                raw,
                tp,
                i,
                w,
                d,
                e,
                quote,
                gap,
                cfmt,
                crate::mainutils::printutils::Rprt_adj::left,
            ));
            width += if is_str { w + gap } else { w };
        }
        out.push('\n');
        out
    }
}

/// Stock printNamedVector: names line(s) right-justified over the value
/// column(s), every column at the common width `w`, gap-separated.
pub(crate) unsafe fn print_named_vector_stock(
    x: Sexp,
    names: Sexp,
    quote: bool,
    n_pr: R_xlen_t,
) -> String {
    // SAFETY: `x` and `names` are rooted live SEXPs with compatible lengths;
    // element access is read-only and bounded by `n_pr`.
    unsafe {
        let raw = x.clone().as_raw();
        let names_raw = names.as_raw();
        let tp = x.typeof_();
        let (print_width, gap, _max) = vector_print_settings();
        let (mut w, d, e) = type_field_width(raw, tp, n_pr, quote);
        let mut wn = 0;
        let cfmt0 = if tp == SEXPTYPE::CPLXSXP {
            complex_fmt(raw, n_pr)
        } else {
            ComplexFmt {
                wr: 0,
                dr: 0,
                er: 0,
                wi: 0,
                di: 0,
                ei: 0,
            }
        };
        crate::mainutils::format::formatStringS(names_raw, n_pr, &mut wn, 0);
        // Names use the unquoted missing label from PrintInit (normally
        // "<NA>"). The numeric-format settings can have a narrower NA width;
        // measure against the same label that EncodeString will emit.
        if (0..n_pr).any(|index| {
            crate::sexp::accessors::STRING_ELT(names_raw, index)
                == crate::sexp::globals::R_NaString()
        }) {
            wn = wn.max(crate::mainutils::print::get_R_print_data().na_width_noquote);
        }
        if w < wn {
            w = wn;
        }
        let nperline = ((print_width / (w + gap)).max(1)) as R_xlen_t;
        let nlines = n_pr / nperline + R_xlen_t::from(n_pr % nperline != 0);
        let mut out = String::new();
        for i in 0..nlines {
            if i != 0 {
                out.push('\n');
            }
            for j in 0..nperline {
                let k = i * nperline + j;
                if k >= n_pr {
                    break;
                }
                out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeString(
                    crate::sexp::accessors::STRING_ELT(names_raw, k),
                    w,
                    0,
                    crate::mainutils::printutils::Rprt_adj::right,
                )));
                out.push_str(&" ".repeat(gap as usize));
            }
            out.push('\n');
            for j in 0..nperline {
                let k = i * nperline + j;
                if k >= n_pr {
                    break;
                }
                let i32k = k as std::os::raw::c_int;
                if matches!(tp, SEXPTYPE::CPLXSXP) {
                    if j != 0 {
                        out.push_str(&" ".repeat(gap as usize));
                    }
                    let c = crate::sexp::accessors::COMPLEX_ELT(raw, i32k);
                    if part_is_na(c.r) || part_is_na(c.i) {
                        out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                            crate::sexp::ffi::NA_REAL,
                            w,
                            0,
                            0,
                            OUT_DEC,
                        )));
                    } else {
                        out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                            c.r, cfmt0.wr, cfmt0.dr, cfmt0.er, OUT_DEC,
                        )));
                        if part_is_nan(c.i) {
                            out.push_str("+NaNi");
                        } else if c.i >= 0.0 {
                            out.push('+');
                            out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                                c.i, cfmt0.wi, cfmt0.di, cfmt0.ei, OUT_DEC,
                            )));
                            out.push('i');
                        } else {
                            out.push('-');
                            out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                                -c.i, cfmt0.wi, cfmt0.di, cfmt0.ei, OUT_DEC,
                            )));
                            out.push('i');
                        }
                    }
                } else if matches!(tp, SEXPTYPE::RAWSXP) {
                    // stock: "%*s%s%*s" with w-2, raw, gap
                    out.push_str(&" ".repeat((w - 2).max(0) as usize));
                    out.push_str(&encode_element_at(raw, tp, k, w, d, e, quote, gap, cfmt0));
                    out.push_str(&" ".repeat(gap as usize));
                } else {
                    let str_quote = if matches!(tp, SEXPTYPE::STRSXP) {
                        quote
                    } else {
                        false
                    };
                    out.push_str(&encode_element_at(
                        raw, tp, k, w, d, e, str_quote, gap, cfmt0,
                    ));
                    out.push_str(&" ".repeat(gap as usize));
                }
            }
        }
        out.push('\n');
        out
    }
}

/// Render an atomic vector exactly like stock print.default: named vectors go
/// through printNamedVector, others through printVector with index labels;
/// both honour options("max.print") truncation.
pub(crate) unsafe fn format_vector_stock(x: Sexp, quote: bool) -> String {
    unsafe { format_vector_stock_n(x, quote, None) }
}

pub(crate) unsafe fn format_vector_stock_n(
    x: Sexp,
    quote: bool,
    max_override: Option<i64>,
) -> String {
    if let Some(message) = result_admission_error(x.clone()) {
        return message;
    }
    unsafe {
        let n = x.clone().len();
        if n == 0 {
            return match x.typeof_() {
                SEXPTYPE::LGLSXP => "logical(0)".to_string(),
                SEXPTYPE::INTSXP => "integer(0)".to_string(),
                SEXPTYPE::REALSXP => "numeric(0)".to_string(),
                SEXPTYPE::CPLXSXP => "complex(0)".to_string(),
                SEXPTYPE::STRSXP => "character(0)".to_string(),
                SEXPTYPE::RAWSXP => "raw(0)".to_string(),
                _ => String::new(),
            };
        }
        let (_width, _gap, opt_max) = vector_print_settings();
        let max = max_override.unwrap_or(opt_max);
        let n_pr = if n <= (max + 1) as R_xlen_t {
            n
        } else {
            max as R_xlen_t
        };

        let mut out = match names_sexp(x.clone()) {
            Some(names) => {
                let title = one_dim_array_title(x.clone());
                let body = print_named_vector_stock(x, names, quote, n_pr);
                match title {
                    Some(title) => format!("{title}\n{body}"),
                    None => body,
                }
            }
            None => print_vector_stock(x, quote, n_pr),
        };
        if n_pr < n {
            out.push_str(&format!(
                " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]\n",
                n - n_pr
            ));
        }
        // Callers own the final newline (print_value appends it; string contexts
        // embed the rendering without one).
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }
}

/// GNU PrintValueRec passes names(dimnames)[1] as the named-vector title.
/// An empty title still emits its line; a missing title label prints "NA".
pub(crate) fn one_dim_array_title(x: Sexp<'_>) -> Option<String> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        if TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) != 1 {
            return None;
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        if dimnames.is_null() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            return None;
        }
        let names = crate::sexp::attrib_core::getAttrib(
            dimnames,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let names = Sexp::from_raw(names)?;
        if names.typeof_() != SEXPTYPE::STRSXP || names.len() == 0 {
            return None;
        }
        Some(string_element_text(names, 0)?.unwrap_or_else(|| "NA".to_string()))
    }
}

/// Names used by stock printNamedVector.
///
/// GNU PrintValueRec for a 1-d array uses **only** `dimnames[[1]]`.
/// A 1-d array with a `names` attribute but empty dimnames prints
/// without a header. Fall back to the names attribute for non-arrays.
pub(crate) fn names_sexp(x: Sexp<'_>) -> Option<Sexp<'_>> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 1 {
            let dimnames = crate::sexp::attrib_core::getAttrib(
                x.clone().as_raw(),
                crate::sexp::attrib_core::R_DimNamesSymbol(),
            );
            if !dimnames.is_null()
                && dimnames != crate::sexp::globals::R_NilValue()
                && TYPEOF(dimnames) == SEXPTYPE::VECSXP
                && XLENGTH(dimnames) >= 1
            {
                let row = VECTOR_ELT(dimnames, 0);
                if let Some(row) = Sexp::from_raw(row)
                    && row.clone().typeof_() == SEXPTYPE::STRSXP
                    && row.clone().len() == x.len()
                {
                    return Some(row);
                }
            }
            return None;
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let names = Sexp::from_raw(names)?;
        if names.clone().typeof_() == SEXPTYPE::STRSXP && names.clone().len() == x.len() {
            Some(names)
        } else {
            None
        }
    }
}
