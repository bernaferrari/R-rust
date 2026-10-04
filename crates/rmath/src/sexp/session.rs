//! R session context — an explicit context for R operations.
//!
//! Instead of relying solely on scattered thread-local globals, this struct
//! provides a unified interface to all R interpreter state.
//!
//! # Overview
//!
//! [`RSession`] encapsulates an [`RInstance`] that owns its own arena,
//! protection stack, and environment state, enabling multiple independent
//! R sessions to coexist within the same process when each session stays on
//! the thread that created it.
//!
//! # Examples
//!
//! ```text
//! use crate::sexp::RSession;
//!
//! let session = RSession::new();
//! assert!(session.is_active());
//! assert!(session.global_env().is_some());
//! ```
//!
//! # Lifecycle
//!
//! Sessions are created with [`RSession::new`] and can be closed with
//! [`RSession::close`]. Once closed, evaluation and variable definition
//! operations become no-ops or return errors.

use std::cell::UnsafeCell;
use std::ffi::{CStr, CString};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(test)]
use std::ptr;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::context::{RError, RSignal};
use super::envir::Environment;
use super::ffi::SEXP;
#[cfg(test)]
use super::ffi::SEXPTYPE;
use super::globals::{R_NilValue, R_UnboundValue};
use super::instance::{
    RInstance, clear_current_instance_if, replace_current_instance, set_current_instance,
};
use super::memory::{ArenaBudget, RArena};
use super::object::{SessionNodeFactory, Sexp};
#[cfg(test)]
use super::protect::R_ProtectCount;
#[cfg(test)]
use super::protect::protect;
use rmath_nmath::rng::{detach_rng, install_rng};
use rmath_nmath::{MathState, RngState, detach_state, install_state};

mod retained;
pub use retained::RetainedValueId;

/// Error returned by safe session evaluation APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct REvalError {
    pub message: String,
}

impl std::fmt::Display for REvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for REvalError {}

pub type RResult<T> = Result<T, REvalError>;

/// Shareable cooperative cancellation handle for an evaluation.
///
/// The atomic storage is intentionally private: embedding layers get an
/// ownership-oriented token with simple request/reset/query operations instead
/// of depending on the interpreter's synchronization detail.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    requested: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Create a token with cancellation initially clear.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a token that has already requested cancellation.
    pub fn cancelled() -> Self {
        let token = Self::new();
        token.request();
        token
    }

    /// Request cancellation. Active evaluations observe this cooperatively.
    pub fn request(&self) {
        self.requested.store(true, Ordering::Relaxed);
    }

    /// Clear a previous cancellation request so the token can be reused.
    pub fn reset(&self) {
        self.requested.store(false, Ordering::Relaxed);
    }

    /// Return whether cancellation has been requested.
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        CancellationToken {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn install_symbol(name: &str) -> Option<SEXP> {
    let name = CString::new(name).ok()?;
    let symbol = unsafe { crate::sexp::symbol::Rf_install(name.as_ptr()) };
    if symbol.is_null() { None } else { Some(symbol) }
}

fn catch_eval_result<'a, F>(f: F) -> RResult<Sexp<'a>>
where
    F: FnOnce() -> Result<Sexp<'a>, String>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value.map_err(|message| REvalError { message }),
        Err(payload) => match payload.downcast::<RSignal>() {
            Ok(signal) => match *signal {
                RSignal::Error { message } => Err(REvalError { message }),
                // A break/next escaping every loop context is an R-level
                // error (upstream: "no loop for break/next, jumping to
                // the top level"), never an escaping Rust panic — the
                RSignal::Break | RSignal::Next => Err(REvalError {
                    message: "no loop for break/next, jumping to the top level".to_string(),
                }),
                // Top-level return(value) is likewise an R-level error
                // (upstream: "no function to return from, jumping to
                // top level"), never an escaping panic.
                RSignal::Return(ticket) => {
                    ticket.discard();
                    Err(REvalError {
                        message: "no function to return from, jumping to top level".to_string(),
                    })
                }
                RSignal::Abort => Err(REvalError {
                    message: "execution aborted".to_string(),
                }),
                // Restart requests own thread-confined GC guards. Even an
                // invalid transfer must be consumed while this session is active,
                // never exposed as a movable panic payload to safe host code.
                RSignal::Restart(ticket) => {
                    ticket.discard();
                    Err(REvalError {
                        message: "restart not on stack".to_string(),
                    })
                }
                RSignal::Jump(ticket) | RSignal::ExitingHandler(ticket) => {
                    ticket.discard();
                    Err(REvalError {
                        message: "unhandled nonlocal evaluation transfer".to_string(),
                    })
                }
                other => std::panic::panic_any(other),
            },
            Err(payload) => match payload.downcast::<RError>() {
                Ok(err) => Err(REvalError {
                    message: err.message.clone(),
                }),
                Err(payload) => std::panic::resume_unwind(payload),
            },
        },
    }
}

fn print_panic_r_error(payload: &(dyn std::any::Any + Send)) -> Option<REvalError> {
    let message = if let Some(err) = payload.downcast_ref::<RError>() {
        err.message.clone()
    } else if let Some(RSignal::Error { message }) = payload.downcast_ref::<RSignal>() {
        message.clone()
    } else {
        return None;
    };
    Some(REvalError { message })
}

fn remember_last_value(value: SEXP) {
    unsafe {
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(c".Last.value".as_ptr()),
            value,
            crate::sexp::globals::R_GlobalEnv(),
        );
    }
}

fn expr_or_nil(expr: SEXP) -> SEXP {
    if expr.is_null() {
        unsafe { R_NilValue() }
    } else {
        expr
    }
}

/// Physical location cleanup belongs to the original runtime even after close.
struct ToplevelExprNoGuard {
    owner: super::owner::OwnerPin,
}
impl ToplevelExprNoGuard {
    fn new(session: &RSession) -> Self {
        Self {
            owner: session
                .owner_token()
                .expect("active script owner")
                .weak_owner()
                .expect("managed script owner")
                .pin()
                .expect("live script owner"),
        }
    }

    fn require_active(&self) -> RResult<()> {
        self.owner.require_live().map_err(|error| REvalError {
            message: error.to_string(),
        })?;
        if super::instance::current_instance_ptr() != Some(self.owner.as_ptr()) {
            return Err(REvalError {
                message: super::object::SexpError::OwnerNotActive.to_string(),
            });
        }
        Ok(())
    }

    fn run_checked<T>(&self, operation: impl FnOnce() -> T) -> RResult<T> {
        self.require_active()?;
        let outcome = catch_unwind(AssertUnwindSafe(operation));
        self.require_active()?;
        match outcome {
            Ok(value) => Ok(value),
            Err(payload) => match print_panic_r_error(payload.as_ref()) {
                Some(error) => Err(error),
                None => std::panic::resume_unwind(payload),
            },
        }
    }
}
impl Drop for ToplevelExprNoGuard {
    fn drop(&mut self) {
        unsafe {
            (*self.owner.as_ptr()).error_state.toplevel_expr_no = 0;
        }
    }
}

struct CurrentInstanceGuard {
    transfer_scope: Option<super::transfer::TransferScopeGuard>,
    /// Keep actual runtime bytes allocated through callback and restoration.
    _active_pin: Option<super::owner::OwnerPin>,
    _previous_pin: Option<super::owner::OwnerPin>,
    previous: Option<*mut RInstance>,
    previous_liveness: Option<super::instance::InstanceLiveness>,
    previous_rng: Option<*mut rmath_nmath::RngState>,
    previous_state: Option<*mut rmath_nmath::MathState>,
}

impl CurrentInstanceGuard {
    /// Suspend ambient dispatch while constructing an independent runtime.
    /// Retain the original previous allocation through bootstrap callbacks,
    /// and restore only its still-live capability on success or unwind.
    fn detached() -> Self {
        let previous = super::instance::current_instance_ptr();
        let previous_pin = previous.and_then(|pointer| unsafe {
            (*pointer)
                .runtime_owner
                .as_ref()
                .and_then(|owner| owner.pin().ok())
        });
        let previous_liveness =
            previous.map(|pointer| unsafe { super::instance::instance_liveness(pointer) });
        let previous_rng = unsafe { rmath_nmath::rng::swap_rng(None) };
        let previous_state = rmath_nmath::state::take_state();
        unsafe { replace_current_instance(None) };
        Self {
            transfer_scope: None,
            _active_pin: None,
            _previous_pin: previous_pin,
            previous,
            previous_liveness,
            previous_rng,
            previous_state,
        }
    }

    unsafe fn new(instance: *mut RInstance) -> Self {
        let owner = unsafe { (*instance).runtime_owner.clone() };
        let active_pin = owner
            .clone()
            .map(|owner| owner.pin().expect("cannot activate a closed R owner"));
        let previous = super::instance::current_instance_ptr();
        let previous_pin = previous.and_then(|pointer| unsafe {
            (*pointer)
                .runtime_owner
                .as_ref()
                .and_then(|owner| owner.pin().ok())
        });
        let previous_liveness =
            previous.map(|owner| unsafe { super::instance::instance_liveness(owner) });
        unsafe { replace_current_instance(Some(instance)) };
        // Scope the nmath RNG to this session for the duration of the
        // activation, mirroring the instance swap above: session-owned
        // streams must not leak across concurrently-live sessions.
        let previous_rng = unsafe {
            rmath_nmath::rng::swap_rng(Some(
                &mut (*instance).rng_state as *mut rmath_nmath::RngState,
            ))
        };
        // Scope the nmath sampler state (rgamma/beta/... caches) the same
        // way; without this a detached session's sampler state resets on
        // every access and stateful algorithms like rgamma's GD loop fail.
        let previous_state = unsafe {
            rmath_nmath::state::replace_state(
                &mut (*instance).math_state as *mut rmath_nmath::MathState,
            )
        };
        let mut guard = CurrentInstanceGuard {
            transfer_scope: None,
            _active_pin: active_pin,
            _previous_pin: previous_pin,
            previous,
            previous_liveness,
            previous_rng,
            previous_state,
        };
        guard.transfer_scope = owner.map(|owner| {
            super::transfer::TransferScopeGuard::enter(owner)
                .expect("cannot enter closed transfer scope")
        });
        guard
    }
}

/// Scope a translated operation to its explicit owner and restore instance,
/// RNG and numerical state on every exit, including an R error unwind.
///
/// # Safety
/// The owner is live on entry; no whole-instance or payload borrow may
/// overlap reentry. Callers using it after a callback must retain a session
/// borrow or check its teardown identity. The guard restores prior state only
/// while that prior owner remains live.
pub(crate) unsafe fn with_instance_active<T>(instance: *mut RInstance, f: impl FnOnce() -> T) -> T {
    let _guard = unsafe { CurrentInstanceGuard::new(instance) };
    f()
}

impl Drop for CurrentInstanceGuard {
    fn drop(&mut self) {
        drop(self.transfer_scope.take());
        // Callback code can destroy an ambient owner stored in thread-local
        // user state. Never reinstall any of that owner's now dangling state.
        let previous_alive = self
            .previous_liveness
            .as_ref()
            .is_none_or(|owner| owner.is_live());
        unsafe {
            replace_current_instance(if previous_alive { self.previous } else { None });
            rmath_nmath::rng::swap_rng(if previous_alive {
                self.previous_rng
            } else {
                None
            });
            rmath_nmath::state::restore_state(if previous_alive {
                self.previous_state
            } else {
                None
            });
        }
    }
}

/// Records owned commands while forwarding to the live device. No borrow of
/// the recording survives a draw call, so recordPlot can snapshot it reentrantly.
#[cfg(feature = "renderplot-device")]
struct RecordingTarget<'a> {
    target: &'a mut dyn r_graphics_engine::DrawTarget,
    recording: std::rc::Rc<std::cell::RefCell<r_graphics_engine::Scene>>,
}
#[cfg(feature = "renderplot-device")]
impl r_graphics_engine::DrawTarget for RecordingTarget<'_> {
    fn dimensions(&self) -> (u32, u32) {
        self.target.dimensions()
    }
    fn measure_text(
        &self,
        text: &str,
        params: &r_graphics_engine::PlotParameters,
    ) -> r_graphics_engine::TextMetrics {
        self.target.measure_text(text, params)
    }
    fn clear(&mut self, color: r_graphics_engine::Color) {
        self.recording.borrow_mut().clear(color);
        self.target.clear(color);
    }
    fn set_clip(&mut self, clip: Option<[f32; 4]>) {
        self.recording.borrow_mut().set_clip(clip);
        self.target.set_clip(clip);
    }
    fn draw_path(&mut self, path: &r_graphics_engine::Path) {
        self.recording.borrow_mut().draw_path(path);
        self.target.draw_path(path);
    }
    fn draw_image(
        &mut self,
        image: &r_graphics_engine::RasterImage,
        transform: [f64; 6],
        interpolate: bool,
    ) {
        self.recording
            .borrow_mut()
            .draw_image(image, transform, interpolate);
        self.target.draw_image(image, transform, interpolate);
    }
    fn draw_text(
        &mut self,
        text: &str,
        point: r_graphics_engine::Point,
        params: &r_graphics_engine::PlotParameters,
    ) {
        self.recording.borrow_mut().draw_text(text, point, params);
        self.target.draw_text(text, point, params);
    }
}

