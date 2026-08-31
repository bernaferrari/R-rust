//! Interpreter worker thread: bounded command queue, callback dispatcher, and
//! worker lifecycle (initialization handshake, bounded shutdown join).

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use r_embed::CancellationToken;

use super::cancellation::map_eval_error;
use super::conversion::{
    AndroidRuntimePaths, DataFramePage, EvalResult, PackageInfo, ProgressUpdate, RValue,
    ResourceLimits, RuntimeInfo, null_eval_result,
};
use super::error::RError;
use super::operation::{OpOutcome, OperationResult, OperationTable};
use super::plot::PlotResult;

/// Capacity of the bounded command queue between API callers and the
/// interpreter worker. `try_send` beyond capacity fails with
/// [`RError::QueueFull`] instead of growing memory without bound.
pub(crate) const QUEUE_CAPACITY: usize = 64;

/// `shutdown`/`Drop` join the worker with this bounded wait; if the worker has
/// not exited in time it is detached (a running interpreter thread cannot be
/// aborted safely).
pub(crate) const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Bounded wait for the callback dispatcher thread at shutdown.
const CALLBACK_JOIN_TIMEOUT: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// Callback interface
// ---------------------------------------------------------------------------

#[uniffi::export(callback_interface)]
/// Ordered operation notifications delivered away from the interpreter
/// thread. `operation_id` is the id returned by `eval_async` or
/// `render_async`; hosts must ignore notifications that do not match their
/// current session and operation identity. Terminal payloads remain
/// recoverable through `take_result`, so callbacks are never the sole owner.
pub trait SessionCallback: Send + Sync + 'static {
    fn on_progress(&self, operation_id: u64, update: ProgressUpdate);
    fn on_output(&self, operation_id: u64, line: String);
    fn on_plot_ready(&self, operation_id: u64, plot: PlotResult);
    fn on_eval_complete(&self, operation_id: u64, result: EvalResult);
    fn on_error(&self, operation_id: u64, error: String);
}

// ---------------------------------------------------------------------------
// Callback dispatcher
// ---------------------------------------------------------------------------

/// Callback delivery policy:
///
/// * Host callbacks are **never invoked on the interpreter thread**. The
///   worker only enqueues small [`SessionEvent`] values; a dedicated
///   dispatcher thread drains the queue and invokes the user callback. A slow
///   or re-entrant callback therefore cannot stall the interpreter or deadlock
///   the worker.
/// * R code has no path that calls back into the host: the callback interface
///   is implemented host-side and only ever fires from the dispatcher thread.
///   Re-entrant callbacks from R code are unsupported by construction, not by
///   runtime checks.
/// * Delivery is ordered (single queue, single dispatcher thread) and
///   asynchronous: events may arrive shortly after the corresponding request
///   already returned.
#[derive(Clone)]
pub(crate) struct CallbackDispatcher {
    inner: Arc<DispatcherInner>,
}

struct DispatcherInner {
    callback: Arc<Mutex<Option<Arc<dyn SessionCallback>>>>,
    tx: Mutex<Option<Sender<SessionEvent>>>,
    exited: Mutex<Option<Receiver<()>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}
/// Small event queued from the worker thread to the callback dispatcher.
#[allow(clippy::large_enum_variant)]
pub(crate) enum SessionEvent {
    Progress(u64, ProgressUpdate),
    Output(u64, String),
    PlotReady(u64, PlotResult),
    EvalComplete(u64, EvalResult),
    Error(u64, String),
}

impl SessionEvent {
    fn deliver(self, callback: &dyn SessionCallback) {
        match self {
            SessionEvent::Progress(operation_id, update) => {
                callback.on_progress(operation_id, update)
            }
            SessionEvent::Output(operation_id, line) => callback.on_output(operation_id, line),
            SessionEvent::PlotReady(operation_id, plot) => {
                callback.on_plot_ready(operation_id, plot)
            }
            SessionEvent::EvalComplete(operation_id, result) => {
                callback.on_eval_complete(operation_id, result)
            }
            SessionEvent::Error(operation_id, message) => callback.on_error(operation_id, message),
        }
    }
}

