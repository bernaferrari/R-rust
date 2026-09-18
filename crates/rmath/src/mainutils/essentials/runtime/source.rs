//! `source`, `sys.source`, `demo`, `example`.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

use super::eval::parse_source_expression_vector;

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
// Complete R runtime — source, sys.source, demo, example
// ---------------------------------------------------------------------------

/// GNU `source(file, local, echo, print.eval, exprs, ...)`.
/// `exprs=` (and the `expr=` partial match used by eval-etc.R) evaluates
/// already-parsed expressions with optional echo/auto-print.
pub unsafe fn do_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let parsed = source_call_args(args);
        if let Some(exprs) = parsed.exprs {
            let env = source_eval_env(parsed.local, rho, false);
            return eval_source_expressions(
                exprs,
                env,
                parsed.echo,
                parsed.print_eval.unwrap_or(parsed.echo),
                &parsed.prompt,
                &parsed.continue_echo,
                parsed.cutoff,
                parsed.deparse_opts,
                parsed.max_deparse_length,
            );

        }
        let file_arg = parsed.file.unwrap_or(R_NilValue());
        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("source: no file specified");
            return R_NilValue();
        }
        let file_path = elt_to_string(file_arg, 0);
        let env = source_eval_env(parsed.local, rho, false);
        match crate::mainutils::browser_files::read_text_or_host(&file_path) {
            Ok(content) => eval_source_text_with_options(
                &content,
                env,
                &file_path,
                parsed.echo,
                parsed.print_eval.unwrap_or(parsed.echo),
                &parsed.prompt,
                &parsed.continue_echo,
                parsed.skip_echo,
                parsed.keep_source,
                parsed.cutoff,
                parsed.deparse_opts,
                parsed.max_deparse_length,
            ),


            Err(e) => {
                base_error(format!("cannot open file '{}': {}", file_path, e));
            }
        }
    }
}


/// GNU `withAutoprint(exprs)` — `source(exprs=..., echo=TRUE, print.eval=TRUE)`
/// in the calling environment. The argument is substituted, not evaluated twice.
pub unsafe fn do_with_autoprint(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let (expr, print_eval, echo) = with_autoprint_args(args);
        if expr.is_null() || expr == R_NilValue() || expr == R_MissingArg() {
            crate::sexp::globals::set_R_Visible(FALSE);
            return R_NilValue();
        }
        let exprs = brace_or_single_expressions(expr);
        let _exprs = protect(exprs);
        let prompt = option_prompt();
        eval_source_expressions(
            exprs,
            rho,
            echo,
            print_eval,
            &prompt,
            "+ ",
            crate::mainutils::deparse::DEFAULT_CUTOFF,
            crate::mainutils::deparse::KEEPNA
                | crate::mainutils::deparse::KEEPINTEGER
                | crate::mainutils::deparse::SHOWATTRIBUTES,
            usize::MAX,
        )

    }
}


struct SourceCallArgs {
    file: Option<SEXP>,
    exprs: Option<SEXP>,
    local: Option<SEXP>,
    echo: bool,
    print_eval: Option<bool>,
    prompt: String,
    continue_echo: String,
    skip_echo: c_int,
    keep_source: bool,
    cutoff: c_int,
    deparse_opts: c_int,
    /// GNU `source(max.deparse.length=)` default 150.
    max_deparse_length: usize,
}



