use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn scale_matches_gnu_argument_matching_shape_and_metadata() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .expect("session");

        session
            .eval(
                r#"
                x <- setNames(c(1, 2, 3), letters[1:3])
                x_before <- x
                by_name <- scale(x, scale = FALSE)
                by_partial <- scale(x, sc = FALSE)
                stopifnot(is.matrix(by_name), identical(dim(by_name), c(3L, 1L)))
                stopifnot(identical(dimnames(by_name), list(names(x), NULL)))
                stopifnot(identical(as.numeric(by_name), c(-1, 0, 1)))
                stopifnot(identical(attr(by_name, "scaled:center"), 2))
                stopifnot(is.null(attr(by_name, "scaled:scale")))
                stopifnot(identical(by_partial, by_name))

                no_transform <- scale(x, cen = FALSE, sc = FALSE)
                reordered <- scale(x, scale = FALSE, center = FALSE)
                positional <- scale(x, FALSE, FALSE)
                stopifnot(identical(as.numeric(no_transform), c(1, 2, 3)))
                stopifnot(identical(no_transform, reordered))
                stopifnot(identical(no_transform, positional))
                stopifnot(is.null(attr(no_transform, "scaled:center")))
                stopifnot(is.null(attr(no_transform, "scaled:scale")))
                stopifnot(identical(x, x_before))

                m <- matrix(c(1, 4, 3, 6), ncol = 2,
                            dimnames = list(c("r1", "r2"), c("u", "v")))
                m_before <- m
                scaled_m <- scale(m, scale = FALSE)
                stopifnot(identical(dim(scaled_m), c(2L, 2L)))
                stopifnot(identical(dimnames(scaled_m), dimnames(m)))
                stopifnot(identical(as.numeric(scaled_m), c(-1.5, 1.5, -1.5, 1.5)))
                stopifnot(identical(attr(scaled_m, "scaled:center"), c(u = 2.5, v = 4.5)))
                stopifnot(identical(m, m_before))

                explicit <- scale(m, center = c(1, 4), scale = c(2, 3))
                stopifnot(identical(as.numeric(explicit), c(0, 1.5, -1 / 3, 2 / 3)))
                stopifnot(identical(attr(explicit, "scaled:center"), c(u = 1, v = 4)))
                stopifnot(identical(attr(explicit, "scaled:scale"), c(u = 2, v = 3)))

                rv <- scale(as.numeric(row(diag(1:8))), scale = FALSE)
                cv <- scale(as.numeric(col(diag(1:8))), scale = FALSE)
                stopifnot(identical(dim(rv), c(64L, 1L)))
                stopifnot(identical(dim(cv), c(64L, 1L)))
                interaction <- model.matrix(~ rv:cv)[, 2]
                expected <- (as.numeric(row(diag(1:8))) - 4.5) *
                            (as.numeric(col(diag(1:8))) - 4.5)
                stopifnot(isTRUE(all.equal(as.numeric(interaction), expected)))
                "scale-gnu-controls-ok"
                "#,
            )
            .unwrap_or_else(|error| panic!("scale parity; portable={portable}; {error}"));
    }
}
