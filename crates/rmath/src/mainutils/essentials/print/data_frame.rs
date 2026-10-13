use super::*;

fn print_data_frame_show_row_names(args: SEXP) -> bool {
    unsafe {
        let row_names_sym = Rf_install(c"row.names".as_ptr());
        let mut arg = CDR(args);
        while !arg.is_null() && arg != R_NilValue() {
            if TAG(arg) == row_names_sym {
                let value = CAR(arg);
                if TYPEOF(value) == SEXPTYPE::LGLSXP {
                    let data = LOGICAL(value);
                    if !data.is_null() {
                        return *data != FALSE;
                    }
                }
            }
            arg = CDR(arg);
        }
        true
    }
}
fn print_data_frame_max(args: SEXP) -> i64 {
    unsafe {
        let max_sym = Rf_install(c"max".as_ptr());
        let mut arg = CDR(args);
        while !arg.is_null() && arg != R_NilValue() {
            if TAG(arg) == max_sym {
                let value = CAR(arg);
                if TYPEOF(value) == SEXPTYPE::INTSXP {
                    let data = INTEGER(value);
                    if !data.is_null() && *data != NA_INTEGER {
                        return (*data as i64).max(1);
                    }
                }
                if TYPEOF(value) == SEXPTYPE::REALSXP {
                    let data = REAL(value);
                    if !data.is_null() && data.read().is_finite() {
                        return data.read() as i64;
                    }
                }
            }
            arg = CDR(arg);
        }
        crate::mainutils::options::GetOptionMaxPrint().max(1) as i64
    }
}
/// GNU `print.data.frame` leaves character and logical NA unencoded
/// (`na.encode = FALSE`) and `print.default` renders those as `<NA>`.
/// Numeric NA stays `NA`.
fn data_frame_cell_text(col: SEXP, i: R_xlen_t) -> String {
    unsafe {
        if col.is_null() {
            return "NULL".to_string();
        }
        let t = TYPEOF(col);
        if t == SEXPTYPE::STRSXP {
            let elt = STRING_ELT(col, i);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                return "<NA>".to_string();
            }
        } else if t == SEXPTYPE::LGLSXP {
            let data = LOGICAL(col);
            if !data.is_null() && *data.add(i as usize) == NA_INTEGER {
                return "<NA>".to_string();
            }
        }
        elt_to_string(col, i)
    }
}

/// Explicit `digits=` only. NULL/missing keeps the historical cell text
/// so default frames stay on `elt_to_string`.
fn data_frame_print_digits(args: SEXP) -> Option<i32> {
    unsafe {
        let sym = Rf_install(c"digits".as_ptr());
        let mut arg = args;
        while !arg.is_null() && arg != R_NilValue() {
            if TAG(arg) == sym {
                let mut value = CAR(arg);
                if !value.is_null() && TYPEOF(value) == SEXPTYPE::PROMSXP {
                    value = crate::sexp::envir::forcePromise(value);
                }
                if value.is_null() || value == R_NilValue() {
                    return None;
                }
                let digits = if TYPEOF(value) == SEXPTYPE::INTSXP && XLENGTH(value) > 0 {
                    *INTEGER(value)
                } else if TYPEOF(value) == SEXPTYPE::REALSXP && XLENGTH(value) > 0 {
                    let number = *REAL(value);
                    if number.is_finite() {
                        number as i32
                    } else {
                        return None;
                    }
                } else {
                    return None;
                };
                if digits == NA_INTEGER || digits < 1 {
                    return None;
                }
                return Some(digits);
            }
            arg = CDR(arg);
        }
        None
    }
}

fn plain_numeric_column(col: SEXP) -> bool {
    unsafe {
        if col.is_null() || col == R_NilValue() {
            return false;
        }
        let class =
            crate::sexp::attrib_core::getAttrib(col, crate::sexp::attrib_core::R_ClassSymbol());
        if !class.is_null() && class != R_NilValue() && XLENGTH(class) > 0 {
            return false;
        }
        let t = TYPEOF(col);
        t == SEXPTYPE::REALSXP || t == SEXPTYPE::CPLXSXP
    }
}

