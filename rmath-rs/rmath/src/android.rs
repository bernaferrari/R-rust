//! Android embedding layer for the rmath-rs R interpreter.
//!
//! This module provides a clean, safe API surface for embedding the R
//! interpreter in Android applications via UniFFI (or any FFI framework).
//!
//! # Design Principles
//!
//! - **No raw pointers cross the FFI boundary.** All inputs/outputs are
//!   owned Rust types (strings, numbers, Vec) or opaque handles.
//! - **Thread-confined sessions.** Each `RSession` owns its own arena and
//!   should be created on the worker thread that uses it; hosts can run
//!   multiple sessions in parallel by giving each tab its own worker.
//! - **Minimal surface.** Only the operations needed for Android embedding
//!   are exposed — eval, print, math, ALTREP.
//! - **Zero-cost.** The safe wrappers compile down to the same operations
//!   as the internal code.

use crate::sexp::RSession as CoreRSession;
use crate::sexp::builder;
#[cfg(test)]
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::memory::ArenaBudget;
use crate::sexp::object::{Sexp, SexpAttribute, SexpComplex, SexpMetadata, SexpValue};
use crate::sexp::output;
use crate::sexp::session::CancellationToken;

// ---------------------------------------------------------------------------
// RSession — per-thread interpreter context
// ---------------------------------------------------------------------------

/// An isolated R interpreter session.
///
/// Each session has its own memory arena and evaluation environment.
/// This is the primary entry point for Android embedding.
///
/// # Thread Safety
///
/// `RSession` is thread-confined. Each worker thread should create and keep its
/// own session; multiple sessions can run in parallel on different workers.
pub struct RSession {
    core: CoreRSession,
}

fn result_from_sexp(sexp: Sexp<'_>) -> RResult {
    let typed = RValue::from_sexp(sexp);
    RResult {
        value: typed.numeric_scalar_value(),
        typed,
        output: output::format_sexp_direct(sexp),
    }
}

fn result_from_eval(sexp: Sexp<'_>, captured: output::RCapturedOutput, visible: bool) -> RResult {
    let mut display = String::new();
    display.push_str(&captured.stdout);
    display.push_str(&captured.stderr);
    if visible {
        if !display.is_empty() && !display.ends_with('\n') {
            display.push('\n');
        }
        display.push_str(&output::format_sexp_direct(sexp));
    }
    let typed = RValue::from_sexp(sexp);
    RResult {
        value: typed.numeric_scalar_value(),
        typed,
        output: display,
    }
}

fn error_result(message: impl Into<String>) -> RResult {
    RResult {
        value: 0.0,
        typed: RValue::Error(message.into()),
        output: String::new(),
    }
    .with_error_output()
}

fn is_valid_package_name(package: &str) -> bool {
    let mut chars = package.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && package
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '.')
}

impl RSession {
    pub fn new() -> Self {
        RSession {
            core: CoreRSession::new(),
        }
    }

    pub fn close(&mut self) {
        self.core.close();
    }

    pub fn is_active(&self) -> bool {
        self.core.is_active()
    }

    pub fn runtime_info(&self) -> RRuntimeInfo {
        RRuntimeInfo {
            is_active: self.is_active(),
            library_paths: self
                .core
                .library_paths()
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            temp_dir: self.core.temp_dir().to_string_lossy().into_owned(),
        }
    }

    pub fn resource_limits(&self) -> RResourceLimits {
        let eval = self.core.eval_limits();
        let arena = self.core.arena_budget();
        RResourceLimits {
            max_eval_depth: eval.max_eval_depth as u64,
            max_execution_time_ms: eval.max_execution_time_ms,
            max_alloc_bytes: arena.max_bytes as u64,
            max_arena_nodes: arena.max_nodes as u64,
        }
    }

    pub fn arena_stats(&mut self) -> RArenaStats {
        self.core
            .with_arena(|arena| RArenaStats {
                active_nodes: arena.node_count() as u64,
                free_nodes: arena.free_count() as u64,
                retained_bytes: arena.total_bytes_allocated() as u64,
                fragmentation_ratio: arena.fragmentation_ratio(),
            })
            .unwrap_or_default()
    }

    pub fn set_resource_limits(&mut self, limits: RResourceLimits) {
        self.core.set_eval_limits(crate::eval::eval::EvalLimits {
            max_eval_depth: saturating_usize(limits.max_eval_depth),
            max_execution_time_ms: limits.max_execution_time_ms,
            max_alloc_bytes: saturating_usize(limits.max_alloc_bytes),
        });
        self.core.set_arena_budget(ArenaBudget::new(
            saturating_usize(limits.max_alloc_bytes),
            saturating_usize(limits.max_arena_nodes),
        ));
    }

    /// Return true when a package exists in this session's configured library paths.
    pub fn package_available(&self, package: &str) -> bool {
        is_valid_package_name(package) && self.core.find_package_path(package).is_some()
    }

    /// Return the resolved package directory, if it exists in this session.
    pub fn package_path(&self, package: &str) -> Option<String> {
        if !is_valid_package_name(package) {
            return None;
        }
        self.core
            .find_package_path(package)
            .map(|path| path.to_string_lossy().into_owned())
    }

    /// Load a pure-R package by name through the same evaluator path as `library()`.
    pub fn load_package(&mut self, package: &str) -> Result<(), String> {
        if !is_valid_package_name(package) {
            return Err("invalid package name".to_string());
        }
        let result = self.eval(&format!("library(\"{package}\")"));
        match result.typed {
            RValue::Error(message) => Err(message),
            _ => Ok(()),
        }
    }

    pub fn set_cancellation_token(&mut self, token: Option<CancellationToken>) {
        self.core.set_cancellation_token(token);
    }

    /// Evaluate with a cancellation token scoped to this call.
    ///
    /// The previous token is restored afterward, so a cancelled Android tab does
    /// not poison later work in the same session or any other session.
    pub fn eval_with_cancellation_token(
        &mut self,
        code: &str,
        token: Option<CancellationToken>,
    ) -> RResult {
        let previous = self.core.replace_cancellation_token(token);
        let result = self.eval(code);
        self.core.set_cancellation_token(previous);
        result
    }

    /// Configure app-private runtime paths for Android embedding.
    ///
    /// `app_files_dir` owns the writable user library, `cache_dir` owns
    /// `tempdir()`, and `bundled_library_dir` points at the read-only package
    /// library shipped with the app.
    pub fn configure_paths(
        &mut self,
        app_files_dir: &str,
        cache_dir: &str,
        bundled_library_dir: Option<&str>,
    ) -> Result<(), String> {
        self.core
            .configure_android_paths(app_files_dir, cache_dir, bundled_library_dir)
            .map_err(|err| err.to_string())
    }

    pub fn eval_integer(&mut self, value: i32) -> RResult {
        self.core
            .with_arena(|arena| builder::scalar_integer_in(arena, value).map(result_from_sexp))
            .flatten()
            .unwrap_or_else(allocation_error)
    }

    pub fn eval_real(&mut self, value: f64) -> RResult {
        self.core
            .with_arena(|arena| builder::scalar_real_in(arena, value).map(result_from_sexp))
            .flatten()
            .unwrap_or_else(allocation_error)
    }

    pub fn eval_int_vector(&mut self, values: &[i32]) -> RResult {
        self.core
            .with_arena(|arena| builder::int_vec_in(arena, values).map(result_from_sexp))
            .flatten()
            .unwrap_or_else(allocation_error)
    }

    pub fn eval_real_vector(&mut self, values: &[f64]) -> RResult {
        self.core
            .with_arena(|arena| builder::real_vec_in(arena, values).map(result_from_sexp))
            .flatten()
            .unwrap_or_else(allocation_error)
    }

    /// Compute a mathematical function on a single value.
    pub fn math_unary(&self, func: RMathFunc, x: f64) -> f64 {
        match func {
            RMathFunc::Abs => x.abs(),
            RMathFunc::Sqrt => libm::sqrt(x),
            RMathFunc::Ceil => libm::ceil(x),
            RMathFunc::Floor => libm::floor(x),
            RMathFunc::Trunc => libm::trunc(x),
            RMathFunc::Exp => libm::exp(x),
            RMathFunc::Log => libm::log(x),
            RMathFunc::Log2 => libm::log2(x),
            RMathFunc::Log10 => libm::log10(x),
            RMathFunc::Sin => libm::sin(x),
            RMathFunc::Cos => libm::cos(x),
            RMathFunc::Tan => libm::tan(x),
            RMathFunc::Asin => libm::asin(x),
            RMathFunc::Acos => libm::acos(x),
            RMathFunc::Atan => libm::atan(x),
            RMathFunc::Gamma => libm::tgamma(x),
            RMathFunc::Lgamma => libm::lgamma(x),
        }
    }

    /// Compute a Bessel function.
    pub fn bessel(&self, func: RBesselFunc, x: f64, alpha: f64, scaled: bool) -> f64 {
        match func {
            RBesselFunc::J => crate::special::bessel::bessel_j(x, alpha),
            RBesselFunc::Y => crate::special::bessel::bessel_y(x, alpha),
            RBesselFunc::I => crate::special::bessel::bessel_i(x, alpha, scaled),
            RBesselFunc::K => crate::special::bessel::bessel_k(x, alpha, scaled),
        }
    }

    /// Probability density function for the normal distribution.
    pub fn dnorm(&self, x: f64, mean: f64, sd: f64, log: bool) -> f64 {
        crate::dist::normal::dnorm(x, mean, sd, if log { 1 } else { 0 })
    }

    pub fn pnorm(&self, x: f64, mean: f64, sd: f64, lower_tail: bool, log: bool) -> f64 {
        crate::dist::normal::pnorm(
            x,
            mean,
            sd,
            if lower_tail { 1 } else { 0 },
            if log { 1 } else { 0 },
        )
    }

