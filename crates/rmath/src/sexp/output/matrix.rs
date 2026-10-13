//! output — extracted verbatim from output.rs.

use super::*;

pub(crate) fn matrix_dims(x: Sexp<'_>) -> Option<(usize, usize)> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        let dim = Sexp::from_raw(dim)?;
        if dim.clone().typeof_() != SEXPTYPE::INTSXP || dim.clone().len() != 2 {
            return None;
        }
        let nrow = dim.clone().integer_elt(0)? as usize;
        let ncol = dim.integer_elt(1)? as usize;
        if nrow.checked_mul(ncol)? > x.len() as usize {
            return None;
        }
        Some((nrow, ncol))
    }
}

pub(crate) struct MatrixDimnames {
    rows: Option<Vec<String>>,
    cols: Option<Vec<String>>,
    /// `names(dimnames)[1]`; `Some` even when empty (GNU `rn != NULL`).
    row_title: Option<String>,
    /// `names(dimnames)[2]`; printed above the first column.
    col_title: Option<String>,
}

pub(crate) fn empty_matrix_dimnames() -> MatrixDimnames {
    MatrixDimnames {
        rows: None,
        cols: None,
        row_title: None,
        col_title: None,
    }
}

/// GNU `GetMatrixDimnames`: `names(dimnames)` is a STRSXP; empty
/// strings stay present so printarray still emits the title row.
pub(crate) fn matrix_dimname_titles(dimnames: SEXP) -> (Option<String>, Option<String>) {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            dimnames,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if names.is_null()
            || names == R_NilValue()
            || TYPEOF(names) != SEXPTYPE::STRSXP
            || XLENGTH(names) < 2
        {
            return (None, None);
        }
        let elt = |i: R_xlen_t| -> String {
            let s = STRING_ELT(names, i);
            if s.is_null() || s == R_NilValue() {
                return String::new();
            }
            let p = CHAR(s);
            if p.is_null() {
                return String::new();
            }
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        };
        (Some(elt(0)), Some(elt(1)))
    }
}

pub(crate) fn matrix_dimnames(x: Sexp<'_>, nrow: usize, ncol: usize) -> MatrixDimnames {
    unsafe {
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        let Some(dimnames) = Sexp::from_raw(dimnames) else {
            return empty_matrix_dimnames();
        };
        if dimnames.clone().typeof_() != SEXPTYPE::VECSXP || dimnames.clone().len() < 2 {
            return empty_matrix_dimnames();
        }
        let (row_title, col_title) = matrix_dimname_titles(dimnames.clone().as_raw());
        // Dimname entries print as labels: an NA name renders "<NA>" (GNU
        // EncodeString in MatrixColumnLabel/MatrixRowLabel) instead of
        // discarding the whole dimname vector.
        let row_names = string_vector_labels(crate::sexp::accessors::VECTOR_ELT(
            dimnames.clone().as_raw(),
            0,
        ))
        .filter(|names| names.len() >= nrow)
        .map(|names| names.into_iter().take(nrow).collect());
        let col_names =
            string_vector_labels(crate::sexp::accessors::VECTOR_ELT(dimnames.as_raw(), 1))
                .filter(|names| names.len() >= ncol)
                .map(|names| names.into_iter().take(ncol).collect());
        MatrixDimnames {
            rows: row_names,
            cols: col_names,
            row_title,
            col_title,
        }
    }
}

/// GNU `init_rl_rn`: a present row title (even `""`) adds `R_MIN_LBLOFF`.
pub(crate) const R_MIN_LBLOFF: usize = 2;

pub(crate) fn matrix_row_geometry(
    row_labels: &[String],
    row_title: Option<&str>,
) -> (usize, usize) {
    // GNU reserves the minimum index-label width even for zero-row matrices.
    let mut row_width = row_labels.iter().map(String::len).max().unwrap_or(4);
    let mut lbloff = 0;
    if let Some(title) = row_title {
        let rnw = title.len();
        lbloff = if rnw < row_width + R_MIN_LBLOFF {
            R_MIN_LBLOFF
        } else {
            rnw.saturating_sub(row_width)
        };
        row_width += lbloff;
    }
    (row_width, lbloff)
}

/// GNU `IndexWidth(n)`: decimal digits of `n` (n >= 1).
pub(crate) fn gnu_index_width(n: usize) -> usize {
    let mut width = 1;
    let mut v = n;
    while v >= 10 {
        v /= 10;
        width += 1;
    }
    width
}