#[cfg(feature = "renderplot-device")]
struct RenderPlotBackendGuard {
    instance: *mut RInstance,
    pin: Option<super::owner::OwnerPin>,
    availability: super::instance::InstanceLiveness,
    previous: Option<*mut dyn r_graphics_engine::DrawTarget>,
}

#[cfg(feature = "renderplot-device")]
impl RenderPlotBackendGuard {
    fn install<'a>(
        instance: *mut RInstance,
        backend: *mut (dyn r_graphics_engine::DrawTarget + 'a),
    ) -> Self {
        let pin = unsafe { (*instance).runtime_owner.clone() }
            .map(|owner| owner.pin().expect("live RenderPlot owner"));
        let availability = unsafe { super::instance::instance_liveness(instance) };
        // SAFETY: lifetime-erasing the backend pointer to 'static for storage
        // in the instance slot. This is sound because the guard's Drop
        // restores the previous slot value before the caller's 'a borrow ends,
        // so the erased pointer can never be observed after it dangles.
        #[allow(clippy::transmute_ptr_to_ptr)] // fat-pointer lifetime erasure; `as` cannot do this
        let erased: *mut (dyn r_graphics_engine::DrawTarget + 'static) =
            unsafe { std::mem::transmute(backend) };
        // P2: strictly-local Cell access; no ambient write intervenes.
        let previous = unsafe { (*instance).current_renderplot_backend.replace(erased) };
        Self {
            instance,
            pin,
            availability,
            previous,
        }
    }
}

#[cfg(feature = "renderplot-device")]
impl Drop for RenderPlotBackendGuard {
    fn drop(&mut self) {
        let instance = match &self.pin {
            Some(pin) => pin.as_ptr(),
            None if self.availability.is_live() => self.instance,
            None => return,
        };
        unsafe {
            (*instance).current_renderplot_backend = self.previous;
        }
    }
}

struct ProtectScope {
    /// Original shared-cell projection, never reconstituted from an address.
    instance: std::ptr::NonNull<RInstance>,
    /// Weak availability observation; scopes do not retain closed owners.
    liveness: super::instance::InstanceLiveness,
    legacy_depth: usize,
    root_depth: u64,
}

impl ProtectScope {
    fn new(instance: *mut RInstance) -> Self {
        let liveness = unsafe { super::instance::instance_liveness(instance) };
        let (legacy_depth, root_depth) = unsafe {
            (
                (*instance).legacy_protect.len(),
                (*instance).root_table.checkpoint(),
            )
        };
        Self {
            instance: std::ptr::NonNull::new(instance).expect("live protection owner"),
            liveness,
            legacy_depth,
            root_depth,
        }
    }
}

impl Drop for ProtectScope {
    fn drop(&mut self) {
        if !self.liveness.is_live() {
            return;
        }
        let instance = self.instance.as_ptr();
        // The observation is weak, and no callback occurs between this check
        // and these short storage operations.
        unsafe {
            (*instance).legacy_protect.truncate(self.legacy_depth);
            (*instance).root_table.restore(self.root_depth);
        }
    }
}

/// An R interpreter session with its own isolated instance state.
///
/// Each `RSession` owns an [`RInstance`] containing a private arena,
/// environment chain, and protection stack. When a session is active,
/// all compatibility accessor functions (`R_GlobalEnv`, protection APIs,
/// `with_arena`, etc.) dispatch to the session's instance.
///
/// # Thread Safety
///
/// `RSession` is thread-confined. Each worker thread should create and keep its
/// own session instance; moving a live session across threads would invalidate
/// the thread-local compatibility dispatch pointer.
pub struct RSession {
    /// Whether this session is active.
    active: bool,
    /// Host values own their physical leases outside the R binding graph.
    retained_values: retained::RetainedValues,
    /// Native projection of the shared interior cell owned below.
    instance: *mut RInstance,
    /// Physical allocation authority. Operation guards clone this original
    /// allocation, so callbacks cannot destroy active runtime bytes.
    _instance_owner: Rc<UnsafeCell<RInstance>>,
    /// Marker that keeps sessions thread-confined at compile time.
    _thread_confined: PhantomData<Rc<()>>,
}

impl RSession {
    /// Bound per-evaluation captured output. `None` preserves the native
    /// unbounded behavior; embedders such as Wasm set a finite limit.
    pub fn set_output_limit(&mut self, max_bytes: Option<usize>) {
        self.inst()
            .output_capture
            .borrow_mut()
            .set_max_bytes(max_bytes);
    }

    /// Create a new R session with its own isolated instance.
    ///
    /// Initializes a fresh [`RInstance`] with its own arena and environment
    /// chain, and installs it as the current compatibility instance on this
    /// thread. Session methods still scope activation explicitly, so nested
    /// operations restore the previous instance.
    pub fn new() -> Self {
        Self::new_with_default_packages(true)
    }

    /// Build the real base runtime without host package discovery. Collector
    /// unit tests use this to avoid loading unrelated installed R packages.
    #[cfg(test)]
    pub(crate) fn new_without_default_packages() -> Self {
        Self::new_with_instance(RInstance::new_for_gc_tests(), false, true)
    }

    fn new_with_default_packages(attach_default_packages: bool) -> Self {
        super::context::install_r_panic_hook();
        Self::new_with_instance(
            RInstance::allocate_for_session(),
            attach_default_packages,
            true,
        )
    }

    #[cfg(test)]
    pub(crate) fn new_for_gc_tests() -> Self {
        Self::new_with_instance(RInstance::new_for_gc_tests(), false, false)
    }

    fn new_with_instance(
        instance: RInstance,
        attach_default_packages: bool,
        initialize_base: bool,
    ) -> Self {
        super::context::install_r_panic_hook();
        let instance_owner = Rc::new(UnsafeCell::new(instance));
        let instance = instance_owner.get();
        // Install the capability before bootstrap can create any values or
        // execute callbacks. Every managed value refers to this original Rc.
        unsafe {
            (*instance).runtime_owner = Some(super::owner::WeakOwner::from_rc(&instance_owner));
        }
        // Own the allocation before installing it, so initialization unwind
        // also detaches the thread-local runtime state through normal Drop.
        let session = RSession {
            active: true,
            retained_values: retained::RetainedValues::default(),
            instance,
            _instance_owner: instance_owner,
            _thread_confined: PhantomData,
        };
        unsafe {
            set_current_instance(instance);
            install_state(&mut (*instance).math_state as *mut MathState);
            install_rng(&mut (*instance).rng_state as *mut RngState);
            // Route the nmath samplers through the full R-level RNG dispatch
            // (all RNG kinds, `.Random.seed` state). The bridge resolves the
            // *current* instance dynamically, so one installation per thread
            // serves every session and falls back to the standalone
            // MultiCarry stream when no instance is active.
            rmath_nmath::rng::set_unif_hook(Some(crate::mainutils::random::nmath_unif_hook));
            // Route the nmath rbinom() algorithm selection through the
            // session's binom.kind (RNGkind(binom.kind=...) / set.seed).
            rmath_nmath::rng::set_binom_kind_hook(Some(
                crate::mainutils::random::nmath_binom_kind_hook,
            ));
            // Route nmath's MATHLIB_WARNING (ML_WARNING's range/precision
            // messages, dpq.h's "non-integer x = %f") through the R warning
            // machinery, so mathlib warnings are catchable and
            // deferred-printed like stock (tryCatch(warning=), warn=1).
            rmath_nmath::error::set_warning_hook(Some(
                crate::mainutils::errors::nmath_warning_hook,
            ));
        }
        if initialize_base {
            unsafe { RInstance::initialize_base_bindings_via(instance) };
        }
        if !attach_default_packages {
            return session;
        }
        session.with_active(|| unsafe {
            let factory = SessionNodeFactory::new(session.owner_token().expect("active session"));
            // GNU defaultPackages: datasets, utils, grDevices, graphics,
            // stats, methods. library() inserts at pos 2, so attach in
            // that order and stats sits just under .GlobalEnv.
            let mut stats_loaded = false;
            for package in ["methods", "datasets", "utils", "grDevices", "graphics", "stats"] {
                let path = crate::mainutils::essentials::find_package_path(package);
                if !path.is_empty() {
                    let loaded = crate::mainutils::essentials::load_pure_r_package(
                        package,
                        std::path::Path::new(&path),
                    )
                    .is_ok();
                    if package == "stats" {
                        stats_loaded = loaded;
                    }
                }
            }
            if stats_loaded && let Ok(exprs) = super::memory::with_arena(|arena| {
                crate::eval::parser::parse_expressions(
                    "library(grDevices); library(graphics); detach(\"package:stats\"); library(stats)",
                    arena,
                    factory.clone(),
                )
            }) {
                for expr in exprs {
                    let _ = crate::eval::eval::Rf_eval(
                        expr.clone().as_raw(),
                        crate::sexp::globals::R_GlobalEnv(),
                    );
                }
            }
            if stats_loaded && let Ok(exprs) = super::memory::with_arena(|arena| {
                crate::eval::parser::parse_expressions(
                    "{ if (\"package:stats\" %in% search()) { assign(\"reorder\", get(\"reorder\", baseenv()), envir = as.environment(\"package:stats\")); if (bindingIsLocked(\"xtabs\", as.environment(\"package:stats\"))) unlockBinding(\"xtabs\", as.environment(\"package:stats\")); assign(\"xtabs\", get(\"xtabs\", baseenv()), envir = as.environment(\"package:stats\")) }; f <- get(\"diff.ts\", baseenv()); for (env in list(baseenv(), get(\".BaseNamespaceEnv\", baseenv()), asNamespace(\"stats\"), as.environment(\"package:stats\"))) { tab <- tryCatch(get(\".__S3MethodsTable__.\", env), error = function(e) NULL); if (!is.null(tab) && exists(\"diff.ts\", tab, inherits = FALSE)) { if (bindingIsLocked(\"diff.ts\", tab)) unlockBinding(\"diff.ts\", tab); assign(\"diff.ts\", f, tab) }; if (exists(\"diff.ts\", env, inherits = FALSE)) { if (bindingIsLocked(\"diff.ts\", env)) unlockBinding(\"diff.ts\", env); assign(\"diff.ts\", f, env) } } }",
                    arena,
                    factory.clone(),
                )
            }) {
                if let Some(expr) = exprs.first() {
                    let _ = crate::eval::eval::Rf_eval(
                        expr.clone().as_raw(),
                        crate::sexp::globals::R_GlobalEnv(),
                    );
                }
            }
        });

        session
    }

    /// Create a session without leaving it installed as the thread's ambient
    /// compatibility instance.
    ///
    /// This is the preferred constructor for app-facing embedding layers.
    /// Session methods still activate the instance for legacy internals, but
    /// merely constructing an Android worker session no longer changes what
    /// unrelated translated code on the same thread sees as current.
    pub(crate) fn new_detached() -> Self {
        Self::construct_detached(Self::new)
    }

    fn construct_detached(constructor: impl FnOnce() -> Self) -> Self {
        let _ambient = CurrentInstanceGuard::detached();
        let session = constructor();
        detach_state(unsafe { &(*session.instance).math_state });
        detach_rng(unsafe { &(*session.instance).rng_state });
        clear_current_instance_if(session.instance);
        session
    }

    /// Short immutable view of the owned interior cell.
    #[inline]
    fn inst(&self) -> &RInstance {
        // SAFETY: the Rc-owned cell remains allocated for this session.
        // Callers keep field borrows local and end them before R callbacks.
        unsafe { &*self.instance }
    }

    fn instance_ptr(&self) -> *mut RInstance {
        self.instance
    }

    fn activate(&self) -> CurrentInstanceGuard {
        assert!(self.is_active(), "cannot activate a closed R session");
        // SAFETY: the session retains the owner for the guard's lifetime.
        unsafe { CurrentInstanceGuard::new(self.instance_ptr()) }
    }

