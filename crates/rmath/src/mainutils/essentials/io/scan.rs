use super::*;

// ---------------------------------------------------------------------------
// Complete I/O: scan, write.table, sink
// ---------------------------------------------------------------------------

pub(crate) fn scan_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

fn split_scan_fields(
    contents: &str,
    sep: &str,
    quote: &str,
    nmax: i64,
    strip_white: bool,
) -> Vec<String> {
    let limit = if nmax > 0 { nmax as usize } else { usize::MAX };
    let spec = TableParseSpec {
        sep: sep.chars().next(),
        quotes: quote.chars().collect(),
        comment: None,
        allow_escape: false,
        strip_white,
        blank_lines_skip: true,
    };
    let mut fields: Vec<String> = parse_table_records(contents, &spec)
        .into_iter()
        .flatten()
        .take(limit)
        .map(|field| field.text)
        .collect();
    if sep == "\n" && fields.last().is_some_and(|field| field.is_empty()) {
        fields.pop();
    }
    fields
}

fn parse_scan_logical(value: &str) -> Option<c_int> {
    match value {
        "TRUE" | "True" | "true" | "T" | "1" => Some(TRUE),
        "FALSE" | "False" | "false" | "F" | "0" => Some(FALSE),
        "NA" => Some(NA_LOGICAL),
        _ => None,
    }
}

pub(crate) unsafe fn named_arg(args: SEXP, name: &str) -> Option<SEXP> {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let tag = TAG(current);
            if !tag.is_null() && tag != R_NilValue() {
                let printname = PRINTNAME(tag);
                if !printname.is_null() {
                    let tag_name = CStr::from_ptr(CHAR(printname)).to_string_lossy();
                    if tag_name == name {
                        return Some(CAR(current));
                    }
                }
            }
            current = CDR(current);
        }
        None
    }
}

