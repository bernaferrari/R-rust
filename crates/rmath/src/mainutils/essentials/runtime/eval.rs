//! `eval`, `substitute`, `quote`, `parse` plus source-parsing helpers.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete R runtime: eval, substitute, quote, parse
// ---------------------------------------------------------------------------

/// R's `local(expr, envir = new.env())` — evaluate `expr` in a fresh child
/// environment and return its value (eval.c `do_local`). The default
/// environment parents to the caller (`_rho`); an explicit ENVSXP `envir`
/// is used as-is (wrapped in a child so assignments stay local).
pub unsafe fn do_local(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let envir_arg = CAR(CDR(args));
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let parent = if envir_arg.is_null() || envir_arg == R_NilValue() {
            _rho
        } else {
            envir_arg
        };
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue());
        if env.is_null() {
            return R_NilValue();
        }
        let _guard = protect(env);
        crate::eval::eval::Rf_eval(expr, env)
    }
}

pub unsafe fn do_eval(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let envir_arg = CAR(CDR(args));
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let envir = if envir_arg.is_null() || envir_arg == R_NilValue() {
            _rho
        } else {
            envir_arg
        };
        // eval.c do_eval(): language/symbol/bytecode values evaluate in
        // `envir`; expression vectors evaluate element-wise returning the
        // last value; any other value is returned unchanged (Rf_eval no
        // longer treats expression vectors as evaluable).
        let bcode = SEXPTYPE::BCODESXP;
        if TYPEOF(expr) == SEXPTYPE::LANGSXP
            || TYPEOF(expr) == SEXPTYPE::SYMSXP
            || TYPEOF(expr) == bcode
        {
            return crate::eval::eval::Rf_eval(expr, envir);
        }
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP {
            let n = XLENGTH(expr);
            let mut result = R_NilValue();
            for i in 0..n {
                let element = VECTOR_ELT(expr, i);
                if element.is_null() || element == R_NilValue() {
                    continue;
                }
                // eval.c's expression loop updates R_Srcref per element
                // (srcref-level show.error.locations: `eval(parse(...))`
                // errors carry `(from <file>#<line>)`).
                crate::mainutils::srcref::set_current_srcref_location(element, expr, i as usize);
                result = crate::eval::eval::Rf_eval(element, envir);
            }
            crate::mainutils::srcref::set_current_srcref_location(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            );
            return result;
        }
        expr
    }
}

/// R's `substitute(expr, env)` — substitute symbols in expression.
pub unsafe fn do_substitute(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::coerce::do_substitute(_call, _op, args, _rho) }
}

/// R's `quote(expr)` — return expression unevaluated.
pub unsafe fn do_quote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, NAMED, SET_NAMED};
        let mut nargs = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            nargs += 1;
            current = CDR(current);
        }
        if nargs != 1 {
            base_error(format!(
                "{nargs} arguments passed to 'quote' which requires 1"
            ));
        }
        let tag = TAG(args);
        if !tag.is_null() && tag != R_NilValue() {
            let name = if TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let printname = PRINTNAME(tag);
                if printname.is_null() {
                    String::new()
                } else {
                    let chars = CHAR(printname);
                    if chars.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(chars).to_string_lossy().into_owned()
                    }
                }
            } else {
                String::new()
            };
            if name != "expr" {
                base_error(format!(
                    "supplied argument name '{name}' does not match 'expr'"
                ));
            }
        }
        let val = CAR(args);
        if val.is_null() || val == R_NilValue() {
            return R_NilValue();
        }
        // ENSURE_NAMEDMAX — prevent modification of source code references
        if NAMED(val) < 2 {
            SET_NAMED(val, 2);
        }
        val
    }
}

