use r_embed::RSession;

#[test]
fn raw_scalar_errors_and_empty_vector_neighbors_match_pinned_gnu() {
    let mut session = RSession::new().expect("full original runtime");
    let output = session
        .eval(include_str!("fixtures/pairlist-raw-scalar/contract.R"))
        .expect("GNU raw scalar admission and error contract");
    assert!(
        output.contains("double raw-child-error-and-neighbors=PASS"),
        "{output}"
    );
    assert!(
        output.contains("complex raw-child-error-and-neighbors=PASS"),
        "{output}"
    );
}
