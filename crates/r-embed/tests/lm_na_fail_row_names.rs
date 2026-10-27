use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn na_fail_model_frame_error_keeps_gnu_compact_row_names() {
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
                error <- tryCatch(
                    lm(c(1, NA, 3) ~ 1, na.action = na.fail),
                    error = identity
                )
                automatic <- model.frame(c(1, NA, 3) ~ 1, na.action = na.pass)
                explicit <- model.frame(c(a = 1, b = NA, c = 3) ~ 1, na.action = na.pass)
                empty <- model.frame(numeric(0) ~ 1, na.action = na.pass)
                omitted <- model.frame(c(a = 1, b = NA, c = 3) ~ 1, na.action = na.omit)
                excluded <- model.frame(c(a = 1, b = NA, c = 3) ~ 1, na.action = na.exclude)
                stopifnot(
                    identical(row.names(automatic), c("1", "2", "3")),
                    identical(.row_names_info(automatic, 0L), c(NA_integer_, 3L)),
                    identical(row.names(explicit), c("a", "b", "c")),
                    identical(.row_names_info(explicit, 0L), c("a", "b", "c")),
                    identical(row.names(empty), character()),
                    identical(.row_names_info(empty, 0L), integer()),
                    identical(row.names(omitted), c("a", "c")),
                    identical(.row_names_info(omitted, 0L), c("a", "c")),
                    identical(row.names(excluded), c("a", "c")),
                    identical(.row_names_info(excluded, 0L), c("a", "c"))
                )
                dput(list(
                    controls = TRUE,
                    class = class(error)[[1L]],
                    message = conditionMessage(error),
                    call = conditionCall(error)
                ))
                "#,
            )
            .unwrap_or_else(|error| panic!("portable={portable}; {error}"));

        assert!(
            output.contains("missing values in object")
                && output.contains("na.fail.default(structure(list(`c(1, NA, 3)`")
                && output.contains("row.names = c(NA, 3L)")
                && !output.contains("row.names = c(NA, -3L)"),
            "portable={portable}; {output}"
        );
    }
}
