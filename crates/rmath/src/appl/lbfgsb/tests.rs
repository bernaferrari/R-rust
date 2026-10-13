use super::*;
use crate::sexp::instance::RInstance;
#[test]
fn lbfgsb_state_is_owned_by_instance() {
    let mut first = RInstance::new();
    let second = RInstance::new();
    first.lbfgsb_state.iter = 42;
    first.lbfgsb_state.theta = 7.5;
    assert_eq!(first.lbfgsb_state.iter, 42);
    assert_eq!(first.lbfgsb_state.theta, 7.5);
    assert_eq!(second.lbfgsb_state.iter, 0);
    assert_eq!(second.lbfgsb_state.theta, 1.0);
}
