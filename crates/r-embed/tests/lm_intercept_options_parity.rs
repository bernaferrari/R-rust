use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn model_frame_rejects_mismatched_columns_before_lm_fit() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let output = session
            .eval(
                r#"
                capture <- function(expr) {
                    tryCatch(eval(substitute(expr)), error = function(e) {
                        list(message = conditionMessage(e), call = conditionCall(e))
                    })
                }

                # .External2(C_modelframe) sees the response, then named dots
                # in their original order; report the first mismatching column.
                weights_first <- capture(
                    model.frame(y ~ 1, data = list(y = 1:3), weights = 1:2, offset = 1:4)
                )
                matrix_y <- matrix(1:6, nrow = 3L)
                short_x <- 1:2
                variable_first <- capture(model.frame(matrix_y ~ short_x))
                frame_data <- data.frame(y = 1:3)
                frame_weights <- capture(
                    model.frame(y ~ 1, data = frame_data, weights = 1:2)
                )
                lm_bad_weights <- capture(lm(c(1, 2, 3) ~ 1, weights = 1:2))
                matrix_rows <- capture(
                    model.frame(y ~ 1, data = list(y = 1:3), weights = matrix(1:4, nrow = 2L))
                )
                weight_data <- data.frame(y = c(1, 2, 4), w = c(1, 1, 8))
                provider_weighted <- capture({
                    fit <- lm(y ~ 1, data = weight_data, weights = w)
                    list(coef = coef(fit), residuals = residuals(fit))
                })
                # A callback/GC during option evaluation must retain valid inputs.
                weighted <- capture({
                    fit <- lm(c(1, 2, 4) ~ 1, weights = local({ gc(); c(1, 1, 8) }))
                    list(coef = coef(fit), residuals = residuals(fit), hat = hatvalues(fit))
                })
                dput(list(
                    weights_first = weights_first,
                    variable_first = variable_first,
                    frame_weights = frame_weights,
                    lm_bad_weights = lm_bad_weights,
                    matrix_rows = matrix_rows,
                    provider_weighted = provider_weighted,
                    weighted_gc = weighted
                ))
                "#,
            )
            .unwrap_or_else(|error| panic!("portable={portable}; {error}"));
        assert!(
            output.contains("variable lengths differ (found for '(weights)')")
                && output.contains("variable lengths differ (found for 'short_x')")
                && output.contains("model.frame.default(y ~ 1")
                && output.contains("model.frame.default(formula = c(1, 2, 3) ~ 1")
                && !output.contains(".External2(C_modelframe")
                && output.contains("provider_weighted")
                && output.contains("(Intercept)` = 3.5")
                && output.contains("weighted_gc"),
            "portable={portable}; {output}"
        );
    }
}
