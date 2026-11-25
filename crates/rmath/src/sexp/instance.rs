#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R instance isolation — per-instance state for concurrent R sessions.
//!
//! # Ambient-access aliasing discipline (P1/P2)
//!
//! Ambient access (`with_required_current_instance` etc.) hands out the raw
//! `*mut RInstance`; field access through it is a raw place access, which
//! carries no Stacked-Borrows protector, so reentrant ambient writes
//! (protect-stack pushes, GC bookkeeping during allocation) are legal.
//! Two rules keep this sound — **Miri is their checker**:
//!
//! - **P1**: never hold a `&mut RInstance` or a `&mut` field borrow across
//!   a call that may allocate, protect, GC, or eval. Form such borrows
//!   only around strictly-local operations and drop them before reentry.
//! - **P2**: never hold a `&RInstance` or `&` field borrow across an
//!   ambient write (mutation through any raw instance path).
//!
//! Violations are UB that the borrow checker cannot see (raw pointers) but
//! Miri reports; `cargo +nightly miri test -p rmath --lib <module>::` is the
//! gate for modules exercising sessions.
//!
//! An `RInstance` owns all mutable state that was previously process-wide or
//! thread-local, enabling multiple independent R sessions to run concurrently
//! within the same process and on the same thread (sequentially).
//!
//! # Scoped compatibility dispatch
//!
//! Ported C-shaped internals still expect ambient accessors such as
//! `R_GlobalEnv`, protection APIs, and `with_arena`. Rust code should reach
//! them through `RSession`, which owns the instance and scopes method
//! activation so nested calls restore the previous instance. Low-level
//! translated tests may still explicitly install an instance while the port is
//! moving toward fully explicit session parameters.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::os::raw::c_int;
use std::rc::{Rc, Weak};
use std::time::Instant;

use super::ffi::{SEXP, SEXPTYPE, SexprecCore};
use super::memory::RArena;
use super::object::{Sexp, SexpResult};
use super::owner::OwnerPin;
#[path = "persistent.rs"]
pub(crate) mod persistent;
use persistent::PersistentHeap;

// ---------------------------------------------------------------------------
// RInstance
// ---------------------------------------------------------------------------

/// Raw error text bytes, kept as a sibling of [`ErrorState`] so legacy
/// `R_curErrorBuf` pointers do not pass through an exclusive state lend.
pub(crate) type ErrorBuffer = Cell<[u8; crate::mainutils::errors::BUFSIZE + 1]>;

pub(crate) struct ErrorState {
    pub warn_length: c_int,
    pub show_error_messages: bool,
    pub show_error_calls: bool,
    pub show_warn_calls: bool,
    pub in_error: c_int,
    pub in_warning: c_int,
    pub in_print_warnings: c_int,
    pub immediate_warning: bool,
    /// Depth of active `suppressWarnings()` frames: C-level warnings raised
    /// inside are dropped before collection, matching upstream where the
    /// muffle restart prevents `vwarningcall_dflt` from ever running.
    pub suppress_warnings: c_int,
    /// Depth of active `suppressMessages()` frames: message emission inside
    /// is dropped, mirroring the suppress_warnings design for the port's
    /// signal-time message path.
    pub suppress_messages: c_int,
    pub no_break_warning: bool,
    pub interrupts_suspended: bool,
    pub interrupts_pending: bool,
    pub collect_warnings: c_int,
    pub nwarnings: c_int,
    pub warnings: RuntimeValue,
    pub handler_stack: RuntimeValue,
    /// R-level `globalCallingHandlers()` list (named functions).
    pub global_calling_handlers: RuntimeValue,

    pub restart_stack: RuntimeValue,
    pub expressions: c_int,
    pub expressions_keep: c_int,
    /// Message of the most recent error rendered into `error_buffer` by
    /// `verrorcall_dflt`, if any. The top-level renderer only trusts the
    /// error buffer when the escaping error's message matches this, so stale
    /// renders from previously caught errors are ignored (mirrors upstream,
    /// where caught errors never reach `verrorcall_dflt` printing).
    pub last_rendered_message: Option<crate::mainutils::errors::RenderedMessage>,
    /// 1-based index of the top-level expression a session script loop is
    /// currently evaluating (0 = no script position is active). The port
    /// parses scripts without srcrefs, so the loop's expression index stands
    /// in for upstream's most-recent-known srcref (`R_Srcref`): the
    /// `show.error.locations` render uses it as the `(from #n)` location.
    pub toplevel_expr_no: usize,
    /// Call attributed to warnings raised while it is set (null = no
    /// override). Base closures that wrap `.Internal` entry points (e.g.
    /// `base::complex`) are single builtins in this port, so a warning
    /// raised inside them has no closure context to attribute through;
    /// handlers install their own call here for the handler's duration.
    pub warning_call: RuntimeValue,
    /// Condition object currently being signaled by `stop(<condition>)`.
    /// This is session state so a caught condition cannot leak across
    /// sequential sessions sharing an OS thread.
    pub signalled_condition: RuntimeValue,
    /// Set by `verrorcall_dflt` when the attributed call is empty
    /// (`errorcall(R_NilValue)` / `call. = FALSE`). GNU `try()` then
    /// prints `Error : ` and attaches a condition with a NULL call.
    pub call_less: bool,
    /// Call used to attribute warnings emitted by an nmath evaluation.
    pub mathlib_warning_call: RuntimeValue,
    /// Non-SEXP handler bookkeeping must follow the active session when
    /// sessions are nested on one host thread.
    pub calling_handlers_signaled: bool,
    pub try_catch_handler_classes: Vec<Vec<String>>,
    /// Call recorded by the most recent error raise (`errorcall` / unused-arg).
    /// `null` means unset; `R_NilValue` means `call. = FALSE`.
    pub last_error_call: RuntimeValue,
    /// True when `last_error_call` is the applied language object (matchArgs,
    /// builtin `attribute_handler_errors`), not `getCurrentCall()` from `stop()`.
    pub last_error_call_explicit: bool,
    /// `framedepth` at the raise site; compared with the enclosing tryCatch
    /// entry depth so `stop()` inside `f()` keeps `f()` rather than doTryCatch.
    pub last_error_nframe: i32,
    /// `framedepth` snapshots pushed with `try_catch_handler_classes`.
    pub try_catch_nframes: Vec<i32>,
    pub current_srcref_location: Option<(String, i32)>,
    /// `sequence()` announces the upcoming recycling default once per session.
    pub sequence_recycling_warned: bool,
}

impl Default for ErrorState {
    fn default() -> Self {
        ErrorState {
            warn_length: 1000,
            show_error_messages: true,
            show_error_calls: false,
            show_warn_calls: false,
            suppress_warnings: 0,
            suppress_messages: 0,
            in_warning: 0,
            in_error: 0,
            in_print_warnings: 0,
            immediate_warning: false,
            no_break_warning: false,
            interrupts_suspended: false,
            interrupts_pending: false,
            collect_warnings: 0,
            toplevel_expr_no: 0,
            nwarnings: 50,
            last_rendered_message: None,
            warning_call: RuntimeValue::empty(),
            signalled_condition: RuntimeValue::empty(),
            call_less: false,
            mathlib_warning_call: RuntimeValue::empty(),
            calling_handlers_signaled: false,
            try_catch_handler_classes: Vec::new(),
            last_error_call: RuntimeValue::empty(),
            last_error_call_explicit: false,
            last_error_nframe: 0,
            try_catch_nframes: Vec::new(),
            current_srcref_location: None,
            sequence_recycling_warned: false,
            warnings: RuntimeValue::empty(),
            handler_stack: RuntimeValue::empty(),
            global_calling_handlers: RuntimeValue::empty(),

            restart_stack: RuntimeValue::empty(),
            expressions: 500,
            expressions_keep: 500,
        }
    }
}

pub(crate) const PROFILING_OPCODE_COUNT: usize = 256;
pub(crate) const NO_PROFILING_OPCODE: c_int = -1;

/// Canonical runtime value ownership, shared with execution-context fields.
/// Each occupied slot owns the original allocation and lifetime witnesses.
/// Raw compatibility getters project from this value; GC never reconstructs it.
pub(crate) type RuntimeValue = super::context::ContextValue;

pub(crate) const PARSE_ERROR_BYTES: usize = 256;
/// Raw parse-error message bytes, kept as a sibling of [`ParseDiagnostics`]
/// so legacy `R_GetParseErrorMsg` pointers do not pass through an exclusive
/// diagnostic lend.
pub(crate) type ParseErrorBuffer = Cell<[u8; PARSE_ERROR_BYTES]>;

#[derive(Default)]
pub(crate) struct ParseDiagnostics {
    pub status: c_int,
    pub column: c_int,
    pub file: RuntimeValue,
    pub context_line: c_int,
    pub context: Vec<String>,
}

pub(crate) struct ProfilingState {
    pub sref: RuntimeValue,
    pub in_bc_interpreter: RuntimeValue,
    pub bcbody: RuntimeValue,
    pub bcrelpc: isize,
    pub profiling: c_int,
    pub mem_profiling: c_int,
    pub gc_profiling: c_int,
    pub line_profiling: c_int,
    pub filter_callframes: c_int,
    pub profiling_error: c_int,
    pub bc_profiling: c_int,
    pub current_opcode: c_int,
    pub opcode_counts: [c_int; PROFILING_OPCODE_COUNT],
    pub profiling_event: c_int,
    pub profile_schedule: Option<crate::eval::profiling::ProfileSchedule>,
    pub profile_generation: u64,
    pub sampling_in_progress: bool,
    pub profile_outfile: c_int,
    pub memory_peak_bytes: usize,
    /// Completed filename offsets into the owning byte buffer.
    pub srcfiles: Vec<usize>,
    pub srcfiles_reservation: Option<super::memory::WorkspaceReservation>,
    pub srcfile_bytes_used: usize,
    pub srcfile_bufcount: usize,
    pub srcfiles_buffer: RuntimeValue,
}

impl Default for ProfilingState {
    fn default() -> Self {
        ProfilingState {
            sref: RuntimeValue::empty(),
            in_bc_interpreter: RuntimeValue::empty(),
            bcbody: RuntimeValue::empty(),
            bcrelpc: -1,
            profiling: 0,
            mem_profiling: 0,
            gc_profiling: 0,
            line_profiling: 0,
            filter_callframes: 0,
            profiling_error: 0,
            bc_profiling: 0,
            current_opcode: NO_PROFILING_OPCODE,
            opcode_counts: [0; PROFILING_OPCODE_COUNT],
            profiling_event: 0,
            profile_schedule: None,
            profile_generation: 0,
            sampling_in_progress: false,
            profile_outfile: -1,
            memory_peak_bytes: 0,
            srcfiles: Vec::new(),
            srcfiles_reservation: None,
            srcfile_bytes_used: 0,
            srcfile_bufcount: 0,
            srcfiles_buffer: RuntimeValue::empty(),
        }
    }
}

