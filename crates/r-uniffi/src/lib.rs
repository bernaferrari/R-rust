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
    #[error("Failed to initialize R session")]
    InitFailed,
    #[error("Evaluation error: {0}")]
    EvalError(String),
    #[error("Render error: {0}")]
    RenderError(String),
    #[error("Session is already closed")]
    SessionClosed,
    #[error("Invalid input")]
    InvalidInput,
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error("Operation cancelled")]
    Cancelled,
    #[error("Session busy")]
    SessionBusy,
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
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct EvalResult {
    pub output: String,
    pub value: RValue,
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
        app_files_dir: String,
        cache_dir: String,
        bundled_library_dir: Option<String>,
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
                SessionCommand::ConfigurePaths {
                    app_files_dir,
                    cache_dir,
                    bundled_library_dir,
                    reply,
                } => {
                    let result = session
                        .configure_android_paths(
                            &app_files_dir,
                            &cache_dir,
                            bundled_library_dir.as_deref(),
                        )
                        .map_err(|e| RError::InitFailed);
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

    pub fn eval(&self, code: String) -> Result<String, RError> {
        self.eval_result(code).map(|result| result.output)
    }

    pub fn eval_result(&self, code: String) -> Result<EvalResult, RError> {
        let tx = self.cmd_tx.lock().unwrap_or_else(|e| e.into_inner());
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, reply_rx) = channel();
        self.cancelled.reset();
        tx.send(SessionCommand::Eval {
            code,
            reply: reply_tx,
        })
        .map_err(|_| RError::SessionClosed)?;
        reply_rx.recv().map_err(|_| RError::SessionClosed)?
    }

    pub fn configure_android_paths(
        &self,
        app_files_dir: String,
        cache_dir: String,
        bundled_library_dir: Option<String>,
    ) -> Result<(), RError> {
        let tx = self.cmd_tx.lock().unwrap_or_else(|e| e.into_inner());
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, reply_rx) = channel();
        tx.send(SessionCommand::ConfigurePaths {
            app_files_dir,
            cache_dir,
            bundled_library_dir,
            reply: reply_tx,
        })
        .map_err(|_| RError::SessionClosed)?;
        reply_rx.recv().map_err(|_| RError::SessionClosed)?
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
        let tx = self.cmd_tx.lock().unwrap_or_else(|e| e.into_inner());
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, reply_rx) = channel();
        tx.send(SessionCommand::Render {
            code,
            width,
            height,
            reply: reply_tx,
        })
        .map_err(|_| RError::SessionClosed)?;
        reply_rx.recv().map_err(|_| RError::SessionClosed)?
    }

    pub fn render_async(&self, code: String, width: u32, height: u32) -> Result<u64, RError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn cancel_without_active_eval_does_not_poison_next_eval() {
        let session = RSession::new().expect("session");

        session.cancel();

        assert_eq!(session.eval("1 + 1".to_string()).unwrap(), "[1] 2");
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

        session
            .configure_android_paths(
                files.to_string_lossy().into_owned(),
                cache.to_string_lossy().into_owned(),
                Some(bundled.to_string_lossy().into_owned()),
            )
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

        let _ = std::fs::remove_dir_all(root);
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
