use r_embed::RSession;

#[test]
fn s4_cache_and_new_follow_original_class_definitions() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/s4-cache-new-contract.R"))
            .unwrap(),
        include_str!("fixtures/s4-cache-new-contract.out")
    );
}
