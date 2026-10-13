#![allow(deprecated)] // translated tests exercise the Sexp compat setters
use super::tests::{literal_string_vector, string_vector, unique_test_root};
use super::*;
#[test]
fn test_proc_time_shape_matches_r_contract() {
    let mut session = RSession::new();

    let len = session.eval("length(proc.time())");
    assert_eq!(len.output, "[1] 5\n");

    let names = session.eval("toString(names(proc.time()))");
    assert_eq!(
        names.output,
        "[1] \"user.self, sys.self, elapsed, user.child, sys.child\"\n"
    );

    let class = session.eval("class(proc.time())");
    assert_eq!(class.output, "[1] \"proc_time\"\n");
}

#[test]
fn test_try_catch_error_handler_subset() {
    let mut session = RSession::new();

    let pass = session.eval("tryCatch(1 + 2, error=function(e) \"caught\")");
    assert_eq!(pass.output, "[1] 3\n");

    let caught = session.eval("tryCatch(stop(\"boom\"), error=function(e) \"caught\")");
    assert_eq!(caught.output, "[1] \"caught\"\n");

    let message = session.eval("tryCatch(stop(\"boom\"), error=function(e) conditionMessage(e))");
    assert_eq!(message.output, "[1] \"boom\"\n");
}

#[test]
fn test_ls_lists_current_frame_like_r() {
    let mut session = RSession::new();

    let empty = session.eval("ls()");
    assert_eq!(empty.output, "character(0)\n");
    assert_eq!(empty.typed, RValue::StringVector(Vec::new()));

    let mut session = RSession::new();
    let sorted = session.eval("y <- 2; x <- 1; ls()");
    assert_eq!(sorted.output, "[1] \"x\" \"y\"\n");
    assert_eq!(sorted.typed, literal_string_vector(&["x", "y"]));

    let mut session = RSession::new();
    let hidden = session.eval(".hidden <- 1; visible <- 2; ls()");
    assert_eq!(hidden.output, "[1] \"visible\"\n");

    let all_names = session.eval("ls(all.names = TRUE)");
    assert_eq!(all_names.output, "[1] \".hidden\" \"visible\"\n");

    let mut session = RSession::new();
    let removed = session.eval("x <- 1; rm(\"x\"); length(ls())");
    assert_eq!(removed.output, "[1] 0\n");
}

#[test]
fn test_runtime_predicates_match_public_r_surface() {
    let mut session = RSession::new();

    let primitive = session.eval("is.primitive(sum)");
    assert_eq!(primitive.output, "[1] TRUE\n");
    assert_eq!(primitive.typed, RValue::Logical(Some(true)));

    let closure = session.eval("is.primitive(function(x) x)");
    assert_eq!(closure.output, "[1] FALSE\n");
    assert_eq!(closure.typed, RValue::Logical(Some(false)));

    let loaded = session.eval("is.loaded(\"R_init_base\")");
    assert_eq!(loaded.output, "[1] FALSE\n");
    assert_eq!(loaded.typed, RValue::Logical(Some(false)));

    let single = session.eval("is.single(1)");
    assert!(matches!(single.typed, RValue::Error(_)));
    assert_eq!(
        single.output.trim_end(),
        "Error in is.single(1) : type \"single\" unimplemented in R"
    );
}

#[test]
fn test_system_disabled_without_host_capabilities() {
    let mut session = RSession::new();
    let result = session.eval("system(\"printf hi\")");
    assert!(
        result
            .output
            .contains("system() is disabled by the session capability policy"),
        "unexpected output: {}",
        result.output
    );
}

#[test]
fn test_system_matches_r_output_and_status_shape() {
    let mut session = RSession::new();
    session.enable_host_process_capabilities();

    let streamed = session.eval("system(\"printf hi\")");
    assert_eq!(streamed.output, "hi");
    assert_eq!(streamed.typed, RValue::Integer(Some(0)));

    let interned = session.eval("system(\"printf hi\", intern = TRUE)");
    assert_eq!(interned.output, "[1] \"hi\"\n");
    assert_eq!(interned.typed, literal_string_vector(&["hi"]));

    let status = session.eval("status <- system(\"false\"); status");
    assert_eq!(status.output, "[1] 1\n");
}

