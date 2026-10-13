//! output — extracted verbatim from output.rs.

use super::*;

thread_local! {
    static PRINT_DISPATCH_EXTRAS: std::cell::Cell<SEXP> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

pub(crate) struct PrintDispatchExtrasGuard {
    previous: SEXP,
}

impl Drop for PrintDispatchExtrasGuard {
    fn drop(&mut self) {
        PRINT_DISPATCH_EXTRAS.with(|slot| slot.set(self.previous));
    }
}

/// Forward extra `print(...)` arguments to recursive `print.<class>` methods.
pub(crate) fn push_print_dispatch_extras(extras: SEXP) -> PrintDispatchExtrasGuard {
    let previous = PRINT_DISPATCH_EXTRAS.with(|slot| slot.replace(extras));
    PrintDispatchExtrasGuard { previous }
}

pub(crate) fn print_dispatch_extras() -> SEXP {
    PRINT_DISPATCH_EXTRAS.with(|slot| slot.get())
}

pub(crate) unsafe fn cons_print_args(x: SEXP) -> SEXP {
    unsafe {
        let extras = print_dispatch_extras();
        let extras = if extras.is_null() {
            R_NilValue()
        } else {
            extras
        };
        crate::sexp::constructors::Rf_cons(x, extras)
    }
}

pub(crate) unsafe fn print_args_except_x(args: SEXP, x: SEXP) -> SEXP {
    unsafe {
        let mut head = R_NilValue();
        let mut tail = R_NilValue();
        let mut cur = args;
        let mut skipped_x = false;
        while !cur.is_null() && cur != R_NilValue() {
            let value = CAR(cur);
            if !skipped_x && value == x {
                skipped_x = true;
            } else {
                let cell = crate::sexp::constructors::Rf_cons(value, R_NilValue());
                crate::sexp::accessors::SETTAG(cell, TAG(cur));
                if head == R_NilValue() {
                    head = cell;
                } else {
                    crate::sexp::accessors::SETCDR(tail, cell);
                }
                tail = cell;
            }
            cur = CDR(cur);
        }
        head
    }
}

/// Copy every `print()` argument except `x` for recursive method dispatch.
pub(crate) unsafe fn copy_print_dispatch_extras(args: SEXP, x: SEXP) -> SEXP {
    unsafe { print_args_except_x(args, x) }
}

pub(crate) fn format_dispatched_show(raw: crate::sexp::ffi::SEXP) -> Option<String> {
    unsafe {
        let path = crate::mainutils::essentials::find_package_path("methods");
        if crate::mainutils::essentials::cached_namespace_by_name("methods").is_none()
            && !path.is_empty()
        {
            let _ = crate::mainutils::essentials::load_pure_r_package(
                "methods",
                std::path::Path::new(&path),
            );
        }
        let namespace = crate::mainutils::essentials::cached_namespace_by_name("methods")?;
        let symbol = crate::sexp::symbol::Rf_install(c"show".as_ptr());
        let mut fun = crate::sexp::envir::R_findVarInFrame(namespace, symbol);
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            return None;
        }
        if TYPEOF(fun) == SEXPTYPE::PROMSXP {
            fun = crate::sexp::envir::forcePromise(fun);
        }
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            return None;
        }
        let _fun = crate::sexp::protect::protect(fun);
        let call = crate::sexp::constructors::Rf_lang2(fun, raw);
        let _call = crate::sexp::protect::protect(call);
        let env = crate::sexp::globals::R_GlobalEnv();
        let guard = OutputCaptureGuard::start();
        // stop() panics. Returning this buffer would look like success, so
        // resume after saving text that was printed before the error.
        let evaluated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(call, env)
        }));
        let captured = guard.finish();
        match evaluated {
            Ok(_) => Some(captured.stdout.trim_end_matches('\n').to_string()),
            Err(payload) => {
                keep_show_stdout_before_error(&captured.stdout);
                std::panic::resume_unwind(payload)
            }
        }
    }
}

pub(crate) fn format_dispatched_print(x: Sexp<'_>) -> Option<String> {
    unsafe {
        let raw = x.as_raw();
        if crate::sexp::accessors::OBJECT(raw) == 0 {
            return None;
        }
        if crate::mainutils::coerce::IS_S4_OBJECT(raw) != 0 {
            return format_dispatched_show(raw);
        }
    }
    let guard = OutputCaptureGuard::start();
    let evaluated =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch_print_method(x)));
    let captured = guard.finish();
    match evaluated {
        Ok(Some(())) => Some(captured.stdout.trim_end_matches('\n').to_string()),
        Ok(None) => None,
        Err(payload) => {
            keep_show_stdout_before_error(&captured.stdout);
            std::panic::resume_unwind(payload)
        }
    }
}

