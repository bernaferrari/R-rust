//! r-uniffi: UniFFI bindings for the R interpreter.
//!
//! This crate provides high-level, safe bindings to the R interpreter
//! for use from Kotlin (Android), Swift (iOS), and Python.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::thread;

use r_embed::CancellationToken;
use uniffi::Object;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum RError {
    #[error("Failed to initialize R session: {0}")]
    InitFailed(String),
    #[error("Evaluation error: {0}")]
    EvalError(String),
    #[error("Render error: {0}")]
    RenderError(String),
    #[error("Session is already closed")]
    SessionClosed,
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error("Operation cancelled")]
    Cancelled,
    #[error("Session busy: {0}")]
    SessionBusy(String),
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProgressUpdate {
    pub progress: f64,
    pub message: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PlotResult {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RValueKind {
    Null,
    Logical,
    Integer,
    Real,
    LogicalVector,
    IntegerVector,
    RealVector,
    StringVector,
    RawVector,
    ComplexVector,
    List,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct RComplexValue {
    pub real: f64,
    pub imaginary: f64,
}

impl From<r_embed::RComplexValue> for RComplexValue {
    fn from(value: r_embed::RComplexValue) -> Self {
        RComplexValue {
            real: value.real,
            imaginary: value.imaginary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RValue {
    pub kind: RValueKind,
    pub logical_scalar: Option<bool>,
    pub integer_scalar: Option<i32>,
    pub real_scalar: Option<f64>,
    pub logical_values: Vec<Option<bool>>,
    pub integer_values: Vec<Option<i32>>,
    pub real_values: Vec<Option<f64>>,
    pub string_values: Vec<Option<String>>,
    pub raw_values: Vec<u8>,
    pub complex_values: Vec<Option<RComplexValue>>,
    pub list_values: Vec<RValue>,
    pub type_name: String,
    pub error: String,
    pub metadata: RMetadata,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RAttribute {
    pub name: String,
    pub value: RValue,
}

#[derive(Debug, Clone, PartialEq, Default, uniffi::Record)]
pub struct RMetadata {
    pub names: Option<Vec<Option<String>>>,
    pub dim: Option<Vec<i32>>,
    pub class: Option<Vec<Option<String>>>,
    pub levels: Option<Vec<Option<String>>>,
    pub attributes: Vec<RAttribute>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct EvalResult {
    pub output: String,
    pub value: RValue,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeInfo {
    pub is_active: bool,
    pub library_paths: Vec<String>,
    pub temp_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AndroidRuntimePaths {
    pub app_files_dir: String,
    pub cache_dir: String,
    pub bundled_library_dir: Option<String>,
    pub user_library_dir: String,
    pub temp_dir: String,
    pub library_paths: Vec<String>,
}

impl From<r_embed::AndroidRuntimePaths> for AndroidRuntimePaths {
    fn from(paths: r_embed::AndroidRuntimePaths) -> Self {
        AndroidRuntimePaths {
            app_files_dir: paths.app_files_dir.clone(),
            cache_dir: paths.cache_dir.clone(),
            bundled_library_dir: paths.bundled_library_dir.clone(),
            user_library_dir: paths.user_library_dir(),
            temp_dir: paths.temp_dir(),
            library_paths: paths.library_paths(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub path: String,
    pub library_path: String,
}

impl From<r_embed::RPackageInfo> for PackageInfo {
    fn from(info: r_embed::RPackageInfo) -> Self {
        PackageInfo {
            name: info.name,
            version: info.version,
            path: info.path,
            library_path: info.library_path,
        }
    }
}

fn empty_value(kind: RValueKind) -> RValue {
    RValue {
        kind,
        logical_scalar: None,
        integer_scalar: None,
        real_scalar: None,
        logical_values: Vec::new(),
        integer_values: Vec::new(),
        real_values: Vec::new(),
        string_values: Vec::new(),
        raw_values: Vec::new(),
        complex_values: Vec::new(),
        list_values: Vec::new(),
        type_name: String::new(),
        error: String::new(),
        metadata: RMetadata::default(),
    }
}

impl From<r_embed::RValue> for RValue {
    fn from(value: r_embed::RValue) -> Self {
        match value {
            r_embed::RValue::Null => empty_value(RValueKind::Null),
            r_embed::RValue::Logical(value) => RValue {
                logical_scalar: value,
                ..empty_value(RValueKind::Logical)
            },
            r_embed::RValue::Integer(value) => RValue {
                integer_scalar: value,
                ..empty_value(RValueKind::Integer)
            },
            r_embed::RValue::Real(value) => RValue {
                real_scalar: value,
                ..empty_value(RValueKind::Real)
            },
            r_embed::RValue::LogicalVector(values) => RValue {
                logical_values: values,
                ..empty_value(RValueKind::LogicalVector)
            },
            r_embed::RValue::IntegerVector(values) => RValue {
                integer_values: values,
                ..empty_value(RValueKind::IntegerVector)
            },
            r_embed::RValue::RealVector(values) => RValue {
                real_values: values,
                ..empty_value(RValueKind::RealVector)
            },
            r_embed::RValue::StringVector(values) => RValue {
                string_values: values,
                ..empty_value(RValueKind::StringVector)
            },
            r_embed::RValue::RawVector(values) => RValue {
                raw_values: values,
                ..empty_value(RValueKind::RawVector)
            },
            r_embed::RValue::ComplexVector(values) => RValue {
                complex_values: values
                    .into_iter()
                    .map(|value| value.map(RComplexValue::from))
                    .collect(),
                ..empty_value(RValueKind::ComplexVector)
            },
            r_embed::RValue::List(values) => RValue {
                list_values: values.into_iter().map(RValue::from).collect(),
                ..empty_value(RValueKind::List)
            },
            r_embed::RValue::Attributed { value, metadata } => {
                let mut value = RValue::from(*value);
                value.metadata = RMetadata::from(metadata);
                value
            }
            r_embed::RValue::Unsupported { type_name } => RValue {
                type_name,
                ..empty_value(RValueKind::Unsupported)
            },
            r_embed::RValue::Error(message) => RValue {
                error: message,
                ..empty_value(RValueKind::Error)
            },
        }
    }
}

impl From<r_embed::RAttribute> for RAttribute {
    fn from(attribute: r_embed::RAttribute) -> Self {
        RAttribute {
            name: attribute.name,
            value: RValue::from(attribute.value),
        }
    }
}

impl From<r_embed::RMetadata> for RMetadata {
    fn from(metadata: r_embed::RMetadata) -> Self {
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

// ---------------------------------------------------------------------------
// Callback interface
// ---------------------------------------------------------------------------

#[uniffi::export(callback_interface)]
pub trait SessionCallback: Send + Sync + 'static {
    fn on_progress(&self, update: ProgressUpdate);
    fn on_output(&self, line: String);
    fn on_plot_ready(&self, plot: PlotResult);
    fn on_eval_complete(&self, result: EvalResult);
    fn on_error(&self, error: String);
}

// ---------------------------------------------------------------------------
// Internal commands
// ---------------------------------------------------------------------------

enum SessionCommand {
    ConfigurePaths {
        paths: AndroidRuntimePaths,
        reply: Sender<Result<(), RError>>,
    },
    RuntimeInfo {
        reply: Sender<Result<RuntimeInfo, RError>>,
    },
    PackageAvailable {
        package: String,
        reply: Sender<Result<bool, RError>>,
    },
    PackagePath {
        package: String,
        reply: Sender<Result<Option<String>, RError>>,
    },
    PackageInfo {
        package: String,
        reply: Sender<Result<Option<PackageInfo>, RError>>,
    },
    InstalledPackages {
        reply: Sender<Result<Vec<PackageInfo>, RError>>,
    },
    LoadPackage {
        package: String,
        reply: Sender<Result<(), RError>>,
    },
    Eval {
        code: String,
        reply: Sender<Result<EvalResult, RError>>,
    },
    Render {
        code: String,
        width: u32,
        height: u32,
        reply: Sender<Result<PlotResult, RError>>,
    },
    Shutdown,
}

// ---------------------------------------------------------------------------
// RSession object
// ---------------------------------------------------------------------------

#[derive(Object)]
pub struct RSession {
    cmd_tx: Mutex<Option<Sender<SessionCommand>>>,
    cancelled: CancellationToken,
    callback: Arc<Mutex<Option<Arc<dyn SessionCallback>>>>,
    operation_id: AtomicU64,
}

fn current_callback(
    callback: &Arc<Mutex<Option<Arc<dyn SessionCallback>>>>,
) -> Option<Arc<dyn SessionCallback>> {
    callback.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn validate_package_name(package: &str) -> Result<(), RError> {
    if package.trim().is_empty() {
        return Err(RError::InvalidInput("package name is empty".to_string()));
    }
    Ok(())
}

fn validate_plot_dimensions(width: u32, height: u32) -> Result<(), RError> {
    if width == 0 || height == 0 {
        return Err(RError::InvalidInput(
            "plot width and height must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

fn spawn_worker(
    cmd_rx: Receiver<SessionCommand>,
    callback: Arc<Mutex<Option<Arc<dyn SessionCallback>>>>,
    cancelled: CancellationToken,
) {
    thread::spawn(move || {
        let mut session = match r_embed::RSession::new() {
            Ok(s) => s,
            Err(e) => {
                if let Some(cb) = current_callback(&callback) {
                    cb.on_error(format!("init failed: {e}"));
                }
                return;
            }
        };

        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                SessionCommand::ConfigurePaths { paths, reply } => {
                    let embed_paths = r_embed::AndroidRuntimePaths::new(
                        paths.app_files_dir,
                        paths.cache_dir,
                        paths.bundled_library_dir,
                    );
                    let result = session
                        .configure_android_runtime(&embed_paths)
                        .map_err(|err| RError::InitFailed(err.to_string()));
                    let _ = reply.send(result);
                }
                SessionCommand::RuntimeInfo { reply } => {
                    let info = session.runtime_info();
                    let _ = reply.send(Ok(RuntimeInfo {
                        is_active: info.is_active,
                        library_paths: info.library_paths,
                        temp_dir: info.temp_dir,
                    }));
                }
                SessionCommand::PackageAvailable { package, reply } => {
                    let _ = reply.send(Ok(session.package_available(&package)));
                }
                SessionCommand::PackagePath { package, reply } => {
                    let _ = reply.send(Ok(session.package_path(&package)));
                }
                SessionCommand::PackageInfo { package, reply } => {
                    let _ = reply.send(Ok(session.package_info(&package).map(PackageInfo::from)));
                }
                SessionCommand::InstalledPackages { reply } => {
                    let packages = session
                        .installed_packages()
                        .into_iter()
                        .map(PackageInfo::from)
                        .collect();
                    let _ = reply.send(Ok(packages));
                }
                SessionCommand::LoadPackage { package, reply } => {
                    let result = session
                        .load_package(&package)
                        .map_err(|err| RError::EvalError(err.to_string()));
                    let _ = reply.send(result);
                }
                SessionCommand::Eval { code, reply } => {
                    let result = session
                        .eval_result_cancellable(&code, &cancelled)
                        .map(|result| EvalResult {
                            output: result.output,
                            value: RValue::from(result.value),
                        })
                        .map_err(|e| {
                            if e.to_string().contains("operation cancelled") {
                                RError::Cancelled
                            } else {
                                RError::EvalError(e.to_string())
                            }
                        });
                    cancelled.reset();

                    if let Some(cb) = current_callback(&callback) {
                        match &result {
                            Ok(result) => cb.on_eval_complete(result.clone()),
                            Err(e) => cb.on_error(e.to_string()),
                        }
                    }

                    let _ = reply.send(result);
                }
                SessionCommand::Render {
                    code,
                    width,
                    height,
                    reply,
                } => {
                    let result = session
                        .render_with_dimensions(&code, width, height)
                        .map(|pixels| PlotResult {
                            width,
                            height,
                            pixels,
                        })
                        .map_err(|e| RError::RenderError(e.to_string()));

                    if let Some(cb) = current_callback(&callback)
                        && let Ok(plot) = &result
                    {
                        cb.on_plot_ready(plot.clone());
                    }

                    let _ = reply.send(result);
                }
                SessionCommand::Shutdown => {
                    session.close();
                    break;
                }
            }
        }
    });
}

impl RSession {
    fn request<T>(
        &self,
        command: impl FnOnce(Sender<Result<T, RError>>) -> SessionCommand,
    ) -> Result<T, RError> {
        let tx = self.cmd_tx.lock().unwrap_or_else(|e| e.into_inner());
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, reply_rx) = channel();
        tx.send(command(reply_tx))
            .map_err(|_| RError::SessionClosed)?;
        reply_rx.recv().map_err(|_| RError::SessionClosed)?
    }
}

#[uniffi::export]
impl RSession {
    #[uniffi::constructor]
    pub fn new() -> Result<Self, RError> {
        let (cmd_tx, cmd_rx) = channel();
        let cancelled = CancellationToken::new();

        let callback = Arc::new(Mutex::new(None));

        spawn_worker(cmd_rx, callback.clone(), cancelled.clone());

        Ok(Self {
            cmd_tx: Mutex::new(Some(cmd_tx)),
            cancelled,
            callback,
            operation_id: AtomicU64::new(0),
        })
    }

    pub fn set_callback(&self, callback: Box<dyn SessionCallback>) {
        *self.callback.lock().unwrap_or_else(|e| e.into_inner()) = Some(callback.into());
    }

    pub fn is_active(&self) -> bool {
        self.cmd_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    pub fn eval(&self, code: String) -> Result<String, RError> {
        self.eval_result(code).map(|result| result.output)
    }

    pub fn eval_result(&self, code: String) -> Result<EvalResult, RError> {
        self.cancelled.reset();
        self.request(|reply| SessionCommand::Eval { code, reply })
    }

    pub fn configure_android_paths(
        &self,
        app_files_dir: String,
        cache_dir: String,
        bundled_library_dir: Option<String>,
    ) -> Result<(), RError> {
        self.configure_android_runtime(android_runtime_paths(
            app_files_dir,
            cache_dir,
            bundled_library_dir,
        ))
    }

    pub fn configure_android_runtime(&self, paths: AndroidRuntimePaths) -> Result<(), RError> {
        self.request(|reply| SessionCommand::ConfigurePaths { paths, reply })
    }

    pub fn runtime_info(&self) -> Result<RuntimeInfo, RError> {
        self.request(|reply| SessionCommand::RuntimeInfo { reply })
    }

    pub fn package_available(&self, package: String) -> Result<bool, RError> {
        validate_package_name(&package)?;
        self.request(|reply| SessionCommand::PackageAvailable { package, reply })
    }

    pub fn package_path(&self, package: String) -> Result<Option<String>, RError> {
        validate_package_name(&package)?;
        self.request(|reply| SessionCommand::PackagePath { package, reply })
    }

    pub fn package_info(&self, package: String) -> Result<Option<PackageInfo>, RError> {
        validate_package_name(&package)?;
        self.request(|reply| SessionCommand::PackageInfo { package, reply })
    }

    pub fn installed_packages(&self) -> Result<Vec<PackageInfo>, RError> {
        self.request(|reply| SessionCommand::InstalledPackages { reply })
    }

    pub fn load_package(&self, package: String) -> Result<(), RError> {
        validate_package_name(&package)?;
        self.request(|reply| SessionCommand::LoadPackage { package, reply })
    }

    pub fn eval_async(&self, code: String) -> Result<u64, RError> {
        let tx = self.cmd_tx.lock().unwrap_or_else(|e| e.into_inner());
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, _) = channel();
        let op_id = self.operation_id.fetch_add(1, Ordering::Relaxed);
        self.cancelled.reset();
        tx.send(SessionCommand::Eval {
            code,
            reply: reply_tx,
        })
        .map_err(|_| RError::SessionClosed)?;
        Ok(op_id)
    }

    pub fn render(&self, code: String, width: u32, height: u32) -> Result<PlotResult, RError> {
        validate_plot_dimensions(width, height)?;
        self.request(|reply| SessionCommand::Render {
            code,
            width,
            height,
            reply,
        })
    }

    pub fn render_async(&self, code: String, width: u32, height: u32) -> Result<u64, RError> {
        validate_plot_dimensions(width, height)?;
        let tx = self.cmd_tx.lock().unwrap_or_else(|e| e.into_inner());
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, _) = channel();
        let op_id = self.operation_id.fetch_add(1, Ordering::Relaxed);
        tx.send(SessionCommand::Render {
            code,
            width,
            height,
            reply: reply_tx,
        })
        .map_err(|_| RError::SessionClosed)?;
        Ok(op_id)
    }

    pub fn cancel(&self) {
        self.cancelled.cancel();
    }

    pub fn cancel_current_operation(&self) {
        self.cancel();
    }

    pub fn close(&self) {
        self.destroy();
    }

    pub fn destroy(&self) {
        self.cancel();
        let mut tx_guard = self.cmd_tx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = tx_guard.take() {
            let _ = tx.send(SessionCommand::Shutdown);
        }
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        self.destroy();
    }
}

#[uniffi::export]
pub fn android_runtime_paths(
    app_files_dir: String,
    cache_dir: String,
    bundled_library_dir: Option<String>,
) -> AndroidRuntimePaths {
    r_embed::AndroidRuntimePaths::new(app_files_dir, cache_dir, bundled_library_dir).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    fn make_test_package(root_name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "{root_name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        let bundled = root.join("bundled-library");
        let pkg = bundled.join("tiny");
        let r_dir = pkg.join("R");
        std::fs::create_dir_all(&r_dir).expect("package R dir");
        std::fs::write(pkg.join("DESCRIPTION"), "Package: tiny\nVersion: 0.0.1\n")
            .expect("description");
        std::fs::write(r_dir.join("tiny.R"), "tiny_value <- function() 42L\n").expect("R source");
        (root, pkg)
    }

    #[test]
    fn cancel_without_active_eval_does_not_poison_next_eval() {
        let session = RSession::new().expect("session");

        session.cancel_current_operation();

        assert_eq!(session.eval("1 + 1".to_string()).unwrap(), "[1] 2");
    }

    #[test]
    fn lifecycle_aliases_close_session() {
        let session = RSession::new().expect("session");

        assert!(session.is_active());
        session.close();
        assert!(!session.is_active());
        assert!(matches!(
            session.eval("1 + 1".to_string()),
            Err(RError::SessionClosed)
        ));
    }

    #[test]
    fn validates_android_facing_inputs() {
        let session = RSession::new().expect("session");

        assert!(matches!(
            session.package_available("   ".to_string()),
            Err(RError::InvalidInput(message)) if message.contains("package")
        ));
        assert!(matches!(
            session.render("plot(c(1), c(1))".to_string(), 0, 120),
            Err(RError::InvalidInput(message)) if message.contains("width")
        ));
    }

    #[test]
    fn eval_result_returns_owned_value() {
        let session = RSession::new().expect("session");

        let result = session.eval_result("1:3".to_string()).expect("eval");

        assert_eq!(result.output, "[1] 1 2 3");
        assert_eq!(result.value.kind, RValueKind::IntegerVector);
        assert_eq!(result.value.integer_values, vec![Some(1), Some(2), Some(3)]);

        let strings = session
            .eval_result("c(\"a\", NA_character_)".to_string())
            .expect("eval strings");
        assert_eq!(strings.value.kind, RValueKind::StringVector);
        assert_eq!(
            strings.value.string_values,
            vec![Some("a".to_string()), None]
        );
    }

    #[test]
    fn eval_result_preserves_value_metadata() {
        let value = RValue::from(r_embed::RValue::Attributed {
            value: Box::new(r_embed::RValue::IntegerVector(vec![Some(1), Some(2)])),
            metadata: r_embed::RMetadata {
                names: Some(vec![Some("a".to_string()), Some("b".to_string())]),
                class: Some(vec![Some("foo".to_string())]),
                ..r_embed::RMetadata::default()
            },
        });

        assert_eq!(value.kind, RValueKind::IntegerVector);
        assert_eq!(value.integer_values, vec![Some(1), Some(2)]);
        assert_eq!(
            value.metadata.names,
            Some(vec![Some("a".to_string()), Some("b".to_string())])
        );
        assert_eq!(value.metadata.class, Some(vec![Some("foo".to_string())]));
    }

    #[test]
    fn eval_result_preserves_raw_and_complex_values() {
        let session = RSession::new().expect("session");

        let raw = session
            .eval_result("as.raw(c(65, 90))".to_string())
            .expect("eval");
        assert_eq!(raw.value.kind, RValueKind::RawVector);
        assert_eq!(raw.value.raw_values, vec![0x41, 0x5a]);

        let complex = RValue::from(r_embed::RValue::ComplexVector(vec![
            Some(r_embed::RComplexValue {
                real: 1.0,
                imaginary: -2.0,
            }),
            None,
        ]));
        assert_eq!(complex.kind, RValueKind::ComplexVector);
        assert_eq!(
            complex.complex_values,
            vec![
                Some(RComplexValue {
                    real: 1.0,
                    imaginary: -2.0,
                }),
                None,
            ]
        );
    }

    #[test]
    fn unsupported_values_carry_type_name_only() {
        let value = RValue::from(r_embed::RValue::Unsupported {
            type_name: "closure".to_string(),
        });

        assert_eq!(value.kind, RValueKind::Unsupported);
        assert_eq!(value.type_name, "closure");
    }

    #[test]
    fn configure_android_paths_runs_on_worker_session() {
        let root = std::env::temp_dir().join(format!(
            "rport-uniffi-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");
        let session = RSession::new().expect("session");
        let paths = android_runtime_paths(
            files.to_string_lossy().into_owned(),
            cache.to_string_lossy().into_owned(),
            Some(bundled.to_string_lossy().into_owned()),
        );

        assert_eq!(
            paths.user_library_dir,
            files
                .join("R")
                .join("library")
                .to_string_lossy()
                .into_owned()
        );
        assert_eq!(
            paths.temp_dir,
            cache.join("Rtmp").to_string_lossy().into_owned()
        );
        assert_eq!(
            paths.library_paths,
            vec![
                files
                    .join("R")
                    .join("library")
                    .to_string_lossy()
                    .into_owned(),
                bundled.to_string_lossy().into_owned(),
            ]
        );

        session
            .configure_android_runtime(paths)
            .expect("configure paths");

        let result = session
            .eval_result(".libPaths()".to_string())
            .expect("lib paths");
        assert_eq!(result.value.kind, RValueKind::StringVector);
        assert_eq!(
            result.value.string_values,
            vec![
                Some(
                    files
                        .join("R")
                        .join("library")
                        .to_string_lossy()
                        .into_owned()
                ),
                Some(bundled.to_string_lossy().into_owned())
            ]
        );
        assert_eq!(
            session.runtime_info().expect("runtime info"),
            RuntimeInfo {
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
    fn package_helpers_run_on_worker_session() {
        let (root, pkg) = make_test_package("rport-uniffi-package");
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");
        let session = RSession::new().expect("session");

        session
            .configure_android_runtime(android_runtime_paths(
                files.to_string_lossy().into_owned(),
                cache.to_string_lossy().into_owned(),
                Some(bundled.to_string_lossy().into_owned()),
            ))
            .expect("configure paths");

        assert!(
            session
                .package_available("tiny".to_string())
                .expect("available")
        );
        assert_eq!(
            session
                .package_path("tiny".to_string())
                .expect("package path"),
            Some(pkg.to_string_lossy().into_owned())
        );
        assert_eq!(
            session
                .package_info("tiny".to_string())
                .expect("package info"),
            Some(PackageInfo {
                name: "tiny".to_string(),
                version: "0.0.1".to_string(),
                path: pkg.to_string_lossy().into_owned(),
                library_path: bundled.to_string_lossy().into_owned(),
            })
        );
        assert_eq!(
            session
                .installed_packages()
                .expect("installed packages")
                .into_iter()
                .map(|package| package.name)
                .collect::<Vec<_>>(),
            vec!["tiny".to_string()]
        );
        assert!(
            !session
                .package_available("../tiny".to_string())
                .expect("invalid package unavailable")
        );
        session
            .load_package("tiny".to_string())
            .expect("load package");
        assert_eq!(
            session.eval("tiny_value()".to_string()).expect("eval"),
            "[1] 42"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parallel_worker_sessions_keep_state_isolated() {
        const WORKERS: usize = 4;

        let barrier = Arc::new(Barrier::new(WORKERS));
        let handles = (0..WORKERS)
            .map(|index| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let session = RSession::new().expect("session");
                    let value = 200 + index as i32;

                    barrier.wait();

                    assert_eq!(
                        session
                            .eval(format!("x <- {value}L\nx"))
                            .expect("assign global"),
                        format!("[1] {value}")
                    );
                    assert_eq!(
                        session.eval("x".to_string()).expect("read global"),
                        format!("[1] {value}")
                    );

                    let plot = session
                        .render(
                            format!(
                                "plot(c(1, 2, 3), c({value}, {next}, {last}), main = \"worker {index}\", col = \"red\", type = \"l\")",
                                next = value + 1,
                                last = value + 2,
                            ),
                            220,
                            160,
                        )
                        .expect("render");
                    assert_eq!(plot.width, 220);
                    assert_eq!(plot.height, 160);
                    assert!(plot.pixels.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
                    assert!(plot.pixels.len() > 256);

                    value
                })
            })
            .collect::<Vec<_>>();

        let mut values = handles
            .into_iter()
            .map(|handle| handle.join().expect("worker should not panic"))
            .collect::<Vec<_>>();
        values.sort_unstable();
        assert_eq!(values, vec![200, 201, 202, 203]);
    }

    #[test]
    fn android_runtime_paths_omits_missing_bundled_library() {
        let paths = android_runtime_paths(
            "/tmp/app-files".to_string(),
            "/tmp/app-cache".to_string(),
            None,
        );

        assert_eq!(paths.user_library_dir, "/tmp/app-files/R/library");
        assert_eq!(paths.temp_dir, "/tmp/app-cache/Rtmp");
        assert_eq!(paths.library_paths, vec!["/tmp/app-files/R/library"]);
    }

    #[test]
    fn render_passes_plot_code_to_worker_session() {
        let session = RSession::new().expect("session");

        let plot = session
            .render("plot(c(1, 2, 3), c(3, 2, 5))".to_string(), 320, 240)
            .expect("render");

        assert_eq!(plot.width, 320);
        assert_eq!(plot.height, 240);
        assert!(plot.pixels.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(plot.pixels.len() > 256);
    }

    #[test]
    fn cancel_stops_running_eval() {
        let session = Arc::new(RSession::new().expect("session"));
        let worker_session = session.clone();
        let worker =
            std::thread::spawn(move || worker_session.eval("repeat { 1 + 1 }".to_string()));

        std::thread::sleep(Duration::from_millis(10));
        session.cancel();

        let err = worker
            .join()
            .expect("worker should not panic")
            .expect_err("eval should be cancelled");
        assert!(matches!(err, RError::Cancelled));
    }
}

// ---------------------------------------------------------------------------
// Scaffolding
// ---------------------------------------------------------------------------

uniffi::setup_scaffolding!("rport");
