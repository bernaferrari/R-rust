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

#[test]
fn s4_class_names_and_inheritance_follow_original_methods_metadata() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/s4-class-name-contract.R"))
            .unwrap(),
        include_str!("fixtures/s4-class-name-contract.out")
    );
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
