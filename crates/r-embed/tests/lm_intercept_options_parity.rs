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
                matrix_rows <- capture(
                    model.frame(y ~ 1, data = list(y = 1:3), weights = matrix(1:4, nrow = 2L))
                )
                # A callback/GC during option evaluation must retain valid inputs.
                weighted <- capture({
                    fit <- lm(c(1, 2, 4) ~ 1, weights = local({ gc(); c(1, 1, 8) }))
                    list(coef = coef(fit), residuals = residuals(fit), hat = hatvalues(fit))
                })
                dput(list(
                    weights_first = weights_first,
                    variable_first = variable_first,
                    matrix_rows = matrix_rows,
                    weighted_gc = weighted
                ))
                "#,
            )
            .unwrap_or_else(|error| panic!("portable={portable}; {error}"));

        assert!(
            output.contains("variable lengths differ (found for '(weights)')")
                && output.contains("variable lengths differ (found for 'short_x')")
                && output.contains("weighted_gc"),
            "portable={portable}; {output}"
        );
    }
}
