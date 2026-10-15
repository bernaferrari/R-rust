//! eval tests (part 3/4) — extracted verbatim from eval/eval.rs

use super::*;
use crate::sexp::session::RSession;

#[cfg(test)]
mod tests_c {
    use super::*;

    #[test]
    fn as_method_definition_sees_methods_namespace() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
m <- methods:::asMethodDefinition(function(x) x)
identical(parent.env(environment(methods:::asMethodDefinition)), asNamespace("methods")) &&
  exists(".anyClassName", environment(methods:::asMethodDefinition), inherits=TRUE) &&
  is(m, "MethodDefinition")
"#,
        );
        let result = result.expect("asMethodDefinition must close over methods");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn make_generic_builds_norm_standard_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
fdef <- getFunction("norm", mustFind=FALSE)
body(fdef) <- substitute(standardGeneric(NAME), list(NAME="norm"))
g <- methods:::makeGeneric(
  "norm", fdef,
  fdefault=getFunction("norm", generic=FALSE, mustFind=FALSE),
  package="base", signature=c("x","type")
)
isS4(g) && is(g, "standardGeneric") && identical(as.character(g@generic)[1], "norm")
"#,
        );
        let result = result.expect("makeGeneric(norm) must produce a standardGeneric");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setter_call_does_not_eval_language_replacement() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r"
f <- function(e, v) { body(e) <- v; e }
g <- f(function(x) 1, quote(y))
identical(body(g), quote(y))
",
        );
        let result = result.expect("body<- must store a language value");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn remove_source_does_not_call_standardgeneric_body() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
g <- utils::removeSource(function(x, type, ...) standardGeneric("norm"))
h <- utils::removeSource(function(x) x + 1)
identical(body(g), quote(standardGeneric("norm"))) && identical(h(1), 2)
"#,
        );
        let result = result.expect("removeSource must not evaluate language bodies");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn language_subassign_preserves_call() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r"
e <- quote(x + 1)
e[2] <- list(e[[2]])
identical(e, quote(x + 1))
",
        );
        let result = result.expect("[<- on language must keep the call");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setgeneric_norm_uses_implicit_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("norm")
is(norm, "standardGeneric") && identical(as.character(norm@generic)[1], "norm")
"#,
        );
        let result = result.expect("setGeneric(norm) must install the implicit generic");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setmethod_norm_dispatches_character_type() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("zzz", slots = c(x = "NULL"))
setMethod("norm", c(x = "zzz", type = "character"), function(x, type, ...) type)
setMethod("rcond", c(x = "zzz", norm = "character"), function(x, norm, ...) norm)
x <- new("zzz")
identical(norm(x, "O"), "O") && identical(norm(x), "O") &&
  identical(rcond(x, "O"), "O") && identical(rcond(x), "O")
"#,
        );
        let result = result.expect("setMethod(norm/rcond) must dispatch like classes-methods.R");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_implicit_norm_rcond_unmodified() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("zzz", slots = c(x = "NULL"))
setMethod( "norm", c(x = "zzz", type = "character"),
          function (x, type, ...) type)
setMethod("rcond", c(x = "zzz", norm = "character"),
          function (x, norm, ...) norm)
m4 <- list(getMethod( "norm", c(x = "ANY", type = "missing")),
           getMethod("rcond", c(x = "ANY", norm = "missing")),
           selectMethod( "norm", c(x = "zzz", type = "missing")),
           selectMethod("rcond", c(x = "zzz", norm = "missing")))
f4 <- lapply(m4, getDataPart)
x <- new("zzz")
stopifnot(all(vapply(m4, is, FALSE, "MethodDefinition")),
          identical(f4[3:4], f4[1:2]),
          identical( norm(x, "O"), "O"),
          identical( norm(x     ), "O"),
          identical(rcond(x, "O"), "O"),
          identical(rcond(x     ), "O"),
          removeGeneric( "norm"),
          removeGeneric("rcond"),
          removeClass("zzz"))
TRUE
"#,
        );
        let result = result.expect("unmodified classes-methods.R:10-29");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn unlist_empty_lists_keep_gnu_type() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(unlist(list(list(), list()), recursive=FALSE)), "list") &&
  identical(length(unlist(list(list(), list()), recursive=FALSE)), 0L) &&
  identical(typeof(unlist(list(integer(0), integer(0)))), "integer") &&
  is.null(unlist(list(NULL, NULL)))
"#,
        );
        let result = result.expect("empty unlist must keep GNU AnswerType");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setclass_contains_empty_slot_superclass() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
foo <- setClass("foo")
bar <- setClass("bar", contains = "foo")
isClass("bar") && extends("bar", "foo")
"#,
        );
        let result = result.expect("setClass(contains=) must complete empty slots");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn seq_int_named_to_before_from_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_script_with_output_capture("identical(seq.int(to = 3, from = 1), 1:3)");
        let result = result.expect("seq.int must matchArgs, not check1arg the first tag");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setmethod_subset_callnextmethod_matches_classes_methods() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
