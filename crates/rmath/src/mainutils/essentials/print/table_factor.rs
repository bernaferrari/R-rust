use super::*;

/// R's `print.table(x)` — print a table object.
pub unsafe fn do_print_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = print_method_x(_call, args, "table");
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        // Table objects are typically arrays (REALSXP/INTSXP with dim attribute)
        let t = TYPEOF(x);
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));

        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) == 2 {
            // 2D table: print as matrix
            let nrow = *INTEGER(dim_attr) as usize;
            let ncol = *INTEGER(dim_attr).add(1) as usize;

            // Get dimnames
            let dn = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dimnames".as_ptr()));
            let has_dn = !dn.is_null() && TYPEOF(dn) == SEXPTYPE::VECSXP;

            // Print row names and values
            for i in 0..nrow {
                let rname = if has_dn && !VECTOR_ELT(dn, 0).is_null() {
                    elt_to_string(VECTOR_ELT(dn, 0), i as R_xlen_t)
                } else {
                    format!("{}", i + 1)
                };
                print!("{:>12} ", rname);
                for j in 0..ncol {
                    let idx = i * ncol + j;
                    let val = if t == SEXPTYPE::REALSXP {
                        format!("{:>6}", *REAL(x).add(idx))
                    } else if t == SEXPTYPE::INTSXP {
                        format!("{:>6}", *INTEGER(x).add(idx))
                    } else {
                        format!("{:>6}", elt_to_string(x, idx as R_xlen_t))
                    };
                    print!("{}", val);
                }
                println!();
            }
            // Print column names
            if has_dn && !VECTOR_ELT(dn, 1).is_null() {
                print!("{:>12} ", "");
                for j in 0..ncol {
                    print!("{:>6}", elt_to_string(VECTOR_ELT(dn, 1), j as R_xlen_t));
                }
                println!();
            }
        } else {
            let n = XLENGTH(x);
            let mut names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP {
                let dn = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dimnames".as_ptr()));
                if !dn.is_null() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 1 {
                    names = VECTOR_ELT(dn, 0);
                }
            }
            let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;
            if has_names {
                let labels: Vec<String> = (0..n).map(|i| elt_to_string(names, i)).collect();
                let values: Vec<String> = (0..n)
                    .map(|i| {
                        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            (*INTEGER(x).add(i as usize)).to_string()
                        } else if t == SEXPTYPE::REALSXP {
                            format!("{}", *REAL(x).add(i as usize))
                        } else {
                            elt_to_string(x, i)
                        }
                    })
                    .collect();
                let width = labels
                    .iter()
                    .zip(&values)
                    .map(|(name, value)| name.len().max(value.len()))
                    .max()
                    .unwrap_or(1);
                let name_line = labels
                    .iter()
                    .map(|name| format!("{name:>width$}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let value_line = values
                    .iter()
                    .map(|value| format!("{value:>width$}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!();
                // GNU print.array of a 1-d table: a space after the last
                // column, so both header and count lines gain a trailing space.
                println!("{name_line} ");
                println!("{value_line} ");

                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                return x;
            }
            for i in 0..n {
                let val = elt_to_string(x, i);
                println!("  {}", val);
            }
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// GNU `print.factor` (base factor.R): build the `xx` character vector via
/// `as.character.factor` (keeping every attribute except levels/class), print
/// it through the stock character-vector printer (`quote`, IndexWidth label
/// justification, width wrapping), then the `Levels:` line with encodeString
/// quoting, the smart max.levels default, and ordered ` < ` separators.
pub unsafe fn do_print_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = print_method_x(_call, args, "factor");
        if x.is_null() || x == R_NilValue() {
            emit_print_text("NULL\n");
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let levels = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"levels".as_ptr()));
        let has_levels = !levels.is_null() && TYPEOF(levels) == SEXPTYPE::STRSXP;

        // print.factor(x, quote = FALSE, max.levels = NULL, width)
        let mut quote = false;
        let mut max_levels_num: Option<i64> = None;
        let mut max_levels_on = true;
        let mut width_opt: Option<i64> = None;
        let quote_sym = Rf_install(c"quote".as_ptr());
        let max_sym = Rf_install(c"max.levels".as_ptr());
        let width_sym = Rf_install(c"width".as_ptr());
        let mut p = CDR(args);
        while !p.is_null() && p != R_NilValue() {
            let tag = TAG(p);
            let v = CAR(p);
            if tag == quote_sym && !v.is_null() && v != R_NilValue() {
                quote = crate::mainutils::seq::asLogical(v) != 0;
            } else if tag == max_sym {
                if v.is_null() || v == R_NilValue() {
                    max_levels_on = true;
                } else if TYPEOF(v) == SEXPTYPE::LGLSXP {
                    let l = if XLENGTH(v) > 0 && !LOGICAL(v).is_null() {
                        *LOGICAL(v)
                    } else {
                        0
                    };
                    if l == 0 {
                        max_levels_on = false;
                    }
                } else {
                    let num = crate::mainutils::seq::asInteger(v);
                    if num <= 0 {
                        max_levels_on = false;
                    } else {
                        max_levels_num = Some(num as i64);
                    }
                }
            } else if tag == width_sym && !v.is_null() && v != R_NilValue() {
                if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                    width_opt = Some(*INTEGER(v) as i64);
                } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                    width_opt = Some(*REAL(v) as i64);
                }
            }
            p = CDR(p);
        }

        let ord = crate::mainutils::essentials::sexp_has_class(x, "ordered");
        if n == 0 {
            emit_print_text(if ord { "ordered()\n" } else { "factor()\n" });
        } else {
            // xx <- character(length(x)); xx[] <- as.character(x)
            let xx = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _xx_guard = protect(xx);
            let t = TYPEOF(x);
            for i in 0..n {
                let ch = if (t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP) && has_levels {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER {
                        crate::sexp::globals::R_NaString()
                    } else if v > 0 && (v as R_xlen_t) <= XLENGTH(levels) {
                        STRING_ELT(levels, (v - 1) as R_xlen_t)
                    } else {
                        // Malformed code: GNU errors; keep the lenient raw
                        // code rendering so diagnostics stay printable.
                        let text = CString::new(format!("{v}")).unwrap_or_default();
                        Rf_mkChar(text.as_ptr())
                    }
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(i as usize);
                    let text = CString::new(format!("{v}")).unwrap_or_default();
                    Rf_mkChar(text.as_ptr())
                } else {
                    let text = CString::new(elt_to_string(x, i)).unwrap_or_default();
                    Rf_mkChar(text.as_ptr())
                };
                SET_STRING_ELT(xx, i, ch);
            }
            // keepAttrs <- setdiff(names(attributes(x)), c("levels", "class"))
            let mut attr = crate::sexp::accessors::ATTRIB(x);
            while !attr.is_null() && attr != R_NilValue() {
                let tag = TAG(attr);
                if !tag.is_null() && tag != R_NilValue() {
                    let name = crate::sexp::accessors::PRINTNAME(tag);
                    if !name.is_null() && name != R_NilValue() && TYPEOF(name) == SEXPTYPE::CHARSXP
                    {
                        let bytes =
                            std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name)).to_bytes();
                        if bytes != b"levels" && bytes != b"class" {
                            crate::sexp::attrib_core::Rf_setAttrib(xx, tag, CAR(attr));
                        }
                    }
                }
                attr = CDR(attr);
            }
            if let Some(sexp) = crate::sexp::object::Sexp::from_raw(xx) {
                let text = crate::sexp::output::format_vector_stock(sexp, quote);
                emit_print_text(&format!("{text}\n"));
            }
        }

        // Levels line: GNU cat(if(drop) paste(format(n), ""), "Levels: ", ...)
        if max_levels_on {
            let lev: Vec<String> = if has_levels {
                let nl = XLENGTH(levels);
                (0..nl)
                    .map(|i| {
                        let charsxp = STRING_ELT(levels, i);
                        if charsxp == crate::sexp::globals::R_NaString() && quote {
                            // paste() coerces the kept NA to "NA"
                            "NA".to_string()
                        } else {
                            let enc = crate::mainutils::printutils::EncodeString(
                                charsxp,
                                0,
                                if quote { b'"' as c_int } else { 0 },
                                crate::mainutils::printutils::Rprt_adj::left,
                            );
                            std::ffi::CStr::from_ptr(enc).to_string_lossy().into_owned()
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let nl = lev.len() as i64;
            let colsep = if ord { " < " } else { " " };
            let width = width_opt.unwrap_or(crate::mainutils::options::GetOptionWidth() as i64);
            // width - (nchar("Levels: ") + 3 + 1 + 3)
            let avail = width - 15;
            let maxl = match max_levels_num {
                Some(given) => given,
                None => {
                    let mut chosen = nl;
                    if nl > 1 {
                        let mut cum = 0i64;
                        for (k, l) in lev.iter().enumerate() {
                            cum += l.chars().count() as i64 + colsep.chars().count() as i64;
                            if cum > avail {
                                chosen = 1.max(k as i64);
                                break;
                            }
                        }
                    }
                    chosen
                }
            };
            let drop = nl > maxl;
            let mut body: Vec<&str> = Vec::new();
            if drop {
                let head = 1.max(maxl - 1) as usize;
                for l in lev.iter().take(head) {
                    body.push(l.as_str());
                }
                body.push("...");
                if maxl > 1 {
                    if let Some(last) = lev.last() {
                        body.push(last.as_str());
                    }
                }
            } else {
                for l in lev.iter() {
                    body.push(l.as_str());
                }
            }
            let mut line = String::new();
            if drop {
                line.push_str(&format!("{nl} "));
            }
            line.push_str("Levels: ");
            line.push_str(&body.join(colsep));
            line.push('\n');
            emit_print_text(&line);
        }
        if let Some(msg) = factor_validity_message(x) {
            let cmsg = std::ffi::CString::new(msg).unwrap_or_default();
            crate::mainutils::errors::Rf_warning1(cmsg.as_ptr());
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}
pub(crate) unsafe fn factor_validity_message(x: SEXP) -> Option<String> {
    unsafe {
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return None;
        }
        let mut is_factor = false;
        for i in 0..XLENGTH(class) {
            let name = elt_to_string(class, i);
            if name == "factor" || name == "ordered" {
                is_factor = true;
                break;
            }
        }
        if !is_factor {
            return None;
        }
        let levels = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"levels".as_ptr()));
        if levels.is_null() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return Some("factor levels must be \"character\"".to_string());
        }
        let n = XLENGTH(levels);
        // GNU .valid.factor: anyDuplicated(levs) — NA equals only another NA,
        // never the literal "NA" string.
        for i in 0..n {
            let here = STRING_ELT(levels, i);
            let here_is_na = here == crate::sexp::globals::R_NaString();
            for j in 0..i {
                let prev = STRING_ELT(levels, j);
                let prev_is_na = prev == crate::sexp::globals::R_NaString();
                let same = if here_is_na || prev_is_na {
                    here_is_na && prev_is_na
                } else {
                    crate::sexp::accessors::CHAR(prev) == crate::sexp::accessors::CHAR(here)
                };
                if same {
                    return Some(format!("duplicated level [{}] in factor", i + 1));
                }
            }
        }
        None
    }
}

