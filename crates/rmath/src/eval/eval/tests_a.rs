//! eval tests (part 1/4) — extracted verbatim from eval/eval.rs

use super::*;
use crate::sexp::builder::scalar_integer_in;
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_lang2};
use crate::sexp::session::RSession;
use crate::sexp::symbol::Rf_install;

#[cfg(test)]
mod tests_a {
    use super::*;

    #[test]
    fn owned_recall_snapshots_survive_collecting_function_lookup_and_context_replacement() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let nil = R_NilValue();
            let environment = session.global_env().unwrap();
            let closure = crate::mainutils::dstruct::mkCLOSXP(
                nil,
                Rf_ScalarInteger(73),
                environment.as_raw(),
            );
            let closure = owner.sexp(closure).unwrap().into_owned().unwrap();
            let identity_call = Rf_lang2(Rf_install(c"identity".as_ptr()), closure.as_raw());
            let identity_call = owner.sexp(identity_call).unwrap().into_owned().unwrap();
            let source = crate::sexp::constructors::Rf_cons(identity_call.as_raw(), nil);
            crate::sexp::accessors::SET_TYPEOF(source, SEXPTYPE::LANGSXP.as_c_int());
            let context = crate::sexp::context::Rf_begincontext_in(
                instance,
                crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_RETURN,
                source,
                environment.as_raw(),
                environment.as_raw(),
                None,
                nil,
                nil,
            );
            let context_cell = crate::sexp::context::retain_context_in(instance, context).unwrap();
            drop(closure);
            drop(identity_call);
            let invoked = std::rc::Rc::new(std::cell::Cell::new(false));
            let observed = invoked.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(true) {
                    return;
                }
                (*context_cell.get())
                    .call
                    .replace_from_raw_in(instance, nil);
                (*context_cell.get())
                    .sysparent
                    .replace_from_raw_in(instance, nil);
                crate::sexp::gengc::full_gc_in(instance);
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = do_recall(nil, nil, nil, environment.as_raw());
            let result = owner.sexp(result).unwrap();
            assert!(
                invoked.get(),
                "function lookup must exercise a collecting callback"
            );
            assert_eq!(result.integer_elt(0), Some(73));
            crate::sexp::context::Rf_endcontext_in(instance, context);
        });
    }

    #[test]
    fn eval_context_evaluates_owner_scoped_expression() {
        let mut session = RSession::new();
        let expr = session
            .with_arena(|arena| {
                scalar_integer_in(arena, 123)
                    .expect("scalar allocation should succeed")
                    .as_raw()
            })
            .expect("session should be active");
        let expr = session.sexp(expr).expect("expr belongs to session");
        let env = session.global_env().expect("global env should exist");

        let result = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ EvalContext::new(env)
            .eval(expr) }
            .expect("self-evaluating scalar should evaluate");

        assert_eq!(result.integer_elt(0), Some(123));
    }

    #[test]
    fn missing_formal_errors_when_forced_through_promise() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture("f <- function(x) x\nf()");
        let err = result.expect_err("forcing a missing formal should fail");
        assert_eq!(err.message, "argument \"x\" is missing, with no default");
    }

    #[test]
    fn primitive_descriptor_exposes_funtab_metadata() {
        let _session = RSession::new();
        let primitive = unsafe { crate::mainutils::names::R_Primitive(c"+".as_ptr()) };
        let descriptor =
            unsafe { PrimitiveDescriptor::from_raw(primitive) }.expect("primitive descriptor");

        assert_eq!(descriptor.name, "+");
        assert_eq!(descriptor.kind, BUILTINSXP);
        assert!(descriptor.table_index >= 0);
        assert_eq!(unsafe { PRIMNAME(primitive) }, "+");
        assert_eq!(unsafe { PRIMPRINT(primitive) }, descriptor.print_flag);
    }

    #[test]
    fn eval_classifier_names_core_evaluation_phases() {
        let _session = RSession::new();
        unsafe {
            let int_expr = Sexp::from_raw(Rf_ScalarInteger(1)).expect("integer scalar");
            assert_eq!(classify_expr(int_expr), EvalKind::SelfEvaluating);

            let symbol = Sexp::from_raw(Rf_install(c"x".as_ptr())).expect("symbol");
            assert_eq!(classify_expr(symbol), EvalKind::Symbol);

            let call = Sexp::from_raw(Rf_lang2(Rf_install(c"quote".as_ptr()), Rf_ScalarInteger(1)))
                .expect("language call");
            assert_eq!(classify_expr(call), EvalKind::Language);

            let expr_vec = Sexp::from_raw(crate::sexp::constructors::Rf_allocVector(
                SEXPTYPE::EXPRSXP,
                0,
            ))
            .expect("expression vector");
            // eval.c Rf_eval returns expression vectors unchanged; only
            // R-level eval() (do_eval) walks their elements.
            assert_eq!(classify_expr(expr_vec), EvalKind::SelfEvaluating);
        }
    }

    #[test]
    fn builtin_function_object_is_self_evaluating() {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_script_with_output_capture("f <- paste\nidentical(eval(f), f)");

        let result = result.expect("builtin function object should evaluate as a value");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn bytecode_disabled_errors_before_interpreting_payload() {
        let mut session = RSession::new();
        let raw_bcode = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::BCODESXP))
            .expect("session should be active");
        let bcode = session
            .sexp(raw_bcode)
            .expect("bytecode belongs to session");
        let env = session.global_env().expect("global env should exist");

        crate::sexp::instance::with_required_current_instance(|inst| unsafe {
            (*inst).eval_state.disable_bytecode = TRUE;
        });

        let err = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ EvalContext::new(env)
            .eval(bcode) }
            .expect_err("disabled bytecode should not execute");
        assert!(err.contains("bytecode evaluation is disabled"));
    }

    #[test]
    fn eval_context_rejects_unowned_expression_handles() {
        let mut session = RSession::new();
        let raw = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("session should be active");
        let expr = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(raw) }.expect("legacy raw wrapper should construct");
        let env = session.global_env().expect("global env should exist");

        let err = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ EvalContext::new(env)
            .eval(expr) }
            .expect_err("unowned expression should be rejected");
        assert!(err.contains("expression is not owner-scoped"));
    }

    #[test]
    fn ddval_first_dot_evaluates_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_script_with_output_capture("f <- function(...) ..1; identical(f(10), 10)");
        let result = result.expect("..1 should read the first dots element");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn ddval_empty_dots_uses_gnu_message() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture("f <- function(...) ..1; f()");
        let err = result.expect_err("empty ... should error");
        assert!(
            err.message
                .contains("the ... list contains fewer than 1 element"),
            "{}",
            err.message
        );
    }

    #[test]
    fn ddval_second_dot_uses_gnu_plural_message() {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_script_with_output_capture("h <- function(...) ..2; h(1)");
        let err = result.expect_err("short ... should error");
        assert!(
            err.message
                .contains("the ... list contains fewer than 2 elements"),
            "{}",
            err.message
        );
    }

    #[test]
    fn ddval_incorrect_context_uses_gnu_message() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture("..1");
        let err = result.expect_err("top-level ..1 should error");
        assert!(
            err.message
                .contains("..1 used in an incorrect context, no ... to look in"),
            "{}",
            err.message
        );

        let (result, _, _) = session.eval_script_with_output_capture("g <- function() ..1; g()");
        let err = result.expect_err("..1 without ... should error");
        assert!(
            err.message
                .contains("..1 used in an incorrect context, no ... to look in"),
            "{}",
            err.message
        );
    }

    #[test]
    fn missing_ddval_matches_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "m <- function(...) missing(..1); identical(c(m(), m(1)), c(TRUE, FALSE))",
        );
        let result = result.expect("missing(..1) should follow GNU Nth-cell rules");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn missing_stays_true_for_unsupplied_default() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "f <- function(x=1, slots) { force(slots); missing(x) }; identical(c(f(slots=0), f(2, slots=0), { y <- 0; g <- function(x=1) { y <- x; missing(x) }; g() }), c(TRUE, FALSE, TRUE))",
        );
        let result = result.expect("missing() must follow GNU default-formal rules");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn missing_is_false_after_assigning_formal() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "f <- function(x) { x <- 5; missing(x) }; identical(c(f(), f(1)), c(FALSE, FALSE))",
        );
        let result = result.expect("assigning a formal must clear missing()");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_load_does_not_steal_lexical_enclosure() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "x <- 99; f <- function() x; invisible(require(methods, quietly=TRUE)); identical(f(), 99)",
        );
        let result = result.expect("user closures must keep lexical scope after methods loads");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn colon_builtin_evaluates_integer_range() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(typeof(`:`), \"special\") && identical(typeof(1L:3L), \"integer\") && identical(as.integer(1:3), c(1L, 2L, 3L))",
        );
        let result = result.expect("':' must be GNU's special colon");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_new_classrepresentation_is_s4() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"classRepresentation\"); isS4(x) && identical(as.character(class(x))[1], \"classRepresentation\")",
        );
        let result = result.expect("GNU new(classRepresentation) must return an S4 object");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_identc_compares_class_name_contents() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); isTRUE(methods:::.identC(\"classRepresentation\", \"classRepresentation\")) && !isTRUE(methods:::.identC(\"classRepresentation\", \"ClassUnionRepresentation\"))",
        );
        let result = result.expect(".identC must compare CHARSXP contents like GNU Seql");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_generic_closure_restores_s4_bit() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); g <- getGeneric(\"show\"); isS4(g) && identical(as.character(class(g))[1], \"standardGeneric\") && identical(as.character(g@generic)[1], \"show\")",
        );
        let result = result.expect("lazy-loaded generics must keep the GNU S4 bit");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_at_assign_stores_s4_slot() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"classRepresentation\"); x@virtual <- TRUE; isTRUE(x@virtual)",
        );
        let result = result.expect("@<- must persist S4 slots like GNU installAttrib");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_slot_assign_via_call() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); x <- new(\"classRepresentation\"); slot(x, \"virtual\", FALSE) <- TRUE; isTRUE(x@virtual)",
        );
        let result = result.expect("slot<- must call GNU R_do_slot_assign");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn slot_assign_default_check_does_not_bind_value_as_check() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("SlotChk", slots = c(x = "numeric"))