    pub fn enable_browser_files(&mut self) {
        let _guard = self.activate();
        unsafe {
            (*self.instance).browser_files_enabled = true;
        }
    }

    pub fn put_browser_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let _guard = self.activate();
        unsafe {
            let result = (*self.instance).browser_files.put(path, bytes);
            if result.is_ok() {
                (*self.instance).browser_files_enabled = true;
            }
            result
        }
    }
    pub fn get_browser_file(&mut self, path: &str) -> Option<Vec<u8>> {
        let _guard = self.activate();
        unsafe {
            (*self.instance)
                .browser_files
                .read(path)
                .map(<[u8]>::to_vec)
        }
    }
    pub fn list_browser_files(&mut self) -> Vec<String> {
        let _guard = self.activate();
        unsafe { (*self.instance).browser_files.names() }
    }
    pub fn remove_browser_file(&mut self, path: &str) -> bool {
        let _guard = self.activate();
        unsafe { (*self.instance).browser_files.remove(path) }
    }

    pub(crate) fn with_active<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        self.with_active_in(|_| f())
    }

    /// Pass the owning instance through a scoped activation.
    ///
    /// A raw owner permits interpreter reentry without holding a protected
    /// `&mut RInstance` borrow. Callers must use short field borrows (P1/P2),
    /// and must not retain the pointer beyond this session's lifetime.
    pub(crate) fn with_active_in<F, T>(&self, f: F) -> T
    where
        F: FnOnce(*mut RInstance) -> T,
    {
        let _guard = self.activate();
        f(self.instance)
    }

    /// Retain this session lifetime without borrowing the RInstance itself.
    pub(crate) fn owner_token(&self) -> Option<super::owner::OwnerToken<'_>> {
        if !self.is_active() {
            return None;
        }
        // SAFETY: self retains the original allocation; the return lifetime
        // prevents closing or moving this session while the token is used.
        Some(unsafe { super::owner::OwnerToken::from_raw(self.instance) })
    }

    /// Check if this session is active.
    ///
    /// Returns `false` after close or revocation by an original-runtime callback.
    pub fn is_active(&self) -> bool {
        // The session retains physical bytes; this callback-free availability
        // snapshot never adopts a runtime from the ambient session stack.
        self.active && unsafe { super::instance::instance_liveness(self.instance).is_live() }
    }

    /// Get the global environment.
    ///
    /// Returns `None` if the global environment pointer is null.
    pub fn global_env(&self) -> Option<Sexp<'_>> {
        self.sexp(self.inst().global_env)
    }

    /// Snapshot the binding names of the global environment as owned strings.
    ///
    /// Walks the frame pairlist exactly like R's `ls(all.names = TRUE)`:
    /// every bound (non-`R_UnboundValue`) symbol, dot-prefixed names included,
    /// sorted lexicographically. No raw `SEXP` crosses the boundary.
    pub fn global_binding_names(&mut self) -> Vec<String> {
        self.with_active(|| unsafe {
            let mut names = Vec::new();
            let env = self.inst().global_env;
            let mut frame = crate::sexp::accessors::FRAME(env);
            while !frame.is_null() && frame != R_NilValue() {
                let value = crate::sexp::accessors::CAR(frame);
                if value != R_UnboundValue()
                    && let Some(name) = crate::sexp::symbol::symbol_name_from_ptr(
                        crate::sexp::accessors::TAG(frame),
                    )
                {
                    names.push(name);
                }
                frame = crate::sexp::accessors::CDR(frame);
            }
            names.sort();
            names
        })
    }

    /// Get the base environment.
    ///
    /// Returns `None` if the base environment pointer is null.
    pub fn base_env(&self) -> Option<Sexp<'_>> {
        self.sexp(self.inst().base_env)
    }

    /// Wrap a raw pointer if it belongs to this session.
    ///
    /// This is the safe public boundary for turning C-shaped `SEXP` values into
    /// Rust `Sexp` handles. Unknown pointers are rejected; accepted pointers are
    /// owned by this session's arena or persistent instance storage, or are one
    /// of R's process-wide immutable sentinels such as `R_NilValue`.
    pub fn sexp(&self, ptr: SEXP) -> Option<Sexp<'_>> {
        if ptr.is_null() {
            return None;
        }
        if let Some(canonical) = immutable_singleton_projection(ptr) {
            Some(unsafe { Sexp::from_static_raw_unchecked(canonical) })
        } else if self.inst().owns_sexp(ptr) {
            // SAFETY: self owns the original pointer and bounds the returned
            // handle's lifetime. No instance borrow survives root installation.
            self.owner_token()?.sexp(ptr).ok()
        } else {
            None
        }
    }

    fn owned_sexp<'session>(
        &'session self,
        ptr: SEXP,
        description: &'static str,
    ) -> RResult<Sexp<'session>> {
        self.sexp(ptr).ok_or_else(|| REvalError {
            message: format!("{description} does not belong to this session"),
        })
    }

    /// Evaluate an expression in this session's global environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is closed or if evaluation
    /// triggers an R error (e.g., undefined variable, type error).
    ///
    /// Raw pointers that do not belong to this session are rejected before
    /// evaluation. Prefer [`RSession::eval_sexp`] when the caller already has a
    /// lifetime-bound [`Sexp`] handle.
    pub(crate) fn eval(&self, expr: SEXP) -> RResult<SEXP> {
        self.eval_sexp_raw(expr).map(|value| value.as_raw())
    }

    /// Evaluate a raw expression pointer after proving it belongs to this session.
    pub(crate) fn eval_sexp_raw(&self, expr: SEXP) -> RResult<Sexp<'_>> {
        let expr = expr_or_nil(expr);
        let expr = self.owned_sexp(expr, "expression")?;
        self.eval_sexp(expr)
    }

    /// Evaluate an expression and return a session-scoped safe wrapper.
    pub fn eval_sexp<'session>(&'session self, expr: Sexp<'_>) -> RResult<Sexp<'session>> {
        if !self.is_active() {
            return Err(REvalError {
                message: "session is closed".to_string(),
            });
        }

        self.with_active(|| {
            crate::mainutils::errors::clear_last_rendered_message();
            let expr = self.owned_sexp(expr.as_raw(), "expression")?;
            let env = self.global_env().ok_or_else(|| REvalError {
                message: "session has no global environment".to_string(),
            })?;
            let result = catch_eval_result(|| unsafe { /* SAFETY: session activates its checked owner; unsafe payload loans must exclude R reentry. */ crate::eval::eval::EvalContext::new(env).eval(expr) })?;
            self.owned_sexp(result.as_raw(), "evaluation result")
        })
    }

    /// Evaluate an expression while capturing output and the final visibility flag.
    ///
    /// This mirrors the top-level embedding contract: explicit output produced by
    /// functions such as `print()` and `cat()` is captured separately from the
    /// implicit printing controlled by `R_Visible`.
    pub(crate) fn eval_with_output_capture(
        &self,
        expr: SEXP,
    ) -> (RResult<SEXP>, super::output::RCapturedOutput, bool) {
        let expr = match self.owned_sexp(expr_or_nil(expr), "expression") {
            Ok(expr) => expr,
            Err(err) => {
                return (Err(err), super::output::RCapturedOutput::default(), false);
            }
        };
        let (result, output, visible) = self.eval_sexp_with_output_capture(expr);
        (result.map(|value| value.as_raw()), output, visible)
    }

    /// Evaluate an expression while capturing output and returning a typed
    /// session-scoped result.
    pub fn eval_sexp_with_output_capture<'session>(
        &'session self,
        expr: Sexp<'_>,
    ) -> (
        RResult<Sexp<'session>>,
        super::output::RCapturedOutput,
        bool,
    ) {
        if !self.is_active() {
            return (
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                super::output::RCapturedOutput::default(),
                false,
            );
        }
        self.with_active(|| {
            self.inst().output_capture.borrow_mut().start();
            let result = self.eval_sexp(expr);
            let visible = self.inst().eval_state.visible != 0;
            let output = self.inst().output_capture.borrow_mut().stop();
            (result, output, visible)
        })
    }

    /// Parse and evaluate source code while capturing output and visibility.
    ///
    /// This keeps embedders on the owner-checked `Sexp` path instead of asking
    /// them to parse into a raw `SEXP` and then prove ownership themselves.
    pub fn eval_code_with_output_capture<'session>(
        &'session mut self,
        code: &str,
    ) -> (
        RResult<Sexp<'session>>,
        super::output::RCapturedOutput,
        bool,
    ) {
        self.eval_code_with_output_capture_then(code, |result, output, visible| {
            (result, output, visible)
        })
    }

    /// Print a top-level script error and keep evaluating when
    /// `catch.script.errors` is set.
    ///
    /// Returns true when the caller should continue the script loop. An Ok
    /// result, and an error while the option is off, both return false so the
    /// caller can stop on the error.
    fn caught_script_error_continues<'a>(
        &self,
        result: &mut RResult<Sexp<'a>>,
        raw_expr: SEXP,
    ) -> bool {
        if result.is_ok() {
            return false;
        }
        let catch_script =
            unsafe { crate::mainutils::options::logical_option_enabled(c"catch.script.errors") };
        if !catch_script {
            return false;
        }
        let message = result.as_ref().unwrap_err().message.clone();
        let text = crate::mainutils::errors::try_last_rendered_message(&message)
            .unwrap_or_else(|| format!("Error: {}\n", message));
        // GNU prints the error on stderr and continues.
        super::output::capture_stderr(&text);
        if !text.ends_with('\n') {
            super::output::capture_stderr("\n");
        }
        if crate::mainutils::errors::collect_warnings() > 0 {
            unsafe {
                crate::mainutils::errors::print_warnings_at_statement_boundary();
            }
        }
        *result = Ok(unsafe { Sexp::from_raw_unchecked(R_NilValue()) });
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        unsafe {
            crate::mainutils::main::Rf_callToplevelHandlers(
                raw_expr,
                crate::sexp::globals::R_NilValue(),
                crate::sexp::ffi::FALSE,
                0,
            );
        }
        // SAFETY: activation owns the live instance. No field borrow
        // survives collection or finalizer reentry.
        unsafe {
            crate::sexp::gengc::run_pending_gc_if_quiescent_in(self.instance);
        }
        true
    }

    /// Parse and evaluate source code, then map the result while this session
    /// is still the scoped active instance.
    ///
    /// Embedding facades use this to convert a borrowed `Sexp` into owned
    /// values without depending on any ambient current session after the eval
    /// call has returned.
    /// Parse and evaluate a multi-expression R script.
    pub fn eval_script_with_output_capture<'session>(
        &'session mut self,
        code: &str,
    ) -> (
        RResult<Sexp<'session>>,
        super::output::RCapturedOutput,
        bool,
    ) {
        self.eval_script_with_output_capture_then(code, |result, output, visible| {
            (result, output, visible)
        })
    }

    pub(crate) fn eval_script_with_output_capture_then<'session, T, F>(
        &'session mut self,
        code: &str,
        f: F,
    ) -> T
    where
        F: FnOnce(RResult<Sexp<'session>>, super::output::RCapturedOutput, bool) -> T,
    {
        if !self.is_active() {
            return f(
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                super::output::RCapturedOutput::default(),
                false,
            );
        }

        let expressions = {
            let _guard = self.activate();
            let factory = SessionNodeFactory::new(self.owner_token().expect("active session"));
            let keep_source = unsafe {
                let opt = crate::mainutils::options::GetOption1(crate::sexp::symbol::Rf_install(
                    c"keep.source".as_ptr(),
                ));
                !opt.is_null() && crate::mainutils::coerce::asLogical(opt) == crate::sexp::ffi::TRUE
            };
            let spans = unsafe {
                /* SAFETY: session activates its checked owner; unsafe payload loans must exclude R reentry. */
                super::memory::with_arena_in(self.instance, |arena| {
                    let mut parser = crate::eval::parser::Parser::new(code, arena, factory);
                    // GNU Rscript keeps `keep.source` false, so function bodies
                    // have no srcref. `setGeneric` uses `identical(body, substitute(...))`.
                    parser.set_keep_srcrefs(keep_source);
                    parser.parse_top_level_with_spans()
                })
            };
            let spans = match spans {
                Ok(spans) => spans,
                Err(err) => {
                    let message = err.to_string();
                    return self.with_active(|| {
                        f(
                            Err(REvalError { message }),
                            super::output::RCapturedOutput::default(),
                            false,
                        )
                    });
                }
            };
            let raw_spans: Vec<_> = spans
                .iter()
                .map(|(expr, start, end)| (expr.clone().as_raw(), *start, *end))
                .collect();
            let exprs: Vec<_> = spans.into_iter().map(|(expr, _, _)| expr).collect();
            let vec_sexp = unsafe {
                crate::sexp::constructors::Rf_allocVector3(
                    crate::sexp::ffi::SEXPTYPE::EXPRSXP,
                    exprs.len() as i64,
                )
            };
            let vec_sexp = self.sexp(vec_sexp).expect("session expression vector");
            unsafe {
                for (i, expr) in exprs.iter().enumerate() {
                    crate::sexp::accessors::SET_VECTOR_ELT(
                        vec_sexp.clone().as_raw(),
                        i as i64,
                        expr.clone().as_raw(),
                    );
                }
                if keep_source {
                    crate::mainutils::srcref::attach_srcrefs_with_spans(
                        &raw_spans,
                        code,
                        "<text>",
                        vec_sexp.clone().as_raw(),
                    );
                }
            }
            exprs
        };
        let expressions = expressions;
        self.with_active(|| {
            let capture = super::output::OutputCaptureGuard::start();
            // Stale error-buffer renders from a previous script must not be
            // trusted by this script's top-level error renderer.
            crate::mainutils::errors::clear_last_rendered_message();
            // Every statement owns its automatic allocation lease for the
            // entire loop, including collection and notification callbacks.
            let mut result: RResult<Sexp<'session>> =
                Ok(unsafe { Sexp::from_raw_unchecked(R_NilValue()) });
            let last_index = expressions.len().saturating_sub(1);
            // Upstream's REPL updates the current srcref per top-level
            // expression; the 1-based loop index is the port's location for
            // show.error.locations rendering.
            let _toplevel_no_guard = ToplevelExprNoGuard::new(self);
            for (index, expr) in expressions.iter().enumerate() {
                let raw_expr = expr.clone().as_raw();
                crate::mainutils::errors::set_toplevel_expr_no(index + 1);
                result = _toplevel_no_guard
                    .run_checked(|| self.eval_sexp(expr.clone()))
                    .and_then(|result| result);
                if let Err(error) = _toplevel_no_guard.require_active() {
                    result = Err(error);
                    break;
                }
                if let Ok(value) = result.as_ref() {
                    if let Err(error) = _toplevel_no_guard
                        .run_checked(|| remember_last_value(value.clone().as_raw()))
                    {
                        result = Err(error);
                        break;
                    }
                }
                if let Err(error) = _toplevel_no_guard
                    .run_checked(|| crate::eval::parser::flush_parsed_expr_warnings(index))
                {
                    result = Err(error);
                    break;
                }
                let visible_flag = if self.inst().eval_state.visible != 0 {
                    1
                } else {
                    0
                };
                // main.c REPL loop: upstream auto-prints EVERY visible
                // top-level expression (PrintValueEnv), not just the final
                // one. Intermediate values render through the same formatter
                // result assembly uses for the final value, written into the
                // captured stream so print() side effects, sink diversion,
                // and auto-printed values interleave in statement order. The
                // final statement keeps its caller-side render path, which
                // also flushes that statement's deferred warnings after the
                // value. A show()/print error is the same script error as eval.
                let to_print = if index != last_index && visible_flag != 0 {
                    result.as_ref().ok().cloned()
                } else {
                    None
                };
                if let Some(value) = to_print {
                    if let Err(err) =
                        _toplevel_no_guard.run_checked(|| super::output::print_value(value))
                    {
                        result = Err(err);
                    }
                }
                if let Err(error) = _toplevel_no_guard.require_active() {
                    result = Err(error);
                    break;
                }
                match _toplevel_no_guard
                    .run_checked(|| self.caught_script_error_continues(&mut result, raw_expr))
                {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
                if result.is_err() {
                    break;
                }
                if let Ok(value) = result.as_ref() {
                    if let Err(error) = _toplevel_no_guard.run_checked(|| unsafe {
                        crate::mainutils::main::Rf_callToplevelHandlers(
                            raw_expr,
                            value.clone().as_raw(),
                            crate::sexp::ffi::TRUE,
                            visible_flag,
                        );
                    }) {
                        result = Err(error);
                        break;
                    }
                }
                if let Err(error) = _toplevel_no_guard.require_active() {
                    result = Err(error);
                    break;
                }
                // main.c REPL tail: after each top-level expression, upstream
                // flushes deferred warnings so they interleave with printed
                // output like Rscript's stderr. The final statement defers to
                // result assembly, which prints the auto-rendered value first.
                if index != last_index && crate::mainutils::errors::collect_warnings() > 0 {
                    if let Err(error) = _toplevel_no_guard.run_checked(|| unsafe {
                        crate::mainutils::errors::print_warnings_at_statement_boundary();
                    }) {
                        result = Err(error);
                        break;
                    }
                }
                // SAFETY: activation owns the live instance. No field borrow
                // survives collection or finalizer reentry.
                if let Err(error) = _toplevel_no_guard.run_checked(|| unsafe {
                    crate::sexp::gengc::run_pending_gc_if_quiescent_in(self.instance);
                }) {
                    result = Err(error);
                    break;
                }
            }
            // SAFETY: activation owns the live instance. No field borrow
            // survives collection or finalizer reentry.
            if let Err(error) = _toplevel_no_guard.run_checked(|| unsafe {
                crate::sexp::gengc::run_pending_gc_if_quiescent_in(self.instance);
            }) {
                result = Err(error);
            }
            if let Err(error) = _toplevel_no_guard.require_active() {
                result = Err(error);
            }
            let visible = self.inst().eval_state.visible != 0;
            let output = capture.finish();
            f(result, output, visible)
        })
    }

    #[cfg(feature = "renderplot-device")]
    pub(crate) fn eval_script_with_output_capture_then_renderplot<'session, 'backend, T, F>(
        &'session mut self,
        code: &str,
        backend: *mut (dyn r_graphics_engine::DrawTarget + 'backend),
        f: F,
    ) -> T
    where
        'backend: 'session,
        F: FnOnce(RResult<Sexp<'session>>, super::output::RCapturedOutput, bool) -> T,
    {
        if !self.is_active() {
            return f(
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                super::output::RCapturedOutput::default(),
                false,
            );
        }

        // Capture the original owner's narrow writable fields before parser
        // handles borrow the session. This session borrow retains the owner
        // throughout evaluation and backend-guard cleanup; no field loan
        // crosses a parser, evaluator or drawing callback.
        let instance = self.instance;
        let (graphics_recording, portable_grid) = unsafe {
            (
                std::ptr::addr_of_mut!((*instance).graphics_recording),
                std::ptr::addr_of_mut!((*instance).portable_grid),
            )
        };
        let expressions = {
            let _guard = self.activate();
            // SAFETY: this active session scopes parsing to its arena; no R callback runs.
            let factory = SessionNodeFactory::new(self.owner_token().expect("active session"));
            unsafe {
                super::memory::with_arena_in(instance, |arena| {
                    crate::eval::parser::parse_expressions(code, arena, factory)
                })
            }
        };
        let expressions = match expressions {
            Ok(exprs) => exprs,
            Err(err) => {
                // Same scoped mapping as the plain script path above.
                let message = err.to_string();
                return self.with_active(|| {
                    f(
                        Err(REvalError { message }),
                        super::output::RCapturedOutput::default(),
                        false,
                    )
                });
            }
        };

        {
            let _guard = self.activate();
            // SAFETY: the caller lends the backend for this synchronous evaluation;
            // the installed forwarding pointer is removed before its local owner drops.
            let target = unsafe { &mut *backend };
            let (width, height) = target.dimensions();
            let recording = std::rc::Rc::new(std::cell::RefCell::new(
                r_graphics_engine::Scene::new(width, height),
            ));
            unsafe {
                *graphics_recording = Some(recording.clone());
                *portable_grid = crate::mainutils::portable_grid::GridState::default();
            }
            let mut forwarding = RecordingTarget { target, recording };
            let _backend_guard = RenderPlotBackendGuard::install(instance, &mut forwarding);
            let capture = super::output::OutputCaptureGuard::start();
            // Remaining parsed statements retain their automatic leases.
            let mut result: RResult<Sexp<'session>> =
                Ok(unsafe { Sexp::from_raw_unchecked(R_NilValue()) });
            let last_index = expressions.len().saturating_sub(1);
            // Same per-expression location as the plain script loop above.
            let _toplevel_no_guard = ToplevelExprNoGuard::new(self);
            for (index, expr) in expressions.iter().enumerate() {
                let raw_expr = expr.clone().as_raw();
                crate::mainutils::errors::set_toplevel_expr_no(index + 1);
                result = _toplevel_no_guard
                    .run_checked(|| self.eval_sexp(expr.clone()))
                    .and_then(|result| result);
                if let Err(error) = _toplevel_no_guard.require_active() {
                    result = Err(error);
                    break;
                }
                if let Ok(value) = result.as_ref() {
                    if let Err(error) = _toplevel_no_guard
                        .run_checked(|| remember_last_value(value.clone().as_raw()))
                    {
                        result = Err(error);
                        break;
                    }
                }
                if let Err(error) = _toplevel_no_guard
                    .run_checked(|| crate::eval::parser::flush_parsed_expr_warnings(index))
                {
                    result = Err(error);
                    break;
                }
                // Same per-expression auto-print as the plain script loop
                // above: every visible non-final top-level statement renders
                // into the captured stream, preserving print()/auto-print
                // interleaving; the final statement renders at result
                // assembly. A print error is the same script error as eval.
                let to_print = if index != last_index && self.inst().eval_state.visible != 0 {
                    result.as_ref().ok().cloned()
                } else {
                    None
                };
                if let Some(value) = to_print {
                    if let Err(err) =
                        _toplevel_no_guard.run_checked(|| super::output::print_value(value))
                    {
                        result = Err(err);
                    }
                }
                if let Err(error) = _toplevel_no_guard.require_active() {
                    result = Err(error);
                    break;
                }
                match _toplevel_no_guard
                    .run_checked(|| self.caught_script_error_continues(&mut result, raw_expr))
                {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
                if result.is_err() {
                    break;
                }
                if let Ok(value) = result.as_ref() {
                    let visible_flag = if self.inst().eval_state.visible != 0 {
                        1
                    } else {
                        0
                    };
                    if let Err(error) = _toplevel_no_guard.run_checked(|| unsafe {
                        crate::mainutils::main::Rf_callToplevelHandlers(
                            raw_expr,
                            value.clone().as_raw(),
                            crate::sexp::ffi::TRUE,
                            visible_flag,
                        );
                    }) {
                        result = Err(error);
                        break;
                    }
                }
                if let Err(error) = _toplevel_no_guard.require_active() {
                    result = Err(error);
                    break;
                }
                // Same main.c REPL-tail flush as the plain script loop; the
                // final statement's warnings flush at result assembly.
                if index != last_index && crate::mainutils::errors::collect_warnings() > 0 {
                    if let Err(error) = _toplevel_no_guard.run_checked(|| unsafe {
                        crate::mainutils::errors::print_warnings_at_statement_boundary();
                    }) {
                        result = Err(error);
                        break;
                    }
                }
                // SAFETY: activation owns the live instance. No field borrow
                // survives collection or finalizer reentry.
                if let Err(error) = _toplevel_no_guard.run_checked(|| unsafe {
                    crate::sexp::gengc::run_pending_gc_if_quiescent_in(self.instance);
                }) {
                    result = Err(error);
                    break;
                }
            }
            // SAFETY: activation owns the live instance. No field borrow
            // survives collection or finalizer reentry.
            if let Err(error) = _toplevel_no_guard.run_checked(|| unsafe {
                crate::sexp::gengc::run_pending_gc_if_quiescent_in(self.instance);
            }) {
                result = Err(error);
            }
            if let Err(error) = _toplevel_no_guard.require_active() {
                result = Err(error);
            }
            let visible = self.inst().eval_state.visible != 0;
            let output = capture.finish();
            f(result, output, visible)
        }
    }

    pub(crate) fn eval_code_with_output_capture_then<'session, T, F>(
        &'session mut self,
        code: &str,
        f: F,
    ) -> T
    where
        F: FnOnce(RResult<Sexp<'session>>, super::output::RCapturedOutput, bool) -> T,
    {
        if !self.is_active() {
            return f(
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                super::output::RCapturedOutput::default(),
                false,
            );
        }

        let expr = {
            let _guard = self.activate();
            let factory = SessionNodeFactory::new(self.owner_token().expect("active session"));
            unsafe {
                /* SAFETY: session activates its checked owner; unsafe payload loans must exclude R reentry. */
                super::memory::with_arena_in(self.instance, |arena| {
                    crate::eval::parser::parse(code, arena, factory)
                })
            }
        };
        let expr = match expr {
            Ok(expr) => expr,
            Err(err) => {
                // Same scoped mapping as the script paths above.
                let message = err.to_string();
                return self.with_active(|| {
                    f(
                        Err(REvalError { message }),
                        super::output::RCapturedOutput::default(),
                        false,
                    )
                });
            }
        };
        self.with_active(|| {
            let capture = super::output::OutputCaptureGuard::start();
            // Single-chunk eval: the parsed expression is script position
            // #1, same location contract as the script loops above.
            let _toplevel_no_guard = ToplevelExprNoGuard::new(self);
            crate::mainutils::errors::set_toplevel_expr_no(1);
            let mut result = _toplevel_no_guard
                .run_checked(|| self.eval_sexp(expr))
                .and_then(|result| result);
            // SAFETY: activation owns the live instance. No field borrow
            // survives collection or finalizer reentry.
            if let Err(error) = _toplevel_no_guard.run_checked(|| unsafe {
                crate::sexp::gengc::run_pending_gc_if_quiescent_in(self.instance);
            }) {
                result = Err(error);
            }
            if let Err(error) = _toplevel_no_guard.require_active() {
                result = Err(error);
            }
            let visible = self.inst().eval_state.visible != 0;
            let output = capture.finish();
            f(result, output, visible)
        })
    }

    /// Evaluate an expression with a custom environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is closed or if evaluation
    /// triggers an R error.
    ///
    /// Raw pointers that do not belong to this session are rejected before
    /// evaluation. Prefer [`RSession::eval_sexp_in`] when the caller already has
    /// lifetime-bound [`Sexp`] handles.
    pub(crate) fn eval_in(&self, expr: SEXP, env: SEXP) -> RResult<SEXP> {
        let expr = self.owned_sexp(expr_or_nil(expr), "expression")?;
        let env = self.owned_sexp(env, "environment")?;
        self.eval_sexp_in(expr, env).map(|value| value.as_raw())
    }

    /// Evaluate an expression in a custom environment and return a
    /// session-scoped safe wrapper.
    pub fn eval_sexp_in<'session>(
        &'session self,
        expr: Sexp<'_>,
        env: Sexp<'_>,
    ) -> RResult<Sexp<'session>> {
        if !self.is_active() {
            return Err(REvalError {
                message: "session is closed".to_string(),
            });
        }

        self.with_active(|| {
            let expr = self.owned_sexp(expr.as_raw(), "expression")?;
            let env = self.owned_sexp(env.as_raw(), "environment")?;
            let result = catch_eval_result(|| unsafe { /* SAFETY: session activates its checked owner; unsafe payload loans must exclude R reentry. */ crate::eval::eval::EvalContext::new(env).eval(expr) })?;
            self.owned_sexp(result.as_raw(), "evaluation result")
        })
    }

    /// Find a variable by name in the global environment.
    ///
    /// Returns `None` if the variable is not found, is unbound, or
    /// is `R_NilValue`.
    ///
    /// Names with interior NUL bytes are rejected.
    pub fn find_var(&self, name: &str) -> Option<Sexp<'_>> {
        self.with_active(|| {
            let symbol = self.sexp(install_symbol(name)?)?;
            let env = Environment::new(self.global_env()?).ok()?;
            let result = unsafe { /* SAFETY: session activates its checked owner; unsafe payload loans must exclude R reentry. */ env.find(symbol) }.ok().flatten()?;
            if result.clone().as_raw() == unsafe { R_UnboundValue() }
                || result.clone().as_raw() == unsafe { R_NilValue() }
            {
                None
            } else {
                Some(result)
            }
        })
    }

    /// Define a variable in the global environment.
    ///
    /// This is a no-op if the session is closed or if the symbol
    /// cannot be interned.
    ///
    /// Names with interior NUL bytes are rejected.
    ///
    /// The value must belong to this session, except for immutable singleton
    /// sentinels such as `NULL`.
    pub fn define_var(&self, name: &str, value: Sexp<'_>) -> bool {
        self.define_var_raw(name, value.as_raw())
    }

    /// Define a variable from a raw pointer for internal compatibility paths.
    ///
    /// The raw value is accepted only after proving it belongs to this session
    /// or is one of R's immutable singleton sentinels.
    fn define_var_raw(&self, name: &str, value: SEXP) -> bool {
        if !self.is_active() {
            return false;
        }
        self.with_active(|| {
            let Some(value) = self.sexp(value) else {
                return false;
            };
            let Some(symbol) = install_symbol(name) else {
                return false;
            };
            let Some(symbol) = self.sexp(symbol) else {
                return false;
            };
            let Some(env) = self.global_env() else {
                return false;
            };
            Environment::new(env)
                .and_then(|env| unsafe { /* SAFETY: session activates its checked owner; unsafe payload loans must exclude R reentry. */ env.define(symbol, value) })
                .is_ok()
        })
    }

    /// Run a closure with mutable access to this session's arena.
    pub fn with_arena<F, T>(&mut self, f: F) -> Option<T>
    where
        F: FnOnce(&mut RArena) -> T,
    {
        if !self.is_active() {
            return None;
        }
        let _guard = self.activate();
        // Borrow only the arena, then process deferred GC after that lend ends.
        // SAFETY: exclusive session borrow excludes other safe access to this owner.
        Some(unsafe { super::memory::with_arena_in(self.instance, f) })
    }

    /// Return the current arena budget for this session.
    pub fn arena_budget(&self) -> ArenaBudget {
        self.inst().arena.budget()
    }

    /// Set the arena budget for this session.
    ///
    /// Existing allocations are kept; future allocations fail if retained arena
    /// memory or active node count would exceed the configured limit.
    pub fn set_arena_budget(&mut self, budget: ArenaBudget) {
        unsafe {
            (*self.instance).arena.set_budget(budget);
        }
    }

    /// Return this session's configured R library search paths.
    pub fn library_paths(&self) -> Vec<std::path::PathBuf> {
        self.inst().path_policy.library_paths().to_vec()
    }

    /// Find an installed package in this session's library search paths.
    pub fn find_package_path(&self, package: &str) -> Option<std::path::PathBuf> {
        self.inst().path_policy.find_package_path(package)
    }

    /// Replace this session's R library search paths.
    pub fn set_library_paths<I, P>(&mut self, paths: I)
    where
        I: IntoIterator<Item = P>,
        P: Into<std::path::PathBuf>,
    {
        unsafe {
            (*self.instance).path_policy.set_library_paths(paths);
        }
    }

    /// Configure Android app-private runtime paths for this session.
    ///
    /// `app_files_dir` owns the user library, `cache_dir` owns `tempdir()`,
    /// and `bundled_library_dir` points at the read-only package library
    /// shipped with the app, when present.
    pub fn configure_android_paths(
        &mut self,
        app_files_dir: impl Into<std::path::PathBuf>,
        cache_dir: impl Into<std::path::PathBuf>,
        bundled_library_dir: Option<impl Into<std::path::PathBuf>>,
    ) -> std::io::Result<()> {
        let policy = crate::mainutils::paths::RuntimePathPolicy::for_android_app(
            app_files_dir,
            cache_dir,
            bundled_library_dir,
        )?;
        unsafe {
            (*self.instance).path_policy = policy;
        }
        Ok(())
    }

    /// Return the session-specific temporary directory used by `tempdir()`.
    pub fn temp_dir(&self) -> &std::path::Path {
        self.inst().path_policy.temp_dir()
    }

    /// Run a function in a protected scope.
    ///
    /// The protection count is saved before calling `f` and any
    /// additional protections added during `f` are automatically
    /// removed after `f` returns. This prevents protection stack
    /// leaks.
    ///
    /// # Examples
    ///
    /// ```text
    /// use crate::sexp::RSession;
    /// use crate::sexp::Sexp;
    /// use crate::sexp::protect::protect_sexp;
    ///
    /// let session = RSession::new();
    /// session.with_protected(|| {
    ///     let _guard = protect_sexp(Sexp::nil());
    /// });
    /// ```
    pub fn with_protected<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        self.with_active(|| {
            let _scope = ProtectScope::new(self.instance_ptr());
            f()
        })
    }

    /// Run the garbage collector.
    ///
    /// Performs a minor GC on the young generation.
    pub fn gc(&self) {
        self.with_active_in(|_| {
            if let Some(owner) = self.owner_token() {
                owner.minor_gc().expect("owning session is active");
            }
        });
    }

    /// Generate a uniform random number using this session's RNG state.
    pub fn unif_rand(&self) -> f64 {
        self.with_active(crate::rng::unif_rand)
    }

    /// Set this session's RNG seed state.
    pub fn set_seed(&self, i1: u32, i2: u32) {
        // Seed the full R-level engine (all RNG kinds, `.Random.seed`), which
        // the nmath samplers share via the uniform hook.
        self.with_active(|| {
            crate::mainutils::random::set_session_seed64(((i1 as i64) << 32) | (i2 as i64))
        });
    }

    /// Set or clear this session's cooperative cancellation token.
    ///
    /// Evaluator loop checks read this token through the active `RInstance`.
    /// The token can be cloned and held by an embedding host so cancellation
    /// remains session-scoped while the synchronization detail stays private.
    pub fn set_cancellation_token(&mut self, token: Option<CancellationToken>) {
        if self.active {
            unsafe {
                (*self.instance).eval_state.cancellation = token;
            }
        }
    }

    /// Replace this session's cooperative cancellation token, returning the old one.
    ///
    /// This gives embedders a scoped, owner-checked way to install a
    /// cancellation token for one evaluation and then restore the previous
    /// state. Closed sessions ignore new flags.
    pub fn replace_cancellation_token(
        &mut self,
        token: Option<CancellationToken>,
    ) -> Option<CancellationToken> {
        if self.active {
            unsafe { std::mem::replace(&mut (*self.instance).eval_state.cancellation, token) }
        } else {
            None
        }
    }

    /// Return this session's current evaluation limits.
    pub fn eval_limits(&self) -> crate::eval::eval::EvalLimits {
        self.inst().eval_state.limits
    }

    /// Set this session's evaluation limits.
    ///
    /// This is the session-owned facade for the evaluator's historical
    /// current-instance limit accessors.
    pub fn set_eval_limits(&mut self, limits: crate::eval::eval::EvalLimits) {
        if self.active {
            unsafe {
                (*self.instance).eval_state.limits = limits;
            }
        }
    }

    /// Reset this session's evaluation limits to the evaluator defaults.
    pub fn reset_eval_limits(&mut self) {
        self.set_eval_limits(crate::eval::eval::EvalLimits::default());
    }

    /// Return this session's capability flags for host-process operations.
    pub fn capabilities(&self) -> super::instance::SessionCapabilities {
        self.inst().eval_state.capabilities
    }

    /// Configure which host-process operations this session may invoke.
    pub fn set_capabilities(&mut self, capabilities: super::instance::SessionCapabilities) {
        if self.active {
            unsafe {
                (*self.instance).eval_state.capabilities = capabilities;
            }
        }
    }

    /// Generate a standard normal random number using this session's RNG state.
    pub fn norm_rand(&self) -> f64 {
        self.with_active(crate::dist::normal::norm_rand)
    }

    /// Run a closure while capturing this session's stdout/stderr buffers.
    pub fn with_output_capture<F, T>(&self, f: F) -> (T, super::output::RCapturedOutput)
    where
        F: FnOnce() -> T,
    {
        self.with_active(|| {
            let capture = super::output::OutputCaptureGuard::start();
            let value = f();
            let output = capture.finish();
            (value, output)
        })
    }

    /// Close this session.
    ///
    /// After closing, [`is_active`](RSession::is_active) returns `false`
    pub fn close(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        unsafe {
            super::instance::revoke_instance_availability(self.instance);
        }
        self.retained_values.clear();
        detach_state(&self.inst().math_state);
        detach_rng(&self.inst().rng_state);
        clear_current_instance_if(self.instance);
    }
}