impl CallbackDispatcher {
    pub(crate) fn new() -> Self {
        let callback: Arc<Mutex<Option<Arc<dyn SessionCallback>>>> = Arc::default();
        let (tx, rx) = channel::<SessionEvent>();
        let (exited_tx, exited_rx) = channel::<()>();
        let thread_callback = Arc::clone(&callback);
        let join = std::thread::spawn(move || {
            for event in rx {
                let callback = thread_callback
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Some(callback) = callback.as_deref() {
                    event.deliver(callback);
                }
            }
            let _ = exited_tx.send(());
        });
        Self {
            inner: Arc::new(DispatcherInner {
                callback,
                tx: Mutex::new(Some(tx)),
                exited: Mutex::new(Some(exited_rx)),
                join: Mutex::new(Some(join)),
            }),
        }
    }

    pub(crate) fn set_callback(&self, callback: Box<dyn SessionCallback>) {
        *self
            .inner
            .callback
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Arc::from(callback));
    }

    /// Enqueue an event for asynchronous delivery; never blocks the caller.
    pub(crate) fn dispatch(&self, event: SessionEvent) {
        let tx = self.inner.tx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = tx.as_ref() {
            let _ = tx.send(event);
        }
    }

    /// Stop the dispatcher: drop the queue, wait up to
    /// [`CALLBACK_JOIN_TIMEOUT`] for the thread to finish any in-flight user
    /// callback, then join (or detach if a user callback is wedged).
    pub(crate) fn shutdown(&self) {
        *self.inner.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let exited = self
            .inner
            .exited
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let join = self
            .inner
            .join
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let Some(join) = join else {
            return;
        };
        let signaled = match exited.map(|exited| exited.recv_timeout(CALLBACK_JOIN_TIMEOUT)) {
            Some(Ok(())) | Some(Err(RecvTimeoutError::Disconnected)) | None => true,
            Some(Err(RecvTimeoutError::Timeout)) => false,
        };
        if signaled {
            let _ = join.join();
        } else {
            // Detach: a wedged user callback must not block teardown forever.
            drop(join);
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Per-operation handle: registry id plus this operation's private
/// cancellation token.
pub(crate) struct OpHandle {
    pub id: u64,
    pub token: CancellationToken,
}

/// Reply channel of a synchronous request; async operations carry `None` and
/// are observed via `operation_status` / `take_result` / callbacks instead.
pub(crate) type Reply<T> = Option<Sender<Result<T, RError>>>;

pub(crate) enum OpKind {
    ConfigurePaths {
        paths: AndroidRuntimePaths,
        reply: Reply<()>,
    },
    RuntimeInfo {
        reply: Reply<RuntimeInfo>,
    },
    ResourceLimits {
        reply: Reply<ResourceLimits>,
    },
    SetResourceLimits {
        limits: ResourceLimits,
        reply: Reply<()>,
    },
    PackageAvailable {
        package: String,
        reply: Reply<bool>,
    },
    PackagePath {
        package: String,
        reply: Reply<Option<String>>,
    },
    PackageInfo {
        package: String,
        reply: Reply<Option<PackageInfo>>,
    },
    InstalledPackages {
        reply: Reply<Vec<PackageInfo>>,
    },
    LoadPackage {
        package: String,
        reply: Reply<()>,
    },
    Eval {
        code: String,
        reply: Reply<EvalResult>,
    },
    DataFramePage {
        name: String,
        offset: u64,
        limit: u64,
        reply: Reply<DataFramePage>,
    },
    Render {
        code: String,
        width: u32,
        height: u32,
        reply: Reply<PlotResult>,
    },
}

pub(crate) enum SessionCommand {
    Op { op: OpHandle, kind: OpKind },
    Shutdown,
}

// ---------------------------------------------------------------------------
// Worker thread
// ---------------------------------------------------------------------------

/// Factory for the worker's interpreter session; injectable for tests.
pub(crate) type WorkerInit =
    Box<dyn FnOnce() -> Result<r_embed::RSession, r_embed::RSessionError> + Send>;

pub(crate) fn default_init() -> WorkerInit {
    Box::new(r_embed::RSession::new)
}

pub(crate) struct WorkerConfig {
    pub commands: Receiver<SessionCommand>,
    pub ready: Sender<Result<(), String>>,
    pub exited: Sender<()>,
    pub table: Arc<Mutex<OperationTable>>,
    pub dispatcher: CallbackDispatcher,
    pub init: WorkerInit,
}

/// Spawn the interpreter worker and return its retained handle.
///
/// `commands` is the bounded queue's sender (kept by [`WorkerHandle`]);
/// `exited_rx` is the receiver of the worker's exit signal.
pub(crate) fn spawn_worker(
    commands: SyncSender<SessionCommand>,
    exited_rx: Receiver<()>,
    config: WorkerConfig,
) -> WorkerHandle {
    let WorkerConfig {
        commands: rx,
        ready,
        exited,
        table,
        dispatcher,
        init,
    } = config;

    let join = std::thread::spawn(move || {
        // Initialization handshake: build the interpreter session on this
        // thread and report the outcome exactly once.
        let initialized = catch_unwind(AssertUnwindSafe(init));
        let mut session = match initialized {
            Ok(Ok(session)) => {
                let _ = ready.send(Ok(()));
                session
            }
            Ok(Err(err)) => {
                let _ = ready.send(Err(err.to_string()));
                return;
            }
            Err(payload) => {
                let _ = ready.send(Err(panic_message(&payload)));
                return;
            }
        };

        while let Ok(command) = rx.recv() {
            match command {
                SessionCommand::Op { op, kind } => {
                    let op_id = op.id;
                    let executed = run_guarded(|| {
                        execute_operation(&mut session, op, kind, &table, &dispatcher);
                        Ok(())
                    });
                    // execute_operation is panic-free by construction; this is
                    // the belt-and-braces record if that ever changes.
                    if let Err(err) = executed {
                        dispatcher.dispatch(SessionEvent::Error(op_id, err.to_string()));
                        lock(&table).complete(op_id, OpOutcome::Failed(err.to_string()));
                    }
                }
                SessionCommand::Shutdown => {
                    session.close();
                    break;
                }
            }
        }
        let _ = exited.send(());
    });

    WorkerHandle::new(commands, exited_rx, join)
}

/// Execute one operation: mark it running, run the interpreter call under a
/// panic guard, record the terminal outcome, deliver callbacks through the
/// dispatcher, and reply to a synchronous caller.
fn execute_operation(
    session: &mut r_embed::RSession,
    op: OpHandle,
    kind: OpKind,
    table: &Arc<Mutex<OperationTable>>,
    dispatcher: &CallbackDispatcher,
) {
    lock(table).mark_running(op.id);

    match kind {
        OpKind::ConfigurePaths { paths, reply } => {
            let result = run_guarded(|| {
                let embed_paths = r_embed::AndroidRuntimePaths::new(
                    paths.app_files_dir,
                    paths.cache_dir,
                    paths.bundled_library_dir,
                );
                session
                    .configure_android_runtime(&embed_paths)
                    .map_err(|err| RError::InitFailed(err.to_string()))
            });
            settle(op, result, table, dispatcher, reply, null_eval_result(""));
        }
        OpKind::RuntimeInfo { reply } => {
            let result = run_guarded(|| {
                let info = session.runtime_info();
                Ok(RuntimeInfo {
                    is_active: info.is_active,
                    library_paths: info.library_paths,
                    temp_dir: info.temp_dir,
                })
            });
            settle(op, result, table, dispatcher, reply, null_eval_result(""));
        }
        OpKind::ResourceLimits { reply } => {
            let result = run_guarded(|| Ok(session.resource_limits().into()));
            settle(op, result, table, dispatcher, reply, null_eval_result(""));
        }
        OpKind::SetResourceLimits { limits, reply } => {
            let result = run_guarded(|| {
                session
                    .set_resource_limits(limits.into())
                    .map_err(|err| RError::EvalError(err.to_string()))
            });
            settle(op, result, table, dispatcher, reply, null_eval_result(""));
        }
        OpKind::PackageAvailable { package, reply } => {
            let result = run_guarded(|| Ok(session.package_available(&package)));
            settle(op, result, table, dispatcher, reply, null_eval_result(""));
        }
        OpKind::PackagePath { package, reply } => {
            let result = run_guarded(|| Ok(session.package_path(&package)));
            settle(op, result, table, dispatcher, reply, null_eval_result(""));
        }
        OpKind::PackageInfo { package, reply } => {
            let result = run_guarded(|| Ok(session.package_info(&package).map(PackageInfo::from)));
            settle(op, result, table, dispatcher, reply, null_eval_result(""));
        }
        OpKind::InstalledPackages { reply } => {
            let result = run_guarded(|| {
                Ok(session
                    .installed_packages()
                    .into_iter()
                    .map(PackageInfo::from)
                    .collect())
            });
            settle(op, result, table, dispatcher, reply, null_eval_result(""));
        }
        OpKind::LoadPackage { package, reply } => {
            let result = run_guarded(|| {
                session
                    .load_package(&package)
                    .map_err(|err| RError::EvalError(err.to_string()))
            });
            settle(op, result, table, dispatcher, reply, null_eval_result(""));
        }
        OpKind::Eval { code, reply } => {
            dispatcher.dispatch(SessionEvent::Progress(
                op.id,
                ProgressUpdate {
                    progress: 0.0,
                    message: "Evaluating...".to_string(),
                },
            ));
            let result = run_guarded(|| {
                session
                    .eval_result_cancellable(&code, &op.token)
                    .map(|output| EvalResult {
                        output: output.output,
                        value: RValue::from(output.value),
                    })
                    .map_err(map_eval_error)
            });
            match result {
                Ok(eval_result) => {
                    dispatcher.dispatch(SessionEvent::Progress(
                        op.id,
                        ProgressUpdate {
                            progress: 1.0,
                            message: "Complete".to_string(),
                        },
                    ));
                    for line in eval_result.output.lines() {
                        dispatcher.dispatch(SessionEvent::Output(op.id, line.to_string()));
                    }
                    dispatcher.dispatch(SessionEvent::EvalComplete(op.id, eval_result.clone()));
                    lock(table).complete(
                        op.id,
                        OpOutcome::Succeeded(OperationResult::Eval {
                            result: eval_result.clone(),
                        }),
                    );
                    fire_reply(reply, Ok(eval_result));
                }
                Err(err) => settle_error(op, err, table, dispatcher, reply),
            }
        }
        OpKind::DataFramePage {
            name,
            offset,
            limit,
            reply,
        } => {
            let result = run_guarded(|| {
                let name = quote_r_string(&name);
                let count = session
                    .eval_result_cancellable(
                        &format!("nrow(get({name}, envir = .GlobalEnv))"),
                        &op.token,
                    )
                    .map_err(map_table_eval_error)?;
                let total_rows = nonnegative_integer(&count.value).ok_or_else(|| {
                    RError::InvalidInput("object is not a rectangular table".to_string())
                })?;
                let end = offset.saturating_add(limit).min(total_rows);
                let slice = if offset >= total_rows {
                    format!("get({name}, envir = .GlobalEnv)[FALSE, , drop = FALSE]")
                } else {
                    format!(
                        "get({name}, envir = .GlobalEnv)[seq.int({}, {}), , drop = FALSE]",
                        offset + 1,
                        end,
                    )
                };
                let page = session
                    .eval_result_cancellable(&slice, &op.token)
                    .map_err(map_table_eval_error)?;
                Ok(DataFramePage {
                    value: RValue::from(page.value),
                    total_rows,
                    offset,
                })
            });
            match result {
                Ok(page) => {
                    lock(table).complete(
                        op.id,
                        OpOutcome::Succeeded(OperationResult::Eval {
                            result: null_eval_result(""),
                        }),
                    );
                    fire_reply(reply, Ok(page));
                }
                Err(err) => settle_error(op, err, table, dispatcher, reply),
            }
        }
        OpKind::Render {
            code,
            width,
            height,
            reply,
        } => {
            dispatcher.dispatch(SessionEvent::Progress(
                op.id,
                ProgressUpdate {
                    progress: 0.0,
                    message: "Rendering...".to_string(),
                },
            ));
            // Rendering has no mid-flight cancellation hook: honor a token
            // that is already cancelled, otherwise run to completion.
            let result = if op.token.is_cancelled() {
                Err(RError::Cancelled)
            } else {
                run_guarded(|| {
                    session
                        .render_with_dimensions(&code, width, height)
                        .map(|png_bytes| PlotResult {
                            width,
                            height,
                            png_bytes,
                        })
                        .map_err(|err| RError::RenderError(err.to_string()))
                })
            };
            // Rendering cannot currently be interrupted mid-device call, but
            // a cancellation requested while it was running still wins at
            // the operation seam: discard the late plot instead of reporting
            // a cancelled operation as successful.
            let result = if op.token.is_cancelled() {
                Err(RError::Cancelled)
            } else {
                result
            };
            match result {
                Ok(plot) => {
                    dispatcher.dispatch(SessionEvent::Progress(
                        op.id,
                        ProgressUpdate {
                            progress: 1.0,
                            message: "Complete".to_string(),
                        },
                    ));
                    dispatcher.dispatch(SessionEvent::PlotReady(op.id, plot.clone()));
                    lock(table).complete(
                        op.id,
                        OpOutcome::Succeeded(OperationResult::Render {
                            result: plot.clone(),
                        }),
                    );
                    fire_reply(reply, Ok(plot));
                }
                Err(err) => settle_error(op, err, table, dispatcher, reply),
            }
        }
    }
}

fn quote_r_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('\"', "\\\"")
            .replace('\r', "\\r")
            .replace('\n', "\\n")
    )
}

