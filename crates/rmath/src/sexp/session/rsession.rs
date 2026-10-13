use super::*;

/// Reserved policy knob for top-level evaluation. `.Last.value` publication
/// is unconditional and GNU-faithful in every mode (main.c `R_ReplFile`
/// publishes per expression in script mode too), so the engine itself no
/// longer branches on this value; embedders keep it to tag sessions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TopLevelEvaluationMode {
    #[default]
    Console,
    Script,
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
    top_level_evaluation_mode: TopLevelEvaluationMode,
    /// Host values own their physical leases outside the R binding graph.
    pub(crate) retained_values: retained::RetainedValues,
    /// Native projection of the shared interior cell owned below.
    pub(crate) instance: *mut RInstance,
    /// Physical allocation authority. Operation guards clone this original
    /// allocation, so callbacks cannot destroy active runtime bytes.
    pub(crate) _instance_owner: Rc<UnsafeCell<RInstance>>,
    /// Marker that keeps sessions thread-confined at compile time.
    _thread_confined: PhantomData<Rc<()>>,
}

impl RSession {
    /// Set the policy tag for subsequent top-level evaluations. `.Last.value`
    /// publication already matches GNU in every mode; this only records the
    /// embedder's intent for the session.
    pub fn set_top_level_evaluation_mode(&mut self, mode: TopLevelEvaluationMode) {
        self.top_level_evaluation_mode = mode;
    }

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

    /// Initialize the full runtime with an explicit path policy installed
    /// before base bootstrap or default package loading.
    pub fn new_with_path_policy(policy: crate::mainutils::paths::RuntimePathPolicy) -> Self {
        Self::new_with_instance(RInstance::allocate_with_path_policy(policy), true, true)
    }

    /// Build the real base runtime without host package discovery. Collector
    /// unit tests use this to avoid loading unrelated installed R packages.
    #[cfg(test)]
    pub(crate) fn new_without_default_packages() -> Self {
        Self::new_with_instance(RInstance::new_for_gc_tests(), false, true)
    }