/// Classify by address, then return the singleton's own pointer provenance.
/// A caller's forged raw pointer is never used to access the sentinel.
pub(crate) fn immutable_singleton_projection(ptr: SEXP) -> Option<SEXP> {
    super::globals::immutable_singleton_projection(ptr)
}

pub(crate) fn is_immutable_singleton(ptr: SEXP) -> bool {
    immutable_singleton_projection(ptr).is_some()
}

impl Default for RSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        // Guards and callback restoration must stop using this owner before
        // field destruction can run provider-defined Rust destructors.
        unsafe {
            super::instance::revoke_instance_availability(self.instance);
        }
        if self.active {
            detach_state(&self.inst().math_state);
            detach_rng(&self.inst().rng_state);
            clear_current_instance_if(self.instance);
        }
        // The owning Rc field releases the instance through ordinary Rust Drop.
    }
}
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(deprecated)] // translated tests exercise the Sexp compat setters
mod tests {
    use super::*;
    use crate::sexp::instance::{
        current_instance_ptr, replace_current_instance, with_current_instance,
    };
    use crate::sexp::protect::{R_PreserveObject, R_ReleaseObject, with_preserved_objects};

    #[test]
    fn detached_bootstrap_restores_only_live_prior_owner_on_callback_and_unwind() {
        for destroy_prior in [false, true] {
            for inject_panic in [false, true] {
                let mut prior = Some(RSession::new_for_gc_tests());
                let projection = prior.as_ref().unwrap().instance;
                let allocation = Rc::downgrade(&prior.as_ref().unwrap()._instance_owner);
                let result = catch_unwind(AssertUnwindSafe(|| {
                    RSession::construct_detached(|| {
                        assert!(current_instance_ptr().is_none());
                        if destroy_prior {
                            drop(prior.take());
                            assert_eq!(allocation.strong_count(), 1);
                        }
                        let session = RSession::new_for_gc_tests();
                        if inject_panic {
                            panic!("injected detached bootstrap unwind");
                        }
                        session
                    })
                }));
                assert_eq!(result.is_err(), inject_panic);
                assert_eq!(
                    current_instance_ptr(),
                    if destroy_prior {
                        None
                    } else {
                        Some(projection)
                    }
                );
                assert_eq!(allocation.strong_count(), if destroy_prior { 0 } else { 1 });
                if let Ok(detached) = result {
                    drop(detached);
                    assert_eq!(
                        current_instance_ptr(),
                        if destroy_prior {
                            None
                        } else {
                            Some(projection)
                        }
                    );
                }
                drop(prior);
                assert!(current_instance_ptr().is_none());
            }
        }
    }

