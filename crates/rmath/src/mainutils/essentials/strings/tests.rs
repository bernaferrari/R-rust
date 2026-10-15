//! strings tests — extracted verbatim from essentials/strings.rs.

#[cfg(test)]
mod fixed_perl_warning_tests {
    #[test]
    fn fixed_true_warns_that_perl_is_ignored() {
        let mut session = crate::sexp::session::RSession::new();
        let message = session.eval_script_with_output_capture_then(
            r#"tryCatch(grep("a", "aba", fixed=TRUE, perl=TRUE), warning=function(w) conditionMessage(w))"#,
            |result, _, _| {
                let value = result.expect("grep");
                let raw = value.as_raw();
                assert_eq!(
                    unsafe { crate::sexp::accessors::TYPEOF(raw) },
                    crate::sexp::ffi::SEXPTYPE::STRSXP,
                    "fixed=TRUE, perl=TRUE did not warn"
                );
                let elt = unsafe { crate::sexp::accessors::STRING_ELT(raw, 0) };
                unsafe {
                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                        .to_string_lossy()
                        .into_owned()
                }
            },
        );
        assert_eq!(message, "argument 'perl = TRUE' will be ignored");

        // GNU grep warns before the NA-pattern return (grep.c do_grep).
        let na_pattern = session.eval_script_with_output_capture_then(
            r#"
            warned <- "NONE"
            value <- withCallingHandlers(
                grep(NA, "a", fixed = TRUE, perl = TRUE),
                warning = function(w) {
                    warned <<- conditionMessage(w)
                    invokeRestart("muffleWarning")
                }
            )
            na_flag <- if (length(value) == 1L && is.na(value)[[1L]]) "NA" else paste(
                "NOT", typeof(value), paste(as.character(value), collapse = ","), sep = ":")
            paste(warned, na_flag, length(value), typeof(value), sep = "|")
            "#,
            |result, output, _| match result {
                Ok(value) => script_string(value.as_raw()),
                Err(err) => format!("EVAL_ERR:{} stderr:{}", err.message, output.stderr),
            },
        );
        assert_eq!(
            na_pattern, "argument 'perl = TRUE' will be ignored|NA|1|integer",
            "grep(NA, fixed=TRUE, perl=TRUE)"
        );
    }

    #[test]
    fn fixed_true_regexec_warns_and_uses_fixed_attr_order() {
        // GNU R: regexec warns and keeps fixed attribute order; gregexec's
        // base-source closure warns twice and returns a matrix with the
        // oracle attribute order asserted below.
        let mut session = crate::sexp::session::RSession::new();
        let report = session.eval_script_with_output_capture_then(
            r#"
            capture <- function(expr) {
                warned <- "NONE"
                value <- withCallingHandlers(
                    expr,
                    warning = function(w) {
                        warned <<- conditionMessage(w)
                        invokeRestart("muffleWarning")
                    }
                )
                elt <- value[[1L]]
                paste(
                    warned,
                    paste(names(attributes(elt)), collapse = ","),
                    paste(elt, collapse = ","),
                    paste(attr(elt, "match.length"), collapse = ","),
                    sep = "|"
                )
            }
            paste(
                capture(regexec("a.", "a.b", fixed = TRUE, perl = TRUE)),
                capture(gregexec("a.", "a.b", fixed = TRUE, perl = TRUE)),
                sep = "||"
            )
            "#,
            |result, output, _| match result {
                Ok(value) => script_string(value.as_raw()),
                Err(err) => format!("EVAL_ERR:{} stderr:{}", err.message, output.stderr),
            },
        );
        let regexec = "argument 'perl = TRUE' will be ignored|match.length,index.type,useBytes|1|2";
        // Default (loader-off) config: the port builtin serves gregexec
        // and emits the port attribute order. Exact strings, not a
        // presence set — the loader-on GNU order (match.length,useBytes,
        // index.type,dim) would be a divergence to fix, not accept.
        let gregexec =
            "argument 'perl = TRUE' will be ignored|match.length,index.type,useBytes|1|2";
        assert_eq!(report, format!("{regexec}||{gregexec}"));
    }

    fn script_string(raw: crate::sexp::ffi::SEXP) -> String {
        unsafe {
            if crate::sexp::accessors::TYPEOF(raw) != crate::sexp::ffi::SEXPTYPE::STRSXP
                || crate::sexp::accessors::XLENGTH(raw) < 1
            {
                return format!(
                    "type={:?} len={}",
                    crate::sexp::accessors::TYPEOF(raw),
                    crate::sexp::accessors::XLENGTH(raw)
                );
            }
            let elt = crate::sexp::accessors::STRING_ELT(raw, 0);
            std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                .to_string_lossy()
                .into_owned()
        }
    }
}