/// Execute the method into the caller's capture, keeping stream order intact.
pub(crate) fn dispatch_print_method(x: Sexp<'_>) -> Option<()> {
    unsafe {
        let raw = x.as_raw();
        if crate::sexp::accessors::OBJECT(raw) == 0 {
            return None;
        }
        let klass =
            crate::sexp::attrib_core::getAttrib(raw, crate::sexp::attrib_core::R_ClassSymbol());
        let env = crate::sexp::globals::R_GlobalEnv();
        let method = crate::mainutils::objects::lookup_s3_method_for_classes(
            "print", klass, env, env, env, false,
        )?;
        let method_ty = TYPEOF(method.method);
        if method_ty != SEXPTYPE::CLOSXP
            && method_ty != SEXPTYPE::BUILTINSXP
            && method_ty != SEXPTYPE::SPECIALSXP
        {
            return None;
        }
        let extras = print_dispatch_extras();
        let extras = if extras.is_null() {
            R_NilValue()
        } else {
            extras
        };
        let args = crate::sexp::constructors::Rf_cons(raw, extras);
        let _args = crate::sexp::protect::protect(args);
        let print_sym = crate::sexp::symbol::Rf_install(c"print".as_ptr());
        let call = crate::sexp::constructors::Rf_cons(print_sym, args);
        if !call.is_null() {
            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        }
        let _call = crate::sexp::protect::protect(call);
        crate::mainutils::essentials::apply_s3_closure_method("print", call, args, env).map(|_| ())
    }
}

/// GNU `PrintValue` dispatches `print` when a class is set. try-error,
/// conditions, srcref, and S4 keep their dedicated printers.
pub(crate) fn auto_print_method(x: Sexp<'_>) -> Option<String> {
    auto_print_method_supported(&x)
        .then(|| format_dispatched_print(x))
        .flatten()
}

pub(crate) fn auto_print_method_supported(x: &Sexp<'_>) -> bool {
    if has_class(x.clone(), "try-error")
        || has_class(x.clone(), "condition")
        || has_class(x.clone(), "srcref")
        // These closures still disagree with the printers that match GNU.
        || has_class(x.clone(), "noquote")
        || has_class(x.clone(), "summaryDefault")
    {
        return false;
    }
    if unsafe {
        crate::mainutils::coerce::IS_S4_OBJECT(x.clone().as_raw()) != 0
            || x.typeof_() == SEXPTYPE::S4SXP
    } {
        return false;
    }
    true
}

