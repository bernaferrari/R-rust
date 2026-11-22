use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn sample_conditions_preserve_calls_and_rng_state_under_both_policies() {
    for portable in [false, true] {
        let mut s = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            s.eval(include_str!("fixtures/sample-condition-contract.R"))
                .unwrap(),
            include_str!("fixtures/sample-condition-contract.out"),
            "portable={portable}"
        );
        s.close();
    }
}

#[test]
fn weighted_sampling_with_ties_preserves_gnu_seeded_sequence() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session.eval(r#"set.seed(1943)
prob <- c(2, 2, 1, 1, 1)
a <- sample.int(5L, 10L, replace=TRUE, prob=prob)
b <- sample.int(5L, 5L, replace=FALSE, prob=prob)
stopifnot(identical(a, c(4L,2L,2L,2L,1L,5L,2L,1L,3L,1L)))
stopifnot(identical(b, c(2L,3L,4L,1L,5L)))
cat('weighted-ties: PASS\n')"#).unwrap(),
        "weighted-ties: PASS\n"
    );
}