foo <- setClass("foo")
bar <- setClass("bar", contains = "foo")
setMethod("[", "foo",  function(x, i, j, ..., flag = FALSE, drop = FALSE) { flag })
setMethod("[", "bar", function(x, i, j, ..., flag = FALSE, drop = FALSE) { callNextMethod() })
BAR <- new("bar")
identical(BAR[1L], FALSE) && identical(BAR[1L, , flag=TRUE], TRUE)
"#,
        );
        let result = result.expect("classes-methods.R callNextMethod must forward flag=");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_as_vector_subassign() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(stuff="numeric"))
as.vector.A <- function (x, mode="any") x@stuff
v <- c(3.5, 0.1)
a <- new("A", stuff=v)
x <- y <- numeric(10)
x[3:4] <- a
y[3:4] <- v
identical(x, y)
"#,
        );
        let result = result.expect("classes-methods.R as.vector S3 method in [<-");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_ops_extra_arg_and_dots() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("MyInteger", representation("integer"))
i <- new("MyInteger", 1L)
m <- matrix(1:6, 2, 3)
setGeneric("genericExtraArg",
           function(x, y, extra) standardGeneric("genericExtraArg"),
           signature="x")
setMethod("genericExtraArg", "ANY", function(x, y=NULL) y)
f <- function(...) length(list(...))
setGeneric("f")
setMethod("f", "character", function(...){ callNextMethod() })
identical(i*m, m) &&
  identical(genericExtraArg("foo", 1L), 1L) &&
  identical(f(1, 2, 3), 3L) &&
  identical(f("a", "b", "c"), 3L)
"#,
        );
        let result = result.expect("classes-methods.R Ops, rematch NULL default, dots");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_dots_missing_and_forwarding() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
f <- function(x, ..., a = b) { b <- "a"; a }
setGeneric("f", signature = "...")
f2 <- function(...) f(...)
identical(f(a=1), 1) && identical(f(), "a") && identical(f2(a=1), 1)
"#,
        );
        let result = result.expect("classes-methods.R missing-arg dots dispatch");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_method_selection_error() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
f <- function(x) x
setGeneric("f")
setMethod("f", signature("NULL"), function(x) NULL)
err <- tryCatch(f(stop("this is mentioned")), error = identity)
identical(err$message, "error in evaluating the argument 'x' in selecting a method for function 'f': this is mentioned")
"#,
        );
        let result = result.expect("classes-methods.R method-selection error wrap");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_oldclass_union_recache() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
where <- environment()
setClass("UnionMemberForOldClassRecache", contains = "VIRTUAL", where = where)
setClassUnion("UnionForOldClassRecache", "UnionMemberForOldClassRecache",
              where = where)
setClass("ParentForOldClassRecache",
         contains = c("UnionForOldClassRecache", "VIRTUAL"), where = where)
setClass("ChildForOldClassRecache",
         contains = c("ParentForOldClassRecache", "VIRTUAL"), where = where)
setOldClass(c("ChildForOldClassRecache", "oldClass"),
            S4Class = "ChildForOldClassRecache", where = where)
union <- getClass("UnionForOldClassRecache", where = where)
child <- getClass("ChildForOldClassRecache", where = where)
"ChildForOldClassRecache" %in% names(union@subclasses) &&
  "UnionForOldClassRecache" %in% names(child@contains)
"#,
        );
        let result = result.expect("classes-methods.R setOldClass union recache");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_setoldclass_s3class_tails() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("oldClassChildForAs",
              "oldClassParentForAs",
              "oldClassGrandParentForAs"))
identical(attr(getClass("oldClassGrandParentForAs")@prototype, ".S3Class"),
          "oldClassGrandParentForAs") &&
  identical(attr(getClass("oldClassParentForAs")@prototype, ".S3Class"),
            c("oldClassParentForAs", "oldClassGrandParentForAs")) &&
  identical(attr(getClass("oldClassChildForAs")@prototype, ".S3Class"),
            c("oldClassChildForAs", "oldClassParentForAs", "oldClassGrandParentForAs"))
"#,
        );
        let result = result.expect("setOldClass must accumulate .S3Class tails");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_as_s4_from_oldclass_upcast() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("oldClassChildForAs",
              "oldClassParentForAs",
              "oldClassGrandParentForAs"))
setClass("GrandParentShimForAs", contains = "oldClassGrandParentForAs")
setClass("ParentShimForAs",
         contains = c("oldClassParentForAs", "GrandParentShimForAs"))
setClass("S4ChildForAs",
         slots = list(extra = "character"),
         contains = "ParentShimForAs")
