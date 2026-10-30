use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn factor_names_replacement_matches_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/names-factor-evidence/controls.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/names-factor-evidence/controls.out"),
            "portable={portable}"
        );
    }
}

#[test]
fn classed_names_dispatch_character_method_across_collection() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let output = session
            .eval(
                r#"
            as.character.demo <- function(x, ...) { gc(); c('é', '日本') }
            for (v in list(structure(1:2, class='demo'),
                           structure(c('x','y'), class='demo'))) {
                x <- 1:2
                names(x) <- v
                cat(paste(names(x), collapse='|'), '\n')
            }
        "#,
            )
            .unwrap();
        assert_eq!(output, "é|日本 \né|日本 \n", "portable={portable}");
    }
}