/// R's `parse(text)` — parse R code strings into an expression vector.
pub unsafe fn do_parse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let keep_source = {
            let explicit = arg_by_name_or_position(args, &["keep.source"], usize::MAX);
            let opt = if explicit != R_NilValue() {
                explicit
            } else {
                crate::mainutils::options::GetOption1(crate::sexp::symbol::Rf_install(
                    c"keep.source".as_ptr(),
                ))
            };
            !opt.is_null() && crate::mainutils::coerce::asLogical(opt) == 1
        };
        let text_arg = arg_by_name_or_position(args, &["text"], 0);
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        if text_arg.is_null() || text_arg == R_NilValue() {
            if !file_arg.is_null() && file_arg != R_NilValue() {
                let file_path = elt_to_string(file_arg, 0);
                let content = crate::mainutils::browser_files::read_text_or_host(&file_path)
                    .unwrap_or_else(|err| {
                        base_error(format!("cannot open file '{}': {}", file_path, err))
                    });
                if keep_source {
                    return parse_with_srcrefs(&content, &file_path);
                }
                return parse_source_expression_vector(&content);
            }
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let n = XLENGTH(text_arg);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let mut source = Vec::with_capacity(n as usize);
        for i in 0..n {
            if TYPEOF(text_arg) == SEXPTYPE::STRSXP && is_string_na(text_arg, i) {
                std::panic::panic_any(RError {
                    message: "invalid 'text' argument".to_string(),
                });
            }
            let text = elt_to_string(text_arg, i);
            source.push(text);
        }
        let combined = source.join("\n");
        if keep_source {
            // Upstream parse(text=) attributes an unnamed srcfile (the
            // renderer falls back to `(from #n)` for it).
            return parse_with_srcrefs(&combined, "<text>");
        }
        parse_source_strings(&source)
    }
}

/// Parse with byte spans and attach srcrefs + srcfile (keep.source).
pub(crate) unsafe fn parse_with_srcrefs(content: &str, filename: &str) -> SEXP {
    unsafe {
        let spans = crate::sexp::memory::with_arena(|arena| {
            let mut parser = crate::eval::parser::Parser::new(content, arena);
            parser
                .parse_top_level_with_spans()
                .map_err(|e| e.to_string())
        });
        match spans {
            Ok(spans) => {
                let exprs: Vec<SEXP> = spans.iter().map(|&(e, _, _)| e).collect();
                let vec_sexp = crate::sexp::constructors::Rf_allocVector3(
                    SEXPTYPE::EXPRSXP,
                    exprs.len() as i64,
                );
                let _vg = crate::sexp::protect::protect(vec_sexp);
                for (i, &e) in exprs.iter().enumerate() {
                    crate::sexp::accessors::SET_VECTOR_ELT(vec_sexp, i as i64, e);
                }
                crate::mainutils::srcref::attach_srcrefs_with_spans(
                    &spans, content, filename, vec_sexp,
                );
                vec_sexp
            }
            Err(msg) => {
                std::panic::panic_any(RError { message: msg });
            }
        }
    }
}

unsafe fn parse_source_strings(source: &[String]) -> SEXP {
    let combined = source.join("\n");
    unsafe { parse_source_expression_vector(&combined) }
}

pub(crate) unsafe fn parse_source_expression_vector(source: &str) -> SEXP {
    unsafe {
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions_strict(source, arena)
                .map_err(|err| err.to_string())
        })
        .unwrap_or_else(|message| std::panic::panic_any(RError { message }));

        let result = Rf_allocVector3(SEXPTYPE::EXPRSXP, parsed.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in parsed.into_iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, value);
        }
        result
    }
}

unsafe fn d_symbol_name(sym: SEXP) -> String {
    unsafe {
        if TYPEOF(sym) != SEXPTYPE::SYMSXP {
            return String::new();
        }
        let pn = PRINTNAME(sym);
        if pn.is_null() {
            return String::new();
        }
        CStr::from_ptr(CHAR(pn)).to_string_lossy().into_owned()
    }
}

unsafe fn d_numeric(x: SEXP) -> Option<f64> {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) == 1 {
            Some(*REAL(x))
        } else if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) == 1 {
            Some(*INTEGER(x) as f64)
        } else {
            None
        }
    }
}

