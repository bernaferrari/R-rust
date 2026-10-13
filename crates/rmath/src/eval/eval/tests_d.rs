//! eval tests (part 4/4) — extracted verbatim from eval/eval.rs

use super::*;
use crate::sexp::builder::scalar_integer_in;
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_lang2};
use crate::sexp::session::RSession;
use crate::sexp::symbol::Rf_install;

#[cfg(test)]
mod tests_d {
    use super::*;

    #[test]
    fn reg_s4_callgeneric_do_call() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("fun", function(x, ...) standardGeneric("fun"))
setMethod("fun", "character", identity)
setMethod("fun", "numeric", function(x) {
  x <- as.character(x)
  callGeneric()
})
identical(fun(1), do.call(fun, list(1)))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "callGeneric: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_source_textconnection_srcref() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
source(textConnection("x <- 42L\n"))
identical(x, 42L)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "source textConnection: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_getsrcref_on_sourced_function() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
source(textConnection("f <- function(x) x\n"), keep.source = TRUE)
!is.null(getSrcref(f)) &&
  identical(paste(as.character.srcref(getSrcref(f)), collapse = "\n"), "function(x) x")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "getSrcref: {e}\nstdout={}\nstderr={}",
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
    fn gnu_getsrcref_rematched_method_prints_source() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
text <- '
setClass("MyClass", representation(val = "numeric"))
setMethod("initialize", signature = "MyClass",
    function(.Object, value) {
        # comment
	.Object@val <- value
	return(.Object)
    })
'
source(textConnection(text), keep.source = TRUE)
out <- paste(capture.output(getSrcref(getMethod("initialize", "MyClass"))), collapse = "\n")
grepl('# comment', out, fixed = TRUE) && grepl('.Object@val', out, fixed = TRUE)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "getSrcref rematch: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_help_try_and_identical_s4_bit() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Foo", representation(name="character"), contains="matrix")
f <- new("Foo", name="Sam", matrix())
m <- as(f, "matrix")
foo_ok <- isS4(m. <- asS4(m)) && identical(m, f@.Data) && .hasSlot(f, "name") && !isS4(m)
a <- 1:5
b <- setClass("B", "integer")(a)
eq_ok <- is.character(all.equal(a, b))
attributes(a) <- attributes(b)
mismatch_ok <- if (!isS4(a)) !identical(a, b) else TRUE
if (!isS4(a)) a <- asS4(a)
foo_ok && eq_ok && mismatch_ok && identical(a, b) && isS4(a)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "identical S4 bit: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_rbind2_a_and_its_matrix() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(a = "matrix"))
setMethod("initialize", signature(.Object = "A"),
    function(.Object, y) {
      .Object@a <- y
      .Object
    })
setMethod("rbind2", signature(x = "A", y = "matrix"),
    function(x, y, ...) {
      x@a <- rbind(x@a, y)
      x
    })
setMethod("dim", "A", function(x) dim(x@a))
mat1 <- matrix(1:9, nrow = 3)
obj1 <- new("A", 10*mat1)
om1 <- rbind(obj1, mat1)
a_ok <- identical(om1, rbind2(obj1, mat1))
removeClass("A")
setClass("its", representation("matrix", dates="POSIXt"))
m <- outer(1:3, setNames(1:5, LETTERS[1:5]))
im <- new("its", m, dates=as.POSIXct(Sys.Date()))
ii  <- rbind(im, im-1)
i.i <- cbind(im, im-7)
its_ok <- identical(m, im@.Data) &&
  identical(m, rbind(im)) && identical(m, cbind(im)) &&
  identical(ii, rbind(m, m-1)) && identical(i.i, cbind(m, m-7))
a_ok && its_ok
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "rbind2/its: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_mondate_cbind_colnames() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("mondate",
         slots = c(timeunits = "character"), contains = "numeric")