/// R's `scan(file, what, nmax, sep, ...)` — read data from a file path.
/// This covers the file-backed scalar-vector surface used by scripts and tests;
/// interactive console and connection scans report explicit R errors.
pub unsafe fn do_scan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Resolve arguments the way stock .Internal(scan(...)) sees them, by
        // named tag first, then positional slot:
        // (file, what, nmax, sep, dec, quote, skip, nlines, na.strings,
        //  flush, fill, text, fileEncoding).
        let by_slot = |names: &[&str], position: usize| -> SEXP {
            arg_by_name_or_position(args, names, position)
        };
        // Stock precedence: text= is used only when file= is missing
        // (scan.R: `if (missing(file) && !missing(text)) file <- textConnection(text)`).
        let text_arg = by_slot(&["text"], 11);
        let file_arg = by_slot(&["file"], 0);
        let what_arg = by_slot(&["what"], 1);
        let nmax_arg = by_slot(&["nmax"], 2);
        let file_given = !file_arg.is_null() && file_arg != R_NilValue();
        let text = if file_given
            || text_arg.is_null()
            || text_arg == R_NilValue()
            || XLENGTH(text_arg) == 0
        {
            None
        } else {
            if TYPEOF(text_arg) != SEXPTYPE::STRSXP {
                scan_error("invalid 'text' argument");
            }
            Some(elt_to_string(text_arg, 0))
        };
        let mut scan_conn_idx: Option<i32> = None;
        let mut scan_conn_bytes: Vec<u8> = Vec::new();
        let nlines = match named_arg(args, "nlines") {
            Some(nl) if !nl.is_null() && nl != R_NilValue() => real_or_default(nl, -1.0) as i64,
            _ => -1_i64,
        };
        let contents = if let Some(text) = text {
            crate::mainutils::browser_files::admit_text(text)
                .unwrap_or_else(|error| scan_error(error.to_string()))
        } else {
            if file_arg.is_null() || file_arg == R_NilValue() || file_arg == R_MissingArg() {
                scan_error("scan() requires a file path in the Android/headless runtime");
            }
            let (filename, from_conn) =
                if TYPEOF(file_arg) == SEXPTYPE::INTSXP && XLENGTH(file_arg) == 1 {
                    let idx = *INTEGER(file_arg);
                    let quote_chars = {
                        let quote_arg = by_slot(&["quote"], 5);
                        if quote_arg.is_null() || quote_arg == R_NilValue() {
                            "\"'".to_string()
                        } else {
                            elt_to_string(quote_arg, 0)
                        }
                    };
                    let sep_for_lines = {
                        let sep_arg = by_slot(&["sep"], 3);
                        if sep_arg.is_null() || sep_arg == R_NilValue() {
                            String::new()
                        } else {
                            elt_to_string(sep_arg, 0)
                        }
                    };
                    let mut bytes = Vec::new();
                    let mut lines_read = 0i64;
                    let mut in_quote: u8 = 0;
                    let mut at_token_start = true;
                    loop {
                        let b = crate::mainutils::connections::connection_fgetc(idx);
                        if b < 0 {
                            break;
                        }
                        let byte = b as u8;
                        bytes.push(byte);
                        if in_quote == 0 {
                            if byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r' {
                                at_token_start = true;
                                if nlines >= 0 && byte == b'\n' {
                                    lines_read += 1;
                                    if lines_read >= nlines {
                                        break;
                                    }
                                }
                            } else if quote_chars.as_bytes().contains(&byte)
                                && (at_token_start || !sep_for_lines.is_empty())
                            {
                                in_quote = byte;
                                at_token_start = false;
                            } else {
                                at_token_start = false;
                            }
                        } else if byte == in_quote {
                            in_quote = 0;
                        }
                    }
                    (
                        String::from_utf8_lossy(&bytes).into_owned(),
                        Some((idx, bytes)),
                    )
                } else if TYPEOF(file_arg) != SEXPTYPE::STRSXP || XLENGTH(file_arg) < 1 {
                    scan_error("scan() currently supports character file paths only");
                } else {
                    (elt_to_string(file_arg, 0), None)
                };
            if from_conn.is_none() && filename.is_empty() {
                scan_error("scan() cannot read from an interactive console in this runtime");
            }
            if let Some((idx, bytes)) = from_conn {
                scan_conn_idx = Some(idx);
                scan_conn_bytes = bytes;
                crate::mainutils::browser_files::admit_text(filename)
                    .unwrap_or_else(|error| scan_error(error.to_string()))
            } else {
                match crate::mainutils::browser_files::read_text_or_host(&filename) {
                    Ok(s) => s,
                    Err(err) => scan_error(format!("cannot open file '{filename}': {err}")),
                }
            }
        };
        let what_type = if what_arg.is_null() || what_arg == R_NilValue() {
            SEXPTYPE::REALSXP.as_c_int()
        } else {
            TYPEOF(what_arg)
        };
        let nmax = if nmax_arg.is_null() || nmax_arg == R_NilValue() {
            -1_i64
        } else {
            real_or_default(nmax_arg, -1.0) as i64
        };
        // nlines is applied after the quote argument is known, so a newline
        // inside quotes still belongs to the same record.
        let skip_arg = by_slot(&["skip"], 6);
        let skip_n: usize = if skip_arg.is_null() || skip_arg == R_NilValue() {
            0
        } else {
            real_or_default(skip_arg, 0.0).max(0.0) as usize
        };
        // Skip within the admitted buffer, avoiding another full text copy.
        let contents = contents
            .splitn(skip_n.saturating_add(1), '\n')
            .nth(skip_n)
            .unwrap_or("");
        let sep_arg = by_slot(&["sep"], 3);
        let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
            String::new()
        } else {
            elt_to_string(sep_arg, 0)
        };
        let quote_arg = by_slot(&["quote"], 5);
        let quote = if quote_arg.is_null() || quote_arg == R_NilValue() {
            "\"'".to_string()
        } else {
            elt_to_string(quote_arg, 0)
        };
        let contents = if nlines >= 0 {
            let quotes: Vec<char> = quote.chars().collect();
            let mut end = 0;
            let mut records = 0i64;
            let mut in_quote: Option<char> = None;
            for (index, c) in contents.char_indices() {
                if records >= nlines {
                    break;
                }
                end = index + c.len_utf8();
                if let Some(q) = in_quote {
                    if c == q {
                        in_quote = None;
                    }
                } else if quotes.contains(&c) {
                    in_quote = Some(c);
                } else if c == '\n' {
                    records += 1;
                }
            }
            &contents[..end]
        } else {
            contents
        };
        let na_arg = by_slot(&["na.strings"], 8);
        let na_strings: Vec<String> = if na_arg.is_null() || na_arg == R_NilValue() {
            vec!["NA".to_string()]
        } else if TYPEOF(na_arg) == SEXPTYPE::STRSXP {
            (0..XLENGTH(na_arg))
                .map(|i| elt_to_string(na_arg, i))
                .collect()
        } else {
            vec!["NA".to_string()]
        };
        let n_arg = named_arg(args, "n");
        let n_limit = match n_arg {
            Some(v) if !v.is_null() && v != R_NilValue() => real_or_default(v, -1.0) as i64,
            _ => -1_i64,
        };
        let item_cap = if n_limit >= 0 { n_limit } else { nmax };
        let strip_arg = named_arg(args, "strip.white");
        let strip_white = match strip_arg {
            Some(v) if !v.is_null() && v != R_NilValue() => real_or_default(v, 0.0) != 0.0,
            _ => false,
        };
        let field_cap = if what_type == SEXPTYPE::VECSXP {
            -1
        } else {
            item_cap
        };
        let values = split_scan_fields(&contents, &sep, &quote, field_cap, strip_white);
        if let Some(idx) = scan_conn_idx {
            if field_cap >= 0 {
                let bytes = &scan_conn_bytes;
                let mut i = 0usize;
                if skip_n > 0 {
                    let mut lines = 0usize;
                    while i < bytes.len() && lines < skip_n {
                        if bytes[i] == b'\n' {
                            lines += 1;
                        }
                        i += 1;
                    }
                }
                let mut seen = 0i64;
                while i < bytes.len() && seen < field_cap {
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i >= bytes.len() {
                        break;
                    }
                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    seen += 1;
                }
                if i < bytes.len() {
                    crate::mainutils::connections::connection_pushback(idx, &bytes[i..]);
                }
            }
        }
        let n = values.len() as R_xlen_t;
        let quiet = match named_arg(args, "quiet") {
            Some(q) if !q.is_null() && q != R_NilValue() && XLENGTH(q) > 0 => {
                TYPEOF(q) != SEXPTYPE::LGLSXP || *LOGICAL(q) != 0
            }
            _ => false,
        };
        if !quiet {
            let item_word = if n == 1 { "item" } else { "items" };
            // GNU scan.c emits "Read %d items" via REprintf (the error
            // stream / message sink), not stdout.
            crate::sexp::output::capture_stderr(&format!("Read {n} {item_word}\n"));
        }
        if what_type == SEXPTYPE::INTSXP {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            let dst = INTEGER(result);
            for (i, value) in values.iter().enumerate() {
                let parsed = if na_strings.iter().any(|s| s == value) {
                    NA_INTEGER
                } else if value == "NA" {
                    scan_error(format!("scan() expected an integer, got '{value}'"))
                } else {
                    value.parse::<c_int>().unwrap_or_else(|_| {
                        scan_error(format!("scan() expected an integer, got '{value}'"))
                    })
                };
                *dst.add(i) = parsed;
            }
            result
        } else if what_type == SEXPTYPE::REALSXP {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            let dst = REAL(result);
            for (i, value) in values.iter().enumerate() {
                let parsed = if na_strings.iter().any(|s| s == value) {
                    NA_REAL
                } else if value == "NA" {
                    scan_error(format!("scan() expected a real, got '{value}'"))
                } else {
                    crate::mainutils::coerce::parse_double_str(value).unwrap_or_else(|| {
                        scan_error(format!("scan() expected a real, got '{value}'"))
                    })
                };
                *dst.add(i) = parsed;
            }
            result
        } else if what_type == SEXPTYPE::LGLSXP {
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            let dst = LOGICAL(result);
            for (i, value) in values.iter().enumerate() {
                let parsed = parse_scan_logical(value).unwrap_or_else(|| {
                    scan_error(format!("scan() expected a logical, got '{value}'"))
                });
                *dst.add(i) = parsed;
            }
            result
        } else if what_type == SEXPTYPE::STRSXP {
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            for (i, value) in values.iter().enumerate() {
                let charsxp = if na_strings.iter().any(|s| s == value) {
                    crate::mainutils::relop::NA_STRING()
                } else {
                    let cstr = CString::new(value.as_str()).unwrap_or_default();
                    crate::sexp::constructors::Rf_mkChar(cstr.as_ptr())
                };
                SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
            result
        } else if what_type == SEXPTYPE::VECSXP {
            let nc = XLENGTH(what_arg);
            if nc <= 0 {
                scan_error("scan() 'what' list is empty");
            }
            let mut col_types = Vec::with_capacity(nc as usize);
            for j in 0..nc {
                let tmpl = VECTOR_ELT(what_arg, j);
                let ty = TYPEOF(tmpl);
                if ty != SEXPTYPE::STRSXP
                    && ty != SEXPTYPE::REALSXP
                    && ty != SEXPTYPE::INTSXP
                    && ty != SEXPTYPE::LGLSXP
                {
                    scan_error("scan() list 'what' only supports atomic fields");
                }
                col_types.push(ty);
            }
            let nrec = if nmax >= 0 { nmax as R_xlen_t } else { n / nc };
            let take = nrec * nc;
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, nc);
            let _p = protect(result);
            for j in 0..nc {
                let ty = col_types[j as usize];
                let col = Rf_allocVector3(ty, nrec);
                SET_VECTOR_ELT(result, j, col);
                for r in 0..nrec {
                    let idx = r * nc + j;
                    let text = if idx < n && idx < take {
                        values[idx as usize].as_str()
                    } else {
                        ""
                    };
                    if ty == SEXPTYPE::STRSXP {
                        let charsxp = if na_strings.iter().any(|s| s == text) {
                            crate::mainutils::relop::NA_STRING()
                        } else {
                            let cstr = CString::new(text).unwrap_or_default();
                            crate::sexp::constructors::Rf_mkChar(cstr.as_ptr())
                        };
                        SET_STRING_ELT(col, r, charsxp);
                    } else if ty == SEXPTYPE::REALSXP {
                        *REAL(col).add(r as usize) = if text == "NA" || text.is_empty() {
                            NA_REAL
                        } else {
                            crate::mainutils::coerce::parse_double_str(text).unwrap_or(NA_REAL)
                        };
                    } else if ty == SEXPTYPE::INTSXP {
                        *INTEGER(col).add(r as usize) = if text == "NA" || text.is_empty() {
                            NA_INTEGER
                        } else {
                            text.parse::<c_int>().unwrap_or(NA_INTEGER)
                        };
                    } else {
                        *LOGICAL(col).add(r as usize) =
                            parse_scan_logical(text).unwrap_or(NA_INTEGER);
                    }
                }
            }
            let names = crate::sexp::attrib_core::getAttrib(
                what_arg,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if !names.is_null() && names != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
            result
        } else {
            scan_error("scan() only supports integer, numeric, logical, and character 'what'")
        }
    }
}