unsafe fn d_diff(expr: SEXP, var: &str) -> SEXP {
    unsafe {
        if TYPEOF(expr) == SEXPTYPE::SYMSXP {
            return if d_symbol_name(expr) == var {
                Rf_ScalarInteger(1)
            } else {
                Rf_ScalarInteger(0)
            };
        }
        if d_numeric(expr).is_some() {
            return Rf_ScalarInteger(0);
        }
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return Rf_ScalarInteger(0);
        }
        let op = CAR(expr);
        let name = d_symbol_name(op);
        if name == "^" {
            let base = CAR(CDR(expr));
            let exp = CAR(CDR(CDR(expr)));
            if d_symbol_name(base) == var {
                if let Some(n) = d_numeric(exp) {
                    if (n - 1.0).abs() < 1e-15 {
                        return Rf_ScalarInteger(1);
                    }
                    let n_s = Rf_ScalarReal(n);
                    if (n - 2.0).abs() < 1e-15 {
                        return crate::sexp::constructors::Rf_lang3(
                            Rf_install(c"*".as_ptr()),
                            n_s,
                            base,
                        );
                    }
                    let nm1 = Rf_ScalarReal(n - 1.0);
                    let pow =
                        crate::sexp::constructors::Rf_lang3(Rf_install(c"^".as_ptr()), base, nm1);
                    return crate::sexp::constructors::Rf_lang3(
                        Rf_install(c"*".as_ptr()),
                        n_s,
                        pow,
                    );
                }
            }
        }
        if name == "+" || name == "-" {
            let a = d_diff(CAR(CDR(expr)), var);
            let b = d_diff(CAR(CDR(CDR(expr))), var);
            if let Some(bv) = d_numeric(b) {
                if bv == 0.0 {
                    return a;
                }
            }
            if name == "+" {
                if let Some(av) = d_numeric(a) {
                    if av == 0.0 {
                        return b;
                    }
                }
            }
            return crate::sexp::constructors::Rf_lang3(op, a, b);
        }
        if name == "*" {
            let a = CAR(CDR(expr));
            let b = CAR(CDR(CDR(expr)));
            if d_numeric(a).is_some() {
                let db = d_diff(b, var);
                if d_numeric(db) == Some(1.0) {
                    return a;
                }
                if d_numeric(db) == Some(0.0) {
                    return Rf_ScalarInteger(0);
                }
                return crate::sexp::constructors::Rf_lang3(op, a, db);
            }
            if d_numeric(b).is_some() {
                let da = d_diff(a, var);
                if d_numeric(da) == Some(1.0) {
                    return b;
                }
                if d_numeric(da) == Some(0.0) {
                    return Rf_ScalarInteger(0);
                }
                return crate::sexp::constructors::Rf_lang3(op, da, b);
            }
        }
        if name == "sin" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"cos".as_ptr()), arg);
            }
        }
        if name == "exp" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"exp".as_ptr()), arg);
            }
        }
        if name == "log" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    arg,
                );
            }
        }
        if name == "cos" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let s = crate::sexp::constructors::Rf_lang2(Rf_install(c"sin".as_ptr()), arg);
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"-".as_ptr()), s);
            }
        }
        if name == "sqrt" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let pow = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    arg,
                    Rf_ScalarReal(-0.5),
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"*".as_ptr()),
                    Rf_ScalarReal(0.5),
                    pow,
                );
            }
        }
        if name == "tan" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let c = crate::sexp::constructors::Rf_lang2(Rf_install(c"cos".as_ptr()), arg);
                let c2 = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    c,
                    Rf_ScalarReal(2.0),
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    c2,
                );
            }
        }
        if name == "asin" || name == "acos" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let x2 = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    arg,
                    Rf_ScalarReal(2.0),
                );
                let inner = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"-".as_ptr()),
                    Rf_ScalarReal(1.0),
                    x2,
                );
                let s = crate::sexp::constructors::Rf_lang2(Rf_install(c"sqrt".as_ptr()), inner);
                let rec = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    s,
                );
                return if name == "acos" {
                    crate::sexp::constructors::Rf_lang2(Rf_install(c"-".as_ptr()), rec)
                } else {
                    rec
                };
            }
        }
        if name == "sinh" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"cosh".as_ptr()), arg);
            }
        }
        if name == "cosh" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"sinh".as_ptr()), arg);
            }
        }
        if name == "atan" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let x2 = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    arg,
                    Rf_ScalarReal(2.0),
                );
                let den = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"+".as_ptr()),
                    Rf_ScalarReal(1.0),
                    x2,
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    den,
                );
            }
        }
        if name == "tanh" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let c = crate::sexp::constructors::Rf_lang2(Rf_install(c"cosh".as_ptr()), arg);
                let c2 = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    c,
                    Rf_ScalarReal(2.0),
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    c2,
                );
            }
        }
        if name == "gamma" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let g = crate::sexp::constructors::Rf_lang2(Rf_install(c"gamma".as_ptr()), arg);
                let dg = crate::sexp::constructors::Rf_lang2(Rf_install(c"digamma".as_ptr()), arg);
                return crate::sexp::constructors::Rf_lang3(Rf_install(c"*".as_ptr()), g, dg);
            }
        }
        if name == "lgamma" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"digamma".as_ptr()), arg);
            }
        }
        if name == "digamma" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"trigamma".as_ptr()), arg);
            }
        }
        if name == "trigamma" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"psigamma".as_ptr()),
                    arg,
                    Rf_ScalarInteger(2),
                );
            }
        }
        if name == "expm1" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"exp".as_ptr()), arg);
            }
        }
        if name == "log1p" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let den = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"+".as_ptr()),
                    Rf_ScalarReal(1.0),
                    arg,
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    den,
                );
            }
        }
        if TYPEOF(expr) == SEXPTYPE::LANGSXP && !name.is_empty() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                &format!("Function '{name}' is not in the derivatives table"),
            );
        }
        Rf_ScalarInteger(0)
    }
}

