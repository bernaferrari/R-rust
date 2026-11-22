use r_embed::RSession;

const ORDER_RAW_SCRIPT: &str = r##"
    raw_value <- charToRaw("A test string")
    order_error <- tryCatch(order(raw_value), error=function(error) error)
    multi_order_error <- tryCatch(order(rep(1, 13), raw_value), error=function(error) error)
    first_multi_order_error <- tryCatch(order(raw_value, 1:13), error=function(error) error)
    rank_error <- tryCatch(rank(raw_value), error=function(error) error)
    sort_error <- tryCatch(sort(raw_value), error=function(error) error)
    partial_sort_error <- tryCatch(sort(raw_value, partial=5), error=function(error) error)
    stopifnot(inherits(order_error, "error"))
    stopifnot(identical(conditionMessage(order_error),
        "unimplemented type 'raw' in 'orderVector1'\n"))
    stopifnot(inherits(multi_order_error, "error"))
    stopifnot(identical(conditionMessage(multi_order_error),
        "unimplemented type 'raw' in 'listgreater'\n"))
    stopifnot(inherits(first_multi_order_error, "error"))
    stopifnot(identical(conditionMessage(first_multi_order_error),
        "unimplemented type 'raw' in 'listgreater'\n"))
    stopifnot(inherits(rank_error, "error"))
    stopifnot(identical(conditionMessage(rank_error), "raw vectors cannot be sorted"))
    stopifnot(inherits(sort_error, "error"))
    stopifnot(identical(conditionMessage(sort_error), "raw vectors cannot be sorted"))
    stopifnot(inherits(partial_sort_error, "error"))
    stopifnot(identical(conditionMessage(partial_sort_error), "raw vectors cannot be sorted"))

    stopifnot(identical(order(c(3, 1, 2)), c(2L, 3L, 1L)))
    stopifnot(identical(rank(c(3, 1, 2)), c(3, 1, 2)))
    stopifnot(identical(sort(c(3, 1, 2)), c(1, 2, 3)))
    stopifnot(identical(order(c(1, 1, 2), c(3, 1, 2)), c(2L, 1L, 3L)))
    stopifnot(identical(order(1:13, raw_value), 1:13))
    stopifnot(identical(raw_value, charToRaw("A test string")))
    evaluated <- 0L
    raw_argument <- function() {
        evaluated <<- evaluated + 1L
        raw_value
    }
    invisible(tryCatch(order(rep(1, 13), raw_argument()), error=function(error) error))
    stopifnot(identical(evaluated, 1L))
    cat("order-raw-errors: PASS\n")
    "##;

fn assert_raw_order_errors(session: &mut RSession) {
    assert_eq!(
        session.eval(ORDER_RAW_SCRIPT).unwrap(),
        "order-raw-errors: PASS\n"
    );
}

#[test]
fn native_order_and_rank_reject_raw_with_gnu_errors() {
    let mut session = RSession::new().unwrap();
    assert_raw_order_errors(&mut session);
}

#[test]
fn portable_order_and_rank_reject_raw_with_gnu_errors() {
    let mut session = RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(
        Vec::new(),
        std::env::temp_dir(),
    ))
    .unwrap();
    assert_raw_order_errors(&mut session);
}

#[test]
fn integer_radix_order_preserves_ties_missing_values_and_group_ends() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"x <- c(3L, 1L, 3L, 2L, 1L, NA_integer_)
stopifnot(identical(order(x, method='radix'), c(2L,5L,4L,1L,3L,6L)))
stopifnot(identical(order(x, na.last=FALSE, method='radix'), c(6L,2L,5L,4L,1L,3L)))
stopifnot(identical(order(x, na.last=NA, method='radix'), c(2L,5L,4L,1L,3L)))
g <- grouping(x)
stopifnot(identical(attr(g, 'ends'), c(2L,3L,5L,6L)))
stopifnot(identical(attr(g, 'maxgrpn'), 2L))
stopifnot(identical(order(c(TRUE,FALSE,TRUE,FALSE), c(2L,2L,1L,1L), method='radix'), c(4L,2L,3L,1L)))
stopifnot(identical(order(c(NA_integer_,1L,NA_integer_,1L), c(3L,2L,1L,1L), na.last=NA, method='radix'), c(4L,2L)))
cat('radix-order: PASS\n')"#
            )
            .unwrap(),
        "radix-order: PASS\n"
    );
}
