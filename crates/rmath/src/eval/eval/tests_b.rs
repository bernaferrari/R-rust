//! eval tests (part 2/4) — extracted verbatim from eval/eval.rs

use super::*;
use crate::sexp::builder::scalar_integer_in;
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_lang2};
use crate::sexp::session::RSession;
use crate::sexp::symbol::Rf_install;

#[cfg(test)]
mod tests_b {
    use super::*;

    #[test]
    fn str_data_frame_aligns_names_and_omits_column_length() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
df <- data.frame(Time=c(1,2,3,4,5,7), demand=c(8.3,10.3,19,16,15.6,19.8))
attr(df, "reference") <- "A1.4, p. 270"
str(df)
f <- ordered(c("Qn1","Qn1","Qn2"), levels=c("Qn1","Qn2","Qn3"))
g <- factor(c("Qn1","Qn1","Qn2"), levels=c("Qn1","Qn2","Qn3"), ordered=TRUE)
str(f)
str(g)
fm <- uptake ~ conc | Plant
attr(fm, ".Environment") <- emptyenv()
str(fm)
labs <- list(x="Ambient carbon dioxide concentration", y="CO2 uptake rate")
attr(df, "labels") <- labs
str(df)
m <- matrix(1:4, 2, 2, dimnames=list(c("a","b"), c("c","d")))
lst <- list(cov=m, center=c(0,0))
str(lst)
d <- as.Date(c("2007-11-11", NA))
str(d)
str(c(1.5, NA_real_))
str(c("MALE", NA_character_))



invisible(NULL)

"#,
        );
        assert!(
            captured.stdout.contains("$ Time  : num  1 2 3 4 5 7")
                && captured
                    .stdout
                    .contains("$ demand: num  8.3 10.3 19 16 15.6 19.8")
                && captured
                    .stdout
                    .contains("- attr(*, \"reference\")= chr \"A1.4, p. 270\""),
            "data.frame str must align names, omit [1:n], print extra attrs, got {:?}",
            captured.stdout
        );
        assert!(
            captured
                .stdout
                .matches("Ord.factor w/ 3 levels \"Qn1\"<\"Qn2\"<\"Qn3\": 1 1 2")
                .count()
                >= 2,
            "both ordered() and factor(ordered=TRUE) must be Ord.factor, got {:?}",
            captured.stdout
        );
        assert!(
            captured
                .stdout
                .contains("Class 'formula'  language uptake ~ conc | Plant")
                && captured.stdout.contains(".Environment")
                && captured.stdout.contains("R_EmptyEnv"),
            "formula str must match GNU Class/language header, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("- attr(*, \"labels\")=List of 2")
                && captured
                    .stdout
                    .contains("$ x: chr \"Ambient carbon dioxide concentration\"")
                && captured.stdout.contains("$ y: chr \"CO2 uptake rate\""),
            "named list attrs must be List of N with $ children, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("$ cov   : int [1:2, 1:2]")
                && captured
                    .stdout
                    .contains("  ..- attr(*, \"dimnames\")=List of 2")
                && captured.stdout.contains("$ center:"),
            "list matrix components must carry nested dimnames, got {:?}",
            captured.stdout
        );
        assert!(
            captured
                .stdout
                .contains("Date[1:2], format: \"2007-11-11\" NA")
                && captured.stdout.contains("num [1:2] 1.5 NA")
                && captured.stdout.contains("chr [1:2] \"MALE\" NA"),
            "Date/NA str must match GNU, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn summary_mixed_range_shares_common_decimals() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "options(digits=7); summary(c(1,100))\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains("1.00")
                && captured.stdout.contains("100.00")
                && captured.stdout.contains("50.50"),
            "named numeric summary must share format() decimals, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("   1 ")
                && !captured.stdout.split_whitespace().any(|w| w == "1"),
            "must not trim 1.00 to 1, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn options_max_print_inf_warns_then_errors() {
        let mut session = RSession::new();
        let (result, captured, _) = session.eval_script_with_output_capture(
            r#"
e1 <- tryCatch(options(max.print=Inf), error=function(e)e)
inherits(e1, "error")
"#,
        );
        let result = result.expect("max.print=Inf must error");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        let text = format!("{}{}", captured.stdout, captured.stderr);
        assert!(
            text.contains(
                "In options(max.print = Inf) : NAs introduced by coercion to integer range"
            ),
            "asInteger(Inf) must warn with the options() call like GNU, got stdout={:?} stderr={:?}",
            captured.stdout,
            captured.stderr
        );
    }

    #[test]
    fn try_catch_finally_runs_after_body() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "x <- character(); r <- tryCatch({ x <- c(x, \"b\"); 1L }, finally = { x <- c(x, \"f\") }); identical(r, 1L) && identical(x, c(\"b\", \"f\"))",
        );
        let result = result.expect("tryCatch finally must run after the body");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn deparse_pi_uses_dbl_dig_not_options_digits() {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_script_with_output_capture("identical(deparse(pi), \"3.14159265358979\")");
        let result = result.expect("deparse must pin R_print.digits to DBL_DIG");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn source_echo_deparses_expression_wrapper_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines('dPut <- function(x, control = c("quoteExpression", "showAttributes", "niceNames", "keepInteger")) dput(x, control = control)', f)
out <- capture.output(source(f, echo = TRUE))
identical(out[grepl("dPut <-", out)][1], '> dPut <- function(x, control = c("quoteExpression", ')
"#,
        );
        let result = result.expect("source(echo=TRUE) must wrap like GNU expression-deparse");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn source_echo_truncates_at_max_deparse_length() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines("hasReal <- function(x) { if (is.double(x) || is.complex(x)) !all((x == round(x, 3)) | is.na(x)) else if (is.logical(x) || is.integer(x) || is.symbol(x) || is.call(x) || is.environment(x) || is.character(x)) FALSE else FALSE }", f)