fn map_table_eval_error(error: r_embed::RSessionError) -> RError {
    match map_eval_error(error) {
        RError::Cancelled => RError::Cancelled,
        _ => RError::InvalidInput("object is not a rectangular table".to_string()),
    }
}

fn nonnegative_integer(value: &r_embed::RValue) -> Option<u64> {
    match value {
        r_embed::RValue::Integer(Some(value)) if *value >= 0 => Some(*value as u64),
        r_embed::RValue::Real(Some(value))
            if value.is_finite() && *value >= 0.0 && value.fract() == 0.0 =>
        {
            Some(*value as u64)
        }
        r_embed::RValue::Attributed { value, .. } => nonnegative_integer(value),
        _ => None,
    }
}

/// Record the outcome and reply for commands without dedicated callbacks.
fn settle<T>(
    op: OpHandle,
    result: Result<T, RError>,
    table: &Arc<Mutex<OperationTable>>,
    dispatcher: &CallbackDispatcher,
    reply: Reply<T>,
    success_payload: EvalResult,
) {
    match result {
        Ok(value) => {
            lock(table).complete(
                op.id,
                OpOutcome::Succeeded(OperationResult::Eval {
                    result: success_payload,
                }),
            );
            fire_reply(reply, Ok(value));
        }
        Err(err) => settle_error(op, err, table, dispatcher, reply),
    }
}