pub(crate) fn gnu_row_index_label(row: usize, nrow: usize) -> String {
    // GNU printarray.c: rlabw = IndexWidth(r + 1) + 3 -- sized for the NEXT
    // row index, so a 9-row matrix gets width 5 ("[10,]"), not 4.
    let rlabw = gnu_index_width(nrow + 1) + 3;
    format!("{:>rlabw$}", format!("[{},]", row + 1))
}

pub(crate) fn real_index(index: usize) -> Option<R_xlen_t> {
    R_xlen_t::try_from(index).ok()
}

pub(crate) fn format_real_matrix_gnu(x: Sexp<'_>, nrow: usize, ncol: usize) -> String {
    use crate::mainutils::format::{compact_real_field, formatReal, real_field};
    use crate::mainutils::printutils::EncodeReal;
    use crate::sexp::accessors::REAL;
    use crate::sexp::ffi::NA_REAL;

    let seq = x
        .compact_seq()
        .filter(|seq| seq.payload_is_null() && seq.is_real());
    if let Some(seq) = seq {
        let mut col_fmt = Vec::with_capacity(ncol);
        for c in 0..ncol {
            let start = c * nrow;
            let fmt = match (real_index(start), real_index(nrow)) {
                (Some(start), Some(rows)) => compact_real_field(seq, start, rows, 0),
                _ => real_field(1, 0, |_| NA_REAL),
            };
            col_fmt.push(fmt);
        }
        return format_matrix_with(x, nrow, ncol, |r, c| {
            let fmt = col_fmt[c];
            let val = real_index(r + c * nrow)
                .map(|index| seq.real_or_na(index))
                .unwrap_or(NA_REAL);
            // SAFETY: EncodeReal writes a session print buffer, not an R node.
            let encoded = unsafe { EncodeReal(val, fmt.w, fmt.d, fmt.e, b'.' as _) };
            if encoded.is_null() {
                String::new()
            } else {
                unsafe { std::ffi::CStr::from_ptr(encoded) }
                    .to_string_lossy()
                    .into_owned()
            }
        });
    }

    unsafe {
        let raw = x.clone().as_raw();
        let data = REAL(raw);
        let mut col_fmt = Vec::with_capacity(ncol);
        for c in 0..ncol {
            let mut w = 0;
            let mut d = 0;
            let mut e = 0;
            if data.is_null() {
                // Same empty width as formatReal on a null pointer. The cells
                // below still encode NA_REAL. The pointer is not offset.
                formatReal(std::ptr::null(), 0, &mut w, &mut d, &mut e, 0);
            } else {
                formatReal(
                    data.add(c * nrow),
                    nrow as R_xlen_t,
                    &mut w,
                    &mut d,
                    &mut e,
                    0,
                );
            }
            col_fmt.push((w, d, e));
        }
        format_matrix_with(x, nrow, ncol, |r, c| {
            let (w, d, e) = col_fmt[c];
            let val = if data.is_null() {
                NA_REAL
            } else {
                // SAFETY: `data` is the live real buffer. A null buffer took the other branch.
                unsafe { *data.add(r + c * nrow) }
            };
            // SAFETY: EncodeReal writes a session print buffer, not an R node.
            let encoded = unsafe { EncodeReal(val, w, d, e, b'.' as _) };
            if encoded.is_null() {
                String::new()
            } else {
                unsafe { std::ffi::CStr::from_ptr(encoded) }
                    .to_string_lossy()
                    .into_owned()
            }
        })
    }
}

pub(crate) fn print_max_cells() -> usize {
    unsafe {
        let extras = print_dispatch_extras();
        let mut cur = extras;
        while !cur.is_null() && cur != R_NilValue() {
            if printable_attribute_name(cur).as_deref() == Some("max") {
                let value = CAR(cur);
                if !value.is_null() && value != R_NilValue() {
                    if TYPEOF(value) == SEXPTYPE::INTSXP && XLENGTH(value) > 0 {
                        let n = *crate::sexp::accessors::INTEGER(value);
                        if n >= 0 {
                            return n as usize;
                        }
                    } else if TYPEOF(value) == SEXPTYPE::REALSXP && XLENGTH(value) > 0 {
                        let n = *crate::sexp::accessors::REAL(value);
                        if n.is_finite() && n >= 0.0 {
                            return n as usize;
                        }
                    }
                }
            }
            cur = CDR(cur);
        }
        crate::mainutils::options::GetOptionMaxPrint().max(0) as usize
    }
}