out <- paste(capture.output(source(f, echo = TRUE)), collapse = "\n")
grepl(" .... [TRUNCATED] ", out, fixed = TRUE)
"#,
        );
        let result = result.expect("source(echo=TRUE) must honor max.deparse.length=150");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn mean_of_empty_numeric_is_nan_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r"
            is.nan(mean(numeric(0))) &&
              is.nan(mean(rep(NA_real_, 2), trim = 0.1, na.rm = TRUE)) &&
              identical(mean(c(1:10, 100), trim = 0.1), 6)
            ",
        );
        let result = result.expect("mean(numeric(0)) must be NaN");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn catch_script_errors_prints_on_stderr_and_continues() {
        let mut session = RSession::new();
        let (result, captured, _) = session.eval_script_with_output_capture(
            r#"
            options(catch.script.errors = TRUE)
            stop("test of 'options(catch.script.errors = TRUE)'")
            TRUE
            "#,
        );
        let result = result.expect("catch.script.errors must continue after stop()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            !captured.stdout.contains("Error"),
            "GNU writes the caught error on stderr; stdout={:?}",
            captured.stdout
        );
        assert!(
            captured
                .stderr
                .contains("Error: test of 'options(catch.script.errors = TRUE)'"),
            "stderr={:?}",
            captured.stderr
        );
    }

    #[test]
    fn print_factor_pads_to_widest_label_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
            fx <- factor(c("a", NA, "b"), exclude="")
            stopifnot(identical(
                capture.output(print(fx)),
                c("[1] a    <NA> b   ", "Levels: a b <NA>")
            ))
            stopifnot(identical(withVisible(print(fx))$visible, FALSE))
            r <- replicate(3, capture.output(print(fx)))
            stopifnot(identical(dim(r), c(2L, 3L)))
            print.foo <- function(x) x
            stopifnot(identical(
                withVisible(print(structure(1, class="foo")))$visible,
                TRUE
            ))
            print.bar <- function(x) invisible(x)
            identical(withVisible(print(structure(1, class="bar")))$visible, FALSE)
            "#,
        );
        let result = result.expect("print.factor must pad to <NA> width");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn as_double_dispatches_s3_on_classed_numeric() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
            x <- structure(pi, class="testit")
            xx <- structure("OK", class="testOK")
            ff <- get("as.double", .GenericArgsEnv)
            body(ff) <- xx
            assign("as.double.testit", ff, .GlobalEnv)
            stopifnot(identical(as.double(x), xx))
            nn <- get("as.numeric", .GenericArgsEnv)
            body(nn) <- xx
            assign("as.numeric.testit", nn, .GlobalEnv)
            stopifnot(identical(as.numeric(x), xx))
            identical(do.call(as.numeric, list(x)), xx)
            "#,
        );
        let result = result.expect("as.double must dispatch S3 on classed numeric");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn print_table_1d_keeps_gnu_trailing_column_space() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
