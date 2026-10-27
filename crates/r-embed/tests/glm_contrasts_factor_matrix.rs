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
                multi <- factor(c("a", "b", "a"))
                stopifnot(ncol(model.matrix(~ multi)) == 2L)
                for (single in list(factor(c("a", "a")),
                                    factor(c(NA_character_, NA_character_)),
                                    factor(character()))) {
                    message <- tryCatch(model.matrix(~ single),
                                        error = function(e) conditionMessage(e))
                    stopifnot(identical(message,
                        "contrasts can be applied only to factors with 2 or more levels"))
                }

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
            expected <- c(0.568591610355818, 0.431314202154426,
                1.46186176130743, 1.78873105620529, 0.4410113224708,
                2.49173500586256, 1.12753590591426, 0.796445456860856,
                0.873202402077571, 1.81399182333093, 2.08251512288905,
                1.36638308969248, 2.81636896807576, 1.90391819141279,
                1.39858523552884, 1.66549505893183, 0.959681272107498,
                0.0217496866307568, 0.226398977645518, 0.808646203652358,
                0.13227671418244, 0.506708955158415, 0.221880455226052,
                0.13697439478698)
            stopifnot(isTRUE(all.equal(unname(coef(fit)), expected,
                                       tolerance = 1e-6)))
            "#,
            )
            .unwrap_or_else(|error| panic!("glm contrast fit; portable={portable}; {error}"));
    }
}