    #[test]
    fn managed_runtime_capability_uses_original_allocation_before_bootstrap() {
        let session = RSession::new_without_default_packages();
        let weak = unsafe { (*session.instance).runtime_owner.clone().unwrap() };
        let pin = weak
            .pin()
            .expect("managed owner installed during construction");
        assert_eq!(pin.as_ptr(), session.instance);
        assert!(unsafe { (*pin.as_ptr()).initialized });
        assert_eq!(Rc::strong_count(&session._instance_owner), 2);
        drop(pin);
        assert_eq!(Rc::strong_count(&session._instance_owner), 1);
    }

    #[test]
    fn managed_ambient_operation_pins_callback_dropped_owner_through_unwind() {
        for inject_panic in [false, true] {
            let mut session = Some(RSession::new_for_gc_tests());
            let allocation = Rc::downgrade(&session.as_ref().unwrap()._instance_owner);
            let pointer = session.as_ref().unwrap().instance;
            let availability = unsafe { super::super::instance::instance_liveness(pointer) };
            let result = catch_unwind(AssertUnwindSafe(|| {
                with_current_instance(|active| {
                    assert_eq!(active, pointer);
                    drop(session.take());
                    assert!(!availability.is_live());
                    assert!(current_instance_ptr().is_none());
                    assert_eq!(allocation.strong_count(), 1);
                    // The operation retains the original physical allocation,
                    // while revocation prevents starting another operation.
                    assert!(with_current_instance(|_| panic!("revoked dispatch")).is_none());
                    assert!(unsafe { (*active).runtime_owner.is_some() });
                    if inject_panic {
                        panic!("injected owner drop unwind");
                    }
                })
                .expect("live ambient owner");
            }));
            assert_eq!(result.is_err(), inject_panic);
            assert_eq!(allocation.strong_count(), 0);
            assert!(current_instance_ptr().is_none());
        }
    }