fn source_call_args(args: SEXP) -> SourceCallArgs {
    unsafe {
        let mut file = None;
        let mut exprs = None;
        let mut local = None;
        let mut echo = false;
        let mut print_eval = None;
        let mut prompt = None;
        let mut continue_echo = None;
        let mut skip_echo: c_int = 0;
        let mut keep_source = None;
        let mut cutoff = crate::mainutils::deparse::DEFAULT_CUTOFF;
        let mut max_deparse_length: usize = 150;
        let mut first_positional = None;
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some("file") => file = Some(value),
                Some(name) if name == "exprs" || name.starts_with("expr") => exprs = Some(value),
                Some("local") => local = Some(value),
                Some("echo") => echo = logical_arg(value, false),
                Some("print.eval") | Some("print.") => print_eval = Some(logical_arg(value, true)),
                Some("prompt.echo") => prompt = Some(elt_to_string(value, 0)),
                Some("continue.echo") => continue_echo = Some(elt_to_string(value, 0)),
                Some("skip.echo") => {
                    let n = crate::mainutils::coerce::asInteger(value);
                    if n != NA_INTEGER && n > 0 {
                        skip_echo = n;
                    }
                }
                Some("keep.source") => keep_source = Some(logical_arg(value, false)),
                Some("width.cutoff") => {
                    let n = crate::mainutils::coerce::asInteger(value);
                    if n != NA_INTEGER
                        && n >= crate::mainutils::deparse::MIN_CUTOFF
                        && n <= crate::mainutils::deparse::MAX_CUTOFF
                    {
                        cutoff = n;
                    }
                }
                Some("max.deparse.length") => {
                    let n = crate::mainutils::coerce::asInteger(value);
                    if n != NA_INTEGER && n >= 0 {
                        max_deparse_length = n as usize;
                    }
                }
                Some(_) => {}
                None => {
                    if positional == 0 {
                        first_positional = Some(value);
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }
        if exprs.is_none() {
            file = file.or(first_positional);
        }
        SourceCallArgs {
            file,
            exprs,
            local,
            echo,
            print_eval,
            prompt: prompt.unwrap_or_else(option_prompt),
            continue_echo: continue_echo.unwrap_or_else(option_continue),
            skip_echo,
            keep_source: keep_source.unwrap_or_else(option_keep_source),
            cutoff,
            deparse_opts: crate::mainutils::deparse::SHOWATTRIBUTES,
            max_deparse_length,
        }
    }
}



fn with_autoprint_args(args: SEXP) -> (SEXP, bool, bool) {
    unsafe {
        let mut expr = R_MissingArg();
        let mut print_eval = true;
        let mut echo = true;
        let mut saw_exprs = false;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some("exprs") => {
                    expr = value;
                    saw_exprs = true;
                }
                Some("evaluated") => {}
                Some("print.") | Some("print.eval") => print_eval = logical_arg(value, true),
                Some("echo") => echo = logical_arg(value, true),
                Some(_) => {}
                None => {
                    if !saw_exprs {
                        expr = value;
                        saw_exprs = true;
                    }
                }
            }
            current = CDR(current);
        }
        (expr, print_eval, echo)
    }
}

fn option_prompt() -> String {
    option_string("prompt", "> ")
}

fn option_continue() -> String {
    option_string("continue", "+ ")
}

fn option_keep_source() -> bool {
    unsafe {
        let opt = crate::mainutils::options::GetOption1(Rf_install(c"keep.source".as_ptr()));
        !opt.is_null() && crate::mainutils::coerce::asLogical(opt) == 1
    }
}


fn option_string(name: &str, default: &str) -> String {
    unsafe {
        let cname = CString::new(name).unwrap_or_default();
        let opt = crate::mainutils::options::GetOption1(Rf_install(cname.as_ptr()));
        if opt.is_null() || opt == R_NilValue() || TYPEOF(opt) != SEXPTYPE::STRSXP {
            default.to_string()
        } else {
            let s = elt_to_string(opt, 0);
            if s.is_empty() {
                default.to_string()
            } else {
                s
            }
        }
    }
}

fn source_eval_env(local: Option<SEXP>, rho: SEXP, default_caller: bool) -> SEXP {
    unsafe {
        match local {
            Some(value) if TYPEOF(value) == SEXPTYPE::ENVSXP => value,
            Some(value) if logical_arg(value, false) => rho,
            Some(_) => crate::sexp::globals::R_GlobalEnv(),
            None if default_caller => rho,
            None => crate::sexp::globals::R_GlobalEnv(),
        }
    }
}

fn logical_arg(value: SEXP, default: bool) -> bool {
    unsafe {
        if value.is_null() || value == R_NilValue() || value == R_MissingArg() {
            return default;
        }
        crate::mainutils::coerce::asLogical(value) != 0
    }
}