/// GNU `print.default(..., quote=)` default TRUE; NA becomes TRUE.
pub(crate) fn print_quote_flag() -> bool {
    unsafe {
        let extras = print_dispatch_extras();
        let mut cur = extras;
        while !cur.is_null() && cur != R_NilValue() {
            if printable_attribute_name(cur).as_deref() == Some("quote") {
                let value = CAR(cur);
                if !value.is_null() && value != R_NilValue() {
                    let q = crate::mainutils::coerce::asLogical(value);
                    if q != crate::sexp::ffi::NA_LOGICAL {
                        return q != 0;
                    }
                }
                return true;
            }
            cur = CDR(cur);
        }
        true
    }
}

pub(crate) fn print_use_source_flag() -> bool {
    unsafe {
        let mut cell = print_dispatch_extras();
        while !cell.is_null() && cell != R_NilValue() {
            if printable_attribute_name(cell).as_deref() == Some("useSource") {
                let value = crate::mainutils::coerce::asLogical(CAR(cell));
                return value != 0;
            }
            cell = CDR(cell);
        }
        true
    }
}

/// GNU `print.default(..., right=)` default FALSE; NA stays FALSE.
pub(crate) fn print_right_flag() -> bool {
    unsafe {
        let extras = print_dispatch_extras();
        let mut cur = extras;
        while !cur.is_null() && cur != R_NilValue() {
            if printable_attribute_name(cur).as_deref() == Some("right") {
                let value = CAR(cur);
                if value.is_null() || value == R_NilValue() {
                    return false;
                }
                let right = crate::mainutils::coerce::asLogical(value);
                return right != crate::sexp::ffi::NA_LOGICAL && right != 0;
            }
            cur = CDR(cur);
        }
        false
    }
}

pub(crate) fn matrix_print_window(nrow: usize, ncol: usize, max: usize) -> (usize, usize) {
    let c_pr = ncol.min(max);
    let mut r_pr = nrow;
    if ncol > 0 && max / ncol < nrow {
        r_pr = max / ncol;
    }
    if ncol > c_pr && r_pr < 1 && nrow > 0 {
        r_pr = 1;
    }
    (r_pr, c_pr)
}

pub(crate) fn matrix_omitted_message(
    nrow: usize,
    ncol: usize,
    r_pr: usize,
    c_pr: usize,
) -> Option<String> {
    if r_pr >= nrow && c_pr >= ncol {
        return None;
    }
    let mut msg = String::from(" [ reached 'max' / getOption(\"max.print\") -- omitted");
    if r_pr < nrow {
        let omitted = nrow - r_pr;
        msg.push_str(&format!(
            " {} row{}",
            omitted,
            if omitted == 1 { "" } else { "s" }
        ));
    }
    if c_pr < ncol {
        if r_pr < nrow {
            msg.push_str(" and");
        }
        let omitted = ncol - c_pr;
        msg.push_str(&format!(
            " {} column{}",
            omitted,
            if omitted == 1 { "" } else { "s" }
        ));
    }
    msg.push_str(" ]");
    Some(msg)
}

