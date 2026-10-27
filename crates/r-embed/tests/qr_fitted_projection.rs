use r_embed::RSession;

#[test]
fn qr_fitted_projects_vector_and_matrix_rhs_without_mutating_input() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"
                X4 <- cbind(a = 1:9, b = c(1:6, 3:1), c = 2:10, d = rep(1, 9))
                decomposition <- qr(X4)
                y <- 2:10
                y_before <- y
                fitted <- qr.fitted(decomposition, y)
                stopifnot(
                    isTRUE(all.equal(y, fitted, tol = 1e-12)),
                    identical(y, y_before),
                    identical(names(fitted), names(y))
                )
                residual <- qr.resid(decomposition, y)
                coefficients <- qr.coef(decomposition, y)
                stopifnot(
                    isTRUE(all.equal(rep(0, length(y)), residual, tol = 1e-12)),
                    isTRUE(all.equal(unname(coefficients)[1:3], c(0, 0, 1), tol = 1e-12)),
                    is.na(coefficients[[4L]]),
                    isTRUE(all.equal(y, qr.fitted(decomposition, y, k = 0L), tol = 1e-12))
                )

                qr_data <- as.double(decomposition$qr)
                qr_rows <- as.integer(nrow(decomposition$qr))
                qr_rank <- as.integer(decomposition$rank)
                qr_aux <- as.double(decomposition$qraux)
                qr_ny <- 1L
                aliased_input <- as.double(y)
                aliased_fit <- .Fortran(
                    .F_dqrxb, qr_data, qr_rows, qr_rank, qr_aux,
                    aliased_input, qr_ny, xb = aliased_input
                )$xb
                separate_fit <- .Fortran(
                    .F_dqrxb, qr_data, qr_rows, qr_rank, qr_aux,
                    as.double(y), qr_ny, xb = double(length(y))
                )$xb
                stopifnot(
                    isTRUE(all.equal(y, aliased_fit, tol = 1e-12)),
                    isTRUE(all.equal(separate_fit, aliased_fit, tol = 1e-12))
                )

                response <- cbind(A = 1:9, B = 2:10, C = 3:11, D = 4:12)
                response_before <- response
                fitted_matrix <- qr.fitted(decomposition, response)
                stopifnot(
                    isTRUE(all.equal(response, fitted_matrix, tol = 1e-12)),
                    identical(response, response_before),
                    identical(dimnames(fitted_matrix), dimnames(response))
                )

                full_rank <- qr(diag(9L))
                full_rank_y <- seq_len(9L)
                stopifnot(
                    full_rank$rank == nrow(full_rank$qr),
                    isTRUE(all.equal(full_rank_y, qr.fitted(full_rank, full_rank_y), tol = 1e-12))
                )

                unnamed <- response
                dimnames(unnamed) <- list(paste0("r", 1:9), NULL)
                stopifnot(isTRUE(all.equal(unnamed, qr.fitted(decomposition, unnamed), tol = 1e-12)))
                cat("qr-fitted-projection: PASS\n")
                "#
            )
            .unwrap(),
        "qr-fitted-projection: PASS\n"
    );
}