/// Lend the original owner's profiler state for a strictly local operation.
///
/// # Safety
/// `instance` must remain live physically, with no overlapping whole-instance
/// borrow. `f` must not call into R or start collection.
pub(crate) unsafe fn with_profiling_state_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut ProfilingState) -> R,
) -> R {
    // SAFETY: the caller retains the allocation; RefCell checks overlapping lends.
    let cell = unsafe { &(*instance).profiling };
    f(&mut cell.borrow_mut())
}

/// Read the sampling flags without lending state to a caller.
///
/// # Safety
/// `instance` is live and the current call retains access to the allocation.
pub(crate) unsafe fn profiling_scheduled_in(instance: *mut RInstance) -> bool {
    // SAFETY: short shared field projection under the caller's liveness guarantee.
    let state = unsafe { &(*instance).profiling }.borrow();
    state.profiling != 0 && state.profile_schedule.is_some()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionCapabilities {
    pub allow_system_commands: bool,
    pub allow_pipe_commands: bool,
    /// Permit host-process mutations such as Sys.setenv/Sys.unsetenv,
    /// setwd, and Sys.setlocale. These are opt-in because process state is
    /// global rather than session-local.
    pub allow_environment_mutation: bool,
}

pub(crate) struct ReplFlags {
    pub(crate) no_echo: Cell<c_int>,
    pub(crate) quiet: Cell<c_int>,
    pub(crate) interactive: Cell<c_int>,
    pub(crate) verbose: Cell<c_int>,
    // These compatibility values do not own protection roots or deferred warnings.
    pub(crate) pp_stack_top: Cell<c_int>,
    pub(crate) collect_warnings: Cell<c_int>,
}

impl Default for ReplFlags {
    fn default() -> Self {
        Self {
            no_echo: Cell::new(0),
            quiet: Cell::new(0),
            interactive: Cell::new(1),
            verbose: Cell::new(0),
            pp_stack_top: Cell::new(0),
            collect_warnings: Cell::new(0),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ScalarPrintState {
    pub(crate) print: crate::mainutils::format::RPrint,
    pub(crate) character_active: bool,
}

pub(crate) struct TraceDebugFlags {
    pub(crate) tracing: Cell<c_int>,
    pub(crate) debugging: Cell<c_int>,
}

impl Default for TraceDebugFlags {
    fn default() -> Self {
        Self {
            tracing: Cell::new(super::ffi::TRUE),
            debugging: Cell::new(super::ffi::TRUE),
        }
    }
}

pub(crate) struct ExecutionState {
    pub(crate) eval_depth: c_int,
    pub(crate) eval_depth_limit: c_int,
    pub(crate) limits: crate::eval::limits::EvalLimits,
    pub(crate) start_time: Option<Instant>,
    pub(crate) cancellation: Option<super::session::CancellationToken>,
}

impl Default for ExecutionState {
    fn default() -> Self {
        Self {
            eval_depth: 0,
            eval_depth_limit: 500,
            limits: crate::eval::limits::EvalLimits::default(),
            start_time: None,
            cancellation: None,
        }
    }
}

pub(crate) type ImportedBytecodeMetadata = [Sexp<'static>; 2];

/// All per-instance state for one independent R session.
///
/// Each `RInstance` has its own arena, environments, and protection stack,
/// completely isolated from other instances.
///
/// `RInstance` is deliberately thread-confined. The compatibility dispatch
/// layer stores a raw current-instance pointer in thread-local state, so moving
/// an instance to another thread could leave the original thread with a stale
/// active-session pointer.
///
/// The raw SEXP pointers inside are valid for as long as the arena is alive.
pub struct RInstance {
    // Only this allocation owns a strong token. It is invalidated before any
    // owned field is destroyed; weak observers never prolong its lifetime.
    liveness: Option<LivenessOwner>,
    /// Weak capability of the sole stable session allocation; never a second runtime.
    pub(crate) runtime_owner: Option<super::owner::WeakOwner>,
    /// Immutable ownership domain; readable while the arena is lent.
    pub(crate) heap_identity: super::heap::HeapIdentity,
    /// Arena allocator for this instance.
    pub arena: RArena,
    /// Sole Rust ownership of persistent headers and symbol-name bytes.
    pub(crate) persistent_nodes: PersistentHeap,
    /// The global environment for this instance.
    pub global_env: SEXP,
    /// The base environment for this instance.
    pub base_env: SEXP,
    /// The empty environment for this instance.
    pub empty_env: SEXP,
    /// Owned environment sentinel nodes for empty/base/global environments.
    ///
    /// Compatibility projections; `persistent_nodes` owns their stable cells.
    pub(crate) env_nodes: Vec<*mut SexprecCore>,
    /// Whether this instance has completed R-level initialization.
    pub(crate) initialized: bool,
    /// Legacy C-port protection stack (strict LIFO, count-based release).
    /// See `protect::LegacyProtectionStack` for the discipline.
    pub(crate) legacy_protect: Rc<super::protect::LegacyProtectionStack>,
    /// Stable, generational root table for Rust-side handle guards
    /// (`RootedSexp` / `ProtectGuard` / `IndexedProtectGuard`): tombstone +
    /// free-list slots released in any order; never truncated by count-based
    /// legacy ops. See `protect::RootTable`.
    pub(crate) root_table: Rc<super::protect::RootTable>,
    /// The permanent preserve stack for this instance.
    pub(crate) preserve_stack: Rc<super::protect::PreservedRoots>,
    /// Cached closures own their exact GC roots until eviction or teardown.
    pub(crate) base_wrappers: RefCell<HashMap<&'static str, super::object::Sexp<'static>>>,
    /// Per-instance execution context stack.
    #[allow(clippy::vec_box)]
    pub(crate) context_stack: Vec<Rc<std::cell::UnsafeCell<super::context::RCNTXT>>>,
    /// Per-instance in-error flag.
    pub(crate) in_error: bool,
    /// Per-instance generational GC state.
    gc_state: RefCell<super::gengc::GcState>,
    /// Per-instance error, warning, interrupt, and expression-limit state.
    error_state: RefCell<ErrorState>,
    // Legacy raw pointers to these bytes must not pass through an exclusive ErrorState lend.
    error_buffer: ErrorBuffer,
    /// Per-instance main memory/GC control state.
    memory_state: RefCell<crate::mainutils::memory_main::MemoryRuntimeState>,
    /// Per-instance finalizer queue and dispatch state.
    finalizer_state: RefCell<crate::mainutils::memory_main::FinalizerState>,
    /// Opt-in permissions for host-process operations.
    capabilities: Cell<SessionCapabilities>,
    /// Original-session print recursion and tag handles.
    print_runtime: crate::mainutils::print::PrintRuntimeState,
    /// Dispatch names for negative primitive offsets in this session.
    portable_primitive_names: RefCell<Vec<String>>,
    /// Print defaults snapshot owning exact-generation roots for its raw view.
    print_data: RefCell<crate::mainutils::print::PrintDataSnapshot>,
    /// Separate storage for legacy printer results.
    printutils: crate::mainutils::printutils::PrintUtilsState,
    /// Scalar formatter settings and the character-coercion override flag.
    scalar_print: Cell<ScalarPrintState>,
    /// Deparse line limit, separate from the evaluator state.
    deparse_state: RefCell<crate::mainutils::deparse::DeparseRuntimeState>,
    /// REPL and compatibility scalar flags; shared Cell access, no state lend.
    repl_flags: ReplFlags,
    /// Session-wide trace and debug switches.
    trace_debug_flags: TraceDebugFlags,
    /// Result visibility, copied without lending evaluator state.
    visibility: Cell<c_int>,
    /// Evaluation limits and counters; borrows end before runtime reentry.
    execution_state: RefCell<ExecutionState>,
    /// Owning marker for tail-call continuations in this session.
    exec_token: RefCell<RuntimeValue>,
    /// Owning source call for the current complex assignment.
    assignment_expr: RefCell<RuntimeValue>,
    parse_diagnostics: RefCell<ParseDiagnostics>,
    // The legacy message pointer must not pass through an exclusive diagnostic lend.
    parse_error_buffer: ParseErrorBuffer,
    /// Session-local profiler counters and owning source references.
    profiling: RefCell<ProfilingState>,
    /// Per-instance names.c runtime caches and initialization marker.
    pub(crate) names_state: crate::mainutils::names::NamesRuntimeState,
    /// Per-instance bind.c cached sentinels.
    pub(crate) bind_state: crate::mainutils::bind::BindRuntimeState,
    /// Per-instance S3/S4 object dispatch state.
    pub(crate) objects_state: crate::mainutils::objects::ObjectsRuntimeState,
    /// Per-instance dotcode/native-call runtime policy cache.
    pub(crate) dotcode_state: crate::mainutils::dotcode::DotcodeRuntimeState,
    /// Per-instance ALTREP class-method registry.
    pub(crate) altrep_state: crate::sexp::altrep::AltrepRuntimeState,
    /// Rooted metadata shared by imported GNU bytecode in this instance.
    imported_bytecode_metadata: RefCell<Option<ImportedBytecodeMetadata>>,
    /// Per-instance LAPACK module dispatcher.
    pub(crate) lapack_state: crate::mainutils::lapack::LapackRuntimeState,
    /// Per-instance internet module state.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) internet_state: crate::modules::internet::internet::InternetRuntimeState,
    /// Per-instance Unix standard console/event callback state.
    pub(crate) sys_std_state: crate::unix::sys_std::SysStdRuntimeState,
    /// Per-instance Unix platform scratch buffer and process timing state.
    pub(crate) sys_unix_state: crate::unix::sys_unix::SysUnixRuntimeState,
    /// Per-instance Unix initialization, console dispatch, and stack state.
    pub(crate) unix_system_state: crate::unix::system::UnixSystemRuntimeState,
    /// Per-instance startup/workspace metadata.
    pub(crate) startup_state: crate::mainutils::startup::StartupRuntimeState,
    /// Per-instance main.c top-level loop and callback state.
    pub(crate) main_state: crate::mainutils::main::MainRuntimeState,
    /// Per-instance gettext catalog and domain state.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub(crate) intl_state: crate::intl::types::IntlRuntimeState,
    /// Per-instance timezone cache for the root tzone module.
    pub(crate) tzone_state: crate::tzone::TzRuntimeState,
    /// Per-instance symbol table for session-local interning.
    pub(crate) symbols: HashMap<String, SEXP>,
    /// Owned SYMSXP nodes for the per-instance symbol table.
    ///
    /// Compatibility projections for symbols and their printed names;
    /// `persistent_nodes` owns both headers and character storage.
    pub(crate) symbol_nodes: Vec<*mut SexprecCore>,
    /// Per-instance Marsaglia-MultiCarry RNG seed state.
    /// Per-instance Marsaglia-MultiCarry RNG seed state (shared with nmath samplers).
    pub(crate) rng_state: rmath_nmath::RngState,
    /// Per-instance R-level RNG kind selected by RNGkind().
    pub(crate) rng_kind: i32,
    /// Per-instance R-level MT RNG state used by mainutils::rng.
    pub(crate) main_rng_state: crate::mainutils::rng::MainRngState,
    /// Per-instance R RNG.c-style state used by mainutils::random.
    pub(crate) random_state: crate::mainutils::random::RNGState,
    /// Per-instance stdout/stderr capture buffers.
    pub(crate) output_capture: RefCell<super::output::OutputCaptureState>,
    /// Per-session process-environment overlay. Seeded from the host
    /// environment at session creation and then owned entirely by the
    /// session: Sys.setenv/Sys.unsetenv never mutate process-wide state, so
    /// concurrent sessions cannot data-race through libc environ.
    pub(crate) session_env: RefCell<std::collections::BTreeMap<String, String>>,
    /// Per-instance options storage (mirrors the global OPTIONS_TABLE).
    pub(crate) options: HashMap<String, super::object::Sexp<'static>>,
    /// Defaults are published through one explicit, reentry-safe phase.
    pub(crate) options_initialization: crate::mainutils::options::OptionsInitialization,
    /// Initial table size of hashed environments, keyed by environment address.
    /// GC contract: entries whose environment is collected are swept by the
    /// `gengc` reference-update pass (the address would otherwise alias a
    /// recycled node and `env.profile` would report the previous size).
    pub(crate) env_hash_sizes: hashbrown::HashMap<usize, i32>,
    /// Per-instance locked environments keyed by raw environment address.
    /// GC contract: entries whose keyed environment is collected are swept by
    /// the `gengc` reference-update pass (the address would otherwise alias a
    /// recycled node).
    pub(crate) locked_environments: HashSet<usize>,
    /// Per-instance locked bindings keyed by (environment address, symbol
    /// address). GC contract: entries whose keyed env or symbol is collected
    /// are swept by the `gengc` reference-update pass.
    pub(crate) locked_bindings: HashSet<(usize, usize)>,
    /// Per-instance active bindings keyed by (environment address, symbol
    /// address). GC contract: values are traced as roots, and entries whose
    /// keyed env or symbol is collected are swept by the `gengc`
    /// reference-update pass.
    pub(crate) active_bindings: HashMap<(usize, usize), SEXP>,
    /// Per-session nmath math state (sampler caches and rank memo tables).
    pub(crate) math_state: rmath_nmath::MathState,
    /// Registered signed-rank counts live in a charged, rooted original-owner R vector.
    pub(crate) signed_rank_cache: Option<crate::library::stats::distn::signrank::Cache>,
    /// Original-runtime projection-pursuit and spline smoother settings.
    pub(crate) ppr_settings: Cell<crate::library::stats::smart::Common>,
    /// Session-local compiler options, independent of evaluator stack state.
    pub(crate) jit_settings: Cell<crate::eval::jit::JitSettings>,
    /// Per-instance stats::bspline recurrence continuation state.
    pub(crate) bspline_state: crate::library::stats::bspline::BsplineState,
    /// Per-instance dynamic loader and native package registry state.
    pub(crate) dynload_state: crate::mainutils::rdynload::DynloadState,
    /// Per-instance connection table.
    pub(crate) connection_table:
        Rc<RefCell<crate::mainutils::connections::ConnectionTable>>,
    /// Session-local output routing. Capture cleanup retains this state independently.
    pub(crate) sink_state: Rc<RefCell<crate::mainutils::connections::SinkState>>,
    pub(crate) browser_files: Rc<crate::mainutils::browser_files::BrowserFileState>,
    /// Per-instance library, cache, and temporary-directory policy.
    pub(crate) path_policy: crate::mainutils::paths::RuntimePathPolicy,
    /// Per-instance file creation mask used by `Sys.umask()`.
    pub(crate) file_creation_umask: u32,
    /// Per-instance cache of pure-R package namespaces keyed by package name.
    /// GC contract: the cached namespace env is traced as a root by `gengc`
    /// (it may have no other referee) and remapped on reference updates.
    pub(crate) package_namespace_cache: HashMap<String, (std::path::PathBuf, SEXP)>,
    /// Selected conditional directives belong to this session's platform and policy.
    /// Plain strings only; this cache retains no interpreter graph.
    pub(crate) namespace_directives_cache:
        HashMap<std::path::PathBuf, crate::mainutils::essentials::NamespaceDirectives>,

    /// Package source directory currently being sourced into a namespace
    /// (set for the duration of `source_package_r_files`): relative file
    /// paths opened by package R code — e.g. crayon's install-time
    /// `read.table("tools/ansi-palettes.txt", ...)` — resolve against it,
    /// standing in for upstream's install-time evaluation against the
    /// package directory.
    pub(crate) loading_package_dir: Option<std::path::PathBuf>,
    /// Packages whose namespace load is still on this session's stack.
    ///
    /// A fresh dependency vector cannot see an outer load, so cache hits
    /// would re-run native installers. Keeping the stack on the session
    /// also keeps a same-thread session switch from observing another
    /// session's loads.
    pub(crate) namespace_loads_in_progress: Vec<String>,
    /// Per-instance headless graphics device registry.
    pub(crate) graphics_device_registry: crate::library::grdevices::device_registry::DeviceRegistry,
    /// Per-instance graphics engine registration state.
    pub(crate) graphics_engine_state: crate::mainutils::engine::GraphicsEngineState,
    /// Optional current DrawTarget (RenderPlot) backend (when the "renderplot-device" feature is enabled).
    /// Used by the DeviceRegistry drawing fns to forward high-quality drawing (skia etc.)
    /// for real R graphics calls on Android/WASM hosts.
    #[cfg(feature = "renderplot-device")]
    pub(crate) current_renderplot_backend: Option<*mut dyn r_graphics_engine::DrawTarget>,
    #[cfg(feature = "renderplot-device")]
    pub(crate) portable_grid: crate::mainutils::portable_grid::GridState,
    #[cfg(feature = "renderplot-device")]
    pub(crate) portable_graphics: crate::mainutils::portable_plot::GraphicsState,
    #[cfg(feature = "renderplot-device")]
    pub(crate) graphics_recording:
        Option<std::rc::Rc<std::cell::RefCell<r_graphics_engine::Scene>>>,
    #[cfg(feature = "renderplot-device")]
    pub(crate) grid_recording_origin: Option<crate::sexp::object::Sexp<'static>>,
    /// Per-instance base graphics `par()` overrides.
    pub(crate) graphics_par_state: crate::library::graphics::par::GraphicsParState,
    /// Per-instance grDevices color palette and scratch buffer state.
    pub(crate) graphics_color_state: crate::library::grdevices::colors::GraphicsColorState,
    /// Per-instance main/colors.c dispatch pointers installed by grDevices.
    pub(crate) color_dispatch_state: crate::mainutils::colors::ColorDispatchState,
    /// Per-instance grid runtime state.
    pub(crate) grid_runtime_state: crate::library::grid::types::GridRuntimeState,
    /// Per-instance graphics::plot3d scratch transformation state.
    pub(crate) plot3d_state: crate::library::graphics::plot3d::Plot3dState,
    /// Per-instance methods package dispatch flags and cache counters.
    pub(crate) methods_dispatch_state:
        crate::library::methods::methods_list_dispatch::MethodsDispatchState,
    /// Per-instance raw cons cells allocated outside the arena.
    pub(crate) raw_cons: Vec<*mut SexprecCore>,
    /// Per-instance transient allocations for R_alloc/vmaxget/vmaxset.
    pub(crate) vmax: Vec<super::memory_ext::TransientAllocation>,
}

/// Configure a child process to inherit exactly the session-owned
/// environment. GNU R semantics make system()/pipe children observe
/// Sys.setenv writes; because the overlay is a complete seeded copy rather
/// than a diff, clearing first also honors Sys.unsetenv removals. Without an
/// active session the child inherits the host environment unchanged.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn apply_session_env(command: &mut std::process::Command) {
    if let Some(env) = with_current_instance(|inst| unsafe { (*inst).session_env.borrow().clone() })
    {
        command.env_clear();
        command.envs(env);
    }
}

/// wasm32 cannot spawn child processes; call sites that still compile there
/// get a no-op because there is no process environment to propagate.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
pub(crate) fn apply_session_env(_command: &mut std::process::Command) {}

/// Capture the host process environment once at session creation. Reads are
/// immutable snapshots taken before any thread could mutate the environment;
/// after creation the session owns its overlay exclusively. wasm32 has no
/// process environment, so sessions start empty there.
fn seed_session_env() -> std::collections::BTreeMap<String, String> {
    #[cfg(target_arch = "wasm32")]
    {
        std::collections::BTreeMap::new()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::vars_os()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.to_string_lossy().into_owned(),
                )
            })
            .collect()
    }
}

impl RInstance {
    pub(crate) fn capabilities(&self) -> SessionCapabilities {
        self.capabilities.get()
    }

    pub(crate) fn set_capabilities(&self, capabilities: SessionCapabilities) {
        self.capabilities.set(capabilities);
    }

    /// Read the current result visibility for this instance.
    #[inline]
    pub(crate) fn visibility(&self) -> c_int {
        self.visibility.get()
    }

    /// Allocate independent runtime storage without executing R code.
    ///
    /// Initialized interpreters are constructed by RSession, which installs
    /// the original weak runtime authority before evaluating base bindings.
    /// This low-level storage constructor creates the three environment
    /// sentinels and an empty arena; it does not publish a movable interpreter.
    pub fn new() -> Self {
        Self::allocate_with_path_policy(crate::mainutils::paths::RuntimePathPolicy::default())
    }

    /// Allocate session storage before its stable shared owner is installed.
    /// Base evaluation must run only after installing that original owner.
    pub(crate) fn allocate_for_session() -> Self {
        Self::allocate_with_path_policy(crate::mainutils::paths::RuntimePathPolicy::default())
    }

    /// Real session storage without evaluating the base library. Intended for
    /// collector and ownership tests whose subject is independent of R code.
    #[cfg(test)]
    pub(crate) fn new_for_gc_tests() -> Self {
        Self::allocate_with_path_policy(crate::mainutils::paths::RuntimePathPolicy::for_gc_tests())
    }

    pub(crate) fn allocate_with_path_policy(
        path_policy: crate::mainutils::paths::RuntimePathPolicy,
    ) -> Self {
        let nil = super::globals::R_NilValue();

        let arena = RArena::new();
        let heap_identity = arena.heap_identity();
        let mut persistent_nodes = PersistentHeap::new(heap_identity.clone());
        let mut env_nodes = Vec::with_capacity(3);
        let empty_env = Self::push_env(&mut persistent_nodes, &mut env_nodes, nil, nil, nil);
        let base_env = Self::push_env(&mut persistent_nodes, &mut env_nodes, nil, empty_env, nil);
        let global_env = Self::push_env(&mut persistent_nodes, &mut env_nodes, nil, base_env, nil);

        let root_table =
            Rc::new(super::protect::RootTable::new(heap_identity.byte_ledger()));
        let legacy_protect =
            Rc::new(super::protect::LegacyProtectionStack::new(heap_identity.byte_ledger()));
        let preserve_stack =
            Rc::new(super::protect::PreservedRoots::new(heap_identity.byte_ledger()));
        RInstance {
            liveness: Some(LivenessOwner::new()),
            runtime_owner: None,
            heap_identity,
            arena,
            persistent_nodes,
            global_env,
            base_env,
            empty_env,
            env_nodes,
            initialized: false,
            legacy_protect,
            root_table,
            preserve_stack,
            base_wrappers: RefCell::default(),
            context_stack: Vec::new(),
            in_error: false,
            gc_state: RefCell::default(),
            error_state: RefCell::default(),
            error_buffer: Cell::new([0; crate::mainutils::errors::BUFSIZE + 1]),
            memory_state: RefCell::default(),
            finalizer_state: RefCell::default(),
            capabilities: Cell::default(),
            print_runtime: crate::mainutils::print::PrintRuntimeState::default(),
            portable_primitive_names: RefCell::default(),
            print_data: RefCell::default(),
            printutils: crate::mainutils::printutils::PrintUtilsState::default(),
            scalar_print: Cell::default(),
            deparse_state: RefCell::default(),
            repl_flags: ReplFlags::default(),
            trace_debug_flags: TraceDebugFlags::default(),
            visibility: Cell::new(super::ffi::TRUE),
            execution_state: RefCell::default(),
            exec_token: RefCell::default(),
            assignment_expr: RefCell::default(),
            parse_diagnostics: RefCell::default(),
            parse_error_buffer: Cell::new([0; PARSE_ERROR_BYTES]),
            profiling: RefCell::default(),
            names_state: crate::mainutils::names::NamesRuntimeState::default(),
            bind_state: crate::mainutils::bind::BindRuntimeState::default(),
            objects_state: crate::mainutils::objects::ObjectsRuntimeState::default(),
            dotcode_state: crate::mainutils::dotcode::DotcodeRuntimeState::default(),
            altrep_state: crate::sexp::altrep::AltrepRuntimeState::default(),
            imported_bytecode_metadata: RefCell::new(None),
            lapack_state: crate::mainutils::lapack::LapackRuntimeState::default(),
            #[cfg(not(target_arch = "wasm32"))]
            internet_state: crate::modules::internet::internet::InternetRuntimeState::default(),
            sys_std_state: crate::unix::sys_std::SysStdRuntimeState::default(),
            sys_unix_state: crate::unix::sys_unix::SysUnixRuntimeState::default(),
            unix_system_state: crate::unix::system::UnixSystemRuntimeState::default(),
            startup_state: crate::mainutils::startup::StartupRuntimeState::default(),
            main_state: crate::mainutils::main::MainRuntimeState::default(),
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            intl_state: crate::intl::types::IntlRuntimeState::default(),
            tzone_state: crate::tzone::TzRuntimeState::default(),
            symbols: HashMap::new(),
            symbol_nodes: Vec::new(),
            rng_state: rmath_nmath::RngState::default(),
            rng_kind: 0,
            main_rng_state: crate::mainutils::rng::MainRngState::default(),
            random_state: crate::mainutils::random::RNGState::new(),
            output_capture: RefCell::new(super::output::OutputCaptureState::default()),
            session_env: RefCell::new(seed_session_env()),
            options: HashMap::new(),
            options_initialization: crate::mainutils::options::OptionsInitialization::Uninitialized,
            env_hash_sizes: hashbrown::HashMap::new(),
            locked_environments: HashSet::new(),
            locked_bindings: HashSet::new(),
            active_bindings: HashMap::new(),
            math_state: rmath_nmath::MathState::default(),
            signed_rank_cache: None,
            ppr_settings: Cell::new(crate::library::stats::smart::Common::default()),
            jit_settings: Cell::default(),
            bspline_state: crate::library::stats::bspline::BsplineState::default(),
            package_namespace_cache: HashMap::new(),
            namespace_directives_cache: HashMap::new(),

            loading_package_dir: None,
            namespace_loads_in_progress: Vec::new(),
            dynload_state: crate::mainutils::rdynload::DynloadState::default(),
            connection_table: Rc::default(),
            sink_state: Rc::default(),
            browser_files: Rc::default(),
            path_policy,
            file_creation_umask: 0o022,
            graphics_device_registry:
                crate::library::grdevices::device_registry::DeviceRegistry::default(),
            graphics_engine_state: crate::mainutils::engine::GraphicsEngineState::default(),
            graphics_par_state: crate::library::graphics::par::GraphicsParState::default(),
            #[cfg(feature = "renderplot-device")]
            current_renderplot_backend: None,
            #[cfg(feature = "renderplot-device")]
            portable_grid: crate::mainutils::portable_grid::GridState::default(),
            #[cfg(feature = "renderplot-device")]
            portable_graphics: crate::mainutils::portable_plot::GraphicsState::default(),
            #[cfg(feature = "renderplot-device")]
            graphics_recording: None,
            #[cfg(feature = "renderplot-device")]
            grid_recording_origin: None,
            graphics_color_state: crate::library::grdevices::colors::GraphicsColorState::default(),
            color_dispatch_state: crate::mainutils::colors::ColorDispatchState::default(),
            grid_runtime_state: crate::library::grid::types::GridRuntimeState::default(),
            plot3d_state: crate::library::graphics::plot3d::Plot3dState::default(),
            methods_dispatch_state:
                crate::library::methods::methods_list_dispatch::MethodsDispatchState::default(),
            raw_cons: Vec::new(),
            vmax: Vec::new(),
        }
    }

    /// Initialize base bindings on the original stable session allocation.
    ///
    /// # Safety
    /// `instance` is the original shared-cell projection with its managed weak
    /// owner already installed. No instance or arena loan overlaps R execution.
    pub(crate) unsafe fn initialize_base_bindings_via(instance: *mut RInstance) {
        // The activation retains the original session allocation across base
        // evaluation and restores ambient state even when bootstrap unwinds.
        unsafe {
            let owner = (*instance)
                .runtime_owner
                .as_ref()
                .expect("base initialization requires a managed runtime owner")
                .clone();
            let pin = owner.pin().expect("live base initialization owner");
            assert_eq!(
                pin.as_ptr(),
                instance,
                "base initialization uses its original runtime"
            );
            super::session::with_instance_active(instance, || {
                // GNU's runtime has a canonical blank CHARSXP before it
                // evaluates base bindings. Install the owning arena's copy
                // now so every STRSXP allocated during bootstrap starts with
                // valid, session-local character references.
                let blank = super::super::mainutils::bind::R_BlankString();
                let installed = {
                    super::memory::with_arena_in(instance, |arena| {
                        arena.install_blank_string(blank)
                    })
                };
                assert!(
                    installed,
                    "installing the runtime blank string must succeed"
                );
                super::init::initialize_base_bindings((*instance).base_env);
                pin.require_live()
                    .expect("base initialization runtime was closed by a callback");
                (*instance).initialized = true;
            });
        }
    }

    /// Allocate an owned permanent environment header in stable Rust cells.
    fn push_env(
        persistent: &mut PersistentHeap,
        env_nodes: &mut Vec<*mut SexprecCore>,
        frame: SEXP,
        enclos: SEXP,
        hashtab: SEXP,
    ) -> SEXP {
        let mut header = SexprecCore::new(SEXPTYPE::ENVSXP);
        header.data = super::ffi::NodeBody::Environment(super::ffi::Envsxp {
            frame: persistent
                .link_from_projection(frame)
                .expect("permanent environment frame"),
            enclos: persistent
                .link_from_projection(enclos)
                .expect("permanent environment parent"),
            hashtab: persistent
                .link_from_projection(hashtab)
                .expect("permanent environment hash"),
        });
        let env = persistent
            .allocate_header(header)
            .expect("persistent environment allocation");
        env_nodes.push(env);
        env
    }

    /// Exact allocation identity for either managed or permanent storage.
    pub(crate) fn node_token(&self, pointer: SEXP) -> Option<super::heap::CheckedNode> {
        self.arena
            .node_token(pointer)
            .or_else(|| self.persistent_nodes.token(pointer))
    }

    /// An address lookup validates membership; only owned cells supply the
    /// pointer subsequently used to access the allocation.
    pub(crate) fn canonical_projection(&self, pointer: SEXP) -> Option<SEXP> {
        if let Some(node) = self.arena.node_token(pointer) {
            self.arena.resolve_node(node.id())
        } else {
            self.persistent_nodes.canonical_projection(pointer)
        }
    }

    /// Membership includes symbol names and persistent environments, whose
    /// stable headers share the arena's checked heap identity.
    pub(crate) fn owns_sexp(&self, pointer: SEXP) -> bool {
        self.arena.contains(pointer) || self.persistent_nodes.contains(pointer)
    }

    /// Set a RenderPlot backend for the duration of a render / graphics operation.
    /// The drawing fns in the device registry will forward to it (when the feature is enabled)
    /// so that real R plot()/grid calls produce output via the skia renderer.
    #[cfg(feature = "renderplot-device")]
    pub unsafe fn set_current_renderplot_backend(
        &mut self,
        backend: *mut dyn r_graphics_engine::DrawTarget,
    ) {
        self.current_renderplot_backend = Some(backend);
    }

    #[cfg(feature = "renderplot-device")]
    pub fn clear_current_renderplot_backend(&mut self) {
        self.current_renderplot_backend = None;
    }
}

/// Free functions (visible to embed etc. when the feature is enabled) to set the
/// current DrawTarget (RenderPlot) backend for the active instance (used during render to
/// forward drawing to skia for real R graphics).
#[cfg(feature = "renderplot-device")]
pub unsafe fn set_current_renderplot_backend(backend: *mut dyn r_graphics_engine::DrawTarget) {
    with_required_current_instance(|inst| unsafe {
        // P2: single-field write; no other raw path touches the instance
        // inside this closure.
        (*inst).current_renderplot_backend = Some(backend);
        (*inst).portable_graphics = crate::mainutils::portable_plot::GraphicsState::default();
    });
}

#[cfg(feature = "renderplot-device")]
pub fn clear_current_renderplot_backend() {
    with_required_current_instance(|inst| unsafe {
        // P2: single-field write; no other raw path touches the instance
        // inside this closure.
        (*inst).current_renderplot_backend = None;
        (*inst).portable_graphics = crate::mainutils::portable_plot::GraphicsState::default();
    });
}

impl Default for RInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RInstance {
    fn drop(&mut self) {
        // Provider destructors can run arbitrary Rust code while fields are
        // released. Mark the owner unavailable before teardown starts.
        drop(self.liveness.take());
        let profiling = self.profiling.get_mut();
        // Release canonical evaluator values while their original arena still
        // exists. Borrowed projections never participate in this teardown.
        *self.assignment_expr.get_mut() = RuntimeValue::empty();
        *self.print_data.get_mut() = crate::mainutils::print::PrintDataSnapshot::default();
        self.printutils.release_print_overrides();
        self.parse_diagnostics.get_mut().file = RuntimeValue::empty();
        *self.exec_token.get_mut() = RuntimeValue::empty();
        profiling.sref = RuntimeValue::empty();
        profiling.in_bc_interpreter = RuntimeValue::empty();
        profiling.bcbody = RuntimeValue::empty();
        profiling.bcrelpc = -1;
        profiling.srcfiles = Vec::new();
        profiling.srcfiles_reservation = None;
        profiling.srcfile_bytes_used = 0;
        profiling.srcfiles_buffer = RuntimeValue::empty();
        drop(self.imported_bytecode_metadata.get_mut().take());
        // Persistent headers and character bytes are owned Rust fields; their
        // pages invalidate outstanding checked identities during normal Drop.
        self.vmax.clear();
        if profiling.profile_outfile >= 0 {
            // Native only: on wasm32 the profiling stubs never open an
            // output file, so there is nothing to close.
            #[cfg(not(target_arch = "wasm32"))]
            unsafe {
                libc::close(profiling.profile_outfile);
            }
            profiling.profile_outfile = -1;
        }
    }
}

impl OwnerPin {
    /// Swap only formatter settings, including when cleaning up a revoked session.
    pub(crate) fn replace_format_print(
        &self,
        print: crate::mainutils::format::RPrint,
    ) -> crate::mainutils::format::RPrint {
        // SAFETY: the pin retains the allocation and the cell access runs no callbacks.
        unsafe {
            with_scalar_print_state_in(self.as_ptr(), |cell| {
                let mut state = cell.get();
                let previous = std::mem::replace(&mut state.print, print);
                cell.set(state);
                previous
            })
        }
    }

    /// The pin still retains this storage when restoring a revoked session.
    pub(crate) fn replace_browse_lines(&self, value: c_int) -> c_int {
        // SAFETY: the pin retains the allocation; only the deparse cell is borrowed.
        unsafe {
            with_deparse_state_in(self.as_ptr(), |state| {
                std::mem::replace(&mut state.browse_lines, value)
            })
        }
    }

    fn imported_bytecode_cache(&self) -> SexpResult<&RefCell<Option<ImportedBytecodeMetadata>>> {
        self.require_live()?;
        // SAFETY: this pin retains the original allocation. Only this shared
        // projection accesses the private cell while the owner is live.
        Ok(unsafe { &(*self.as_ptr()).imported_bytecode_metadata })
    }

    pub(crate) fn imported_bytecode_metadata(&self) -> SexpResult<Option<ImportedBytecodeMetadata>> {
        Ok(self.imported_bytecode_cache()?.borrow().clone())
    }

    pub(crate) fn publish_imported_bytecode_metadata(
        &self,
        values: ImportedBytecodeMetadata,
    ) -> SexpResult<ImportedBytecodeMetadata> {
        let mut cache = self.imported_bytecode_cache()?.borrow_mut();
        if let Some(published) = cache.as_ref() {
            return Ok(published.clone());
        }
        *cache = Some(values.clone());
        Ok(values)
    }
}

/// A teardown observation, not an ownership lease. Keeping this value alive
/// cannot keep a destroyed RInstance or its numerical state installed.
#[derive(Clone)]
pub(crate) struct InstanceLiveness(Weak<()>);
impl InstanceLiveness {
    pub(crate) fn is_live(&self) -> bool {
        self.0.strong_count() != 0
    }
}

/// Revoke all weak owner observations before terminal close or destruction.
/// This touches only the availability field, never lending the whole owner.
/// # Safety
/// The pointer is a live writable owner projection; no borrow of its
/// availability field overlaps this operation.
pub(crate) unsafe fn revoke_instance_availability(instance: *mut RInstance) {
    unsafe {
        drop((*instance).liveness.take());
    }
}

/// Snapshot an owner's teardown identity without borrowing its fields during
/// later callbacks. The control block also distinguishes reused addresses.
/// # Safety
/// The pointer must refer to a live allocation when this function is called.
/// A returned observer can subsequently outlive that allocation safely.
pub(crate) unsafe fn instance_liveness(instance: *mut RInstance) -> InstanceLiveness {
    let token = unsafe { &(*instance).liveness };
    InstanceLiveness(
        token
            .as_ref()
            .map_or_else(Weak::new, |owner| Rc::downgrade(&owner.strong)),
    )
}

/// Snapshot an owner's copyable teardown generation; see [`InstanceGeneration`].
/// # Safety
/// The pointer must refer to a live allocation when this function is called.
/// The returned witness can subsequently outlive that allocation safely.
pub(crate) unsafe fn instance_generation(instance: *mut RInstance) -> InstanceGeneration {
    let token = unsafe { &(*instance).liveness };
    token
        .as_ref()
        .map_or(InstanceGeneration::DEAD, |owner| owner.generation)
}

/// The strong side of both teardown witnesses. Dropping it (explicit
/// revocation or `RInstance::drop`) revokes the weak [`InstanceLiveness`]
/// observers and the copyable [`InstanceGeneration`] witnesses together,
/// before any other instance field is released.
struct LivenessOwner {
    strong: Rc<()>,
    generation: InstanceGeneration,
}

impl LivenessOwner {
    fn new() -> Self {
        Self {
            strong: Rc::new(()),
            generation: InstanceGeneration::acquire(),
        }
    }
}

impl Drop for LivenessOwner {
    fn drop(&mut self) {
        self.generation.release();
    }
}

/// Lend GC controls and heap limits for a local operation, not runtime reentry.
///
/// # Safety
/// `instance` must point to a live allocation throughout the lend, without
/// an overlapping mutable whole-instance reference.
pub(crate) unsafe fn with_memory_state_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut crate::mainutils::memory_main::MemoryRuntimeState) -> R,
) -> R {
    // SAFETY: the caller retains the allocation; project only this field.
    let cell = unsafe { &(*instance).memory_state };
    f(&mut cell.borrow_mut())
}

