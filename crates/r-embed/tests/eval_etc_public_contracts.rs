use r_embed::{RSession, RuntimePathPolicy};

fn check_both_policies(code: &str) {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session.eval(code).unwrap(),
            "[1] TRUE\n",
            "portable={portable}"
        );
    }
}

#[test]
fn ordinary_expression_printing_keeps_explicit_deparse_precision_separate() {
    check_both_policies(include_str!("fixtures/expression-print-public-contract.R"));
}

#[test]
fn primitive_formals_are_null_while_args_keeps_its_prototype() {
    check_both_policies(include_str!("fixtures/primitive-formals-public-contract.R"));
}

#[test]
fn warnings_summary_resolves_its_public_method_and_preserves_groups() {
    check_both_policies(include_str!("fixtures/warnings-summary-public-contract.R"));
}