unsafe fn brace_or_single_expressions(expr: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP {
            return expr;
        }
        if TYPEOF(expr) == SEXPTYPE::VECSXP {
            let n = XLENGTH(expr);
            let out = Rf_allocVector3(SEXPTYPE::EXPRSXP, n);
            let _out = protect(out);
            for i in 0..n {
                SET_VECTOR_ELT(out, i, VECTOR_ELT(expr, i));
            }
            return out;
        }
        if TYPEOF(expr) == SEXPTYPE::LANGSXP
            && CAR(expr) == crate::sexp::symbol::R_BraceSymbol()
        {

            let mut n = 0;
            let mut cell = CDR(expr);
            while !cell.is_null() && cell != R_NilValue() {
                n += 1;
                cell = CDR(cell);
            }
            let out = Rf_allocVector3(SEXPTYPE::EXPRSXP, n);
            let _out = protect(out);
            cell = CDR(expr);
            let mut i = 0;
            while !cell.is_null() && cell != R_NilValue() {
                SET_VECTOR_ELT(out, i, CAR(cell));
                i += 1;
                cell = CDR(cell);
            }
            return out;
        }
        let out = Rf_allocVector3(SEXPTYPE::EXPRSXP, 1);
        let _out = protect(out);
        SET_VECTOR_ELT(out, 0, expr);
        out
    }
}

unsafe fn eval_source_expressions(
    exprs: SEXP,
    env: SEXP,
    echo: bool,
    print_eval: bool,
    prompt: &str,
    continue_echo: &str,
    cutoff: c_int,
    deparse_opts: c_int,
    max_deparse_length: usize,
) -> SEXP {
    unsafe {
        let n = if exprs.is_null() || exprs == R_NilValue() {
            0
        } else {
            XLENGTH(exprs)
        };
        let mut last_value = R_NilValue();
        let mut last_visible = FALSE;
        for i in 0..n {
            let expr = VECTOR_ELT(exprs, i);
            if echo {
                echo_source_expression(
                    expr,
                    prompt,
                    continue_echo,
                    cutoff,
                    deparse_opts,
                    max_deparse_length,
                );
            }
            last_value = crate::eval::eval::Rf_eval(expr, env);
            last_visible = crate::sexp::globals::R_Visible();
            if print_eval && last_visible != FALSE {
                let print_args = Rf_cons(last_value, R_NilValue());
                let _print_args = protect(print_args);
                crate::mainutils::essentials_basic::do_print(
                    R_NilValue(),
                    R_NilValue(),
                    print_args,
                    env,
                );
            }
        }
        with_visible_result(last_value, last_visible)
    }
}

/// GNU `source()` echo when there is no srcref: deparse the length-1
/// `expression(ei)`, drop the `expression(` prefix (substr from 12),
/// prompt-prefix each line, then `nchar(dep,"c")-1` / `max.deparse.length`.
unsafe fn echo_source_expression(
    expr: SEXP,
    prompt: &str,
    continue_echo: &str,
    cutoff: c_int,
    deparse_opts: c_int,
    max_deparse_length: usize,
) {
    unsafe {
        let wrapped = if TYPEOF(expr) == SEXPTYPE::EXPRSXP {
            expr
        } else {
            let one = Rf_allocVector3(SEXPTYPE::EXPRSXP, 1);
            let _one = protect(one);
            SET_VECTOR_ELT(one, 0, expr);
            one
        };
        let dumped = crate::mainutils::deparse::deparse1WithCutoff(
            wrapped,
            false,
            cutoff,
            true,
            deparse_opts,
            -1,
        );
        let _dumped = protect(dumped);
        let mut text = String::new();
        if !dumped.is_null() && dumped != R_NilValue() && TYPEOF(dumped) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(dumped) {
                if i > 0 {
                    text.push('\n');
                }
                text.push_str(&elt_to_string(dumped, i));
            }
        }
        // GNU: substr(paste(deparse(ei), collapse="\n"), 12L, 1e6)
        const EXPR_PREFIX: &str = "expression(";
        if text.starts_with(EXPR_PREFIX) {
            text = text[EXPR_PREFIX.len()..].to_string();
        }
        let mut dep = String::new();
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                dep.push('\n');
            }
            dep.push_str(if i == 0 { prompt } else { continue_echo });
            dep.push_str(line);
        }
        // GNU: nd <- nchar(dep, "c") - 1L; substr to nd unless truncated.
        let nd = dep.chars().count().saturating_sub(1);
        let truncated = nd > max_deparse_length;
        let keep = if truncated { max_deparse_length } else { nd };
        let trimmed: String = dep.chars().take(keep).collect();
        let mut line = trimmed;
        if truncated {
            line.push_str(" .... [TRUNCATED] ");
        }
        line.push('\n');

        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&line);
        } else {
            print!("{line}");
        }
    }
}




