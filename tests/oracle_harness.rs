#[cfg(test)]
mod tests {
    use r_test_harness::{discover_test_cases, TestOutcome, TestProfile, TestRunner};

    #[test]
    #[cfg_attr(not(feature = "oracle-tests"), ignore)]
    fn run_all_oracle_tests() {
        let tests = discover_test_cases("upstream/tests").unwrap();
        let runner = TestRunner::new();

        for test in tests {
            let res = runner.compare(&test).unwrap();
            assert!(
                matches!(res.outcome, TestOutcome::Pass),
                "Test {} failed: {:?}",
                test.name,
                res
            );
        }
    }

    #[test]
    #[ignore]
    fn generate_golden_outputs() {
        let tests = discover_test_cases("upstream/tests").unwrap();
        let runner = TestRunner::new();

        for profile in TestProfile::all() {
            let allowed: Vec<_> = tests
                .iter()
                .filter(|t| profile.is_test_allowed(&t.name))
                .collect();

            for test in allowed {
                let stock = runner
                    .run_test(test, r_test_harness::Implementation::StockR)
                    .unwrap();
                let hash = r_test_harness::compute_hash(&stock.stdout);

                let path = format!("profiles/golden/{}/{}.golden", profile.name(), test.name);
                r_test_harness::write_golden(path.as_ref(), &hash).unwrap();
            }
        }
    }
}
