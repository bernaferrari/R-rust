use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn table_public_printing_preserves_chronology_shape_and_options() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!("fixtures/table-print-public-contract.R"))
                .unwrap(),
            include_str!("fixtures/table-print-public-contract.out"),
            "portable={portable}"
        );
        assert_eq!(
            session
                .eval(include_str!(
                    "../../../tests/upstream-core/cases/008_data_frame_factor_table.R"
                ))
                .unwrap(),
            include_str!("fixtures/table-curated-008.out"),
            "original curated008 portable={portable}"
        );
    }
}