/// Lend evaluation controls for a local operation, not runtime reentry.
///
/// # Safety
/// `instance` must point to a live allocation throughout the lend, without
/// an overlapping mutable whole-instance reference.
pub(crate) unsafe fn with_execution_state_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut ExecutionState) -> R,
) -> R {
    // SAFETY: the caller retains the allocation; project only this field.
    let cell = unsafe { &(*instance).execution_state };
    f(&mut cell.borrow_mut())
}

/// Lend print data for a strictly local read or update.
///
/// # Safety
/// The original instance remains allocated and has no active whole-instance loan.
pub(crate) unsafe fn with_print_data_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut crate::mainutils::print::PrintDataSnapshot) -> R,
) -> R {
    // SAFETY: project only the checked cell from the retained instance.
    let cell = unsafe { &(*instance).print_data };
    f(&mut cell.borrow_mut())
}

/// Access legacy encoder storage on the original session.
///
/// # Safety
/// The original instance remains allocated and has no overlapping exclusive
/// whole-instance borrow for the duration of this local operation.
pub(crate) unsafe fn with_printutils_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&crate::mainutils::printutils::PrintUtilsState) -> R,
) -> R {
    // SAFETY: the caller retains the original allocation; the fields use interior mutability.
    f(unsafe { &(*instance).printutils })
}

