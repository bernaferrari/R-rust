use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn names_replacement_drops_value_metadata_like_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!(
                "fixtures/names-replacement-metadata/controls.R"
            ))
            .unwrap();
        let expected = include_str!("fixtures/names-replacement-metadata/controls.out");
        for (index, (actual, expected)) in actual.lines().zip(expected.lines()).enumerate() {
            assert_eq!(actual, expected, "portable={portable}, row={index}");
        }
        assert_eq!(actual.lines().count(), expected.lines().count());
    }
}