object <- new("S4ChildForAs",
              structure(list(),
                        class = c("oldClassParentForAs",
                                  "oldClassGrandParentForAs")),
              extra = "x")
parent <- as(object, "ParentShimForAs")
grandparent <- as(object, "GrandParentShimForAs")
isS4(parent) && is(parent, "ParentShimForAs") &&
  identical(as.character(class(parent)), "ParentShimForAs") &&
  isS4(grandparent) && is(grandparent, "GrandParentShimForAs") &&
  identical(as.character(class(grandparent)), "GrandParentShimForAs")
"#,
        );
        let result = result.expect("classes-methods.R as() S4-from-old-class upcast");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_cancoerce_multiclass_s3() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(stuff="numeric"))
setOldClass("foo")
setAs("foo", "A", function(from) new("A", foo=from))
o3 <- structure(1:7, class = c("foo", "bar"))
canCoerce(o3, "A")
"#,
        );
        let result = result.expect("classes-methods.R canCoerce length(class)>1");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_basegeneric_missing_signature() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("BaseGeneric", function(x, y, ...) standardGeneric("BaseGeneric"))
setMethod("BaseGeneric", signature(x = "numeric", y = "numeric"), function(x,y, ...) x + y)
errXY <- try(BaseGeneric(X = 1, Y = 2))
err1  <- try(BaseGeneric(1))
err1Y <- try(BaseGeneric(1, Y = 2))
identical(3, BaseGeneric(1, 2)) &&
  inherits(errXY, "try-error") &&
  grepl('x = "missing", y = "missing"', attr(errXY,"condition")$message) &&
  inherits(err1,  "try-error") &&
  grepl('x = "numeric", y = "missing"', attr(err1, "condition")$message) &&
  identical(err1, err1Y)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "classes-methods.R BaseGeneric missing-arg signatures: {e}\nstdout={}\nstderr={}",
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
    fn try_call_less_errors_are_identical_across_expressions() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
e1 <- try(stop(simpleError("x")), silent=TRUE)
e2 <- try((function() stop(simpleError("x")))(), silent=TRUE)
identical(e1, e2) &&
  identical(as.character(e1), "Error : x\n") &&
  is.null(attr(e1, "condition")$call)
"#,
        );
        let result = result.expect("try() call-less errors share GNU Error : prefix");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_sealclass() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("fooSeal", slots = c(name = "character"), sealed = TRUE)
ok1 <- isSealedClass("fooSeal")
ok2 <- inherits(try(setClass("fooSeal"), silent=TRUE), "try-error")
ok3 <- isTRUE(removeClass("fooSeal"))
setClass("fooSeal")
sealClass("fooSeal")
ok4 <- isSealedClass("fooSeal")
ok5 <- isTRUE(removeClass("fooSeal"))
ok1 && ok2 && ok3 && ok4 && ok5
"#,
        );
        let result = result.expect("classes-methods.R sealClass");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn is_base_namespace_matches_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
isBaseNamespace(.BaseNamespaceEnv) &&
  identical(asNamespace("base"), .BaseNamespaceEnv) &&
  !isBaseNamespace(asNamespace("methods")) &&
  !identical(.BaseNamespaceEnv, baseenv()) &&
  identical(environment(summary), .BaseNamespaceEnv) &&
  identical(environmentName(.BaseNamespaceEnv), "base") &&
  identical(parent.env(.BaseNamespaceEnv), .GlobalEnv) &&
  isNamespace(.BaseNamespaceEnv) &&
  !isNamespace(baseenv()) &&
  identical(methods::getPackageName(environment(summary)), "base")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "isBaseNamespace is GNU namespace.R: {e}\nstdout={}\nstderr={}",
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
    fn base_namespace_bindings_stay_shared() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
exists(".ArgsEnv", envir=baseenv(), inherits=FALSE) &&
  exists(".ArgsEnv", envir=.BaseNamespaceEnv, inherits=FALSE) &&
  exists(".GenericArgsEnv", envir=baseenv(), inherits=FALSE) &&
  exists(".GenericArgsEnv", envir=.BaseNamespaceEnv, inherits=FALSE) &&
  local({
    original <- get(".ArgsEnv", envir=baseenv(), inherits=FALSE)
    on.exit({ assign(".ArgsEnv", original, envir=baseenv())
              lockBinding(".ArgsEnv", baseenv()) })
    unlockBinding(".ArgsEnv", baseenv())
    assign(".ArgsEnv", 1, envir=baseenv())
    shared <- identical(get(".ArgsEnv", envir=.BaseNamespaceEnv, inherits=FALSE), 1)
    assign(".ArgsEnv", original, envir=.BaseNamespaceEnv)
    shared && identical(get(".ArgsEnv", envir=baseenv(), inherits=FALSE), original)
  }) &&
  identical(ls(baseenv()), ls(.BaseNamespaceEnv)) &&
  !identical(.BaseNamespaceEnv, baseenv()) &&
  identical(parent.env(.BaseNamespaceEnv), .GlobalEnv)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "base namespace frame share: {e}\nstdout={}\nstderr={}",
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
    fn setgeneric_skeleton_matches_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