    fn new_with_default_packages(attach_default_packages: bool) -> Self {
        crate::sexp::context::install_r_panic_hook();
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
        crate::sexp::context::install_r_panic_hook();
        let instance_owner = Rc::new(UnsafeCell::new(instance));
        let instance = instance_owner.get();
        // Install the capability before bootstrap can create any values or
        // execute callbacks. Every managed value refers to this original Rc.
        unsafe {
            (*instance).runtime_owner =
                Some(crate::sexp::owner::WeakOwner::from_rc(&instance_owner));
        }
        // Own the allocation before installing it, so initialization unwind
        // also detaches the thread-local runtime state through normal Drop.
        let session = RSession {
            active: true,
            top_level_evaluation_mode: TopLevelEvaluationMode::Console,
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
            // GNU main.c seals base after its source/system initialization,
            // before loading default packages. Both environments share bindings.
            session.with_active(|| unsafe {
                let owner = session.owner_token().expect("active base bootstrap");
                let namespace = owner
                    .sexp(crate::sexp::envir::R_BaseNamespace())
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .expect("original base namespace");
                let base = owner
                    .sexp(crate::sexp::globals::R_BaseEnv())
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .expect("original base environment");
                let mut frame = namespace
                    .try_frame()
                    .expect("base frame")
                    .into_owned()
                    .expect("owned base frame");
                let mut symbols = Vec::new();
                while !frame.is_nil() {
                    symbols.push(
                        frame
                            .try_tag()
                            .expect("base binding tag")
                            .into_owned()
                            .expect("owned base tag"),
                    );
                    frame = frame
                        .try_cdr()
                        .expect("base frame tail")
                        .into_owned()
                        .expect("owned base tail");
                }
                crate::sexp::envir::lock_environment_raw(namespace.as_raw());
                for symbol in symbols {
                    crate::sexp::envir::lock_binding_raw(namespace.as_raw(), symbol.as_raw());
                }
                crate::sexp::envir::lock_environment_raw(base.as_raw());
                // GNU main.c runs the base package sources through
                // R_ReplFile (main.c:1067), so bootstrap leaves
                // `.Last.value` bound on the symbol cell; sealing base then
                // locks it (envir.c R_LockEnvironment walks the symbol
                // table). Mirror that end state: baseenv carries a locked
                // NULL placeholder that only the REPL's raw cell writes
                // (SET_SYMVALUE) ever update.
                let last_value_symbol = crate::sexp::symbol::Rf_install(c".Last.value".as_ptr());
                crate::sexp::accessors::SET_SYMVALUE(
                    last_value_symbol,
                    crate::sexp::globals::R_NilValue(),
                );
                crate::sexp::envir::lock_binding_raw(base.as_raw(), last_value_symbol);
                for name in [c".Device", c".Devices"] {
                    let symbol = crate::sexp::symbol::Rf_install(name.as_ptr());
                    crate::sexp::envir::unlock_binding_raw(base.as_raw(), symbol);
                }
                owner.require_active().expect("live base bootstrap owner");
            });
        }
        if !attach_default_packages {
            return session;
        }
        session.with_active(|| unsafe {
            // GNU registers the compiler namespace at engine boot.
            let _ = crate::eval::compiler::namespace();
            let factory = SessionNodeFactory::new(session.owner_token().expect("active session"));
            // GNU defaultPackages: datasets, utils, grDevices, graphics,
            // stats, methods. library() inserts at pos 2, so attach in
            // that order and stats sits just under .GlobalEnv.
            let mut stats_loaded = false;
            for package in ["methods", "datasets", "utils", "grDevices", "graphics", "stats"] {
                // The authenticated builtin datasets image owns the
                // datasets namespace even when a library tree is visible:
                // the builtin lazydata promises back `datasets::` lookups.
                if package == "datasets" {
                    crate::library::datasets::attach().unwrap_or_else(|message| {
                        std::panic::panic_any(crate::sexp::context::RError { message })
                    });
                    continue;
                }
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
                } else if package == "methods" {
                    crate::library::methods::portable::attach().unwrap_or_else(|message| {
                        std::panic::panic_any(crate::sexp::context::RError { message })
                    });
                } else if let Some(image) = crate::library::portable_package::image(package) {
                    image.attach().unwrap_or_else(|message| {
                        std::panic::panic_any(crate::sexp::context::RError { message })
                    });
                } else if package == "datasets" {
                    crate::library::datasets::attach().unwrap_or_else(|message| {
                        std::panic::panic_any(crate::sexp::context::RError { message })
                    });
                }
            }
            if stats_loaded && let Ok(exprs) = crate::sexp::memory::with_arena(|arena| {
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
            if stats_loaded && let Ok(exprs) = crate::sexp::memory::with_arena(|arena| {
                crate::eval::parser::parse_expressions(
                    "local({ if (\"package:stats\" %in% search()) { assign(\"reorder\", get(\"reorder\", baseenv()), envir = as.environment(\"package:stats\")); if (bindingIsLocked(\"xtabs\", as.environment(\"package:stats\"))) unlockBinding(\"xtabs\", as.environment(\"package:stats\")); assign(\"xtabs\", get(\"xtabs\", baseenv()), envir = as.environment(\"package:stats\")) }; f <- get(\"diff.ts\", baseenv()); for (env in list(baseenv(), get(\".BaseNamespaceEnv\", baseenv()), asNamespace(\"stats\"), as.environment(\"package:stats\"))) { tab <- tryCatch(get(\".__S3MethodsTable__.\", env), error = function(e) NULL); if (!is.null(tab) && exists(\"diff.ts\", tab, inherits = FALSE)) { if (bindingIsLocked(\"diff.ts\", tab)) unlockBinding(\"diff.ts\", tab); assign(\"diff.ts\", f, tab) }; if (exists(\"diff.ts\", env, inherits = FALSE)) { if (bindingIsLocked(\"diff.ts\", env)) unlockBinding(\"diff.ts\", env); assign(\"diff.ts\", f, env) } } })",
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
        // GNU sessions start with R_Visible = TRUE; the package-loading
        // dance above can leave the flag FALSE (e.g. invisible detach()).
        session.with_active(|| crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE));

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

    pub(crate) fn new_detached_with_path_policy(
        policy: crate::mainutils::paths::RuntimePathPolicy,
    ) -> Self {
        Self::construct_detached(|| Self::new_with_path_policy(policy))
    }

    pub(crate) fn construct_detached(constructor: impl FnOnce() -> Self) -> Self {
        let _ambient = CurrentInstanceGuard::detached();
        let session = constructor();
        detach_state(unsafe { &(*session.instance).math_state });
        detach_rng(unsafe { &(*session.instance).rng_state });
        clear_current_instance_if(session.instance);
        session
    }

    /// Short immutable view of the owned interior cell.
    #[inline]
    pub(crate) fn inst(&self) -> &RInstance {
        // SAFETY: the Rc-owned cell remains allocated for this session.
        // Callers keep field borrows local and end them before R callbacks.
        unsafe { &*self.instance }
    }

    pub(crate) fn instance_ptr(&self) -> *mut RInstance {
        self.instance
    }

    pub(crate) fn activate(&self) -> CurrentInstanceGuard {
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
    pub(crate) fn owner_token(&self) -> Option<crate::sexp::owner::OwnerToken<'_>> {
        if !self.is_active() {
            return None;
        }
        // SAFETY: self retains the original allocation; the return lifetime
        // prevents closing or moving this session while the token is used.
        Some(unsafe { crate::sexp::owner::OwnerToken::from_raw(self.instance) })
    }

    /// Check if this session is active.
    ///
    /// Returns `false` after close or revocation by an original-runtime callback.
    pub fn is_active(&self) -> bool {
        // The session retains physical bytes; this callback-free availability
        // snapshot never adopts a runtime from the ambient session stack.
        self.active && unsafe { crate::sexp::instance::instance_liveness(self.instance).is_live() }
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
    ) -> (RResult<SEXP>, crate::sexp::output::RCapturedOutput, bool) {
        let expr = match self.owned_sexp(expr_or_nil(expr), "expression") {
            Ok(expr) => expr,
            Err(err) => {
                return (
                    Err(err),
                    crate::sexp::output::RCapturedOutput::default(),
                    false,
                );
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
        crate::sexp::output::RCapturedOutput,
        bool,
    ) {
        if !self.is_active() {
            return (
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                crate::sexp::output::RCapturedOutput::default(),
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
        crate::sexp::output::RCapturedOutput,
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
        crate::sexp::output::capture_stderr(&text);
        if !text.ends_with('\n') {
            crate::sexp::output::capture_stderr("\n");
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
        crate::sexp::output::RCapturedOutput,
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
        F: FnOnce(RResult<Sexp<'session>>, crate::sexp::output::RCapturedOutput, bool) -> T,
    {
        if !self.is_active() {
            return f(
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                crate::sexp::output::RCapturedOutput::default(),
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
                crate::sexp::memory::with_arena_in(self.instance, |arena| {
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
                            crate::sexp::output::RCapturedOutput::default(),
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
            let capture = crate::sexp::output::OutputCaptureGuard::start();
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
                // GNU main.c publishes after EVERY successful top-level
                // expression in both loops — R_ReplFile:99 (Rscript file
                // mode) and the console read-eval-print at main.c:265/402 —
                // regardless of batch or interactive mode. SET_SYMVALUE is a
                // raw cell write: it bypasses active bindings and the base
                // environment lock.
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
                        _toplevel_no_guard.run_checked(|| crate::sexp::output::print_value(value))
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
        F: FnOnce(RResult<Sexp<'session>>, crate::sexp::output::RCapturedOutput, bool) -> T,
    {
        if !self.is_active() {
            return f(
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                crate::sexp::output::RCapturedOutput::default(),
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
                crate::sexp::memory::with_arena_in(instance, |arena| {
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
                        crate::sexp::output::RCapturedOutput::default(),
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
            let capture = crate::sexp::output::OutputCaptureGuard::start();
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
                // Same unconditional publication as the plain script loop
                // above (GNU R_ReplFile:99 SETs the cell in script mode).
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
                        _toplevel_no_guard.run_checked(|| crate::sexp::output::print_value(value))
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
        F: FnOnce(RResult<Sexp<'session>>, crate::sexp::output::RCapturedOutput, bool) -> T,
    {
        if !self.is_active() {
            return f(
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                crate::sexp::output::RCapturedOutput::default(),
                false,
            );
        }

        let expr = {
            let _guard = self.activate();
            let factory = SessionNodeFactory::new(self.owner_token().expect("active session"));
            unsafe {
                /* SAFETY: session activates its checked owner; unsafe payload loans must exclude R reentry. */
                crate::sexp::memory::with_arena_in(self.instance, |arena| {
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
                        crate::sexp::output::RCapturedOutput::default(),
                        false,
                    )
                });
            }
        };
        self.with_active(|| {
            let capture = crate::sexp::output::OutputCaptureGuard::start();
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
        Some(unsafe { crate::sexp::memory::with_arena_in(self.instance, f) })
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
    pub fn capabilities(&self) -> crate::sexp::instance::SessionCapabilities {
        self.inst().eval_state.capabilities
    }

    /// Configure which host-process operations this session may invoke.
    pub fn set_capabilities(&mut self, capabilities: crate::sexp::instance::SessionCapabilities) {
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
    pub fn with_output_capture<F, T>(&self, f: F) -> (T, crate::sexp::output::RCapturedOutput)
    where
        F: FnOnce() -> T,
    {
        self.with_active(|| {
            let capture = crate::sexp::output::OutputCaptureGuard::start();
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
            crate::sexp::instance::revoke_instance_availability(self.instance);
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
    crate::sexp::globals::immutable_singleton_projection(ptr)
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
            crate::sexp::instance::revoke_instance_availability(self.instance);
        }
        if self.active {
            detach_state(&self.inst().math_state);
            detach_rng(&self.inst().rng_state);
            clear_current_instance_if(self.instance);
        }
        // The owning Rc field releases the instance through ordinary Rust Drop.
    }
}
