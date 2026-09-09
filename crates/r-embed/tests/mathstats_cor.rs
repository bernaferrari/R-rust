use r_embed::RSession;

#[test]
fn cor_matrix_matches_r_shape_values_and_names() {
    let mut session = RSession::new().expect("session");
    session
        .eval(
            r#"
            x <- cbind(sun=c(1,2,4,8), water=c(2,4,1,3),
                       growth=c(4,1,3,2), stress=c(5,4,2,1))
            z <- cor(x)
            if (!identical(dim(z), c(4L, 4L))) stop("dim")
            if (!identical(dimnames(z), list(colnames(x), colnames(x)))) stop("names")
            expected <- c(1, 0.0417028828114149, -0.291920179679905, -0.943628519391342,
              0.0417028828114149, 1, -0.8, 0.14142135623731, -0.291920179679905,
              -0.8, 1, 0.282842712474619, -0.943628519391342, 0.14142135623731,
              0.282842712474619, 1)
            if (max(abs(as.vector(z) - expected)) > 1e-12) stop("pinned GNU R values")
            z
            "#,
        )
        .expect("matrix cor");
}

#[test]
fn cor_matrix_vector_keeps_r_matrix_shape_and_paired_vector_path() {
    let mut session = RSession::new().expect("session");
    session
        .eval(
            r#"
            x <- cbind(a=c(1,2,4,8), b=c(2,4,1,3))
            y <- c(3,1,4,2)
            z <- cor(x, y)
            if (!identical(dim(z), c(2L, 1L))) stop("dim")
            if (!identical(dimnames(z), list(colnames(x), NULL))) stop("names")
            if (!(abs(z[1,1] + 0.04170288281141494) < 1e-6)) stop("a")
            if (!(abs(z[2,1] + 1) < 1e-6)) stop("b")
            stopifnot(abs(cor(c(1,2,4), c(2,4,8)) - 1) < 1e-12)
            "#,
        )
        .expect("matrix/vector cor");
}

#[test]
fn cor_matrix_rejects_nonfinite_until_use_modes_are_supported() {
    let mut session = RSession::new().expect("session");
    assert!(session.eval("cor(matrix(c(1, NA, 3, 2), nrow=2))").is_err());
}

#[test]
fn cor_does_not_silently_ignore_requested_method() {
    let mut session = RSession::new().unwrap();
    assert!(
        session
            .eval("x <- matrix(1:8, nrow=4); cor(x, x, method='spearman')")
            .is_err()
    );
    session
        .eval("cor(x, x, method='pearson')")
        .expect("recovery");
}

#[test]
fn cor_vector_rejects_recycling_and_propagates_missing_by_default() {
    let mut session = RSession::new().unwrap();
    let unequal = session.eval("cor(c(1,2,3), c(1,2))");
    assert!(unequal.is_err(), "unequal cor result: {unequal:?}");
    assert_eq!(
        session.eval("is.na(cor(c(1,NA,3), c(1,2,3)))").unwrap(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("cor(c(1,NA,3), c(1,2,3), use='complete.obs')")
            .unwrap(),
        "[1] 1"
    );
    assert_eq!(
        session
            .eval("is.na(cor(c(NA,NA), c(1,2), use='na.or.complete'))")
            .unwrap(),
        "[1] TRUE"
    );
    assert!(
        session
            .eval("cor(c(NA,NA), c(1,2), use='complete.obs')")
            .is_err()
    );
    assert!(session.eval("cor(c(NA,2), c(1,2), use='all.obs')").is_err());
    assert_eq!(
        session.eval("is.na(cor(c(1,NaN,3), c(1,2,3)))").unwrap(),
        "[1] TRUE"
    );
    assert_eq!(
        session.eval("is.nan(cor(c(1,Inf,3), c(1,2,3)))").unwrap(),
        "[1] TRUE"
    );
}

#[test]
fn cor_complete_pairs_distinguish_zero_one_and_empty() {
    let mut s = RSession::new().unwrap();
    assert_eq!(
        s.eval("is.na(cor(c(1,NA), c(1,NA), use='complete.obs'))")
            .unwrap(),
        "[1] TRUE"
    );
    assert!(
        s.eval("cor(numeric(), numeric(), use='complete.obs')")
            .is_err()
    );
    assert!(
        s.eval("cor(numeric(), numeric(), use='pairwise.complete.obs')")
            .is_err()
    );
    assert_eq!(
        s.eval("is.na(cor(numeric(), numeric(), use='na.or.complete'))")
            .unwrap(),
        "[1] TRUE"
    );
}