m1 <- new("mondate", 1:4, timeunits = "hrs")
m2 <- new("mondate", 7:8, timeunits = "min")
identical(colnames(cbind(m1+1, deparse.level=2)), "m1 + 1") &&
  is.null(colnames(cbind(m1+1, deparse.level=0))) &&
  is.null(colnames(cbind(m1+1, deparse.level=1))) &&
  identical(colnames(cbind(m1)), "m1") &&
  identical(colnames(cbind(m1, M2 = 2, deparse.level=0)), c("", "M2")) &&
  identical(colnames(cbind(m1, M2 = 2)), c("m1", "M2"))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "mondate cbind: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_formula_and_dataframe_slots() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(slot1="numeric", slot2="logical"))
setClass("D1", contains="A", representation(design="data.frame"))
setClass("D2", contains="D1")
validObject(a <- new("A", slot1=77, slot2=TRUE))
validObject(D. <- new("D2", a, design = data.frame(x = 1)))
setClass("B", contains="A", representation(design="formula"))
setClass("C", contains="B")
a <- new("A", slot1=77, slot2=TRUE)
validObject(C1 <- new("C", a, design = x ~ y))
C2 <- new("C", slot1=a@slot1, slot2=a@slot2, design=x ~ y)
identical(C1, C2) &&
  identical(formula(), formula(NULL)) &&
  length(N <- new("formula")) == 0 && inherits(N, "formula") &&
  length(N <- new("table")) == 0 && is.table(N) &&
  validObject(N <- new("summary.table")) &&
  length(N <- new("ordered")) == 0 && is.ordered(N)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "formula slots: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_stats4_mle_classes() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