o <- new("SlotChk")
slot(o, "x") <- 1:3
identical(as.numeric(o@x), c(1, 2, 3))
"#,
        );
        let result = result.expect("slot(obj, name) <- value must not bind value to check=");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn new_copies_slots_from_superclass_object() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("TrackN", slots = c(x = "numeric", y = "numeric"))
setClass("TrackCurveN", contains = "TrackN", slots = c(smooth = "numeric"))
t1 <- new("TrackN", x = 1:4, y = 5:8)
t2 <- new("TrackCurveN", t1, smooth = 9:12)
identical(as.numeric(t2@x), as.numeric(1:4)) &&
  identical(as.numeric(t2@y), as.numeric(5:8)) &&
  identical(as.numeric(t2@smooth), as.numeric(9:12))
"#,
        );
        let result = result.expect("new(Class, super, slot=) must copy superclass slots");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn as_assign_superclass_replace_does_not_crash() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("AsTrk", slots = c(x = "numeric", y = "numeric"))
setClass("AsCurve", contains = "AsTrk", slots = c(smooth = "numeric"))
t1 <- new("AsTrk", x = 1:3, y = 4:6)
o <- new("AsCurve")
as(o, "AsTrk") <- t1
identical(as.numeric(o@x), as.numeric(1:3)) &&
  identical(as.numeric(o@y), as.numeric(4:6))