e <- asNamespace("methods")
hit_base_ns <- FALSE
for (i in seq_len(16L)) {
  if (isBaseNamespace(e)) { hit_base_ns <- TRUE; break }
  if (identical(e, emptyenv())) break
  e <- parent.env(e)
}
empty_parent_errors <- inherits(tryCatch(parent.env(emptyenv()), error = function(e) e), "error")
name <- "foo"
def <- function(object, arg) standardGeneric("foo")
std <- substitute(standardGeneric(NAME), list(NAME = name))
setGeneric("foo", def)
hit_base_ns &&
  empty_parent_errors &&
  identical(body(def), std) &&
  identical(as.character(class(foo)), "standardGeneric") &&
  !identical(as.environment("package:methods"), asNamespace("methods"))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "setGeneric skeleton: {e}\nstdout={}\nstderr={}",
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
    fn objects_is_gnu_ls_alias() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
exists("objects", envir=baseenv(), inherits=FALSE) &&
  typeof(objects) == "closure" &&
  identical(objects, ls) &&
  identical(objects(envir=baseenv()), ls(envir=baseenv()))
"#,
        );
        // GNU attach.R: `ls <- objects <- function(...)`: one shared
        // UseMethod closure, not separate primitives.
        let result = result.expect("GNU attach.R: objects is the ls closure");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_namespace_lists_class_metadata_bindings() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
ns <- asNamespace("methods")
sum(startsWith(ls(envir=ns, all.names=TRUE), ".__C__")) > 0 &&
  exists("cacheMetaData", envir=ns, inherits=FALSE)
"#,
        );
        let result = result.expect("methods ns lists sourced .__C__ class bindings");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn getgenerics_keeps_package_attribute() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
gens <- methods:::.getGenerics(asNamespace("methods"))
length(gens) > 0 && identical(typeof(attr(gens, "package")), "character") &&
  length(attr(gens, "package")) == length(gens)
"#,
        );
        let result = result.expect(".getGenerics must keep the package attribute");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn show_body_assign_after_methods_onload_cache() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
out <- capture.output(show(`body<-`))
any(grepl("showMethods(`body<-`)", out, fixed=TRUE))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "show(`body<-`) after methods load: {e}\nstdout={}",
                output.stdout
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn library_binding_is_gnu_default_library() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
exists(".Library", envir=baseenv(), inherits=FALSE) &&
  is.character(.Library) &&
  length(.Library) == 1L &&
  nzchar(.Library)
"#,
        );
        let result = result.expect("GNU .Library is R.home('library')");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rmpkg_strips_package_prefix() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"identical(.rmpkg("package:methods"), "methods") && identical(.rmpkg("methods"), "methods")"#,
        );
        let result = result.expect("GNU attach.R .rmpkg");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn trace_is_gnu_base_wrapper() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
exists("trace", envir=baseenv(), inherits=FALSE) &&
  is.function(trace) &&
  exists("untrace", envir=baseenv(), inherits=FALSE) &&
  is.function(untrace)
"#,
        );
        let result = result.expect("GNU methodsSupport.R: trace/untrace");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn tracing_state_toggles_do_trace() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r"
old <- tracingState(TRUE)
n <- 0L
tracingState(FALSE)
.doTrace(n <- 1L)
off <- n
tracingState(TRUE)
.doTrace(n <- 2L)
on <- n
tracingState(old)
identical(off, 0L) && identical(on, 2L)
",
        );
        let result = result.expect("tracingState must gate .doTrace");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_setis_simple_as() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", slots = c(x = "NULL"))
setClass("B", slots = c(x = "NULL"))
setIs("A", "B",
      test = function(.) { TRUE },
      coerce = function(.) new("B"),
      replace = function(., value) new("B"))
B <- as(new("A"), "B")
identical(B, new("B"))
"#,
        );
        match result {
            Ok(v) => assert_eq!(v.logical_elt(0), Some(TRUE)),
            Err(err) => panic!("setIs/as: {err}\nstdout={}", output.stdout),
        }
    }

    #[test]
    fn classes_methods_toeplitz_two_arg() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
!exists("toeplitz", envir=baseenv(), inherits=FALSE) &&
  identical(environment(stats::toeplitz), asNamespace("stats")) &&
  identical(
    as.vector(toeplitz(c(-1, 0, 0), c(-1, 11, 0))),
    c(-1, 0, 0, 11, -1, 0, 0, 11, -1)
  ) &&
  identical(as.vector(toeplitz(1:3)), c(1L, 2L, 3L, 2L, 1L, 2L, 3L, 2L, 1L))