    #[test]
    fn managed_activation_pins_previous_owner_without_restoring_revoked_state() {
        let current = RSession::new_for_gc_tests();
        let mut previous = Some(RSession::new_for_gc_tests());
        let allocation = Rc::downgrade(&previous.as_ref().unwrap()._instance_owner);
        current.with_active(|| {
            drop(previous.take());
            assert_eq!(allocation.strong_count(), 1);
            assert_eq!(current_instance_ptr(), Some(current.instance));
        });
        assert_eq!(allocation.strong_count(), 0);
        assert!(current_instance_ptr().is_none());
    }

    #[test]
    fn owned_session_projection_survives_moves_and_reentry() {
        let session = RSession::new_for_gc_tests();
        let projection = session.instance_ptr();
        let observer = unsafe { super::super::instance::instance_liveness(projection) };
        let mut moved = Vec::new();
        moved.push(session);
        moved.reserve(32);
        let session = moved.pop().unwrap();
        assert_eq!(session.instance_ptr(), projection);
        assert_eq!(Rc::strong_count(&session._instance_owner), 1);
        session.with_protected(|| {
            assert_eq!(current_instance_ptr(), Some(projection));
            let value = session.global_env().unwrap();
            assert!(value.is_environment());
            session.gc();
            assert!(value.is_environment());
        });
        drop(session);
        assert!(!observer.is_live());
        assert!(current_instance_ptr().is_none());
    }

    #[test]
    fn owner_close_during_activation_never_restores_closed_state() {
        for inject_panic in [false, true] {
            let left = RSession::new_for_gc_tests();
            let mut right = RSession::new_for_gc_tests();
            let observer =
                unsafe { super::super::instance::instance_liveness(right.instance_ptr()) };
            let result = catch_unwind(AssertUnwindSafe(|| {
                left.with_protected(|| {
                    right.close();
                    assert!(!observer.is_live());
                    assert_eq!(current_instance_ptr(), Some(left.instance_ptr()));
                    if inject_panic {
                        panic!("injected close callback unwind");
                    }
                });
            }));
            assert_eq!(result.is_err(), inject_panic);
            assert!(current_instance_ptr().is_none());
            assert!(right.owner_token().is_none());
            assert!(catch_unwind(AssertUnwindSafe(|| right.with_active(|| ()))).is_err());
            assert!(current_instance_ptr().is_none());
            // Detachment is idempotent and cannot disturb another owner.
            left.with_active(|| {
                right.close();
            });
            assert!(current_instance_ptr().is_none());
        }
    }

    #[test]
    fn weak_protection_scope_skips_destroyed_owner_even_on_unwind() {
        for inject_panic in [false, true] {
            let session = RSession::new_for_gc_tests();
            let pointer = session.instance_ptr();
            let observer = unsafe { super::super::instance::instance_liveness(pointer) };
            let mut owner = Some(session);
            let mut fresh = None;
            let result = catch_unwind(AssertUnwindSafe(|| {
                let _scope = ProtectScope::new(pointer);
                unsafe {
                    super::super::protect::protect_raw_pointer(R_NilValue());
                }
                // This is a user callback's teardown/reentry pattern: the
                // scope retains a weak witness, never the physical allocation.
                drop(owner.take());
                fresh = Some(RSession::new_for_gc_tests());
                unsafe {
                    super::super::protect::protect_raw_pointer(R_NilValue());
                }
                if inject_panic {
                    panic!("injected scope teardown unwind");
                }
            }));
            assert_eq!(result.is_err(), inject_panic);
            assert!(!observer.is_live());
            assert_eq!(R_ProtectCount(), 1);
            unsafe {
                super::super::protect::unprotect_count(1);
            }
            drop(fresh);
        }
    }

