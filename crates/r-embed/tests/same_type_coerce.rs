use r_embed::{RSession, RuntimePathPolicy};
#[test]
fn same_type_atomic_helpers_match_gnu_values_metadata_and_isolation() {
    for portable in [false, true] {
        let mut s = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            s.eval(include_str!("fixtures/same-type-coerce/contract.R"))
                .unwrap(),
            include_str!("fixtures/same-type-coerce/contract.out"),
            "portable={portable}"
        );
    }
}
