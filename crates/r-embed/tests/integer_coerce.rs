use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn integer_coercion_matches_gnu_public_values_and_attribute_policy() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/integer-coerce/contract.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/integer-coerce/contract.out"),
            "portable={portable}"
        );
    }
}

#[test]
fn integer_coercion_public_budget_denial_is_typed_and_recovers_under_same_cap() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        session
            .eval("x<-as.numeric(seq_len(1000000L));invisible(as.integer(1L));gc();invisible(NULL)")
            .unwrap();
        let before = session.memory_usage().unwrap();
        let mut limits = session.resource_limits();
        limits.max_alloc_bytes = before.total_bytes + 65536;
        session.set_resource_limits(limits).unwrap();
        let error = session.eval("invisible(as.integer(x))").unwrap_err();
        assert!(
            matches!(
                error,
                r_embed::RSessionError::EvalError(_)
                    | r_embed::RSessionError::EvalErrorWithOutput { .. }
            ),
            "{error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("R allocation failed: memory or node budget exceeded"),
            "portable={portable}: {error}"
        );
        assert_eq!(
            session.resource_limits().max_alloc_bytes,
            limits.max_alloc_bytes
        );
        assert_eq!(session.eval("length(x)").unwrap(), "[1] 1000000\n");
        assert_eq!(session.eval("as.integer(x[1L])").unwrap(), "[1] 1\n");
        // A denial-triggered full GC can release pre-existing bootstrap
        // workspace; it must not increase the original workspace footprint.
        assert!(session.memory_usage().unwrap().workspace_bytes <= before.workspace_bytes);
    }
}