/// Access the original session's portable primitive registry for a local operation.
///
/// # Safety
/// The instance must remain allocated during the call, without an overlapping
/// exclusive whole-instance borrow.
pub(crate) unsafe fn with_portable_primitive_names_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&RefCell<Vec<String>>) -> R,
) -> R {
    // SAFETY: the caller retains the original allocation.
    f(unsafe { &(*instance).portable_primitive_names })
}

/// Access only the scalar print cell, without lending the evaluator state.
///
/// # Safety
/// The original instance remains allocated and has no overlapping exclusive
/// whole-instance borrow for the duration of the local operation.
pub(crate) unsafe fn with_scalar_print_state_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&Cell<ScalarPrintState>) -> R,
) -> R {
    // SAFETY: the caller retains the original allocation.
    f(unsafe { &(*instance).scalar_print })
}

pub(crate) fn with_current_scalar_print_state<R>(
    f: impl FnOnce(&Cell<ScalarPrintState>) -> R,
) -> Option<R> {
    // SAFETY: the active allocation is retained for this local operation.
    with_current_instance(|instance| unsafe { with_scalar_print_state_in(instance, f) })
}

/// Lend deparse bookkeeping for a local operation.
///
/// # Safety
/// The original instance remains allocated and has no overlapping exclusive
/// whole-instance borrow.
pub(crate) unsafe fn with_deparse_state_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut crate::mainutils::deparse::DeparseRuntimeState) -> R,
) -> R {
    // SAFETY: project only the checked cell from the retained instance.
    let cell = unsafe { &(*instance).deparse_state };
    f(&mut cell.borrow_mut())
}