fx <- factor(c("a", NA, "b"), exclude="")
r <- replicate(3, capture.output(print(fx)))
identical(capture.output(print(table(r[2,]))), c("", "Levels: a b <NA> ", "               3 "))
"#,
        );
        let result = result.expect("print.table must emit GNU's trailing column space");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn source_max_deparse_length_inf_does_not_warn() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines("1+1", f)
invisible(source(f, echo = TRUE, max.deparse.length = Inf))
TRUE
"#,
        );
        let result = result.expect("source(max.deparse.length=Inf) must be legal");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            !output.stderr.contains("integer range") && !output.stdout.contains("integer range"),
            "Inf must not coerce through asInteger: stdout={:?} stderr={:?}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn s4_list_class_new_keeps_unnamed_data_part() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("mp1Port", slots = c(prec = "integer", d = "integer"))
setClass("mpPort", contains = "list")
m <- new("mpPort", list(new("mp1Port"), new("mp1Port", prec=1L, d=3:5)))
identical(length(m), 2L) && identical(typeof(m), "list")
"#,
        );
        let result = result.expect("new(list-class, list(...)) must keep .Data");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn complex_assign_does_not_leave_tmp_binding() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
e2 <- quote(c(a = 1, b = 2))
names(e2)[2] <- "a b c"
!exists("*tmp*", inherits = FALSE)
"#,
        );
        let result = result.expect("applydefine must unbind *tmp*");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn failed_subassign_does_not_leave_tmp_binding() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
test <- 1:10
tryCatch(test[2:4] <- ls, error = function(e) NULL)
!exists("*tmp*", inherits = FALSE)
"#,
        );
        let result = result.expect("failed [<- must not leave *tmp*");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn source_returns_withvisible_list_invisibly() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines("1+1", f)
r <- source(f, echo = FALSE)
identical(r, list(value = 2, visible = TRUE)) &&
  !withVisible(source(f, echo = FALSE))$visible
"#,
        );
        let result = result.expect("source() must return invisible(list(value, visible))");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn sys_source_returns_invisible_null() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- tempfile()
writeLines("1+1", f)
e <- new.env()
r <- sys.source(f, envir = e)
is.null(r) && !withVisible(sys.source(f, envir = e))$visible
"#,
        );
        let result = result.expect("sys.source() must return invisible NULL");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn identical_ignores_function_env_and_srcref_by_default_flags() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
A <- function(x, y, ...) {
    B <- function(a, b, ...) { match.call() }
    B(x+y, ...)
}
pd0 <- function(expr, backtick = TRUE, ...) parse(text = deparse(expr, backtick=backtick, ...))
id_epd <- function(expr, control = "all", ...) eval(pd0(expr, control=control, ...))
identical(A, id_epd(A), ignore.environment = TRUE, ignore.bytecode = TRUE, ignore.srcref = TRUE)
            "#,
        );
        let result = result.expect("check_EPD function identical must ignore env/srcref");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn all_equal_formula_ignores_environment() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
fm <- y ~ f(x)
pd0 <- function(expr, backtick = TRUE, ...) parse(text = deparse(expr, backtick=backtick, ...))
id_epd <- function(expr, control = "all", ...) eval(pd0(expr, control=control, ...))
isTRUE(all.equal(fm, id_epd(fm), check.environment = FALSE))
"#,
        );
        let result = result.expect("all.equal.formula must ignore .Environment");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn deparse_all_preserves_na_list_names() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
