use r_embed::RSession;

const TSP_ASSIGNMENT_SCRIPT: &str = r##"
    x <- ts(rnorm(6), frequency=7)
    before <- x
    before_tsp <- tsp(x)
    failure <- tryCatch({ x[8] <- NA; NULL }, error=function(e) conditionMessage(e))
    stopifnot(identical(failure, "only replacement of elements is allowed"))
    stopifnot(identical(x, before), identical(class(x), "ts"))
    stopifnot(identical(tsp(x), before_tsp))

    grown <- 1:3
    grown[5] <- 9
    stopifnot(identical(as.numeric(grown), c(1, 2, 3, NA, 9)))
    raw_tsp <- 1:3
    attr(raw_tsp, "tsp") <- c(1, 3, 1)
    raw_tsp[5] <- 9
    stopifnot(identical(attr(raw_tsp, "tsp"), c(1, 3, 1)))
    stopifnot(identical(as.numeric(raw_tsp), c(1, 2, 3, NA, 9)))
    for (pair in list(list(1:3, 9), list(c(TRUE, FALSE, TRUE), 9L),
                      list(c(1, 2, 3), 9+0i), list(1:3, "nine"))) {
        source <- pair[[1]]
        attr(source, "tsp") <- c(1, 3, 1)
        attr(source, "custom") <- list(label="retained")
        names(source) <- c("a", "b", "c")
        changed <- source
        changed[5] <- pair[[2]]
        stopifnot(identical(attr(changed, "tsp"), c(1, 3, 1)))
        stopifnot(identical(attr(changed, "custom"), attr(source, "custom")))
        stopifnot(identical(names(changed), c("a", "b", "c", "", "")))
        stopifnot(length(source) == 3L, identical(attr(source, "tsp"), c(1, 3, 1)))
        changed[5] <- pair[[2]]
        stopifnot(identical(attr(changed, "tsp"), c(1, 3, 1)))
        attr(changed, "custom")$label <- "result"
        stopifnot(identical(attr(source, "custom")$label, "retained"))
        attr(source, "custom")$label <- "source"
        stopifnot(identical(attr(changed, "custom")$label, "result"))
        attr(changed, "tsp") <- NULL
        stopifnot(identical(attr(source, "tsp"), c(1, 3, 1)))
    }
    cat("tsp-assignment-parity: PASS\n")
    "##;

fn assert_tsp_assignment_parity(session: &mut RSession) {
    assert_eq!(
        session.eval(TSP_ASSIGNMENT_SCRIPT).unwrap(),
        "tsp-assignment-parity: PASS\n"
    );
}

#[test]
fn native_tsp_assignment_preserves_gnu_error_order() {
    let mut session = RSession::new().unwrap();
    assert_tsp_assignment_parity(&mut session);
}

#[test]
fn portable_tsp_assignment_preserves_gnu_error_order() {
    let mut session = RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(
        Vec::new(),
        std::env::temp_dir(),
    ))
    .unwrap();
    assert_tsp_assignment_parity(&mut session);
}