/// GNU `format.data.frame` formats each numeric column with `digits`.
fn format_plain_numeric_column(col: SEXP, nrow: R_xlen_t, digits: i32) -> Vec<String> {
    unsafe {
        let trim = Rf_ScalarLogical(TRUE);
        let dig = Rf_ScalarInteger(digits);
        let nsmall = Rf_ScalarInteger(0);
        let width = Rf_ScalarInteger(0);
        let adj = Rf_ScalarInteger(3);
        let na_encode = Rf_ScalarLogical(TRUE);
        let scientific = Rf_ScalarLogical(NA_LOGICAL);
        let decimal = Rf_mkString(c".".as_ptr());
        let mut guards = Vec::new();
        for value in [
            trim, dig, nsmall, width, adj, na_encode, scientific, decimal,
        ] {
            guards.push(protect(value));
        }
        // .Internal(format) is positional: trim, digits, nsmall, width, adj, na.encode, scientific, decimal.mark.
        let mut rest = R_NilValue();
        for value in [
            decimal, scientific, na_encode, adj, width, nsmall, dig, trim,
        ] {
            rest = Rf_cons(value, rest);
            guards.push(protect(rest));
        }
        let args = Rf_cons(col, rest);
        guards.push(protect(args));
        let formatted = do_format(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            args,
            crate::sexp::globals::R_GlobalEnv(),
        );
        if formatted.is_null() || formatted == R_NilValue() || TYPEOF(formatted) != SEXPTYPE::STRSXP
        {
            return vec![String::new(); nrow.max(0) as usize];
        }
        guards.push(protect(formatted));
        let mut values = Vec::with_capacity(nrow as usize);
        for i in 0..nrow {
            let text = if i < XLENGTH(formatted) {
                elt_to_string(formatted, i).trim().to_string()
            } else {
                String::new()
            };
            values.push(text);
        }
        values
    }
}

fn print_data_frame_column_texts(
    x: SEXP,
    ncol: R_xlen_t,
    nrow: R_xlen_t,
    digits: Option<i32>,
) -> (Vec<String>, Vec<Vec<String>>) {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;
        let mut headers = Vec::with_capacity(ncol as usize);
        let mut columns = Vec::with_capacity(ncol as usize);
        for j in 0..ncol {
            let col = VECTOR_ELT(x, j as R_xlen_t);
            if crate::mainutils::essentials::sexp_has_class(col, "data.frame") && XLENGTH(col) > 0 {
                let (mut inner_headers, inner_columns) =
                    print_data_frame_column_texts(col, XLENGTH(col), nrow, digits);
                if inner_headers.len() == 1
                    && inner_headers[0].starts_with("[,")
                    && has_names
                    && j < XLENGTH(names)
                {
                    inner_headers[0] = elt_to_string(names, j);
                }
                headers.extend(inner_headers);
                columns.extend(inner_columns);
                continue;
            }
            if !print_data_frame_column(col) {
                continue;
            }
            let header = if has_names && j < XLENGTH(names) {
                elt_to_string(names, j)
            } else {
                format!("[,{}]", j + 1)
            };
            headers.push(header);
            let values = if digits.is_some_and(|digits| digits > 0) && plain_numeric_column(col) {
                format_plain_numeric_column(col, nrow, digits.unwrap_or(7))
            } else {
                let mut values = Vec::with_capacity(nrow as usize);
                for i in 0..nrow {
                    values.push(data_frame_cell_text(col, i));
                }
                values
            };
            columns.push(values);
        }
        (headers, columns)
    }
}
fn print_data_frame_column(col: SEXP) -> bool {
    unsafe {
        if col.is_null() || col == R_NilValue() {
            return true;
        }
        if crate::mainutils::essentials::sexp_has_class(col, "data.frame") && XLENGTH(col) == 0 {
            return false;
        }
        true
    }
}

fn emit_print_data_frame_line(line: &str) {
    if crate::sexp::output::is_capturing() {
        crate::sexp::output::capture_stdout(&format!("{line}\n"));
    } else {
        println!("{line}");
    }
}