ok <- require("stats4", quietly=TRUE)
if (!isTRUE(ok)) {
  FALSE
} else {
  validObject(sig <- new("signature", obj = "mle"))
  cl4 <- getClasses("package:stats4")
  all(c("package", "names") %in% slotNames(sig)) &&
    identical(getClasses(which(search() == "package:stats4")), cl4) &&
    all(c("mle", "profile.mle", "summary.mle") %in% cl4)
}
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "stats4: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_signature_obj_after_removeclass() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(concat!(
            "invisible(require(methods, quietly=TRUE))\n",
            "setClass(\"SIG\", contains=\"signature\")\n",
            "invisible(lapply(getClasses(globalenv()), removeClass))\n",
            "validObject(new(\"signature\", obj = \"mle\"))\n",
        ));
        let result = result.unwrap_or_else(|e| {
            panic!(
                "signature after removeClass: {e}\nstdout={}\nstderr={}",
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
    fn exists_search_name_is_where_not_mode() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(concat!(
            "invisible(require(methods, quietly=TRUE))\n",
            "exists(\".__C__signature\", \"package:methods\", inherits=FALSE) &&\n",
            "  exists(\"pi\", \"package:base\") &&\n",
            "  !exists(\".__C__signature\", \".GlobalEnv\", inherits=FALSE)\n",
        ));
        let result = result.unwrap_or_else(|e| {
            panic!(
                "exists where-string: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_slot_assign_without_methods() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(slot1="numeric", slot2="logical"))
setClass("B", contains="A", representation(design="formula"))
setClass("C", contains="B")
a <- new("A", slot1=77, slot2=TRUE)
C1 <- new("C", a, design = x ~ y)
detach("package:methods", force=TRUE)
C1@slot1 <- pi
identical(C1@slot1, pi)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "slot assign without methods: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_generic_warning_handler_pr16111() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
f <- function() {
    signal <- FALSE
    withCallingHandlers({ g(sqrt(-1)) }, warning = function(w) {
        signal <<- TRUE
        invokeRestart("muffleWarning")
    })
    signal
}
g <- function(x) x
op <- options(warn = 2)
ok1 <- isTRUE(f())
setGeneric("g")
ok2 <- isTRUE(f())
options(op)
ok1 && ok2
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "PR16111: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_as_vector_unlist_generic_formals() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
identical(formals(getGeneric("as.vector")), formals(base::as.vector)) &&
  identical(formals(getGeneric("unlist")), formals(base::unlist))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "as.vector/unlist formals: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_virtual_integer_subclass() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("myInteger", contains=c("integer", "VIRTUAL"))
setClass("mySubInteger", contains="myInteger")
x <- new("mySubInteger", 1L)
y <- new("mySubInteger")
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "virtual integer: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_s3_dispatch_without_methods_attached() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
methods::setClass("test1", methods::representation(date="POSIXct"))
methods::setClass("test2", contains="test1")
test <- function(x) UseMethod("test", x)
test.test1 <- function(x) "Hi"
detach("package:methods", force=TRUE)
out <- test(methods::new("test2", date=as.POSIXct("2003-10-09")))
identical(out, "Hi")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S3 dispatch without methods: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_coerce_method_uses_methods_new() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", slots = c(foo = "numeric"))
setClass("Ap", contains = "A", slots = c(p = "character"))
cd <- getClassDef("Ap")
identical(deparse(body(cd@contains[["A"]]@coerce)[[2]])[1], "value <- methods::new(\"A\")")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "coerce method body: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_resolve_class_list_package_conflict() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("htest1", slots=c(a="numeric",b="data.frame"), package="package1")
setClass("htest2", slots=c(a="logical"), package="package2")
class_env1 <- .GlobalEnv
class_env2 <- .GlobalEnv
class.list = list(
    package1=getClassDef("htest1", where=class_env1),
    package2=getClassDef("htest2", where=class_env2)
)
firstclass  <- methods:::.resolveClassList(class.list,.GlobalEnv, package="package1")
secondclass <- methods:::.resolveClassList(class.list,.GlobalEnv, package="package2")
alsofirstclass <- methods:::.resolveClassList(class.list,.GlobalEnv, package="package3")
!identical(firstclass, secondclass) &&
  identical(firstclass, class.list[[1]]) &&
  identical(secondclass, class.list[[2]]) &&
  identical(alsofirstclass, class.list[[1]])
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "resolveClassList: {e}\nstdout={}\nstderr={}",
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
    fn cbind2_default_negative_deparse_level_does_not_redispatch() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Num", contains="numeric")
a <- new("Num", 1:3)
identical(as.vector(cbind(a)), 1:3) && identical(as.vector(cbind2(a)), 1:3)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "cbind2 -1L: {e}\nstdout={}\nstderr={}",
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
    fn show_print_s4_bit_on_matrix_does_not_recurse() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Foo", representation(name="character"), contains="matrix")
(f <- new("Foo", name="Sam", matrix()))
m <- as(f, "matrix")
stopifnot(isS4(m. <- asS4(m)), identical(m, f@.Data), .hasSlot(f, "name"))
show(m.)
print(m.)
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "show/print S4 matrix: {e}\nstdout={}\nstderr={}",
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
        assert!(
            output.stdout.contains("[,1]") || output.stdout.contains("NA"),
            "expected matrix PrintValueRec, got stdout={}",
            output.stdout
        );
        assert!(
            !output.stdout.contains("An object of class \"S4\""),
            "recursion-guard stub still printed: {}",
            output.stdout
        );
    }

    #[test]
    fn cbind_mixed_s4_and_atomic_uses_cbind2_default() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Num", contains="numeric")
a <- new("Num", 1:3)
r <- cbind(a, 4)
identical(dim(r), c(3L, 2L)) && identical(as.vector(r)[1:3], c(1, 2, 3))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "cbind mixed: {e}\nstdout={}\nstderr={}",
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
    fn rbind_null_then_data_frame_binds_columns() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r"
df <- data.frame(a = 1:2)
r <- rbind(NULL, df)
identical(r$a, 1:2)
",
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "rbind NULL df: {e}\nstdout={}\nstderr={}",
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
    fn cbind_untagged_symbol_names_the_column() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
x <- 1:3
identical(colnames(cbind(x)), "x") && identical(rownames(rbind(x)), "x")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "cbind names: {e}\nstdout={}\nstderr={}",
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
    fn language_subset_minus_one_names_assign() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
f <- function(x, extrarg = TRUE) NULL
cl <- quote(Gfun(m2))
mc <- match.call(f, cl, expand.dots = FALSE)
stopifnot(identical(deparse(mc), "Gfun(x = m2)"))
stopifnot(identical(names(mc[-1L]), "x"))
mc[-1L] <- lapply(names(mc[-1L]), as.name)
stopifnot(identical(deparse(mc), "Gfun(x = x)"))
stopifnot(identical(deparse(`names<-`(quote(f(x = 1)), NULL)), "f(1)"))
e <- quote(f(x = 1))
names(e) <- NULL
stopifnot(identical(deparse(e), "f(1)"))
qq <- quote(f(a))
err <- tryCatch({ qq[-1L] <- list(); "NOERROR" }, error = function(e) e$message)
stopifnot(identical(err, "replacement has length zero"))
TRUE

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "lang subset names assign: {e}\nstdout={}\nstderr={}",
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
    fn is_namespace_loaded_reports_base() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(isNamespaceLoaded("base"), TRUE) &&
  identical(isNamespaceLoaded("no_such_namespace_zzz"), FALSE)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "isNamespaceLoaded: {e}\nstdout={}\nstderr={}",
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
    fn aic_pfit_uses_s3_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
pfit <- function(data) {
    m <- mean(data)
    loglik <- sum(dpois(data, m))
    ans <- list(par = m, loglik = loglik)
    class(ans) <- "pfit"
    ans
}
AIC.pfit <- function(object, ..., k = 2) -2 * object$loglik + k
identical(AIC(pfit(1:10)), AIC.pfit(pfit(1:10)))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "AIC.pfit: {e}\nstdout={}\nstderr={}",
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
    fn unlist_of_logicals_stays_logical() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(unlist(list(TRUE, FALSE))), "logical") &&
  identical(unlist(list(TRUE, FALSE)), c(TRUE, FALSE)) &&
  identical(typeof(unlist(list(TRUE, 1L))), "integer")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "unlist logicals: {e}\nstdout={}\nstderr={}",
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
    fn s4_containing_array_and_ts() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
t. <- ts(1:10, frequency = 4, start = c(1959, 2))
setClass("Arr", contains = "array")
x <- new("Arr", cbind(17))
setClass("Ts", contains = "ts")
tt <- new("Ts", t.)
t2 <- as(t., "Ts")
setClass("ts2", representation(x = "Ts", y = "ts"))
tt2 <- new("ts2", x = t2, y = t.)
stopifnot(isTRUE(all.equal(getOption("ts.eps"), 1e-5)),
          dim(x) == c(1, 1),
          is(tt, "ts"), is(t2, "ts"),
          length(tt) == length(t.),
          identical(tt2@x, t2), identical(tt2@y, t.))
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "Arr/Ts classes: {e}\nstdout={}\nstderr={}",
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
    fn setmethod_wrong_formals_order_signals() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(tools, quietly=TRUE))
setGeneric("test1", function(x, printit = TRUE, name = "tmp")
           standardGeneric("test1"))
tryCatch({
  tools::assertCondition(
    setMethod("test1", "numeric", function(x, name, printit) match.call()),
    "warning", "error")
  TRUE
}, error = function(e) FALSE)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "setMethod formals: {e}\nstdout={}\nstderr={}",
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
    fn stats4_getclass_mle_from_where() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(stats4, quietly=TRUE))
c1 <- getClass("mle", where = "stats4")
c2 <- getClass("mle", where = "package:stats4")
s1 <- getMethod("summary", "mle", where = "stats4")
s2 <- getMethod("summary", "mle", where = "package:stats4")
!is.null(methods:::.getClassesFromCache("mle")) &&
  is(c1, "classRepresentation") &&
  is(s1, "MethodDefinition") &&
  identical(c1, c2) && identical(s1, s2) &&
  is(getClass("mle"), "classRepresentation")

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "stats4 mle where: {e}\nstdout={}\nstderr={}",
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
    fn attributes_null_clears_s4_bit() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("moo", representation("matrix"))
x <- new("moo", .Data = matrix(1:4, 2))
attributes(x) <- NULL
a <- !isS4(x)
y <- new("moo", .Data = matrix(1:4, 2))
attributes(y) <- list()
a && !isS4(y)

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "attributes<- NULL S4: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_moo_matrix_data_slot() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("moo", representation("matrix"))
m <- matrix(1:4, 2, dimnames= list(NULL, c("A","B")))
nf <- new("moo", .Data = m)
n2 <- new("moo", 3:1, 3,2)
n3 <- new("moo", 1:6, ncol=2)
identical(m, as(nf, "matrix")) &&
  identical(matrix(3:1,3,2), as(n2, "matrix")) &&
  identical(matrix(1:6,ncol=2), as(n3, "matrix"))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "moo matrix .Data: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_arr_ts_contains() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
t. <- ts(1:10, frequency = 4, start = c(1959, 2))
setClass("Arr", contains= "array"); x <- new("Arr", cbind(17))
setClass("Ts",  contains= "ts");   tt <- new("Ts", t.); t2 <- as(t., "Ts")
setClass("ts2", representation(x = "Ts", y = "ts"))
tt2 <- new("ts2", x=t2, y=t.)
all(dim(x) == c(1,1)) && is(tt, "ts") && is(t2, "ts") &&
  length(tt) == length(t.) &&
  identical(tt2@x, t2) && identical(tt2@y, t.)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "Arr/Ts contains: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_rbind_generic_dots() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
a <- identical(rbind(1), matrix(1,1,1))
setGeneric("rbind", function(..., deparse.level=1)
	   standardGeneric("rbind"), signature = "...")
a && identical(rbind(1), matrix(1,1,1))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "rbind generic dots: {e}\nstdout={}\nstderr={}",
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
    fn gnu_order_is_closure_with_formals() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(names(formals(order)), c("...", "na.last", "decreasing", "method")) &&
  !is.primitive(order) &&
  identical(order(c(3, 1, 2)), c(2L, 3L, 1L))

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "gnu order formals: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_order_setgeneric_dots() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("order", signature="...",
	   function (..., na.last=TRUE, decreasing=FALSE)
	   standardGeneric("order"))
identical(rbind(1), matrix(1,1,1))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "order setGeneric dots: {e}\nstdout={}\nstderr={}",
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
    fn getgenerics_stats4_lists_exported_generics() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(stats4, quietly=TRUE))