LNA <- setNames(as.list(c(1,2,99)), c("A", "NA", NA))
pd0 <- function(expr, backtick = TRUE, ...) parse(text = deparse(expr, backtick=backtick, ...))
identical(LNA, eval(pd0(LNA, control = "all")))
"#,
        );
        let result = result.expect("deparse(control=all) must keep NA list names");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn dput_quote_expression_wraps_language() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
dPut <- function(x, control = c("quoteExpression", "showAttributes", "niceNames", "keepInteger"))
    dput(x, control = control)
A <- function(x) { x }
identical(paste(capture.output(dPut(body(A))), collapse = "\n"), "quote({\n    x\n})")
"#,
        );
        let result = result.expect("dput(control=quoteExpression) must wrap language in quote()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn deparse_all_warns_when_sourceable_is_false() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
w <- character()
withCallingHandlers(
    deparse(y ~ x, control = "all"),
    warning = function(e) {
        w <<- conditionMessage(e)
        invokeRestart("muffleWarning")
    }
)
identical(w, "deparse may be incomplete")
"#,
        );
        let result = result.expect("deparse(control=all) on a formula must warn incomplete");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn summary_warnings_collapses_identical_deparse_warnings() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
options(warn = 0)
f <- function(x) deparse(x, control = "all")
invisible({
    f(y ~ x)
    f(a ~ b)
})
summary(warnings())
identical(class(summary(warnings())), "summary.warnings")
"#,
        );
        let result = result.expect("summary(warnings()) must run");
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "class(summary(warnings())) must be summary.warnings, output={output:?}"
        );
        assert!(
            output.stdout.contains("2 identical warnings:"),
            "expected collapsed identical-warnings header, got {output:?}"
        );
        assert!(
            output.stdout.contains("deparse may be incomplete"),
            "expected incomplete-deparse text, got {output:?}"
        );
        assert!(
            !output.stdout.contains("1x :"),
            "identical warnings must not print per-item 1x tags, got {output:?}"
        );
    }

    #[test]

    fn unlist_preserves_na_names_for_dput() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
LNA <- setNames(as.list(c(1, 2, 99)), c("A", "NA", NA))
iNA <- unlist(LNA)
identical(names(iNA), c("A", "NA", NA)) &&
  identical(paste(capture.output(dput(iNA)), collapse = "\n"),
            "structure(c(1, 2, 99), names = c(\"A\", \"NA\", NA))")
"#,
        );
        let result = result.expect("unlist must keep NA names so dput uses structure()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn all_equal_s4_formula_subclass_uses_language_path() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
mForm <- setClass("mFormAe", contains = "formula")
mf <- mForm(~ f(x))
pd0 <- function(expr, backtick = TRUE, ...) parse(text = deparse(expr, backtick=backtick, ...))
id_epd <- function(expr, control = "all", ...) eval(pd0(expr, control=control, ...))
isTRUE(all.equal(mf, id_epd(mf), check.environment = FALSE))
"#,
        );
        let result = result.expect("S4 formula subclass all.equal must follow language/deparse");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_class_representations_compare_slots() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("IdentA", contains = "formula")
setClass("IdentB", contains = "list")
!identical(getClass("IdentA"), getClass("IdentB"))
"#,
        );
        let result = result.expect("distinct S4 class defs must not be identical");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn formula_s4_subclass_uses_language_typeof() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
mForm <- setClass("mFormIdent", contains = "formula")
extends("mFormIdent", "oldClass") &&
  isS4(mf <- mForm(~ f(x))) &&
  identical(typeof(mf), "language") &&
  identical(mf, eval(parse(text = deparse(mf))))