pub(crate) fn format_matrix_with<F>(x: Sexp<'_>, nrow: usize, ncol: usize, value_at: F) -> String
where
    F: Fn(usize, usize) -> String,
{
    let max = print_max_cells();
    let (r_pr, c_pr) = matrix_print_window(nrow, ncol, max);
    let dn = matrix_dimnames(x, nrow, ncol);
    let row_labels: Vec<String> = (0..r_pr)
        .map(|r| {
            dn.rows
                .as_ref()
                .and_then(|names| names.get(r))
                .cloned()
                .unwrap_or_else(|| gnu_row_index_label(r, nrow))
        })
        .collect();
    let col_labels: Vec<String> = (0..c_pr)
        .map(|c| {
            dn.cols
                .as_ref()
                .and_then(|names| names.get(c))
                .cloned()
                .unwrap_or_else(|| format!("[,{}]", c + 1))
        })
        .collect();
    let (row_width, lbloff) = matrix_row_geometry(&row_labels, dn.row_title.as_deref());
    let mut values = vec![vec![String::new(); c_pr]; r_pr];
    let mut widths = Vec::with_capacity(c_pr);
    for c in 0..c_pr {
        let mut width = col_labels[c].len().max(1);
        for r in 0..r_pr {
            let value = value_at(r, c);
            width = width.max(value.len());
            values[r][c] = value;
        }
        widths.push(width);
    }

    let (page_width, gap, _) = vector_print_settings();
    let page_width = page_width.max(10) as usize;
    let gap = gap.max(0) as usize;
    let separator = " ".repeat(gap);
    let mut blocks = Vec::new();
    let mut start = 0;
    while start < c_pr {
        let mut used = row_width;
        let mut end = start;
        while end < c_pr {
            let extra = widths[end] + gap;
            if end > start && used + extra > page_width {
                break;
            }
            used += extra;
            end += 1;
        }
        if end == start {
            end += 1;
        }
        blocks.push((start, end));
        start = end;
    }
    if blocks.is_empty() {
        blocks.push((0, 0));
    }

    let mut lines = Vec::new();
    if r_pr == 0 {
        let mut header = if c_pr == 0 { "    " } else { "     " }.to_string();
        for c in 0..c_pr {
            if c > 0 {
                header.push_str(&separator);
            }
            header.push_str(&format!("{:>width$}", col_labels[c], width = widths[c]));
        }
        lines.push(header);
    } else {
        for &(cs, ce) in &blocks {
            if let Some(cn) = dn.col_title.as_deref() {
                lines.push(format!("{:row_width$}{cn}", ""));
            }
            let mut header = if let Some(rn) = dn.row_title.as_deref() {
                format!("{rn:<row_width$}")
            } else {
                " ".repeat(row_width)
            };
            if row_width > 0 && cs < ce {
                header.push_str(&separator);
            }
            for c in cs..ce {
                header.push_str(&format!("{:>width$}", col_labels[c], width = widths[c]));
                if c + 1 < ce {
                    header.push_str(&separator);
                }
            }
            lines.push(header);
            for r in 0..r_pr {
                let label = format!("{:lbloff$}{}", "", row_labels[r]);
                let mut line = format!("{label:<row_width$}");
                for c in cs..ce {
                    line.push_str(&separator);
                    line.push_str(&format!("{:>width$}", values[r][c], width = widths[c]));
                }
                lines.push(line);
            }
        }
    }

    if let Some(omitted) = matrix_omitted_message(nrow, ncol, r_pr, c_pr) {
        lines.push(omitted);
    }
    lines.join("\n")
}

pub(crate) fn format_character_matrix_with<F>(
    x: Sexp<'_>,
    nrow: usize,
    ncol: usize,
    value_at: F,
) -> String
where
    F: Fn(usize, usize) -> String,
{
    let (r_pr, c_pr) = matrix_print_window(nrow, ncol, print_max_cells());
    // GNU's zero-column character printer still emits every row label;
    // its omission message uses the separately computed print window.
    let displayed_rows = if c_pr == 0 { nrow } else { r_pr };
    let dn = matrix_dimnames(x, nrow, ncol);
    let row_labels: Vec<String> = (0..displayed_rows)
        .map(|r| {
            dn.rows
                .as_ref()
                .and_then(|names| names.get(r))
                .cloned()
                .unwrap_or_else(|| gnu_row_index_label(r, nrow))
        })
        .collect();
    let col_labels: Vec<String> = (0..c_pr)
        .map(|c| {
            dn.cols
                .as_ref()
                .and_then(|names| names.get(c))
                .cloned()
                .unwrap_or_else(|| format!("[,{}]", c + 1))
        })
        .collect();
    let (row_width, lbloff) = matrix_row_geometry(&row_labels, dn.row_title.as_deref());
    let right = print_right_flag();
    let align = |text: &str, width: usize| -> String {
        if right {
            format!("{text:>width$}")
        } else {
            format!("{text:<width$}")
        }
    };
    let empty_row_labs = row_labels.iter().all(|s| s.is_empty());
    let mut values = vec![vec![String::new(); c_pr]; r_pr];
    let mut widths = Vec::with_capacity(c_pr);
    for c in 0..c_pr {
        let mut width = col_labels[c].len().max(1);
        // Field width includes omitted rows, as GNU printStringMatrix does.
        for r in 0..nrow {
            let value = value_at(r, c);
            width = width.max(value.len());
            if r < r_pr {
                values[r][c] = value;
            }
        }
        widths.push(width);
    }

    let (page_width, gap, _) = vector_print_settings();
    let page_width = page_width.max(10) as usize;
    let gap = gap.max(0) as usize;
    let separator = " ".repeat(gap);
    let mut blocks = Vec::new();
    let mut start = 0;
    while start < c_pr {
        let mut used = row_width;
        let mut end = start;
        while end < c_pr {
            let extra = widths[end]
                + if row_width > 0 || end > start || empty_row_labs {
                    gap
                } else {
                    0
                };
            if end > start && used + extra > page_width {
                break;
            }
            used += extra;
            end += 1;
        }
        if end == start {
            end += 1;
        }
        blocks.push((start, end));
        start = end;
    }

    if blocks.is_empty() {
        blocks.push((0, 0));
    }

    let mut lines = Vec::new();
    if r_pr == 0 {
        let mut header = "    ".to_string();
        for c in 0..c_pr {
            header.push_str(&separator);
            header.push_str(&align(&col_labels[c], widths[c]));
        }
        lines.push(header);
    }
    for &(cs, ce) in blocks.iter().filter(|_| r_pr > 0) {
        if let Some(cn) = dn.col_title.as_deref() {
            lines.push(format!("{:row_width$}{cn}", ""));
        }

        let mut header = if let Some(rn) = dn.row_title.as_deref() {
            format!("{rn:<row_width$}")
        } else {
            " ".repeat(row_width)
        };
        if row_width > 0 && cs < ce {
            header.push_str(&separator);
        }
        for c in cs..ce {
            if c > cs || (row_width == 0 && empty_row_labs) {
                // leading space is added per cell on data rows; header
                // matches once the first empty row-label space is emitted.
            }
            if c > cs || row_width > 0 {
                if c > cs {
                    header.push_str(&separator);
                }
            } else if empty_row_labs {
                header.push_str(&separator);
            }
            // Gap sits outside the field; `right` chooses the side of the pad.
            header.push_str(&align(&col_labels[c], widths[c]));
        }
        lines.push(header);

        for r in 0..displayed_rows {
            let label = format!("{:lbloff$}{}", "", row_labels[r]);
            let mut line = format!("{label:<row_width$}");
            for c in cs..ce {
                line.push_str(&separator);
                line.push_str(&align(&values[r][c], widths[c]));
            }
            lines.push(line);
        }
    }
    if let Some(omitted) = matrix_omitted_message(nrow, ncol, r_pr, c_pr) {
        lines.push(omitted);
    }
    lines.join("\n")
}

pub(crate) fn format_complex_matrix_gnu(x: Sexp<'_>, nrow: usize, ncol: usize) -> String {
    unsafe {
        use crate::mainutils::format::formatComplex;
        use crate::mainutils::printutils::EncodeComplex;
        use crate::sexp::accessors::COMPLEX;
        let data = COMPLEX(x.clone().as_raw());
        let mut col_fmt = Vec::with_capacity(ncol);
        for c in 0..ncol {
            let mut wr = 0;
            let mut dr = 0;
            let mut er = 0;
            let mut wi = 0;
            let mut di = 0;
            let mut ei = 0;
            formatComplex(
                data.add(c * nrow),
                nrow as R_xlen_t,
                &mut wr,
                &mut dr,
                &mut er,
                &mut wi,
                &mut di,
                &mut ei,
                0,
            );
            col_fmt.push((wr, dr, er, wi, di, ei));
        }
        format_matrix_with(x, nrow, ncol, |r, c| {
            let (wr, dr, er, wi, di, ei) = col_fmt[c];
            let encoded = EncodeComplex(
                *data.add(r + c * nrow),
                wr,
                dr,
                er,
                wi,
                di,
                ei,
                b".\0".as_ptr() as *const std::os::raw::c_char,
            );
            if encoded.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(encoded)
                    .to_string_lossy()
                    .into_owned()
            }
        })
    }
}