e4 <- as.environment("package:stats4")
gg4 <- getGenerics(e4)
em <- as.environment("package:methods")
ggm <- getGenerics(em)
gms <- c("addNextMethod", "body<-", "cbind2", "initialize",
	 "loadMethod", "Ops", "rbind2", "show")
stopifnot(c("BIC", "coef", "confint", "logLik", "plot", "profile",
            "show", "summary", "update", "vcov") %in% gg4,
          unlist(lapply(gg4, function(g) !is.null(getGeneric(g, where = e4)))),
          unlist(lapply(gg4, function(g) !is.null(getGeneric(g)))),
          isGeneric("show", where=e4),
          hasMethods("show", where=e4),
          unlist(lapply(ggm, function(g) !is.null(getGeneric(g, where = em)))),
          gms %in% ggm,
          gms %in% tools:::get_S4_generics_with_methods(em),
          identical(as.character(gg4),
                    tools:::get_S4_generics_with_methods(e4)))
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "getGenerics(stats4): {e}\nstdout={}\nstderr={}",
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
    fn as_double_uses_as_numeric_s4_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("c1", "numeric")
setClass("c2", "numeric")
x_c1 <- new("c1")
setMethod("as.numeric", "c1", function(x, ...) 42+pi)
setMethod(as.double, "c2", function(x, ...) x@.Data+pi)
x_c2 <- new("c2", pi)
identical(as.numeric(x_c1), as.double(x_c1)) &&
  identical(as.double(x_c1), 42+pi) &&
  identical(as.numeric(x_c2), as.double(x_c2)) &&
  isTRUE(all.equal(as.vector(as.numeric(x_c2)), pi + pi))