/// Copy the original instance's result visibility without lending its state.
///
/// # Safety
/// `instance` must remain physically allocated for the access, with no
/// overlapping exclusive borrow of the whole instance.
#[inline]
pub(crate) unsafe fn visibility_in(instance: *mut RInstance) -> c_int {
    // SAFETY: the caller retains the allocation; project only the scalar cell.
    unsafe { &(*instance).visibility }.get()
}

/// Update result visibility, including during cleanup of a revoked session.
///
/// # Safety
/// `instance` must remain physically allocated for the access, with no
/// overlapping exclusive borrow of the whole instance.
#[inline]
pub(crate) unsafe fn set_visibility_in(instance: *mut RInstance, value: c_int) {
    // SAFETY: the caller retains the allocation; project only the scalar cell.
    unsafe { &(*instance).visibility }.set(value);
}

/// Retain the token independently of its slot across allocation and callbacks.
///
/// # Safety
/// `instance` must remain live for the call, with no overlapping exclusive
/// borrow of the whole instance.
pub(crate) unsafe fn exec_token_in(instance: *mut RInstance) -> Option<super::object::Sexp<'static>> {
    // SAFETY: project only the checked slot in the retained allocation.
    let slot = unsafe { &(*instance).exec_token };
    slot.borrow().owned()
}

