use super::*;

// ---------------------------------------------------------------------------
// Complete I/O — European CSV, delimited, fixed-width
// ---------------------------------------------------------------------------

/// R's `read.csv2(file, ...)` — European CSV reader (semicolons as separator).
pub fn do_read_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let file_path = resolve_package_relative_path(elt_to_string(file_arg, 0));

    // Read file
    let content = match crate::mainutils::browser_files::read_text_or_host(&file_path) {
        Ok(s) => s,
        Err(e) => {
            base_error(format!("cannot read file '{}': {}", file_path, e));
        }
    };

    let mut lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return R_NilValue();
    }

    // Header from first line
    let header_line = lines.remove(0);
    let col_names: Vec<String> = header_line
        .split(';')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect();

    let ncols = col_names.len();
    if ncols == 0 {
        return R_NilValue();
    }

    // Parse data rows — European format uses comma as decimal separator
    let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
    for line in &lines {
        let fields: Vec<&str> = line.split(';').collect();
        for j in 0..ncols {
            let val = if j < fields.len() {
                // Replace comma decimal with dot
                let cleaned = fields[j].trim().replace(',', ".");
                cleaned.parse::<f64>().unwrap_or(NA_REAL)
            } else {
                NA_REAL
            };
            col_data[j].push(val);
        }
    }

    // Build list result (data.frame)
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = protect(result);

    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
    let _p2 = protect(names_vec);

    for j in 0..ncols {
        let nrow = col_data[j].len();
        let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
        if !col.is_null() {
            let mut dst = owner_sexp_mut(col);
            for (i, &v) in col_data[j].iter().enumerate() {
                sexp_ok(dst.try_set_real_elt(i as R_xlen_t, v));
            }
        }
        crate::sexp::accessors::SET_VECTOR_ELT(result, j as R_xlen_t, col);

        let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
        let charsxp = mk_char_cstr(&cstr);
        if !charsxp.is_null() {
            crate::sexp::accessors::SET_STRING_ELT(names_vec, j as R_xlen_t, charsxp);
        }
    }

    // Set names
    crate::sexp::attrib_core::setAttrib(result, install_str("names"), names_vec);
    // Set class to data.frame
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    let _p3 = protect(class_vec);
    let cstr = c"data.frame";
    let charsxp = mk_char_cstr(&cstr);
    if !charsxp.is_null() {
        crate::sexp::accessors::SET_STRING_ELT(class_vec, 0 as R_xlen_t, charsxp);
    }
    crate::sexp::attrib_core::setAttrib(result, install_str("class"), class_vec);
    result
}

/// R's `write.csv2(x, file, ...)` — European CSV writer (semicolons, comma decimal).
pub fn do_write_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let file_arg = CAR(CDR(args));
    let file_path = elt_to_string(file_arg, 0);

    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let ncols = XLENGTH(x) as usize;

    // Get names if available
    let names_attr = crate::sexp::attrib_core::getAttrib(x, install_str("names"));

    let mut out = String::new();
    if ncols == 0 {
        out.push_str("\"\"\n");
    }

    // Header
    if ncols > 0 && !names_attr.is_null() && names_attr != R_NilValue() {
        let mut headers: Vec<String> = Vec::new();
        for j in 0..ncols {
            let nm = elt_to_string(names_attr, j as R_xlen_t);
            headers.push(format!("\"{}\"", nm));
        }
        out.push_str(&headers.join(";"));
        out.push('\n');
    }

    let t = TYPEOF(x);
    // Data rows — dispatch on type exactly like do_write_csv:
    // only VECSXP payloads are column pointers; atomic vectors are
    // formatted as a single column; anything else is an error.
    if ncols > 0 {
        // TYPEOF returns c_int; only a VECSXP payload may be read as
        // column pointers. Atomic vectors format as a single column.
        let nrows = if t == SEXPTYPE::VECSXP {
            let col = crate::sexp::accessors::VECTOR_ELT(x, 0);
            if !col.is_null() {
                XLENGTH(col) as usize
            } else {
                0
            }
        } else if t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::STRSXP
        {
            XLENGTH(x) as usize
        } else {
            std::panic::panic_any(RError {
                message: format!("cannot handle 'x' of type {:?}", t),
            });
        };

        for i in 0..nrows {
            let mut row: Vec<String> = Vec::new();
            for j in 0..ncols {
                let val = if t == SEXPTYPE::VECSXP {
                    let col = crate::sexp::accessors::VECTOR_ELT(x, j as R_xlen_t);
                    if !col.is_null() {
                        elt_to_string(col, i as R_xlen_t)
                    } else {
                        "NA".to_string()
                    }
                } else {
                    elt_to_string(x, i as R_xlen_t)
                };
                // Use comma as decimal separator for European format
                let eu_val = val.replace('.', ",");
                row.push(format!("\"{}\"", eu_val));
            }
            out.push_str(&row.join(";"));
            out.push('\n');
        }
    }

    // Write to file
    if let Err(e) = crate::mainutils::browser_files::write_text_or_host(&file_path, out.as_bytes())
    {
        base_error(format!("cannot write file '{}': {}", file_path, e));
    }

    R_NilValue()
}

/// R's `read.delim(file, ...)` — delimited file reader.
pub fn do_read_delim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let sep_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
        R_NilValue()
    } else {
        CAR(CDR(args))
    };

    let file_path = resolve_package_relative_path(elt_to_string(file_arg, 0));
    let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
        "\t".to_string()
    } else {
        elt_to_string(sep_arg, 0)
    };

    // Read file
    let content = match crate::mainutils::browser_files::read_text_or_host(&file_path) {
        Ok(s) => s,
        Err(e) => {
            base_error(format!("cannot read file '{}': {}", file_path, e));
        }
    };

    let mut lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return R_NilValue();
    }

    // Header
    let header_line = lines.remove(0);
    let col_names: Vec<String> = header_line
        .split(&sep)
        .map(|s| s.trim().to_string())
        .collect();

    let ncols = col_names.len();
    if ncols == 0 {
        return R_NilValue();
    }

    // Parse rows
    let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
    for line in &lines {
        let fields: Vec<&str> = line.split(&sep).collect();
        for j in 0..ncols {
            let val = if j < fields.len() {
                fields[j].trim().parse::<f64>().unwrap_or(NA_REAL)
            } else {
                NA_REAL
            };
            col_data[j].push(val);
        }
    }

    // Build data.frame result
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = protect(result);

    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
    let _p2 = protect(names_vec);

    for j in 0..ncols {
        let nrow = col_data[j].len();
        let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
        if !col.is_null() {
            let mut dst = owner_sexp_mut(col);
            for (i, &v) in col_data[j].iter().enumerate() {
                sexp_ok(dst.try_set_real_elt(i as R_xlen_t, v));
            }
        }
        crate::sexp::accessors::SET_VECTOR_ELT(result, j as R_xlen_t, col);

        let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
        let charsxp = mk_char_cstr(&cstr);
        if !charsxp.is_null() {
            crate::sexp::accessors::SET_STRING_ELT(names_vec, j as R_xlen_t, charsxp);
        }
    }

    crate::sexp::attrib_core::setAttrib(result, install_str("names"), names_vec);
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    let _p3 = protect(class_vec);
    let cstr = c"data.frame";
    let charsxp = mk_char_cstr(&cstr);
    if !charsxp.is_null() {
        crate::sexp::accessors::SET_STRING_ELT(class_vec, 0 as R_xlen_t, charsxp);
    }
    crate::sexp::attrib_core::setAttrib(result, install_str("class"), class_vec);
    result
}

