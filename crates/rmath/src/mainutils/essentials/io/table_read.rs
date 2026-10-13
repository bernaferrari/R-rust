use super::*;

// ---------------------------------------------------------------------------
// Complete I/O — read.csv, write.csv, read.table
// ---------------------------------------------------------------------------

/// R's `read.csv(file, header=TRUE, sep=",")` — read a CSV file (simplified).
/// Returns a list (data.frame) of columns as REALSXP vectors.
pub unsafe fn do_read_csv(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let header = Rf_ScalarLogical(1);
        let _h = protect(header);
        let sep = Rf_mkString(c",".as_ptr());
        let _s = protect(sep);
        let file = CAR(args);
        let rest = CDR(args);
        let built = Rf_cons(file, Rf_cons(header, Rf_cons(sep, rest)));
        let _b = protect(built);
        SETTAG(CDR(built), Rf_install(c"header".as_ptr()));
        SETTAG(CDR(CDR(built)), Rf_install(c"sep".as_ptr()));
        do_read_table(call, op, built, rho)
    }
}

/// R's `write.csv(x, file, row.names=TRUE)` — write a CSV file (simplified).
pub unsafe fn do_write_csv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };
        let row_names_arg = if CDR(args).is_null()
            || CDR(args) == R_NilValue()
            || CDR(CDR(args)).is_null()
            || CDR(CDR(args)) == R_NilValue()
        {
            R_NilValue()
        } else {
            CAR(CDR(CDR(args)))
        };

        let file_path = elt_to_string(file_arg, 0);
        let write_row_names = if row_names_arg.is_null() || row_names_arg == R_NilValue() {
            true
        } else {
            let v = real_or_default(row_names_arg, 1.0);
            v != 0.0
        };

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(x);
        let mut lines: Vec<String> = Vec::new();

        if t == SEXPTYPE::VECSXP {
            // Data.frame-like list
            let ncols = XLENGTH(x);
            let nrow = if ncols > 0 {
                let first_col = VECTOR_ELT(x, 0);
                XLENGTH(first_col)
            } else {
                0
            };

            // Get column names
            let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));

            // Header
            let mut header_parts: Vec<String> = Vec::new();
            if write_row_names {
                header_parts.push(String::new());
            }
            for j in 0..ncols {
                let nm = if !names.is_null() {
                    elt_to_string(names, j)
                } else {
                    format!("V{}", j + 1)
                };
                header_parts.push(format!("\"{}\"", nm));
            }
            lines.push(header_parts.join(","));

            // Data rows
            for i in 0..nrow {
                let mut row_parts: Vec<String> = Vec::new();
                if write_row_names {
                    let rn = crate::sexp::attrib_core::getAttrib(
                        x,
                        crate::sexp::attrib_core::R_RowNamesSymbol(),
                    );
                    let label = if TYPEOF(rn) == SEXPTYPE::STRSXP && XLENGTH(rn) == nrow {
                        format!("\"{}\"", elt_to_string(rn, i))
                    } else {
                        (i + 1).to_string()
                    };
                    row_parts.push(label);
                }
                for j in 0..ncols {
                    let col = VECTOR_ELT(x, j);
                    row_parts.push(elt_to_string(col, i));
                }
                lines.push(row_parts.join(","));
            }
        } else if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP {
            // Simple vector — write as single column
            let n = XLENGTH(x);
            lines.push("\"x\"".to_string());
            for i in 0..n {
                lines.push(elt_to_string(x, i));
            }
        }

        let content = lines.join("\n") + "\n";
        if let Err(e) =
            crate::mainutils::browser_files::write_text_or_host(&file_path, content.as_bytes())
        {
            base_error(format!("cannot write file '{}': {}", file_path, e));
        }

        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

