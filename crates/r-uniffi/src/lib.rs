//! r-uniffi: UniFFI bindings for the R interpreter.
//!
//! This crate provides high-level, safe bindings to the R interpreter
//! for use from Kotlin (Android), Swift (iOS), and Python.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;

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

#[derive(Debug, Clone, uniffi::Record)]
pub struct EvalResult {
    pub output: String,
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
        reply: Sender<Result<String, RError>>,
    },
    Render {
        width: u32,
        height: u32,
        reply: Sender<Result<PlotResult, RError>>,
    },
    Cancel,
    Shutdown,
}

// ---------------------------------------------------------------------------
// RSession object
// ---------------------------------------------------------------------------

#[derive(Object)]
pub struct RSession {
    cmd_tx: Mutex<Option<Sender<SessionCommand>>>,
    cancelled: Arc<AtomicBool>,
    callback: Mutex<Option<Arc<dyn SessionCallback>>>,
    operation_id: AtomicU64,
}

fn spawn_worker(
    cmd_rx: Receiver<SessionCommand>,
    callback: Arc<Mutex<Option<Arc<dyn SessionCallback>>>>,
    cancelled: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let mut session = match r_embed::RSession::new() {
            Ok(s) => s,
            Err(e) => {
                if let Some(cb) = callback.lock().unwrap().as_ref() {
                    cb.on_error(format!("init failed: {e}"));
                }
                return;
            }
        };

        while let Ok(cmd) = cmd_rx.recv() {
            if cancelled.load(Ordering::SeqCst) {
                cancelled.store(false, Ordering::SeqCst);
                continue;
            }

            match cmd {
                SessionCommand::Eval { code, reply } => {
                    let result = session
                        .eval(&code)
                        .map_err(|e| RError::EvalError(e.to_string()));

                    if let Some(cb) = callback.lock().unwrap().as_ref() {
                        match &result {
                            Ok(output) => cb.on_eval_complete(EvalResult {
                                output: output.clone(),
                            }),
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

                    if let Some(cb) = callback.lock().unwrap().as_ref()
                        && let Ok(plot) = &result
                    {
                        cb.on_plot_ready(plot.clone());
                    }

                    let _ = reply.send(result);
                }
                SessionCommand::Cancel => {
                    cancelled.store(true, Ordering::SeqCst);
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
        let cancelled = Arc::new(AtomicBool::new(false));

        spawn_worker(cmd_rx, Arc::new(Mutex::new(None)), cancelled.clone());

        Ok(Self {
            cmd_tx: Mutex::new(Some(cmd_tx)),
            cancelled,
            callback: Mutex::new(None),
            operation_id: AtomicU64::new(0),
        })
    }

    pub fn set_callback(&self, callback: Box<dyn SessionCallback>) {
        *self.callback.lock().unwrap() = Some(callback.into());
    }

    pub fn eval(&self, code: String) -> Result<String, RError> {
        let tx = self.cmd_tx.lock().map_err(|_| RError::SessionClosed)?;
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, reply_rx) = channel();
        tx.send(SessionCommand::Eval {
            code,
            reply: reply_tx,
        })
        .map_err(|_| RError::SessionClosed)?;
        reply_rx.recv().map_err(|_| RError::SessionClosed)?
    }

    pub fn eval_async(&self, code: String) -> Result<u64, RError> {
        let tx = self.cmd_tx.lock().map_err(|_| RError::SessionClosed)?;
        let tx = tx.as_ref().ok_or(RError::SessionClosed)?;
        let (reply_tx, _) = channel();
        let op_id = self.operation_id.fetch_add(1, Ordering::Relaxed);
        tx.send(SessionCommand::Eval {
            code,
            reply: reply_tx,
        })
        .map_err(|_| RError::SessionClosed)?;
        Ok(op_id)
    }

    pub fn render(&self, _code: String, width: u32, height: u32) -> Result<PlotResult, RError> {
        let tx = self.cmd_tx.lock().map_err(|_| RError::SessionClosed)?;
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
        let tx = self.cmd_tx.lock().map_err(|_| RError::SessionClosed)?;
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
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(tx_guard) = self.cmd_tx.lock()
            && let Some(tx) = tx_guard.as_ref()
        {
            let _ = tx.send(SessionCommand::Cancel);
        }
    }

    pub fn destroy(&self) {
        self.cancel();
        if let Ok(mut tx_guard) = self.cmd_tx.lock()
            && let Some(tx) = tx_guard.take()
        {
            let _ = tx.send(SessionCommand::Shutdown);
        }
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        self.destroy();
    }
}

// ---------------------------------------------------------------------------
// Scaffolding
// ---------------------------------------------------------------------------

uniffi::setup_scaffolding!("rport");