"#,
        );
        let result = result.expect("GNU toeplitz(x, r) first column/row");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn setmethod_toeplitz_implicit_generic() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("Atoep", slots = c(x = "NULL"))
x <- c(-1, 0, 0)
r <- c(-1, 11, 0)
T3 <- toeplitz(x, r)
g <- implicitGeneric("toeplitz")
env_ok <- !exists("toeplitz", envir=baseenv(), inherits=FALSE) &&
  identical(environment(get("toeplitz", envir=asNamespace("stats"), inherits=FALSE)), asNamespace("stats"))
setMethod("toeplitz", "Atoep", function(x, ...) x)
identical(names(formals(g)), c("x", "...")) &&
  env_ok &&
  identical(T3, toeplitz(x, r)) &&
  is(selectMethod(toeplitz, "numeric"), "MethodDefinition") &&
  removeGeneric("toeplitz")
"#,
        );
        let result = result.expect("classes-methods.R toeplitz implicit generic");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_trace_coerce_signature() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
trr <- quote(list(.Generic, .Method, .defined, .target))
sig <- c("ANY", "logical")
m0 <- selectMethod(coerce, signature = sig)
a0 <- as(0, "logical")
trace(coerce, tracer = trr, signature = sig)
m1 <- selectMethod(coerce, signature = sig)
a1 <- as(0, "logical")
untrace(coerce, signature = sig)
m2 <- selectMethod(coerce, signature = sig)
is(m0, "MethodDefinition") &&
  !is(m0, "MethodDefinitionWithTrace") &&
  is(m1, "MethodDefinitionWithTrace") &&
  identical(m0, m2) && identical(a0, a1)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "classes-methods.R PR#18823 trace(coerce): {e}\nstdout={}\nstderr={}",
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
    fn class_attribute_is_namedmax_on_return() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
f <- structure(function(x) x, class = c("myfun", "function"))
cn <- class(f)
cn[] <- paste0(cn, "WithTrace")
identical(class(f), c("myfun", "function")) &&
  identical(cn, c("myfunWithTrace", "functionWithTrace"))
"#,
        );
        let result = result.expect("class() must not alias the live class attribute");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_trace_multiclass_function() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("myfun", "function"))
f <- structure(function(x) x, class = c("myfun", "function"))
n <- 0
suppressMessages(trace("f", quote(n <<- n + 1), print = FALSE))
f1 <- f(1)
untrace("f")
identical(f1, 1) && identical(n, 1) &&
  identical(class(f), c("myfun", "function")) &&
  identical(f(2), 2) && identical(n, 1)
"#,
        );
        let result = result.expect("classes-methods.R trace multi-string class()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_trace_s4_function_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("myS4Fun", contains = "function", slots = c(label = "character"))
g <- new("myS4Fun", function(x) x, label = "kept")
n <- 0
suppressMessages(trace("g", quote(n <<- n + 1), print = FALSE))
g1 <- g(1)
stopifnot(identical(g@label, "kept"))
untrace("g")
identical(g1, 1) && identical(n, 1) &&
  is(g, "myS4Fun") && identical(g@label, "kept") &&
  identical(g(2), 2) && identical(n, 1)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "classes-methods.R trace S4 function class: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_trace_s3_s4_function_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("myS3FunS4", contains = c("function", "VIRTUAL"),
         slots = c(label = "character"))
setOldClass(c("myS3Fun", "function"), S4Class = "myS3FunS4")
h <- structure(function(x) x, class = c("myS3Fun", "function"),
               label = "kept")
n <- 0
suppressMessages(trace("h", quote(n <<- n + 1), print = FALSE))
h1 <- h(1)
stopifnot(identical(h@label, "kept"))
untrace("h")
identical(h1, 1) && identical(n, 1) &&
  identical(class(h), c("myS3Fun", "function")) &&
  identical(attr(h, "label"), "kept") &&
  identical(h(2), 2) && identical(n, 1)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "classes-methods.R trace S3/S4 function class: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn signature_class_names_slot_is_argument_names() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