"#,
        );
        let result = result.expect("as<- superclass replace must not crash");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn register_s3method_is_invisible() {
        let mut session = RSession::new();
        let (result, stdout, _) = session.eval_script_with_output_capture(
            r#"
registerS3method("print", "RegInv", function(x) invisible(x))
identical(withVisible(registerS3method("print", "RegInv2", function(x) x))$visible, FALSE)
"#,
        );
        let result = result.expect("registerS3method must be invisible");
        assert!(
            !stdout.stdout.contains("NULL"),
            "registerS3method must not auto-print NULL, got {:?}",
            stdout.stdout
        );
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn methods_externalptr_typeof_and_class() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "invisible(require(methods, quietly=TRUE)); p <- methods:::.newExternalptr(); identical(typeof(p), \"externalptr\") && identical(class(p), \"externalptr\") && is(p, \"externalptr\")",
        );
        let result = result.expect("externalptr must have GNU typeof/class");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn invisible_is_gnu_builtin() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "identical(typeof(invisible), \"builtin\") && identical(invisible(1L), 1L)",
        );
        let result = result.expect("invisible must be GNU's evaluated builtin");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rnorm_named_mean_leaves_positional_for_sd() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "set.seed(1); a <- rnorm(1, 0, mean = 10); set.seed(1); b <- rnorm(1, mean = 10, sd = 0); identical(a, b) && identical(a, 10)",
        );
        let result = result.expect("rnorm must match GNU exact-then-positional formals");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rnorm_partial_names_and_docall_match_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "set.seed(1); a <- rnorm(1, m=10, s=0); set.seed(1); b <- do.call(rnorm, list(1, mean=10, sd=0)); identical(a, b) && identical(a, 10)",
        );
        let result = result.expect("rnorm partial names and do.call must match GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rnorm_unused_argument_errors_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            "inherits(tryCatch(rnorm(1, foo=1), error=function(e) e), \"error\")",
        );
        let result = result.expect("unused rnorm argument must error");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn rnorm_named_mean_after_stats_load_matches_gnu() {
        let mut session = RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            r#"
invisible(require(stats, quietly=TRUE))
set.seed(1); a <- rnorm(1, 0, mean = 10)
set.seed(1); b <- rnorm(1, mean = 10, sd = 0)
identical(typeof(rnorm), "closure") && identical(a, b) && isTRUE(all.equal(a, 10))
"#,
        );
        let result = result.unwrap_or_else(|e| {
            panic!(
                "stats rnorm matching: {e}\nstdout={}\nstderr={}",
                output.stdout, output.stderr
            )
        });
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn language_implicit_class_follows_gnu_lang2str() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(class(quote((x))), "(") &&
  identical(class(quote({1})), "{") &&
  identical(class(quote(if (TRUE) 1)), "if") &&
  identical(class(quote(sin(x))), "call") &&
  identical(class(expression((x))), "expression")
"#,
        );
        let result = result.expect("class() of language objects must use GNU lang2str");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn usemethod_dispatches_on_paren_and_expression() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
