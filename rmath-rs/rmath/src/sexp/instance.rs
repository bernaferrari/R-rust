#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R instance isolation — per-instance state for concurrent R sessions.
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

use std::alloc::{Layout, dealloc};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::os::raw::{c_char, c_int};
use std::time::Instant;

use super::ffi::{SEXP, SEXPTYPE, SexprecCore};
use super::memory::RArena;

// ---------------------------------------------------------------------------
// RInstance
// ---------------------------------------------------------------------------

pub(crate) struct ErrorState {
    pub warn_length: c_int,
    pub show_error_messages: bool,
    pub show_error_calls: bool,
    pub show_warn_calls: bool,
    pub in_error: c_int,
    pub in_warning: c_int,
    pub in_print_warnings: c_int,
    pub immediate_warning: bool,
    pub no_break_warning: bool,
    pub interrupts_suspended: bool,
    pub interrupts_pending: bool,
    pub collect_warnings: c_int,
    pub nwarnings: c_int,
    pub warnings: SEXP,
    pub handler_stack: SEXP,
    pub restart_stack: SEXP,
    pub error_buffer: [u8; crate::mainutils::errors::BUFSIZE + 1],
    pub expressions: c_int,
    pub expressions_keep: c_int,
    /// Message of the most recent error rendered into `error_buffer` by
    /// `verrorcall_dflt`, if any. The top-level renderer only trusts the
    /// error buffer when the escaping error's message matches this, so stale
    /// renders from previously caught errors are ignored (mirrors upstream,
    /// where caught errors never reach `verrorcall_dflt` printing).
    pub last_rendered_message: Option<String>,
}

impl Default for ErrorState {
    fn default() -> Self {
        ErrorState {
            warn_length: 1000,
            show_error_messages: true,
            show_error_calls: false,
            show_warn_calls: false,
            in_error: 0,
            in_warning: 0,
            in_print_warnings: 0,
            immediate_warning: false,
            no_break_warning: false,
            interrupts_suspended: false,
            interrupts_pending: false,
            collect_warnings: 0,
            nwarnings: 50,
            warnings: std::ptr::null_mut(),
            handler_stack: std::ptr::null_mut(),
            restart_stack: std::ptr::null_mut(),
            error_buffer: [0; crate::mainutils::errors::BUFSIZE + 1],
            expressions: 500,
            expressions_keep: 500,
            last_rendered_message: None,
        }
    }
}

pub(crate) const PROFILING_OPCODE_COUNT: usize = 256;
pub(crate) const NO_PROFILING_OPCODE: c_int = -1;

pub(crate) struct ProfilingState {
    pub sref: SEXP,
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
    pub profile_outfile: c_int,
    pub memory_peak_bytes: usize,
    pub srcfiles: *mut *mut c_char,
    pub srcfile_bufcount: usize,
    pub srcfiles_buffer: SEXP,
}

