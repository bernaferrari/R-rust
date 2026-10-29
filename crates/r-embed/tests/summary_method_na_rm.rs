use r_embed::{RResourceLimits, RSession, RuntimePathPolicy};

const SCRIPT: &str = include_str!("fixtures/summary-method-na-rm.R");
const EXPECTED: &str = include_str!("fixtures/summary-method-na-rm.out");

#[test]
fn summary_methods_receive_na_rm_default_and_keep_argument_order() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session.eval(SCRIPT).unwrap();
        assert_eq!(actual, EXPECTED, "portable={portable}");
    }
}

#[test]
fn summary_argument_copy_budget_denial_recovers() {
    let mut session = RSession::new().unwrap();
    session
        .eval(
            "Summary.summary_probe <- function(..., na.rm=FALSE) na.rm; x <- structure(1:3, class='summary_probe')",
        )
        .unwrap();

    let limits = RResourceLimits {
        max_arena_nodes: 1,
        ..RResourceLimits::default()
    };
    session.set_resource_limits(limits).unwrap();
    let denied = session.eval("sum(x)").unwrap_err();
    assert!(denied.to_string().to_lowercase().contains("alloc"));

    let baseline = session.eval("list(1L)").unwrap_err();
    assert!(baseline.to_string().to_lowercase().contains("alloc"));

    session
        .set_resource_limits(RResourceLimits::default())
        .unwrap();
    assert!(session.eval("sum(x)").unwrap().contains("FALSE"));
}