abc <- function(x, ...) UseMethod("abc", x)
abc.default <- function(x, ...) "default"
"abc.(" <- function(x) "paren"
abc.expression <- function(x) "expr"
identical(abc(expression((x))), "expr") &&
  identical(abc(quote((x))), "paren") &&
  identical(abc(quote(sin(x))), "default")
"#,
        );
        let result = result.expect("UseMethod must see GNU implicit language classes");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn mode_and_str_of_paren_language_follow_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(mode(quote((x))), "(") &&
  identical(mode(quote(sin(x))), "call") &&
  identical(mode(quote({1})), "call") &&
  identical(mode(quote(if (TRUE) 1)), "call") &&
  identical(class(formals(function(a = 1) NULL)), "pairlist") &&
  identical(mode(formals(function(a = 1) NULL)), "pairlist") &&
  identical(class(new.env()), "environment")
"#,
        );
        let result = result.expect("mode() of calls is ( vs call; class uses type2str");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture("str(quote((x)))\n");
        assert!(
            captured.stdout.contains(r#"language, mode "(": (x)"#),
            "str() of paren language must include GNU mode suffix, got {:?}",
            captured.stdout
        );
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture("str(quote(if (TRUE) 1))\n");
        assert!(
            captured.stdout.contains(" language if (TRUE) 1"),
            "str() of if-call must not add a mode suffix, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains(r#"mode "if""#),
            "str() must not treat lang2str heads as mode(), got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn match_closure_args_follows_gnu_three_pass() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r"
f <- function(abc, abd, ...) list(abc, abd, list(...))
identical(f(abc = 1, abd = 2, extra = 3), list(1, 2, list(extra = 3))) &&
  identical(f(1, 2, 3), list(1, 2, list(3))) &&
  identical(f(abd = 2, abc = 1), list(1, 2, list())) &&
  identical(f(abc = 1, ab = 2), list(1, 2, list()))
",
        );
        let result = result.expect("closure matching must follow GNU matchArgs");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn formatc_default_width_is_digits_plus_one() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"identical(formatC(2^30, digits = 12), "   1073741824")"#,
        );
        let result = result.expect("formatC default width is digits+1");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn print_digits_argument_is_honored() {
        let mut session = RSession::new();
        let (_, captured, _) =
            session.eval_script_with_output_capture("print(c(2.44140624e-04, 8), digits = 1)\n");
        assert!(
            captured.stdout.contains("[1] 2e-04 8e+00") || captured.stdout.contains("[1] 0.0002 8"),
            "print(..., digits=1) must honor digits, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn cat_null_still_emits_sep() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"cat(NULL, "x"); cat("\n"); cat(if (FALSE) "\n", formatC(1, width = 2), ":", "\n")"#,
        );
        assert!(
            captured.stdout.contains(" x\n") && captured.stdout.contains("  1 :"),
            "cat(NULL, ...) must keep sep, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn format_info_honors_digits_argument() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r"
x2 <- c(0.099999994, 0.2)
v <- 6:8
names(v) <- v
m <- sapply(v, format.info, x = x2)
identical(as.vector(m), c(3L, 1L, 0L, 10L, 8L, 0L, 11L, 9L, 0L))
",
        );
        let result = result.expect("format.info digits must follow GNU");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn namesgets_coerces_integer_to_character() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
v <- 6:8
names(v) <- v
identical(names(v), c("6", "7", "8")) &&
  identical(dimnames(sapply(v, format.info, x = c(0.099999994, 0.2)))[[2]], c("6", "7", "8"))
"#,
        );
        let result = result.expect("names<- must store character names");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn signif_recycles_digits_like_gnu_math2() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r"
