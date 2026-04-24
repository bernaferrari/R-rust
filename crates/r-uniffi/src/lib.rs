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
    List,
    Unsupported,
    Error,
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
    pub string_values: Vec<String>,
    pub list_values: Vec<RValue>,
    pub type_name: String,
    pub display: String,
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
        list_values: Vec::new(),
        type_name: String::new(),
        display: String::new(),
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
            r_embed::RValue::List(values) => RValue {
                list_values: values.into_iter().map(RValue::from).collect(),
                ..empty_value(RValueKind::List)
            },
            r_embed::RValue::Unsupported { type_name, display } => RValue {
                type_name,
                display,
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
    Eval {
        code: String,
        reply: Sender<Result<EvalResult, RError>>,
    },
    Render {
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
                    width,
                    height,
                    reply,
                } => {
                    let result = session
                        .render_with_dimensions("", width, height)
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

    pub fn render(&self, _code: String, width: u32, height: u32) -> Result<PlotResult, RError> {
        let tx = self.cmd_tx.lock().unwrap_or_else(|e| e.into_inner());
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, reply_rx) = channel();
        tx.send(SessionCommand::Render {
            width,
            height,
            reply: reply_tx,
        })
        .map_err(|_| RError::SessionClosed)?;
        reply_rx.recv().map_err(|_| RError::SessionClosed)?
    }

    pub fn render_async(&self, _code: String, width: u32, height: u32) -> Result<u64, RError> {
        let tx = self.cmd_tx.lock().unwrap_or_else(|e| e.into_inner());
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, _) = channel();
        let op_id = self.operation_id.fetch_add(1, Ordering::Relaxed);
        tx.send(SessionCommand::Render {
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