fn emit_print_data_frame_columns(
    headers: &[String],
    columns: &[Vec<String>],
    widths: &[usize],
    row_labels: &[String],
    show_row_names: bool,
    row_width: usize,
    print_rows: usize,
    start: usize,
    end: usize,
) {
    if start >= end {
        return;
    }
    let header = headers[start..end]
        .iter()
        .enumerate()
        .map(|(idx, name)| format!("{:>width$}", name, width = widths[start + idx]))
        .collect::<Vec<_>>()
        .join(" ");
    // With row labels shown, stock pads the header by the label
    // column width plus one separator; with row.names = FALSE the
    // label column is empty strings, leaving exactly one separator.
    let label_pad = if show_row_names {
        " ".repeat(row_width)
    } else {
        String::new()
    };
    emit_print_data_frame_line(&format!("{label_pad} {header}"));

    for row in 0..print_rows {
        let mut cells = Vec::with_capacity((end - start) + usize::from(show_row_names));
        // Stock left-justifies row labels (auto 1..n, explicit numeric,
        // and character row names alike) inside the label column; a hidden
        // label column still contributes its separator space.
        if show_row_names {
            cells.push(format!(
                "{:<row_width$}",
                row_labels.get(row).map(String::as_str).unwrap_or("")
            ));
        } else {
            cells.push(String::new());
        }
        for idx in start..end {
            let value = columns[idx].get(row).map(String::as_str).unwrap_or("");
            cells.push(format!("{:>width$}", value, width = widths[idx]));
        }
        emit_print_data_frame_line(&cells.join(" "));
    }
}
fn print_empty_data_frame(x: SEXP, ncol: R_xlen_t) {
    unsafe {
        if ncol == 0 {
            let row_names = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::attrib_core::R_RowNamesSymbol(),
            );
            let n = if !row_names.is_null()
                && row_names != R_NilValue()
                && (TYPEOF(row_names) == SEXPTYPE::STRSXP || TYPEOF(row_names) == SEXPTYPE::INTSXP)
            {
                XLENGTH(row_names)
            } else {
                0
            };
            let line = if n == 1 {
                "data frame with 0 columns and 1 row".to_string()
            } else {
                format!("data frame with 0 columns and {n} rows")
            };
            emit_print_data_frame_line(&line);
            return;
        }
        let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
        if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) > 0 {
            let mut line = String::from("[1]");
            for i in 0..XLENGTH(names) {
                line.push(' ');
                line.push_str(&elt_to_string(names, i));
            }
            emit_print_data_frame_line(&line);
        }
        emit_print_data_frame_line("<0 rows> (or 0-length row.names)");
    }
}

/// Derive the row labels of a data.frame for printing.
///
/// Mirrors stock `print.data.frame`: automatic compact row names (`c(NA, n)`
/// stored as a length-2 integer vector) expand to `1..n`; explicit integer or
/// character `row.names` are used verbatim.
fn data_frame_row_labels(x: SEXP, nrow: R_xlen_t, limit: R_xlen_t) -> Vec<String> {
    let n = nrow.min(limit).max(0);
    let row_names =
        unsafe { crate::sexp::attrib_core::getAttrib(x, Rf_install(c"row.names".as_ptr())) };
    unsafe {
        if !row_names.is_null() {
            let t = TYPEOF(row_names);
            if t == SEXPTYPE::STRSXP && XLENGTH(row_names) == nrow {
                return (0..n).map(|i| elt_to_string(row_names, i)).collect();
            }
            if t == SEXPTYPE::INTSXP && nrow > 0 && XLENGTH(row_names) == nrow {
                let data = INTEGER(row_names);
                if !data.is_null() {
                    let first = *data;
                    let is_compact =
                        XLENGTH(row_names) == 2 && first == crate::sexp::ffi::NA_INTEGER;
                    if !is_compact {
                        return (0..n)
                            .map(|i| data.add(i as usize).read().to_string())
                            .collect();
                    }
                }
            }
        }
    }
    (1..=n).map(|i| i.to_string()).collect()
}

pub(crate) unsafe fn print_method_x(call: SEXP, args: SEXP, class: &str) -> SEXP {
    unsafe {
        if !call.is_null() && call != R_NilValue() {
            let matched =
                crate::mainutils::match_mod::match_formal_slots(call, args, &["x", "..."]);
            if let Some(&x) = matched.first()
                && !x.is_null()
                && x != R_NilValue()
                && crate::mainutils::essentials::sexp_has_class(x, class)
            {
                return x;
            }
        }
        let mut p = args;
        while !p.is_null() && p != R_NilValue() {
            let v = CAR(p);
            if !v.is_null() && crate::mainutils::essentials::sexp_has_class(v, class) {
                return v;
            }
            p = CDR(p);
        }
        CAR(args)
    }
}