/// One parsed table cell: the field text plus whether it was quoted.
/// Quoting only survives into blank/`NA` decisions — na.strings match
/// either way (upstream scan accepts a quoted "NA" as NA) while a blank
/// unquoted field is NA for typed columns and "" for character ones.
#[derive(Clone)]
pub(crate) struct TableField {
    pub(crate) text: String,
    pub(crate) quoted: bool,
}

/// Lexing rules shared by record parsing: separator, quote set, comment
/// character, and whitespace handling, mirroring upstream scan semantics.
pub(crate) struct TableParseSpec {
    pub(crate) sep: Option<char>,
    pub(crate) quotes: Vec<char>,
    pub(crate) comment: Option<char>,
    pub(crate) strip_white: bool,
    pub(crate) blank_lines_skip: bool,
    pub(crate) allow_escape: bool,
}

fn table_push_field(
    record: &mut Vec<TableField>,
    field: &mut String,
    quoted: &mut bool,
    started: &mut bool,
    spec: &TableParseSpec,
) {
    let mut text = std::mem::take(field);
    if !*quoted && spec.strip_white {
        text = text.trim().to_string();
    }
    record.push(TableField {
        text,
        quoted: *quoted,
    });
    *quoted = false;
    *started = false;
}

/// Split raw file text into records of fields, honoring the separator,
/// quote characters (including doubled quotes and newlines inside quoted
/// fields), the comment character, and blank-line skipping.
pub(crate) fn parse_table_records(content: &str, spec: &TableParseSpec) -> Vec<Vec<TableField>> {
    let mut records: Vec<Vec<TableField>> = Vec::new();
    let mut record: Vec<TableField> = Vec::new();
    let mut field = String::new();
    let mut field_quoted = false;
    let mut field_started = false;
    let mut quote: Option<char> = None;
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == '\\' {
                if let Some(next) = chars.next() {
                    if next == q {
                        field.push(q);
                    } else if spec.allow_escape {
                        field.push(match next {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            other => other,
                        });
                    } else {
                        field.push('\\');
                        field.push(next);
                    }
                }
            } else if c == q {
                if chars.peek() == Some(&q) {
                    chars.next();
                    field.push(q);
                } else {
                    quote = None;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        if spec.quotes.contains(&c) && (spec.sep.is_some() || !field_started) {
            quote = Some(c);
            field_quoted = true;
            field_started = true;
            continue;
        }
        if Some(c) == spec.sep {
            table_push_field(
                &mut record,
                &mut field,
                &mut field_quoted,
                &mut field_started,
                spec,
            );
            continue;
        }
        if spec.sep.is_none() && (c == ' ' || c == '\t' || c == '\r') {
            if field_started {
                table_push_field(
                    &mut record,
                    &mut field,
                    &mut field_quoted,
                    &mut field_started,
                    spec,
                );
            }
            continue;
        }
        if c == '\n' {
            if record.is_empty() && !field_started && !field_quoted {
                if spec.blank_lines_skip {
                    continue;
                }
                records.push(Vec::new());
                continue;
            }
            if field_started || field_quoted || spec.sep.is_some() {
                table_push_field(
                    &mut record,
                    &mut field,
                    &mut field_quoted,
                    &mut field_started,
                    spec,
                );
            }
            records.push(std::mem::take(&mut record));
            continue;
        }
        if c == '\r' {
            continue;
        }
        if Some(c) == spec.comment {
            while let Some(&nc) = chars.peek() {
                if nc == '\n' {
                    break;
                }
                chars.next();
            }
            continue;
        }
        field.push(c);
        field_started = true;
    }
    if quote.is_some() || field_started || field_quoted || !record.is_empty() {
        if field_started || field_quoted || spec.sep.is_some() {
            table_push_field(
                &mut record,
                &mut field,
                &mut field_quoted,
                &mut field_started,
                spec,
            );
        }
        records.push(record);
    }
    records
}

/// Declared column types from `colClasses`, recycled across columns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TableColClass {
    Logical,
    Integer,
    Numeric,
    Character,
    Factor,
    Null,
    Infer,
}

