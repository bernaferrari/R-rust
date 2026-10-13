use super::*;
#[test]
fn test_l_pretty_returns_axis_ticks() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let scale = Rf_allocVector(SEXPTYPE::REALSXP, 2);
        *REAL(scale) = 0.0;
        *REAL(scale).add(1) = 10.0;
        let ticks = L_pretty(scale);
        assert_eq!(TYPEOF(ticks), SEXPTYPE::REALSXP);
        assert!(LENGTH(ticks) >= 2);
        assert!(*REAL(ticks) <= *REAL(ticks).add((LENGTH(ticks) - 1) as usize));
    }
}
