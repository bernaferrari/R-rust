//! output — extracted verbatim from output.rs.

use super::*;

/// Format a value for top-level emission, excluding the caller-owned final
/// line terminator.
///
/// `printList()` emits a separator newline after every non-empty list
/// element, including the last one. Nested lists already include that
/// separator in their body; the outermost list still needs one more so
/// the next top-level print is separated by two blanks.
pub(crate) fn format_sexp_top_level(x: Sexp<'_>) -> String {
    format_sexp_direct(x)
}

pub(crate) fn first_deparse_line(text: crate::sexp::ffi::SEXP) -> Option<String> {
    unsafe {
        if text.is_null() || XLENGTH(text) == 0 {
            return None;
        }
        let charsxp = STRING_ELT(text, 0);
        if charsxp.is_null() {
            return None;
        }
        let chars = CHAR(charsxp);
        if chars.is_null() {
            None
        } else {
            Some(
                std::ffi::CStr::from_ptr(chars)
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }
}

pub(crate) fn primitive_args_prototype(name: &str) -> Option<crate::sexp::ffi::SEXP> {
    unsafe {
        let name_c = std::ffi::CString::new(name).ok()?;
        let symbol = crate::sexp::symbol::Rf_install(name_c.as_ptr());
        let base = crate::sexp::globals::R_BaseEnv();
        for registry in [".ArgsEnv", ".GenericArgsEnv"] {
            let registry_c = std::ffi::CString::new(registry).ok()?;
            let registry_sym = crate::sexp::symbol::Rf_install(registry_c.as_ptr());
            let mut env = crate::sexp::envir::R_findVarInFrame(base, registry_sym);
            if env.is_null() || env == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            if TYPEOF(env) == SEXPTYPE::PROMSXP {
                env = crate::eval::eval::Rf_eval(env, base);
            }
            if env.is_null()
                || env == crate::sexp::globals::R_UnboundValue()
                || TYPEOF(env) != SEXPTYPE::ENVSXP
            {
                continue;
            }
            let proto = crate::sexp::envir::R_findVarInFrame(env, symbol);
            if !proto.is_null()
                && proto != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(proto) == SEXPTYPE::CLOSXP
            {
                return Some(proto);
            }
        }
        None
    }
}

pub(crate) fn format_primitive(x: Sexp<'_>) -> String {
    unsafe {
        let name = crate::eval::primitive::PRIMNAME(x.as_raw());
        let primitive = format!(".Primitive(\"{name}\")");
        let Some(proto) = primitive_args_prototype(name) else {
            return primitive;
        };
        let text = crate::mainutils::deparse::deparse1m(
            proto,
            false,
            crate::mainutils::deparse::DEFAULTDEPARSE,
        );
        match first_deparse_line(text) {
            Some(line) if !line.is_empty() => format!("{line} {primitive}"),
            _ => primitive,
        }
    }
}

pub(crate) fn deparse_expression_one(expr: SEXP) -> String {
    unsafe {
        // GNU print.c DEFAULTDEPARSE is keepNA | keepInteger | niceNames.
        // `deparse()` adds showAttributes, so a class string with a package
        // attribute prints as `"foo"` and deparses as structure(...).
        let opts =
            crate::mainutils::deparse::DEFAULTDEPARSE & !crate::mainutils::deparse::SHOWATTRIBUTES;
        let text = crate::mainutils::deparse::deparse1WithCutoff(
            expr,
            false,
            crate::mainutils::deparse::MAX_CUTOFF,
            true,
            opts,
            -1,
        );
        let n = XLENGTH(text);
        if n <= 1 {
            return first_deparse_line(text).unwrap_or_default();
        }
        let mut parts = Vec::with_capacity(n as usize);
        for i in 0..n {
            let charsxp = STRING_ELT(text, i);
            if charsxp.is_null() {
                parts.push(String::new());
                continue;
            }
            let chars = CHAR(charsxp);
            if chars.is_null() {
                parts.push(String::new());
            } else {
                parts.push(
                    std::ffi::CStr::from_ptr(chars)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        parts.join("\n")
    }
}

pub(crate) fn format_expression_vector(x: Sexp<'_>) -> String {
    unsafe {
        let raw = x.as_raw();
        let n = XLENGTH(raw);
        if n == 0 {
            return "expression()".to_string();
        }
        let parts = (0..n)
            .map(|i| deparse_expression_one(VECTOR_ELT(raw, i)))
            .collect::<Vec<_>>();
        format!("expression({})", parts.join(", "))
    }
}

pub fn format_sexp_direct(x: Sexp<'_>) -> String {
    if let Some(message) = result_admission_error(x.clone()) {
        return message;
    }

    if has_class(x.clone(), "try-error") {
        return unsafe { format_try_error(x) };
    }
    if has_class(x.clone(), "condition") {
        return unsafe { format_condition(x) };
    }
    if let Some(text) = auto_print_method(x.clone()) {
        return text;
    }
    // The type match below does not format S4, so dispatch show() here.
    if unsafe {
        crate::mainutils::coerce::IS_S4_OBJECT(x.clone().as_raw()) != 0
            || x.typeof_() == SEXPTYPE::S4SXP
    } {
        if let Some(text) = format_dispatched_show(x.as_raw()) {
            return text;
        }
    }
    format_sexp_body(x)
}

pub(crate) fn format_sexp_body(x: Sexp<'_>) -> String {
    match x.clone().typeof_() {
        SEXPTYPE::NILSXP => "NULL".to_string(),
        SEXPTYPE::INTSXP => {
            if x.clone().len() == 0 {
                let empty = if has_names_attribute(x.clone()) {
                    "named integer(0)"
                } else {
                    "integer(0)"
                };
                return format_with_printable_attributes(empty.to_string(), x);
            }
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                return output;
            }
            if let Some(output) = format_table(x.clone()) {
                return output;
            }
            if let Some(output) = format_factor(x.clone()) {
                return output;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::REALSXP => {
            if has_class(x.clone(), "difftime") {
                return format_difftime_vector(x);
            }
            if has_class(x.clone(), "POSIXct") {
                return format_posixct_vector(x, true);
            }
            if has_class(x.clone(), "Date") {
                return format_date_vector(x);
            }
            if x.clone().len() == 0 {
                return format_with_printable_attributes("numeric(0)".to_string(), x);
            }
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                return output;
            }
            if let Some(output) = format_table(x.clone()) {
                return output;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::LGLSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if x.clone().len() == 0 {
                return format_with_printable_attributes("logical(0)".to_string(), x);
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }

        SEXPTYPE::CPLXSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if x.clone().len() == 0 {
                return format_with_printable_attributes("complex(0)".to_string(), x);
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }

        SEXPTYPE::STRSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                return output;
            }
            let quote = print_quote_flag()
                && !has_class(x.clone(), "noquote")
                && !has_class(x.clone(), "table");
            let base = unsafe { format_vector_stock(x.clone(), quote) };
            format_with_printable_attributes(base, x)
        }

        SEXPTYPE::RAWSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if x.clone().len() == 0 {
                return format_with_printable_attributes("raw(0)".to_string(), x);
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::VECSXP => {
            if has_class(x.clone(), "summary.warnings") {
                return unsafe {
                    crate::mainutils::essentials::format_summary_warnings(x.as_raw())
                        .trim_end_matches('\n')
                        .to_string()
                };
            }
            if has_class(x.clone(), "POSIXlt") {
                return format_posixlt_vector(x);
            }
            if let Some(output) = format_data_frame(x.clone()) {
                return output;
            }
            format_list(x)
        }

        SEXPTYPE::EXPRSXP => format_expression_vector(x),
        SEXPTYPE::SYMSXP | SEXPTYPE::LANGSXP | SEXPTYPE::CLOSXP => {
            let base = deparse_expression_one(x.clone().as_raw());
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::SPECIALSXP | SEXPTYPE::BUILTINSXP => {
            format_with_printable_attributes(format_primitive(x.clone()), x)
        }

        SEXPTYPE::LISTSXP => format_pairlist(x),
        SEXPTYPE::ENVSXP => format_environment(x),
        tp => {
            let type_name = match tp {
                SEXPTYPE::RAWSXP => "raw",
                SEXPTYPE::CPLXSXP => "complex",
                SEXPTYPE::CLOSXP => "closure",
                SEXPTYPE::CHARSXP => "charsxp",
                _ => "unknown",
            };
            format!("[{}; length={}]", type_name, x.len())
        }
    }
}

/// First class string of an object, for print.condition-style rendering.
pub(crate) unsafe fn first_class_string(x: Sexp<'_>) -> Option<String> {
    unsafe {
        let klass = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        let klass = Sexp::from_raw(klass)?;
        if klass.clone().typeof_() != SEXPTYPE::STRSXP || klass.clone().len() == 0 {
            return None;
        }
        let s = crate::sexp::accessors::STRING_ELT(klass.as_raw(), 0);
        if s.is_null() {
            return None;
        }
        let chars = crate::sexp::accessors::CHAR(s);
        if chars.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .ok()
            .map(str::to_string)
    }
}

/// Stock print.condition: `<class: msg>` or `<class in <deparsed call>: msg>`.
/// Conditions are `list(message, call, ...)` per R_makeErrorCondition.
pub(crate) unsafe fn format_condition(x: Sexp<'_>) -> String {
    // SAFETY: callers provide a rooted live condition object. Each optional
    // list/string field is tag-checked before raw element access.
    unsafe {
        let raw = x.clone().as_raw();
        let class = first_class_string(x.clone()).unwrap_or_else(|| "condition".to_string());
        let mut message = String::new();
        let mut call_text = String::new();
        if x.clone().typeof_() == SEXPTYPE::VECSXP && x.clone().len() >= 1 {
            let msg = crate::sexp::accessors::VECTOR_ELT(raw, 0);
            if !msg.is_null() && msg != crate::sexp::globals::R_NilValue() {
                if let Some(sexp) = Sexp::from_raw(msg) {
                    if sexp.clone().typeof_() == SEXPTYPE::STRSXP && sexp.clone().len() >= 1 {
                        let elt = crate::sexp::accessors::STRING_ELT(sexp.as_raw(), 0);
                        if !elt.is_null() {
                            let chars = crate::sexp::accessors::CHAR(elt);
                            if !chars.is_null() {
                                if let Ok(s) = std::ffi::CStr::from_ptr(chars).to_str() {
                                    message = s.to_string();
                                }
                            }
                        }
                    }
                }
            }
            if x.len() >= 2 {
                let call = crate::sexp::accessors::VECTOR_ELT(raw, 1);
                if !call.is_null() && call != crate::sexp::globals::R_NilValue() {
                    let dcall = crate::mainutils::deparse::deparse1s(call);
                    if !dcall.is_null() && dcall != crate::sexp::globals::R_NilValue() {
                        let elt = crate::sexp::accessors::STRING_ELT(dcall, 0);
                        if !elt.is_null() {
                            let chars = crate::sexp::accessors::CHAR(elt);
                            if !chars.is_null() {
                                if let Ok(s) = std::ffi::CStr::from_ptr(chars).to_str() {
                                    call_text = format!(" in {s}");
                                }
                            }
                        }
                    }
                }
            }
        }
        format!("<{class}{call_text}: {message}>")
    }
}

/// Stock print.default on a try-error object: the message string rendered as
/// a character vector, then the class and condition attributes.
pub(crate) unsafe fn format_try_error(x: Sexp<'_>) -> String {
    // SAFETY: callers provide a rooted live try-error object; attribute and
    // vector reads remain within the lifetime carried by `x`.
    unsafe {
        let mut out = if x.clone().typeof_() == SEXPTYPE::STRSXP {
            format_vector_stock(x.clone(), true)
        } else {
            format!("{}\n", format_sexp_direct(x.clone()))
        };
        out.push('\n');
        unsafe {
            let klass = crate::sexp::attrib_core::getAttrib(
                x.clone().as_raw(),
                crate::sexp::attrib_core::R_ClassSymbol(),
            );
            let has_condition = {
                let cond_sym = crate::sexp::symbol::Rf_install(
                    b"condition\0".as_ptr() as *const std::os::raw::c_char
                );
                let cond = crate::sexp::attrib_core::getAttrib(x.clone().as_raw(), cond_sym);
                !cond.is_null() && cond != R_NilValue()
            };
            if let Some(klass) = Sexp::from_raw(klass) {
                if klass.clone().typeof_() == SEXPTYPE::STRSXP {
                    out.push_str("attr(,\"class\")\n");
                    out.push_str(&format_vector_stock(klass, true));
                    // print_value owns the final newline; add the separator
                    // between attribute sections here.
                    if has_condition {
                        out.push('\n');
                    }
                }
            }
            let cond_sym = crate::sexp::symbol::Rf_install(
                b"condition\0".as_ptr() as *const std::os::raw::c_char
            );
            let cond = crate::sexp::attrib_core::getAttrib(x.as_raw(), cond_sym);
            if let Some(cond) = Sexp::from_raw(cond) {
                if cond.clone().typeof_() != SEXPTYPE::NILSXP {
                    out.push_str("attr(,\"condition\")\n");
                    out.push_str(&format_condition(cond));
                }
            }
        }
        out
    }
}

pub(crate) fn format_environment_public(x: Sexp<'_>) -> String {
    format_environment(x)
}

pub(crate) fn format_environment(x: Sexp<'_>) -> String {
    let raw = x.as_raw();
    let name = unsafe {
        if raw == crate::sexp::globals::R_GlobalEnv() {
            "R_GlobalEnv".to_string()
        } else if raw == crate::sexp::globals::R_BaseEnv() {
            "base".to_string()
        } else if raw == crate::sexp::globals::R_EmptyEnv() {
            "R_EmptyEnv".to_string()
        } else {
            format!("{raw:p}")
        }
    };
    format!("<environment: {name}>")
}