unsafe fn with_visible_result(value: SEXP, visible: i32) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _result = protect(result);
        SET_VECTOR_ELT(result, 0, value);
        SET_VECTOR_ELT(result, 1, Rf_ScalarLogical(visible));
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _names = protect(names);
        SET_STRING_ELT(names, 0, Rf_mkChar(c"value".as_ptr()));
        SET_STRING_ELT(names, 1, Rf_mkChar(c"visible".as_ptr()));
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}


/// R's `sys.source(file, envir, ...)` — source an R file into a specific environment.
pub unsafe fn do_sys_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let envir_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("sys.source: no file specified");
            return R_NilValue();
        }
        let file_path = elt_to_string(file_arg, 0);
        let target_env = if !envir_arg.is_null() && envir_arg != R_NilValue() {
            envir_arg
        } else {
            rho
        };

        match crate::mainutils::browser_files::read_text_or_host(&file_path) {
            Ok(content) => eval_source_text_with_name(&content, target_env, &file_path),
            Err(e) => {
                base_error(format!("cannot open file '{}': {}", file_path, e));
            }
        }
    }
}

unsafe fn eval_source_text_with_options(
    content: &str,
    env: SEXP,
    filename: &str,
    echo: bool,
    print_eval: bool,
    prompt: &str,
    continue_echo: &str,
    skip_echo: c_int,
    keep_source: bool,
    cutoff: c_int,
    deparse_opts: c_int,
    max_deparse_length: usize,
) -> SEXP {
    unsafe {
        if !echo {
            return eval_source_text_with_name(content, env, filename);
        }
        if !keep_source {
            let parsed = parse_source_expression_vector(content);
            let _parsed = protect(parsed);
            if parsed.is_null() || parsed == R_NilValue() {
                crate::sexp::globals::set_R_Visible(FALSE);
                return R_NilValue();
            }
            let n = XLENGTH(parsed);
            let mut result = R_NilValue();
            for i in 0..n {
                let element = VECTOR_ELT(parsed, i);
                if element.is_null() || element == R_NilValue() {
                    continue;
                }

                if crate::sexp::output::is_capturing() {
                    crate::sexp::output::capture_stdout("\n");
                } else {
                    print!("\n");
                }
                echo_source_expression(
                    element,
                    prompt,
                    continue_echo,
                    cutoff,
                    deparse_opts,
                    max_deparse_length,
                );


                result = crate::eval::eval::Rf_eval(element, env);
                if print_eval && crate::sexp::globals::R_Visible() != FALSE {
                    let print_args = Rf_cons(result, R_NilValue());
                    let _print_args = protect(print_args);
                    crate::mainutils::essentials_basic::do_print(
                        R_NilValue(),
                        R_NilValue(),
                        print_args,
                        env,
                    );
                }
            }
            crate::sexp::globals::set_R_Visible(FALSE);
            return result;
        }
        // GNU source() echo with keep.source: original file text via spans
        // (comments, spacing, skip.echo header).

        let spans = crate::sexp::memory::with_arena(|arena| {
            let mut parser = crate::eval::parser::Parser::new(content, arena);
            parser
                .parse_top_level_with_spans()
                .map_err(|e| e.to_string())
        });
        let Ok(spans) = spans else {
            return eval_source_text_with_name(content, env, filename);
        };
        let lines: Vec<&str> = content.lines().collect();
        let nlines = lines.len() as i32;
        let vec_sexp = crate::sexp::constructors::Rf_allocVector3(
            crate::sexp::ffi::SEXPTYPE::EXPRSXP,
            spans.len() as i64,
        );
        let _vg = protect(vec_sexp);
        for (i, &(e, _, _)) in spans.iter().enumerate() {
            crate::sexp::accessors::SET_VECTOR_ELT(vec_sexp, i as i64, e);
        }
        let mut lastshown: i32 = 0;
        let mut result = R_NilValue();
        for (i, &(expr, start, end)) in spans.iter().enumerate() {

            if expr.is_null() || expr == R_NilValue() {
                continue;
            }
            let firstl = byte_line(content, start);
            let last_byte = end.saturating_sub(1).max(start);
            let lastl = byte_line(content, last_byte);
            if i == 0 {
                lastshown = skip_echo.min(lastl.saturating_sub(1)).max(0);
            }
            if lastshown < lastl {
                echo_original_lines(
                    &lines,
                    lastshown + 1,
                    lastl,
                    firstl - lastshown,
                    prompt,
                    continue_echo,
                    true,
                );
                lastshown = lastl;
            }
            result = crate::eval::eval::Rf_eval(expr, env);
            if print_eval && crate::sexp::globals::R_Visible() != FALSE {
                let print_args = Rf_cons(result, R_NilValue());
                let _print_args = protect(print_args);
                crate::mainutils::essentials_basic::do_print(
                    R_NilValue(),
                    R_NilValue(),
                    print_args,
                    env,
                );
            }
        }
        if lastshown < nlines {
            echo_original_lines(
                &lines,
                lastshown + 1,
                nlines,
                nlines - lastshown,
                prompt,
                continue_echo,
                true,
            );
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

fn byte_line(src: &str, byte: usize) -> i32 {
    src.as_bytes()[..byte.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as i32
        + 1
}

/// GNU `trySrcLines` + prompt/continue prefixing.
fn echo_original_lines(
    lines: &[&str],
    from: i32,
    to: i32,
    mut leading: i32,
    prompt: &str,
    continue_echo: &str,
    spaced: bool,
) {
    if from < 1 || to < from {
        return;
    }
    let start = (from as usize).saturating_sub(1);
    let end = (to as usize).min(lines.len());
    if start >= end {
        return;
    }
    let mut dep: Vec<&str> = lines[start..end].to_vec();
    while let Some(first) = dep.first() {
        if first.chars().all(|c| c.is_whitespace()) {
            dep.remove(0);
            leading -= 1;
        } else {
            break;
        }
    }
    if dep.is_empty() {
        return;
    }
    leading = leading.max(0);
    let mut text = String::new();
    if spaced {
        text.push('\n');
    }
    for (i, line) in dep.iter().enumerate() {
        let prefix = if (i as i32) < leading {
            prompt
        } else {
            continue_echo
        };
        text.push_str(prefix);
        text.push_str(line);
        text.push('\n');
    }
    if crate::sexp::output::is_capturing() {
        crate::sexp::output::capture_stdout(&text);
    } else {
        print!("{text}");
    }
}


unsafe fn eval_source_text_with_name(content: &str, env: SEXP, filename: &str) -> SEXP {
    unsafe {
        // keep.source = TRUE: parse with byte spans and attach srcrefs
        // (upstream source() keeps source refs when the option is on, so
        // show.error.locations renders `(from <file>#<line>)`).
        let keep_source = {
            let opt = crate::mainutils::options::GetOption1(crate::sexp::symbol::Rf_install(
                c"keep.source".as_ptr(),
            ));
            !opt.is_null() && crate::mainutils::coerce::asLogical(opt) == 1
        };
        if keep_source {
            let spans = crate::sexp::memory::with_arena(|arena| {
                let mut parser = crate::eval::parser::Parser::new(content, arena);
                parser
                    .parse_top_level_with_spans()
                    .map_err(|e| e.to_string())
            });
            let mut result = R_NilValue();
            if let Ok(spans) = spans {
                let exprs: Vec<SEXP> = spans.iter().map(|&(e, _, _)| e).collect();
                let vec_sexp = crate::sexp::constructors::Rf_allocVector3(
                    crate::sexp::ffi::SEXPTYPE::EXPRSXP,
                    exprs.len() as i64,
                );
                let _vg = crate::sexp::protect::protect(vec_sexp);
                for (i, &e) in exprs.iter().enumerate() {
                    crate::sexp::accessors::SET_VECTOR_ELT(vec_sexp, i as i64, e);
                }
                crate::mainutils::srcref::attach_srcrefs_with_spans(
                    &spans, content, filename, vec_sexp,
                );
                for (i, &(expr, _, _)) in spans.iter().enumerate() {
                    let _ = i;
                    if expr.is_null() || expr == R_NilValue() {
                        continue;
                    }
                    crate::mainutils::srcref::set_current_srcref_location(
                        expr, vec_sexp, i as usize,
                    );
                    result = crate::eval::eval::Rf_eval(expr, env);
                }
                crate::mainutils::srcref::set_current_srcref_location(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return result;
        }
        let parsed = parse_source_expression_vector(content);
        let _parsed = protect(parsed);
        // do_eval()-style element-wise evaluation: Rf_eval returns an
        // expression vector unchanged, so source() walks the statements
        // itself (eval.c eval expression loop).
        let result = if parsed.is_null() || parsed == R_NilValue() {
            R_NilValue()
        } else {
            let mut result = R_NilValue();
            let n = XLENGTH(parsed);
            for i in 0..n {
                let element = VECTOR_ELT(parsed, i);
                if element.is_null() || element == R_NilValue() {
                    continue;
                }
                result = crate::eval::eval::Rf_eval(element, env);
            }
            result
        };

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

unsafe fn eval_source_text(content: &str, env: SEXP) -> SEXP {
    unsafe { eval_source_text_with_name(content, env, "") }
}

/// R's `demo(topic, ...)` — run a demo (simplified).
pub unsafe fn do_demo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let topic_arg = CAR(args);
        if topic_arg.is_null() || topic_arg == R_NilValue() {
            eprintln!("demo: no topic specified");
            return R_NilValue();
        }
        let topic = elt_to_string(topic_arg, 0);
        // Look for demo in common locations
        let demo_path = find_package_demo(&topic);
        if demo_path.is_empty() {
            eprintln!("No demo available for topic '{}'", topic);
            return R_NilValue();
        }
        match std::fs::read_to_string(&demo_path) {
            Ok(_content) => {
                eprintln!("Demo for topic: {}", topic);
                // In a full impl, parse and eval demo content
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(e) => {
                eprintln!("Error reading demo '{}': {}", topic, e);
                R_NilValue()
            }
        }
    }
}

/// GNU `example(topic)` — `substitute(topic)` unless `character.only`.
pub unsafe fn do_example(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(fun) = utils_example_closure() {
            return crate::eval::closure::applyClosure(
                call,
                fun,
                args,
                rho,
                R_NilValue(),
                TRUE,
            );
        }
        let topic_arg = CAR(args);
        if topic_arg.is_null() || topic_arg == R_NilValue() || topic_arg == R_MissingArg() {
            eprintln!("example: no topic specified");
            return R_NilValue();
        }
        let topic = topic_name_from_arg(topic_arg);
        let example_path = find_package_example(&topic);
        if example_path.is_empty() {
            eprintln!("No examples available for topic '{}'", topic);
            return R_NilValue();
        }
        match std::fs::read_to_string(&example_path) {
            Ok(_content) => {
                eprintln!("Examples for topic: {}", topic);
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(e) => {
                eprintln!("Error reading example '{}': {}", topic, e);
                R_NilValue()
            }
        }
    }
}

unsafe fn utils_example_closure() -> Option<SEXP> {
    unsafe {
        let namespace =
            crate::mainutils::essentials::load_package_namespace_by_name("utils").ok()?;
        let symbol = Rf_install(c"example".as_ptr());
        let mut value = crate::sexp::envir::R_findVarInFrame(namespace, symbol);
        if value.is_null() || value == crate::sexp::globals::R_UnboundValue() {
            return None;
        }
        if TYPEOF(value) == SEXPTYPE::PROMSXP {
            value = crate::sexp::envir::forcePromise(value);
        }
        if TYPEOF(value) == SEXPTYPE::CLOSXP {
            Some(value)
        } else {
            None
        }
    }
}

unsafe fn topic_name_from_arg(topic_arg: SEXP) -> String {
    unsafe {
        match TYPEOF(topic_arg) {
            t if t == SEXPTYPE::SYMSXP => {
                let pname = PRINTNAME(topic_arg);
                if pname.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(CHAR(pname))
                        .to_string_lossy()
                        .into_owned()
                }
            }
            _ => elt_to_string(topic_arg, 0),
        }
    }
}