unsafe fn parse_table_col_classes(arg: SEXP, col_names: &[String]) -> Vec<TableColClass> {
    unsafe {
        let ncols = col_names.len();
        let mut declared: Vec<Option<TableColClass>> = vec![None; ncols];
        if !arg.is_null() && arg != R_NilValue() && TYPEOF(arg) == SEXPTYPE::STRSXP {
            let n = XLENGTH(arg) as usize;
            let names =
                crate::sexp::attrib_core::getAttrib(arg, crate::sexp::attrib_core::R_NamesSymbol());
            let named = !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) == n as i64;
            if named {
                let mut missing = false;
                for src in 0..n {
                    if is_string_na(arg, src as R_xlen_t) {
                        continue;
                    }
                    let nm = elt_to_string(names, src as R_xlen_t);
                    if let Some(j) = col_names.iter().position(|c| c == &nm) {
                        declared[j] =
                            Some(table_col_class_name(&elt_to_string(arg, src as R_xlen_t)));
                    } else {
                        missing = true;
                    }
                }
                if missing {
                    let msg = std::ffi::CString::new("not all columns named in 'colClasses' exist")
                        .unwrap_or_default();
                    crate::mainutils::errors::Rf_warning(msg.as_ptr());
                }
            } else if n > 0 {
                for j in 0..ncols {
                    let src = j % n;
                    if is_string_na(arg, src as R_xlen_t) {
                        continue;
                    }
                    declared[j] = Some(table_col_class_name(&elt_to_string(arg, src as R_xlen_t)));
                }
            }
        }
        declared
            .into_iter()
            .map(|d| d.unwrap_or(TableColClass::Infer))
            .collect()
    }
}

fn table_col_class_name(name: &str) -> TableColClass {
    match name {
        "logical" => TableColClass::Logical,
        "integer" => TableColClass::Integer,
        "numeric" | "double" | "real" => TableColClass::Numeric,
        "character" => TableColClass::Character,
        "factor" => TableColClass::Factor,
        "NULL" => TableColClass::Null,
        _ => TableColClass::Character,
    }
}

/// Upstream `make.logical` accepts the strict TRUE/FALSE spellings.
fn parse_table_logical(text: &str) -> Option<c_int> {
    match text {
        "T" | "TRUE" | "true" | "True" => Some(TRUE),
        "F" | "FALSE" | "false" | "False" => Some(FALSE),
        _ => None,
    }
}

/// Parse an R numeric literal: f64 syntax plus the leading-dot forms
/// (".5", "-.5") that Rust's parser rejects.
fn parse_table_double(text: &str) -> Option<f64> {
    let s = text.trim();
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    if let Some(rest) = s.strip_prefix('.') {
        format!("0.{rest}").parse::<f64>().ok()
    } else if let Some(rest) = s.strip_prefix("-.") {
        format!("-0.{rest}").parse::<f64>().ok()
    } else if let Some(rest) = s.strip_prefix("+.") {
        format!("0.{rest}").parse::<f64>().ok()
    } else {
        None
    }
}

fn table_field_is_na(field: &TableField, na_strings: &[String]) -> bool {
    na_strings.contains(&field.text)
}

fn table_field_is_blank(field: &TableField) -> bool {
    !field.quoted && field.text.is_empty()
}

/// Infer one column's type the way `type.convert` does: logical, then
/// integer, then numeric, falling back to character.
fn infer_table_col_class(fields: &[&TableField], na_strings: &[String]) -> TableColClass {
    let mut is_logical = true;
    let mut is_integer = true;
    let mut is_numeric = true;
    for field in fields {
        if table_field_is_na(field, na_strings) || table_field_is_blank(field) {
            continue;
        }
        if parse_table_logical(&field.text).is_none() {
            is_logical = false;
        }
        if field.text.trim().parse::<i32>().is_err() {
            is_integer = false;
        }
        if parse_table_double(&field.text).is_none() {
            is_numeric = false;
        }
    }
    if is_logical {
        TableColClass::Logical
    } else if is_integer {
        TableColClass::Integer
    } else if is_numeric {
        TableColClass::Numeric
    } else {
        TableColClass::Character
    }
}