/// R's `write.table(x, file, sep=" ", ...)` — write data to file.
pub unsafe fn do_write_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x_arg = R_NilValue();
        let mut file_arg = R_NilValue();
        let mut sep = " ".to_string();
        let mut row_names = true;
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            let name = tag_name(current);
            match name.as_deref() {
                Some("x") => x_arg = arg,
                Some("file") => file_arg = arg,
                Some("sep") => sep = elt_to_string(arg, 0),
                Some("row.names") => {
                    row_names = !(TYPEOF(arg) == SEXPTYPE::LGLSXP && *LOGICAL(arg) == 0)
                }
                _ => {
                    match positional {
                        0 => x_arg = arg,
                        1 => file_arg = arg,
                        2 => sep = elt_to_string(arg, 0),
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }
        if x_arg.is_null()
            || x_arg == R_NilValue()
            || file_arg.is_null()
            || file_arg == R_NilValue()
        {
            return R_NilValue();
        }
        let filename = elt_to_string(file_arg, 0);
        let sep = sep.as_str();
        let _ = row_names;
        let rn = crate::sexp::attrib_core::getAttrib(
            x_arg,
            crate::sexp::attrib_core::R_RowNamesSymbol(),
        );

        let mut output = String::new();
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);

        if t == SEXPTYPE::VECSXP {
            // Data frame-like: write columns
            let ncols = n;
            let nrows = if n > 0 {
                XLENGTH(VECTOR_ELT(x_arg, 0))
            } else {
                0
            };
            if ncols == 0 {
                output.push_str("\"\"\n");
            }
            // Write header with column names
            let names_sym = Rf_install(c"names".as_ptr());
            let names = crate::sexp::attrib_core::getAttrib(x_arg, names_sym);
            if ncols > 0
                && !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
            {
                let mut header = Vec::new();
                if row_names {
                    header.push(String::new());
                }
                for j in 0..ncols {
                    let charsxp = crate::sexp::accessors::STRING_ELT(names, j);
                    if !charsxp.is_null() {
                        let s = crate::sexp::accessors::CHAR(charsxp);
                        if !s.is_null() {
                            header.push(
                                std::ffi::CStr::from_ptr(s)
                                    .to_str()
                                    .unwrap_or("")
                                    .to_string(),
                            );
                        } else {
                            header.push(String::new());
                        }
                    } else {
                        header.push(String::new());
                    }
                }
                output.push_str(&header.join(sep));
                output.push('\n');
            }
            // Write rows
            for i in 0..nrows {
                let mut row = Vec::new();
                if row_names {
                    let label = if TYPEOF(rn) == SEXPTYPE::STRSXP && XLENGTH(rn) == nrows {
                        elt_to_string(rn, i)
                    } else {
                        (i + 1).to_string()
                    };
                    row.push(format!("\"{}\"", label.replace('"', "\"\"")));
                }
                for j in 0..ncols {
                    let col = VECTOR_ELT(x_arg, j);
                    if !col.is_null() && col != R_NilValue() {
                        let cell = elt_to_string(col, i);
                        if TYPEOF(col) == SEXPTYPE::STRSXP {
                            row.push(format!("\"{}\"", cell.replace('"', "\"\"")));
                        } else {
                            row.push(cell);
                        }
                    } else {
                        row.push("NA".to_string());
                    }
                }
                output.push_str(&row.join(sep));
                output.push('\n');
            }
        } else {
            // Atomic vector: write as single column
            for i in 0..n {
                output.push_str(&elt_to_string(x_arg, i));
                output.push('\n');
            }
        }

        if let Err(error) =
            crate::mainutils::browser_files::write_text_or_host(&filename, output.as_bytes())
        {
            base_error(error.to_string());
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `sink(file, append, type, split)` — redirect output to a connection.
pub unsafe fn do_sink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut file_arg = R_NilValue();
        let mut append_arg = Rf_ScalarLogical(FALSE);
        let mut type_arg = Rf_mkString(c"output".as_ptr());
        let mut split_arg = Rf_ScalarLogical(FALSE);
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match tag_name(current).as_deref() {
                Some("file") => file_arg = arg,
                Some("append") => append_arg = arg,
                Some("type") => type_arg = arg,
                Some("split") => split_arg = arg,
                _ => {
                    match positional {
                        0 => file_arg = arg,
                        1 => append_arg = arg,
                        2 => type_arg = arg,
                        3 => split_arg = arg,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let split = logical_scalar_or(split_arg, FALSE);
        let is_message_sink = !type_arg.is_null()
            && type_arg != R_NilValue()
            && TYPEOF(type_arg) == SEXPTYPE::STRSXP
            && elt_to_string(type_arg, 0) == "message";

        let (target, close_on_exit) = if file_arg.is_null() || file_arg == R_NilValue() {
            if is_message_sink {
                (Rf_ScalarInteger(2), FALSE)
            } else {
                (Rf_ScalarInteger(-1), FALSE)
            }
        } else if inherits_class(file_arg, "connection") {
            (file_arg, FALSE)
        } else if TYPEOF(file_arg) == SEXPTYPE::STRSXP {
            if is_message_sink {
                base_error("'file' must be NULL or an already open connection");
            }
            let append = logical_scalar_or(append_arg, FALSE) != FALSE;
            let open = if append { "a" } else { "w" };
            let open_sxp = Rf_mkString(CString::new(open).unwrap_or_default().as_ptr());
            // GNU sink.R calls file(file, if(append) "a" else "w"): only
            // description and open are passed positionally, so file()'s
            // blocking (pos 2) and encoding (pos 3) keep their defaults.
            let file_args = Rf_cons(file_arg, Rf_cons(open_sxp, R_NilValue()));
            (
                crate::mainutils::connections::do_file(_call, _op, file_args, _rho),
                TRUE,
            )
        } else {
            base_error("'file' must be NULL, a connection or a character string");
        };
        if is_message_sink && split != FALSE {
            base_error("cannot split the message connection");
        }

        let normalized = Rf_cons(
            target,
            Rf_cons(
                Rf_ScalarLogical(close_on_exit),
                Rf_cons(
                    Rf_ScalarLogical(if is_message_sink { TRUE } else { FALSE }),
                    Rf_cons(Rf_ScalarLogical(split), R_NilValue()),
                ),
            ),
        );
        crate::mainutils::connections::do_sink(_call, _op, normalized, _rho);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `sink.number(type)` — report output or message sink depth.
pub unsafe fn do_sink_number(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let type_arg = if args.is_null() || args == R_NilValue() {
            Rf_mkString(c"output".as_ptr())
        } else {
            CAR(args)
        };
        let is_message_sink = !type_arg.is_null()
            && type_arg != R_NilValue()
            && TYPEOF(type_arg) == SEXPTYPE::STRSXP
            && elt_to_string(type_arg, 0) == "message";
        let normalized = Rf_cons(
            Rf_ScalarLogical(if is_message_sink { TRUE } else { FALSE }),
            R_NilValue(),
        );
        crate::mainutils::connections::do_sinkNumber(_call, _op, normalized, _rho)
    }
}

unsafe fn logical_scalar_or(arg: SEXP, default: c_int) -> c_int {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            return default;
        }
        if TYPEOF(arg) == SEXPTYPE::LGLSXP || TYPEOF(arg) == SEXPTYPE::INTSXP {
            *INTEGER(arg)
        } else {
            default
        }
    }
}