    pub fn qnorm(&self, p: f64, mean: f64, sd: f64, lower_tail: bool, log: bool) -> f64 {
        crate::dist::normal::qnorm(
            p,
            mean,
            sd,
            if lower_tail { 1 } else { 0 },
            if log { 1 } else { 0 },
        )
    }

    pub fn unif_rand(&self) -> f64 {
        self.core.unif_rand()
    }

    pub fn set_seed(&self, i1: u32, i2: u32) {
        self.core.set_seed(i1, i2);
    }

    pub fn norm_rand(&self) -> f64 {
        self.core.norm_rand()
    }

    /// Parse and evaluate an R expression.
    ///
    /// Parses and evaluates code against this session's isolated global
    /// environment.
    pub fn eval(&mut self, code: &str) -> RResult {
        let (result, captured, visible) = self.core.eval_code_with_output_capture(code);
        match result {
            Ok(result) => result_from_eval(result, captured, visible),
            Err(e) => error_result(e.to_string()),
        }
    }
}

fn allocation_error() -> RResult {
    error_result("allocation failed")
}

impl Default for RSession {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of an R evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct RResult {
    /// Legacy numeric scalar view for existing simple callers.
    pub value: f64,
    /// Owned typed value for Android/FFI callers that should not parse output.
    pub typed: RValue,
    /// R-style display output.
    pub output: String,
}

fn saturating_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// Runtime state needed by Android hosts to wire package libraries and temp files.
#[derive(Debug, Clone, PartialEq)]
pub struct RRuntimeInfo {
    pub is_active: bool,
    pub library_paths: Vec<String>,
    pub temp_dir: String,
}

/// Snapshot of one session's arena allocator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RArenaStats {
    pub active_nodes: u64,
    pub free_nodes: u64,
    pub retained_bytes: u64,
    pub fragmentation_ratio: f64,
}

impl Default for RArenaStats {
    fn default() -> Self {
        Self {
            active_nodes: 0,
            free_nodes: 0,
            retained_bytes: 0,
            fragmentation_ratio: 0.0,
        }
    }
}

/// Host-owned resource limits for Android sessions.
///
/// A value of `0` means unlimited for that dimension, except
/// `max_eval_depth`, where `0` selects the evaluator default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RResourceLimits {
    pub max_eval_depth: u64,
    pub max_execution_time_ms: u64,
    pub max_alloc_bytes: u64,
    pub max_arena_nodes: u64,
}

impl Default for RResourceLimits {
    fn default() -> Self {
        RResourceLimits {
            max_eval_depth: 500,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        }
    }
}

impl RResult {
    fn with_error_output(mut self) -> Self {
        if let RValue::Error(message) = &self.typed {
            self.output = format!("Error: {message}");
        }
        self
    }
}

/// Owned representation of evaluated R values suitable for FFI boundaries.
#[derive(Debug, Clone, PartialEq)]
pub enum RValue {
    Null,
    Logical(Option<bool>),
    Integer(Option<i32>),
    Real(Option<f64>),
    LogicalVector(Vec<Option<bool>>),
    IntegerVector(Vec<Option<i32>>),
    RealVector(Vec<Option<f64>>),
    StringVector(Vec<Option<String>>),
    RawVector(Vec<u8>),
    ComplexVector(Vec<Option<RComplexValue>>),
    List(Vec<RValue>),
    Attributed {
        value: Box<RValue>,
        metadata: RMetadata,
    },
    Unsupported {
        type_name: String,
    },
    Error(String),
}

/// Named owned R attribute for Android/UniFFI callers.
#[derive(Debug, Clone, PartialEq)]
pub struct RAttribute {
    pub name: String,
    pub value: RValue,
}

/// Owned R metadata for Android/UniFFI callers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RMetadata {
    pub names: Option<Vec<Option<String>>>,
    pub dim: Option<Vec<i32>>,
    pub class: Option<Vec<Option<String>>>,
    pub levels: Option<Vec<Option<String>>>,
    pub attributes: Vec<RAttribute>,
}

/// Owned complex number for Android/UniFFI callers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RComplexValue {
    pub real: f64,
    pub imaginary: f64,
}

impl From<SexpComplex> for RComplexValue {
    fn from(value: SexpComplex) -> Self {
        RComplexValue {
            real: value.real,
            imaginary: value.imaginary,
        }
    }
}

impl RValue {
    pub fn from_sexp(sexp: Sexp<'_>) -> Self {
        match sexp.to_owned_value() {
            Ok(value) => RValue::from_owned_value(value),
            Err(_error) => RValue::Unsupported {
                type_name: format!("invalid {}", sexp.typeof_().0),
            },
        }
    }

    fn from_owned_value(value: SexpValue) -> Self {
        match value {
            SexpValue::Null => RValue::Null,
            SexpValue::Logical(value) => RValue::Logical(value),
            SexpValue::Integer(value) => RValue::Integer(value),
            SexpValue::Real(value) => RValue::Real(value),
            SexpValue::LogicalVector(values) => RValue::LogicalVector(values),
            SexpValue::IntegerVector(values) => RValue::IntegerVector(values),
            SexpValue::RealVector(values) => RValue::RealVector(values),
            SexpValue::StringVector(values) => RValue::StringVector(values),
            SexpValue::RawVector(values) => RValue::RawVector(values),
            SexpValue::ComplexVector(values) => RValue::ComplexVector(
                values
                    .into_iter()
                    .map(|value| value.map(RComplexValue::from))
                    .collect(),
            ),
            SexpValue::List(values) => {
                RValue::List(values.into_iter().map(RValue::from_owned_value).collect())
            }
            SexpValue::Attributed { value, metadata } => RValue::Attributed {
                value: Box::new(RValue::from_owned_value(*value)),
                metadata: RMetadata::from(metadata),
            },
            SexpValue::Unsupported { type_name } => RValue::Unsupported { type_name },
        }
    }

    fn numeric_scalar_value(&self) -> f64 {
        match self {
            RValue::Integer(Some(value)) => *value as f64,
            RValue::Integer(None) => f64::NAN,
            RValue::Real(Some(value)) => *value,
            RValue::Real(None) => f64::NAN,
            RValue::Logical(Some(true)) => 1.0,
            RValue::Logical(Some(false)) => 0.0,
            RValue::Logical(None) => f64::NAN,
            RValue::IntegerVector(values) => values
                .first()
                .and_then(|value| *value)
                .map(|value| value as f64)
                .unwrap_or(f64::NAN),
            RValue::RealVector(values) => {
                values.first().and_then(|value| *value).unwrap_or(f64::NAN)
            }
            RValue::LogicalVector(values) => match values.first().copied().flatten() {
                Some(true) => 1.0,
                Some(false) => 0.0,
                None => f64::NAN,
            },
            RValue::Attributed { value, .. } => value.numeric_scalar_value(),
            _ => 0.0,
        }
    }
}

impl From<SexpAttribute> for RAttribute {
    fn from(attribute: SexpAttribute) -> Self {
        RAttribute {
            name: attribute.name,
            value: RValue::from_owned_value(attribute.value),
        }
    }
}

impl From<SexpMetadata> for RMetadata {
    fn from(metadata: SexpMetadata) -> Self {
        RMetadata {
            names: metadata.names,
            dim: metadata.dim,
            class: metadata.class,
            levels: metadata.levels,
            attributes: metadata
                .attributes
                .into_iter()
                .map(RAttribute::from)
                .collect(),
        }
    }
}

/// Supported mathematical functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RMathFunc {
    Abs,
    Sqrt,
    Ceil,
    Floor,
    Trunc,
    Exp,
    Log,
    Log2,
    Log10,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Gamma,
    Lgamma,
}

/// Supported Bessel function families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RBesselFunc {
    J,
    Y,
    I,
    K,
}

// ---------------------------------------------------------------------------
// Free functions for simple operations (no session needed)
// ---------------------------------------------------------------------------

/// Compute `dnorm(x, mean, sd)`.
pub fn dnorm_free(x: f64, mean: f64, sd: f64) -> f64 {
    crate::dist::normal::dnorm(x, mean, sd, 0)
}

pub fn pnorm_free(x: f64, mean: f64, sd: f64) -> f64 {
    crate::dist::normal::pnorm(x, mean, sd, 1, 0)
}

