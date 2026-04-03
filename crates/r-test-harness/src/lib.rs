use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

pub mod normalize;
pub mod profiles;
pub mod runner;

pub use normalize::OutputNormalizer;
pub use profiles::TestProfile;
pub use runner::TestRunner;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Implementation {
    StockR,
    RPort,
}

impl fmt::Display for Implementation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Implementation::StockR => write!(f, "stock-r"),
            Implementation::RPort => write!(f, "rport"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestCase {
    pub path: PathBuf,
    pub name: String,
    pub category: String,
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub test_case: TestCase,
    pub implementation: Implementation,
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub runtime_ns: u64,
}

#[derive(Debug, Clone)]
pub enum TestOutcome {
    Pass,
    Fail,
    Mismatch,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct ComparisonResult {
    pub test_case: TestCase,
    pub outcome: TestOutcome,
    pub stock_result: TestResult,
    pub rport_result: TestResult,
    pub diff: Option<String>,
}

pub fn discover_test_cases(root: &Path) -> Result<Vec<TestCase>> {
    let mut cases = Vec::new();

    for entry in WalkDir::new(root) {
        let entry = entry?;
        if entry.file_type().is_file()
            && entry.path().extension() == Some(OsStr::new("R"))
        {
            let rel_path = entry.path().strip_prefix(root)?;
            let name = rel_path.with_extension("").to_string_lossy().into_owned();
            let category = rel_path.parent()
                .unwrap_or_else(|| Path::new(""))
                .to_string_lossy()
                .into_owned();

            cases.push(TestCase {
                path: entry.path().to_owned(),
                name,
                category,
            });
        }
    }

    Ok(cases)
}

pub fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn read_golden(path: &Path) -> Result<Option<String>> {
    if path.exists() {
        Ok(Some(fs::read_to_string(path)?))
    } else {
        Ok(None)
    }
}

pub fn write_golden(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}
