use super::*;

// ---------------------------------------------------------------------------
// S3 print dispatch — type-specific print methods
// ---------------------------------------------------------------------------

/// R's `print.integer(x)` — print integer vector with index labels.
pub unsafe fn do_print_integer(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("integer(0)");
            return R_NilValue();
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.numeric(x)` — print numeric (double) vector with index labels.
pub unsafe fn do_print_numeric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("numeric(0)");
            return R_NilValue();
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.logical(x)` — print logical vector with index labels.
pub unsafe fn do_print_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("logical(0)");
            return R_NilValue();
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.character(x)` — print character vector with index labels.
pub unsafe fn do_print_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("character(0)");
            return R_NilValue();
        }
        let n = XLENGTH(x);
        if n == 0 {
            println!("character(0)");
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.complex(x)` — print complex vector with index labels.
pub unsafe fn do_print_complex(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("complex(0)");
            return R_NilValue();
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

pub(crate) fn function_srcref_text(x: SEXP) -> Option<String> {
    unsafe {
        let sr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"srcref".as_ptr()));
        if sr.is_null() || sr == R_NilValue() || TYPEOF(sr) != SEXPTYPE::INTSXP || XLENGTH(sr) < 3 {
            return None;
        }
        let srcfile = crate::sexp::attrib_core::getAttrib(sr, Rf_install(c"srcfile".as_ptr()));
        if srcfile.is_null() || srcfile == R_NilValue() || TYPEOF(srcfile) != SEXPTYPE::ENVSXP {
            return None;
        }
        let lines = crate::sexp::envir::R_findVarInFrame(srcfile, Rf_install(c"lines".as_ptr()));
        if lines.is_null() || lines == R_NilValue() || TYPEOF(lines) != SEXPTYPE::STRSXP {
            return None;
        }
        let first = *INTEGER(sr) as i64;
        let first_col = *INTEGER(sr).add(1) as i64;
        let last = *INTEGER(sr).add(2) as i64;
        let last_col = *INTEGER(sr).add(3) as i64;
        if first < 1 || last < first || last > XLENGTH(lines) {
            return None;
        }
        let mut out = String::new();
        for i in (first - 1)..last {
            let s = STRING_ELT(lines, i);
            let mut text = if !s.is_null() && s != crate::sexp::globals::R_NaString() {
                let p = CHAR(s);
                if p.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(p)
                        .to_str()
                        .unwrap_or("")
                        .to_string()
                }
            } else {
                String::new()
            };
            if i == first - 1 && first_col > 1 {
                let start = (first_col as usize).saturating_sub(1).min(text.len());
                text = text[start..].to_string();
            }
            if i == last - 1
                && last_col > 0
                && (last_col as usize) < text.len()
                && !(first == last && first_col > 1)
            {
                text.truncate(last_col as usize);
            }
            out.push_str(&text);
            out.push('\n');
        }
        Some(out)
    }
}

/// R's `print.function(x)` — GNU PrintClosure: deparse the closure, then
/// the enclosing environment pointer.
pub unsafe fn do_print_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            emit_print_text("NULL\n");
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::CLOSXP && t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
            return do_print(_call, _op, args, _rho);
        }
        if t != SEXPTYPE::CLOSXP {
            emit_print_text("<primitive>\n");
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        // GNU PrintClosure prints the source/deparse body, then the
        // environment line whenever CLOENV(s) != R_GlobalEnv — on both the
        // srcref-source and the deparse path.
        if let Some(mut source) = function_srcref_text(x) {
            append_closure_environment(x, &mut source);
            emit_print_text(&source);
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        let lines = crate::mainutils::deparse::deparse1(
            x,
            false,
            crate::mainutils::deparse::DEFAULTDEPARSE,
        );
        let _lines = crate::sexp::protect::protect(lines);
        let mut rendered = String::new();
        if TYPEOF(lines) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(lines) {
                let s = STRING_ELT(lines, i);
                if !s.is_null() && s != crate::sexp::globals::R_NaString() {
                    let p = CHAR(s);
                    if !p.is_null() {
                        rendered.push_str(std::ffi::CStr::from_ptr(p).to_str().unwrap_or(""));
                    }
                }
                rendered.push('\n');
            }
        }
        append_closure_environment(x, &mut rendered);

        emit_print_text(&rendered);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// Append the closure's environment line, mirroring GNU `PrintClosure`:
/// printed on every path for which `CLOENV(s) != R_GlobalEnv`.
pub(crate) unsafe fn append_closure_environment(x: SEXP, rendered: &mut String) {
    unsafe {
        let env = crate::sexp::accessors::CLOENV(x);
        if !env.is_null() && env != R_NilValue() && env != crate::sexp::globals::R_GlobalEnv() {
            let enc = crate::mainutils::printutils::EncodeEnvironment(env);
            if !enc.is_null() {
                rendered.push_str(std::ffi::CStr::from_ptr(enc).to_str().unwrap_or(""));
                rendered.push('\n');
            }
        }
    }
}

/// R's `print.environment(x)` — print environment summary.
pub unsafe fn do_print_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::ENVSXP {
            return do_print(_call, _op, args, _rho);
        }
        // Print environment name
        let name = if x == crate::sexp::globals::R_GlobalEnv() {
            "R_GlobalEnv".to_string()
        } else if x == crate::sexp::globals::R_EmptyEnv() {
            "R_EmptyEnv".to_string()
        } else if x == crate::sexp::globals::R_BaseEnv() {
            "base".to_string()
        } else {
            "<environment>".to_string()
        };
        println!("<environment: {}>", name);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `enc2native(x)` — normalize character encodings to the native runtime encoding.
pub unsafe fn do_enc2native(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_enc2(args) }
}