/// Install a validated token, or clear the slot if `token` is null.
///
/// # Safety
/// `instance` must remain live for the call, with no overlapping exclusive
/// borrow of the whole instance. A non-null `token` must be a live initialized
/// projection with no conflicting payload loan.
pub(crate) unsafe fn set_exec_token_in(instance: *mut RInstance, token: SEXP) {
    // Validate and retain the new value before borrowing the slot.
    let replacement = unsafe { RuntimeValue::from_raw_in(instance, token) };
    // SAFETY: project only the checked slot in the retained allocation.
    let slot = unsafe { &(*instance).exec_token };
    let previous = slot.replace(replacement);
    drop(previous);
}

/// Retain the source call for condition construction without lending the slot.
///
/// # Safety
/// `instance` must remain live for the call, with no overlapping exclusive
/// borrow of the whole instance.
pub(crate) unsafe fn assignment_expr_in(
    instance: *mut RInstance,
) -> Option<super::object::Sexp<'static>> {
    // SAFETY: project only the checked slot in the retained allocation.
    let slot = unsafe { &(*instance).assignment_expr };
    slot.borrow().owned()
}

/// Swap the rooted source call. The displaced value leaves the slot before
/// it can be dropped, including during cleanup of a revoked session.
///
/// # Safety
/// `instance` must remain physically live for the call, with no overlapping
/// exclusive borrow of the whole instance. `replacement` must be empty or
/// a rooted value already validated for this instance.
pub(crate) unsafe fn replace_assignment_expr_in(
    instance: *mut RInstance,
    replacement: RuntimeValue,
) -> RuntimeValue {
    // SAFETY: project only the checked slot in the retained allocation.
    let slot = unsafe { &(*instance).assignment_expr };
    slot.replace(replacement)
}

/// Lend diagnostics for a local operation that cannot reenter the runtime.
///
/// # Safety
/// The caller retains the instance for the whole call, without any overlapping
/// exclusive whole-instance borrow. A returned raw buffer pointer is usable
/// only while the original instance remains allocated.
pub(crate) unsafe fn with_parse_diagnostics_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut ParseDiagnostics, &ParseErrorBuffer) -> R,
) -> R {
    // SAFETY: project only these two fields of the retained allocation.
    let (state, buffer) = unsafe {
        (
            &(*instance).parse_diagnostics,
            &(*instance).parse_error_buffer,
        )
    };
    f(&mut state.borrow_mut(), buffer)
}

/// Access the active diagnostics without exposing the session pointer.
pub(crate) fn with_parse_diagnostics<R>(
    f: impl FnOnce(&mut ParseDiagnostics, &ParseErrorBuffer) -> R,
) -> Option<R> {
    with_current_instance(|instance| {
        // SAFETY: the ambient accessor retains this allocation for the lend.
        unsafe { with_parse_diagnostics_in(instance, f) }
    })
}

/// Lend the finalizer queue for local bookkeeping or callback-free tracing.
///
/// # Safety
/// `instance` must point to a live allocation throughout the lend, without
/// an overlapping mutable whole-instance reference.
pub(crate) unsafe fn with_finalizer_state_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut crate::mainutils::memory_main::FinalizerState) -> R,
) -> R {
    // SAFETY: the caller retains the allocation; project only this field.
    let cell = unsafe { &(*instance).finalizer_state };
    f(&mut cell.borrow_mut())
}

/// Lend collector state for a local operation, not a GC cycle or callback.
///
/// # Safety
/// `instance` must point to a live allocation throughout the lend, without
/// an overlapping mutable whole-instance reference.
pub(crate) unsafe fn with_gc_state_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut super::gengc::GcState) -> R,
) -> R {
    // SAFETY: the caller retains the allocation; project only this field.
    let cell = unsafe { &(*instance).gc_state };
    f(&mut cell.borrow_mut())
}

/// Lend `instance.error_state` exclusively for a strictly-local update.
///
/// A nested lend of the same owner's error state (for example from a callback
/// that re-enters an error-state accessor) panics instead of creating a second
/// aliasing `&mut ErrorState`, mirroring the arena lend check.
/// # Safety
/// `instance` points to a live owner allocation for the duration of the call.
/// A mutable whole-instance reference must not overlap the lend.
pub(crate) unsafe fn with_error_state_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut ErrorState) -> R,
) -> R {
    // SAFETY: forwards the original owner and lend contract.
    unsafe { with_error_state_and_buffer_in(instance, |state, _| f(state)) }
}

/// Lend diagnostic state and the stable, separately stored error bytes.
///
/// # Safety
/// The owner and aliasing requirements of [`with_error_state_in`] apply.
pub(crate) unsafe fn with_error_state_and_buffer_in<R>(
    instance: *mut RInstance,
    f: impl FnOnce(&mut ErrorState, &ErrorBuffer) -> R,
) -> R {
    // SAFETY: the caller retains this allocation. Project only these
    // fields; the RefCell checks overlapping state lends, and the sibling
    // Cell does not invalidate previously returned raw buffer pointers.
    let (cell, buffer) = unsafe { (&(*instance).error_state, &(*instance).error_buffer) };
    let mut state = cell.try_borrow_mut().unwrap_or_else(|_| {
        panic!("reentrant error-state access; release the error state before re-entering it")
    });
    f(&mut state, buffer)
}

/// Recycled generation cells. A cell is only ever leaked once and then reused,
/// so the pool is bounded by the peak number of simultaneously live owners.
static GENERATION_CELLS: std::sync::Mutex<Vec<&'static std::sync::atomic::AtomicU64>> =
    std::sync::Mutex::new(Vec::new());
/// Generations are never reissued, so a recycled cell cannot revive a stale
/// witness (no ABA): each witness compares against its own unique value.
static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static DEAD_GENERATION_CELL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A `Copy` teardown witness for one `RInstance`.
///
/// Unlike [`InstanceLiveness`] (a `Weak`), this can be embedded in `Copy`
/// capabilities such as `OwnerToken`. It is a `'static` cell plus the unique
/// generation the owner stored in it; the owner overwrites the cell with `0`
/// before any of its fields are torn down. `is_live()` therefore implies the
/// original owner allocation has not started teardown (and so has not been
/// freed), and distinguishes a different owner later placed at the same
/// address. Generation `0` is never issued.
#[derive(Clone, Copy)]
pub(crate) struct InstanceGeneration {
    cell: &'static std::sync::atomic::AtomicU64,
    generation: u64,
}

impl InstanceGeneration {
    /// A witness that is never live.
    pub(crate) const DEAD: Self = Self {
        cell: &DEAD_GENERATION_CELL,
        generation: 0,
    };

    fn acquire() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        assert_ne!(generation, 0, "instance generation counter exhausted");
        let recycled = GENERATION_CELLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop();
        let cell = recycled.unwrap_or_else(|| Box::leak(Box::new(AtomicU64::new(0))));
        cell.store(generation, Ordering::Release);
        Self { cell, generation }
    }

    fn release(self) {
        use std::sync::atomic::Ordering;
        if self.generation == 0 {
            return;
        }
        self.cell.store(0, Ordering::Release);
        GENERATION_CELLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(self.cell);
    }

    pub(crate) fn is_live(self) -> bool {
        self.generation != 0
            && self.cell.load(std::sync::atomic::Ordering::Acquire) == self.generation
    }
}

// ---------------------------------------------------------------------------
// Thread-local current instance
// ---------------------------------------------------------------------------

/// Managed ambient dispatch carries only a weak capability. Standalone
/// translated fixtures remain an explicitly unsafe lifetime-bound boundary.
#[derive(Clone)]
pub(crate) struct BorrowedRuntime {
    pointer: *mut RInstance,
    liveness: InstanceLiveness,
    generation: InstanceGeneration,
    heap: super::heap::HeapIdentity,
}

impl BorrowedRuntime {
    pub(crate) fn snapshots(
        &self,
    ) -> (
        super::owner::BorrowedOwnerRef,
        InstanceLiveness,
        super::heap::HeapIdentity,
    ) {
        (
            super::owner::BorrowedOwnerRef::new(
                std::ptr::NonNull::new(self.pointer).expect("installed borrowed runtime"),
                self.generation,
            ),
            self.liveness.clone(),
            self.heap.clone(),
        )
    }
}

#[derive(Clone)]
enum CurrentRuntime {
    Managed(super::owner::WeakOwner),
    Borrowed(BorrowedRuntime),
}

impl CurrentRuntime {
    unsafe fn from_raw(pointer: *mut RInstance) -> Self {
        match unsafe { (*pointer).runtime_owner.clone() } {
            Some(owner) => Self::Managed(owner),
            None => Self::Borrowed(BorrowedRuntime {
                pointer,
                liveness: unsafe { instance_liveness(pointer) },
                generation: unsafe { instance_generation(pointer) },
                // SAFETY: installation requires a live standalone instance.
                // Snapshot the immutable heap capability before publishing it.
                heap: unsafe { (*pointer).heap_identity.clone() },
            }),
        }
    }

    fn projection(&self) -> Option<*mut RInstance> {
        match self {
            // This is an identity observation only: no callback or field
            // access occurs here. Dereferencing operations acquire a pin in
            // with_current_instance before releasing the TLS borrow.
            Self::Managed(owner) => owner.is_live().then(|| owner.identity_ptr()),
            Self::Borrowed(owner) => owner.liveness.is_live().then_some(owner.pointer),
        }
    }
}