"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "as.double/as.numeric: {e}\nstdout={}\nstderr={}",
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
    fn aic_pfit_survives_stats4_generic() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(stats4, quietly=TRUE))
pfit <- function(data) {
    m <- mean(data)
    loglik <- sum(dpois(data, m))
    ans <- list(par = m, loglik = loglik)
    class(ans) <- "pfit"
    ans
}
AIC.pfit <- function(object, ..., k = 2) -2 * object$loglik + k
identical(AIC(pfit(1:10)), AIC.pfit(pfit(1:10)))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "AIC.pfit stats4: {e}\nstdout={}\nstderr={}",
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
    fn callgeneric_after_unclass_uses_default() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("Gfun", function(x, ...) standardGeneric("Gfun"),
           useAsDefault = function(x, ...) sum(x, ...))
setClass("mmat2", contains="matrix")
setMethod(Gfun, signature(x = "mmat2"),
          function(x, extrarg = TRUE) {
              x <- unclass(x)
              callGeneric()
          })
m2 <- new("mmat2", matrix(1:12, 3,4))
identical(Gfun(m2), 78L)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "callGeneric: {e}\nstdout={}\nstderr={}",
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
    fn callgeneric_passes_extra_formals() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("Gfun", function(x, ...) standardGeneric("Gfun"),
           useAsDefault = function(x, ...) sum(x, ...))