"#,
        );
        let result = result.expect("S4 formula subclass must deparse/parse like GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_list_dput_includes_data_part() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("mp1Dput", slots = c(prec = "integer", d = "integer"))
setClass("mpDput", contains = "list")
m <- new("mpDput", list(new("mp1Dput", prec=1L, d=3:5)))
out <- paste(capture.output(dput(m)), collapse = "\n")
grepl(".Data", out, fixed = TRUE) && grepl("prec = 1L", out, fixed = TRUE)
"#,
        );
        let result = result.expect("dput of a list-class S4 object must emit .Data");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn capture_output_writes_local_text_connection() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(utils::capture.output(cat(\"hi\\n\")), \"hi\")",
        );
        let result = result.expect("capture.output must assign the local textConnection");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn capture_output_print_function_does_not_leak() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
out <- capture.output(print(function(fun, envir, value) NULL))
length(out) >= 1L && grepl("^function", out[1])
"#,
        );
        let result = result.expect("capture.output(print.function) must capture the signature");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            !output.stdout.contains("function(fun, envir, value)"),
            "print.function leaked past capture.output:\n{}",
            output.stdout
        );
    }

    #[test]
    fn character_matrix_colnames_left_align_like_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
m <- matrix(c("numeric", "ANY"), 2, 1, dimnames=list(c("target","defined"), "x"))
out <- capture.output(print(m))
identical(out[1], "        x        ")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "print.matrix colnames: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}",
            output.stdout
        );
    }

    #[test]
    fn print_character_matrix_honors_quote_false() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
x <- c(.Data="function", generic="character")
text <- format(c(names(x), as.character(x)), justify="right")
text <- matrix(text, nrow=2L, ncol=2L, byrow=TRUE)
dimnames(text) <- list(c("Name:", "Class:"), rep.int("", 2))
out <- capture.output(print(text, quote=FALSE))
identical(out, c(
  "                          ",
  "Name:      .Data   generic",
  "Class:  function character"
))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "quote=FALSE matrix: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn cat_file_stdout_writes_to_captured_stdout() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
out <- capture.output(cat("hello-stdout", file=stdout()))
identical(out, "hello-stdout")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "cat(file=stdout()): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn showclass_extends_uses_stdout_connection() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
out <- capture.output(showClass("standardGeneric"))
any(grepl('Class "genericFunction", directly', out, fixed=TRUE))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "showClass Extends: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn envrefclass_as_environment_uses_xdata() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
x <- new("envRefClass")
is.environment(x) &&
  identical(typeof(as.environment(x)), "environment") &&
  identical(typeof(x$show), "closure") &&
  identical(
    capture.output(show(x)),
    "Reference class object of class \"envRefClass\""
  )
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "envRefClass show: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn envrefclass_cold_show_and_print_match_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
x <- new("envRefClass")
gnu <- "Reference class object of class \"envRefClass\""
identical(capture.output(show(x)), gnu) &&
  identical(capture.output(print(x)), gnu)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "envRefClass cold show/print: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn s4_method_match_call_sees_user_call() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
setClass("m1", contains = "matrix")
setGeneric("foo", function(x, ...) standardGeneric("foo"))
setMethod("foo", "m1", function(x, ...) cat(" <m1> ", format(match.call()), "\n"))
identical(
  capture.output(foo(new("m1"), bla = TRUE)),
  " <m1>  foo(x = new(\"m1\"), bla = TRUE) "
)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S4 match.call: {e}\nstdout={:?}\nstderr={:?}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={:?}\nstderr={:?}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn asis_matrix_data_frame_subset_matches_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
d2 <- data.frame(b = I(matrix(1:6, 3, 2)))
identical(names(d2), "b") &&
  identical(dim(d2[2,]), c(1L, 2L)) &&
  identical(as.vector(d2[2,]), c(2L, 5L)) &&
  identical(d2[-1,], d2[2:3,]) &&
  identical(
    capture.output(d2[2,]),
    c("     [,1] [,2]", "[1,]    2    5")
  ) &&
  identical(
    capture.output(d2[1:2,]),
    c("     [,1] [,2]", "[1,]    1    4", "[2,]    2    5")
  )
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "AsIs matrix data.frame subset: {e}\nstdout={:?}\nstderr={:?}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={:?}\nstderr={:?}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn s4_bit_matrix_show_and_print_match_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
setClass("Foo", representation(name="character"), contains="matrix")
f <- new("Foo", name="Sam", matrix())
m <- as(f, "matrix")
m. <- asS4(m)
gnu <- c("     [,1]", "[1,]   NA")
identical(capture.output(show(m.)), gnu) &&
  identical(capture.output(print(m.)), gnu)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S4-bit matrix show/print: {e}\nstdout={:?}\nstderr={:?}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={:?}\nstderr={:?}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn num_with_id_xtfrm_matches_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
