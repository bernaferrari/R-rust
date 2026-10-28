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