setClass("mmat2", contains="matrix")
setMethod(Gfun, signature(x = "mmat2"),
          function(x, extrarg = TRUE) {
              x <- unclass(x)
              callGeneric()
          })
m2 <- new("mmat2", diag(3))
identical(Gfun(m2, extrarg = FALSE), 3)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "callGeneric extrarg: {e}\nstdout={}\nstderr={}",
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
    fn rematch_definition_wraps_extra_formals_in_local() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("Gfun", function(x, ...) standardGeneric("Gfun"),
           useAsDefault = function(x, ...) sum(x, ...))
setClass("mmat2", contains="matrix")
setMethod(Gfun, signature(x = "mmat2"),
          function(x, extrarg = TRUE) {
              x <- unclass(x)
              callGeneric()
          })
isRematched(getMethod("Gfun", "mmat2"))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "rematch: {e}\nstdout={}\nstderr={}",
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
    fn hashed_env_names_include_hash_bindings() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
e <- new.env(hash=TRUE, parent=emptyenv())
assign("brob#ANY", 1, envir=e)
assign(".hidden", 2, envir=e)
identical(sort(names(e)), sort(c("brob#ANY", ".hidden")))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "hashed env names: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn is_na_preserves_matrix_dim() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r"
m <- matrix(c(1L, NA_integer_, 3L, 4L), 2, 2)
identical(dim(is.na(m)), c(2L, 2L)) &&
  identical(as.integer(colSums(is.na(m))), c(1L, 0L))
",
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "is.na dim: {e}\nstdout={}\nstderr={}",
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
    fn logic_group_selectmethod_inherited_and() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("brob", contains="numeric")
logic2 <- function(e1,e2) e1
setMethod("Logic", signature("brob", "ANY"), logic2)
setMethod("Logic", signature("ANY", "brob"), logic2)
m <- selectMethod("&", c("brob","brob"), optional=TRUE)
is(m, "MethodDefinition")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "selectMethod &: {e}\nstdout={}\nstderr={}",
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
    fn logic_group_brob_and_errors() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
