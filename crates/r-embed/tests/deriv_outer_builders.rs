use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn deriv_outer_builders_match_pinned_gnu_cse_shapes_values_and_recovery() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        if portable {
            session.enable_browser_files();
        }
        let output = session
            .eval(include_str!("fixtures/deriv-outer-public-contract.R"))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/deriv-outer-public-contract.out"),
            "portable={portable}"
        );
    }
}

#[test]
fn deriv_unknown_names_match_pinned_gnu_and_recover() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        if portable {
            session.enable_browser_files();
        }
        let output = session
            .eval(include_str!("fixtures/deriv-unknown-names.R"))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/deriv-unknown-names.out"),
            "portable={portable}"
        );
    }
}

#[test]
fn symbolic_powers_match_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        if portable {
            session.enable_browser_files();
        }
        let output = session
            .eval(include_str!("fixtures/deriv-symbolic-powers.R"))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/deriv-symbolic-powers.out"),
            "portable={portable}"
        );
    }
}

#[test]
fn derivative_table_matches_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        if portable {
            session.enable_browser_files();
        }
        let output = session
            .eval(include_str!("fixtures/deriv-table-audit.R"))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/deriv-table-audit.out"),
            "portable={portable}"
        );
    }
}

#[test]
fn mixed_unary_derivatives_match_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        if portable {
            session.enable_browser_files();
        }
        let output = session
            .eval(include_str!("fixtures/deriv-mixed-unary.R"))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/deriv-mixed-unary.out"),
            "portable={portable}"
        );
    }
}
