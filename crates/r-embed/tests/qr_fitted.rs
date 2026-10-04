use r_embed::RSession;

#[test]
fn qr_fitted_resid_linpack_match_gnu_two_column_example() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("x <- matrix(c(1,2,3,4,5,6), 3, 2); q <- qr(x); y <- c(1,2,3); f <- qr.fitted(q, y); r <- qr.resid(q, y); c(length(f)==3L, length(r)==3L, max(abs(f-y))<1e-12, max(abs(r))<1e-12)")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE TRUE");
}

#[test]
fn qr_fitted_resid_handle_matrix_y() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("x <- matrix(c(1,2,3,4,5,6), 3, 2); q <- qr(x); ym <- x; fm <- qr.fitted(q, ym); rm <- qr.resid(q, ym); c(identical(dim(fm), c(3L,2L)), identical(dim(rm), c(3L,2L)), max(abs(fm-ym))<1e-12, max(abs(rm))<1e-12)")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE TRUE");
}

/// GNU R 4.6.1 `tests/reg-tests-1a.R` keeps names on `qr.fitted`, and an
/// overdetermined two-column design projects to 1, 2.5, 4, 5.5.
#[test]
fn qr_fitted_keeps_names_and_projects_overdetermined_y() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            r#"
            local({
              q4 <- qr(cbind(a = 1:9, b = c(1:6, 3:1), c = 2:10, d = rep(1, 9)))
              y4 <- cbind(A = 1:9, B = 2:10, C = 3:11, D = 4:12)
              dimnames(y4)[[1]] <- paste0("c.", 1:9)
              y1 <- y4[, 2]
              names_ok <- identical(names(qr.fitted(q4, y1)), paste0("c.", 1:9))
              fit_ok <- isTRUE(all.equal(y1, qr.fitted(q4, y1), tolerance = 1e-12))
              q <- qr(matrix(c(1, 1, 1, 1, 1, 2, 3, 4), 4))
              y <- c(1, 3, 3, 6)
              f <- qr.fitted(q, y)
              r <- qr.resid(q, y)
              num_ok <- isTRUE(all.equal(f, c(1, 2.5, 4, 5.5), tolerance = 1e-10)) &&
                isTRUE(all.equal(r, c(0, 0.5, -1, 0.5), tolerance = 1e-10))
              if (names_ok && fit_ok && num_ok) "ok" else
                paste(names_ok, fit_ok, num_ok, deparse1(f), deparse1(r))
            })
            "#,
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] \"ok\"");
}

#[test]
fn qr_fitted_resid_reject_row_mismatch_and_lapack() {
    let mut session = RSession::new().unwrap();
    let error = session
        .eval("qr.fitted(qr(matrix(1:6,3,2)), 1:2)")
        .unwrap_err();
    assert!(error.to_string().contains("same number of rows"), "{error}");
    let error = session
        .eval("qr.fitted(qr(matrix(1:6,3,2), LAPACK=TRUE), 1:3)")
        .unwrap_err();
    assert!(error.to_string().contains("LAPACK"), "{error}");
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