fn settle_error<T>(
    op: OpHandle,
    err: RError,
    table: &Arc<Mutex<OperationTable>>,
    dispatcher: &CallbackDispatcher,
    reply: Reply<T>,
) {
    lock(table).complete(op.id, failed_outcome(&err));
    dispatcher.dispatch(SessionEvent::Error(op.id, err.to_string()));
    fire_reply(reply, Err(err));
}

fn failed_outcome(err: &RError) -> OpOutcome {
    match err {
        RError::Cancelled => OpOutcome::Cancelled,
        other => OpOutcome::Failed(other.to_string()),
    }
}

fn fire_reply<T>(reply: Reply<T>, result: Result<T, RError>) {
    if let Some(reply) = reply {
        let _ = reply.send(result);
    }
}

fn run_guarded<T>(execute: impl FnOnce() -> Result<T, RError>) -> Result<T, RError> {
    catch_unwind(AssertUnwindSafe(execute))
        .unwrap_or_else(|payload| Err(RError::InternalError(panic_message(&payload))))
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_else(|| "interpreter worker panicked".to_string())
}

/// Lock a mutex, recovering from poisoning: the guarded data is a state
/// machine with no cross-field invariants, so a panicked holder must not
/// deadlock the session forever.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Worker handle
// ---------------------------------------------------------------------------