f <- function(x) x
setGeneric("f")
setMethod("f", "numeric", function(x) x)
md <- selectMethod("f", "numeric")
identical(md@target@names, "x") && identical(md@defined@names, "x")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "signature@names: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_gnu_skip_path_without_matrix() {
        let mut session = RSession::new();
        let vendor = include_str!("../../../../../tests/upstream-r/vendor/classes-methods.R");
        let lines: Vec<&str> = vendor.lines().collect();
        let mut src = String::from("invisible(require(methods, quietly=TRUE))\n");
        for (i, line) in lines.iter().enumerate() {
            let lineno = i + 1;
            // Matrix 47-120 and 287-305 omitted (GNU skip when Matrix is absent).
            if (47..=120).contains(&lineno) || (287..=305).contains(&lineno) {
                continue;
            }
            src.push_str(line);
            src.push('\n');
        }
        src.push_str("TRUE\n");

        let (result, output, _) = session.eval_script_with_output_capture(&src);
        let result = result.unwrap_or_else(|e| {
            panic!(
                "classes-methods.R GNU skip path (Matrix omitted): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn classes_methods_show_backticked_replacement() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
stopifnot(is.function(`body<-`))
any(grepl("showMethods(`body<-`)", capture.output(show(`body<-`)), fixed=TRUE))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "show(`body<-`): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_package_slot_assign_sets_attribute() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
w <- `packageSlot<-`("a", ".GlobalEnv")
identical(attr(w, "package"), ".GlobalEnv")
"#,
        );
        let result = result.expect("methods::packageSlot<- must set the package attribute");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_new_oldclass_function_trace_class_name() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("myfun", "function"))
setClass("myfunWithTrace", contains = c("myfun", "traceable"))
a <- new("myfunWithTrace")
isS4(a) && identical(as.character(class(a))[1], "myfunWithTrace")
"#,
        );
        let result = result.expect("new() must stamp S4 class on oldClass+function prototype");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn s4_new_trace_class_with_function_def() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setOldClass(c("myfun", "function"))
setClass("myfunWithTrace", contains = c("myfun", "traceable"))
f <- structure(function(x) x, class = c("myfun", "function"))
a <- new("myfunWithTrace", f)
isS4(a) && identical(as.character(class(a))[1], "myfunWithTrace")
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "new(myfunWithTrace, def): {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_dispatch_on_after_require_and_reload() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
options(useFancyQuotes=FALSE)
invisible(require(methods, quietly=TRUE))
on1 <- .isMethodsDispatchOn()
invisible(require(stats4, quietly=TRUE))
detach("package:methods")
invisible(require("methods", quietly=TRUE))
on1 && .isMethodsDispatchOn()
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                ".isMethodsDispatchOn after require/reload: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn traced_generic_uses_default_after_setmethod_widens_signature() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
f <- function(x, y) c(x, y)
setGeneric("f")
setMethod("f", c("character", "character"), function(x, y) paste(x, y))
labs <- sort(ls(environment(f)$.AllMTable, all.names=TRUE))
trace("f", quote(x <- c("A", x)), exit = quote(xy <<- c(x, "Z")), print = FALSE)
identical(labs, c("ANY#ANY", "character#character")) &&
  identical(f(4, 5), c("A", "4", "5")) &&
  identical(xy, c("A", "4", "Z"))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "traced generic default: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn cbind2_s4_method_used_by_cbind() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("myMat", representation(x = "numeric"))