/// Print an R object to the captured output (or stdout if not capturing).
///
/// This is the Rust implementation of R's Rf_PrintValue. For Android
/// embedding, use [`start_capture`] before evaluation and [`stop_capture`]
/// after to collect printed output as a string.
pub fn print_value(x: Sexp<'_>) {
    if let Some(message) = result_admission_error(x.clone()) {
        emit(&message);
        emit("\n");
        return;
    }
    // Objects of class "try-error" print per stock print.default: the
    // message string as a character vector plus class/condition attrs.
    // Condition objects print via print.condition.
    if has_class(x.clone(), "try-error") {
        unsafe { emit(&format!("{}\n", format_try_error(x))) };
        return;
    }
    if has_class(x.clone(), "condition") {
        unsafe { emit(&format!("{}\n", format_condition(x))) };
        return;
    }
    if has_class(x.clone(), "srcref") {
        unsafe {
            crate::mainutils::print::PrintValueEnv(
                x.clone().as_raw(),
                crate::sexp::globals::R_GlobalEnv(),
            );
        }
        return;
    }

    // GNU auto-print of S4 objects goes through PrintValueEnv -> show().
    if unsafe { crate::mainutils::objects::IS_S4_OBJECT(x.clone().as_raw()) } != 0 {
        unsafe {
            crate::mainutils::print::PrintValueEnv(
                x.clone().as_raw(),
                crate::sexp::globals::R_GlobalEnv(),
            );
        }
        return;
    }

    if auto_print_method_supported(&x) && dispatch_print_method(x.clone()).is_some() {
        return;
    }

    match x.clone().typeof_() {
        SEXPTYPE::SYMSXP | SEXPTYPE::LANGSXP | SEXPTYPE::CLOSXP => {
            let is_closure = x.clone().typeof_() == SEXPTYPE::CLOSXP;
            if is_closure && print_use_source_flag() {
                if let Some(mut source) =
                    crate::mainutils::essentials::print::function_srcref_text(x.clone().as_raw())
                {
                    // GNU PrintClosure appends the environment line after
                    // the source text as well (print.c:189-198).
                    unsafe {
                        crate::mainutils::essentials::print::append_closure_environment(
                            x.clone().as_raw(),
                            &mut source,
                        )
                    };
                    emit(&source);
                    return;
                }
            }
            let base = deparse_expression_one(x.clone().as_raw());
            let closure_raw = if is_closure {
                Some(x.clone().as_raw())
            } else {
                None
            };
            print_with_printable_attributes(&base, x);
            if let Some(raw) = closure_raw {
                let mut tail = String::new();
                unsafe {
                    crate::mainutils::essentials::print::append_closure_environment(raw, &mut tail)
                };
                emit(&tail);
            }
        }
        SEXPTYPE::SPECIALSXP | SEXPTYPE::BUILTINSXP => {
            print_with_printable_attributes(&format_primitive(x.clone()), x);
        }

        SEXPTYPE::LISTSXP => {
            print_pairlist_with_path(x, "");
        }

        SEXPTYPE::NILSXP => {
            emit("NULL\n");
        }
        SEXPTYPE::INTSXP => {
            if let Some(output) = format_matrix_body(x.clone()) {
                print_with_printable_attributes(&output, x);
                return;
            }
            if x.clone().len() == 0 {
                let empty = if has_names_attribute(x.clone()) {
                    "named integer(0)"
                } else {
                    "integer(0)"
                };
                print_with_printable_attributes(empty, x);
                return;
            }

            if let Some(output) = format_summary_default(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_table(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_factor(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            print_with_printable_attributes(&base, x);
        }
        SEXPTYPE::REALSXP => {
            if let Some(output) = format_matrix_body(x.clone()) {
                print_with_printable_attributes(&output, x);
                return;
            }
            if x.clone().len() == 0
                && !has_class(x.clone(), "difftime")
                && !has_class(x.clone(), "POSIXct")
                && !has_class(x.clone(), "Date")
            {
                print_with_printable_attributes("numeric(0)", x);
                return;
            }

            if let Some(output) = format_summary_default(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_table(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if has_class(x.clone(), "difftime") {
                emit(&format!("{}\n", format_difftime_vector(x)));
                return;
            }
            if has_class(x.clone(), "POSIXct") {
                emit(&format!("{}\n", format_posixct_vector(x, true)));
                return;
            }
            if has_class(x.clone(), "Date") {
                emit(&format!("{}\n", format_date_vector(x)));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            print_with_printable_attributes(&base, x);
        }
        SEXPTYPE::LGLSXP => {
            if let Some(output) = format_matrix_body(x.clone()) {
                print_with_printable_attributes(&output, x);
                return;
            }
            if x.clone().len() == 0 {
                print_with_printable_attributes("logical(0)", x);
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            print_with_printable_attributes(&base, x);
        }

        SEXPTYPE::CPLXSXP => {
            if let Some(output) = format_matrix_body(x.clone()) {
                print_with_printable_attributes(&output, x);
                return;
            }
            if x.clone().len() == 0 {
                print_with_printable_attributes("complex(0)", x);
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            print_with_printable_attributes(&base, x);
        }

        SEXPTYPE::STRSXP => {
            if let Some(output) = format_matrix_body(x.clone()) {
                print_with_printable_attributes(&output, x);
                return;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            let quote = print_quote_flag()
                && !has_class(x.clone(), "noquote")
                && !has_class(x.clone(), "table");
            let base = unsafe { format_vector_stock(x.clone(), quote) };
            print_with_printable_attributes(&base, x);
        }
        SEXPTYPE::RAWSXP => {
            if let Some(output) = format_matrix_body(x.clone()) {
                print_with_printable_attributes(&output, x);
                return;
            }
            if x.clone().len() == 0 {
                print_with_printable_attributes("raw(0)", x);
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            print_with_printable_attributes(&base, x);
        }
        SEXPTYPE::VECSXP => {
            if has_class(x.clone(), "summary.warnings") {
                let text =
                    unsafe { crate::mainutils::essentials::format_summary_warnings(x.as_raw()) };

                emit(&text);
                return;
            }

            if has_class(x.clone(), "POSIXlt") {
                emit(&format!("{}\n", format_posixlt_vector(x)));
                return;
            }
            // GNU PrintValue dispatches print() for objects. Auto-print of
            // `(dd <- data.frame(...))` must use print.data.frame, not the
            // internal formatter (which panics on POSIXt columns).
            if has_class(x.clone(), "data.frame") {
                unsafe {
                    let args = crate::sexp::constructors::Rf_cons(
                        x.clone().as_raw(),
                        crate::sexp::globals::R_NilValue(),
                    );
                    let _g = crate::sexp::protect::protect(args);
                    crate::mainutils::essentials::do_print_data_frame(
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        args,
                        crate::sexp::globals::R_GlobalEnv(),
                    );
                }
                return;
            }
            print_list_with_path(x, "");
        }

        SEXPTYPE::EXPRSXP => {
            // GNU PrintExpression: deparse1w with the print cutoff, so long
            // lines wrap inside expression(...) exactly like print() does.
            // Element-wise MAX_CUTOFF deparse never wrapped them (PR#15534
            // derivatives printed .grad/.hessian lines unwrapped).
            let opts = crate::mainutils::deparse::DEFAULTDEPARSE
                & !crate::mainutils::deparse::SHOWATTRIBUTES;
            let text =
                unsafe { crate::mainutils::deparse::deparse1w(x.clone().as_raw(), false, opts) };
            let _text_guard = unsafe { crate::sexp::protect::protect(text) };
            let mut joined = String::new();
            for i in 0..unsafe { XLENGTH(text) } {
                let chars = unsafe { CHAR(STRING_ELT(text, i)) };
                if i > 0 {
                    joined.push('\n');
                }
                if !chars.is_null() {
                    joined
                        .push_str(&(unsafe { std::ffi::CStr::from_ptr(chars) }.to_string_lossy()));
                }
            }
            print_with_printable_attributes(&joined, x);
        }
        SEXPTYPE::ENVSXP => {
            print_with_printable_attributes(&format_environment(x.clone()), x);
        }
        tp => {
            let type_name = match tp {
                SEXPTYPE::RAWSXP => "raw",
                SEXPTYPE::CPLXSXP => "complex",
                SEXPTYPE::SYMSXP => "symbol",
                SEXPTYPE::CLOSXP => "closure",
                SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => "pairlist",
                SEXPTYPE::CHARSXP => "charsxp",
                SEXPTYPE::PROMSXP => "promise",
                SEXPTYPE::DOTSXP => "...",
                SEXPTYPE::SPECIALSXP => "special",
                SEXPTYPE::BUILTINSXP => "builtin",
                SEXPTYPE::EXTPTRSXP => "externalptr",
                SEXPTYPE::WEAKREFSXP => "weakref",
                SEXPTYPE::BCODESXP => "bytecode",
                SEXPTYPE::OBJSXP => "object",
                _ => "unknown",
            };
            let output = format!("[{}; length={}]", type_name, x.len());
            emit(&output);
            emit("\n");
        }
    }
}

/// Print an R object's structure (like str()).
pub fn print_structure(x: Sexp<'_>, indent: usize) {
    let prefix = "  ".repeat(indent);

    match x.clone().typeof_() {
        SEXPTYPE::INTSXP => {
            let vals: Vec<_> = x.clone().iter_integer().take(10).collect();
            let suffix = if x.clone().len() > 10 { ", ..." } else { "" };
            let output = format!("{}int [{}]: {:?}{}", prefix, x.len(), vals, suffix);
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::REALSXP => {
            let vals: Vec<_> = x.clone().iter_real().take(10).collect();
            let suffix = if x.clone().len() > 10 { ", ..." } else { "" };
            let output = format!("{}double [{}]: {:?}{}", prefix, x.len(), vals, suffix);
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::STRSXP => {
            let output = format!("{}character [{}]", prefix, x.len());
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::VECSXP => {
            let output = format!("{}list [{}]", prefix, x.clone().len());
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
            for (i, elem) in x.iter_vector().take(5).enumerate() {
                print_structure(elem, indent + 1);
            }
        }
        _ => {
            print_value(x);
        }
    }
}

/// FFI function: Rf_PrintValue
pub(crate) unsafe fn Rf_PrintValue(x: SEXP) {
    if let Some(s) = unsafe { Sexp::from_raw(x) } {
        print_value(s);
    }
}

/// FFI function: Rf_PrintValueEnv (print with environment context)
pub(crate) unsafe fn Rf_PrintValueEnv(x: SEXP, _env: SEXP) {
    if let Some(s) = unsafe { Sexp::from_raw(x) } {
        print_value(s);
    }
}
