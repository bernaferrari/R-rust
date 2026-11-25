//! String utilities: startsWith/endsWith, str_pad/count/replace/detect/extract/interp/locate, strwrap, quotes, system.file

use super::*;

pub fn do_format_info(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let m = match_formals(call, args, &["x", "digits", "nsmall"]);
    let missing = crate::sexp::globals::R_MissingArg();
    let force = |v: SEXP| -> SEXP {
        if !v.is_null() && TYPEOF(v) == SEXPTYPE::PROMSXP {
            force_promise(v)
        } else {
            v
        }
    };
    let x = force(*m.first().unwrap_or(&R_NilValue()));
    if x.is_null() || x == R_NilValue() || x == missing {
        return R_NilValue();
    }
    let digits = force(m.get(1).copied().unwrap_or(missing));
    let digits = if digits.is_null() || digits == missing {
        R_NilValue()
    } else {
        digits
    };
    let nsmall = force(m.get(2).copied().unwrap_or(missing));
    let nsmall = if nsmall.is_null() || nsmall == R_NilValue() || nsmall == missing {
        Rf_ScalarInteger(0)
    } else {
        nsmall
    };

    let tail = Rf_cons(nsmall, R_NilValue());
    let _tail_guard = protect(tail);
    let middle = Rf_cons(digits, tail);
    let _middle_guard = protect(middle);
    let normalized_args = Rf_cons(x, middle);
    let _args_guard = protect(normalized_args);
    crate::mainutils::paste_impl::do_formatinfo(call, op, normalized_args, rho)
}

// ---------------------------------------------------------------------------
// String operations: startsWith, endsWith, str_pad, str_count, str_replace
// ---------------------------------------------------------------------------

/// R's `startsWith(x, prefix)` — check if strings start with prefix.
pub fn do_startsWith(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    super::vector_text_entry::evaluate(args, rho, super::vector_text_owned::Operation::Prefix)
}

pub fn do_endsWith(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    super::vector_text_entry::evaluate(args, rho, super::vector_text_owned::Operation::Suffix)
}




// ---------------------------------------------------------------------------
// R runtime type checks: is.language, is.call, is.symbol, is.name,
//   is.pairlist, is.function, is.expression, is.environment
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// Complete string/vector
// ---------------------------------------------------------------------------


/// GNU `strwrap(...)` — dispatch to the verbatim base closure so wrapping
/// (width 0.9*getOption("width"), indent/exdent/prefix/initial) is identical.
pub fn do_strwrap(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let fun = find_var_in_frame(crate::sexp::globals::R_BaseEnv(), install_str("strwrap"));
        let _fun_guard = crate::sexp::protect::protect(fun);
        // Rebuild the call with the already-evaluated arguments and their
        // original tags so the closure's default expressions (width, prefix,
        // initial = prefix) apply exactly as in GNU.
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


fn wrap_text_words(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let current_len = current.chars().count();
        let word_len = word.chars().count();
        let next_len = if current.is_empty() {
            word_len
        } else {
            current_len + 1 + word_len
        };
        if !current.is_empty() && next_len >= width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}


/// R's `system.file(..., package)` — find files inside an installed package.
pub fn do_system_file(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let package_arg = arg_by_name_or_position(args, &["package"], usize::MAX);
    let package = if package_arg.is_null() || package_arg == R_NilValue() {
        "base".to_string()
    } else {
        let n = XLENGTH(package_arg);
        if n != 1 {
            package_error("'package' must be of length 1");
        }
        elt_to_string(package_arg, 0)
    };

    let package_path = find_package_path(&package);
    let must_work = named_logical_arg(args, "mustWork").unwrap_or(false);
    if package_path.is_empty() {
        if must_work {
            package_error(format!("no file found for package '{}'", package));
        }
        return mk_string_cstr(c"");
    }

    let parts = system_file_parts(args);
    let mut path = PathBuf::from(&package_path);
    for part in &parts {
        if !part.is_empty() {
            path.push(part);
        }
    }
    // Installation flattens a package's inst/ subtree into its root.
    // This port loads source-shaped trees directly (the corpus bundles
    // unpacked tarballs), so a missing path falls back to the inst/
    // layout: system.file("fortunes", package = "fortunes") must find
    // inst/fortunes just like an installed tree's fortunes/.
    if !path.exists() && !parts.is_empty() {
        let mut inst_path = PathBuf::from(&package_path).join("inst");
        for part in &parts {
            if !part.is_empty() {
                inst_path.push(part);
            }
        }
        if inst_path.exists() {
            path = inst_path;
        }
    }

    if path.exists() {
        mk_string_cstr(&CString::new(path.to_string_lossy().into_owned()).unwrap_or_default())
    } else {
        if must_work {
            package_error(format!(
                "no file found for requested path in package '{}'",
                package
            ));
        }
        mk_string_cstr(c"")
    }
}

fn system_file_parts(args: SEXP) -> Vec<String> {
    {
        let mut parts = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                let value = CAR(current);
                if !value.is_null() && value != R_NilValue() && TYPEOF(value) == SEXPTYPE::STRSXP {
                    for i in 0..XLENGTH(value) {
                        if !is_string_na(value, i) {
                            parts.push(elt_to_string(value, i));
                        }
                    }
                }
            }
            current = CDR(current);
        }
        parts
    }
}

// ---------------------------------------------------------------------------
// Complete string operations — str_locate, str_sub variants
// ---------------------------------------------------------------------------