/// R's `print.data.frame(x)` — print a data.frame nicely with aligned columns.
pub unsafe fn do_print_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // GNU print(x, ...) UseMethod matches `x` before the method runs.
        // `print(width = 101, df)` therefore passes the data.frame as `x`,
        // not CAR(args) which is still the named `width` actual.
        let x = print_method_x(_call, args, "data.frame");
        if x.is_null() || x == R_NilValue() {
            emit_print_data_frame_line("NULL");
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return do_print(_call, _op, args, _rho);
        }
        let ncol = XLENGTH(x);
        let nrow = {
            let rn = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::attrib_core::R_RowNamesSymbol(),
            );
            if !rn.is_null()
                && rn != R_NilValue()
                && TYPEOF(rn) == SEXPTYPE::INTSXP
                && XLENGTH(rn) == 2
                && *INTEGER(rn) == NA_INTEGER
            {
                (*INTEGER(rn).add(1) as i64).unsigned_abs() as i64
            } else if !rn.is_null() && rn != R_NilValue() && XLENGTH(rn) > 0 {
                XLENGTH(rn)
            } else if ncol > 0 {
                let first = VECTOR_ELT(x, 0);
                if first.is_null() { 0 } else { XLENGTH(first) }
            } else {
                0
            }
        };
        if ncol == 0 || nrow == 0 {
            print_empty_data_frame(x, ncol);
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        let show_row_names = print_data_frame_show_row_names(args);
        // GNU formats x[seq_len(n0),] so column/label widths ignore omitted rows.
        let max_print = print_data_frame_max(args) as R_xlen_t;
        let n0 = if ncol > 0 {
            (max_print / ncol) as usize
        } else {
            nrow as usize
        };
        let print_rows = (nrow as usize).min(n0);
        let digits = data_frame_print_digits(args);
        let (headers, columns) =
            print_data_frame_column_texts(x, ncol, print_rows as R_xlen_t, digits);
        let row_labels = data_frame_row_labels(x, nrow, print_rows as R_xlen_t);
        let shown_labels = &row_labels[..];
        // GNU formatString uses 0 for a blank row name; a minimum of 1 inserts a pad column.
        let row_width = shown_labels
            .iter()
            .map(|label| label.len())
            .max()
            .unwrap_or(0);
        let widths: Vec<usize> = headers
            .iter()
            .zip(&columns)
            .map(|(header, values)| {
                values
                    .iter()
                    .fold(header.len(), |max, value| max.max(value.len()))
            })
            .collect();

        if headers.is_empty() {
            for row in 0..print_rows {
                let line = if show_row_names {
                    format!(
                        "{:<row_width$}",
                        row_labels.get(row).map(String::as_str).unwrap_or("")
                    )
                } else {
                    String::new()
                };
                emit_print_data_frame_line(&line);
            }
        } else {
            // GNU keeps the next column only while the line stays strictly under width.
            let page_width = crate::mainutils::options::GetOptionWidth().max(10) as usize;
            let rlabw = if show_row_names { row_width } else { 0 };
            let gap = 1usize;
            let mut start = 0;
            while start < headers.len() {
                let mut used = rlabw;
                let mut end = start;
                loop {
                    used += widths[end] + gap;
                    end += 1;
                    if end >= headers.len() || used + widths[end] + gap >= page_width {
                        break;
                    }
                }
                emit_print_data_frame_columns(
                    &headers,
                    &columns,
                    &widths,
                    &row_labels,
                    show_row_names,
                    row_width,
                    print_rows,
                    start,
                    end,
                );
                start = end;
            }
        }
        if (nrow as usize) > n0 {
            emit_print_data_frame_line(&format!(
                " [ reached 'max' / getOption(\"max.print\") -- omitted {} rows ]",
                (nrow as usize) - n0
            ));
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}
/// GNU `summary.data.frame` — character table of per-column summaries.
pub unsafe fn do_summary_data_frame(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return do_summary_default(call, op, args, rho);
        }
        let ncol = XLENGTH(x);
        let maxsum = summary_tagged_int(args, "maxsum", 7);
        let opt_digits = crate::mainutils::options::GetOptionDigits();
        let digits = summary_tagged_int(args, "digits", (opt_digits - 3).max(3));
        let col_names_attr =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());

        let mut columns: Vec<Vec<String>> = Vec::with_capacity(ncol as usize);
        let mut label_widths: Vec<usize> = Vec::with_capacity(ncol as usize);
        let mut headers: Vec<String> = Vec::with_capacity(ncol as usize);
        for j in 0..ncol {
            let header = if !col_names_attr.is_null()
                && TYPEOF(col_names_attr) == SEXPTYPE::STRSXP
                && j < XLENGTH(col_names_attr)
            {
                elt_to_string(col_names_attr, j)
            } else {
                format!("[,{}]", j + 1)
            };
            headers.push(header);
            let col = VECTOR_ELT(x, j);
            let sms = summarize_frame_column(col, maxsum, rho);
            let _sms = protect(sms);
            let (cells, lw) = format_summary_column(sms, digits, rho);

            label_widths.push(lw);
            columns.push(cells);
        }
        let nrow = columns.iter().map(Vec::len).max().unwrap_or(0);
        for col in &mut columns {
            col.resize(nrow, String::new());
        }
        let blanks = " ".repeat(label_widths.iter().copied().max().unwrap_or(0) + 2);
        for (i, header) in headers.iter_mut().enumerate() {
            let pad = ((label_widths[i] as f64) - (header.chars().count() as f64) / 2.0)
                .floor()
                .max(0.0) as usize;
            let pad = pad.min(blanks.len());
            *header = format!("{}{header}", &blanks[..pad]);
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, (nrow * ncol as usize) as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _r = protect(result);
        for (j, col) in columns.iter().enumerate() {
            for (i, cell) in col.iter().enumerate() {
                let cstr = CString::new(cell.as_str()).unwrap_or_default();
                SET_STRING_ELT(
                    result,
                    (j * nrow + i) as R_xlen_t,
                    crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                );
            }
        }
        crate::mainutils::essentials::set_two_dim_attr(result, nrow as R_xlen_t, ncol);
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, nrow as R_xlen_t);
        let _rn = protect(rn);
        for i in 0..nrow {
            SET_STRING_ELT(
                rn,
                i as R_xlen_t,
                crate::sexp::constructors::Rf_mkChar(c"".as_ptr()),
            );
        }
        let cn = Rf_allocVector3(SEXPTYPE::STRSXP, ncol);
        for j in 0..ncol {
            let ch = if !col_names_attr.is_null()
                && TYPEOF(col_names_attr) == SEXPTYPE::STRSXP
                && j < XLENGTH(col_names_attr)
            {
                STRING_ELT(col_names_attr, j)
            } else {
                let label = CString::new(format!("[,{}]", j + 1)).unwrap_or_default();
                Rf_mkChar(label.as_ptr())
            };
            let ch = {
                let column = VECTOR_ELT(x, j);
                let dim = crate::sexp::attrib_core::getAttrib(
                    column,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                let mc = if !dim.is_null() && TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) >= 2 {
                    *INTEGER(dim).add(1)
                } else {
                    0
                };
                if mc > 1 {
                    let dn = crate::sexp::attrib_core::getAttrib(
                        column,
                        crate::sexp::attrib_core::R_DimNamesSymbol(),
                    );
                    let cnames =
                        if !dn.is_null() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 2 {
                            VECTOR_ELT(dn, 1)
                        } else {
                            R_NilValue()
                        };
                    let mut bytes = Vec::new();
                    let outer = CStr::from_ptr(CHAR(ch)).to_bytes();
                    for k in 0..mc as i64 {
                        bytes.extend_from_slice(outer);
                        bytes.push(b'.');
                        if TYPEOF(cnames) == SEXPTYPE::STRSXP && k < XLENGTH(cnames) {
                            bytes.extend_from_slice(
                                CStr::from_ptr(CHAR(STRING_ELT(cnames, k))).to_bytes(),
                            );
                        }
                    }
                    Rf_mkChar(CString::new(bytes).unwrap_or_default().as_ptr())
                } else {
                    ch
                }
            };
            SET_STRING_ELT(cn, j, ch);
        }
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        let class = Rf_mkString(c"table".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        result
    }
}

