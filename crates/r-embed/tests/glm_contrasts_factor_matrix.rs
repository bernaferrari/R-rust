use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn integer_matrix_as_factor_uses_distinct_values() {
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
            x <- diag(1:8)
            f <- as.factor(x)
            stopifnot(length(f) == 64L)
            stopifnot(identical(levels(f), as.character(0:8)))
            stopifnot(identical(as.integer(f), match(as.vector(x), 0:8)))
            stopifnot(identical(unique.default(x), unique(as.vector(x))))
            stopifnot(is.matrix(unique(x)))
            stopifnot(nrow(unique(x)) == 8L)
            stopifnot(length(levels(f)) == 9L)
            "#,
            )
            .unwrap_or_else(|error| panic!("factor matrix controls; portable={portable}; {error}"));

        session
            .eval(
                r#"
            data(occupationalStatus)
            Diag <- as.factor(diag(1:8))
            Rscore <- scale(as.numeric(row(occupationalStatus)), scale = FALSE)
            Cscore <- scale(as.numeric(col(occupationalStatus)), scale = FALSE)
            fit <- glm(Freq ~ origin + destination + Diag + Rscore:Cscore,
                       family = poisson, data = occupationalStatus)
            stopifnot(length(levels(Diag)) == 9L)
            stopifnot(ncol(model.matrix(fit)) == 24L)
            stopifnot(length(coef(fit)) == 24L)
            stopifnot(identical(colnames(model.matrix(fit)), c(
                "(Intercept)", paste0("origin", 2:8),
                paste0("destination", 2:8), paste0("Diag", 1:8),
                "Rscore:Cscore")))
            stopifnot(length(coef(fit)) == 24L)
            "#,
            )
            .unwrap_or_else(|error| panic!("glm contrast fit; portable={portable}; {error}"));
    }
}