/// R's `enc2utf8(x)` — normalize character encodings to UTF-8.
pub unsafe fn do_enc2utf8(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_enc2(args) }
}

unsafe fn do_enc2(args: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            base_error("argument is not a character vector");
        }
        crate::mainutils::duplicate::duplicate(x)
    }
}

/// R's `print.formula(x)` — print formula.
pub unsafe fn do_print_formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        // Formulas are typically LANGSXP with ~ operator
        let t = TYPEOF(x);
        if t == SEXPTYPE::LANGSXP {
            let op = CAR(x);
            if !op.is_null()
                && TYPEOF(op) == SEXPTYPE::SYMSXP
                && std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::PRINTNAME(op),
                ))
                .to_bytes()
                    == b"~"
            {
                // GNU print.formula: print.default(unclass(x)) — the
                // language object is deparsed at the default width cutoff
                // (60), so long formulas wrap across lines.
                let deparsed = crate::mainutils::deparse::deparse_symbolic(x, true);
                let _d = protect(deparsed);
                if !deparsed.is_null() && TYPEOF(deparsed) == SEXPTYPE::STRSXP {
                    for i in 0..XLENGTH(deparsed) {
                        emit_print_text(&format!("{}\n", elt_to_string(deparsed, i)));
                    }
                }
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                return x;
            }
        }
        do_print(_call, _op, args, _rho)
    }
}

/// R's `print.call(x)` — print call/language object.
pub unsafe fn do_print_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        // Print as deparse-like output
        let s = do_deparse(_call, _op, args, _rho);
        if !s.is_null() && TYPEOF(s) == SEXPTYPE::STRSXP {
            let n = XLENGTH(s);
            for i in 0..n {
                println!("{}", elt_to_string(s, i));
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.pairlist(x)` — print pairlist.
pub unsafe fn do_print_pairlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let mut cur = x;
        let mut i = 0;
        while !cur.is_null() && cur != R_NilValue() && TYPEOF(cur) == SEXPTYPE::LISTSXP {
            let tag = crate::sexp::accessors::TAG(cur);
            let val = CAR(cur);
            let name = if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let s = crate::sexp::accessors::CHAR(pname);
                    if !s.is_null() {
                        std::ffi::CStr::from_ptr(s)
                            .to_str()
                            .unwrap_or("")
                            .to_string()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let val_str = elt_to_string(val, 0);
            if name.is_empty() {
                println!("[[{}]]\n{}", i + 1, val_str);
            } else {
                println!("${}\n{}", name, val_str);
            }
            cur = CDR(cur);
            i += 1;
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.raw(x)` — print raw (byte) vector.
pub unsafe fn do_print_raw(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::RAWSXP {
            // Not a raw vector, fall back to default print
            return do_print(_call, _op, args, _rho);
        }
        let n = XLENGTH(x);
        if n == 0 {
            println!("raw(0)");
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        let raw_ptr = RAW(x);
        let mut parts: Vec<String> = Vec::new();
        let display_n = n.min(999);
        for i in 0..display_n {
            let byte = *raw_ptr.add(i as usize);
            parts.push(format!("{:02x}", byte));
        }
        if n > 999 {
            parts.push("...".to_string());
        }
        // Print in R's raw vector style: [1] "00" "ff" "ab" ...
        let mut line = String::from("[1] ");
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            let _ = std::fmt::Write::write_fmt(&mut line, format_args!("\"{}\"", p));
            // Wrap lines roughly every 16 entries for readability
            if (i + 1) % 16 == 0 && i + 1 < parts.len() {
                println!("{}", line);
                line = format!("[{}] ", i + 2);
            }
        }
        if !line.is_empty() {
            println!("{}", line);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}
#[cfg(test)]
mod pairlist_print_tests {
    use super::*;
    use crate::sexp::object::PairlistBuilder;

    #[test]
    fn print_pairlist_accepts_owned_untagged_cells() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().expect("live session owner");
            let value = session
                .sexp(unsafe { Rf_ScalarInteger(42) })
                .expect("owned integer");
            let mut list = PairlistBuilder::new_in(owner);
            list.push(value, None).expect("untagged cell");
            let list = list.finish().expect("owned pairlist");
            assert!(list.try_tag().expect("captured untagged cell").is_nil());

            let mut arguments = PairlistBuilder::new_in(owner);
            arguments.push(list.clone(), None).expect("print argument");
            let arguments = arguments.finish().expect("owned print arguments");
            let result = unsafe {
                do_print_pairlist(R_NilValue(), R_NilValue(), arguments.as_raw(), R_NilValue())
            };
            assert_eq!(result, list.as_raw());
        });
    }
}
