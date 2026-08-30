//! The exported `RSession` object: initialization handshake, bounded request
//! path, and the public UniFFI API.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, TrySendError, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uniffi::Object;

use super::cancellation::new_token;
use super::conversion::{
    AndroidRuntimePaths, EvalResult, PackageInfo, ResourceLimits, RuntimeInfo,
    android_runtime_paths, validate_package_name,
};
use super::error::RError;
use super::operation::{OperationStatus, OperationTable, RETAINED_COMPLETED};
use super::plot::{PlotResult, validate_plot_dimensions};
use super::worker::{
    CallbackDispatcher, OpHandle, OpKind, QUEUE_CAPACITY, SessionCallback, SessionCommand,
    WorkerConfig, WorkerHandle, WorkerInit, default_init, lock, spawn_worker,
};

/// Upper bound on how long the constructor waits for the worker to build its
/// interpreter session before failing with [`RError::InitFailed`].
pub(crate) const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default deadline for synchronous requests (per-call overrides exist on the
/// internal `request_with_timeout` seam). A timed-out request also requests
/// cancellation of its operation so the worker unwinds promptly.
pub(crate) const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// A UniFFI-facing R interpreter session.
///
/// All interpretation runs on a dedicated worker thread behind a bounded
/// command queue. Each operation owns a private cancellation token; see
/// [`super::cancellation`] for the policy.
#[derive(Object)]
pub struct RSession {
    worker: Mutex<Option<WorkerHandle>>,
    table: Arc<Mutex<OperationTable>>,
    dispatcher: CallbackDispatcher,
    next_operation_id: AtomicU64,
}

#[uniffi::export]
impl RSession {
    /// Create a session: spawn the interpreter worker, run `RSession::new` on
    /// it, and block (at most [`INIT_TIMEOUT`]) until the handshake reports
    /// success. Initialization failure or timeout is a constructor error.
    #[uniffi::constructor]
    pub fn new() -> Result<Self, RError> {
        RSession::initialize(default_init(), INIT_TIMEOUT)
    }

    pub fn set_callback(&self, callback: Box<dyn SessionCallback>) {
        self.dispatcher.set_callback(callback);
    }

    pub fn is_active(&self) -> bool {
        lock(&self.worker).is_some()
    }

    pub fn eval(&self, code: String) -> Result<String, RError> {
        self.eval_result(code).map(|result| result.output)
    }

    pub fn eval_result(&self, code: String) -> Result<EvalResult, RError> {
        self.request(|reply| OpKind::Eval {
            code,
            reply: Some(reply),
        })
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
        self.request(|reply| OpKind::ConfigurePaths {
            paths,
            reply: Some(reply),
        })
    }

    pub fn runtime_info(&self) -> Result<RuntimeInfo, RError> {
        self.request(|reply| OpKind::RuntimeInfo { reply: Some(reply) })
    }

    pub fn resource_limits(&self) -> Result<ResourceLimits, RError> {
        self.request(|reply| OpKind::ResourceLimits { reply: Some(reply) })
    }

    pub fn set_resource_limits(&self, limits: ResourceLimits) -> Result<(), RError> {
        self.request(|reply| OpKind::SetResourceLimits {
            limits,
            reply: Some(reply),
        })
    }

    pub fn package_available(&self, package: String) -> Result<bool, RError> {
        validate_package_name(&package)?;
        self.request(|reply| OpKind::PackageAvailable {
            package,
            reply: Some(reply),
        })
    }

    pub fn package_path(&self, package: String) -> Result<Option<String>, RError> {
        validate_package_name(&package)?;
        self.request(|reply| OpKind::PackagePath {
            package,
            reply: Some(reply),
        })
    }

    pub fn package_info(&self, package: String) -> Result<Option<PackageInfo>, RError> {
        validate_package_name(&package)?;
        self.request(|reply| OpKind::PackageInfo {
            package,
            reply: Some(reply),
        })
    }

    pub fn installed_packages(&self) -> Result<Vec<PackageInfo>, RError> {
        self.request(|reply| OpKind::InstalledPackages { reply: Some(reply) })
    }

    pub fn load_package(&self, package: String) -> Result<(), RError> {
        validate_package_name(&package)?;
        self.request(|reply| OpKind::LoadPackage {
            package,
            reply: Some(reply),
        })
    }

    pub fn render(&self, code: String, width: u32, height: u32) -> Result<PlotResult, RError> {
        validate_plot_dimensions(width, height)?;
        self.request(|reply| OpKind::Render {
            code,
            width,
            height,
            reply: Some(reply),
        })
    }

    pub fn eval_async(&self, code: String) -> Result<u64, RError> {
        self.enqueue(true, || OpKind::Eval { code, reply: None })
    }

    pub fn render_async(&self, code: String, width: u32, height: u32) -> Result<u64, RError> {
        validate_plot_dimensions(width, height)?;
        self.enqueue(true, || OpKind::Render {
            code,
            width,
            height,
            reply: None,
        })
    }

    /// Request cancellation of every active (queued or running) operation.
    /// Each operation owns a private token, so cancelling now can never affect
    /// operations submitted later.
    pub fn cancel(&self) {
        lock(&self.table).cancel_all();
    }

    /// Compatibility alias for `cancel` (per-operation tokens removed the old
    /// "current operation" ambiguity).
    pub fn cancel_current_operation(&self) {
        self.cancel();
    }

    /// Request cancellation of one operation by id. Returns an error when the
    /// id was never registered; cancelling an already-finished operation is a
    /// no-op success.
    pub fn cancel_operation(&self, op_id: u64) -> Result<(), RError> {
        let mut table = lock(&self.table);
        if table.cancel_operation(op_id) || table.is_known(op_id) {
            Ok(())
        } else {
            Err(RError::InvalidInput(format!(
                "unknown operation id {op_id}"
            )))
        }
    }

    /// Snapshot of an operation's state machine position (see
    /// [`super::operation`]): `Queued`, `Running`, a terminal state, or
    /// `Unknown` / `Expired`.
    pub fn operation_status(&self, op_id: u64) -> OperationStatus {
        lock(&self.table).status(op_id)
    }

    /// Consume a completed operation, returning its terminal status
    /// (`Succeeded`/`Failed`/`Cancelled`). Queued and running operations are
    /// left untouched; a consumed (or evicted) id reports `Expired` once.
    pub fn take_result(&self, op_id: u64) -> OperationStatus {
        lock(&self.table).take_result(op_id)
    }

    /// Stop the worker: cancel all active operations, send `Shutdown`, and
    /// join the worker with a bounded wait.
    pub fn shutdown_worker(&self) {
        lock(&self.table).cancel_all();
        if let Some(mut worker) = lock(&self.worker).take() {
            worker.shutdown();
        }
    }
}

impl RSession {
    /// Shared constructor seam: injectable worker init + handshake deadline
    /// (exercised by the reliability tests).
    pub(crate) fn initialize(init: WorkerInit, init_timeout: Duration) -> Result<Self, RError> {
        let (command_tx, command_rx) = sync_channel(QUEUE_CAPACITY);
        let (ready_tx, ready_rx) = channel();
        let (exited_tx, exited_rx) = channel();
        let table = Arc::new(Mutex::new(OperationTable::new(RETAINED_COMPLETED)));
        let dispatcher = CallbackDispatcher::new();
        let mut worker = spawn_worker(
            command_tx,
            exited_rx,
            WorkerConfig {
                commands: command_rx,
                ready: ready_tx,
                exited: exited_tx,
                table: Arc::clone(&table),
                dispatcher: dispatcher.clone(),
                init,
            },
        );

        match ready_rx.recv_timeout(init_timeout) {
            Ok(Ok(())) => Ok(Self {
                worker: Mutex::new(Some(worker)),
                table,
                dispatcher,
                next_operation_id: AtomicU64::new(0),
            }),
            Ok(Err(message)) => {
                // Init failed on the worker: it is already exiting, join it.
                worker.shutdown();
                Err(RError::InitFailed(message))
            }
            Err(_) => {
                // Init hung past the deadline: the constructor must fail on
                // time. Close the queue and detach; the worker exits (or not)
                // on its own — a hung init cannot be aborted safely.
                worker.detach();
                Err(RError::InitFailed(format!(
                    "R interpreter initialization did not complete within {}s",
                    init_timeout.as_secs()
                )))
            }
        }
    }

    fn allocate_operation_id(&self) -> u64 {
        self.next_operation_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Register an operation and enqueue it on the bounded queue. Full queue →
    /// [`RError::QueueFull`]; closed queue → [`RError::SessionClosed`]; the
    /// registry entry is rolled back on failure.
    pub(crate) fn enqueue(
        &self,
        retained: bool,
        make: impl FnOnce() -> OpKind,
    ) -> Result<u64, RError> {
        let worker_guard = lock(&self.worker);
        let worker = worker_guard.as_ref().ok_or(RError::SessionClosed)?;
        let id = self.allocate_operation_id();
        let token = new_token();
        lock(&self.table).register(id, token.clone(), retained);

        let command = SessionCommand::Op {
            op: OpHandle { id, token },
            kind: make(),
        };
        match worker.try_send(command) {
            Ok(()) => Ok(id),
            Err(TrySendError::Full(_)) => {
                lock(&self.table).forget(id);
                Err(RError::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => {
                lock(&self.table).forget(id);
                Err(RError::SessionClosed)
            }
        }
    }

    fn request<T>(
        &self,
        make: impl FnOnce(Sender<Result<T, RError>>) -> OpKind,
    ) -> Result<T, RError> {
        self.request_with_timeout(make, DEFAULT_REQUEST_TIMEOUT)
    }

    /// Synchronous request path: enqueue, then wait with a deadline. Timeout
    /// maps to [`RError::Timeout`] and requests cancellation of the affected
    /// operation so the single worker thread does not stay wedged behind it.
    pub(crate) fn request_with_timeout<T>(
        &self,
        make: impl FnOnce(Sender<Result<T, RError>>) -> OpKind,
        timeout: Duration,
    ) -> Result<T, RError> {
        let (reply_tx, reply_rx) = channel();
        let op_id = self.enqueue(false, || make(reply_tx))?;
        match reply_rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                lock(&self.table).cancel_operation(op_id);
                Err(RError::Timeout {
                    after_ms: timeout.as_millis() as u64,
                })
            }
            Err(RecvTimeoutError::Disconnected) => Err(RError::SessionClosed),
        }
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        self.shutdown_worker();
        self.dispatcher.shutdown();
    }
}