setMethod("cbind2", signature(x = "myMat", y = "missing"), function(x,y) x)
m <- new("myMat", x = c(1, pi))
identical(m, methods:::cbind(m)) && identical(m, cbind(m))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "cbind2: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_head_through_callgeneric_local() {
        let mut session = RSession::new();
        let vendor = include_str!("../../../../../tests/upstream-r/vendor/reg-S4.R");
        let src: String = vendor.lines().take(889).collect::<Vec<_>>().join("\n");
        let (result, output, _) = session.eval_script_with_output_capture(&src);
        result.unwrap_or_else(|e| {
            panic!(
                "reg-S4.R through EOF: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
    }

    #[test]
    fn classes_methods_head_through_implicit_norm() {
        let mut session = RSession::new();
        let vendor = include_str!("../../../../../tests/upstream-r/vendor/classes-methods.R");
        let src: String = vendor.lines().take(284).collect::<Vec<_>>().join("\n");
        let (result, output, _) = session.eval_script_with_output_capture(&src);
        result.unwrap_or_else(|e| {
            panic!(
                "classes-methods.R through sealClass: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
    }

    #[test]
    fn classes_methods_unmodified_gnu_file() {
        let mut session = RSession::new();
        let vendor = include_str!("../../../../../tests/upstream-r/vendor/classes-methods.R");
        let (result, output, _) = session.eval_script_with_output_capture(vendor);
        result.unwrap_or_else(|e| {
            panic!(
                "unmodified classes-methods.R: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
    }

    #[test]
    fn reg_s4_rbind_after_setgeneric_dots() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setGeneric("rbind", function(..., deparse.level=1)
           standardGeneric("rbind"), signature = "...")
identical(rbind(1), matrix(1,1,1))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "rbind after setGeneric: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_median_list_subclass() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            concat!(
                "invisible(require(methods, quietly=TRUE))\n",
                "setClass(\"L\", contains = \"list\")\n",
                "setMethod(\"Compare\", signature(e1=\"L\", e2=\"ANY\"),\n",
                "          function(e1,e2) sapply(e1, .Generic, e2=e2))\n",
                "setMethod(\"Summary\", \"L\",\n",
                "          function(x, ..., na.rm=FALSE) {x <- unlist(x); callNextMethod()})\n",
                "setMethod(\"[\", signature(x=\"L\", i=\"ANY\", j=\"missing\",drop=\"missing\"),\n",
                "          function(x,i,j,drop) new(class(x), x@.Data[i]))\n",
                "setMethod(\"xtfrm\", \"L\", function(x) xtfrm(unlist(x@.Data)))\n",
                "mean.L <- function(x, ...) new(\"L\", mean(unlist(x@.Data), ...))\n",
                "x <- new(\"L\", 1:3); x2 <- x[-2]\n",
                "all(unlist(x2) == (1:3)[-2]) && is(mx <- median(x), \"L\") && mx == 2 && median(x2) == x[2]\n",
            ),
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "median L: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_list_class_subset_keeps_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("L", contains = "list")
setMethod("[", signature(x="L", i="ANY", j="missing",drop="missing"),
          function(x,i,j,drop) new(class(x), x@.Data[i]))
x <- new("L", 1:3)
x2 <- x[-2]
isS4(x2) && identical(as.character(class(x2))[1], "L") &&
  identical(unlist(x2), (1:3)[-2]) &&
  identical(unlist(x[2]), 2L)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S4 list [ method: {e}\nstdout={}\nstderr={}",
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
    fn gnu_median_is_closure_with_default() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
identical(typeof(median), "closure") &&
  identical(typeof(median.default), "closure") &&
  identical(median(1:3), 2L) &&
  identical(median(c(1, 3)), 2) &&
  identical(sort(c(3, 1, NA)), c(1, 3)) &&
  isTRUE(2 == list(2)) &&
  identical(as.vector(2 == list(1, 2, 3)), c(FALSE, TRUE, FALSE))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "gnu median wrapper: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_median_simple_list_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("L", contains = "list")
setMethod("Compare", signature(e1="L", e2="ANY"),
          function(e1,e2) sapply(e1, .Generic, e2=e2))
setMethod("Summary", "L",
	  function(x, ..., na.rm=FALSE) {x <- unlist(x); callNextMethod()})
setMethod("[", signature(x="L", i="ANY", j="missing",drop="missing"),
          function(x,i,j,drop) new(class(x), x@.Data[i]))
setMethod("xtfrm", "L", function(x) xtfrm(unlist(x@.Data)))
mean.L <- function(x, ...) new("L", mean(unlist(x@.Data), ...))
x <- new("L", 1:3); x2 <- x[-2]
identical(unlist(x2), (1:3)[-2]) &&
  is(mx <- median(x), "L") && isTRUE(mx == 2) &&
  isTRUE(median(x2) == x[2])

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "median S4 L: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_sig_as_packageslot_and_factor_validity() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
assertError <- tools::assertError
setClass("SIG", contains="signature")
pkg_ok <- packageSlot(class(S <- new("SIG"))) == ".GlobalEnv" &&
  packageSlot(class(ss <- new("signature"))) == "methods" &&
  packageSlot(class(as(S, "signature"))) == "methods"
ok.f <- gl(3,5, labels = letters[1:3])
bad.f <- structure(rep(1:3, each=5), levels=c("a","a","b"), class="factor")
validObject(ok.f)
bad_ok <- inherits(tryCatch(validObject(bad.f), error=function(e) e), "error")
setClass("myF", contains = "factor")
validObject(new("myF", ok.f))
myf_bad <- inherits(tryCatch(validObject(new("myF", bad.f)), error=function(e) e), "error")
removeClass("myF")
as_ok <- is.vector(as(structure(1:3, class = "foobar"), "vector"))
setClass("numWithId", representation(id = "character"), contains = "numeric")
x <- new("numWithId", 1:3, id = "An Example")
setMethod("xtfrm", "numWithId", function(x) x@.Data)
pkg_ok && bad_ok && myf_bad && as_ok && identical(xtfrm(x), 1:3)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "SIG/factor/xtfrm: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_subset_callnextmethod_drop_and_slot_names() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("C1", representation(a = "numeric"))
setClass("C2", contains = "C1")
setMethod("[", "C1", function(x,i,j,...,drop=TRUE)
	  cat("drop in C1-[ :", drop, "\n"))
setMethod("[", "C2", function(x,i,j,...,drop=TRUE) {
    cat("drop in C2-[ :", drop, "\n")
    callNextMethod()
})
x <- new("C1"); y <- new("C2")
o1 <- paste(capture.output(x[1, drop=FALSE]), collapse="\n")
o2 <- paste(capture.output(y[1, drop=FALSE]), collapse="\n")
grepl("drop in C1-[ : FALSE", o1, fixed=TRUE) &&
  grepl("drop in C2-[ : FALSE", o2, fixed=TRUE) &&
  grepl("drop in C1-[ : FALSE", o2, fixed=TRUE)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "callNextMethod drop: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_reserved_slot_names_except_class() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
problNames <- c("names", "dimnames", "row.names",
                "class", "comment", "dim", "tsp")
myTry <- function(expr, ...) tryCatch(expr, error = function(e) e)
tstSlotname <- function(nm) {
    r <- myTry(setClass("foo", representation =
                        structure(list("character"), names = nm)))
    if(is(r, "error")) return(r$message)
    ch <- LETTERS[1:5]
    x <- myTry(do.call(new, structure(list("foo", ch), names=c("", nm))))
    if(is(x, "error")) return(x$message)
    y <- myTry(new("foo"));		 if(is(y, "error")) return(y$message)
    r <- myTry(capture.output(show(x))); if(is(r, "error")) return(r$message)
    r <- myTry(capture.output(show(y))); if(is(r, "error")) return(r$message)
    slot(y, nm) <- slot(x, nm)
    stopifnot(validObject(x), identical(x,y), identical(slot(x, nm), ch))
    return(TRUE)
}
R <- sapply(problNames, tstSlotname, simplify = FALSE)
out <- capture.output(str(R))
is.character(R[["class"]]) &&
  all(vapply(R[names(R) != "class"], isTRUE, NA)) &&
  identical(R[["class"]], "\"class\" is a reserved slot name and cannot be redefined") &&
  any(grepl("tsp", out, fixed = TRUE)) &&
  any(grepl('chr "\\\\"class\\\\" is a reserved', out))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "reserved slot names: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_sample_implicit_generic_from_base() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("C1", representation(a = "numeric"))
setClass("C2", contains = "C1")
setMethod("sample", "C2",
          function(x, size, replace=FALSE, prob=NULL) {"sample.C2"})
is(sample,"standardGeneric") &&
  identical(sample@signature, c("x", "size")) &&
  identical(packageSlot(sample), "base") &&
  identical({set.seed(3); sample(3)}, 1:3)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "sample implicit generic: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_qqplot_generic_and_nested_slot_subassign() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
qq_ok <- is.function(qqplot) && identical(class(qqplot), "function")
setGeneric("qqplot", function(x, y, ...) standardGeneric("qqplot"))
qq_gen <- is(qqplot, "standardGeneric") && identical(qqplot@signature, c("x","y"))
setClass("foo", representation(x = "numeric"))
f <- new("foo", x = pi*1:2)
L <- list()
L$A <- f
L$A@x[] <- 7
dup_ok <- !identical(f, L$A) && identical(L$A@x, c(7, 7)) &&
  isTRUE(all.equal(f@x, pi*1:2))
qq_ok && qq_gen && dup_ok
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "qqplot/nested slot: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_classunion_prototype_intorchar() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClassUnion("OptionalPOSIXct", c("POSIXct", "NULL"))
setClassUnion("IntOrChar", c("integer", "character"))
is.null(getClass("OptionalPOSIXct")@prototype) &&
  is.integer(getClass("IntOrChar")@prototype) &&
  "IntOrChar" %in% extends(getClass("character")) &&
  "IntOrChar" %in% extends(getClass("integer")) &&
  identical(isGeneric("&&"), FALSE)
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "classUnion prototype: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_mapply_length_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(aa="integer"))
aa <- 11:16
a <- new("A", aa=aa)
setMethod(length, "A", function(x) length(x@aa))
setMethod(`[[`,   "A", function(x, i, j, ...) x@aa[[i]])
setMethod(`[`,    "A", function(x, i, j, ...) new("A", aa = x@aa[i]))
len_ok <- length(a) == 6 && identical(a[[5]], aa[[5]]) &&
  identical(a, rev(rev(a))) && identical(rev(a)@aa, rev(aa))
map_ok <- identical(mapply(`*`, aa, rep(1:3, 2)), mapply(`*`, a,  rep(1:3, 2)))
len_ok && map_ok

"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "S4 mapply length: {e}\nstdout={}\nstderr={}",
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
    fn reg_s4_is_unsorted_method() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("A", representation(aa="integer"))
aa <- 11:16
a <- new("A", aa=aa)
setMethod(length, "A", function(x) length(x@aa))
setMethod(`[`, "A", function(x, i, j, ...) new("A", aa = x@aa[i]))
setMethod("is.unsorted", "A", function(x, na.rm, strictly)
    is.unsorted(x@aa, na.rm=na.rm, strictly=strictly))
!is.unsorted(a) && is.unsorted(rev(a)) &&
  identical(rev(a)@aa, rev(aa))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "is.unsorted S4: {e}\nstdout={}\nstderr={}",
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
}