stopifnot(all(getGroupMembers("Logic") %in% c("&", "|")),
          any(getGroupMembers("Ops") == "Logic"))
setClass("brob", contains="numeric")
b <- new("brob", 3.14)
logic.brob.error <- function(nm)
    stop("logic operator '", nm, "' not applicable to brobs")
logic2 <- function(e1,e2) logic.brob.error(.Generic)
setMethod("Logic", signature("brob", "ANY"), logic2)
setMethod("Logic", signature("ANY", "brob"), logic2)
generic_ok <- isTRUE(tryCatch({ getGeneric("&")(b, b); FALSE }, error=function(e)
    grepl("not applicable to brobs", conditionMessage(e), fixed=TRUE)))
prim_ok <- isTRUE(tryCatch({ b & b; FALSE }, error=function(e)
    grepl("not applicable to brobs", conditionMessage(e), fixed=TRUE)))
generic_ok && prim_ok
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "logic group brob: {e}\nstdout={}\nstderr={}",
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
    fn stats4_hasmethods_coef_after_require() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
invisible(require(stats4, quietly=TRUE))
isTRUE(isGeneric("coef")) && isTRUE(hasMethods("coef"))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "stats4 hasMethods(coef): {e}\nstdout={}\nstderr={}",
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
    fn as_environment_null_is_defunct_like_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
inherits(tryCatch(as.environment(NULL), error=function(e) e), "error") &&
  grepl("as.environment\\(NULL\\)' is defunct",
        tryCatch(as.environment(NULL), error=function(e) conditionMessage(e)))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "as.environment(NULL): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn print_is_usemethod_closure_and_primitive_builtins_match_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(print), "closure") &&
  identical(typeof(sum), "builtin") &&
  isTRUE(is.primitive(sum)) &&
  isTRUE(!is.primitive(print)) &&
  isTRUE(!is.primitive(function(x) x))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "print/sum primitive: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setmethod_print_on_s4_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("bar", representation(a="numeric"))
setMethod("print", "bar", function(x, ...) cat("S4 print method\n"))
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "setMethod(print): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_autoprint_uses_show_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("bar", representation(a="numeric"))
foo <- new("bar", a=pi)
setMethod("show", "bar", function(object){cat("show method\n")})
foo
TRUE
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S4 auto-print show: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            output.stdout.contains("show method"),
            "auto-print of S4 bar must call show(); stdout={:?}",
            output.stdout
        );
        assert!(
            !output.stdout.contains("[object; length=0]")
                && !output.stdout.contains("[unknown; length=0]"),
            "auto-print must not emit port object stub; stdout={:?}",
            output.stdout
        );
    }

    #[test]
    fn t_and_f_are_symbols_bound_to_logicals() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(T, TRUE) && identical(F, FALSE) &&
  identical(typeof(quote(F())[[1]]), "symbol") &&
  identical(quote(F())[[1]], as.symbol("F")) &&
  identical(
    paste(deparse(substitute(F(), list(F = quote(n <<- n + 1)))), collapse = " "),
    "(n <<- n + 1)()"
  ) &&
  { F <- 5; identical(F, 5) }
"#,
        );
        let result = result.expect("GNU T/F are symbols, not parser keywords");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_namespace_exports_body_assign() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
exists(".__NAMESPACE__.", envir=asNamespace("methods"), inherits=FALSE) &&
  "body<-" %in% names(.getNamespaceInfo(asNamespace("methods"), "exports")) &&
  identical(methods:::.minimalName("body<-", "methods", qName=TRUE, chkXport=TRUE), "`body<-`")
"#,
        );
        let result = result.expect("methods .__NAMESPACE__. exports include body<-");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn show_getgeneric_body_assign_backticks() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