setClass("numWithId", representation(id = "character"), contains = "numeric")
x <- new("numWithId", 1:3, id = "An Example")
first <- trimws(capture.output(xtfrm(x)))
setMethod("xtfrm", "numWithId", function(x) x@.Data)
second <- trimws(capture.output(xtfrm(x)))
identical(first, c("[1] 1 2 3", 'attr(,"id")', '[1] "An Example"')) &&
  identical(second, "[1] 1 2 3") &&
  identical(xtfrm(x), 1:3)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "numWithId xtfrm: {e}\nstdout={:?}\nstderr={:?}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={:?}\nstderr={:?}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn str_s4_formal_class_matches_gnu() {
        // capture.output(str(<S4>)) drives deep bounded evaluator recursion;
        // debug frames exceed libtest's default thread stack (release and the
        // 8 MiB main-thread stacks are fine). Run on an explicit deep stack
        // like the owned-depth tests.
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(str_s4_formal_class_body)
            .unwrap()
            .join()
            .unwrap();
    }

    fn str_s4_formal_class_body() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
cn <- "integer or NULL"
setClassUnion(cn, members = c("integer", "NULL"))
setClass("c1", representation(x = "integer", code = cn))
nc <- new("c1", x = 1:2)
identical(
  capture.output(str(nc)),
  c(
    "Formal class 'c1' [package \".GlobalEnv\"] with 2 slots",
    "  ..@ x   : int [1:2] 1 2",
    "  ..@ code: NULL"
  )
)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S4 str Formal class: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn gnu_signature_str_has_names_and_package_slots() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
sig <- new("signature", obj = "mle")
out <- capture.output(str(sig))
identical(
  out,
  c(
    "Formal class 'signature' [package \"methods\"] with 3 slots",
    "  ..@ .Data  : chr \"mle\"",
    "  ..@ names  : chr \"obj\"",
    "  ..@ package: chr \"\""
  )
)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "signature str: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(
            result.logical_elt(0),
            Some(TRUE),
            "stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
    }

    #[test]
    fn message_writes_stderr_not_stdout() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
out <- capture.output(message("hello-msg"))
identical(out, character(0))
"#,
        );
        let result = result.expect("capture.output must not swallow message()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            !output.stdout.contains("hello-msg"),
            "message() leaked onto stdout:\n{}",
            output.stdout
        );
        assert!(
            output.stderr.contains("hello-msg"),
            "message() must appear on stderr:\n{}",
            output.stderr
        );
    }

    #[test]
    fn with_autoprint_capture_output_splits_gnu_lines() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
CO <- utils::capture.output
out <- CO(withAutoprint({ x <- 1:2; cat("x=", x, "\n") }))
identical(out[1], paste0(getOption("prompt"), "x <- 1:2")) &&
  length(out) >= 3L &&
  identical(out[3], "x= 1 2 ")
"#,
        );
        let result = result.expect("withAutoprint capture.output must emit GNU lines");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn with_autoprint_is_gnu_source_wrapper() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
is.function(withAutoprint) &&
  identical(names(formals(withAutoprint))[1:3], c("exprs", "evaluated", "local")) &&
  grepl("source(", paste(deparse(body(withAutoprint)), collapse = "\n"), fixed = TRUE)
"#,
        );
        let result = result.expect("withAutoprint must be GNU's source() wrapper");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn unlist_recursive_false_keeps_list_of_lists() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
u <- unlist(list(list(.Data = "list"), list()), recursive = FALSE)
identical(typeof(u), "list") && identical(names(u), ".Data") && identical(u$.Data, "list")
"#,
        );
        let result = result.expect("unlist(recursive=FALSE) must concatenate lists like GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn set_class_contains_list_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly = TRUE))
setClass("mListPort", contains = "list")
s <- getClass("mListPort")@slots
identical(typeof(s), "list") && identical(names(s), ".Data") && identical(as.character(s$.Data), "list")
"#,
        );
        let result = result.expect("setClass(contains='list') must use GNU methods::setClass");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn unlist_recursive_false_splices_mixed_atomic_and_list() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