thread_local! {
    /// Availability-gated weak capability, without keeping closed sessions alive.
    static CURRENT_INSTANCE: RefCell<Option<CurrentRuntime>> = const { RefCell::new(None) };

    /// Borrow depth counter for ambient `RInstance` views derived from the
    /// thread-local raw pointer. This remains a diagnostic monitor while the
    /// translated C-shaped entrypoints are being migrated to explicit
    /// `RInstance` parameters.
    pub(crate) static INSTANCE_MUT_BORROW_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub(crate) struct InstanceBorrowDepthGuard;

impl Drop for InstanceBorrowDepthGuard {
    fn drop(&mut self) {
        INSTANCE_MUT_BORROW_DEPTH.with(|c| {
            let depth = c.get();
            debug_assert!(depth > 0, "ambient RInstance borrow depth underflow");
            c.set(depth.saturating_sub(1));
        });
    }
}

pub(crate) fn enter_instance_borrow() -> InstanceBorrowDepthGuard {
    INSTANCE_MUT_BORROW_DEPTH.with(|c| c.set(c.get() + 1));
    InstanceBorrowDepthGuard
}

pub(crate) fn instance_borrow_depth() -> usize {
    INSTANCE_MUT_BORROW_DEPTH.with(Cell::get)
}

/// Acquire &mut RInstance view (via the current raw ptr) with depth tracking.
/// We do not panic on depth>0 (controlled safe-point+gc and similar patterns
/// legitimately nest acquires under an outer lend); the counter makes the
/// previously-silent aliasing visible and the quiescence GC policy + arena
/// guard keep the hazardous cases from arising.
fn acquire_instance_mut<F, R>(ptr: *mut RInstance, f: F) -> R
where
    F: FnOnce(*mut RInstance) -> R,
{
    let _borrow = enter_instance_borrow();
    // REDESIGNED (2026-09): the callback receives
    // the *mut RInstance itself; every field access through it is a raw
    // place access carrying no protector, so reentrant ambient writes
    // (protect push, GC bookkeeping) never pop a live reference. The
    // aliasing discipline is P1/P2 (see the module docs): no &mut/& of
    // the instance or a field may be held across an ambient-capable call
    // (allocation, protect, eval); Miri is the checker for that rule.
    f(ptr)
}

/// Set the current thread-local R instance for translated compatibility code.
///
/// # Safety
///
/// The caller must ensure that `instance` points to a valid, live `RInstance`
/// and that it is restored or cleared before the pointed-to instance is dropped.
pub unsafe fn set_current_instance(instance: *mut RInstance) {
    unsafe { replace_current_instance(Some(instance)) };
}

/// Replace the ambient capability and return its previous live projection.
///
/// # Safety
/// Standalone pointers must remain allocated while installed. Managed session
/// pointers install their original weak capability and reject revoked owners.
pub unsafe fn replace_current_instance(instance: Option<*mut RInstance>) -> Option<*mut RInstance> {
    let replacement = instance.map(|pointer| unsafe { CurrentRuntime::from_raw(pointer) });
    CURRENT_INSTANCE.with(|ci| {
        let mut current = ci.borrow_mut();
        let previous = current.as_ref().and_then(CurrentRuntime::projection);
        *current = replacement;
        previous
    })
}

/// Clear the current thread-local R instance.
///
/// Should be called when an `RSession` is closed or dropped.
pub fn clear_current_instance() {
    CURRENT_INSTANCE.with(|ci| {
        *ci.borrow_mut() = None;
    });
}

/// Clear the current thread-local R instance only if it matches `instance`.
///
/// Returns `true` when the pointer matched and the thread-local slot was
/// cleared. This prevents an older session from detaching a newer active
/// session that became current on the same thread.
pub fn clear_current_instance_if(instance: *const RInstance) -> bool {
    CURRENT_INSTANCE.with(|ci| {
        let mut current = ci.borrow_mut();
        if current
            .as_ref()
            .map(|runtime| match runtime {
                CurrentRuntime::Managed(owner) => owner.identity_ptr(),
                CurrentRuntime::Borrowed(owner) => owner.pointer,
            })
            .map(|ptr| std::ptr::eq(ptr as *const RInstance, instance))
            .unwrap_or(false)
        {
            *current = None;
            true
        } else {
            false
        }
    })
}

/// Return the current raw instance pointer, if one is active.
#[inline]
pub(crate) fn current_instance_ptr() -> Option<*mut RInstance> {
    CURRENT_INSTANCE.with(|ci| ci.borrow().as_ref().and_then(CurrentRuntime::projection))
}

/// Find an older singleton bank retained by this thread's live runtime.
pub(crate) fn current_retained_singleton(pointer: SEXP) -> Option<super::globals::SingletonLease> {
    let heap = CURRENT_INSTANCE.with(|ci| match ci.borrow().as_ref()? {
        CurrentRuntime::Managed(owner) => owner.live_heap_identity(),
        CurrentRuntime::Borrowed(owner) => owner.liveness.is_live().then(|| owner.heap.clone()),
    })?;
    heap.retained_singleton(pointer)
}

/// Retain a checked value in the installed domain while copying its contents.
/// The operation receives a handle scoped to the captured runtime snapshot.
/// No raw runtime field is read and no TLS or arena loan spans the operation.
pub(crate) fn with_current_value<T>(
    pointer: SEXP,
    operation: impl for<'value> FnOnce(&super::object::Sexp<'value>) -> T,
) -> Option<T> {
    let current = CURRENT_INSTANCE.with(|ci| ci.borrow().clone())?;
    match current {
        CurrentRuntime::Managed(owner) => {
            let _pin = owner.pin().ok()?;
            let value = owner.node_factory().ok()?.wrap(pointer).ok()?;
            Some(operation(&value))
        }
        CurrentRuntime::Borrowed(owner) => {
            let domain = super::object::NodeDomain::from_borrowed_runtime(&owner);
            let value = domain.wrap(pointer).ok()?;
            Some(operation(&value))
        }
    }
}

/// Whether a runtime capability is installed, even if its owner has been revoked.
/// Callers that support unscoped fixtures can distinguish a truly absent
/// runtime from a configured capability that failed its liveness check.
#[inline]
pub(crate) fn current_instance_is_configured() -> bool {
    CURRENT_INSTANCE.with(|ci| ci.borrow().is_some())
}

/// Return whether this thread currently has an active runtime instance.
///
/// Code that only needs a capability check should use this instead of
/// obtaining a raw instance pointer it will never dereference.
#[inline]
pub(crate) fn has_current_instance() -> bool {
    current_instance_ptr().is_some()
}

/// Execute a closure with a reference to the current instance, if active.
///
/// Returns `None` (and does not call `f`) if no instance is currently active.
#[inline]
pub fn with_current_instance<F, R>(f: F) -> Option<R>
where
    F: FnOnce(*mut RInstance) -> R,
{
    // Pin the selected allocation while the TLS capability is borrowed, then
    // end that borrow before arbitrary runtime code. The operation needs no
    // clone of unrelated heap/singleton capabilities. Its strong pin still
    // retains original storage through callback teardown and cleanup unwind.
    let (pointer, _pin) = CURRENT_INSTANCE.with(|ci| {
        let current = ci.borrow();
        match current.as_ref()? {
            CurrentRuntime::Managed(owner) => {
                let pin = owner.pin().ok()?;
                Some((pin.as_ptr(), Some(pin)))
            }
            CurrentRuntime::Borrowed(owner) => {
                owner.liveness.is_live().then_some((owner.pointer, None))
            }
        }
    })?;
    Some(acquire_instance_mut(pointer, f))
}

/// Execute a closure with the current instance.
///
/// Mutable interpreter state must be accessed through an active `RInstance`.
/// A missing instance indicates an unscoped runtime entrypoint that should be
/// routed through `RSession` before it reaches interpreter internals.
#[inline]
pub fn with_required_current_instance<F, R>(f: F) -> R
where
    F: FnOnce(*mut RInstance) -> R,
{
    with_current_instance(f).expect("mutable R runtime state requires an active RInstance")
}

/// Copy the active session's host-process capabilities, if any.
pub(crate) fn current_capabilities() -> Option<SessionCapabilities> {
    // SAFETY: the active allocation is retained and only the capability cell is read.
    with_current_instance(|instance| unsafe { &(*instance).capabilities }.get())
}

pub(crate) fn required_current_capabilities() -> SessionCapabilities {
    current_capabilities().expect("mutable R runtime state requires an active RInstance")
}

/// Access the active session's print handles for a local operation.
pub(crate) fn with_current_print_runtime<R>(
    f: impl FnOnce(&crate::mainutils::print::PrintRuntimeState) -> R,
) -> R {
    with_required_current_instance(|instance| {
        // SAFETY: the active allocation is retained; only the print handles are lent.
        f(unsafe { &(*instance).print_runtime })
    })
}

/// Read or update the active session's scalar REPL flags without lending evaluator state.
#[inline]
pub(crate) fn with_repl_flags<R>(f: impl FnOnce(&ReplFlags) -> R) -> Option<R> {
    // SAFETY: the active allocation is retained and this projection touches only Cells.
    with_current_instance(|instance| f(unsafe { &(*instance).repl_flags }))
}

#[inline]
pub(crate) fn with_required_repl_flags<R>(f: impl FnOnce(&ReplFlags) -> R) -> R {
    with_repl_flags(f).expect("mutable R runtime state requires an active RInstance")
}

/// Access the active session's trace and debug cells without lending evaluator state.
pub(crate) fn with_trace_debug_flags<R>(f: impl FnOnce(&TraceDebugFlags) -> R) -> Option<R> {
    // SAFETY: the active allocation is retained; this projects only the two cells.
    with_current_instance(|instance| f(unsafe { &(*instance).trace_debug_flags }))
}

/// Return whether the active session has requested cooperative cancellation.
#[inline]
pub fn is_cancellation_requested() -> bool {
    with_current_instance(|inst| unsafe {
        with_execution_state_in(inst, |state| {
            state.cancellation.as_ref().is_some_and(|token| token.is_requested())
        })
    })
    .unwrap_or(false)
}

/// Raise an R error if the active session has requested cancellation.
#[inline]
pub fn check_cancellation() {
    if is_cancellation_requested() {
        std::panic::panic_any(crate::sexp::context::RSignal::Error {
            message: "operation cancelled".to_string(),
        });
    }
}

// ---------------------------------------------------------------------------
// Checked ambient field lends
// ---------------------------------------------------------------------------
//
// The helpers below encapsulate the raw `(*instance).field` derefs that
// binding/environment bookkeeping performs through the ambient capability.
// Each lends a disjoint field view for one strictly-local operation under the
// P1/P2 discipline documented at the module head: the callback must not
// allocate, protect, collect, or eval while the lend is live.

/// Borrowed view of an instance's binding-visibility tables
/// (`locked_environments`, `locked_bindings`, `active_bindings`).
pub(crate) struct BindingTables<'i> {
    pub(crate) locked_environments: &'i mut HashSet<usize>,
    pub(crate) locked_bindings: &'i mut HashSet<(usize, usize)>,
    pub(crate) active_bindings: &'i mut HashMap<(usize, usize), SEXP>,
}

