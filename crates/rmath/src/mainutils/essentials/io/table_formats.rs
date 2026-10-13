use super::*;

// ---------------------------------------------------------------------------
// Complete I/O — European CSV, delimited, fixed-width
// ---------------------------------------------------------------------------

/// R's `read.csv2(file, ...)` — European CSV reader (semicolons as separator).
pub unsafe fn do_read_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            crate::sexp::accessors::SET_VECTOR_ELT(result, j as R_xlen_t, col);

            let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(names_vec, j as R_xlen_t, charsxp);
            }
        }

        // Set names
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);
        // Set class to data.frame
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = c"data.frame";
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            crate::sexp::accessors::SET_STRING_ELT(class_vec, 0 as R_xlen_t, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class_vec);
        result
    }
}

/// R's `write.csv2(x, file, ...)` — European CSV writer (semicolons, comma decimal).
pub unsafe fn do_write_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = CAR(CDR(args));
        let file_path = elt_to_string(file_arg, 0);

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let ncols = XLENGTH(x) as usize;

        // Get names if available
        let names_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));

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
        if let Err(e) =
            crate::mainutils::browser_files::write_text_or_host(&file_path, out.as_bytes())
        {
            base_error(format!("cannot write file '{}': {}", file_path, e));
        }

        R_NilValue()
    }
}

/// R's `read.delim(file, ...)` — delimited file reader.
pub unsafe fn do_read_delim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            crate::sexp::accessors::SET_VECTOR_ELT(result, j as R_xlen_t, col);

            let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(names_vec, j as R_xlen_t, charsxp);
            }
        }

        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = c"data.frame";
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            crate::sexp::accessors::SET_STRING_ELT(class_vec, 0 as R_xlen_t, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class_vec);
        result
    }
}

/// R's `read.fwf(file, widths, ...)` — fixed-width file reader.
pub unsafe fn do_read_fwf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let widths_arg = CAR(CDR(args));

        let file_path = resolve_package_relative_path(elt_to_string(file_arg, 0));

        let nfields = XLENGTH(widths_arg);
        if nfields == 0 {
            // Upstream read.fwf computes its block widths via
            // rep_len(sep, length(first) - 1L); stock R attributes this
            // error to that call.
            let sym = |name: &str| {
                crate::sexp::symbol::Rf_install(
                    std::ffi::CString::new(name).unwrap_or_default().as_ptr(),
                )
            };
            let one_l = crate::sexp::constructors::Rf_ScalarInteger(1);
            let length_first = crate::sexp::constructors::Rf_lang2(sym("length"), sym("first"));
            let minus = crate::sexp::constructors::Rf_lang3(sym("-"), length_first, one_l);
            let _guards = protect(one_l);
            let _guard2 = protect(length_first);
            let _guard3 = protect(minus);
            let nil = R_NilValue();
            let tail = crate::sexp::constructors::Rf_cons(minus, nil);
            let _guard4 = protect(tail);
            let args2 = crate::sexp::constructors::Rf_cons(sym("sep"), tail);
            let _guard5 = protect(args2);
            let rep_len_call = crate::sexp::constructors::Rf_cons(sym("rep_len"), args2);
            if !rep_len_call.is_null() {
                (*rep_len_call)
                    .sxpinfo
                    .set_type(crate::sexp::ffi::SEXPTYPE::LANGSXP);
            }
            crate::mainutils::errors::errorcall_str(rep_len_call, "invalid 'length.out' value");
        }
        let mut widths: Vec<i64> = Vec::new();
        for i in 0..nfields {
            let w = if TYPEOF(widths_arg) == SEXPTYPE::REALSXP {
                let rp = REAL(widths_arg);
                *rp.add(i as usize) as i64
            } else if TYPEOF(widths_arg) == SEXPTYPE::INTSXP {
                let ip = INTEGER(widths_arg);
                *ip.add(i as usize) as i64
            } else {
                1_i64
            };
            widths.push(w);
        }
        let mut header = false;
        let mut cell = args;
        let mut arg_i = 0i32;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let named = !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP;
            let name = if named {
                CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "header" || (name.is_empty() && arg_i == 2) {
                let v = CAR(cell);
                header = crate::main::coerce::asLogical(v) == TRUE;
            }
            arg_i += 1;
            cell = CDR(cell);
        }

        // Read file
        let content = match crate::mainutils::browser_files::read_text_or_host(&file_path) {
            Ok(s) => s,
            Err(e) => {
                base_error(format!("cannot read file '{}': {}", file_path, e));
            }
        };

        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        let header_names = if header && !lines.is_empty() {
            lines[0]
                .split_whitespace()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let data_lines: Vec<&str> = if header {
            lines.iter().skip(1).copied().collect()
        } else {
            lines
        };
        let ncols = widths.iter().filter(|&&width| width >= 0).count();
        let nrows = data_lines.len();
        let mut col_text: Vec<Vec<String>> = vec![vec![String::new(); nrows]; ncols];
        let mut col_num: Vec<Vec<f64>> = vec![vec![NA_REAL; nrows]; ncols];
        let mut col_numeric = vec![true; ncols];
        for (i, line) in data_lines.iter().enumerate() {
            let mut pos = 0usize;
            let mut out_col = 0usize;
            let bytes = line.as_bytes();
            for &width in &widths {
                let span = width.unsigned_abs() as usize;
                if width < 0 {
                    pos = pos.saturating_add(span);
                    continue;
                }
                let end = pos.saturating_add(span).min(bytes.len());
                let field = if pos < bytes.len() {
                    String::from_utf8_lossy(&bytes[pos..end]).into_owned()
                } else {
                    String::new()
                };
                let trimmed = field.trim();
                if trimmed.is_empty() {
                    col_num[out_col][i] = NA_REAL;
                } else if let Ok(v) = trimmed.parse::<f64>() {
                    col_num[out_col][i] = v;
                } else {
                    col_numeric[out_col] = false;
                }
                col_text[out_col][i] = field;
                pos = pos.saturating_add(span);
                out_col += 1;
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let col = if col_numeric[j] {
                let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrows as R_xlen_t);
                if !col.is_null() {
                    let dst = REAL(col);
                    for (i, &v) in col_num[j].iter().enumerate() {
                        *dst.add(i) = v;
                    }
                }
                col
            } else {
                let col = Rf_allocVector3(SEXPTYPE::STRSXP, nrows as R_xlen_t);
                if !col.is_null() {
                    for (i, text) in col_text[j].iter().enumerate() {
                        let cstr = CString::new(text.as_str()).unwrap_or_default();
                        SET_STRING_ELT(col, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    }
                }
                col
            };
            SET_VECTOR_ELT(result, j as R_xlen_t, col);
            let label = header_names
                .get(j)
                .cloned()
                .unwrap_or_else(|| format!("V{}", j + 1));
            let cstr = CString::new(label).unwrap_or_default();
            SET_STRING_ELT(names_vec, j as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = c"data.frame";
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            crate::sexp::accessors::SET_STRING_ELT(class_vec, 0 as R_xlen_t, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class_vec);
        result
    }
}