u <- unlist(list(1:2, list(3)), recursive = FALSE)
identical(typeof(u), "list") && identical(as.numeric(unlist(u)), c(1, 2, 3))
"#,
        );
        let result = result.expect("unlist(recursive=FALSE) must splice atomics like GNU c()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn example_uses_unevaluated_topic_name() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly = TRUE))
local({
  new <- 42
  w <- tryCatch(
    example(new),
    warning = function(e) conditionMessage(e),
    error = function(e) conditionMessage(e)
  )
  msg <- paste(as.character(w), collapse = " ")
  !grepl("42", msg, fixed = TRUE) && (is.character(w) && grepl("new", msg, fixed = TRUE) || !is.character(w))

})
"#,




        );
        let result = result.expect("example(new) must look up topic new, not evaluate new");
        assert_eq!(result.logical_elt(0), Some(TRUE), "output={output:?}");
    }

    #[test]
    fn find_package_null_lists_attached_methods() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly = TRUE))
any(grepl("/methods$", find.package(NULL)))
"#,
        );
        let result = result.expect("find.package(NULL) must list attached package paths");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn utils_namespace_loads_without_windows_s3_methods() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "is.environment(asNamespace(\"utils\")) && is.function(utils::getAnywhere)",
        );
        let result = result.expect("utils namespace must load with lazy S3 methods");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rep_is_gnu_special() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(typeof(rep), \"special\") && identical(rep(1L, 3L), c(1L, 1L, 1L))",
        );
        let result = result.expect("rep must be GNU's special");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_definition_data_slot_is_the_closure() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"MethodDefinition\"); isS4(x) && typeof(x) == \"closure\" && is.function(x@.Data) && isTRUE(validObject(x))",
        );
        let result = result.expect("MethodDefinition@.Data is the closure like GNU getDataPart");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_setmethod_dispatches_on_s4_class() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"foo\", representation(x=\"numeric\", y=\"numeric\")); xx <- new(\"foo\", x=1, y=2); ff <- args(getGeneric(\"$\")); body(ff) <- \"testit\"; setMethod(\"$\", \"foo\", ff); identical(getGeneric(\"$\")(xx), \"testit\") && identical(xx$x, \"testit\")",
        );
        let result = result.expect("setMethod + $ dispatch must match GNU primitives.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_match_signature_for_subset_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); s <- matchSignature(\"foo\", getGeneric(\"[\")); identical(as.character(s), \"foo\") && identical(names(s), \"x\")",
        );
        let result = result.expect("matchSignature([) must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_setmethod_subset_generic_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"foo\", representation(x=\"numeric\", y=\"numeric\")); xx <- new(\"foo\", x=1, y=2); ff <- args(getGeneric(\"[\")); body(ff) <- \"testit\"; setMethod(\"[\", \"foo\", ff); identical(getGeneric(\"[\")(xx), \"testit\") && identical(xx[], \"testit\")",
        );
        let result = result.expect("setMethod([) must match GNU primitives.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_setmethod_double_bracket_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"foo\", representation(x=\"numeric\", y=\"numeric\")); xx <- new(\"foo\", x=1, y=2); ff <- args(getGeneric(\"[[\")); body(ff) <- \"testit\"; setMethod(\"[[\", \"foo\", ff); identical(getGeneric(\"[[\")(xx), \"testit\") && identical(xx[[\"x\"]], \"testit\")",
        );
        let result = result.expect("setMethod([[) must match GNU primitives.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn methods_setmethod_caret_ops_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("foo", representation(x="numeric"))
xx <- new("foo", x=1)
ff <- args(getGeneric("^"))
body(ff) <- "testit"
setMethod("^", "foo", ff)
g <- getGeneric("^")
s <- methods:::.matchSigLength(matchSignature("foo", g), g, environment(g), TRUE)
identical(as.character(s), c("foo","ANY")) &&
  isTRUE(all(c("ANY#ANY","foo#ANY") %in% ls(environment(g)$.MTable, all.names=TRUE))) &&
  identical(g(xx), "testit") &&
  identical(xx^2, "testit")
"#,
        );
        let result = result.expect("setMethod(^) must store foo#ANY and dispatch like GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn primitives_internal_generics_dispatch_s3_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
x <- structure(pi, class="testit")
xx <- structure("OK", class="testOK")
ok <- TRUE
for (f in c("cumvar", "dim", "dimnames", "xtfrm")) {
  method <- paste(f, "testit", sep=".")
  ff <- get(f, .GenericArgsEnv)
  body(ff) <- xx
  assign(method, ff, .GlobalEnv)
  res <- eval(substitute(ff(x), list(ff=as.name(f))))
  ok <- ok && identical(res, xx)
  rm(list=method, envir=.GlobalEnv)
}
assign("levels<-.testit", function(x, value) xx, .GlobalEnv)
y <- x
ok <- ok && identical(eval(substitute(`levels<-`(y, value=pi))), xx)
ok
"#,
        );
        let result = result.expect("internal generics must UseMethod like GNU primitives.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn unlist_as_vector_lengths_are_gnu_closures() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("foo", representation(x="numeric", y="numeric"))
xx <- new("foo", x=1, y=2)
ff <- args(getGeneric("unlist"))
body(ff) <- "testit"
setMethod("unlist", "foo", ff)
identical(typeof(unlist), "closure") &&
  identical(typeof(as.vector), "closure") &&
  identical(typeof(lengths), "closure") &&
  identical(unlist(list(1, 2:3)), c(1, 2, 3)) &&
  identical(as.vector(c(a=1), "any"), 1) &&
  identical(as.integer(lengths(list(1:2, 3))), c(2L, 1L)) &&
  identical(getGeneric("unlist")(xx), "testit")
"#,
        );
        let result = result.expect("unlist/as.vector/lengths must be GNU closures");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn stop_pastes_arguments_and_primitives_reject_wrong_names() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