/// Execute `f` with the current instance's binding tables, if an instance is
/// active.
///
/// `f` must perform only strictly-local reads/writes on the lent tables; no
/// ambient-capable call may run while the lend is live (P1/P2).
pub(crate) fn with_binding_tables<R>(f: impl FnOnce(BindingTables<'_>) -> R) -> Option<R> {
    with_current_instance(|instance| {
        // SAFETY: disjoint strictly-local field lends under P1/P2; the
        // closure contract above keeps the lends short and non-reentrant.
        unsafe {
            f(BindingTables {
                locked_environments: &mut (*instance).locked_environments,
                locked_bindings: &mut (*instance).locked_bindings,
                active_bindings: &mut (*instance).active_bindings,
            })
        }
    })
}

/// Execute `f` with the current instance's binding tables.
///
/// Panics when no runtime instance is active, mirroring
/// [`with_required_current_instance`].
pub(crate) fn with_required_binding_tables<R>(f: impl FnOnce(BindingTables<'_>) -> R) -> R {
    with_binding_tables(f).expect("mutable R binding state requires an active RInstance")
}

/// Clone the active instance's heap identity, if an instance is active.
///
/// Callers use the snapshot for canonical node lookups without touching the
/// raw instance pointer again.
pub(crate) fn current_heap_identity() -> Option<super::heap::HeapIdentity> {
    with_current_instance(|instance| {
        // SAFETY: P2 strictly-local single-field clone; no ambient write
        // intervenes inside the closure.
        unsafe { (*instance).heap_identity.clone() }
    })
}

/// Execute `f` with the current instance's hash-environment size table.
///
/// Same strictly-local lend contract as [`with_binding_tables`].
pub(crate) fn with_required_env_hash_sizes<R>(
    f: impl FnOnce(&mut hashbrown::HashMap<usize, i32>) -> R,
) -> R {
    with_required_current_instance(|instance| {
        // SAFETY: P2 strictly-local field lend; `f` performs no ambient call.
        unsafe { f(&mut (*instance).env_hash_sizes) }
    })
}

/// Execute `f` with the current instance's persistent-node heap.
///
/// Same strictly-local lend contract as [`with_binding_tables`]; `f` may
/// allocate persistent nodes (which is strictly local to this field) but must
/// not run interpreter callbacks.
pub(crate) fn with_required_persistent_heap<R>(f: impl FnOnce(&mut PersistentHeap) -> R) -> R {
    with_required_current_instance(|instance| {
        // SAFETY: P2 strictly-local field lend; persistent allocation touches
        // no other instance field and runs no ambient callback.
        unsafe { f(&mut (*instance).persistent_nodes) }
    })
}

/// Execute `f` with the current instance's in-error flag.
///
/// Same strictly-local lend contract as [`with_binding_tables`].
pub(crate) fn with_required_in_error<R>(f: impl FnOnce(&mut bool) -> R) -> R {
    with_required_current_instance(|instance| {
        // SAFETY: P2 strictly-local single-field lend; `f` performs no ambient
        // call.
        unsafe { f(&mut (*instance).in_error) }
    })
}

/// Pin the ambient instance's runtime owner, if it has one.
///
/// Used when suspending or restoring ambient dispatch: the pin keeps the
/// previous owner physically live across bootstrap callbacks.
pub(crate) fn current_runtime_pin() -> Option<super::owner::OwnerPin> {
    current_instance_ptr().and_then(|pointer| {
        // SAFETY: strictly-local `runtime_owner` read under P2; the ambient
        // pointer names a live instance while it is installed as current.
        unsafe {
            (*pointer)
                .runtime_owner
                .as_ref()
                .and_then(|owner| owner.pin().ok())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_owns_environment_sentinels() {
        let instance = RInstance::new();

        assert_eq!(instance.env_nodes.len(), 3);
        assert!(!instance.empty_env.is_null());
        assert!(!instance.base_env.is_null());
        assert!(!instance.global_env.is_null());

        unsafe {
            assert_eq!((*instance.empty_env).sxpinfo.type_of(), SEXPTYPE::ENVSXP);
            assert_eq!((*instance.base_env).sxpinfo.type_of(), SEXPTYPE::ENVSXP);
            assert_eq!((*instance.global_env).sxpinfo.type_of(), SEXPTYPE::ENVSXP);
            assert_eq!(
                crate::sexp::accessors::ENCLOS(instance.base_env),
                instance.empty_env
            );
            assert_eq!(
                crate::sexp::accessors::ENCLOS(instance.global_env),
                instance.base_env
            );
        }
    }

    #[test]
    fn test_ambient_instance_borrow_depth_resets_after_panic() {
        let mut instance = RInstance::new();
        let previous = unsafe { replace_current_instance(Some(&mut instance as *mut _)) };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_current_instance(|_| panic!("intentional ambient borrow panic"));
        }));

        assert!(result.is_err());
        assert_eq!(instance_borrow_depth(), 0);
        unsafe {
            replace_current_instance(previous);
        }
    }
}

#[cfg(test)]
mod owned_evaluator_root_tests {
    use super::*;
    use crate::sexp::session::RSession;

    fn profile_root(state: &mut ProfilingState, index: usize) -> &mut RuntimeValue {
        match index {
            3 => &mut state.sref,
            4 => &mut state.srcfiles_buffer,
            5 => &mut state.in_bc_interpreter,
            6 => &mut state.bcbody,
            _ => unreachable!(),
        }
    }

    fn replace_profile_root(
        state: &mut ProfilingState,
        index: usize,
        value: RuntimeValue,
    ) -> RuntimeValue {
        std::mem::replace(profile_root(state, index), value)
    }

    #[test]
    fn owned_evaluator_fields_are_sole_roots_and_release_replaced_values() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let replace = |index: usize, pointer: SEXP| match index {
                0 => {
                    let value = RuntimeValue::from_raw_in(instance, pointer);
                    drop(replace_assignment_expr_in(instance, value));
                }
                1 => {
                    let value = RuntimeValue::from_raw_in(instance, pointer);
                    drop(with_parse_diagnostics_in(instance, |state, _| {
                        std::mem::replace(&mut state.file, value)
                    }));
                }
                2 => set_exec_token_in(instance, pointer),
                _ => {
                    let value = RuntimeValue::from_raw_in(instance, pointer);
                    drop(with_profiling_state_in(instance, |state| {
                        replace_profile_root(state, index, value)
                    }));
                }
            };
            for index in 0..7 {
                let value = owner
                    .node_factory()
                    .allocate(|arena| {
                        Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1))
                    })
                    .unwrap();
                let pointer = value.as_raw();
                let original = crate::sexp::memory::checked_projection(pointer).unwrap().1;
                replace(index, pointer);
                drop(value);
                owner.full_gc().unwrap();
                assert!(
                    original.is_live(),
                    "field {index} must own its original value"
                );
                let replacement = owner
                    .node_factory()
                    .allocate(|arena| {
                        Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::RAWSXP, 7))
                    })
                    .unwrap();
                let pointer = replacement.as_raw();
                let next = crate::sexp::memory::checked_projection(pointer).unwrap().1;
                replace(index, pointer);
                drop(replacement);
                owner.full_gc().unwrap();
                assert!(
                    !original.is_live(),
                    "field {index} must release its replaced value"
                );
                assert!(next.is_live());
                replace(index, std::ptr::null_mut());
                owner.full_gc().unwrap();
                assert!(
                    !next.is_live(),
                    "cleared field {index} must release its value"
                );
            }
        });
    }

    #[test]
    fn owned_evaluator_fields_reject_foreign_replacement_without_losing_root() {
        let session = RSession::new_for_gc_tests();
        let foreign = RSession::new_for_gc_tests();
        let foreign_value = foreign.with_active(|| {
            foreign
                .owner_token()
                .unwrap()
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1)))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let replace = |index: usize, pointer: SEXP| match index {
                0 => {
                    let value = RuntimeValue::from_raw_in(instance, pointer);
                    drop(replace_assignment_expr_in(instance, value));
                }
                1 => {
                    let value = RuntimeValue::from_raw_in(instance, pointer);
                    drop(with_parse_diagnostics_in(instance, |state, _| {
                        std::mem::replace(&mut state.file, value)
                    }));
                }
                2 => set_exec_token_in(instance, pointer),
                _ => {
                    let value = RuntimeValue::from_raw_in(instance, pointer);
                    drop(with_profiling_state_in(instance, |state| {
                        replace_profile_root(state, index, value)
                    }));
                }
            };
            let read = |index: usize| match index {
                0 => assignment_expr_in(instance)
                    .map_or(std::ptr::null_mut(), |value| value.as_raw()),
                1 => with_parse_diagnostics_in(instance, |state, _| state.file.as_raw()),
                2 => exec_token_in(instance)
                    .map_or(std::ptr::null_mut(), |value| value.as_raw()),
                _ => {
                    with_profiling_state_in(instance, |state| profile_root(state, index).as_raw())
                }
            };
            for index in 0..7 {

                let value = owner
                    .node_factory()
                    .allocate(|arena| {
                        Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1))
                    })
                    .unwrap();
                let pointer = value.as_raw();
                let original = crate::sexp::memory::checked_projection(pointer).unwrap().1;
                replace(index, pointer);
                drop(value);
                let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    replace(index, foreign_value.as_raw());
                }));
                assert!(rejected.is_err());
                assert_eq!(read(index), pointer);
                owner.full_gc().unwrap();
                assert!(original.is_live());
                replace(index, std::ptr::null_mut());
                owner.full_gc().unwrap();
                assert!(!original.is_live());
            }
        });
    }
}

#[cfg(test)]
mod owned_error_root_tests {
    use super::*;
    use crate::sexp::session::RSession;

    fn slot(state: &mut ErrorState, index: usize) -> &mut RuntimeValue {
        match index {
            0 => &mut state.warnings,
            1 => &mut state.handler_stack,
            2 => &mut state.global_calling_handlers,
            3 => &mut state.restart_stack,
            4 => &mut state.warning_call,
            5 => &mut state.signalled_condition,
            6 => &mut state.mathlib_warning_call,
            7 => &mut state.last_error_call,
            _ => unreachable!(),
        }
    }

    #[test]
    fn owned_error_fields_preserve_original_roots_and_release_replacements() {
        let session = RSession::new_for_gc_tests();
        let foreign = RSession::new_for_gc_tests();
        let foreign_value = foreign.with_active(|| {
            foreign
                .owner_token()
                .unwrap()
                .node_factory()
                .character("foreign")
                .unwrap()
                .into_owned()
                .unwrap()
        });
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let factory = owner.node_factory();
            for index in 0..8 {
                let value = factory.character("sole field root").unwrap();
                let pointer = value.as_raw();
                let original = crate::sexp::memory::checked_projection(pointer).unwrap().1;
                let owned = RuntimeValue::from_raw_in(instance, pointer);
                drop(with_error_state_in(instance, |state| {
                    std::mem::replace(slot(state, index), owned)
                }));
                drop(value);
                owner.full_gc().unwrap();
                assert!(original.is_live());
                let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let owned = RuntimeValue::from_raw_in(instance, foreign_value.as_raw());
                    drop(with_error_state_in(instance, |state| {
                        std::mem::replace(slot(state, index), owned)
                    }));
                }));
                assert!(rejected.is_err());
                assert_eq!(
                    with_error_state_in(instance, |state| slot(state, index).as_raw()),
                    pointer
                );
                let replacement = factory.character("replacement").unwrap();
                let next = crate::sexp::memory::checked_projection(replacement.as_raw())
                    .unwrap()
                    .1;
                let owned = RuntimeValue::from_raw_in(instance, replacement.as_raw());
                drop(with_error_state_in(instance, |state| {
                    std::mem::replace(slot(state, index), owned)
                }));
                drop(replacement);
                owner.full_gc().unwrap();
                assert!(!original.is_live());
                assert!(next.is_live());
                drop(with_error_state_in(instance, |state| {
                    std::mem::take(slot(state, index))
                }));
                owner.full_gc().unwrap();
                assert!(!next.is_live());
            }
        });
    }
}