#[test]
fn test_eval_subtraction() {
    let mut session = RSession::new();
    let result = session.eval("10 - 3");
    assert!(
        (result.value - 7.0).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_multiplication() {
    let mut session = RSession::new();
    let result = session.eval("4 * 5");
    assert!(
        (result.value - 20.0).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_division() {
    let mut session = RSession::new();
    let result = session.eval("10 / 3");
    assert!(
        (result.value - 3.3333333333333335).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_power() {
    let mut session = RSession::new();
    let result = session.eval("2 ^ 10");
    assert!(
        (result.value - 1024.0).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_precedence() {
    let mut session = RSession::new();
    let result = session.eval("2 + 3 * 4");
    assert!(
        (result.value - 14.0).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_comparison() {
    let mut session = RSession::new();
    let lt = session.eval("1 < 2");
    assert!((lt.value - 1.0).abs() < 1e-10);

    let gt = session.eval("3 > 5");
    assert!((gt.value - 0.0).abs() < 1e-10);

    let eq = session.eval("7 == 7");
    assert!((eq.value - 1.0).abs() < 1e-10);
}

#[test]
fn test_eval_arithmetic_recycling_and_integer_overflow_warnings() {
    let mut session = RSession::new();

    let recycled = session.eval("c(1, 2, 3) + c(10, 20)");
    assert!(
        recycled
            .output
            .contains("longer object length is not a multiple of shorter object length"),
        "output: {}",
        recycled.output
    );
    assert_eq!(
        recycled.typed,
        RValue::RealVector(vec![Some(11.0), Some(22.0), Some(13.0)])
    );

    let overflow = session.eval("c(1L, 2147483647L) + c(1L, 1L)");
    assert!(
        overflow.output.contains("NAs produced by integer overflow"),
        "output: {}",
        overflow.output
    );
    assert_eq!(overflow.typed, RValue::IntegerVector(vec![Some(2), None]));
}

#[test]
fn test_eval_arithmetic_preserves_vector_and_matrix_attributes() {
    let mut session = RSession::new();

    let named = session.eval("x <- c(a = 1, b = 2, c = 3)\nx + 1");
    assert_eq!(
        named.typed,
        RValue::Attributed {
            value: Box::new(RValue::RealVector(vec![Some(2.0), Some(3.0), Some(4.0)])),
            metadata: RMetadata {
                names: Some(vec![
                    Some("a".to_string()),
                    Some("b".to_string()),
                    Some("c".to_string()),
                ]),
                attributes: vec![RAttribute {
                    name: "names".to_string(),
                    value: RValue::StringVector(vec![
                        Some("a".to_string()),
                        Some("b".to_string()),
                        Some("c".to_string()),
                    ]),
                }],
                ..RMetadata::default()
            },
        }
    );

    let matrix = session.eval(
        "m <- matrix(1:4, nrow = 2)\n\
             dimnames(m) <- list(c(\"r1\", \"r2\"), c(\"c1\", \"c2\"))\n\
             m + 1",
    );
    assert_eq!(
        matrix.typed,
        RValue::Attributed {
            value: Box::new(RValue::RealVector(vec![
                Some(2.0),
                Some(3.0),
                Some(4.0),
                Some(5.0),
            ])),
            metadata: RMetadata {
                dim: Some(vec![2, 2]),
                // Stock attribute order: dim was set first by matrix(),
                // then dimnames by dimnames<-.
                attributes: vec![
                    RAttribute {
                        name: "dim".to_string(),
                        value: RValue::IntegerVector(vec![Some(2), Some(2)]),
                    },
                    RAttribute {
                        name: "dimnames".to_string(),
                        value: RValue::List(vec![
                            RValue::StringVector(vec![
                                Some("r1".to_string()),
                                Some("r2".to_string()),
                            ]),
                            RValue::StringVector(vec![
                                Some("c1".to_string()),
                                Some("c2".to_string()),
                            ]),
                        ]),
                    },
                ],
                ..RMetadata::default()
            },
        }
    );
}

#[test]
fn test_eval_character_and_logical_comparisons_follow_r_recycling() {
    let mut session = RSession::new();

    let character = session.eval("c(\"a\", \"b\", NA_character_) < c(\"b\", \"a\", \"c\")");
    assert_eq!(
        character.typed,
        RValue::LogicalVector(vec![Some(true), Some(false), None])
    );

    let logical = session.eval("c(TRUE, FALSE, NA) == c(1L, 0L, 1L)");
    assert_eq!(
        logical.typed,
        RValue::LogicalVector(vec![Some(true), Some(true), None])
    );
}

#[test]
fn test_eval_complex_arithmetic_returns_typed_complex_vectors() {
    let mut session = RSession::new();

    let sum = session.eval("as.complex(c(1, 2)) + as.complex(c(3, 4))");
    assert_eq!(
        sum.typed,
        RValue::ComplexVector(vec![
            Some(RComplexValue {
                real: 4.0,
                imaginary: 0.0,
            }),
            Some(RComplexValue {
                real: 6.0,
                imaginary: 0.0,
            }),
        ])
    );

    let product = session.eval("as.complex(c(1, 2)) * as.complex(c(3, 4))");
    assert_eq!(
        product.typed,
        RValue::ComplexVector(vec![
            Some(RComplexValue {
                real: 3.0,
                imaginary: 0.0,
            }),
            Some(RComplexValue {
                real: 8.0,
                imaginary: 0.0,
            }),
        ])
    );
}

#[test]
fn test_eval_assignment() {
    let mut session = RSession::new();
    let assign_result = session.eval("x <- 42");
    assert!(
        !assign_result.output.contains("Error"),
        "assign failed: {}",
        assign_result.output
    );

    let lookup = session.eval("x");
    assert!(
        !lookup.output.contains("Error"),
        "lookup failed: {}",
        lookup.output
    );

    let super_assign = session.eval("x <- 0; f <- function() { x <<- 1 }; f(); x");
    assert_eq!(super_assign.output, "[1] 1\n");
}

#[test]
fn test_eval_multiple_expressions_returns_last_value() {
    let mut session = RSession::new();
    let result = session.eval("x <- c(10, 20, 30)\nx");
    assert_eq!(result.output, "[1] 10 20 30\n");
}

#[test]
fn test_eval_integer_subassignment() {
    let mut session = RSession::new();
    let result = session.eval("x <- c(10, 20, 30)\nx[2] <- 99\nx");
    assert_eq!(result.output, "[1] 10 99 30\n");
}

#[test]
fn test_eval_logical_subset_recycles_and_preserves_na() {
    let mut session = RSession::new();
    let recycled = session.eval("(1:5)[c(TRUE, FALSE)]");
    let with_na = session.eval("(1:3)[c(TRUE, NA)]");
    let longer = session.eval("(1:3)[c(TRUE, FALSE, TRUE, TRUE)]");

    assert_eq!(recycled.output, "[1] 1 3 5\n");
    assert_eq!(with_na.output, "[1]  1 NA  3\n");
    assert_eq!(longer.output, "[1]  1  3 NA\n");
}

#[test]
fn test_eval_numeric_subset_preserves_na_and_negative_exclusion() {
    let mut session = RSession::new();
    let positive = session.eval("(1:3)[c(1, 0, NA, 4)]");
    let integer = session.eval("(1:3)[c(1L, 0L, NA_integer_, 4L)]");
    let negative = session.eval("(1:3)[-2]");

    assert_eq!(positive.output, "[1]  1 NA NA\n");
    assert_eq!(integer.output, "[1]  1 NA NA\n");
    assert_eq!(negative.output, "[1] 1 3\n");
}

#[test]
fn test_eval_double_bracket_subset() {
    let mut session = RSession::new();
    let list_numeric = session.eval("list(a = 10, b = 20)[[2]]");
    let list_name = session.eval("list(a = 10, b = 20)[[\"b\"]]");
    let atomic = session.eval("c(10, 20, 30)[[2]]");

    assert_eq!(list_numeric.output, "[1] 20\n");
    assert_eq!(list_name.output, "[1] 20\n");
    assert_eq!(atomic.output, "[1] 20\n");
}

#[test]
fn test_eval_sort_unique_rev() {
    let mut session = RSession::new();
    let sorted = session.eval("sort(c(3, 1, 2))");
    let sorted_desc = session.eval("sort(c(3, 1, 2), decreasing = TRUE)");
    let unique = session.eval("unique(c(1, 1, 2, 1))");
    let reversed = session.eval("rev(c(1, 2, 3))");

    assert_eq!(sorted.output, "[1] 1 2 3\n");
    assert_eq!(sorted_desc.output, "[1] 3 2 1\n");
    assert_eq!(unique.output, "[1] 1 2\n");
    assert_eq!(reversed.output, "[1] 3 2 1\n");
}

#[test]
fn test_eval_match_in_set_ops_and_extrema_indices() {
    let mut session = RSession::new();
    let matched = session.eval("match(c(2, 4), c(1, 2, 3))");
    let contains = session.eval("c(2, 4) %in% c(1, 2, 3)");
    let union = session.eval("union(c(1, 2), c(2, 3))");
    let intersect = session.eval("intersect(c(1, 2), c(2, 3))");
    let setdiff = session.eval("setdiff(c(1, 2, 3), c(2, 4))");
    let setequal = session.eval("setequal(c(1, 2), c(2, 1, 1))");
    let which_min = session.eval("which.min(c(3, 1, 2))");
    let which_max = session.eval("which.max(c(3, 1, 2))");

    assert_eq!(matched.output, "[1]  2 NA\n");
    assert_eq!(contains.output, "[1]  TRUE FALSE\n");
    assert_eq!(union.output, "[1] 1 2 3\n");
    assert_eq!(intersect.output, "[1] 2\n");
    assert_eq!(setdiff.output, "[1] 1 3\n");
    assert_eq!(setequal.output, "[1] TRUE\n");
    assert_eq!(which_min.output, "[1] 2\n");
    assert_eq!(which_max.output, "[1] 1\n");
}

#[test]
fn test_eval_matrix_sequence_and_logic_helpers() {
    let mut session = RSession::new();
    let any = session.eval("any(c(FALSE, TRUE))");
    let all = session.eval("all(c(TRUE, FALSE))");
    let matrix = session.eval("matrix(1:4, nrow = 2)");
    let dim = session.eval("dim(matrix(1:4, nrow = 2))");
    let nrow = session.eval("nrow(matrix(1:4, nrow = 2))");
    let ncol = session.eval("ncol(matrix(1:4, nrow = 2))");
    let diag = session.eval("diag(3)");
    let diff = session.eval("diff(c(1, 4, 9))");
    let names = session.eval("names(setNames(c(1, 2), c(\"a\", \"b\")))");
    let seq_len = session.eval("seq_len(3)");
    let seq_along = session.eval("seq_along(c(4, 5))");

    assert_eq!(any.output, "[1] TRUE\n");
    assert_eq!(all.output, "[1] FALSE\n");
    assert_eq!(
        matrix.output,
        "     [,1] [,2]\n[1,]    1    3\n[2,]    2    4\n"
    );
    assert_eq!(dim.output, "[1] 2 2\n");
    assert_eq!(nrow.output, "[1] 2\n");
    assert_eq!(ncol.output, "[1] 2\n");
    assert_eq!(
        diag.output,
        "     [,1] [,2] [,3]\n[1,]    1    0    0\n[2,]    0    1    0\n[3,]    0    0    1\n"
    );
    assert_eq!(diff.output, "[1] 3 5\n");
    assert_eq!(names.output, "[1] \"a\" \"b\"\n");
    assert_eq!(seq_len.output, "[1] 1 2 3\n");
    assert_eq!(seq_along.output, "[1] 1 2\n");
}

#[test]
fn test_eval_environment_file_and_predicate_helpers() {
    let mut session = RSession::new();
    let missing = session.eval("exists(\"x\")");
    let assigned = session.eval("assign(\"y\", 2)\nget(\"y\")");
    let removed = session.eval("x <- 1\nrm(\"x\")\nexists(\"x\")");
    let tempdir_exists = session.eval("file.exists(tempdir())");
    let inherits = session.eval("inherits(1, \"numeric\")");
    let to_string = session.eval("toString(c(1, 2, 3))");
    let is_vector = session.eval("is.vector(c(1, 2))");
    let is_data_frame = session.eval("is.data.frame(data.frame(a = 1))");

    assert_eq!(missing.output, "[1] FALSE\n");
    assert_eq!(assigned.output, "[1] 2\n");
    assert_eq!(removed.output, "[1] FALSE\n");
    assert_eq!(tempdir_exists.output, "[1] TRUE\n");
    assert_eq!(inherits.output, "[1] TRUE\n");
    assert_eq!(to_string.output, "[1] \"1, 2, 3\"\n");
    assert_eq!(is_vector.output, "[1] TRUE\n");
    assert_eq!(is_data_frame.output, "[1] TRUE\n");
}

#[test]
fn test_eval_negative_row_exclusion_can_create_empty_data_frame() {
    let mut session = RSession::new();
    let setup = session.eval(
        "x <- data.frame(a = 1:3)\n\
             y <- x[-(1:3), , drop = FALSE]",
    );

    assert_eq!(setup.output, "");
    assert_eq!(session.eval("dim(y)").output, "[1] 0 1\n");
    assert_eq!(session.eval("nrow(y)").output, "[1] 0\n");
    assert_eq!(session.eval("ncol(y)").output, "[1] 1\n");
    assert_eq!(session.eval("names(y)").output, "[1] \"a\"\n");
    assert_eq!(session.eval("is.data.frame(y)").output, "[1] TRUE\n");
}

#[test]
fn test_eval_registered_distribution_and_cumulative_helpers() {
    let mut session = RSession::new();
    let dnorm = session.eval("dnorm(0)");
    let pnorm = session.eval("pnorm(0)");
    let qnorm = session.eval("qnorm(0.5)");
    let dpois = session.eval("dpois(2, 3)");
    let dbinom = session.eval("dbinom(2, 5, 0.5)");
    let dgamma = session.eval("dgamma(2, 3)");
    let dcauchy = session.eval("dcauchy(0)");
    let cumsum = session.eval("cumsum(c(1, 2, 3))");
    let cumprod = session.eval("cumprod(c(1, 2, 3))");
    let cumvar_first = session.eval("is.na(cumvar(c(1, 2, 3))[1])");
    let cumvar_second = session.eval("cumvar(c(1, 2, 3))[2]");
    let cumvar_third = session.eval("cumvar(c(1, 2, 3))[3]");

    assert!((dnorm.value - 0.3989422804014327).abs() < 1e-12);
    assert_eq!(pnorm.output, "[1] 0.5\n");
    assert_eq!(qnorm.output, "[1] 0\n");
    assert!((dpois.value - 0.22404180765538775).abs() < 1e-12);
    assert!((dbinom.value - 0.3125).abs() < 1e-12);
    assert!((dgamma.value - 0.2706705664732254).abs() < 1e-12);
    assert!((dcauchy.value - 0.3183098861837907).abs() < 1e-12);
    assert_eq!(cumsum.output, "[1] 1 3 6\n");
    assert_eq!(cumprod.output, "[1] 1 2 6\n");
    assert_eq!(cumvar_first.output, "[1] TRUE\n");
    assert_eq!(cumvar_second.output, "[1] 0.5\n");
    assert_eq!(cumvar_third.output, "[1] 1\n");
}

#[test]
fn test_eval_normal_helpers_respect_tail_and_log_flags() {
    let mut session = RSession::new();

    let dnorm_log = session.eval("dnorm(0, 0, 1, TRUE)");
    let pnorm_upper = session.eval("pnorm(1, 0, 1, FALSE, FALSE)");
    let pnorm_log = session.eval("pnorm(1, 0, 1, TRUE, TRUE)");
    let qnorm_upper = session.eval("qnorm(0.25, 0, 1, FALSE, FALSE)");
    let qnorm_log = session.eval("qnorm(log(0.25), 0, 1, TRUE, TRUE)");

    assert!((dnorm_log.value - session.dnorm(0.0, 0.0, 1.0, true)).abs() < 1e-12);
    assert!((pnorm_upper.value - session.pnorm(1.0, 0.0, 1.0, false, false)).abs() < 1e-12);
    assert!((pnorm_log.value - session.pnorm(1.0, 0.0, 1.0, true, true)).abs() < 1e-12);
    assert!((qnorm_upper.value - session.qnorm(0.25, 0.0, 1.0, false, false)).abs() < 1e-12);
    assert!((qnorm_log.value - session.qnorm(0.25_f64.ln(), 0.0, 1.0, true, true)).abs() < 1e-12);
}

#[test]
fn test_eval_raw_string_helpers() {
    let mut session = RSession::new();
    let as_raw = session.eval("as.raw(c(65, 90))");
    let char_to_raw = session.eval("charToRaw(\"AZ\")");
    let raw_to_char = session.eval("rawToChar(as.raw(c(65, 90)))");

    assert_eq!(as_raw.output, "[1] 41 5a\n");
    assert_eq!(char_to_raw.output, "[1] 41 5a\n");
    assert_eq!(raw_to_char.output, "[1] \"AZ\"\n");
}

#[test]
fn test_eval_replacement_and_order_predicate_helpers() {
    let mut session = RSession::new();
    let names = session.eval("x <- c(1, 2)\nnames(x) <- c(\"a\", \"b\")\nnames(x)");
    let class = session.eval("x <- 1\nclass(x) <- \"foo\"\nclass(x)");
    let unsorted = session.eval("is.unsorted(c(1, 3, 2))");
    let sorted = session.eval("is.unsorted(c(1, 2, 3))");

    assert_eq!(names.output, "[1] \"a\" \"b\"\n");
    assert_eq!(class.output, "[1] \"foo\"\n");
    assert_eq!(unsorted.output, "[1] TRUE\n");
    assert_eq!(sorted.output, "[1] FALSE\n");
}

#[test]
fn test_eval_additional_distribution_helpers() {
    let mut session = RSession::new();
    let cases = [
        ("dexp(1)", "[1] 0.3678794\n"),
        ("pexp(1)", "[1] 0.6321206\n"),
        ("dbeta(0.5, 2, 3)", "[1] 1.5\n"),
        ("pbeta(0.5, 2, 3)", "[1] 0.6875\n"),
        ("qbeta(0.5, 2, 3)", "[1] 0.3857276\n"),
        ("dt(0, 5)", "[1] 0.3796067\n"),
        ("pt(0, 5)", "[1] 0.5\n"),
        ("qt(0.5, 5)", "[1] 0\n"),
        ("dchisq(2, 3)", "[1] 0.2075537\n"),
        ("pchisq(2, 3)", "[1] 0.4275933\n"),
        ("qchisq(0.5, 3)", "[1] 2.365974\n"),
        ("dweibull(2, 3)", "[1] 0.004025552\n"),
        ("pweibull(2, 3)", "[1] 0.9996645\n"),
        ("qweibull(0.5, 3)", "[1] 0.884997\n"),
        ("df(1, 5, 10)", "[1] 0.4954798\n"),
        ("pf(1, 5, 10)", "[1] 0.5348806\n"),
        ("qf(0.5, 5, 10)", "[1] 0.9319332\n"),
        ("dnbinom(2, 5, 0.5)", "[1] 0.1171875\n"),
        ("pnbinom(2, 5, 0.5)", "[1] 0.2265625\n"),
        ("qnbinom(0.5, 5, 0.5)", "[1] 4\n"),
        ("dgeom(2, 0.5)", "[1] 0.125\n"),
        ("pgeom(2, 0.5)", "[1] 0.875\n"),
        ("qgeom(0.5, 0.5)", "[1] 0\n"),
    ];

    for (code, expected) in cases {
        assert_eq!(session.eval(code).output, expected, "{code}");
    }
}

#[test]
fn test_eval_named_vector_names() {
    let mut session = RSession::new();
    let result = session.eval("names(c(a = 1, b = 2))");
    assert_eq!(result.output, "[1] \"a\" \"b\"\n");
}

#[test]
fn test_eval_for_loop_atomic_and_factor_sequences() {
    let mut session = RSession::new();
    let cases = [
        (
            "out <- c(); for (x in c(TRUE, FALSE)) out <- c(out, x); paste(out, collapse = ',')",
            "[1] \"TRUE,FALSE\"\n",
        ),
        (
            "out <- c(); for (x in c(1L, 2L)) out <- c(out, x); paste(out, collapse = ',')",
            "[1] \"1,2\"\n",
        ),
        (
            "out <- c(); for (x in c(1.5, 2.5)) out <- c(out, x); paste(out, collapse = ',')",
            "[1] \"1.5,2.5\"\n",
        ),
        (
            "out <- c(); for (x in c('a', 'b')) out <- c(out, x); paste(out, collapse = ',')",
            "[1] \"a,b\"\n",
        ),
        (
            "out <- raw(); for (x in as.raw(c(65, 90))) out <- c(out, x); out",
            "[1] 41 5a\n",
        ),
        (
            "out <- c(); for (x in list(1, 2)) out <- c(out, x); paste(out, collapse = ',')",
            "[1] \"1,2\"\n",
        ),
        (
            "out <- c(); for (x in factor(c('b', 'a'), levels = c('a', 'b'))) out <- c(out, x); paste(out, collapse = ',')",
            "[1] \"b,a\"\n",
        ),
    ];

    for (code, expected) in cases {
        assert_eq!(session.eval(code).output, expected, "{code}");
    }
}

#[test]
fn test_eval_c_empty_and_raw_coercion_match_r() {
    let mut session = RSession::new();
    let cases = [
        ("typeof(c())", "[1] \"NULL\"\n"),
        ("typeof(c(raw()))", "[1] \"raw\"\n"),
        ("typeof(c(raw(), as.raw(1)))", "[1] \"raw\"\n"),
        ("typeof(c(as.raw(1), TRUE))", "[1] \"logical\"\n"),
        ("c(as.raw(65), as.raw(90))", "[1] 41 5a\n"),
    ];

    for (code, expected) in cases {
        assert_eq!(session.eval(code).output, expected, "{code}");
    }
}

#[test]
fn test_eval_lapply_prints_list() {
    let mut session = RSession::new();
    let result = session.eval("lapply(c(1, 2), function(x) x + 1)");
    assert_eq!(result.output.trim_end(), "[[1]]\n[1] 2\n\n[[2]]\n[1] 3");
}

#[test]
fn test_eval_dollar_list_access() {
    let mut session = RSession::new();
    let exact = session.eval("x <- list(a = 1, b = 2)\nx$a");
    let partial = session.eval("x <- list(alpha = 11, beta = 22)\nx$al");
    let missing = session.eval("x <- list(a = 1)\nx$b");
    assert_eq!(exact.output, "[1] 1\n");
    assert_eq!(partial.output, "[1] 11\n");
    assert_eq!(missing.output, "NULL\n");
}

#[test]
fn test_eval_mean_numeric() {
    let mut session = RSession::new();
    let numeric = session.eval("mean(c(1, 2, 3))");
    let sequence = session.eval("mean(1:4)");
    let na = session.eval("mean(c(1, NA))");
    let na_removed = session.eval("mean(c(1, NA), na.rm = TRUE)");
    let nan_then_na = session.eval("mean(c(NaN, NA))");
    let na_then_nan = session.eval("mean(c(NA, NaN))");
    assert_eq!(numeric.output, "[1] 2\n");
    assert_eq!(sequence.output, "[1] 2.5\n");
    assert_eq!(na.output, "[1] NA\n");
    assert_eq!(na_removed.output, "[1] 1\n");
    // GNU .Internal(mean) keeps the first ISNAN payload; NA does not trump NaN.
    assert_eq!(nan_then_na.output, "[1] NaN\n");
    assert_eq!(na_then_nan.output, "[1] NA\n");
}

#[test]
fn test_eval_summary_na_rm() {
    let mut session = RSession::new();
    let sum = session.eval("sum(c(1, NA, 3), na.rm = TRUE)");
    let min = session.eval("min(c(3, NA, 1), na.rm = TRUE)");
    let range = session.eval("range(c(3, NA, 1), na.rm = TRUE)");
    assert_eq!(sum.output, "[1] 4\n");
    assert_eq!(min.output, "[1] 1\n");
    assert_eq!(range.output, "[1] 1 3\n");
}

#[test]
fn test_eval_summary_type_and_missing_parity() {
    let mut session = RSession::new();

    assert_eq!(session.eval("sum(integer(0))").output, "[1] 0\n");
    assert_eq!(session.eval("prod(integer(0))").output, "[1] 1\n");
    assert_eq!(
        session.eval("typeof(sum(1L, 2L))").output,
        "[1] \"integer\"\n"
    );
    assert_eq!(
        session.eval("typeof(sum(1L, 2.5))").output,
        "[1] \"double\"\n"
    );
    assert_eq!(
        session.eval("typeof(range(1L:3L))").output,
        "[1] \"integer\"\n"
    );
    assert_eq!(
        session.eval("range(c(1L, NA_integer_))").output,
        "[1] NA NA\n"
    );
    assert_eq!(session.eval("sum(NaN)").output, "[1] NaN\n");
    assert_eq!(session.eval("sum(1+2i, 3+4i)").output, "[1] 4+6i\n");
    assert_eq!(session.eval("prod(1+2i, 3+4i)").output, "[1] -5+10i\n");
}

#[test]
fn test_eval_factor_labels() {
    let mut session = RSession::new();
    let result = session.eval("x <- factor(c(\"b\", \"a\", \"b\", \"c\"))\nx");
    assert_eq!(result.output, "[1] b a b c\nLevels: a b c\n");
}

#[test]
fn test_eval_closure_positional_arg() {
    let mut session = RSession::new();
    let result = session.eval("f <- function(x) x + 1\nf(41)");
    assert_eq!(result.output, "[1] 42\n");
}

#[test]
fn test_eval_closure_lexical_scope() {
    let mut session = RSession::new();
    let result = session.eval("make <- function(x) function(y) x + y\nadd2 <- make(2)\nadd2(40)");
    assert_eq!(result.output, "[1] 42\n");
}

#[test]
fn test_eval_closure_default_arg() {
    let mut session = RSession::new();
    let result = session.eval("f <- function(x, y = x + 1) y\nf(41)");
    assert_eq!(result.output, "[1] 42\n");
}

#[test]
fn test_eval_closure_lazy_unused_arg() {
    let mut session = RSession::new();
    let result = session.eval("f <- function(x) 1\nf(unknown_symbol)");
    assert_eq!(result.output, "[1] 1\n");
}

#[test]
fn test_eval_closure_named_args() {
    let mut session = RSession::new();
    let result = session.eval("f <- function(x, y) x + y\nf(y = 40, x = 2)");
    assert_eq!(result.output, "[1] 42\n");
}

#[test]
fn test_eval_closure_return() {
    let mut session = RSession::new();
    let result = session.eval("f <- function() return(42)\nf()");
    assert_eq!(result.output, "[1] 42\n");
}

#[test]
fn test_eval_on_exit_runs_for_normal_and_return_paths() {
    let mut session = RSession::new();

    let normal = session.eval(
        "x <- 0; f <- function() { on.exit({ x <<- 1 }); 42 }; y <- f(); all(c(y == 42, x == 1))",
    );
    assert_eq!(normal.typed, RValue::Logical(Some(true)));

    let returned = session.eval(
            "x <- 0; f <- function() { on.exit({ x <<- 2 }); return(7); x <<- 99 }; y <- f(); all(c(y == 7, x == 2))",
        );
    assert_eq!(returned.typed, RValue::Logical(Some(true)));
}

#[test]
fn test_eval_on_exit_add_and_after_order() {
    let mut session = RSession::new();

    let fifo = session.eval(
            "x <- c(); f <- function() { on.exit({ x <<- c(x, 1) }, add = TRUE); on.exit({ x <<- c(x, 2) }, add = TRUE) }; f(); all(x == c(1, 2))",
        );
    assert_eq!(fifo.typed, RValue::Logical(Some(true)));

    let lifo = session.eval(
            "x <- c(); f <- function() { on.exit({ x <<- c(x, 1) }, add = TRUE); on.exit({ x <<- c(x, 2) }, add = TRUE, after = FALSE) }; f(); all(x == c(2, 1))",
        );
    assert_eq!(lifo.typed, RValue::Logical(Some(true)));
}

#[test]
fn test_nested_on_exit_runs_before_return() {
    let mut session = RSession::new();
    let result = session.eval(
            "x <- c(); outer <- function() { on.exit({ x <<- c(x, \"o\") }); inner <- function() { on.exit({ x <<- c(x, \"i\") }); return(7) }; inner() }; y <- outer(); all(c(y == 7, identical(x, c(\"i\", \"o\"))))",
        );
    assert_eq!(result.typed, RValue::Logical(Some(true)));
}

#[test]
fn test_return_value_survives_gc_in_on_exit() {
    let mut session = RSession::new();
    let result = session.eval(
            "f <- function() { value <- seq_len(2000); on.exit(gc()); value }; y <- f(); identical(y, seq_len(2000))",
        );
    assert_eq!(result.typed, RValue::Logical(Some(true)));
}

#[test]
fn test_on_exit_with_break_next_same_frame() {
    let mut session = RSession::new();
    // break/next must share the loop's cloenv (GNU findcontext); nested
    // function break is a top-level error. Exercise same-frame control
    // flow plus the enclosing function's on.exit.
    let brk = session.eval(
            "x <- c(); f <- function() { on.exit({ x <<- c(x, \"e\") }); for (i in 1:3) { if (i == 2) break; x <<- c(x, i) }; x <<- c(x, \"d\") }; f(); all(identical(x, c(1, \"d\", \"e\")))",
        );
    assert_eq!(brk.typed, RValue::Logical(Some(true)));

    let nxt = session.eval(
            "x <- c(); f <- function() { on.exit({ x <<- c(x, \"e\") }); for (i in 1:3) { if (i == 2) next; x <<- c(x, i) } }; f(); all(identical(x, c(1, 3, \"e\")))",
        );
    assert_eq!(nxt.typed, RValue::Logical(Some(true)));
}

#[test]
fn test_on_exit_and_tryCatch_with_return_and_error() {
    let mut session = RSession::new();
    let returned = session.eval(
            "x <- 0; f <- function() { on.exit({ x <<- x + 1 }); tryCatch(return(5), error = function(e) 0) }; y <- f(); all(c(y == 5, x == 1))",
        );
    assert_eq!(returned.typed, RValue::Logical(Some(true)));

    let caught = session.eval(
            "x <- 0; f <- function() { on.exit({ x <<- x + 10 }); tryCatch(stop(\"boom\"), error = function(e) 42) }; y <- f(); all(c(y == 42, x == 10))",
        );
    assert_eq!(caught.typed, RValue::Logical(Some(true)));

    // tryCatch does not change rho, so break targets the enclosing loop.
    let break_through = session.eval(
            "x <- c(); for (i in 1:3) { tryCatch({ x <<- c(x, \"t\"); break }, error = function(e) NULL); x <<- c(x, \"L\") }; all(identical(x, c(\"t\")))",
        );
    assert_eq!(break_through.typed, RValue::Logical(Some(true)));
}

#[test]
fn test_s4_registry_constructs_slot_objects() {
    let mut session = RSession::new();

    let result = session.eval(
            "setClass(\"Person\", slots = c(name = \"character\", age = \"numeric\")); x <- new(\"Person\", name = \"Ada\", age = 37); all(c(isS4(x), is(x, \"Person\"), slot(x, \"name\") == \"Ada\", slot(x, \"age\") == 37, all(slotNames(\"Person\") == c(\"name\", \"age\"))))",
        );
    assert_eq!(result.typed, RValue::Logical(Some(true)));
}

#[test]
fn test_s4_registry_validity_virtual_and_slot_errors() {
    let mut session = RSession::new();

    let validity = session.eval(
            "setClass(\"Checked\", slots = c(value = \"numeric\")); setValidity(\"Checked\", function(object) TRUE); TRUE",
        );
    assert_eq!(validity.typed, RValue::Logical(Some(true)));

    let virtual_class =
        session.eval("setClass(\"VirtualThing\", \"VIRTUAL\"); isVirtualClass(\"VirtualThing\")");
    assert_eq!(virtual_class.typed, RValue::Logical(Some(true)));

    let virtual_new = session.eval("new(\"VirtualThing\")");
    assert!(
        matches!(&virtual_new.typed, RValue::Error(message) if message.contains("virtual")),
        "{virtual_new:?}"
    );

    let unknown_slot = session
        .eval("setClass(\"Strict\", slots = c(name = \"character\")); new(\"Strict\", nope = 1)");
    assert!(
        matches!(&unknown_slot.typed, RValue::Error(message) if message.contains("nope")),
        "{unknown_slot:?}"
    );
}

#[test]
fn test_eval_missing_arg() {
    let mut session = RSession::new();
    let missing = session.eval("f <- function(x) missing(x)\nf()");
    let present = session.eval("f <- function(x) missing(x)\nf(1)");
    assert_eq!(missing.output, "[1] TRUE\n");
    assert_eq!(present.output, "[1] FALSE\n");
}

#[test]
fn test_eval_missing_arg_error() {
    let mut session = RSession::new();
    let result = session.eval("f <- function(x) x\nf()");
    // Call attribution is now faithful to stock R: the error carries the
    // failing call, like Rscript's stderr rendering.
    assert_eq!(
        result.output,
        "Error in f() : argument \"x\" is missing, with no default\n"
    );
}

#[test]
fn test_sessions_keep_globals_isolated_on_same_thread() {
    let mut left = RSession::new();
    let mut right = RSession::new();

    assert!(!left.eval("x <- 11").output.contains("Error"));
    assert!(!right.eval("x <- 29").output.contains("Error"));

    let left_x = left.eval("x");
    let right_x = right.eval("x");

    assert!((left_x.value - 11.0).abs() < 1e-10, "left: {left_x:?}");
    assert!((right_x.value - 29.0).abs() < 1e-10, "right: {right_x:?}");
}

#[test]
fn test_parallel_sessions_keep_globals_isolated() {
    let left = std::thread::spawn(|| {
        let mut session = RSession::new();
        assert!(!session.eval("x <- 101").output.contains("Error"));
        session.eval("x").value
    });
    let right = std::thread::spawn(|| {
        let mut session = RSession::new();
        assert!(!session.eval("x <- 202").output.contains("Error"));
        session.eval("x").value
    });

    assert!((left.join().expect("left session panicked") - 101.0).abs() < 1e-10);
    assert!((right.join().expect("right session panicked") - 202.0).abs() < 1e-10);
}

#[test]
fn test_parallel_android_sessions_stress_isolated_state_paths_and_cancellation() {
    const WORKERS: usize = 4;
    const ITERS: usize = 8;

    let handles = (0..WORKERS)
        .map(|worker| {
            std::thread::spawn(move || {
                let root = unique_test_root(&format!("android-parallel-{worker}"));
                let files = root.join("files");
                let cache = root.join("cache");
                let bundled = root.join("bundled-library");
                let package = format!("pkg{worker}");
                let package_dir = bundled.join(&package);
                std::fs::create_dir_all(&package_dir).expect("package dir");
                std::fs::write(
                    package_dir.join("DESCRIPTION"),
                    format!("Package: {package}\nVersion: 0.0.1\n"),
                )
                .expect("description");

                let mut session = RSession::new();
                session
                    .configure_paths(
                        files.to_str().expect("utf8 files path"),
                        cache.to_str().expect("utf8 cache path"),
                        Some(bundled.to_str().expect("utf8 bundled path")),
                    )
                    .expect("configure paths");
                session.set_seed(100 + worker as u32, 200 + worker as u32);

                let mut rng_bits = Vec::with_capacity(ITERS);
                for iter in 0..ITERS {
                    let cancelled = CancellationToken::cancelled();
                    let cancelled_result =
                        session.eval_with_cancellation_token("1 + 1", Some(cancelled));
                    assert_eq!(cancelled_result.output, "Error: operation cancelled\n");

                    let code = format!("local_value <- {}; local_value", worker * 100 + iter);
                    let result = session.eval(&code);
                    assert_eq!(result.value, (worker * 100 + iter) as f64, "{result:?}");
                    assert_eq!(
                        session.eval("exists(\"local_value\")").typed,
                        RValue::Logical(Some(true))
                    );
                    rng_bits.push(session.unif_rand().to_bits());
                }

                assert!(session.package_available(&package));
                assert_eq!(
                    session.package_path(&package),
                    Some(package_dir.to_string_lossy().into_owned())
                );
                let temp_dir = cache.join("Rtmp").to_string_lossy().into_owned();
                assert_eq!(
                    session.eval("tempdir()").typed,
                    string_vector(vec![temp_dir.clone()])
                );
                assert_eq!(session.eval("1 + 1").output, "[1] 2\n");

                let _ = std::fs::remove_dir_all(root);
                (worker, rng_bits, temp_dir)
            })
        })
        .collect::<Vec<_>>();

    let summaries = handles
        .into_iter()
        .map(|handle| handle.join().expect("parallel session worker panicked"))
        .collect::<Vec<_>>();

    assert_eq!(summaries.len(), WORKERS);
    let temp_dirs = summaries
        .iter()
        .map(|(_, _, temp_dir)| temp_dir.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(temp_dirs.len(), WORKERS);

    let rng_sequences = summaries
        .iter()
        .map(|(_, rng_bits, _)| rng_bits.as_slice())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(rng_sequences.len(), WORKERS);
}

#[test]
fn test_eval_cancellation_token_is_scoped_to_session_call() {
    let mut cancelled = RSession::new();
    let mut active = RSession::new();
    let flag = CancellationToken::cancelled();

    let cancelled_result = cancelled.eval_with_cancellation_token("1 + 1", Some(flag));
    assert_eq!(cancelled_result.output, "Error: operation cancelled\n");

    let active_result = active.eval("1 + 1");
    assert_eq!(active_result.output, "[1] 2\n");

    let next_cancelled_eval = cancelled.eval("1 + 1");
    assert_eq!(next_cancelled_eval.output, "[1] 2\n");
}

#[test]
fn test_sessions_keep_rng_isolated_on_same_thread() {
    let left = RSession::new();
    let right = RSession::new();

    // Identical seeds reproduce one stream per session without leaking
    // across concurrently-live sessions (fresh sessions randomize lazily
    // from the clock, like stock R without a .Random.seed).
    left.set_seed(10, 20);
    right.set_seed(10, 20);
    let left_first = left.unif_rand();
    let right_first = right.unif_rand();
    let left_second = left.unif_rand();
    let right_second = right.unif_rand();

    assert_eq!(left_first, right_first);
    assert_eq!(left_second, right_second);
    assert_ne!(left_first, left_second);
}

#[test]
fn test_session_seed_is_local() {
    let left = RSession::new();
    let right = RSession::new();

    left.set_seed(10, 20);
    right.set_seed(10, 20);
    assert_eq!(left.unif_rand(), right.unif_rand());

    left.set_seed(30, 40);
    assert_ne!(left.unif_rand(), right.unif_rand());
}

#[test]
fn test_sessions_keep_rlevel_mt_stream_isolated() {
    // R-level set.seed (Mersenne-Twister default) is per-session: two
    // concurrently-live sessions seeded differently and evaluated in
    // interleaved order each replay their own stock stream
    // (goldens from R 4.6.1 / 4.7.0-dev, identical on both).
    let stock_seed1 = [0.26550866314209998, 0.37212389963679016];
    let stock_seed5 = [0.20021445257589221, 0.68521859566681087];

    let mut left = RSession::new();
    let mut right = RSession::new();
    left.eval("set.seed(1)");
    right.eval("set.seed(5)");

    // Interleaved draws: each session consumes only its own stream.
    let l1 = left.eval("runif(1)").value;
    let r1 = right.eval("runif(1)").value;
    let l2 = left.eval("runif(1)").value;
    let r2 = right.eval("runif(1)").value;

    assert_eq!(l1, stock_seed1[0]);
    assert_eq!(l2, stock_seed1[1]);
    assert_eq!(r1, stock_seed5[0]);
    assert_eq!(r2, stock_seed5[1]);

    // The persisted kind word is session-local too (MT default =
    // 10403), and the two sessions hold different engine states.
    assert_eq!(left.eval(".Random.seed[1]").value, 110403.0);
    assert_eq!(right.eval(".Random.seed[1]").value, 110403.0);
    assert_ne!(
        left.eval(".Random.seed[3]").value,
        right.eval(".Random.seed[3]").value
    );
}

#[test]
fn test_runif_rlevel_contract_and_rng_consumption() {
    let mut session = RSession::new();
    let seeded = session.eval("set.seed(1); runif(3)");
    assert_eq!(seeded.output, "[1] 0.2655087 0.3721239 0.5728534\n");

    // Defaults, vector recycling, and a zero-length request match R's
    // vectorized primitive and do not consume an extra draw for n = 0.
    let recycled = session.eval("set.seed(1); runif(2, c(10, 20), 30)");
    assert!(recycled.output.contains("15.31017"));
    assert!(recycled.output.contains("23.721"));
    let zero = session.eval("set.seed(1); runif(0); runif(1)");
    assert!(zero.output.contains("0.2655087"));

    // Equal bounds return the bound without consuming the RNG stream;
    // reversed and non-finite bounds raise the standard R error.
    let unchanged = session.eval("set.seed(1); runif(1, 4, 4); runif(1)");
    assert!(unchanged.output.contains("4\n[1] 0.2655087"));
    assert!(session.eval("runif(1, 2, 1)").value.is_nan());
    assert!(session.eval("runif(1, NaN, 1)").value.is_nan());
}

#[test]
fn test_eval_null() {
    let mut session = RSession::new();
    let result = session.eval("NULL");
    assert_eq!(result.output, "NULL\n");
}

#[test]
fn test_eval_unary_minus() {
    let mut session = RSession::new();
    let result = session.eval("-5");
    assert!(
        (result.value - (-5.0)).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_complex_expr() {
    let mut session = RSession::new();
    let result = session.eval("(1 + 2) * (3 + 4)");
    assert!(
        (result.value - 21.0).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_true_false() {
    let mut session = RSession::new();
    let t = session.eval("TRUE");
    assert!((t.value - 1.0).abs() < 1e-10);

    let f = session.eval("FALSE");
    assert!((f.value - 0.0).abs() < 1e-10);
}

fn adversarial_iterations(default: u64) -> u64 {
    std::env::var("RPORT_ADVERSARIAL_ITERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[test]
fn adversarial_eval_errors_and_subsets_do_not_panic() {
    let fixed = [
        "stop('intentional')",
        "f <- function(x) x; f()",
        "c(1, 2, 3)[[10]]",
        "c(1, 2, 3)[c(1, -2)]",
        "list(a = 1)$missing$value",
        "if (c(TRUE, FALSE)) 1 else 2",
        "while (TRUE) { stop('bounded error') }",
    ];

    for code in fixed {
        let mut session = RSession::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.eval(code)));
        assert!(result.is_ok(), "eval panicked for fixed input: {code:?}");
    }

    for seed in 0..adversarial_iterations(128).min(512) {
        let mut session = RSession::new();
        let a = (seed % 7) as i32 - 2;
        let b = ((seed / 7) % 7) as i32 - 2;
        let c = ((seed / 49) % 7) as i32 - 2;
        let code = format!("c(10, 20, 30)[c({a}, {b}, {c})]");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.eval(&code)));
        assert!(
            result.is_ok(),
            "subset eval panicked for seed {seed}: {code}"
        );
    }
}

#[test]
fn adversarial_owned_value_conversion_handles_generated_vectors() {
    for seed in 0..adversarial_iterations(32).min(128) {
        let mut session = RSession::new();
        let len = (seed % 8) + 1;
        let values = (0..len)
            .map(|idx| ((seed + idx * 13) % 17).to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let numeric = session.eval(&format!("c({values})"));
        match numeric.typed {
            RValue::RealVector(items) => assert_eq!(items.len(), len as usize),
            RValue::Real(_) if len == 1 => {}
            ref other => panic!("expected real vector for seed {seed}, got {other:?}"),
        }

        let strings = (0..len)
            .map(|idx| format!("\"s{seed}_{idx}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let character = session.eval(&format!("c({strings})"));
        match character.typed {
            RValue::StringVector(items) => assert_eq!(items.len(), len as usize),
            ref other => panic!("expected string vector for seed {seed}, got {other:?}"),
        }
    }
}
