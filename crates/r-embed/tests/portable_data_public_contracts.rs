use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn original_utils_data_preserves_topics_overwrite_conditions_and_namespace_identity() {
    let code = include_str!("fixtures/portable-data-public-contract.R");
    for portable in [false, true] {
        // Recreate the public session after the previous one is dropped so a
        // namespace loaded by an earlier evaluation cannot supply its bindings.
        for generation in 0..2 {
            let mut session = if portable {
                RSession::new_with_path_policy(RuntimePathPolicy::new(
                    Vec::new(),
                    std::env::temp_dir(),
                ))
            } else {
                RSession::new()
            }
            .unwrap();
            assert_eq!(
                session.eval(code).unwrap(),
                "[1] TRUE\n",
                "portable={portable}, generation={generation}"
            );
        }
    }
}