/// GNU `print.rle(x, digits = getOption("digits"), prefix = "", ...)` (base
/// rle.R): "Run Length Encoding" header, then `str()` of `$lengths` (default
/// digits.d) and `$values` (digits.d = `digits`), joined onto the cat() text.
pub unsafe fn do_print_rle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = print_method_x(_call, args, "rle");
        if x.is_null()
            || x == R_NilValue()
            || TYPEOF(x) != SEXPTYPE::VECSXP
            || !crate::mainutils::essentials::sexp_has_class(x, "rle")
        {
            return do_print_default(_call, _op, args, _rho);
        }
        let component = |name: &str| -> SEXP {
            let names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
                return R_NilValue();
            }
            for i in 0..XLENGTH(names) {
                if elt_to_string(names, i) == name {
                    return VECTOR_ELT(x, i);
                }
            }
            R_NilValue()
        };

        let lengths = component("lengths");
        let _l_guard = protect(lengths);
        let values = component("values");
        let _v_guard = protect(values);

        // str(x$lengths, ...) with strOptions() default digits.d
        emit_print_text("Run Length Encoding\n  lengths:");
        let lengths_args = Rf_cons(lengths, R_NilValue());
        let _la_guard = protect(lengths_args);
        let _g1 = StrDigitsGuard::new(None);
        do_str(R_NilValue(), R_NilValue(), lengths_args, _rho);
        drop(_g1);

        // str(x$values, digits.d = getOption("digits"), ...)
        emit_print_text("  values :");
        let digits = crate::mainutils::format::format_get_R_print().digits;
        let digits_arg = Rf_ScalarInteger(digits);
        let _d_guard = protect(digits_arg);
        let dd_tail = Rf_cons(digits_arg, R_NilValue());
        let _dd_guard = protect(dd_tail);
        SETTAG(dd_tail, Rf_install(c"digits.d".as_ptr()));
        let values_args = Rf_cons(values, dd_tail);
        let _va_guard = protect(values_args);
        do_str(R_NilValue(), R_NilValue(), values_args, _rho);

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// GNU `print.eigen(x, ...)`: header line plus `print(unclass(x), ...)`;
/// returns invisible x (base eigen.R).
pub unsafe fn do_print_eigen(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = print_method_x(_call, args, "eigen");
        if x.is_null()
            || x == R_NilValue()
            || TYPEOF(x) != SEXPTYPE::VECSXP
            || !crate::mainutils::essentials::sexp_has_class(x, "eigen")
        {
            return do_print_default(_call, _op, args, _rho);
        }
        emit_print_text("eigen() decomposition\n");
        let unclassed = crate::mainutils::duplicate::Rf_duplicate(x);
        let _u_guard = protect(unclassed);
        crate::sexp::attrib_core::setAttrib(
            unclassed,
            crate::sexp::attrib_core::R_ClassSymbol(),
            R_NilValue(),
        );
        let inner_args = Rf_cons(unclassed, CDR(args));
        let _a_guard = protect(inner_args);
        do_print_default(R_NilValue(), R_NilValue(), inner_args, _rho);

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// GNU `print.proc_time(x, ...)`: `print(summary(x, ...))`; invisible x.
/// `summary.proc_time` adds the child times into user/sys, keeps the first
/// three entries and names them user/system/elapsed (base time.R).
pub unsafe fn do_print_proc_time(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::REALSXP {
            return x;
        }
        let src = REAL(x);
        let n = XLENGTH(x).min(5) as usize;
        let at = |i: usize| -> f64 { if i < n { *src.add(i) } else { f64::NAN } };
        let mut vals = [at(0), at(1), at(2)];
        let v4 = at(3);
        if !v4.is_nan() {
            vals[0] += v4;
        }
        let v5 = at(4);
        if !v5.is_nan() {
            vals[1] += v5;
        }

        let out = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        let _out_guard = protect(out);
        for (i, v) in vals.iter().enumerate() {
            *REAL(out).add(i) = *v;
        }
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        let _names_guard = protect(names);
        for (i, name) in ["user", "system", "elapsed"].iter().enumerate() {
            let cstr = std::ffi::CString::new(*name).unwrap_or_default();
            SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(out, crate::sexp::attrib_core::R_NamesSymbol(), names);

        let print_args = Rf_cons(out, R_NilValue());
        let _pa_guard = protect(print_args);
        do_print_default(R_NilValue(), R_NilValue(), print_args, _rho);
        x
    }
}