unsafe fn summarize_frame_column(col: SEXP, maxsum: i32, rho: SEXP) -> SEXP {
    unsafe {
        if col.is_null() || col == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        if let Some(levels) = aggregate_factor_levels(col) {
            return summary_factor_result(col, levels, maxsum);
        }
        let args = Rf_cons(col, R_NilValue());
        let _a = protect(args);
        do_summary_default(std::ptr::null_mut(), std::ptr::null_mut(), args, rho)
    }
}

unsafe fn format_summary_column(sms: SEXP, digits: i32, rho: SEXP) -> (Vec<String>, usize) {
    unsafe {
        if sms.is_null() || sms == R_NilValue() {
            return (Vec::new(), 0);
        }
        let names_attr =
            crate::sexp::attrib_core::getAttrib(sms, crate::sexp::attrib_core::R_NamesSymbol());
        let n = XLENGTH(sms);
        let mut labels = Vec::with_capacity(n as usize);
        for i in 0..n {
            labels.push(
                if !names_attr.is_null()
                    && TYPEOF(names_attr) == SEXPTYPE::STRSXP
                    && i < XLENGTH(names_attr)
                {
                    elt_to_string(names_attr, i)
                } else {
                    String::new()
                },
            );
        }
        let lw = labels.iter().map(|s| s.chars().count()).max().unwrap_or(0);
        let labels: Vec<String> = labels.into_iter().map(|s| format!("{s:<lw$}")).collect();

        let digits_s = Rf_ScalarInteger(digits);
        let _ds = protect(digits_s);
        let rest = Rf_cons(digits_s, R_NilValue());
        SETTAG(rest, Rf_install(c"digits".as_ptr()));
        let _rest = protect(rest);
        let fmt_args = Rf_cons(sms, rest);
        let _fa = protect(fmt_args);
        let formatted = crate::mainutils::essentials::do_format(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fmt_args,
            rho,
        );

        let _f = protect(formatted);
        let mut cells = Vec::with_capacity(n as usize);
        for i in 0..n {
            let value = if !formatted.is_null()
                && TYPEOF(formatted) == SEXPTYPE::STRSXP
                && i < XLENGTH(formatted)
            {
                elt_to_string(formatted, i)
            } else {
                String::new()
            };
            cells.push(format!(
                "{}:{value}  ",
                labels.get(i as usize).cloned().unwrap_or_default()
            ));
        }
        (cells, lw)
    }
}