z <- c(2.002566e-308, 2.447581e-308)
m <- outer(z, 0:3, signif)
identical(format(m[, 1], digits = 1), format(m[, 2], digits = 1)) &&
  !identical(format(m[, 2], scientific = TRUE), format(m[, 4], scientific = TRUE))
",
        );
        let result = result.expect("signif must recycle a digits vector");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn cat_uses_scientific_for_large_whole_doubles() {
        let mut session = RSession::new();
        let (_, captured, _) = session
            .eval_script_with_output_capture("cat(signif(1.234567891234567e27, 1), \"\\n\")\n");
        assert!(
            captured.stdout.contains("1e+27"),
            "cat of signif(1e27, 1) must be scientific, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("10000000000000000"),
            "cat must not dump the full integer mantissa, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn signif_empty_operands_follow_gnu_math2() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
identical(signif(numeric(0), 3), numeric(0)) &&
  identical(signif(numeric(0)), numeric(0)) &&
  inherits(tryCatch(signif(1:3, numeric(0)), error = identity), "error")
"#,
        );
        let result = result.expect("signif empty operands must follow GNU math2");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn cat_honors_live_digits_for_scientific() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "options(digits=8); cat(signif(1.234567891234567e27, 8), \"\\n\")\n",
        );
        assert!(
            captured.stdout.contains("1.2345679e+27"),
            "cat at digits=8 must keep 8 sig digits, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_character_matrix_and_noquote_follow_gnu() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r"
m1 <- matrix(letters[1:24], 6, 4)
m1
noquote(m1)
m1
invisible(NULL)
",
        );
        assert!(
            captured.stdout.contains("[,1]") && captured.stdout.contains("[1,]"),
            "character matrix must print as a matrix, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("\"a\""),
            "default character matrix print must quote cells, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains(" a    ") || captured.stdout.contains("[1,] a"),
            "noquote character matrix must drop quotes, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("[1] \"a\" \"b\" \"c\""),
            "character matrix must not flatten to a quoted vector, got {:?}",
            captured.stdout
        );
        let quoted_blocks = captured.stdout.matches("\"a\"").count();
        assert!(
            quoted_blocks >= 2,
            "noquote must not mutate m1; later print must still quote, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn format_data_frame_prints_like_gnu() {
        let mut session = RSession::new();
        let (result, captured, _) = session.eval_script_with_output_capture(
            r#"
zz <- data.frame("(row names)" = c("aaaaa", "b"), check.names = FALSE)
format(zz)
invisible(NULL)
"#,
        );
        let _ = result;
        assert!(
            captured.stdout.contains("1       aaaaa") && captured.stdout.contains("2           b"),
            "format(data.frame) must use data.frame layout, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains('$') && !captured.stdout.contains("[1] \"19\""),
            "format(data.frame) must not list-print, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn expand_model_frame_honors_subset_and_na_expand() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
set.seed(321)
dd <- data.frame(x = 1:5, y = rnorm(5), z = c(1, 2, NA, 4, 5))
model <- glm(y ~ x, data = dd, subset = 1:4, na.action = na.omit)
a <- expand.model.frame(model, "z", na.expand = FALSE)
b <- expand.model.frame(model, "z", na.expand = TRUE)
is.data.frame(a) && is.data.frame(b) &&
  identical(names(a), c("y","x","z")) &&
  identical(row.names(a), c("1","2","4")) &&
  identical(a$z, c(1,2,4)) &&
  identical(row.names(b), c("1","2","3","4")) &&
  identical(b$z, c(1,2,NA,4))
"#,
        );
        let result = result.expect("expand.model.frame must honor subset and na.expand");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn summary_data_frame_returns_gnu_table() {
        let mut session = RSession::new();
        let (result, captured, _) = session.eval_script_with_output_capture(
            r#"
dd <- data.frame(event = c(1, 9, 18, 14.74, 20, 23),
                 station = factor(c("117","1028","113","117","135","117")))
s <- summary(dd)
inherits(s, "table") && is.matrix(s) && typeof(s) == "character" &&
  grepl("Min.", s[1,1], fixed=TRUE)
"#,
        );

        let result = result.expect("summary.data.frame must return a table");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(
            !captured.stdout.contains("[1] \"list\""),
            "summary(data.frame) must not fall through to typeof, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_language_objects_keep_class_and_pairlist_cells() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
obj <- structure(quote(stop("should not be evaluated")), class = "foo")
list(obj)
pairlist(obj)
structure(list(), attr = obj)
invisible(NULL)
"#,
        );
        assert!(
            captured
                .stdout
                .contains("stop(\"should not be evaluated\")")
                && captured.stdout.contains("attr(,\"class\")")
                && captured.stdout.contains("[1] \"foo\""),
            "language objects must print deparsed call plus class, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("[pairlist; length=0]"),
            "pairlist(obj) must print cells, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("attr(,\"attr\")"),
            "list attributes that are language objects must print, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_foo_dispatches_inside_lists_and_attributes() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
obj <- structure(quote(stop("should not be evaluated")), class = "foo")
print.foo <- function(x, ...) cat("dispatched\n")
list(obj)
structure(list(), attr = obj)
invisible(NULL)
"#,
        );
        assert!(
            captured.stdout.contains("dispatched"),
            "print.foo must run for classed list children, got {:?}",
            captured.stdout
        );
        assert!(
            !captured
                .stdout
                .contains("stop(\"should not be evaluated\")"),
            "default language print must not run after print.foo, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn recursive_print_prefers_s4_show_over_print() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
print.callS4Class <- function(x, ...) stop("should not be dispatched")
.CallS4Class <- setClass("callS4Class", slots = c(x = "numeric"))
setMethod("show", "callS4Class", function(object) cat("S4 show!\n"))
x <- .CallS4Class(x = 1)
list(x)
invisible(NULL)
"#,
        );
        assert!(
            captured.stdout.contains("S4 show!"),
            "recursive print must call show() for S4 objects, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("should not be dispatched"),
            "print.callS4Class must not run, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn recursive_print_forwards_user_print_arguments() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r#"
obj <- structure(quote(stop("should not be evaluated")), class = "foo")
print.foo <- function(x, other = FALSE, digits = 0L, ...) {
    cat("digits: ", digits, "\n")
    stopifnot(other, digits == 4, !...length())
}
print(list(obj), digits = 4, other = TRUE)
invisible(NULL)
"#,
        );
        assert!(
            captured.stdout.contains("digits:  4"),
            "recursive print must forward digits/other, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_primitive_includes_argsenv_formals() {
        let mut session = RSession::new();
        let (_, captured, _) =
            session.eval_script_with_output_capture("print(base::list)\ninvisible(NULL)\n");
        assert_eq!(
            captured.stdout.trim_end(),
            "function (...)  .Primitive(\"list\")",
            "PrintSpecial must wrap ArgsEnv formals, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn dots_length_treats_empty_dots_as_zero() {
        let mut session = RSession::new();
        let (result, _, _) = session
            .eval_script_with_output_capture("h <- function(...) ...length(); identical(h(), 0L)");
        let result = result.expect("empty ... must have length 0");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn print_matrix_honors_max_argument() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "print(matrix(nrow = 100, ncol = 4), max = 5)\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains("omitted 99 rows"),
            "matrix print must honour max=, got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("[2,]"),
            "truncated matrix must not print a second row, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_array_uses_gnu_slice_headers() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "print(array(dim = c(2, 2, 2)), max = 4)\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains(", , 1"),
            "3-D arrays must print GNU slice headers, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("omitted 1 slice"),
            "array print must honour max=, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn summary_true_prints_like_gnu() {
        let mut session = RSession::new();
        let (_, captured, _) =
            session.eval_script_with_output_capture("summary(TRUE)\ninvisible(NULL)\n");
        assert_eq!(
            captured.stdout, "   Mode    TRUE \nlogical       1 \n",
            "summary(TRUE) must match GNU print.summaryDefault, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn summary_pi_prints_gnu_digits() {
        let mut session = RSession::new();
        let (_, captured, _) = session
            .eval_script_with_output_capture("options(digits=7); summary(pi)\ninvisible(NULL)\n");
        assert!(
            captured.stdout.contains("3.142"),
            "named numeric summaryDefault must use digits=max(3,digits-3), got {:?}",
            captured.stdout
        );
        assert!(
            !captured.stdout.contains("3.1 ") && !captured.stdout.contains("3.141593"),
            "must not use 1 decimal or full digits, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn str_ts_uses_digits_d_for_range_and_preview() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            r"
z <- ts(c(112,118,132,129,121,135,148,148,136,119, rep(120, 134)), frequency=12, start=c(1949,1))
str(z)
y <- ts(c(200.1, 199.5, 199.4, 198.9, 199, 200.2, 198.6, 200, 200.3, 201.2))
str(y)
w <- ts(c(10.01, 10.07, 10.32, 9.75, 10.33, 10.13, 10.36, 10.32, 10.13, 10.16))
str(w)
invisible(NULL)
",
        );
        assert!(
            captured.stdout.contains("from 1949 to 1961:"),
            "str.ts must format tsp with digits.d=3, got {:?}",
            captured.stdout
        );
        assert!(
            captured
                .stdout
                .contains("112 118 132 129 121 135 148 148 136 119 ..."),
            "integer-like ts preview is vec.len*2.5, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("200 200 199 199 199 ...")
                && !captured.stdout.contains("200.1"),
            "non-integer-like ts preview uses digits.d and 1.25*vec.len, got {:?}",
            captured.stdout
        );
        assert!(
            captured.stdout.contains("10.01 10.07 10.32 9.75 10.33 ..."),
            "preview slice must share format() decimals, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn print_noquote_empty_character_omits_class() {
        let mut session = RSession::new();
        let (_, captured, _) = session
            .eval_script_with_output_capture("print(noquote(character(0)))\ninvisible(NULL)\n");
        assert_eq!(
            captured.stdout.trim_end(),
            "character(0)",
            "GNU print.noquote strips class before print, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn rnorm_named_mean_after_positional_sd_matches_gnu() {
        let mut session = RSession::new();
        let (result, captured, _) = session.eval_script_with_output_capture(
            r#"
set.seed(1); a <- rnorm(1, 0, mean = 10)
set.seed(1); b <- rnorm(1, mean = 10, sd = 0)
set.seed(1); c <- rnorm(sd = 0, n = 1, mean = 10)
dup <- tryCatch(rnorm(1, mean = 1, mean = 2), error = function(e) e)
unk <- tryCatch(rnorm(1, foo = 2), error = function(e) e)
identical(a, b) && identical(a, c) && isTRUE(all.equal(a, 10)) &&
  grepl("formal argument \"mean\" matched by multiple actual arguments", conditionMessage(dup), fixed = TRUE) &&
  grepl("unused argument (foo = 2)", conditionMessage(unk), fixed = TRUE)
"#,
        );
        assert_eq!(
            result
                .expect("rnorm matching should evaluate")
                .logical_elt(0),
            Some(TRUE),
            "stdout={:?} stderr={:?}",
            captured.stdout,
            captured.stderr
        );
    }

    #[test]
    fn str_named_character_quotes_like_gnu() {
        let mut session = RSession::new();
        let (_, captured, _) = session
            .eval_script_with_output_capture("str(c(F=0.3, `Tail area`=60))\ninvisible(NULL)\n");
        assert!(
            captured.stdout.contains("chr [1:2] \"F\" \"Tail area\""),
            "str() of character names must quote, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn only_internal_generics_dispatch_s3_on_classed_args() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
x <- structure(pi, class="testit")
retracemem.testit <- function(x, previous=NULL) 42
res <- try(retracemem(x), silent=TRUE)
length.testit <- function(x) 99
stopifnot(inherits(res, "try-error") || !identical(res, 42))
stopifnot(identical(length(x), 99))
stopifnot(identical(typeof(`body<-`), "closure"))
stopifnot(identical(typeof(`formals<-`), "closure"))
TRUE
"#,
        );
        let result = result.expect("internal-generic S3 gate");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn str_prints_nan_and_na_like_gnu() {
        let mut session = RSession::new();
        let (_, captured, _) = session.eval_script_with_output_capture(
            "str(c(NaN, 1))\nstr(c(NA_real_, 1))\ninvisible(NULL)\n",
        );
        assert!(
            captured.stdout.contains("num [1:2] NaN 1")
                && captured.stdout.contains("num [1:2] NA 1"),
            "str() must distinguish NaN from NA, got {:?}",
            captured.stdout
        );
    }

    #[test]
    fn alist_keeps_missing_formals_like_gnu() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_script_with_output_capture(
            r#"
a <- alist(x=, y=2)
stopifnot(identical(names(a), c("x","y")))
stopifnot(identical(a$y, 2))
g <- function(x) x+1
formals(g) <- a
stopifnot(identical(names(formals(g)), c("x","y")))
TRUE
"#,
        );
        let result = result.expect("alist");
        assert_eq!(result.logical_elt(0), Some(TRUE));
    }
}