any(grepl("showMethods(`body<-`)", capture.output(show(getGeneric("body<-"))), fixed=TRUE))
"#,
        );
        let result = result.expect("show(getGeneric(body<-)) backticks");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_generic_data_part_does_not_clear_live_object() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
obj <- `body<-`
invisible(obj@.Data)
isS4(obj) && identical(as.character(obj@generic)[1], "body<-")
"#,
        );
        let result = result.expect("getDataPart must not unset S4 on the live generic");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_namespace_has_no_empty_c_or_rep() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
!exists("rep", envir=asNamespace("methods"), inherits=FALSE) &&
  !exists("c", envir=asNamespace("methods"), inherits=FALSE)
"#,
        );
        let result = result.expect("methods namespace must not bind empty c/rep");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn gnu_norm_rcond_are_closures_with_implicit_methods() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(norm), "closure") &&
  identical(names(formals(norm)), c("x", "type")) &&
  identical(typeof(rcond), "closure") &&
  identical(names(formals(rcond)), c("x", "norm", "triangular", "uplo", "..."))
"#,
        );

        let result = result.expect("norm/rcond implicit methods must match GNU classes-methods.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn seq_along_dispatches_length_and_warns_on_coercion() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
x <- structure(pi, class="testit")
length.testit <- function(x) "OK"
try(eval(substitute(ff(x), list(ff=as.name("seq_along")))), silent=TRUE)
TRUE
"#,
        );
        let result = result.expect("seq_along must dispatch length()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        let text = format!("{output:?}");
        assert!(
            text.contains("NAs introduced by coercion"),
            "missing coercion warning: {text}"
        );
        assert!(
            text.contains("In eval"),
            "missing GNU In-eval attribution: {text}"
        );
    }

    #[test]
    fn methods_setclass_defines_s4_class() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"C_new_object\", slots=c(x=\"numeric\")); x <- new(\"C_new_object\"); isS4(x) && identical(as.character(class(x))[1], \"C_new_object\")",
        );
        let result = result.expect("setClass + new must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_null_slot_roundtrips_gnu_sentinel() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"classRepresentation\"); x@validity <- NULL; is.null(x@validity) && isTRUE(methods:::.hasSlot(x, \"validity\"))",
        );
        let result =
            result.expect("NULL slots must store GNU pseudo_NULL and still count as present");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn language_double_bracket_and_dollar_assign() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "fdef <- quote(function(from, to = TO, strict = TRUE) NULL); fdef[[2L]]$to <- \"C_new_object\"; identical(as.character(fdef[[2L]]$to), \"C_new_object\")",
        );
        let result = result.expect("[[ and $<- must work on language/pairlist like GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn null_dollar_assign_grows_named_list() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "x <- NULL; x$foo <- 1L; identical(x, list(foo = 1L))",
        );
        let result = result.expect("NULL$foo <- must coerce to a named list like GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_new_accepts_named_slots() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); setClass(\"C_new_object\", slots=c(x=\"numeric\")); x <- new(\"C_new_object\", x=4:6); isS4(x) && identical(as.numeric(x@x), c(4,5,6))",
        );
        let result = result.expect("new(Class, slot=) must match GNU initialize");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn as_function_defaults_envir_and_returns_functions() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "f <- function(z) z + 1L; identical(as.function(f), f) && identical(as.function(alist(x=, x + 1L))(2L), 3L)",
        );
        let result = result.expect("as.function must default envir like GNU parent.frame()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn class_of_null_is_null_string() {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_script_with_output_capture("identical(class(NULL), \"NULL\")");
        let result = result.expect("class(NULL) must be GNU's implicit NULL class");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setdiff_null_has_gnu_length_zero() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(setdiff(NULL, \"x\"), NULL) && length(setdiff(NULL, \"x\")) == 0L && is.null(setdiff(NULL, \"x\"))",
        );
        let result = result.expect("setdiff(NULL, *) must be NULL with length 0");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
}