sm <- tryCatch(stop("failure on ", "abs"), error=function(e) conditionMessage(e))
am <- tryCatch(do.call(abs, list(zZ=NULL)), error=function(e) conditionMessage(e))
nm <- tryCatch(do.call(nargs, list(zZ=NULL)), error=function(e) conditionMessage(e))
grepl("failure on abs", sm, fixed=TRUE) &&
  grepl("does not match|unused argument", am) &&
  grepl("requires 0", nm, fixed=TRUE)
"#,
        );
        let result = result.expect("stop paste and primitive name checks must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn exists_honors_envir_and_implicit_summary_generics() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
e <- new.env(parent=emptyenv())
invisible(require(methods, quietly=TRUE))
g <- getGeneric("sum")
!exists("sum", envir=e, inherits=FALSE) &&
  !exists("sum", envir=e, inherits=TRUE) &&
  !exists("sum", envir=.GlobalEnv, inherits=FALSE) &&
  isTRUE(exists("sum", envir=.GlobalEnv, inherits=TRUE)) &&
  isTRUE(exists("sum", envir=baseenv(), inherits=FALSE)) &&
  !is.primitive(g) &&
  identical(names(formals(g)), c("x", "...", "na.rm")) &&
  is(g, "genericFunction")
"#,
        );
        let result = result.expect("exists() and getGeneric(sum) must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn compiled_remove_source_keeps_extracted_missing_formals() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- function(z) is.name(z) && !missing(z)
g <- utils::removeSource(function(x) 1)
isTRUE(f(formals(function(x) x)[[1]])) &&
  identical(names(formals(g)), "x") &&
  identical(g(1), 1)
"#,
        );
        let result = result.expect("GETVAR must return a supplied empty-name value");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
    #[test]
    fn implicit_norm_generic_is_standard_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
g <- implicitGeneric("norm")
isS4(g) && is(g, "standardGeneric") &&
  identical(as.character(g@generic)[1], "norm") &&
  identical(as.character(attr(g, "generic"))[1], "norm")
"#,
        );
        let result = result.expect("methods must register the implicit norm generic");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
}
