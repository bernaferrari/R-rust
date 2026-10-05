use r_embed::RSession;

const ORACLE: &str = include_str!("fixtures/array_attributes_gnu.R");

#[test]
fn tsp_assignment_matches_pinned_gnu_attribute_contract() {
    let mut session = RSession::new().expect("session");
    session.eval(ORACLE).expect("oracle definitions");
    assert_eq!(
        session
            .eval("verify_tsp(); cat('tsp-ok')")
            .expect("tsp contract"),
        "tsp-ok"
    );
}

#[test]
fn singleton_array_drop_keeps_only_unambiguous_names() {
    let mut session = RSession::new().expect("session");
    session.eval(ORACLE).expect("oracle definitions");
    assert_eq!(
        session
            .eval("verify_drop(); cat('drop-ok')")
            .expect("drop contract"),
        "drop-ok"
    );
}