pub(crate) fn format_matrix(x: Sexp<'_>) -> Option<String> {
    format_matrix_body(x.clone()).map(|body| format_with_printable_attributes(body, x))
}

pub(crate) fn format_matrix_body(x: Sexp<'_>) -> Option<String> {
    let Some((nrow, ncol)) = matrix_dims(x.clone()) else {
        return format_array(x);
    };
    let body = match x.clone().typeof_() {
        SEXPTYPE::INTSXP => format_matrix_with(x.clone(), nrow, ncol, |r, c| {
            format_integer_element(x.clone(), (r + c * nrow) as i64)
        }),
        SEXPTYPE::REALSXP => format_real_matrix_gnu(x.clone(), nrow, ncol),
        SEXPTYPE::LGLSXP => format_matrix_with(x.clone(), nrow, ncol, |r, c| {
            format_logical_element(x.clone(), (r + c * nrow) as i64)
        }),
        SEXPTYPE::CPLXSXP => format_complex_matrix_gnu(x.clone(), nrow, ncol),
        SEXPTYPE::RAWSXP => format_matrix_with(x.clone(), nrow, ncol, |r, c| {
            format_raw_element(x.clone(), (r + c * nrow) as i64)
        }),
        SEXPTYPE::STRSXP => {
            let quote = print_quote_flag()
                && !has_class(x.clone(), "noquote")
                && !has_class(x.clone(), "table");
            format_character_matrix_with(x.clone(), nrow, ncol, |r, c| {
                format_string_element_maybe_quoted(x.clone(), (r + c * nrow) as i64, quote)
            })
        }

        _ => return None,
    };
    Some(body)
}
