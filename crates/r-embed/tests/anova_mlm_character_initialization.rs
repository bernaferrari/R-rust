use r_embed::{RSession, RuntimePathPolicy};

const ANOVA_MLM_CHARACTER_CONTROL: &str = r#"
empty <- character(2)
zero <- character(0)
invisible(gc())
stopifnot(identical(empty, c("", "")), !anyNA(empty), identical(zero, character()))
dat <- matrix(c(9,7,8,8,12,11,8,13, 6,5,6,3,6,7,10,9,
                10,13,8,13,12,14,14,16, 9,11,13,14,16,12,15,14),
              ncol=4, dimnames=list(s=1:8, c=1:4))
mlmfit <- lm(dat ~ 1)
tab <- anova(mlmfit, X=~1)
stopifnot(is.data.frame(tab), nrow(tab) == 2L,
          identical(rownames(tab), c("(Intercept)", "Residuals")),
          "Pillai" %in% names(tab), abs(tab$Pillai[1] - 0.90126) < 1e-5)
"anova-mlm-character-initialization-ok"
"#;

#[test]
fn anova_mlm_uses_initialized_character_vector_slots_in_both_policies() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap_or_else(|error| panic!("portable={portable}; {error}"));
        let output = session
            .eval(ANOVA_MLM_CHARACTER_CONTROL)
            .unwrap_or_else(|error| panic!("portable={portable}; {error}"));
        assert!(
            output.contains("anova-mlm-character-initialization-ok"),
            "portable={portable}; output={output}"
        );
    }
}