/// R's `read.table(file, header = FALSE, sep = "", quote = "\"'", dec = ".",
/// na.strings = "NA", colClasses = NA, nrows = -1, skip = 0, fill, ...)`.
/// Reads a delimited file into a data.frame, honoring the arguments the
/// corpus exercises: separator, quoting (doubled quotes, embedded
/// separators/newlines inside quoted fields), comment characters,
/// na.strings, colClasses (typed, "NULL" to skip, NA to infer), header
/// handling, and blank-line skipping.
pub unsafe fn do_read_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let named_only = usize::MAX;
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        let header_arg = arg_by_name_or_position(args, &["header"], 1);
        let sep_arg = arg_by_name_or_position(args, &["sep"], 2);
        let quote_arg = arg_by_name_or_position(args, &["quote"], 3);
        let col_classes_arg = arg_by_name_or_position(args, &["colClasses"], named_only);
        let na_strings_arg =
            arg_by_name_or_position(args, &["na.strings", "NA.strings"], named_only);
        let strings_as_factors_arg =
            arg_by_name_or_position(args, &["stringsAsFactors"], named_only);
        let comment_arg = arg_by_name_or_position(args, &["comment.char"], named_only);
        let strip_white_arg = arg_by_name_or_position(args, &["strip.white"], named_only);
        let blank_skip_arg = arg_by_name_or_position(args, &["blank.lines.skip"], named_only);
        let fill_arg = arg_by_name_or_position(args, &["fill"], named_only);
        let nrows_arg = arg_by_name_or_position(args, &["nrows"], named_only);
        let skip_arg = arg_by_name_or_position(args, &["skip"], named_only);
        let text_arg = arg_by_name_or_position(args, &["text"], named_only);
        let row_names_arg = arg_by_name_or_position(args, &["row.names"], named_only);
        let col_names_arg = arg_by_name_or_position(args, &["col.names"], named_only);

        let sep_text = if sep_arg.is_null() || sep_arg == R_NilValue() {
            String::new()
        } else {
            elt_to_string(sep_arg, 0)
        };
        let sep = sep_text.chars().next();
        let mut header = !header_arg.is_null()
            && header_arg != R_NilValue()
            && real_or_default(header_arg, 0.0) != 0.0;
        let quote_text = if quote_arg.is_null() || quote_arg == R_NilValue() {
            "\"'".to_string()
        } else {
            elt_to_string(quote_arg, 0)
        };
        let comment_text = if comment_arg.is_null() || comment_arg == R_NilValue() {
            "#".to_string()
        } else {
            elt_to_string(comment_arg, 0)
        };
        let na_strings: Vec<String> = if na_strings_arg.is_null() || na_strings_arg == R_NilValue()
        {
            vec!["NA".to_string()]
        } else if TYPEOF(na_strings_arg) == SEXPTYPE::STRSXP {
            (0..XLENGTH(na_strings_arg))
                .map(|i| elt_to_string(na_strings_arg, i))
                .collect()
        } else {
            vec!["NA".to_string()]
        };
        let strings_as_factors = !strings_as_factors_arg.is_null()
            && strings_as_factors_arg != R_NilValue()
            && crate::mainutils::coerce::asLogical(strings_as_factors_arg) != 0;
        let blank_lines_skip = blank_skip_arg.is_null()
            || blank_skip_arg == R_NilValue()
            || real_or_default(blank_skip_arg, 1.0) != 0.0;
        let fill = if fill_arg.is_null() || fill_arg == R_NilValue() {
            !blank_lines_skip
        } else {
            real_or_default(fill_arg, 0.0) != 0.0
        };
        let strip_white = if strip_white_arg.is_null() || strip_white_arg == R_NilValue() {
            sep.is_some()
        } else {
            real_or_default(strip_white_arg, if sep.is_some() { 1.0 } else { 0.0 }) != 0.0
        };
        let nrows: i64 = if nrows_arg.is_null() || nrows_arg == R_NilValue() {
            -1
        } else {
            real_or_default(nrows_arg, -1.0) as i64
        };
        let skip: usize = if skip_arg.is_null() || skip_arg == R_NilValue() {
            0
        } else {
            real_or_default(skip_arg, 0.0).max(0.0) as usize
        };

        let content = if !text_arg.is_null()
            && text_arg != R_NilValue()
            && TYPEOF(text_arg) == SEXPTYPE::STRSXP
        {
            let text = (0..XLENGTH(text_arg))
                .map(|i| elt_to_string(text_arg, i))
                .collect::<Vec<_>>()
                .join("\n");
            crate::mainutils::browser_files::admit_text(text)
                .unwrap_or_else(|error| scan_error(error.to_string()))
        } else if TYPEOF(file_arg) == SEXPTYPE::INTSXP && XLENGTH(file_arg) >= 1 {
            let con = *INTEGER(file_arg);
            let mut bytes = Vec::new();
            loop {
                let c = crate::mainutils::connections::connection_fgetc(con);
                if c < 0 {
                    break;
                }
                bytes.push(c as u8);
            }
            crate::mainutils::browser_files::admit_text(
                String::from_utf8_lossy(&bytes).into_owned(),
            )
            .unwrap_or_else(|error| scan_error(error.to_string()))
        } else {
            if file_arg.is_null()
                || file_arg == R_NilValue()
                || TYPEOF(file_arg) != SEXPTYPE::STRSXP
            {
                scan_error("invalid 'file' argument");
            }
            let file_path = resolve_package_relative_path(elt_to_string(file_arg, 0));
            match crate::mainutils::browser_files::read_text_or_host(&file_path) {
                Ok(s) => s,
                Err(e) => {
                    scan_error(format!("cannot open file '{file_path}': {e}"));
                }
            }
        };

        let spec = TableParseSpec {
            sep,
            quotes: quote_text.chars().collect(),
            comment: comment_text.chars().next(),
            allow_escape: false,
            strip_white,
            blank_lines_skip,
        };
        let mut records = parse_table_records(&content, &spec);
        if skip > 0 {
            let drop = skip.min(records.len());
            records.drain(0..drop);
        }
        if nrows >= 0 {
            records.truncate(nrows as usize);
        }

        if records.is_empty() {
            return R_NilValue();
        }

        let mut data: Vec<Vec<TableField>> = records;
        let mut col_names: Vec<String> = Vec::new();
        if !header
            && data.len() >= 2
            && data[0].len() + 1 == data[1].len()
            && data[1..].iter().all(|row| row.len() == data[1].len())
        {
            header = true;
        }

        if header {
            let header_row = data.remove(0);
            col_names = header_row
                .iter()
                .map(|f| f.text.trim().replace(['\n', '\r'], "."))
                .collect();
        } else if !col_names_arg.is_null()
            && col_names_arg != R_NilValue()
            && TYPEOF(col_names_arg) == SEXPTYPE::STRSXP
        {
            col_names = (0..XLENGTH(col_names_arg))
                .map(|i| elt_to_string(col_names_arg, i))
                .collect();
        }
        let mut taken_row_names: Option<Vec<String>> = None;
        if header
            && !col_names.is_empty()
            && !data.is_empty()
            && data.iter().all(|row| row.len() == col_names.len() + 1)
        {
            let mut names = Vec::with_capacity(data.len());
            for row in &mut data {
                names.push(row.remove(0).text);
            }
            taken_row_names = Some(names);
        }
        if taken_row_names.is_none()
            && header
            && col_names.first().is_some_and(|name| name.is_empty())
            && data.iter().all(|row| row.len() == col_names.len())
        {
            col_names.remove(0);
            let mut names = Vec::with_capacity(data.len());
            for row in &mut data {
                names.push(row.remove(0).text);
            }
            taken_row_names = Some(names);
        }

        let mut ncols = col_names.len();
        for row in &data {
            ncols = ncols.max(row.len());
        }
        if ncols == 0 {
            return R_NilValue();
        }
        while col_names.len() < ncols {
            col_names.push(format!("V{}", col_names.len() + 1));
        }

        let mut padded: Vec<Vec<TableField>> = Vec::with_capacity(data.len());
        for (i, row) in data.iter().enumerate() {
            if row.len() > ncols {
                scan_error(format!("line {} did not have {} elements", i + 1, ncols));
            }
            let mut row = row.clone();
            while row.len() < ncols {
                if !fill && header {
                    scan_error(format!("line {} did not have {} elements", i + 1, ncols));
                }
                row.push(TableField {
                    text: String::new(),
                    quoted: false,
                });
            }
            padded.push(row);
        }
        let data = padded;
        let nrow = data.len() as R_xlen_t;

        let classes = parse_table_col_classes(col_classes_arg, &col_names);

        // Optional row.names: a single integer names the column to use,
        // a character vector supplies the names directly.
        let row_names_column: Option<usize> =
            if !row_names_arg.is_null() && row_names_arg != R_NilValue() {
                let row_names_type = TYPEOF(row_names_arg);
                if row_names_type == SEXPTYPE::INTSXP && XLENGTH(row_names_arg) == 1 {
                    Some(INTEGER_ELT(row_names_arg, 0).unsigned_abs() as usize)
                } else if row_names_type == SEXPTYPE::REALSXP && XLENGTH(row_names_arg) == 1 {
                    Some(REAL_ELT(row_names_arg, 0) as usize)
                } else {
                    None
                }
            } else {
                None
            };

        let mut out_names: Vec<String> = Vec::new();
        let mut out_cols: Vec<SEXP> = Vec::new();
        for j in 0..ncols {
            let declared = classes[j];
            let fields: Vec<&TableField> = data.iter().map(|row| &row[j]).collect();
            let class = if declared == TableColClass::Infer {
                infer_table_col_class(&fields, &na_strings)
            } else {
                declared
            };
            if class == TableColClass::Null || Some(j + 1) == row_names_column {
                continue;
            }
            let col = match class {
                TableColClass::Logical => {
                    let col = Rf_allocVector3(SEXPTYPE::LGLSXP, nrow);
                    let _guard = protect(col);
                    for (i, field) in fields.iter().enumerate() {
                        let value = if table_field_is_na(field, &na_strings)
                            || table_field_is_blank(field)
                        {
                            NA_INTEGER
                        } else {
                            parse_table_logical(&field.text).unwrap_or(NA_INTEGER)
                        };
                        *LOGICAL(col).add(i) = value;
                    }
                    col
                }
                TableColClass::Integer => {
                    let col = Rf_allocVector3(SEXPTYPE::INTSXP, nrow);
                    let _guard = protect(col);
                    for (i, field) in fields.iter().enumerate() {
                        let value = if table_field_is_na(field, &na_strings)
                            || table_field_is_blank(field)
                        {
                            NA_INTEGER
                        } else {
                            field.text.trim().parse::<i32>().unwrap_or(NA_INTEGER)
                        };
                        *INTEGER(col).add(i) = value;
                    }
                    col
                }
                TableColClass::Numeric => {
                    let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow);
                    let _guard = protect(col);
                    for (i, field) in fields.iter().enumerate() {
                        let value = if table_field_is_na(field, &na_strings)
                            || table_field_is_blank(field)
                        {
                            NA_REAL
                        } else {
                            parse_table_double(&field.text).unwrap_or(NA_REAL)
                        };
                        *REAL(col).add(i) = value;
                    }
                    col
                }
                TableColClass::Character | TableColClass::Factor => {
                    let col = Rf_allocVector3(SEXPTYPE::STRSXP, nrow);
                    let _guard = protect(col);
                    for (i, field) in fields.iter().enumerate() {
                        let value = if table_field_is_na(field, &na_strings) {
                            crate::mainutils::relop::NA_STRING()
                        } else {
                            let cstr = CString::new(field.text.as_str()).unwrap_or_default();
                            crate::sexp::constructors::Rf_mkChar(cstr.as_ptr())
                        };
                        SET_STRING_ELT(col, i as R_xlen_t, value);
                    }
                    if class == TableColClass::Factor {
                        let mut levels: Vec<String> = fields
                            .iter()
                            .map(|f| f.text.clone())
                            .filter(|t| !na_strings.contains(t))
                            .collect();
                        levels.sort();
                        levels.dedup();
                        crate::mainutils::essentials::tables::set_factor_attrs(col, &levels);
                    }
                    col
                }
                TableColClass::Null | TableColClass::Infer => unreachable!(),
            };
            out_cols.push(col);
            out_names.push(col_names[j].clone());
        }
        if strings_as_factors {
            for col in &mut out_cols {
                if TYPEOF(*col) != SEXPTYPE::STRSXP {
                    continue;
                }
                let n = XLENGTH(*col);
                let mut levels: Vec<String> = Vec::new();
                for i in 0..n {
                    let ch = STRING_ELT(*col, i);
                    if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                        continue;
                    }
                    let s = elt_to_string(*col, i);
                    if !levels.iter().any(|level| level == &s) {
                        levels.push(s);
                    }
                }
                levels.sort();
                let coded = Rf_allocVector3(SEXPTYPE::INTSXP, n);
                let _coded = protect(coded);
                for i in 0..n {
                    let ch = STRING_ELT(*col, i);
                    if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                        *INTEGER(coded).add(i as usize) = NA_INTEGER;
                    } else {
                        let s = elt_to_string(*col, i);
                        let code = levels.iter().position(|level| level == &s).unwrap_or(0);
                        *INTEGER(coded).add(i as usize) = code as i32 + 1;
                    }
                }
                crate::mainutils::essentials::tables::set_factor_attrs(coded, &levels);
                *col = coded;
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, out_cols.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, out_cols.len() as R_xlen_t);
        let _names_guard = protect(names_vec);
        for (j, col) in out_cols.iter().enumerate() {
            SET_VECTOR_ELT(result, j as R_xlen_t, *col);
            let cstr = CString::new(out_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(names_vec, j as R_xlen_t, charsxp);
            }
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);

        let explicit_row_names = !row_names_arg.is_null()
            && row_names_arg != R_NilValue()
            && TYPEOF(row_names_arg) == SEXPTYPE::STRSXP
            && XLENGTH(row_names_arg) == nrow;
        if let Some(names) = taken_row_names {
            let labels = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
            let _labels_guard = protect(labels);
            for (i, name) in names.iter().enumerate() {
                let cstr = CString::new(name.as_str()).unwrap_or_default();
                SET_STRING_ELT(
                    labels,
                    i as R_xlen_t,
                    crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"row.names".as_ptr()), labels);
        } else if explicit_row_names {
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"row.names".as_ptr()),
                row_names_arg,
            );
        } else if let Some(k) = row_names_column
            && k >= 1
            && k <= ncols
        {
            let labels = Rf_allocVector3(SEXPTYPE::STRSXP, nrow);
            let _labels_guard = protect(labels);
            for i in 0..nrow {
                let cstr = CString::new(data[i as usize][k - 1].text.as_str()).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                SET_STRING_ELT(labels, i, charsxp);
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"row.names".as_ptr()), labels);
        } else {
            crate::mainutils::essentials::functional::set_compact_row_names(result, nrow);
        }
        crate::mainutils::essentials::functional::set_data_frame_class(result);
        result
    }
}