    #[test]
    fn owned_parsed_script_survives_deferred_and_reentrant_collection() {
        let mut session = RSession::new_for_gc_tests();
        let notifications = Rc::new(std::cell::Cell::new(0));
        let observed = notifications.clone();
        super::super::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            // Parse results already own leases before the arena lend finishes.
            super::super::gengc::full_gc();
        }));
        unsafe {
            (*session.instance).gc_state.gc_pending = true;
        }
        let (result, _, _) = session.eval_script_with_output_capture("7L; 11L; 19L");
        let result = result.expect("every parsed statement remains alive");
        assert_eq!(result.integer_elt(0), Some(19));
        assert!(notifications.get() > 0);
        super::super::gengc::full_gc();
        assert_eq!(result.integer_elt(0), Some(19));
    }

    #[test]
    fn owned_parsed_single_expression_survives_result_mapping_collection() {
        let mut session = RSession::new_for_gc_tests();
        session.eval_code_with_output_capture_then("23L", |result, _, _| {
            let result = result.expect("owned parse result");
            super::super::gengc::full_gc();
            assert_eq!(result.integer_elt(0), Some(23));
        });
    }

    #[cfg(feature = "renderplot-device")]
    #[test]
    fn owned_renderplot_statements_survive_reentrant_gc_and_restore_backend_after_error() {
        use r_graphics_engine::{DrawTarget, Scene};

        // Both concrete backends outlive the session's temporary raw slots.
        let mut previous = Scene::new(64, 48);
        let mut target = Scene::new(96, 72);
        let previous_backend: *mut dyn DrawTarget = &mut previous;
        let backend: *mut dyn DrawTarget = &mut target;
        let mut session = RSession::new_for_gc_tests();
        let instance = session.instance_ptr();
        let notifications = Rc::new(std::cell::Cell::new(0));
        let observed = notifications.clone();
        super::super::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            super::super::gengc::full_gc();
        }));
        // SAFETY: this fixture owns the instance and both backend objects for
        // the entire test. No interpreter field or payload loan is retained.
        unsafe {
            (*instance).current_renderplot_backend = Some(previous_backend);
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        }

        let value = session.eval_script_with_output_capture_then_renderplot(
            "7L + 1L; 11L + 2L; 19L + 3L",
            backend,
            |result, _, _| {
                let value = result.expect("all parsed statements survive allocation GC");
                unsafe {
                    assert!(!std::ptr::eq(
                        (*instance).current_renderplot_backend.unwrap(),
                        previous_backend,
                    ));
                    super::super::gengc::full_gc_in(instance);
                }
                assert_eq!(value.try_integer_elt(0).unwrap(), 22);
                value
            },
        );
        unsafe {
            assert!(std::ptr::eq(
                (*instance).current_renderplot_backend.unwrap(),
                previous_backend,
            ));
            super::super::gengc::full_gc_in(instance);
        }
        assert_eq!(value.try_integer_elt(0).unwrap(), 22);
        assert!(notifications.get() >= 3);
        drop(value);

        let error = session.eval_script_with_output_capture_then_renderplot(
            "31L + 1L; undefined_renderplot_gc_symbol; 37L + 1L",
            backend,
            |result, _, _| result.expect_err("undefined symbol must propagate"),
        );
        assert!(error.message.contains("undefined_renderplot_gc_symbol"));
        unsafe {
            assert!(std::ptr::eq(
                (*instance).current_renderplot_backend.unwrap(),
                previous_backend,
            ));
        }

        session.eval_script_with_output_capture_then_renderplot(
            "41L + 1L; 43L + 1L; 47L + 1L",
            backend,
            |result, _, _| {
                let value = result.expect("renderplot evaluation recovers after error");
                unsafe { super::super::gengc::full_gc_in(instance) };
                assert_eq!(value.try_integer_elt(0).unwrap(), 48);
            },
        );
        unsafe {
            assert!(std::ptr::eq(
                (*instance).current_renderplot_backend.unwrap(),
                previous_backend,
            ));
            (*instance).memory_state.gc_force_gap = 0;
            (*instance).current_renderplot_backend = None;
        }
    }

    #[test]
    fn test_session_creation() {
        let session = RSession::new();
        assert!(session.is_active());
        assert!(session.global_env().is_some());
        assert!(session.base_env().is_some());
        assert!(matches!(
            session
                .global_env()
                .expect("session has global env")
                .owner(),
            crate::sexp::object::SexpOwner::Session(_)
        ));
    }

    #[test]
    fn test_detached_session_constructor_restores_current_instance() {
        let current = RSession::new();
        let current_ptr = current.instance_ptr();

        let detached = RSession::new_detached();
        assert!(detached.is_active());
        assert_eq!(current_instance_ptr(), Some(current_ptr));

        let result = detached.with_output_capture(|| {
            crate::sexp::output::capture_stdout("detached");
        });
        assert_eq!(result.1.stdout, "detached");
        assert_eq!(current_instance_ptr(), Some(current_ptr));
    }

    #[test]
    fn test_session_sexp_rejects_foreign_arena_pointer() {
        let mut left = RSession::new();
        let right = RSession::new();

        let ptr = left
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("left session should be active");

        let left_value = left.sexp(ptr).expect("left owns pointer");
        assert!(matches!(
            left_value.owner(),
            crate::sexp::object::SexpOwner::Session(_)
        ));
        assert!(right.sexp(ptr).is_none());
    }

    #[test]
    fn test_session_eval_sexp_returns_session_scoped_wrapper() {
        let mut session = RSession::new();
        let expr = session
            .with_arena(|arena| {
                crate::sexp::builder::scalar_integer_in(arena, 7)
                    .expect("scalar allocation should succeed")
                    .as_raw()
            })
            .expect("session should be active");
        let expr = session.sexp(expr).expect("expr belongs to session");

        let result = session
            .eval_sexp(expr)
            .expect("self-evaluating scalar should evaluate");

        assert_eq!(result.clone().integer_elt(0), Some(7));
        assert!(result.is_owner_scoped());
    }

    #[test]
    fn test_session_eval_with_output_capture_returns_typed_wrapper() {
        let mut session = RSession::new();
        let expr = session
            .with_arena(|arena| {
                crate::sexp::builder::scalar_integer_in(arena, 8)
                    .expect("scalar allocation should succeed")
                    .as_raw()
            })
            .expect("session should be active");
        let expr = session.sexp(expr).expect("expr belongs to session");

        let (result, output, visible) = session.eval_sexp_with_output_capture(expr);
        let result = result.expect("self-evaluating scalar should evaluate");

        assert_eq!(result.integer_elt(0), Some(8));
        assert!(output.stdout.is_empty());
        assert!(visible);
    }

    #[test]
    fn test_session_eval_code_with_output_capture_keeps_parsing_session_owned() {
        let mut session = RSession::new();

        let (result, output, visible) = session.eval_code_with_output_capture("print(1); 2");
        let result = result.expect("source should parse and evaluate");

        assert_eq!(output.stdout, "[1] 1\n");
        assert_eq!(result.real_elt(0), Some(2.0));
        assert!(visible);
    }

    #[test]
    fn test_trailing_empty_actual_uses_formal_default() {
        let mut session = RSession::new();

        let (result, output, visible) = session.eval_code_with_output_capture("identical(0, -0,)");
        let result = result.expect("the empty num.eq actual should select its default");

        assert_eq!(result.logical_elt(0), Some(1));
        assert!(output.stdout.is_empty());
        assert!(visible);
    }

    #[test]
    fn test_closure_trailing_empty_actual_uses_formal_default() {
        let mut session = RSession::new();

        let (result, output, visible) =
            session.eval_code_with_output_capture("(function(x, option = TRUE) option)(0,)");
        let result = result.expect("an empty actual should select the closure formal's default");

        assert_eq!(result.logical_elt(0), Some(1));
        assert!(output.stdout.is_empty());
        assert!(visible);
    }

    #[test]
    fn test_parenthesized_assignment_is_visible() {
        let mut session = RSession::new();

        let (result, output, visible) =
            session.eval_script_with_output_capture("(x <- 0 * (-1)); invisible(NULL)");
        result.expect("parenthesized assignment should evaluate");

        assert_eq!(output.stdout, "[1] 0\n");
        assert!(!visible, "the final invisible(NULL) remains invisible");
    }

    #[test]
    fn test_script_auto_print_keeps_separator_after_attributes_list() {
        let mut session = RSession::new();

        let (result, output, visible) = session.eval_script_with_output_capture(
            "attributes(structure(1:2, tsp = c(1, 2, 1), class = 'ts')); \
             cat('after\\n'); invisible(NULL)",
        );
        result.expect("attributes list and following expression should evaluate");

        assert_eq!(
            output.stdout,
            "$tsp\n[1] 1 2 1\n\n$class\n[1] \"ts\"\n\nafter\n"
        );
        assert!(!visible);
    }

    #[test]
    fn test_session_script_parse_error_maps_to_upstream_message() {
        let mut session = RSession::new();

        // Parse failures must surface as a clean REvalError rendered like
        // Rscript ("Error: unexpected ')' in ..."), never as a panic from
        // the error-conversion path touching unscoped runtime state.
        let (result, output, visible) = session.eval_script_with_output_capture("print(1 + )");
        let err = result.expect_err("syntax error must fail the script");
        assert_eq!(err.message, "unexpected ')' in \"print(1 + )\"");
        assert!(output.stdout.is_empty());
        assert!(!visible);

        // The session stays usable after a parse failure.
        let (result, output, _) = session.eval_script_with_output_capture("print(40 + 2)");
        assert_eq!(
            result.expect("session should recover").real_elt(0),
            Some(42.0)
        );
        assert_eq!(output.stdout, "[1] 42\n");
    }

    #[test]
    fn test_session_script_parse_error_at_end_of_input_has_no_context() {
        let mut session = RSession::new();

        let (result, _, _) = session.eval_script_with_output_capture("1 + +");
        let err = result.expect_err("incomplete input must fail the script");
        assert_eq!(err.message, "unexpected end of input");
    }

    #[test]
    fn test_session_malformed_script_has_no_prefix_side_effects() {
        let mut session = RSession::new();

        for (name, malformed) in [
            ("unterminated_string_side_effect", "\"unterminated"),
            ("unknown_input_side_effect", "\0"),
        ] {
            let script = format!("{name} <- 1; {malformed}");
            let (result, output, visible) = session.eval_script_with_output_capture(&script);
            assert!(result.is_err(), "malformed script unexpectedly succeeded");
            assert!(output.stdout.is_empty());
            assert!(!visible);
            drop(result);

            let probe = format!("exists(\"{name}\")");
            let (result, _, _) = session.eval_script_with_output_capture(&probe);
            assert_eq!(
                result
                    .expect("existence probe should evaluate")
                    .logical_elt(0),
                Some(crate::sexp::ffi::FALSE),
                "valid prefix mutated the session before the parse failure"
            );
        }
    }

    #[test]
    fn test_session_close() {
        let mut session = RSession::new();
        assert!(session.is_active());
        session.close();
        assert!(!session.is_active());
    }

    #[test]
    fn test_session_close_non_current_keeps_current_instance() {
        let mut older = RSession::new();
        let newer = RSession::new();
        let newer_instance = newer.instance_ptr();

        let previous = unsafe { replace_current_instance(Some(newer.instance_ptr())) };
        older.close();

        assert!(
            with_current_instance(|inst| std::ptr::eq(inst as *const RInstance, newer_instance))
                .unwrap_or(false)
        );
        assert!(newer.is_active());
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_session_drop_non_current_keeps_current_instance() {
        let older = RSession::new();
        let newer = RSession::new();
        let newer_instance = newer.instance_ptr();

        let previous = unsafe { replace_current_instance(Some(newer.instance_ptr())) };
        drop(older);

        assert!(
            with_current_instance(|inst| std::ptr::eq(inst as *const RInstance, newer_instance))
                .unwrap_or(false)
        );
        assert!(newer.is_active());
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_explicit_gc_owner_restores_nested_activation() {
        let left = RSession::new_without_default_packages();
        let right = RSession::new_without_default_packages();
        left.with_active_in(|left_inst| {
            unsafe {
                (*left_inst).gc_state.gc_pending = true;
            }
            right.with_active_in(|right_inst| unsafe {
                assert_eq!(with_current_instance(|inst| inst), Some(right_inst));
                (*right_inst).gc_state.gc_pending = true;
                // An evaluation frame must defer collection for this owner.
                (*right_inst).eval_state.eval_depth = 1;
                super::super::gengc::run_pending_gc_if_quiescent_in(right_inst);
                assert!((*right_inst).gc_state.gc_pending);
                (*right_inst).eval_state.eval_depth = 0;
                super::super::gengc::run_pending_gc_if_quiescent_in(right_inst);
                assert!(!(*right_inst).gc_state.gc_pending);
                assert!((*left_inst).gc_state.gc_pending);
            });
            assert_eq!(with_current_instance(|inst| inst), Some(left_inst));
            unsafe {
                super::super::gengc::run_pending_gc_if_quiescent_in(left_inst);
                assert!(!(*left_inst).gc_state.gc_pending);
            }
            // A callback unwind must also restore the outer owner.
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                right.with_active_in(|_| panic!("nested activation"));
            }));
            assert!(panic.is_err());
            assert_eq!(with_current_instance(|inst| inst), Some(left_inst));
        });
    }

    #[test]
    fn test_session_method_activation_restores_previous_instance() {
        let older = RSession::new();
        let older_instance = older.instance_ptr();
        let newer = RSession::new();
        let newer_instance = newer.instance_ptr();

        let previous = unsafe { replace_current_instance(Some(newer.instance_ptr())) };
        assert!(
            current_instance_ptr()
                .map(|ptr| std::ptr::eq(ptr as *const RInstance, newer_instance))
                .unwrap_or(false)
        );

        let activated_older = older.with_active(|| {
            current_instance_ptr()
                .map(|ptr| std::ptr::eq(ptr as *const RInstance, older_instance))
                .unwrap_or(false)
        });

        assert!(activated_older);
        assert!(
            current_instance_ptr()
                .map(|ptr| std::ptr::eq(ptr as *const RInstance, newer_instance))
                .unwrap_or(false)
        );
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_session_isolates_srcref_and_handler_bookkeeping_on_one_thread() {
        let outer = RSession::new();
        let inner = RSession::new();
        outer.with_active(|| unsafe {
            (*outer.instance_ptr()).error_state.current_srcref_location =
                Some(("outer.R".into(), 7));
            (*outer.instance_ptr())
                .error_state
                .try_catch_handler_classes
                .push(vec!["warning".into()]);
            (*outer.instance_ptr())
                .error_state
                .calling_handlers_signaled = true;
        });
        inner.with_active(|| unsafe {
            assert_eq!(
                (*inner.instance_ptr()).error_state.current_srcref_location,
                None
            );
            assert!(
                (*inner.instance_ptr())
                    .error_state
                    .try_catch_handler_classes
                    .is_empty()
            );
            assert!(
                !(*inner.instance_ptr())
                    .error_state
                    .calling_handlers_signaled
            );
        });
        outer.with_active(|| unsafe {
            assert_eq!(
                (*outer.instance_ptr()).error_state.current_srcref_location,
                Some(("outer.R".into(), 7))
            );
            assert_eq!(
                (*outer.instance_ptr())
                    .error_state
                    .try_catch_handler_classes,
                vec![vec![String::from("warning")]]
            );
            assert!(
                (*outer.instance_ptr())
                    .error_state
                    .calling_handlers_signaled
            );
        });
    }

    #[test]
    fn test_session_output_capture_is_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let (_, left_output) = left.with_output_capture(|| {
            crate::sexp::output::capture_stdout("left out");
            crate::sexp::output::capture_stderr("left err");
        });
        let (_, right_output) = right.with_output_capture(|| {
            crate::sexp::output::capture_stdout("right out");
            crate::sexp::output::capture_stderr("right err");
        });

        assert_eq!(left_output.stdout, "left out");
        assert_eq!(left_output.stderr, "left err");
        assert_eq!(right_output.stdout, "right out");
        assert_eq!(right_output.stderr, "right err");
    }

    #[test]
    fn test_session_eval_limits_are_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.set_eval_limits(crate::eval::eval::EvalLimits {
            max_eval_depth: 7,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
        });
        right.set_eval_limits(crate::eval::eval::EvalLimits {
            max_eval_depth: 19,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
        });

        assert_eq!(left.eval_limits().max_eval_depth, 7);
        assert_eq!(right.eval_limits().max_eval_depth, 19);
        assert_eq!(
            left.with_active(crate::eval::eval::get_eval_limits)
                .max_eval_depth,
            7
        );
        assert_eq!(
            right
                .with_active(crate::eval::eval::get_eval_limits)
                .max_eval_depth,
            19
        );

        left.reset_eval_limits();
        assert_eq!(left.eval_limits(), crate::eval::eval::EvalLimits::default());
        assert_eq!(right.eval_limits().max_eval_depth, 19);
    }

    #[test]
    fn test_eval_depth_guard_drops_against_original_session() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_active(|| {
            let guard = crate::eval::eval::check_eval_depth().expect("left depth should increment");
            assert_eq!(unsafe { (*left.instance).eval_state.eval_depth }, 1);
            assert_eq!(unsafe { (*right.instance).eval_state.eval_depth }, 0);

            let previous = unsafe { replace_current_instance(Some(right.instance_ptr())) };
            drop(guard);
            assert_eq!(unsafe { (*left.instance).eval_state.eval_depth }, 0);
            assert_eq!(unsafe { (*right.instance).eval_state.eval_depth }, 0);
            unsafe {
                replace_current_instance(previous);
            }
        });
    }

    #[test]
    fn test_eval_timer_guard_drops_against_original_session() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_active(|| {
            let guard = crate::eval::eval::EvalTimerGuard::start_if_needed();
            assert!(unsafe { (*left.instance).eval_state.start_time.is_some() });
            assert!(unsafe { (*right.instance).eval_state.start_time.is_none() });

            let previous = unsafe { replace_current_instance(Some(right.instance_ptr())) };
            drop(guard);
            assert!(unsafe { (*left.instance).eval_state.start_time.is_none() });
            assert!(unsafe { (*right.instance).eval_state.start_time.is_none() });
            unsafe {
                replace_current_instance(previous);
            }
        });
    }

    #[test]
    fn test_eval_with_limits_restores_session_state() {
        let mut session = RSession::new();
        let original_limits = crate::eval::eval::EvalLimits {
            max_eval_depth: 11,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
        };
        session.set_eval_limits(original_limits);
        unsafe { (*session.instance).eval_state.start_time = Some(std::time::Instant::now()) };

        let expr = session
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("session should be active");
        let expr = session.sexp(expr).expect("expr belongs to session");
        assert!(unsafe {
            /* SAFETY: fresh fixture with no borrowed payload views. */
            expr.clone().set_integer_elt(0, 7)
        });
        let env = session.global_env().expect("session has global env");

        let result = session.with_active(|| unsafe {
            crate::eval::eval::eval_with_limits(
                expr,
                env,
                crate::eval::eval::EvalLimits {
                    max_eval_depth: 3,
                    max_execution_time_ms: 1_000,
                    max_alloc_bytes: 0,
                },
            )
        });
        let result = result.expect("self-evaluating vector should evaluate");
        assert_eq!(result.integer_elt(0), Some(7));
        assert_eq!(
            unsafe { (*session.instance).eval_state.limits },
            original_limits
        );
        assert!(unsafe { (*session.instance).eval_state.start_time.is_some() });
    }

    #[test]
    fn test_session_cancellation_token_is_session_owned() {
        let mut cancelled = RSession::new();
        let mut active = RSession::new();
        let token = CancellationToken::cancelled();

        cancelled.set_cancellation_token(Some(token));
        let (result, _, _) = cancelled.eval_code_with_output_capture("1 + 1");
        let err = result.expect_err("cancelled session should reject eval");
        assert_eq!(err.message, "operation cancelled");

        let (result, _, _) = active.eval_code_with_output_capture("1 + 1");
        let result = result.expect("second session should not inherit cancellation");
        assert_eq!(result.real_elt(0), Some(2.0));
    }

    #[test]
    fn test_session_eval_closed() {
        let mut session = RSession::new();
        session.close();
        let result = session.eval(ptr::null_mut());
        assert!(result.is_err());
    }

    #[test]
    fn test_session_define_and_find_var_with_rust_str() {
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("session should be active");
        let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(value) }.expect("integer vector allocation failed");
        assert!(unsafe {
            /* SAFETY: fresh fixture with no borrowed payload views. */
            sexp.set_integer_elt(0, 42)
        });

        let value = session.sexp(value).expect("value belongs to session");
        assert!(session.define_var("session_defined_value", value));

        let found = session
            .find_var("session_defined_value")
            .expect("defined value should be found");
        assert_eq!(found.integer_elt(0), Some(42));
    }

    #[test]
    fn test_session_define_var_interns_symbol_in_target_session() {
        let mut older = RSession::new();
        let newer = RSession::new();

        let value = older
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("older session should be active");
        let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(value) }.expect("integer vector allocation failed");
        assert!(unsafe {
            /* SAFETY: fresh fixture with no borrowed payload views. */
            sexp.set_integer_elt(0, 123)
        });

        let value = older.sexp(value).expect("value belongs to older session");
        assert!(older.define_var("session_local_symbol", value));

        let found = older
            .find_var("session_local_symbol")
            .expect("older session should own symbol binding");
        assert_eq!(found.integer_elt(0), Some(123));
        assert!(newer.find_var("session_local_symbol").is_none());
    }

    #[test]
    fn test_session_define_var_rejects_foreign_value() {
        let mut owner = RSession::new();
        let other = RSession::new();

        let value = owner
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("owner session should be active");
        let value = owner.sexp(value).expect("value belongs to owner");

        assert!(!other.define_var("foreign_value", value));
        assert!(other.find_var("foreign_value").is_none());
    }

    #[test]
    fn test_session_rejects_interior_nul_symbol_names() {
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("session should be active");
        let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(value) }.expect("integer vector allocation failed");
        assert!(unsafe {
            /* SAFETY: fresh fixture with no borrowed payload views. */
            sexp.set_integer_elt(0, 99)
        });

        let value = session.sexp(value).expect("value belongs to session");
        assert!(!session.define_var("session_bad\0name", value));

        assert!(session.find_var("session_bad\0name").is_none());
        assert!(session.find_var("session_bad").is_none());
    }

    #[test]
    fn test_session_protected_scope() {
        let session = RSession::new();
        let depth_before = session.with_active(R_ProtectCount);
        session.with_protected(|| {
            std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(0x1 as SEXP) });
            std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(0x2 as SEXP) });
            // Leaked guards land in the root table; the count-based legacy
            // view does not see them.
            crate::sexp::protect::with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots.len(), 2);
            });
        });
        let depth_after = session.with_active(R_ProtectCount);
        assert_eq!(depth_before, depth_after);
        // The scope unwind truncated the leaked roots away.
        crate::sexp::protect::with_protected_objects(|_, roots| {
            assert!(roots.iter().all(|&p| p.is_null()))
        });
    }

    #[test]
    fn test_session_protected_scope_unwinds_on_panic() {
        let session = RSession::new();
        let depth_before = session.with_active(R_ProtectCount);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.with_protected(|| {
                std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(0x1 as SEXP) });
                panic!("forced protected-scope unwind");
            });
        }));

        assert!(result.is_err());
        let depth_after = session.with_active(R_ProtectCount);
        assert_eq!(depth_before, depth_after);
        crate::sexp::protect::with_protected_objects(|_, roots| {
            assert!(roots.iter().all(|&p| p.is_null()))
        });
    }

    #[test]
    fn test_session_protect_and_preserve_are_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();
        let protected = 0x1 as SEXP;
        let preserved = 0x2 as SEXP;

        left.with_active(|| {
            std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(protected) });
            unsafe {
                R_PreserveObject(preserved);
            }
            crate::sexp::protect::with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots, &[protected]);
            });
            with_preserved_objects(|objects| assert_eq!(objects, &[preserved]));
        });

        right.with_active(|| {
            crate::sexp::protect::with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert!(roots.is_empty());
            });
            with_preserved_objects(|objects| assert!(objects.is_empty()));
        });

        left.with_active(|| {
            // A nested scope reclaims only ITS OWN leaks; the root leaked
            // outside any scope survives until session teardown.
            left.with_protected(|| {
                std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(0x3 as SEXP) });
            });
            crate::sexp::protect::with_protected_objects(|_, roots| {
                assert_eq!(roots, &[protected]);
            });
            unsafe {
                R_ReleaseObject(preserved);
            }
            with_preserved_objects(|objects| assert!(objects.is_empty()));
        });
    }

    #[test]
    fn test_session_gc() {
        let session = RSession::new();
        // Should not panic
        session.gc();
    }

    #[test]
    fn test_session_arena_budget_controls_future_allocations() {
        let mut session = RSession::new();
        let node_bytes = std::mem::size_of::<crate::sexp::ffi::SexprecCore>();
        let current_bytes = session
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        let current_nodes = session.with_arena(|arena| arena.node_count()).unwrap();
        let budget = ArenaBudget::new(current_bytes + node_bytes, current_nodes + 1);
        session.set_arena_budget(budget);
        assert_eq!(session.arena_budget(), budget);

        session
            .with_arena(|arena| {
                assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
                assert!(arena.alloc_node(SEXPTYPE::REALSXP).is_null());
            })
            .unwrap();
    }

    #[test]
    fn test_session_default() {
        let session = RSession::default();
        assert!(session.is_active());
    }

    // -----------------------------------------------------------------------
    // Concurrent session stress tests
    // -----------------------------------------------------------------------

    #[test]
    fn concurrent_sessions_are_independent() {
        // Spawn 4 threads, each creating their own RSession, defining a
        // unique variable, evaluating code, and verifying isolation.
        let handles: Vec<_> = (0..4)
            .map(|i| {
                std::thread::spawn(move || {
                    let mut session = RSession::new();
                    assert!(session.is_active());
                    assert!(session.global_env().is_some());

                    // Define a variable unique to this thread/session
                    let var_name = format!("thread_var_{i}");
                    let value = session
                        .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
                        .expect("session should be active");
                    let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(value) }.expect("allocation failed");
                    assert!(unsafe { /* SAFETY: fresh fixture with no borrowed payload views. */ sexp.set_integer_elt(0, (i + 100) as i32) });

                    let value = session.sexp(value).expect("value belongs to session");
                    assert!(session.define_var(&var_name, value));

                    // Verify the variable
                    let found = session
                        .find_var(&var_name)
                        .expect("should find the variable we just defined");
                    assert_eq!(
                        found.integer_elt(0),
                        Some((i + 100) as i32),
                        "thread {i} got wrong value"
                    );

                    // Verify other threads' variables are NOT visible
                    for j in 0..4 {
                        if j != i {
                            let other_name = format!("thread_var_{j}");
                            assert!(
                                session.find_var(&other_name).is_none(),
                                "thread {i} should NOT see thread {j}'s variable"
                            );
                        }
                    }

                    drop(found);
                    session.close();
                    assert!(!session.is_active());
                    (i, true)
                })
            })
            .collect();

        for h in handles {
            let (thread_id, ok) = h.join().expect("thread should not panic");
            assert!(ok, "thread {thread_id} failed");
        }
    }

    #[test]
    fn concurrent_sessions_eval_code_independently() {
        // Each thread evaluates R code and checks isolation of captured output.
        let handles: Vec<_> = (0..4)
            .map(|i| {
                std::thread::spawn(move || {
                    let mut session = RSession::new();

                    let code = format!("{i} + 100");
                    let (result, _output, _visible) = session.eval_code_with_output_capture(&code);
                    let result = result.expect("eval should succeed");
                    assert_eq!(
                        result.real_elt(0),
                        Some((i + 100) as f64),
                        "thread {i} computed wrong result"
                    );
                    drop(result);
                    session.close();
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread should not panic");
        }
    }

    #[test]
    fn concurrent_sessions_output_capture_isolated() {
        // Verify that output capture in one session doesn't leak to another
        // session on a different thread.
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let barrier_clone = barrier.clone();

        let h1 = std::thread::spawn(move || {
            let session = RSession::new();
            barrier_clone.wait(); // synchronize start
            let (_, output) = session.with_output_capture(|| {
                crate::sexp::output::capture_stdout("thread_one_output");
            });
            assert_eq!(output.stdout, "thread_one_output");
            // Must NOT contain thread_two's output
            assert!(
                !output.stdout.contains("thread_two"),
                "output leaked between threads"
            );
        });

        let h2 = std::thread::spawn(move || {
            let session = RSession::new();
            barrier.wait(); // synchronize start
            let (_, output) = session.with_output_capture(|| {
                crate::sexp::output::capture_stdout("thread_two_output");
            });
            assert_eq!(output.stdout, "thread_two_output");
            assert!(
                !output.stdout.contains("thread_one"),
                "output leaked between threads"
            );
        });

        h1.join().expect("thread 1 panicked");
        h2.join().expect("thread 2 panicked");
    }

    #[test]
    fn concurrent_sessions_rng_independent() {
        // RNG state should be per-session. Two threads with the same seed
        // should get the same sequence, and different threads should not
        // interfere with each other.
        let handles: Vec<_> = (0..2)
            .map(|i| {
                std::thread::spawn(move || {
                    let session = RSession::new();
                    session.set_seed(42, 42);
                    let r1 = session.unif_rand();
                    let r2 = session.unif_rand();
                    // Same seed should give same first value across sessions
                    (i, r1, r2)
                })
            })
            .collect();

        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().expect("thread panicked"))
            .collect();

        // Both threads used seed (42,42), so they should get the same sequence
        assert_eq!(
            results[0].1, results[1].1,
            "same seed should produce same first random number"
        );
        assert_eq!(
            results[0].2, results[1].2,
            "same seed should produce same second random number"
        );
    }
}

#[cfg(test)]
#[path = "gc_owner_teardown_tests.rs"]
mod gc_owner_teardown_tests;

#[cfg(test)]
#[path = "session/public_capture_tests.rs"]
mod public_capture_tests;