/// GNU `D(expr, name)`.
pub unsafe fn do_D(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut expr = CAR(args);
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP && XLENGTH(expr) >= 1 {
            expr = VECTOR_ELT(expr, 0);
        }
        let name_s = CAR(CDR(args));
        let var = if TYPEOF(name_s) == SEXPTYPE::STRSXP {
            CStr::from_ptr(CHAR(STRING_ELT(name_s, 0)))
                .to_string_lossy()
                .into_owned()
        } else {
            d_symbol_name(name_s)
        };
        d_diff(expr, &var)
    }
}

/// GNU `deriv(~expr, name)` as an evaluable expression.
pub unsafe fn do_deriv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut expr = CAR(args);
        if TYPEOF(expr) == SEXPTYPE::LANGSXP && d_symbol_name(CAR(expr)) == "~" {
            expr = CAR(CDR(expr));
        }
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP && XLENGTH(expr) >= 1 {
            expr = VECTOR_ELT(expr, 0);
        }
        let name_s = CAR(CDR(args));
        let var = if TYPEOF(name_s) == SEXPTYPE::STRSXP {
            CStr::from_ptr(CHAR(STRING_ELT(name_s, 0)))
                .to_string_lossy()
                .into_owned()
        } else {
            d_symbol_name(name_s)
        };
        let d = d_diff(expr, &var);
        let _d = protect(d);
        let e_txt = crate::mainutils::deparse::deparse1line(expr, false);
        let _et = protect(e_txt);
        let d_txt = crate::mainutils::deparse::deparse1line(d, false);
        let _dt = protect(d_txt);
        let e_s = CStr::from_ptr(CHAR(STRING_ELT(e_txt, 0))).to_string_lossy();
        let d_s = CStr::from_ptr(CHAR(STRING_ELT(d_txt, 0))).to_string_lossy();
        let src = format!("{{ .value <- {e_s}; attr(.value, \"gradient\") <- {d_s}; .value }}");
        parse_source_expression_vector(&src)
    }
}

/// GNU `deriv3(~expr, name)` with hessian.
pub unsafe fn do_deriv3(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut expr = CAR(args);
        if TYPEOF(expr) == SEXPTYPE::LANGSXP && d_symbol_name(CAR(expr)) == "~" {
            expr = CAR(CDR(expr));
        }
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP && XLENGTH(expr) >= 1 {
            expr = VECTOR_ELT(expr, 0);
        }
        let name_s = CAR(CDR(args));
        let var = if TYPEOF(name_s) == SEXPTYPE::STRSXP {
            CStr::from_ptr(CHAR(STRING_ELT(name_s, 0)))
                .to_string_lossy()
                .into_owned()
        } else {
            d_symbol_name(name_s)
        };
        let d = d_diff(expr, &var);
        let _d = protect(d);
        let h = d_diff(d, &var);
        let _h = protect(h);
        let e_txt = crate::mainutils::deparse::deparse1line(expr, false);
        let _et = protect(e_txt);
        let d_txt = crate::mainutils::deparse::deparse1line(d, false);
        let _dt = protect(d_txt);
        let h_txt = crate::mainutils::deparse::deparse1line(h, false);
        let _ht = protect(h_txt);
        let e_s = CStr::from_ptr(CHAR(STRING_ELT(e_txt, 0))).to_string_lossy();
        let d_s = CStr::from_ptr(CHAR(STRING_ELT(d_txt, 0))).to_string_lossy();
        let h_s = CStr::from_ptr(CHAR(STRING_ELT(h_txt, 0))).to_string_lossy();
        let src = format!(
            "{{ .value <- {e_s}; attr(.value, \"gradient\") <- {d_s}; attr(.value, \"hessian\") <- {h_s}; .value }}"
        );
        parse_source_expression_vector(&src)
    }
}