pub fn qnorm_free(p: f64, mean: f64, sd: f64) -> f64 {
    crate::dist::normal::qnorm(p, mean, sd, 1, 0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn string_vector(values: Vec<String>) -> RValue {
        RValue::StringVector(values.into_iter().map(Some).collect())
    }

    fn literal_string_vector(values: &[&str]) -> RValue {
        string_vector(values.iter().map(|value| (*value).to_string()).collect())
    }

    fn unique_test_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rport-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ))
    }

    fn write_package(
        root: &Path,
        name: &str,
        namespace: &str,
        r_source: &str,
    ) -> std::path::PathBuf {
        let pkg = root.join(name);
        let r_dir = pkg.join("R");
        std::fs::create_dir_all(&r_dir).expect("package R dir");
        std::fs::write(
            pkg.join("DESCRIPTION"),
            format!("Package: {name}\nVersion: 0.0.1\n"),
        )
        .expect("description");
        std::fs::write(pkg.join("NAMESPACE"), namespace).expect("namespace");
        std::fs::write(r_dir.join(format!("{name}.R")), r_source).expect("R source");
        pkg
    }

    #[test]
    fn test_session_new() {
        let mut session = RSession::new();
        assert!(
            session
                .core
                .with_arena(|arena| arena.node_count() > 0)
                .unwrap_or(false)
        );
        assert_eq!(session.eval("1 + 1").output, "[1] 2");
    }

    #[test]
    fn test_android_path_policy_drives_libpaths_find_package_and_tempdir() {
        let root = std::env::temp_dir().join(format!(
            "rport-android-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");
        let base_pkg = bundled.join("base");
        std::fs::create_dir_all(&base_pkg).expect("package dir");
        std::fs::write(base_pkg.join("DESCRIPTION"), "Package: base\n").expect("description");

        let mut session = RSession::new();
        session
            .configure_paths(
                files.to_str().expect("utf8 files path"),
                cache.to_str().expect("utf8 cache path"),
                Some(bundled.to_str().expect("utf8 bundled path")),
            )
            .expect("configure paths");

        let lib_paths = session.eval(".libPaths()");
        assert_eq!(
            lib_paths.typed,
            string_vector(vec![
                files
                    .join("R")
                    .join("library")
                    .to_string_lossy()
                    .into_owned(),
                bundled.to_string_lossy().into_owned()
            ])
        );

        let base_path = session.eval("find.package(\"base\")");
        assert_eq!(
            base_path.typed,
            string_vector(vec![base_pkg.to_string_lossy().into_owned()])
        );
        let installed = session.eval("installed.packages()");
        match installed.typed {
            RValue::Attributed { value, metadata } => {
                assert_eq!(metadata.dim, Some(vec![1, 16]));
                match *value {
                    RValue::StringVector(values) => {
                        assert_eq!(values[0], Some("base".to_string()));
                        assert_eq!(values[1], Some(bundled.to_string_lossy().into_owned()));
                    }
                    other => panic!("expected installed package string matrix, got {other:?}"),
                }
            }
            other => panic!("expected attributed installed package matrix, got {other:?}"),
        }

        let tempdir = session.eval("tempdir()");
        assert_eq!(
            tempdir.typed,
            string_vector(vec![cache.join("Rtmp").to_string_lossy().into_owned()])
        );
        assert_eq!(session.eval("file.exists(tempdir())").output, "[1] TRUE");
        assert_eq!(
            session.runtime_info(),
            RRuntimeInfo {
                is_active: true,
                library_paths: vec![
                    files
                        .join("R")
                        .join("library")
                        .to_string_lossy()
                        .into_owned(),
                    bundled.to_string_lossy().into_owned()
                ],
                temp_dir: cache.join("Rtmp").to_string_lossy().into_owned(),
            }
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_library_loads_pure_r_package_from_android_paths() {
        let root = unique_test_root("android-package");
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");
        let pkg = write_package(
            &bundled,
            "tiny",
            "export(tiny_value, tiny_label)\n",
            "tiny_secret <- function() 42L\ntiny_value <- function() tiny_secret()\ntiny_label <- \"loaded\"\n",
        );
        let data_dir = pkg.join("data");
        std::fs::create_dir_all(&data_dir).expect("data dir");
        std::fs::write(data_dir.join("tiny_data.R"), "tiny_data <- 314L\n").expect("data file");

        let mut session = RSession::new();
        session
            .configure_paths(
                files.to_str().expect("utf8 files path"),
                cache.to_str().expect("utf8 cache path"),
                Some(bundled.to_str().expect("utf8 bundled path")),
            )
            .expect("configure paths");

        let require = session.eval("require(\"tiny\")");
        assert_eq!(require.output, "");
        assert_eq!(require.typed, RValue::Logical(Some(true)));
        assert_eq!(session.eval("tiny_value()").output, "[1] 42");
        assert_eq!(
            session.eval("data(package = \"tiny\")").typed,
            string_vector(vec!["tiny_data".to_string()])
        );
        assert_eq!(
            session
                .eval("data(\"tiny_data\", package = \"tiny\")\ntiny_data")
                .output,
            "[1] 314"
        );
        assert_eq!(
            session.eval("tiny_label").typed,
            string_vector(vec!["loaded".to_string()])
        );
        let secret = session.eval("tiny_secret");
        assert!(matches!(secret.typed, RValue::Error(_)), "{secret:?}");
        assert_eq!(session.eval("library(\"tiny\")").output, "");
        assert_eq!(
            session.eval("find.package(\"tiny\")").typed,
            string_vector(vec![pkg.to_string_lossy().into_owned()])
        );
        let search = session.eval("search()");
        match search.typed {
            RValue::StringVector(values) => {
                assert!(
                    values.contains(&Some("package:tiny".to_string())),
                    "{values:?}"
                );
            }
            other => panic!("expected search path string vector, got {other:?}"),
        }
        assert_eq!(session.eval("detach(\"package:tiny\")").typed, RValue::Null);
        let search = session.eval("search()");
        match search.typed {
            RValue::StringVector(values) => {
                assert!(
                    !values.contains(&Some("package:tiny".to_string())),
                    "{values:?}"
                );
            }
            other => panic!("expected search path string vector, got {other:?}"),
        }
        let after_detach = session.eval("tiny_value");
        assert!(
            matches!(after_detach.typed, RValue::Error(_)),
            "{after_detach:?}"
        );
        assert_eq!(session.eval("require(\"tiny\")").output, "");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_library_supports_simple_namespace_imports_and_export_patterns() {
        let root = unique_test_root("android-namespace");
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");

        write_package(
            &bundled,
            "depall",
            "export(dep_value, dep_helper)\n",
            "dep_value <- function() 7L\ndep_helper <- function(x) x + 1L\ndep_hidden <- function() 99L\n",
        );
        write_package(
            &bundled,
            "depfrom",
            "export(dep_pick, dep_extra)\n",
            "dep_pick <- function() 11L\ndep_extra <- function() 100L\n",
        );
        let tiny = write_package(
            &bundled,
            "tiny",
            "import(depall)\nimportFrom(depfrom, dep_pick)\nexport(tiny_value)\nexportPattern(\"^tiny_\")\n",
            "tiny_value <- function() dep_helper(dep_value()) + dep_pick()\ntiny_label <- \"namespace\"\ntiny_hidden <- function() dep_hidden()\ntiny_imported <- function() dep_pick()\n",
        );

        let mut session = RSession::new();
        session
            .configure_paths(
                files.to_str().expect("utf8 files path"),
                cache.to_str().expect("utf8 cache path"),
                Some(bundled.to_str().expect("utf8 bundled path")),
            )
            .expect("configure paths");

        assert_eq!(session.eval("library(\"tiny\")").output, "");
        assert_eq!(session.eval("tiny_value()").output, "[1] 19");
        assert_eq!(
            session.eval("tiny_label").typed,
            string_vector(vec!["namespace".to_string()])
        );
        assert_eq!(session.eval("tiny_imported()").output, "[1] 11");

        let dep_value = session.eval("dep_value");
        assert!(matches!(dep_value.typed, RValue::Error(_)), "{dep_value:?}");
        let dep_pick = session.eval("dep_pick");
        assert!(matches!(dep_pick.typed, RValue::Error(_)), "{dep_pick:?}");
        let dep_hidden = session.eval("dep_hidden");
        assert!(
            matches!(dep_hidden.typed, RValue::Error(_)),
            "{dep_hidden:?}"
        );

        assert_eq!(
            session.eval("find.package(\"tiny\")").typed,
            string_vector(vec![tiny.to_string_lossy().into_owned()])
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_s3_method_registry_and_namespace_directives() {
        let root = unique_test_root("android-s3");
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");
        write_package(
            &bundled,
            "tiny",
            "export(tiny_generic)\nS3method(tiny_generic, myclass)\n",
            "tiny_generic <- function(x) UseMethod(\"tiny_generic\", x)\ntiny_generic.myclass <- function(x) 77L\n",
        );

        let mut session = RSession::new();
        session
            .configure_paths(
                files.to_str().expect("utf8 files path"),
                cache.to_str().expect("utf8 cache path"),
                Some(bundled.to_str().expect("utf8 bundled path")),
            )
            .expect("configure paths");

        assert_eq!(session.eval("library(\"tiny\")").output, "");
        assert_eq!(
            session
                .eval("hasS3method(\"tiny_generic\", \"myclass\")")
                .typed,
            RValue::Logical(Some(true))
        );
        assert_eq!(
            session
                .eval("getS3method(\"tiny_generic\", \"myclass\")(1L)")
                .output,
            "[1] 77"
        );
        assert_eq!(
            session
                .eval("x <- 1L\nclass(x) <- \"myclass\"\ntiny_generic(x)")
                .output,
            "[1] 77"
        );

        let private_method = session.eval("tiny_generic.myclass");
        assert!(
            matches!(private_method.typed, RValue::Error(_)),
            "{private_method:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_library_rejects_native_code_packages_explicitly() {
        let root = unique_test_root("android-native-policy");
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");
        write_package(
            &bundled,
            "tiny",
            "useDynLib(tiny)\nexport(tiny_value)\n",
            "tiny_value <- function() 42L\n",
        );

        let mut session = RSession::new();
        session
            .configure_paths(
                files.to_str().expect("utf8 files path"),
                cache.to_str().expect("utf8 cache path"),
                Some(bundled.to_str().expect("utf8 bundled path")),
            )
            .expect("configure paths");

        let result = session.eval("library(\"tiny\")");
        match result.typed {
            RValue::Error(message) => {
                assert!(message.contains("useDynLib(tiny)"), "{message}");
                assert!(message.contains("pure-R Android runtime"), "{message}");
            }
            other => panic!("expected native package load error, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_data_rejects_serialized_package_data_explicitly() {
        let root = unique_test_root("android-data-policy");
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");
        let pkg = write_package(
            &bundled,
            "tiny",
            "export(tiny_value)\n",
            "tiny_value <- function() 42L\n",
        );
        let data_dir = pkg.join("data");
        std::fs::create_dir_all(&data_dir).expect("data dir");
        std::fs::write(data_dir.join("tiny_data.rda"), b"unsupported").expect("rda file");

        let mut session = RSession::new();
        session
            .configure_paths(
                files.to_str().expect("utf8 files path"),
                cache.to_str().expect("utf8 cache path"),
                Some(bundled.to_str().expect("utf8 bundled path")),
            )
            .expect("configure paths");

        let result = session.eval("data(\"tiny_data\", package = \"tiny\")");
        match result.typed {
            RValue::Error(message) => {
                assert!(
                    message.contains("unsupported serialized/lazy data"),
                    "{message}"
                );
                assert!(message.contains("data/*.R only"), "{message}");
            }
            other => panic!("expected data policy error, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_session_eval_integer() {
        let mut session = RSession::new();
        let result = session.eval_integer(42);
        assert_eq!(result.value, 42.0);
        assert!(result.output.contains("42"));
        assert_eq!(result.typed, RValue::Integer(Some(42)));
    }

    #[test]
    fn test_session_eval_real() {
        let mut session = RSession::new();
        let result = session.eval_real(3.14);
        assert!((result.value - 3.14).abs() < f64::EPSILON);
        assert!(result.output.contains("3.14"));
    }

    #[test]
    fn test_session_eval_int_vector() {
        let mut session = RSession::new();
        let result = session.eval_int_vector(&[1, 2, 3]);
        assert!(result.output.contains('1'));
        assert_eq!(
            result.typed,
            RValue::IntegerVector(vec![Some(1), Some(2), Some(3)])
        );
    }

    #[test]
    fn test_eval_returns_owned_typed_values() {
        let mut session = RSession::new();
        let strings = session.eval("c(\"a\", \"b\")");
        let strings_with_na = session.eval("c(\"a\", NA_character_)");
        let logical = session.eval("TRUE");
        let list = session.eval("list(1, \"x\")");

        assert_eq!(strings.typed, literal_string_vector(&["a", "b"]));
        assert_eq!(
            strings_with_na.typed,
            RValue::StringVector(vec![Some("a".to_string()), None])
        );
        assert_eq!(logical.typed, RValue::Logical(Some(true)));
        assert_eq!(
            list.typed,
            RValue::List(vec![RValue::Real(Some(1.0)), literal_string_vector(&["x"])])
        );
    }

    #[test]
    fn test_eval_preserves_metadata_in_owned_typed_values() {
        let _session = crate::sexp::session::RSession::new();
        let mut arena = crate::sexp::memory::RArena::new();
        let vector = Sexp::from_raw(arena.alloc_vector(SEXPTYPE::INTSXP, 2)).expect("vector");
        vector.try_set_integer_elt(0, 1).expect("set integer");
        vector.try_set_integer_elt(1, 2).expect("set integer");

        let names = Sexp::from_raw(arena.alloc_vector(SEXPTYPE::STRSXP, 2)).expect("names");
        names
            .try_set_string_elt(0, Sexp::from_raw(arena.alloc_charsxp(b"a")).expect("name"))
            .expect("set name");
        names
            .try_set_string_elt(1, Sexp::from_raw(arena.alloc_charsxp(b"b")).expect("name"))
            .expect("set name");

        let class = Sexp::from_raw(arena.alloc_vector(SEXPTYPE::STRSXP, 1)).expect("class");
        class
            .try_set_string_elt(
                0,
                Sexp::from_raw(arena.alloc_charsxp(b"foo")).expect("class"),
            )
            .expect("set class");

        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        let class_cell = arena.cons(class.as_raw(), nil, unsafe {
            crate::sexp::symbol::Rf_install(c"class".as_ptr())
        });
        let names_cell = arena.cons(names.as_raw(), class_cell, unsafe {
            crate::sexp::symbol::Rf_install(c"names".as_ptr())
        });
        unsafe { crate::sexp::accessors::SET_ATTRIB(vector.as_raw(), names_cell) };

        let typed = RValue::from_sexp(vector);
        let RValue::Attributed { value, metadata } = typed else {
            panic!("expected attributed value");
        };

        assert_eq!(*value, RValue::IntegerVector(vec![Some(1), Some(2)]));
        assert_eq!(
            metadata.names,
            Some(vec![Some("a".to_string()), Some("b".to_string())])
        );
        assert_eq!(metadata.class, Some(vec![Some("foo".to_string())]));
        assert!(
            metadata
                .attributes
                .iter()
                .any(|attribute| attribute.name == "names")
        );
        assert_eq!(value.numeric_scalar_value(), 1.0);
    }

    #[test]
    fn test_typed_values_expose_raw_and_complex_without_print_parsing() {
        let mut session = RSession::new();
        let raw = session.eval("as.raw(c(65, 90))");
        assert_eq!(raw.typed, RValue::RawVector(vec![0x41, 0x5a]));

        let mut arena = crate::sexp::memory::RArena::new();
        let complex = Sexp::from_raw(arena.alloc_vector(SEXPTYPE::CPLXSXP, 2)).unwrap();
        complex
            .try_set_complex_elt(0, crate::sexp::Rcomplex { r: 1.0, i: -2.0 })
            .unwrap();
        complex
            .try_set_complex_elt(
                1,
                crate::sexp::Rcomplex {
                    r: crate::sexp::NA_REAL,
                    i: 0.0,
                },
            )
            .unwrap();

        assert_eq!(
            RValue::from_sexp(complex),
            RValue::ComplexVector(vec![
                Some(RComplexValue {
                    real: 1.0,
                    imaginary: -2.0,
                }),
                None,
            ])
        );
    }

    #[test]
    fn test_unsupported_typed_values_only_carry_type_name() {
        let mut arena = crate::sexp::memory::RArena::new();
        let closure = Sexp::from_raw(arena.alloc_node(SEXPTYPE::CLOSXP)).unwrap();

        assert_eq!(
            RValue::from_sexp(closure),
            RValue::Unsupported {
                type_name: "closure".to_string(),
            }
        );
    }

    #[test]
    fn test_session_math_unary() {
        let session = RSession::new();
        assert!((session.math_unary(RMathFunc::Sqrt, 4.0) - 2.0).abs() < 1e-10);
        assert!((session.math_unary(RMathFunc::Exp, 0.0) - 1.0).abs() < 1e-10);
        assert!(session.math_unary(RMathFunc::Log, 1.0).abs() < 1e-10);
        assert!(session.math_unary(RMathFunc::Sin, 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_session_bessel() {
        let session = RSession::new();
        let j0 = session.bessel(RBesselFunc::J, 0.0, 0.0, false);
        assert!((j0 - 1.0).abs() < 1e-10);

        let i0 = session.bessel(RBesselFunc::I, 0.0, 0.0, false);
        assert!((i0 - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_session_dnorm_pnorm() {
        let session = RSession::new();
        let d = session.dnorm(0.0, 0.0, 1.0, false);
        assert!((d - 0.3989422804014327).abs() < 1e-10);

        let p = session.pnorm(0.0, 0.0, 1.0, true, false);
        assert!((p - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_free_functions() {
        assert!((dnorm_free(0.0, 0.0, 1.0) - 0.3989422804014327).abs() < 1e-10);
        assert!((pnorm_free(0.0, 0.0, 1.0) - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_eval_integer_literal() {
        let mut session = RSession::new();
        let result = session.eval("42");
        assert!(
            (result.value - 42.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_real_literal() {
        let mut session = RSession::new();
        let result = session.eval("3.14");
        assert!(
            (result.value - 3.14).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_addition() {
        let mut session = RSession::new();
        let result = session.eval("1 + 2");
        assert!(
            (result.value - 3.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_honors_visibility_for_assignment_and_invisible() {
        let mut session = RSession::new();

        let assigned = session.eval("x <- 42");
        assert_eq!(assigned.output, "");
        assert_eq!(assigned.typed, RValue::Real(Some(42.0)));

        let invisible = session.eval("invisible(7)");
        assert_eq!(invisible.output, "");
        assert_eq!(invisible.typed, RValue::Real(Some(7.0)));

        let visible = session.eval("x");
        assert_eq!(visible.output, "[1] 42");
    }

    #[test]
    fn test_eval_captures_explicit_output_without_implicit_reprint() {
        let mut session = RSession::new();

        let cat = session.eval("cat(\"hello\")");
        assert_eq!(cat.output, "hello");
        assert_eq!(cat.typed, RValue::Null);

        let printed = session.eval("print(1)");
        assert_eq!(printed.output, "[1] 1\n");
        assert_eq!(printed.typed, RValue::Real(Some(1.0)));
    }

    #[test]
    fn test_stopifnot_is_invisible_on_success_and_errors_on_failure() {
        let mut session = RSession::new();

        let ok = session.eval("stopifnot(TRUE)");
        assert_eq!(ok.output, "");
        assert_eq!(ok.typed, RValue::Null);

        let err = session.eval("stopifnot(FALSE)");
        assert!(matches!(err.typed, RValue::Error(_)));
        assert!(err.output.contains("FALSE is not TRUE"));
    }

    #[test]
    fn test_with_visible_returns_visible_list_with_captured_flag() {
        let mut session = RSession::new();

        let visible = session.eval("withVisible(1)");
        assert_eq!(visible.output, "$value\n[1] 1\n\n$visible\n[1] TRUE");
        let RValue::Attributed { value, metadata } = visible.typed else {
            panic!("expected attributed withVisible result");
        };
        assert_eq!(
            *value,
            RValue::List(vec![RValue::Real(Some(1.0)), RValue::Logical(Some(true))])
        );
        assert_eq!(
            metadata.names,
            Some(vec![Some("value".to_string()), Some("visible".to_string())])
        );

        let invisible = session.eval("withVisible(invisible(1))");
        assert_eq!(invisible.output, "$value\n[1] 1\n\n$visible\n[1] FALSE");
        let RValue::Attributed { value, metadata } = invisible.typed else {
            panic!("expected attributed withVisible result");
        };
        assert_eq!(
            *value,
            RValue::List(vec![RValue::Real(Some(1.0)), RValue::Logical(Some(false))])
        );
        assert_eq!(
            metadata.names,
            Some(vec![Some("value".to_string()), Some("visible".to_string())])
        );
    }

    #[test]
    fn test_capture_output_evaluates_expression_under_capture() {
        let mut session = RSession::new();

        let printed = session.eval("capture.output(print(1))");
        assert_eq!(printed.output, "[1] \"[1] 1\"");
        assert_eq!(printed.typed, literal_string_vector(&["[1] 1"]));

        let cat = session.eval("capture.output(cat(\"hello\"))");
        assert_eq!(cat.output, "[1] \"hello\"");
        assert_eq!(cat.typed, literal_string_vector(&["hello"]));
    }

    #[test]
    fn test_stop_warning_message_and_suppression() {
        let mut session = RSession::new();

        let stopped = session.eval("stop(\"boom\")");
        assert!(matches!(stopped.typed, RValue::Error(_)));
        assert_eq!(stopped.output, "Error: boom");

        let warned = session.eval("warning(\"careful\"); 1");
        assert_eq!(warned.output, "Warning message:\ncareful \n[1] 1");

        let messaged = session.eval("message(\"hi\"); 1");
        assert_eq!(messaged.output, "hi\n[1] 1");

        let suppress_warning = session.eval("suppressWarnings(warning(\"careful\")); 1");
        assert_eq!(suppress_warning.output, "[1] 1");

        let suppress_message = session.eval("suppressMessages(message(\"hi\")); 1");
        assert_eq!(suppress_message.output, "[1] 1");
    }

    #[test]
    fn test_regexpr_reports_match_attributes() {
        let mut session = RSession::new();

        let result = session.eval("regexpr(\"a\", c(\"cat\", \"dog\"))");
        assert_eq!(
            result.output,
            "[1]  2 -1\nattr(,\"match.length\")\n[1]  1 -1\nattr(,\"index.type\")\n[1] \"chars\"\nattr(,\"useBytes\")\n[1] TRUE"
        );

        let match_length =
            session.eval("attr(regexpr(\"a\", c(\"cat\", \"dog\")), \"match.length\")");
        assert_eq!(match_length.output, "[1]  1 -1");

        let use_bytes = session.eval("attr(regexpr(\"a\", \"cat\"), \"useBytes\")");
        assert_eq!(use_bytes.output, "[1] TRUE");
    }

    #[test]
    fn test_sample_int_uniform_shape_and_errors() {
        let mut session = RSession::new();

        let permutation = session.eval("all(sort(sample.int(5)) == 1:5)");
        assert_eq!(permutation.output, "[1] TRUE");

        let replace = session.eval("length(sample.int(3, 7, TRUE))");
        assert_eq!(replace.output, "[1] 7");

        let too_large = session.eval("sample.int(3, 4, FALSE)");
        assert!(matches!(too_large.typed, RValue::Error(_)));
        assert!(
            too_large
                .output
                .contains("cannot take a sample larger than the population when 'replace = FALSE'")
        );
    }

    #[test]
    fn test_sample_matches_core_r_shape_and_type_rules() {
        let mut session = RSession::new();

        let shortcut = session.eval("all(sort(sample(5)) == 1:5)");
        assert_eq!(shortcut.output, "[1] TRUE");

        let integer_type = session.eval("is.integer(sample(5, 3))");
        assert_eq!(integer_type.typed, RValue::Logical(Some(true)));

        let character_type = session.eval("is.character(sample(c(\"a\", \"b\", \"c\"), 2))");
        assert_eq!(character_type.typed, RValue::Logical(Some(true)));

        let logical_type = session.eval("is.logical(sample(c(TRUE, FALSE), 2))");
        assert_eq!(logical_type.typed, RValue::Logical(Some(true)));

        let names = session.eval(
            "x <- c(a = 10, b = 20, c = 30)\ny <- sample(x, 2)\nall(c(length(names(y)) == 2, names(y) %in% names(x)))",
        );
        assert_eq!(names.typed, RValue::Logical(Some(true)));

        let too_large = session.eval("sample(c(\"a\", \"b\"), 3, FALSE)");
        assert!(matches!(too_large.typed, RValue::Error(_)));
        assert!(
            too_large
                .output
                .contains("cannot take a sample larger than the population when 'replace = FALSE'")
        );

        let weighted_replace =
            session.eval("all(sample(c(\"a\", \"b\", \"c\"), 5, TRUE, c(0, 0, 1)) == \"c\")");
        assert_eq!(weighted_replace.typed, RValue::Logical(Some(true)));

        let weighted_no_replace = session.eval("all(sample(1:3, 2, FALSE, c(0, 1, 1)) != 1L)");
        assert_eq!(weighted_no_replace.typed, RValue::Logical(Some(true)));

        let impossible = session.eval("sample(1:3, 2, FALSE, c(1, 0, 0))");
        assert!(matches!(impossible.typed, RValue::Error(_)));
        assert!(impossible.output.contains("too few positive probabilities"));
    }

    #[test]
    fn test_proc_time_shape_matches_r_contract() {
        let mut session = RSession::new();

        let len = session.eval("length(proc.time())");
        assert_eq!(len.output, "[1] 5");

        let names = session.eval("toString(names(proc.time()))");
        assert_eq!(
            names.output,
            "[1] \"user.self, sys.self, elapsed, user.child, sys.child\""
        );

        let class = session.eval("class(proc.time())");
        assert_eq!(class.output, "[1] \"proc_time\"");
    }

    #[test]
    fn test_try_catch_error_handler_subset() {
        let mut session = RSession::new();

        let pass = session.eval("tryCatch(1 + 2, error=function(e) \"caught\")");
        assert_eq!(pass.output, "[1] 3");

        let caught = session.eval("tryCatch(stop(\"boom\"), error=function(e) \"caught\")");
        assert_eq!(caught.output, "[1] \"caught\"");

        let message =
            session.eval("tryCatch(stop(\"boom\"), error=function(e) conditionMessage(e))");
        assert_eq!(message.output, "[1] \"boom\"");
    }

    #[test]
    fn test_ls_lists_current_frame_like_r() {
        let mut session = RSession::new();

        let empty = session.eval("ls()");
        assert_eq!(empty.output, "character(0)");
        assert_eq!(empty.typed, RValue::StringVector(Vec::new()));

        let mut session = RSession::new();
        let sorted = session.eval("y <- 2; x <- 1; ls()");
        assert_eq!(sorted.output, "[1] \"x\" \"y\"");
        assert_eq!(sorted.typed, literal_string_vector(&["x", "y"]));

        let mut session = RSession::new();
        let hidden = session.eval(".hidden <- 1; visible <- 2; ls()");
        assert_eq!(hidden.output, "[1] \"visible\"");

        let all_names = session.eval("ls(all.names = TRUE)");
        assert_eq!(all_names.output, "[1] \".hidden\" \"visible\"");

        let mut session = RSession::new();
        let removed = session.eval("x <- 1; rm(\"x\"); length(ls())");
        assert_eq!(removed.output, "[1] 0");
    }

    #[test]
    fn test_runtime_predicates_match_public_r_surface() {
        let mut session = RSession::new();

        let primitive = session.eval("is.primitive(sum)");
        assert_eq!(primitive.output, "[1] TRUE");
        assert_eq!(primitive.typed, RValue::Logical(Some(true)));

        let closure = session.eval("is.primitive(function(x) x)");
        assert_eq!(closure.output, "[1] FALSE");
        assert_eq!(closure.typed, RValue::Logical(Some(false)));

        let loaded = session.eval("is.loaded(\"R_init_base\")");
        assert_eq!(loaded.output, "[1] FALSE");
        assert_eq!(loaded.typed, RValue::Logical(Some(false)));

        let single = session.eval("is.single(1)");
        assert!(matches!(single.typed, RValue::Error(_)));
        assert_eq!(single.output, "Error: type \"single\" unimplemented in R");
    }

    #[test]
    fn test_system_matches_r_output_and_status_shape() {
        let mut session = RSession::new();

        let streamed = session.eval("system(\"printf hi\")");
        assert_eq!(streamed.output, "hi");
        assert_eq!(streamed.typed, RValue::Integer(Some(0)));

        let interned = session.eval("system(\"printf hi\", intern = TRUE)");
        assert_eq!(interned.output, "[1] \"hi\"");
        assert_eq!(interned.typed, literal_string_vector(&["hi"]));

        let status = session.eval("status <- system(\"false\"); status");
        assert_eq!(status.output, "[1] 1");
    }

    #[test]
    fn test_eval_subtraction() {
        let mut session = RSession::new();
        let result = session.eval("10 - 3");
        assert!(
            (result.value - 7.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_multiplication() {
        let mut session = RSession::new();
        let result = session.eval("4 * 5");
        assert!(
            (result.value - 20.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_division() {
        let mut session = RSession::new();
        let result = session.eval("10 / 3");
        assert!(
            (result.value - 3.3333333333333335).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_power() {
        let mut session = RSession::new();
        let result = session.eval("2 ^ 10");
        assert!(
            (result.value - 1024.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_precedence() {
        let mut session = RSession::new();
        let result = session.eval("2 + 3 * 4");
        assert!(
            (result.value - 14.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_comparison() {
        let mut session = RSession::new();
        let lt = session.eval("1 < 2");
        assert!((lt.value - 1.0).abs() < 1e-10);

        let gt = session.eval("3 > 5");
        assert!((gt.value - 0.0).abs() < 1e-10);

        let eq = session.eval("7 == 7");
        assert!((eq.value - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_eval_arithmetic_recycling_and_integer_overflow_warnings() {
        let mut session = RSession::new();

        let recycled = session.eval("c(1, 2, 3) + c(10, 20)");
        assert!(
            recycled
                .output
                .contains("longer object length is not a multiple of shorter object length"),
            "output: {}",
            recycled.output
        );
        assert_eq!(
            recycled.typed,
            RValue::RealVector(vec![Some(11.0), Some(22.0), Some(13.0)])
        );

        let overflow = session.eval("c(1L, 2147483647L) + c(1L, 1L)");
        assert!(
            overflow.output.contains("NAs produced by integer overflow"),
            "output: {}",
            overflow.output
        );
        assert_eq!(overflow.typed, RValue::IntegerVector(vec![Some(2), None]));
    }

    #[test]
    fn test_eval_arithmetic_preserves_vector_and_matrix_attributes() {
        let mut session = RSession::new();

        let named = session.eval("x <- c(a = 1, b = 2, c = 3)\nx + 1");
        assert_eq!(
            named.typed,
            RValue::Attributed {
                value: Box::new(RValue::RealVector(vec![Some(2.0), Some(3.0), Some(4.0)])),
                metadata: RMetadata {
                    names: Some(vec![
                        Some("a".to_string()),
                        Some("b".to_string()),
                        Some("c".to_string()),
                    ]),
                    attributes: vec![RAttribute {
                        name: "names".to_string(),
                        value: RValue::StringVector(vec![
                            Some("a".to_string()),
                            Some("b".to_string()),
                            Some("c".to_string()),
                        ]),
                    }],
                    ..RMetadata::default()
                },
            }
        );

        let matrix = session.eval(
            "m <- matrix(1:4, nrow = 2)\n\
             dimnames(m) <- list(c(\"r1\", \"r2\"), c(\"c1\", \"c2\"))\n\
             m + 1",
        );
        assert_eq!(
            matrix.typed,
            RValue::Attributed {
                value: Box::new(RValue::RealVector(vec![
                    Some(2.0),
                    Some(3.0),
                    Some(4.0),
                    Some(5.0),
                ])),
                metadata: RMetadata {
                    dim: Some(vec![2, 2]),
                    attributes: vec![
                        RAttribute {
                            name: "dimnames".to_string(),
                            value: RValue::List(vec![
                                RValue::StringVector(vec![
                                    Some("r1".to_string()),
                                    Some("r2".to_string()),
                                ]),
                                RValue::StringVector(vec![
                                    Some("c1".to_string()),
                                    Some("c2".to_string()),
                                ]),
                            ]),
                        },
                        RAttribute {
                            name: "dim".to_string(),
                            value: RValue::IntegerVector(vec![Some(2), Some(2)]),
                        },
                    ],
                    ..RMetadata::default()
                },
            }
        );
    }

    #[test]
    fn test_eval_character_and_logical_comparisons_follow_r_recycling() {
        let mut session = RSession::new();

        let character = session.eval("c(\"a\", \"b\", NA_character_) < c(\"b\", \"a\", \"c\")");
        assert_eq!(
            character.typed,
            RValue::LogicalVector(vec![Some(true), Some(false), None])
        );

        let logical = session.eval("c(TRUE, FALSE, NA) == c(1L, 0L, 1L)");
        assert_eq!(
            logical.typed,
            RValue::LogicalVector(vec![Some(true), Some(true), None])
        );
    }

    #[test]
    fn test_eval_complex_arithmetic_returns_typed_complex_vectors() {
        let mut session = RSession::new();

        let sum = session.eval("as.complex(c(1, 2)) + as.complex(c(3, 4))");
        assert_eq!(
            sum.typed,
            RValue::ComplexVector(vec![
                Some(RComplexValue {
                    real: 4.0,
                    imaginary: 0.0,
                }),
                Some(RComplexValue {
                    real: 6.0,
                    imaginary: 0.0,
                }),
            ])
        );

        let product = session.eval("as.complex(c(1, 2)) * as.complex(c(3, 4))");
        assert_eq!(
            product.typed,
            RValue::ComplexVector(vec![
                Some(RComplexValue {
                    real: 3.0,
                    imaginary: 0.0,
                }),
                Some(RComplexValue {
                    real: 8.0,
                    imaginary: 0.0,
                }),
            ])
        );
    }

    #[test]
    fn test_eval_assignment() {
        let mut session = RSession::new();
        let assign_result = session.eval("x <- 42");
        assert!(
            !assign_result.output.contains("Error"),
            "assign failed: {}",
            assign_result.output
        );

        let lookup = session.eval("x");
        assert!(
            !lookup.output.contains("Error"),
            "lookup failed: {}",
            lookup.output
        );
    }

    #[test]
    fn test_eval_multiple_expressions_returns_last_value() {
        let mut session = RSession::new();
        let result = session.eval("x <- c(10, 20, 30)\nx");
        assert_eq!(result.output, "[1] 10 20 30");
    }

    #[test]
    fn test_eval_integer_subassignment() {
        let mut session = RSession::new();
        let result = session.eval("x <- c(10, 20, 30)\nx[2] <- 99\nx");
        assert_eq!(result.output, "[1] 10 99 30");
    }

    #[test]
    fn test_eval_logical_subset_recycles_and_preserves_na() {
        let mut session = RSession::new();
        let recycled = session.eval("(1:5)[c(TRUE, FALSE)]");
        let with_na = session.eval("(1:3)[c(TRUE, NA)]");
        let longer = session.eval("(1:3)[c(TRUE, FALSE, TRUE, TRUE)]");

        assert_eq!(recycled.output, "[1] 1 3 5");
        assert_eq!(with_na.output, "[1]  1 NA  3");
        assert_eq!(longer.output, "[1]  1  3 NA");
    }

    #[test]
    fn test_eval_numeric_subset_preserves_na_and_negative_exclusion() {
        let mut session = RSession::new();
        let positive = session.eval("(1:3)[c(1, 0, NA, 4)]");
        let integer = session.eval("(1:3)[c(1L, 0L, NA_integer_, 4L)]");
        let negative = session.eval("(1:3)[-2]");

        assert_eq!(positive.output, "[1]  1 NA NA");
        assert_eq!(integer.output, "[1]  1 NA NA");
        assert_eq!(negative.output, "[1] 1 3");
    }

    #[test]
    fn test_eval_double_bracket_subset() {
        let mut session = RSession::new();
        let list_numeric = session.eval("list(a = 10, b = 20)[[2]]");
        let list_name = session.eval("list(a = 10, b = 20)[[\"b\"]]");
        let atomic = session.eval("c(10, 20, 30)[[2]]");

        assert_eq!(list_numeric.output, "[1] 20");
        assert_eq!(list_name.output, "[1] 20");
        assert_eq!(atomic.output, "[1] 20");
    }

    #[test]
    fn test_eval_sort_unique_rev() {
        let mut session = RSession::new();
        let sorted = session.eval("sort(c(3, 1, 2))");
        let sorted_desc = session.eval("sort(c(3, 1, 2), decreasing = TRUE)");
        let unique = session.eval("unique(c(1, 1, 2, 1))");
        let reversed = session.eval("rev(c(1, 2, 3))");

        assert_eq!(sorted.output, "[1] 1 2 3");
        assert_eq!(sorted_desc.output, "[1] 3 2 1");
        assert_eq!(unique.output, "[1] 1 2");
        assert_eq!(reversed.output, "[1] 3 2 1");
    }

    #[test]
    fn test_eval_match_in_set_ops_and_extrema_indices() {
        let mut session = RSession::new();
        let matched = session.eval("match(c(2, 4), c(1, 2, 3))");
        let contains = session.eval("c(2, 4) %in% c(1, 2, 3)");
        let union = session.eval("union(c(1, 2), c(2, 3))");
        let intersect = session.eval("intersect(c(1, 2), c(2, 3))");
        let setdiff = session.eval("setdiff(c(1, 2, 3), c(2, 4))");
        let setequal = session.eval("setequal(c(1, 2), c(2, 1, 1))");
        let which_min = session.eval("which.min(c(3, 1, 2))");
        let which_max = session.eval("which.max(c(3, 1, 2))");

        assert_eq!(matched.output, "[1]  2 NA");
        assert_eq!(contains.output, "[1]  TRUE FALSE");
        assert_eq!(union.output, "[1] 1 2 3");
        assert_eq!(intersect.output, "[1] 2");
        assert_eq!(setdiff.output, "[1] 1 3");
        assert_eq!(setequal.output, "[1] TRUE");
        assert_eq!(which_min.output, "[1] 2");
        assert_eq!(which_max.output, "[1] 1");
    }

    #[test]
    fn test_eval_matrix_sequence_and_logic_helpers() {
        let mut session = RSession::new();
        let any = session.eval("any(c(FALSE, TRUE))");
        let all = session.eval("all(c(TRUE, FALSE))");
        let matrix = session.eval("matrix(1:4, nrow = 2)");
        let dim = session.eval("dim(matrix(1:4, nrow = 2))");
        let nrow = session.eval("nrow(matrix(1:4, nrow = 2))");
        let ncol = session.eval("ncol(matrix(1:4, nrow = 2))");
        let diag = session.eval("diag(3)");
        let diff = session.eval("diff(c(1, 4, 9))");
        let names = session.eval("names(setNames(c(1, 2), c(\"a\", \"b\")))");
        let seq_len = session.eval("seq_len(3)");
        let seq_along = session.eval("seq_along(c(4, 5))");

        assert_eq!(any.output, "[1] TRUE");
        assert_eq!(all.output, "[1] FALSE");
        assert_eq!(
            matrix.output,
            "     [,1] [,2]\n[1,]    1    3\n[2,]    2    4"
        );
        assert_eq!(dim.output, "[1] 2 2");
        assert_eq!(nrow.output, "[1] 2");
        assert_eq!(ncol.output, "[1] 2");
        assert_eq!(
            diag.output,
            "     [,1] [,2] [,3]\n[1,]    1    0    0\n[2,]    0    1    0\n[3,]    0    0    1"
        );
        assert_eq!(diff.output, "[1] 3 5");
        assert_eq!(names.output, "[1] \"a\" \"b\"");
        assert_eq!(seq_len.output, "[1] 1 2 3");
        assert_eq!(seq_along.output, "[1] 1 2");
    }

    #[test]
    fn test_eval_environment_file_and_predicate_helpers() {
        let mut session = RSession::new();
        let missing = session.eval("exists(\"x\")");
        let assigned = session.eval("assign(\"y\", 2)\nget(\"y\")");
        let removed = session.eval("x <- 1\nrm(\"x\")\nexists(\"x\")");
        let tempdir_exists = session.eval("file.exists(tempdir())");
        let inherits = session.eval("inherits(1, \"numeric\")");
        let to_string = session.eval("toString(c(1, 2, 3))");
        let is_vector = session.eval("is.vector(c(1, 2))");
        let is_data_frame = session.eval("is.data.frame(data.frame(a = 1))");

        assert_eq!(missing.output, "[1] FALSE");
        assert_eq!(assigned.output, "[1] 2");
        assert_eq!(removed.output, "[1] FALSE");
        assert_eq!(tempdir_exists.output, "[1] TRUE");
        assert_eq!(inherits.output, "[1] TRUE");
        assert_eq!(to_string.output, "[1] \"1, 2, 3\"");
        assert_eq!(is_vector.output, "[1] TRUE");
        assert_eq!(is_data_frame.output, "[1] TRUE");
    }

    #[test]
    fn test_eval_registered_distribution_and_cumulative_helpers() {
        let mut session = RSession::new();
        let dnorm = session.eval("dnorm(0)");
        let pnorm = session.eval("pnorm(0)");
        let qnorm = session.eval("qnorm(0.5)");
        let dpois = session.eval("dpois(2, 3)");
        let dbinom = session.eval("dbinom(2, 5, 0.5)");
        let dgamma = session.eval("dgamma(2, 3)");
        let dcauchy = session.eval("dcauchy(0)");
        let cumsum = session.eval("cumsum(c(1, 2, 3))");
        let cumprod = session.eval("cumprod(c(1, 2, 3))");

        assert!((dnorm.value - 0.3989422804014327).abs() < 1e-12);
        assert_eq!(pnorm.output, "[1] 0.5");
        assert_eq!(qnorm.output, "[1] 0");
        assert!((dpois.value - 0.22404180765538775).abs() < 1e-12);
        assert!((dbinom.value - 0.3125).abs() < 1e-12);
        assert!((dgamma.value - 0.2706705664732254).abs() < 1e-12);
        assert!((dcauchy.value - 0.3183098861837907).abs() < 1e-12);
        assert_eq!(cumsum.output, "[1] 1 3 6");
        assert_eq!(cumprod.output, "[1] 1 2 6");
    }

    #[test]
    fn test_eval_normal_helpers_respect_tail_and_log_flags() {
        let mut session = RSession::new();

        let dnorm_log = session.eval("dnorm(0, 0, 1, TRUE)");
        let pnorm_upper = session.eval("pnorm(1, 0, 1, FALSE, FALSE)");
        let pnorm_log = session.eval("pnorm(1, 0, 1, TRUE, TRUE)");
        let qnorm_upper = session.eval("qnorm(0.25, 0, 1, FALSE, FALSE)");
        let qnorm_log = session.eval("qnorm(log(0.25), 0, 1, TRUE, TRUE)");

        assert!((dnorm_log.value - session.dnorm(0.0, 0.0, 1.0, true)).abs() < 1e-12);
        assert!((pnorm_upper.value - session.pnorm(1.0, 0.0, 1.0, false, false)).abs() < 1e-12);
        assert!((pnorm_log.value - session.pnorm(1.0, 0.0, 1.0, true, true)).abs() < 1e-12);
        assert!((qnorm_upper.value - session.qnorm(0.25, 0.0, 1.0, false, false)).abs() < 1e-12);
        assert!(
            (qnorm_log.value - session.qnorm(0.25_f64.ln(), 0.0, 1.0, true, true)).abs() < 1e-12
        );
    }

    #[test]
    fn test_eval_raw_string_helpers() {
        let mut session = RSession::new();
        let as_raw = session.eval("as.raw(c(65, 90))");
        let char_to_raw = session.eval("charToRaw(\"AZ\")");
        let raw_to_char = session.eval("rawToChar(as.raw(c(65, 90)))");

        assert_eq!(as_raw.output, "[1] 41 5a");
        assert_eq!(char_to_raw.output, "[1] 41 5a");
        assert_eq!(raw_to_char.output, "[1] \"AZ\"");
    }

    #[test]
    fn test_eval_replacement_and_order_predicate_helpers() {
        let mut session = RSession::new();
        let names = session.eval("x <- c(1, 2)\nnames(x) <- c(\"a\", \"b\")\nnames(x)");
        let class = session.eval("x <- 1\nclass(x) <- \"foo\"\nclass(x)");
        let unsorted = session.eval("is.unsorted(c(1, 3, 2))");
        let sorted = session.eval("is.unsorted(c(1, 2, 3))");

        assert_eq!(names.output, "[1] \"a\" \"b\"");
        assert_eq!(class.output, "[1] \"foo\"");
        assert_eq!(unsorted.output, "[1] TRUE");
        assert_eq!(sorted.output, "[1] FALSE");
    }

    #[test]
    fn test_eval_additional_distribution_helpers() {
        let mut session = RSession::new();
        let cases = [
            ("dexp(1)", "[1] 0.3678794"),
            ("pexp(1)", "[1] 0.6321206"),
            ("dbeta(0.5, 2, 3)", "[1] 1.5"),
            ("pbeta(0.5, 2, 3)", "[1] 0.6875"),
            ("qbeta(0.5, 2, 3)", "[1] 0.3857276"),
            ("dt(0, 5)", "[1] 0.3796067"),
            ("pt(0, 5)", "[1] 0.5"),
            ("qt(0.5, 5)", "[1] 0"),
            ("dchisq(2, 3)", "[1] 0.2075537"),
            ("pchisq(2, 3)", "[1] 0.4275933"),
            ("qchisq(0.5, 3)", "[1] 2.365974"),
            ("dweibull(2, 3)", "[1] 0.004025552"),
            ("pweibull(2, 3)", "[1] 0.9996645"),
            ("qweibull(0.5, 3)", "[1] 0.884997"),
            ("df(1, 5, 10)", "[1] 0.4954798"),
            ("pf(1, 5, 10)", "[1] 0.5348806"),
            ("qf(0.5, 5, 10)", "[1] 0.9319332"),
            ("dnbinom(2, 5, 0.5)", "[1] 0.1171875"),
            ("pnbinom(2, 5, 0.5)", "[1] 0.2265625"),
            ("qnbinom(0.5, 5, 0.5)", "[1] 4"),
            ("dgeom(2, 0.5)", "[1] 0.125"),
            ("pgeom(2, 0.5)", "[1] 0.875"),
            ("qgeom(0.5, 0.5)", "[1] 0"),
        ];

        for (code, expected) in cases {
            assert_eq!(session.eval(code).output, expected, "{code}");
        }
    }

    #[test]
    fn test_eval_named_vector_names() {
        let mut session = RSession::new();
        let result = session.eval("names(c(a = 1, b = 2))");
        assert_eq!(result.output, "[1] \"a\" \"b\"");
    }

    #[test]
    fn test_eval_lapply_prints_list() {
        let mut session = RSession::new();
        let result = session.eval("lapply(c(1, 2), function(x) x + 1)");
        assert_eq!(result.output, "[[1]]\n[1] 2\n\n[[2]]\n[1] 3");
    }

    #[test]
    fn test_eval_dollar_list_access() {
        let mut session = RSession::new();
        let exact = session.eval("x <- list(a = 1, b = 2)\nx$a");
        let partial = session.eval("x <- list(alpha = 11, beta = 22)\nx$al");
        let missing = session.eval("x <- list(a = 1)\nx$b");
        assert_eq!(exact.output, "[1] 1");
        assert_eq!(partial.output, "[1] 11");
        assert_eq!(missing.output, "NULL");
    }

    #[test]
    fn test_eval_mean_numeric() {
        let mut session = RSession::new();
        let numeric = session.eval("mean(c(1, 2, 3))");
        let sequence = session.eval("mean(1:4)");
        let na = session.eval("mean(c(1, NA))");
        let na_removed = session.eval("mean(c(1, NA), na.rm = TRUE)");
        assert_eq!(numeric.output, "[1] 2");
        assert_eq!(sequence.output, "[1] 2.5");
        assert_eq!(na.output, "[1] NA");
        assert_eq!(na_removed.output, "[1] 1");
    }

    #[test]
    fn test_eval_summary_na_rm() {
        let mut session = RSession::new();
        let sum = session.eval("sum(c(1, NA, 3), na.rm = TRUE)");
        let min = session.eval("min(c(3, NA, 1), na.rm = TRUE)");
        let range = session.eval("range(c(3, NA, 1), na.rm = TRUE)");
        assert_eq!(sum.output, "[1] 4");
        assert_eq!(min.output, "[1] 1");
        assert_eq!(range.output, "[1] 1 3");
    }

    #[test]
    fn test_eval_factor_labels() {
        let mut session = RSession::new();
        let result = session.eval("x <- factor(c(\"b\", \"a\", \"b\", \"c\"))\nx");
        assert_eq!(result.output, "[1] b a b c\nLevels: a b c");
    }

    #[test]
    fn test_eval_closure_positional_arg() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x) x + 1\nf(41)");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_closure_lexical_scope() {
        let mut session = RSession::new();
        let result =
            session.eval("make <- function(x) function(y) x + y\nadd2 <- make(2)\nadd2(40)");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_closure_default_arg() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x, y = x + 1) y\nf(41)");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_closure_lazy_unused_arg() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x) 1\nf(unknown_symbol)");
        assert_eq!(result.output, "[1] 1");
    }

    #[test]
    fn test_eval_closure_named_args() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x, y) x + y\nf(y = 40, x = 2)");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_closure_return() {
        let mut session = RSession::new();
        let result = session.eval("f <- function() return(42)\nf()");
        assert_eq!(result.output, "[1] 42");
    }

    #[test]
    fn test_eval_missing_arg() {
        let mut session = RSession::new();
        let missing = session.eval("f <- function(x) missing(x)\nf()");
        let present = session.eval("f <- function(x) missing(x)\nf(1)");
        assert_eq!(missing.output, "[1] TRUE");
        assert_eq!(present.output, "[1] FALSE");
    }

    #[test]
    fn test_eval_missing_arg_error() {
        let mut session = RSession::new();
        let result = session.eval("f <- function(x) x\nf()");
        assert_eq!(
            result.output,
            "Error: argument \"x\" is missing, with no default"
        );
    }

    #[test]
    fn test_sessions_keep_globals_isolated_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        assert!(!left.eval("x <- 11").output.contains("Error"));
        assert!(!right.eval("x <- 29").output.contains("Error"));

        let left_x = left.eval("x");
        let right_x = right.eval("x");

        assert!((left_x.value - 11.0).abs() < 1e-10, "left: {left_x:?}");
        assert!((right_x.value - 29.0).abs() < 1e-10, "right: {right_x:?}");
    }

    #[test]
    fn test_parallel_sessions_keep_globals_isolated() {
        let left = std::thread::spawn(|| {
            let mut session = RSession::new();
            assert!(!session.eval("x <- 101").output.contains("Error"));
            session.eval("x").value
        });
        let right = std::thread::spawn(|| {
            let mut session = RSession::new();
            assert!(!session.eval("x <- 202").output.contains("Error"));
            session.eval("x").value
        });

        assert!((left.join().expect("left session panicked") - 101.0).abs() < 1e-10);
        assert!((right.join().expect("right session panicked") - 202.0).abs() < 1e-10);
    }

    #[test]
    fn test_parallel_android_sessions_stress_isolated_state_paths_and_cancellation() {
        const WORKERS: usize = 4;
        const ITERS: usize = 8;

        let handles = (0..WORKERS)
            .map(|worker| {
                std::thread::spawn(move || {
                    let root = unique_test_root(&format!("android-parallel-{worker}"));
                    let files = root.join("files");
                    let cache = root.join("cache");
                    let bundled = root.join("bundled-library");
                    let package = format!("pkg{worker}");
                    let package_dir = bundled.join(&package);
                    std::fs::create_dir_all(&package_dir).expect("package dir");
                    std::fs::write(
                        package_dir.join("DESCRIPTION"),
                        format!("Package: {package}\nVersion: 0.0.1\n"),
                    )
                    .expect("description");

                    let mut session = RSession::new();
                    session
                        .configure_paths(
                            files.to_str().expect("utf8 files path"),
                            cache.to_str().expect("utf8 cache path"),
                            Some(bundled.to_str().expect("utf8 bundled path")),
                        )
                        .expect("configure paths");
                    session.set_seed(100 + worker as u32, 200 + worker as u32);

                    let mut rng_bits = Vec::with_capacity(ITERS);
                    for iter in 0..ITERS {
                        let cancelled = CancellationToken::cancelled();
                        let cancelled_result =
                            session.eval_with_cancellation_token("1 + 1", Some(cancelled));
                        assert_eq!(cancelled_result.output, "Error: operation cancelled");

                        let code = format!("local_value <- {}; local_value", worker * 100 + iter);
                        let result = session.eval(&code);
                        assert_eq!(result.value, (worker * 100 + iter) as f64, "{result:?}");
                        assert_eq!(
                            session.eval("exists(\"local_value\")").typed,
                            RValue::Logical(Some(true))
                        );
                        rng_bits.push(session.unif_rand().to_bits());
                    }

                    assert!(session.package_available(&package));
                    assert_eq!(
                        session.package_path(&package),
                        Some(package_dir.to_string_lossy().into_owned())
                    );
                    let temp_dir = cache.join("Rtmp").to_string_lossy().into_owned();
                    assert_eq!(
                        session.eval("tempdir()").typed,
                        string_vector(vec![temp_dir.clone()])
                    );
                    assert_eq!(session.eval("1 + 1").output, "[1] 2");

                    let _ = std::fs::remove_dir_all(root);
                    (worker, rng_bits, temp_dir)
                })
            })
            .collect::<Vec<_>>();

        let summaries = handles
            .into_iter()
            .map(|handle| handle.join().expect("parallel session worker panicked"))
            .collect::<Vec<_>>();

        assert_eq!(summaries.len(), WORKERS);
        let temp_dirs = summaries
            .iter()
            .map(|(_, _, temp_dir)| temp_dir.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(temp_dirs.len(), WORKERS);

        let rng_sequences = summaries
            .iter()
            .map(|(_, rng_bits, _)| rng_bits.as_slice())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(rng_sequences.len(), WORKERS);
    }

    #[test]
    fn test_eval_cancellation_token_is_scoped_to_session_call() {
        let mut cancelled = RSession::new();
        let mut active = RSession::new();
        let flag = CancellationToken::cancelled();

        let cancelled_result = cancelled.eval_with_cancellation_token("1 + 1", Some(flag));
        assert_eq!(cancelled_result.output, "Error: operation cancelled");

        let active_result = active.eval("1 + 1");
        assert_eq!(active_result.output, "[1] 2");

        let next_cancelled_eval = cancelled.eval("1 + 1");
        assert_eq!(next_cancelled_eval.output, "[1] 2");
    }

    #[test]
    fn test_sessions_keep_rng_isolated_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let left_first = left.unif_rand();
        let right_first = right.unif_rand();
        let left_second = left.unif_rand();
        let right_second = right.unif_rand();

        assert_eq!(left_first, right_first);
        assert_eq!(left_second, right_second);
        assert_ne!(left_first, left_second);
    }

    #[test]
    fn test_session_seed_is_local() {
        let left = RSession::new();
        let right = RSession::new();

        left.set_seed(10, 20);
        right.set_seed(10, 20);
        assert_eq!(left.unif_rand(), right.unif_rand());

        left.set_seed(30, 40);
        assert_ne!(left.unif_rand(), right.unif_rand());
    }

    #[test]
    fn test_eval_null() {
        let mut session = RSession::new();
        let result = session.eval("NULL");
        assert_eq!(result.output, "NULL");
    }

    #[test]
    fn test_eval_unary_minus() {
        let mut session = RSession::new();
        let result = session.eval("-5");
        assert!(
            (result.value - (-5.0)).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_complex_expr() {
        let mut session = RSession::new();
        let result = session.eval("(1 + 2) * (3 + 4)");
        assert!(
            (result.value - 21.0).abs() < 1e-10,
            "value: {}",
            result.value
        );
    }

    #[test]
    fn test_eval_true_false() {
        let mut session = RSession::new();
        let t = session.eval("TRUE");
        assert!((t.value - 1.0).abs() < 1e-10);

        let f = session.eval("FALSE");
        assert!((f.value - 0.0).abs() < 1e-10);
    }

    fn adversarial_iterations(default: u64) -> u64 {
        std::env::var("RPORT_ADVERSARIAL_ITERS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }

    #[test]
    fn adversarial_eval_errors_and_subsets_do_not_panic() {
        let fixed = [
            "stop('intentional')",
            "f <- function(x) x; f()",
            "c(1, 2, 3)[[10]]",
            "c(1, 2, 3)[c(1, -2)]",
            "list(a = 1)$missing$value",
            "if (c(TRUE, FALSE)) 1 else 2",
            "while (TRUE) { stop('bounded error') }",
        ];

        for code in fixed {
            let mut session = RSession::new();
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.eval(code)));
            assert!(result.is_ok(), "eval panicked for fixed input: {code:?}");
        }

        for seed in 0..adversarial_iterations(128).min(512) {
            let mut session = RSession::new();
            let a = (seed % 7) as i32 - 2;
            let b = ((seed / 7) % 7) as i32 - 2;
            let c = ((seed / 49) % 7) as i32 - 2;
            let code = format!("c(10, 20, 30)[c({a}, {b}, {c})]");
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.eval(&code)));
            assert!(
                result.is_ok(),
                "subset eval panicked for seed {seed}: {code}"
            );
        }
    }

    #[test]
    fn adversarial_owned_value_conversion_handles_generated_vectors() {
        for seed in 0..adversarial_iterations(32).min(128) {
            let mut session = RSession::new();
            let len = (seed % 8) + 1;
            let values = (0..len)
                .map(|idx| ((seed + idx * 13) % 17).to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let numeric = session.eval(&format!("c({values})"));
            match numeric.typed {
                RValue::RealVector(items) => assert_eq!(items.len(), len as usize),
                RValue::Real(_) if len == 1 => {}
                ref other => panic!("expected real vector for seed {seed}, got {other:?}"),
            }

            let strings = (0..len)
                .map(|idx| format!("\"s{seed}_{idx}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let character = session.eval(&format!("c({strings})"));
            match character.typed {
                RValue::StringVector(items) => assert_eq!(items.len(), len as usize),
                ref other => panic!("expected string vector for seed {seed}, got {other:?}"),
            }
        }
    }
}