/// R's `format.data.frame(x, ..., justify = "none")`.
/// GNU `format.data.frame(x, ...)` — dispatch to the verbatim base
/// closure (per-column format dispatch, corrupt-frame padding, exact
/// as.data.frame.list assembly with cut.names).
pub unsafe fn do_format_data_frame(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = crate::sexp::envir::R_findVarInFrame(
            crate::sexp::globals::R_BaseEnv(),
            crate::sexp::symbol::Rf_install(c"format.data.frame".as_ptr()),
        );
        let _fun_guard = crate::sexp::protect::protect(fun);
        let mut cells: Vec<SEXP> = Vec::new();
        let mut _guards = Vec::new();
        let mut cursor = args;
        while !cursor.is_null() && cursor != R_NilValue() {
            let cell = Rf_cons(CAR(cursor), R_NilValue());
            _guards.push(crate::sexp::protect::protect(cell));
            cells.push(cell);
            SETTAG(cell, TAG(cursor));
            cursor = CDR(cursor);
        }
        let call = Rf_cons(fun, R_NilValue());
        let _call_guard = crate::sexp::protect::protect(call);
        let mut tail = call;
        for cell in &cells {
            SETCDR(tail, *cell);
            tail = *cell;
        }
        crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        crate::eval::eval::Rf_eval(call, rho)
    }
}

unsafe fn format_data_frame_rest_args(args: SEXP) -> SEXP {
    unsafe {
        let rest = CDR(args);
        let mut cell = rest;
        while !cell.is_null() && cell != R_NilValue() {
            if tag_name(cell).as_deref() == Some("justify") {
                return rest;
            }
            cell = CDR(cell);
        }
        let none = crate::sexp::constructors::Rf_mkString(c"none".as_ptr());
        let _none = protect(none);
        let cell = Rf_cons(none, rest);
        SETTAG(cell, Rf_install(c"justify".as_ptr()));
        cell
    }
}

unsafe fn mark_asis_if_character(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return x;
        }
        if crate::mainutils::objects::inherits2(x, c"AsIs".as_ptr()) != FALSE {
            return x;
        }
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class_vec.is_null() {
            return x;
        }
        let _c = protect(class_vec);
        SET_STRING_ELT(
            class_vec,
            0,
            crate::sexp::constructors::Rf_mkChar(c"AsIs".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class_vec,
        );
        x
    }
}