/// Retained handle for the interpreter worker: the bounded command sender,
/// the exit signal receiver, and the join handle.
pub(crate) struct WorkerHandle {
    commands: Option<SyncSender<SessionCommand>>,
    exited: Mutex<Receiver<()>>,
    join: Option<JoinHandle<()>>,
    detached: bool,
}

impl WorkerHandle {
    pub(crate) fn new(
        commands: SyncSender<SessionCommand>,
        exited: Receiver<()>,
        join: JoinHandle<()>,
    ) -> Self {
        Self {
            commands: Some(commands),
            exited: Mutex::new(exited),
            join: Some(join),
            detached: false,
        }
    }
    /// Enqueue without ever blocking; a full queue surfaces as
    /// [`RError::QueueFull`] at the caller.
    #[allow(clippy::result_large_err)]
    pub(crate) fn try_send(
        &self,
        command: SessionCommand,
    ) -> Result<(), TrySendError<SessionCommand>> {
        match &self.commands {
            Some(commands) => commands.try_send(command),
            None => Err(TrySendError::Disconnected(command)),
        }
    }

    /// Send `Shutdown`, wait [`SHUTDOWN_JOIN_TIMEOUT`] for exit, then join.
    /// If the worker is wedged (e.g. inside a non-cancellable render), detach
    /// instead of blocking the caller forever.
    pub(crate) fn shutdown(&mut self) {
        if let Some(commands) = self.commands.take() {
            // A full queue cannot accept Shutdown; dropping `commands` closes
            // the queue and the worker exits after draining the pending
            // operations (whose tokens the caller cancelled first).
            let _ = commands.try_send(SessionCommand::Shutdown);
        }
        self.join_bounded(SHUTDOWN_JOIN_TIMEOUT);
    }

    /// Release a worker whose initialization never completed: close the
    /// command queue and detach. The thread exits on its own once `init`
    /// returns; a hung init cannot be aborted safely.
    pub(crate) fn detach(mut self) {
        drop(self.commands.take());
        // Drop the exit receiver so the worker's exit signal is not left
        // pending on a channel nobody reads.
        let exited = self.exited.get_mut().unwrap_or_else(|e| e.into_inner());
        *exited = channel::<()>().1;
        drop(self.join.take());
        self.detached = true;
    }

    fn join_bounded(&mut self, timeout: Duration) -> bool {
        let signaled = {
            let exited = self.exited.lock().unwrap_or_else(|e| e.into_inner());
            match exited.recv_timeout(timeout) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
                Err(RecvTimeoutError::Timeout) => false,
            }
        };
        if signaled {
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        } else {
            // Detach: a running thread cannot be aborted; leave it alone.
            drop(self.join.take());
        }
        signaled
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        if self.detached {
            return;
        }
        self.commands = None; // close the queue so the worker can exit
        self.join_bounded(SHUTDOWN_JOIN_TIMEOUT);
    }
}