impl Default for ProfilingState {
    fn default() -> Self {
        ProfilingState {
            sref: std::ptr::null_mut(),
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
            profile_outfile: -1,
            memory_peak_bytes: 0,
            srcfiles: std::ptr::null_mut(),
            srcfile_bufcount: 0,
            srcfiles_buffer: std::ptr::null_mut(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionCapabilities {
    pub allow_system_commands: bool,
    pub allow_pipe_commands: bool,
}

pub(crate) struct EvalControlState {
    pub capabilities: SessionCapabilities,
    pub no_echo: c_int,
    pub quiet: c_int,
    pub interactive: c_int,
    pub verbose: c_int,
    pub current_expr: SEXP,
    pub visible: c_int,
    pub eval_depth: c_int,
    pub eval_depth_limit: c_int,
    pub pp_stack_top: c_int,
    pub collect_warnings: c_int,
    pub parse_error_msg: [u8; 256],
    pub parse_error: c_int,
    pub parse_error_col: c_int,
    pub parse_error_file: SEXP,
    pub parse_context_line: c_int,
    pub parse_context: Vec<String>,
    pub relop_lang_option: c_int,
    pub tracing_state: c_int,
    pub debugging_state: c_int,
    pub format_print: crate::mainutils::format::RPrint,
    pub printutils: crate::mainutils::printutils::PrintUtilsState,
    pub printvector: crate::mainutils::printvector::R_PrintData,
    pub print: crate::mainutils::print::PrintRuntimeState,
    pub deparse: crate::mainutils::deparse::DeparseRuntimeState,
    pub radixsort: crate::mainutils::radixsort::RadixSortState,
    pub limits: crate::eval::eval::EvalLimits,
    pub start_time: Option<Instant>,
    pub bc_stack: crate::eval::bc_stack::R_bcstack_t,
    pub bc_int_active: c_int,
    pub min_jit_score: c_int,
    pub loop_jit_score: c_int,
    pub jit_enabled: c_int,
    pub compile_pkgs: c_int,
    pub disable_bytecode: c_int,
    pub check_constants: c_int,
    pub exec_token: SEXP,
    pub profiling: ProfilingState,
    pub cancellation: Option<crate::sexp::session::CancellationToken>,
}

impl Default for EvalControlState {
    fn default() -> Self {
        EvalControlState {
            capabilities: SessionCapabilities::default(),
            no_echo: 0,
            quiet: 0,
            interactive: 1,
            verbose: 0,
            current_expr: std::ptr::null_mut(),
            visible: 1,
            eval_depth: 0,
            eval_depth_limit: 500,
            pp_stack_top: 0,
            collect_warnings: 0,
            parse_error_msg: [0; 256],
            parse_error: 0,
            parse_error_col: 0,
            parse_error_file: std::ptr::null_mut(),
            parse_context_line: 0,
            parse_context: Vec::new(),
            relop_lang_option: 0,
            tracing_state: crate::sexp::ffi::TRUE,
            debugging_state: crate::sexp::ffi::TRUE,
            format_print: crate::mainutils::format::RPrint::default(),
            printutils: crate::mainutils::printutils::PrintUtilsState::default(),
            printvector: crate::mainutils::printvector::R_PrintData::default(),
            print: crate::mainutils::print::PrintRuntimeState::default(),
            deparse: crate::mainutils::deparse::DeparseRuntimeState::default(),
            radixsort: crate::mainutils::radixsort::RadixSortState::default(),
            limits: crate::eval::eval::EvalLimits::default(),
            start_time: None,
            bc_stack: crate::eval::bc_stack::R_bcstack_t::new(256),
            bc_int_active: 0,
            min_jit_score: 50,
            loop_jit_score: 50,
            jit_enabled: 0,
            compile_pkgs: 0,
            disable_bytecode: 0,
            check_constants: 0,
            exec_token: std::ptr::null_mut(),
            profiling: ProfilingState::default(),
            cancellation: None,
        }
    }
}

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
    /// Arena allocator for this instance.
    pub arena: RArena,
    /// The global environment for this instance.
    pub global_env: SEXP,
    /// The base environment for this instance.
    pub base_env: SEXP,
    /// The empty environment for this instance.
    pub empty_env: SEXP,
    /// Owned environment sentinel nodes for empty/base/global environments.
    #[allow(clippy::vec_box)]
    pub(crate) env_nodes: Vec<Box<SexprecCore>>,
    /// Whether this instance has completed R-level initialization.
    pub(crate) initialized: bool,
    /// The protection stack for this instance.
    pub(crate) protect_stack: RefCell<Vec<SEXP>>,
    /// The permanent preserve stack for this instance.
    pub(crate) preserve_stack: RefCell<Vec<SEXP>>,
    /// Per-instance execution context stack.
    #[allow(clippy::vec_box)]
    pub(crate) context_stack: Vec<Box<super::context::RCNTXT>>,
    /// Per-instance in-error flag.
    pub(crate) in_error: bool,
    /// Per-instance generational GC state.
    pub(crate) gc_state: super::gengc::GcState,
    /// Per-instance error, warning, interrupt, and expression-limit state.
    pub(crate) error_state: ErrorState,
    /// Per-instance main memory/GC control state.
    pub(crate) memory_state: crate::mainutils::memory_main::MemoryRuntimeState,
    /// Per-instance evaluator and REPL control state.
    pub(crate) eval_state: EvalControlState,
    /// Per-instance names.c runtime caches and initialization marker.
    pub(crate) names_state: crate::mainutils::names::NamesRuntimeState,
    /// Per-instance bind.c cached sentinels.
    pub(crate) bind_state: crate::mainutils::bind::BindRuntimeState,
    /// Per-instance S3/S4 object dispatch state.
    pub(crate) objects_state: crate::mainutils::objects::ObjectsRuntimeState,
    /// Per-instance dotcode/native-call runtime policy cache.
    pub(crate) dotcode_state: crate::mainutils::dotcode::DotcodeRuntimeState,
    /// Per-instance ALTREP class-method registry.
    pub(crate) altrep_state: crate::mainutils::altrep::AltrepRuntimeState,
    /// Per-instance serialization lazy-load cache and read-depth state.
    pub(crate) serialize_state: crate::mainutils::serialize::SerializeRuntimeState,
    /// Per-instance LAPACK module dispatcher.
    pub(crate) lapack_state: crate::mainutils::lapack::LapackRuntimeState,
    /// Per-instance internet module state.
    pub(crate) internet_state: crate::modules::internet::internet::InternetRuntimeState,
    /// Per-instance libcurl module scratch/progress state.
    pub(crate) libcurl_state: crate::modules::internet::libcurl::LibcurlRuntimeState,
    /// Per-instance embedded HTTP server socket, worker, and handler state.
    pub(crate) httpd_state: crate::modules::internet::rhttpd::HttpdRuntimeState,
    /// Per-instance X11 graphics defaults and device counters.
    #[cfg(not(target_os = "android"))]
    pub(crate) x11_state: crate::modules::x11::dev_x11::X11RuntimeState,
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
    #[cfg(not(target_os = "android"))]
    pub(crate) intl_state: crate::intl::types::IntlRuntimeState,
    /// Per-instance GraphApp GUI compatibility runtime state.
    #[cfg(not(target_os = "android"))]
    pub(crate) graphapp_state: crate::graphapp::runtime::GraphAppRuntimeState,
    /// Per-instance timezone cache for the root tzone module.
    pub(crate) tzone_state: crate::tzone::TzRuntimeState,
    /// Per-instance appl::lbfgsb solver continuation state.
    pub(crate) lbfgsb_state: crate::appl::lbfgsb::LbfgsbState,
    /// Per-instance symbol table for session-local interning.
    pub(crate) symbols: HashMap<String, SEXP>,
    /// Owned SYMSXP nodes for the per-instance symbol table.
    #[allow(clippy::vec_box)]
    pub(crate) symbol_nodes: Vec<Box<SexprecCore>>,
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
    /// Per-instance options storage (mirrors the global OPTIONS_TABLE).
    pub options: HashMap<String, SEXP>,
    /// Whether the instance options have been initialized with defaults.
    pub options_initialized: bool,
    /// Per-instance environment hash side tables.
    pub(crate) env_hash_tables: hashbrown::HashMap<usize, hashbrown::HashMap<usize, SEXP>>,
    /// Per-instance locked environments keyed by raw environment address.
    pub(crate) locked_environments: HashSet<usize>,
    /// Per-instance locked bindings keyed by (environment address, symbol address).
    pub(crate) locked_bindings: HashSet<(usize, usize)>,
    /// Per-instance active bindings keyed by (environment address, symbol address).
    pub(crate) active_bindings: HashMap<(usize, usize), SEXP>,
    /// Per-session nmath math state (sampler caches and rank memo tables).
    pub(crate) math_state: rmath_nmath::MathState,
    /// Per-instance stats::loess workspace buffers.
    pub(crate) loess_workspace_state: crate::library::stats::loessc::LoessWorkspaceState,
    /// Per-instance stats::bspline recurrence continuation state.
    pub(crate) bspline_state: crate::library::stats::bspline::BsplineState,
    /// Per-instance stats::fexact traversal continuation state.
    pub(crate) fexact_state: crate::library::stats::fexact::FexactState,
    /// Per-instance stats::fft factorization plan state.
    pub(crate) fft_state: crate::library::stats::fft::FftState,
    /// Per-instance dynamic loader and native package registry state.
    pub(crate) dynload_state: crate::mainutils::rdynload::DynloadState,
    /// Per-instance connection table and sink state.
    pub(crate) connections_state: crate::mainutils::connections::ConnectionsState,
    /// Per-instance library, cache, and temporary-directory policy.
    pub(crate) path_policy: crate::mainutils::paths::RuntimePathPolicy,
    /// Per-instance counter for unique `tempfile()` names.
    pub(crate) tempfile_counter: u64,
    /// Per-instance file creation mask used by `Sys.umask()`.
    pub(crate) file_creation_umask: u32,
    /// Per-instance cache of pure-R package namespaces keyed by package name.
    pub(crate) package_namespace_cache: HashMap<String, (std::path::PathBuf, SEXP)>,
    /// Per-instance headless graphics device registry.
    pub(crate) graphics_device_registry: crate::library::grdevices::device_registry::DeviceRegistry,
    /// Per-instance graphics engine registration state.
    pub(crate) graphics_engine_state: crate::mainutils::engine::GraphicsEngineState,
    /// Optional current DrawTarget (RenderPlot) backend (when the "renderplot-device" feature is enabled).
    /// Used by the DeviceRegistry drawing fns to forward high-quality drawing (skia etc.)
    /// for real R graphics calls on Android/WASM hosts.
    #[cfg(feature = "renderplot-device")]
    pub(crate) current_renderplot_backend: Option<*mut dyn r_graphics_engine::DrawTarget>,
    /// Per-instance base graphics `par()` overrides.
    pub(crate) graphics_par_state: crate::library::graphics::par::GraphicsParState,
    /// Per-instance grDevices color palette and scratch buffer state.
    pub(crate) graphics_color_state: crate::library::grdevices::colors::GraphicsColorState,
    /// Per-instance main/colors.c dispatch pointers installed by grDevices.
    pub(crate) color_dispatch_state: crate::mainutils::colors::ColorDispatchState,
    /// Per-instance grDevices PostScript/PDF font registry state.
    pub(crate) postscript_font_state: crate::library::grdevices::devps::PostScriptFontState,
    /// Per-instance grDevices Windows backend scratch state.
    #[cfg(not(target_os = "android"))]
    pub(crate) windows_device_state: crate::library::grdevices::devwindows::WindowsDeviceState,
    /// Per-instance grid runtime state.
    pub(crate) grid_runtime_state: crate::library::grid::types::GridRuntimeState,
    /// Per-instance graphics::plot3d scratch transformation state.
    pub(crate) plot3d_state: crate::library::graphics::plot3d::Plot3dState,
    /// Per-instance graphics dendrogram scratch state.
    pub(crate) dendrogram_state: crate::library::graphics::plot::DendrogramState,
    /// Per-instance methods package dispatch flags and cache counters.
    pub(crate) methods_dispatch_state:
        crate::library::methods::methods_list_dispatch::MethodsDispatchState,
    /// Per-instance parallel fork child/process bookkeeping.
    #[cfg(all(unix, not(target_os = "android")))]
    pub(crate) parallel_fork_state: crate::library::parallel::fork::ForkRuntimeState,
    /// Per-instance raw cons cells allocated outside the arena.
    pub(crate) raw_cons: Vec<*mut SexprecCore>,
    /// Per-instance transient allocations for R_alloc/vmaxget/vmaxset.
    pub(crate) vmax: Vec<(*mut u8, Layout)>,
}

impl RInstance {
    /// Create a new, fully independent R instance.
    ///
    /// This allocates three persistent environment sentinels (empty → base →
    /// global) owned by the instance, plus an empty arena and protect stack.
    pub fn new() -> Self {
        let nil = unsafe { super::globals::R_NilValue() };

        // Box heap addresses remain stable when the Vec reallocates, so raw
        // SEXP pointers can form the environment chain safely.
        let mut env_nodes = Vec::with_capacity(3);
        let empty_env = Self::push_env(&mut env_nodes, nil, nil, nil);
        let base_env = Self::push_env(&mut env_nodes, nil, empty_env, nil);
        let global_env = Self::push_env(&mut env_nodes, nil, base_env, nil);

        let mut instance = RInstance {
            arena: RArena::new(),
            global_env,
            base_env,
            empty_env,
            env_nodes,
            initialized: false,
            protect_stack: RefCell::new(Vec::new()),
            preserve_stack: RefCell::new(Vec::new()),
            context_stack: Vec::new(),
            in_error: false,
            gc_state: super::gengc::GcState::default(),
            error_state: ErrorState::default(),
            memory_state: crate::mainutils::memory_main::MemoryRuntimeState::default(),
            eval_state: EvalControlState::default(),
            names_state: crate::mainutils::names::NamesRuntimeState::default(),
            bind_state: crate::mainutils::bind::BindRuntimeState::default(),
            objects_state: crate::mainutils::objects::ObjectsRuntimeState::default(),
            dotcode_state: crate::mainutils::dotcode::DotcodeRuntimeState::default(),
            altrep_state: crate::mainutils::altrep::AltrepRuntimeState::default(),
            serialize_state: crate::mainutils::serialize::SerializeRuntimeState::default(),
            lapack_state: crate::mainutils::lapack::LapackRuntimeState::default(),
            internet_state: crate::modules::internet::internet::InternetRuntimeState::default(),
            libcurl_state: crate::modules::internet::libcurl::LibcurlRuntimeState::default(),
            httpd_state: crate::modules::internet::rhttpd::HttpdRuntimeState::default(),
            #[cfg(not(target_os = "android"))]
            x11_state: crate::modules::x11::dev_x11::X11RuntimeState::default(),
            sys_std_state: crate::unix::sys_std::SysStdRuntimeState::default(),
            sys_unix_state: crate::unix::sys_unix::SysUnixRuntimeState::default(),
            unix_system_state: crate::unix::system::UnixSystemRuntimeState::default(),
            startup_state: crate::mainutils::startup::StartupRuntimeState::default(),
            main_state: crate::mainutils::main::MainRuntimeState::default(),
            #[cfg(not(target_os = "android"))]
            intl_state: crate::intl::types::IntlRuntimeState::default(),
            #[cfg(not(target_os = "android"))]
            graphapp_state: crate::graphapp::runtime::GraphAppRuntimeState::default(),
            tzone_state: crate::tzone::TzRuntimeState::default(),
            lbfgsb_state: crate::appl::lbfgsb::LbfgsbState::default(),
            symbols: HashMap::new(),
            symbol_nodes: Vec::new(),
            rng_state: rmath_nmath::RngState::default(),
            rng_kind: 0,
            main_rng_state: crate::mainutils::rng::MainRngState::default(),
            random_state: crate::mainutils::random::RNGState::new(),
            output_capture: RefCell::new(super::output::OutputCaptureState::default()),
            options: HashMap::new(),
            options_initialized: false,
            env_hash_tables: hashbrown::HashMap::new(),
            locked_environments: HashSet::new(),
            locked_bindings: HashSet::new(),
            active_bindings: HashMap::new(),
            math_state: rmath_nmath::MathState::default(),
            loess_workspace_state: crate::library::stats::loessc::LoessWorkspaceState::default(),
            bspline_state: crate::library::stats::bspline::BsplineState::default(),
            fexact_state: crate::library::stats::fexact::FexactState::default(),
            fft_state: crate::library::stats::fft::FftState::default(),
            dynload_state: crate::mainutils::rdynload::DynloadState::default(),
            connections_state: crate::mainutils::connections::ConnectionsState::default(),
            path_policy: crate::mainutils::paths::RuntimePathPolicy::default(),
            tempfile_counter: 0,
            file_creation_umask: 0o022,
            package_namespace_cache: HashMap::new(),
            graphics_device_registry:
                crate::library::grdevices::device_registry::DeviceRegistry::default(),
            graphics_engine_state: crate::mainutils::engine::GraphicsEngineState::default(),
            graphics_par_state: crate::library::graphics::par::GraphicsParState::default(),
            #[cfg(feature = "renderplot-device")]
            current_renderplot_backend: None,
            graphics_color_state: crate::library::grdevices::colors::GraphicsColorState::default(),
            color_dispatch_state: crate::mainutils::colors::ColorDispatchState::default(),
            postscript_font_state: crate::library::grdevices::devps::PostScriptFontState::default(),
            #[cfg(not(target_os = "android"))]
            windows_device_state:
                crate::library::grdevices::devwindows::WindowsDeviceState::default(),
            grid_runtime_state: crate::library::grid::types::GridRuntimeState::default(),
            plot3d_state: crate::library::graphics::plot3d::Plot3dState::default(),
            dendrogram_state: crate::library::graphics::plot::DendrogramState::default(),
            methods_dispatch_state:
                crate::library::methods::methods_list_dispatch::MethodsDispatchState::default(),
            #[cfg(all(unix, not(target_os = "android")))]
            parallel_fork_state: crate::library::parallel::fork::ForkRuntimeState::default(),
            raw_cons: Vec::new(),
            vmax: Vec::new(),
        };

        instance.initialize_base_bindings();
        instance.initialized = true;
        instance
    }

    /// Install core base bindings with this instance active.
    pub fn initialize_base_bindings(&mut self) {
        let previous = unsafe { replace_current_instance(Some(self as *mut RInstance)) };
        unsafe {
            super::init::initialize_base_bindings(self.base_env);
            replace_current_instance(previous);
        }
    }

    /// Allocate an owned environment node outside the arena.
    fn push_env(
        env_nodes: &mut Vec<Box<SexprecCore>>,
        frame: SEXP,
        enclos: SEXP,
        hashtab: SEXP,
    ) -> SEXP {
        let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::ENVSXP));
        let env: SEXP = &mut *boxed as *mut _;
        unsafe {
            (*env).data.envsxp.frame = frame;
            (*env).data.envsxp.enclos = enclos;
            (*env).data.envsxp.hashtab = hashtab;
        }
        env_nodes.push(boxed);
        env
    }

    /// Return true if this instance owns the given SEXP pointer.
    ///
    /// This covers arena nodes plus the persistent nodes stored directly on the
    /// instance (environment sentinels, interned symbols, and raw cons cells).
    pub(crate) fn owns_sexp(&self, ptr: SEXP) -> bool {
        if ptr.is_null() {
            return false;
        }

        self.arena.contains(ptr)
            || self.env_nodes.iter().any(|node| std::ptr::eq(&**node, ptr))
            || self
                .symbol_nodes
                .iter()
                .any(|node| std::ptr::eq(&**node, ptr))
            || self.raw_cons.iter().any(|raw| std::ptr::eq(*raw, ptr))
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
    pub unsafe fn clear_current_renderplot_backend(&mut self) {
        self.current_renderplot_backend = None;
    }
}

/// Free functions (visible to embed etc. when the feature is enabled) to set the
/// current DrawTarget (RenderPlot) backend for the active instance (used during render to
/// forward drawing to skia for real R graphics).
#[cfg(feature = "renderplot-device")]
pub unsafe fn set_current_renderplot_backend(backend: *mut dyn r_graphics_engine::DrawTarget) {
    with_required_current_instance(|inst| {
        inst.current_renderplot_backend = Some(backend);
    });
}

#[cfg(feature = "renderplot-device")]
pub unsafe fn clear_current_renderplot_backend() {
    with_required_current_instance(|inst| {
        inst.current_renderplot_backend = None;
    });
}

impl Default for RInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RInstance {
    fn drop(&mut self) {
        for ptr in self.raw_cons.drain(..) {
            if !ptr.is_null() {
                unsafe {
                    let _ = Box::from_raw(ptr);
                }
            }
        }
        for (ptr, layout) in self.vmax.drain(..) {
            if !ptr.is_null() && layout.size() > 0 {
                unsafe {
                    dealloc(ptr, layout);
                }
            }
        }
        if self.eval_state.profiling.profile_outfile >= 0 {
            unsafe {
                libc::close(self.eval_state.profiling.profile_outfile);
            }
            self.eval_state.profiling.profile_outfile = -1;
        }
    }
}

// ---------------------------------------------------------------------------
// Thread-local current instance
// ---------------------------------------------------------------------------

thread_local! {
    /// Pointer to the currently active `RInstance`, if any.
    ///
    /// Stored as a raw pointer to avoid requiring `Sync` on `RInstance`.
    /// The instance itself is owned by an `RSession` (via `Box<RInstance>`),
    /// so the pointer is valid for the lifetime of that session.
    static CURRENT_INSTANCE: RefCell<Option<*mut RInstance>> = const { RefCell::new(None) };

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
    F: FnOnce(&mut RInstance) -> R,
{
    let _borrow = enter_instance_borrow();
    // SAFETY: original design used raw ptr precisely to support ambient access
    // patterns including the safe point extra-protect bracketing gc.
    unsafe { f(&mut *ptr) }
}

/// Set the current thread-local R instance for translated compatibility code.
///
/// # Safety
///
/// The caller must ensure that `instance` points to a valid, live `RInstance`
/// and that it is restored or cleared before the pointed-to instance is dropped.
pub unsafe fn set_current_instance(instance: *mut RInstance) {
    CURRENT_INSTANCE.with(|ci| {
        *ci.borrow_mut() = Some(instance);
    });
}

/// Replace the current thread-local R instance and return the previous value.
///
/// This is the primitive used by scoped session activation. It only stores raw
/// pointers; callers remain responsible for ensuring any non-null pointer stays
/// valid while installed.
pub unsafe fn replace_current_instance(instance: Option<*mut RInstance>) -> Option<*mut RInstance> {
    CURRENT_INSTANCE.with(|ci| {
        let mut current = ci.borrow_mut();
        let previous = *current;
        *current = instance;
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
pub fn current_instance_ptr() -> Option<*mut RInstance> {
    CURRENT_INSTANCE.with(|ci| *ci.borrow())
}

/// Execute a closure with a reference to the current instance, if active.
///
/// Returns `None` (and does not call `f`) if no instance is currently active.
#[inline]
pub fn with_current_instance<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut RInstance) -> R,
{
    CURRENT_INSTANCE.with(|ci| {
        let borrow = ci.borrow();
        match *borrow {
            Some(ptr) => {
                // Guarded: prevents overlapping &mut derived from the raw current ptr.
                Some(acquire_instance_mut(ptr, f))
            }
            None => None,
        }
    })
}

/// Execute a closure with the current instance.
///
/// Mutable interpreter state must be accessed through an active `RInstance`.
/// A missing instance indicates an unscoped runtime entrypoint that should be
/// routed through `RSession` before it reaches interpreter internals.
#[inline]
pub fn with_required_current_instance<F, R>(f: F) -> R
where
    F: FnOnce(&mut RInstance) -> R,
{
    with_current_instance(f).expect("mutable R runtime state requires an active RInstance")
}

/// Return whether the active session has requested cooperative cancellation.
#[inline]
pub fn is_cancellation_requested() -> bool {
    with_current_instance(|inst| {
        inst.eval_state
            .cancellation
            .as_ref()
            .is_some_and(|token| token.is_requested())
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
            assert_eq!((*instance.base_env).data.envsxp.enclos, instance.empty_env);
            assert_eq!((*instance.global_env).data.envsxp.enclos, instance.base_env);
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
