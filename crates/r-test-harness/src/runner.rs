use anyhow::{Context, Result};
use std::path::Path;
use std::time::Instant;

use super::{
    ComparisonResult, Implementation, OutputNormalizer, TestCase, TestOutcome, TestResult,
};

#[derive(Debug, Clone)]
pub struct TestRunner {
    stock_r_path: Option<String>,
    normalizer: OutputNormalizer,
    timeout_ms: u64,
}

impl Default for TestRunner {
    fn default() -> Self {
        Self {
            stock_r_path: None,
            normalizer: OutputNormalizer::new(),
            timeout_ms: 30000,
        }
    }
}

impl TestRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_stock_r(mut self, path: &str) -> Self {
        self.stock_r_path = Some(path.to_string());
        self
    }

    pub fn run_test(&self, test: &TestCase, implementation: Implementation) -> Result<TestResult> {
        let start = Instant::now();

        let output = match implementation {
            Implementation::StockR => self.run_stock_r(test)?,
            Implementation::RPort => self.run_rport(test)?,
        };

        let runtime_ns = start.elapsed().as_nanos() as u64;

        Ok(TestResult {
            test_case: test.clone(),
            implementation,
            exit_code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
            runtime_ns,
        })
    }

    fn run_stock_r(&self, test: &TestCase) -> Result<std::process::Output> {
        let r_path = self.stock_r_path.as_deref().unwrap_or("R");

        let output = std::process::Command::new(r_path)
            .arg("--vanilla")
            .arg("--slave")
            .arg("-f")
            .arg(&test.path)
            .output()
            .context("Failed to execute stock R")?;

        Ok(output)
    }

    fn run_rport(&self, test: &TestCase) -> Result<std::process::Output> {
        let rport_path = std::env::var("CARGO_BIN_EXE_rport")
            .unwrap_or_else(|_| "target/debug/rport".to_string());

        let output = std::process::Command::new(rport_path)
            .arg("--vanilla")
            .arg("--slave")
            .arg("-f")
            .arg(&test.path)
            .output()
            .context("Failed to execute rport")?;

        Ok(output)
    }

    pub fn compare(&self, test: &TestCase) -> Result<ComparisonResult> {
        let stock = self.run_test(test, Implementation::StockR)?;
        let rport = self.run_test(test, Implementation::RPort)?;

        let outcome = if stock.exit_code != rport.exit_code {
            TestOutcome::Mismatch
        } else if !self.normalizer.bitwise_equal(&stock.stdout, &rport.stdout) {
            TestOutcome::Mismatch
        } else if !self.normalizer.bitwise_equal(&stock.stderr, &rport.stderr) {
            TestOutcome::Mismatch
        } else {
            TestOutcome::Pass
        };

        let diff = if matches!(outcome, TestOutcome::Mismatch) {
            Some(self.generate_diff(&stock, &rport))
        } else {
            None
        };

        Ok(ComparisonResult {
            test_case: test.clone(),
            outcome,
            stock_result: stock,
            rport_result: rport,
            diff,
        })
    }

    fn generate_diff(&self, stock: &TestResult, rport: &TestResult) -> String {
        let norm_stock_buf = self.normalizer.normalize(&stock.stdout);
        let norm_rport_buf = self.normalizer.normalize(&rport.stdout);

        let norm_stock = String::from_utf8_lossy(&norm_stock_buf);
        let norm_rport = String::from_utf8_lossy(&norm_rport_buf);

        let mut diff = String::new();
        diff.push_str("=== STOCK R ===\n");
        diff.push_str(&norm_stock);
        diff.push_str("\n=== RPORT ===\n");
        diff.push_str(&norm_rport);
        diff.push('\n');

        diff
    }
}
