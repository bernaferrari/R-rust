use super::*;

// ---------------------------------------------------------------------------
// RSession — per-thread interpreter context
// ---------------------------------------------------------------------------

/// An isolated R interpreter session.
///
/// Each session has its own memory arena and evaluation environment.
/// This is the primary entry point for Android embedding.
///
/// # Thread Safety
///
/// `RSession` is thread-confined. Each worker thread should create and keep its
/// own session; multiple sessions can run in parallel on different workers.
pub struct RSession {
    pub(crate) core: CoreRSession,
    pub(crate) result_limit: Option<usize>,
}

fn result_from_sexp(sexp: Sexp<'_>) -> RResult {
    let typed = RValue::from_sexp(sexp.clone());
    RResult {
        value: typed.numeric_scalar_value(),
        typed,
        output: output::format_sexp_direct(sexp),
        stdout: String::new(),
        stderr: String::new(),
    }
}

fn append_captured(output: &mut output::RCapturedOutput, next: output::RCapturedOutput) {
    output.stdout.push_str(&next.stdout);
    output.stderr.push_str(&next.stderr);
    output.interleaved.push_str(&next.interleaved);
    output.truncated |= next.truncated;
}

fn with_output_authority<T>(
    original: &crate::sexp::owner::WeakOwner,
    operation: impl FnOnce() -> T,
) -> Result<T, String> {
    crate::sexp::owner::with_runtime(original, |_| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
            Ok(value) => Ok(value),
            Err(_payload) if !original.is_live() => {
                Err(crate::sexp::object::SexpError::RootUnavailable.to_string())
            }
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
    .map_err(|error| error.to_string())?
}

fn result_from_eval(
    sexp: Sexp<'_>,
    mut captured: output::RCapturedOutput,
    visible: bool,
    limit: Option<usize>,
    export_value: bool,
    original: Option<&crate::sexp::owner::WeakOwner>,
) -> RResult {
    let Some(owner) = original else {
        return error_result_with_captured("session is closed", &captured, None);
    };
    let result = with_output_authority(owner, || {
        if let Some(limit) = limit {
            if visible || export_value {
                if let Err(message) = result_budget::admit(sexp.clone(), limit) {
                    return error_result_with_captured(message, &captured, original);
                }
            }
        }
        if visible {
            match output::capture_printed_value(sexp.clone()) {
                Ok(printed) => append_captured(&mut captured, printed),
                Err(error) => {
                    append_captured(&mut captured, error.captured);
                    return error_result_with_captured(error.message, &captured, original);
                }
            }
        }
        if !owner.is_live() {
            return error_result_with_captured(
                crate::sexp::object::SexpError::RootUnavailable.to_string(),
                &captured,
                original,
            );
        }
        if unsafe { crate::mainutils::errors::collect_warnings() } > 0 {
            if let Some(block) = unsafe { crate::mainutils::errors::take_warnings_block() } {
                captured.stderr.push_str(&block);
                captured.interleaved.push_str(&block);
            }
        }
        let typed = if export_value {
            RValue::from_sexp(sexp)
        } else {
            RValue::Null
        };
        let mut stdout = captured.stdout.clone();
        let mut display = captured.interleaved.clone();
        if captured.truncated {
            let marker = "\n[captured console output truncated by runtime limit]";
            stdout.push_str(marker);
            display.push_str(marker);
        }
        if let Some(limit) = limit {
            result_budget::truncate(&mut display, limit);
        }
        RResult {
            value: typed.numeric_scalar_value(),
            typed,
            output: display,
            stdout,
            stderr: captured.stderr.clone(),
        }
    });
    result.unwrap_or_else(|message| error_result_with_captured(message, &captured, original))
}

fn error_result(message: impl Into<String>) -> RResult {
    RResult {
        value: 0.0,
        typed: RValue::Error(message.into()),
        output: String::new(),
        stdout: String::new(),
        stderr: String::new(),
    }
    .with_error_output()
}

/// Preserve program output, then append the exact rendering of this error once.
fn error_result_with_captured(
    message: impl Into<String>,
    captured: &crate::sexp::output::RCapturedOutput,
    original: Option<&crate::sexp::owner::WeakOwner>,
) -> RResult {
    let message = message.into();
    // A closed owner grants no error-state access. An enclosing different
    // runtime must never supply this session's error or deferred warnings.
    let details = original.and_then(|owner| {
        with_output_authority(owner, || {
            let rendered = crate::mainutils::errors::try_last_rendered_message(&message);
            let warnings = if unsafe { crate::mainutils::errors::collect_warnings() } > 0 {
                unsafe { crate::mainutils::errors::take_warnings_block() }
            } else {
                None
            };
            (rendered, warnings)
        })
        .ok()
    });
    let (rendered, warnings) = details.unwrap_or_default();
    let rendered = rendered.unwrap_or_else(|| format!("Error: {message}\n"));
    let mut stdout = captured.stdout.clone();
    let mut stderr = captured.stderr.clone();
    let mut display = captured.interleaved.clone();
    stderr.push_str(&rendered);
    display.push_str(&rendered);
    if let Some(block) = warnings {
        let warning = format!("In addition: {block}");
        stderr.push_str(&warning);
        display.push_str(&warning);
    }
    if captured.truncated {
        let marker = "\n[captured console output truncated by runtime limit]";
        stdout.push_str(marker);
        display.push_str(marker);
    }
    RResult {
        value: 0.0,
        typed: RValue::Error(message),
        output: display,
        stdout,
        stderr,
    }
}

fn is_valid_package_name(package: &str) -> bool {
    let mut chars = package.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && package
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '.')
}

impl RSession {
    pub fn set_output_limit(&mut self, max_bytes: Option<usize>) {
        self.core.set_output_limit(max_bytes);
    }

    pub fn set_result_limit(&mut self, max_bytes: Option<usize>) {
        self.result_limit = max_bytes;
    }

    pub fn new() -> Self {
        RSession {
            core: CoreRSession::new_detached(),
            result_limit: None,
        }
    }

    /// Configure library discovery before the runtime loads default packages.
    pub fn new_with_path_policy(policy: RuntimePathPolicy) -> Self {
        RSession {
            core: CoreRSession::new_detached_with_path_policy(policy),
            result_limit: None,
        }
    }

    pub fn close(&mut self) {
        self.core.close();
    }
    pub fn enable_browser_files(&mut self) {
        self.core.enable_browser_files();
    }
    pub fn import_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.core.put_browser_file(path, bytes)
    }
    pub fn export_file(&mut self, path: &str) -> Option<Vec<u8>> {
        self.core.get_browser_file(path)
    }
    pub fn list_files(&mut self) -> Vec<String> {
        self.core.list_browser_files()
    }
    pub fn remove_file(&mut self, path: &str) -> bool {
        self.core.remove_browser_file(path)
    }

    pub fn is_active(&self) -> bool {
        self.core.is_active()
    }

    /// Evaluate and retain a value in private owning engine storage.
    pub fn define_retained(&mut self, code: &str) -> Result<RetainedValueId, String> {
        self.core
            .define_retained(code)
            .map_err(|error| error.message)
    }

    /// Copy the retained value directly, without R lookup or auto-printing.
    pub fn retained_snapshot(&self, id: RetainedValueId) -> Result<RValue, String> {
        self.core
            .with_retained_value(id, |value| {
                if let Some(limit) = self.result_limit {
                    result_budget::admit(value.clone(), limit).map_err(|message| {
                        crate::sexp::session::REvalError {
                            message: message.into(),
                        }
                    })?;
                }
                value
                    .to_owned_value()
                    .map(RValue::from_owned_value)
                    .map_err(|error| crate::sexp::session::REvalError {
                        message: error.to_string(),
                    })
            })
            .map_err(|error| error.message)
    }

    pub fn validate_retained(&self, id: RetainedValueId) -> Result<(), String> {
        self.core
            .validate_retained(id)
            .map_err(|error| error.message)
    }

    pub fn set_retained(&mut self, id: RetainedValueId, code: &str) -> Result<(), String> {
        self.core
            .set_retained(id, code)
            .map_err(|error| error.message)
    }

    pub fn update_retained(&mut self, id: RetainedValueId, code: &str) -> Result<(), String> {
        self.core
            .update_retained(id, code)
            .map_err(|error| error.message)
    }

    pub fn remove_retained(&mut self, id: RetainedValueId) -> Result<(), String> {
        self.core.remove_retained(id).map_err(|error| error.message)
    }

    pub fn runtime_info(&self) -> RRuntimeInfo {
        RRuntimeInfo {
            is_active: self.is_active(),
            library_paths: self
                .core
                .library_paths()
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            temp_dir: self.core.temp_dir().to_string_lossy().into_owned(),
        }
    }

    pub fn resource_limits(&self) -> RResourceLimits {
        let eval = self.core.eval_limits();
        let arena = self.core.arena_budget();
        RResourceLimits {
            max_eval_depth: eval.max_eval_depth as u64,
            max_execution_time_ms: eval.max_execution_time_ms,
            max_alloc_bytes: arena.max_bytes as u64,
            max_arena_nodes: arena.max_nodes as u64,
        }
    }

    pub fn arena_stats(&mut self) -> RArenaStats {
        self.core
            .with_arena(|arena| RArenaStats {
                active_nodes: arena.node_count() as u64,
                free_nodes: arena.free_count() as u64,
                retained_bytes: arena.total_bytes_allocated() as u64,
                fragmentation_ratio: arena.fragmentation_ratio(),
            })
            .unwrap_or_default()
    }

    pub fn set_capabilities(&mut self, capabilities: crate::sexp::instance::SessionCapabilities) {
        self.core.set_capabilities(capabilities);
    }

    /// Tag the session's top-level evaluation policy. `.Last.value`
    /// publication is GNU-faithful in every mode (script included), so the
    /// engine does not branch on this value.
    pub fn set_top_level_evaluation_mode(&mut self, mode: TopLevelEvaluationMode) {
        self.core.set_top_level_evaluation_mode(mode);
    }

    /// Enable trusted process capabilities for desktop-style hosts. Mobile
    /// and WASM sessions retain the deny-by-default native-code policy.
    pub fn enable_host_process_capabilities(&mut self) {
        self.set_capabilities(crate::sexp::instance::SessionCapabilities {
            allow_system_commands: true,
            allow_pipe_commands: true,
            allow_environment_mutation: true,
        });
        self.core.with_active(|| {
            crate::mainutils::rdynload::set_native_extensions_enabled(true);
            // GNU Rscript: setlocale(LC_ALL, "") then LC_NUMERIC back to "C"
            // so collation follows the host but numeric formatting stays C.
            #[cfg(not(target_arch = "wasm32"))]
            unsafe {
                libc::setlocale(libc::LC_ALL, c"".as_ptr());
                libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
            }
        });
    }

    pub fn set_resource_limits(&mut self, limits: RResourceLimits) {
        self.core.set_eval_limits(crate::eval::eval::EvalLimits {
            max_eval_depth: saturating_usize(limits.max_eval_depth),
            max_execution_time_ms: limits.max_execution_time_ms,
            max_alloc_bytes: saturating_usize(limits.max_alloc_bytes),
        });
        let node_cap = saturating_usize(limits.max_arena_nodes);
        self.core.set_arena_budget(ArenaBudget::new(
            saturating_usize(limits.max_alloc_bytes),
            node_cap,
        ));
        // The node guard is cumulative (never refunded by collection): a
        // host that caps `max_arena_nodes` must see churn-heavy workloads
        // fail with a catchable error instead of silently recycling
        // garbage-collected nodes under the cap forever.
        self.core.with_arena(|arena| arena.set_node_guard(node_cap));
    }

    /// Return true when a package exists in this session's configured library paths.
    pub fn package_available(&self, package: &str) -> bool {
        is_valid_package_name(package) && self.core.find_package_path(package).is_some()
    }

    /// Return the resolved package directory, if it exists in this session.
    pub fn package_path(&self, package: &str) -> Option<String> {
        if !is_valid_package_name(package) {
            return None;
        }
        self.core
            .find_package_path(package)
            .map(|path| path.to_string_lossy().into_owned())
    }

    /// Load a pure-R package by name through the same evaluator path as `library()`.
    pub fn load_package(&mut self, package: &str) -> Result<(), String> {
        if !is_valid_package_name(package) {
            return Err("invalid package name".to_string());
        }
        let result = self.eval(&format!("library(\"{package}\")"));
        match result.typed {
            RValue::Error(message) => Err(message),
            _ => Ok(()),
        }
    }

    /// Snapshot the global environment's binding names (`ls(all.names=TRUE)`).
    ///
    /// Owned strings only — no raw `SEXP` crosses this boundary.
    pub fn global_binding_names(&mut self) -> Vec<String> {
        self.core.global_binding_names()
    }

    pub fn set_cancellation_token(&mut self, token: Option<CancellationToken>) {
        self.core.set_cancellation_token(token);
    }

    /// Evaluate with a cancellation token scoped to this call.
    ///
    /// The previous token is restored afterward, so a cancelled Android tab does
    /// not poison later work in the same session or any other session.
    pub fn eval_with_cancellation_token(
        &mut self,
        code: &str,
        token: Option<CancellationToken>,
    ) -> RResult {
        self.eval_script_with_cancellation_token(code, token)
    }

    /// Check whether `code` is a syntactically complete R program without
    /// evaluating it.
    ///
    /// Runs the same parser as evaluation against this session's arena:
    /// `Ok(true)` when the code parses as complete top-level expressions —
    /// including complete-but-malformed input such as a stray `)`, which
    /// evaluation then reports with the usual upstream-shaped parse error —
    /// and `Ok(false)` when the parser reports an unexpected end of input
    /// (interactive hosts show a continuation prompt). `Err` is reserved for
    /// session-level failure.
    pub fn is_syntax_complete(&mut self, code: &str) -> Result<bool, String> {
        let owner = self
            .core
            .owner_token()
            .ok_or_else(|| "session closed".to_string())?;
        let factory = crate::sexp::object::SessionNodeFactory::new(owner);
        self.core.with_active_in(|instance| unsafe {
            // SAFETY: the session retains the owner; only the arena is lent.
            crate::sexp::memory::with_arena_in(instance, |arena| {
                match crate::eval::parser::parse_expressions(code, arena, factory) {
                    Err(e) if e.0 == "unexpected end of input" => Ok(false),
                    Ok(_) | Err(_) => Ok(true),
                }
            })
        })
    }

    /// Configure app-private runtime paths for Android embedding.
    ///
    /// `app_files_dir` owns the writable user library, `cache_dir` owns
    /// `tempdir()`, and `bundled_library_dir` points at the read-only package
    /// library shipped with the app.
    pub fn configure_paths(
        &mut self,
        app_files_dir: &str,
        cache_dir: &str,
        bundled_library_dir: Option<&str>,
    ) -> Result<(), String> {
        self.core
            .configure_android_paths(app_files_dir, cache_dir, bundled_library_dir)
            .map_err(|err| err.to_string())
    }

    pub fn eval_integer(&mut self, value: i32) -> RResult {
        self.core
            .with_arena(|arena| builder::scalar_integer_in(arena, value).map(result_from_sexp))
            .flatten()
            .unwrap_or_else(allocation_error)
    }

    pub fn eval_real(&mut self, value: f64) -> RResult {
        self.core
            .with_arena(|arena| builder::scalar_real_in(arena, value).map(result_from_sexp))
            .flatten()
            .unwrap_or_else(allocation_error)
    }

    pub fn eval_int_vector(&mut self, values: &[i32]) -> RResult {
        self.core
            .with_arena(|arena| builder::int_vec_in(arena, values).map(result_from_sexp))
            .flatten()
            .unwrap_or_else(allocation_error)
    }

    pub fn eval_real_vector(&mut self, values: &[f64]) -> RResult {
        self.core
            .with_arena(|arena| builder::real_vec_in(arena, values).map(result_from_sexp))
            .flatten()
            .unwrap_or_else(allocation_error)
    }

    /// Compute a mathematical function on a single value.
    pub fn math_unary(&self, func: RMathFunc, x: f64) -> f64 {
        match func {
            RMathFunc::Abs => x.abs(),
            RMathFunc::Sqrt => libm::sqrt(x),
            RMathFunc::Ceil => libm::ceil(x),
            RMathFunc::Floor => libm::floor(x),
            RMathFunc::Trunc => libm::trunc(x),
            RMathFunc::Exp => libm::exp(x),
            RMathFunc::Log => libm::log(x),
            RMathFunc::Log2 => libm::log2(x),
            RMathFunc::Log10 => libm::log10(x),
            RMathFunc::Sin => libm::sin(x),
            RMathFunc::Cos => libm::cos(x),
            RMathFunc::Tan => libm::tan(x),
            RMathFunc::Asin => libm::asin(x),
            RMathFunc::Acos => libm::acos(x),
            RMathFunc::Atan => libm::atan(x),
            RMathFunc::Gamma => libm::tgamma(x),
            RMathFunc::Lgamma => libm::lgamma(x),
        }
    }

    /// Compute a Bessel function.
    pub fn bessel(&self, func: RBesselFunc, x: f64, alpha: f64, scaled: bool) -> f64 {
        match func {
            RBesselFunc::J => crate::special::bessel::bessel_j(x, alpha),
            RBesselFunc::Y => crate::special::bessel::bessel_y(x, alpha),
            RBesselFunc::I => crate::special::bessel::bessel_i(x, alpha, scaled),
            RBesselFunc::K => crate::special::bessel::bessel_k(x, alpha, scaled),
        }
    }

    /// Probability density function for the normal distribution.
    pub fn dnorm(&self, x: f64, mean: f64, sd: f64, log: bool) -> f64 {
        crate::dist::normal::dnorm(x, mean, sd, if log { 1 } else { 0 })
    }

    pub fn pnorm(&self, x: f64, mean: f64, sd: f64, lower_tail: bool, log: bool) -> f64 {
        crate::dist::normal::pnorm(
            x,
            mean,
            sd,
            if lower_tail { 1 } else { 0 },
            if log { 1 } else { 0 },
        )
    }

    pub fn qnorm(&self, p: f64, mean: f64, sd: f64, lower_tail: bool, log: bool) -> f64 {
        crate::dist::normal::qnorm(
            p,
            mean,
            sd,
            if lower_tail { 1 } else { 0 },
            if log { 1 } else { 0 },
        )
    }

    pub fn unif_rand(&self) -> f64 {
        self.core.unif_rand()
    }

    pub fn set_seed(&self, i1: u32, i2: u32) {
        self.core.set_seed(i1, i2);
    }

    pub fn norm_rand(&self) -> f64 {
        self.core.norm_rand()
    }

    /// Parse and evaluate an R expression.
    ///
    /// Parses and evaluates code against this session's isolated global
    /// environment.
    pub fn eval(&mut self, code: &str) -> RResult {
        self.eval_script(code)
    }

    /// Parse and evaluate a multi-expression R script.
    pub fn eval_script(&mut self, code: &str) -> RResult {
        self.eval_script_with_cancellation_token(code, None)
    }

    /// Evaluate a script with a cancellation token scoped to this call.
    pub fn eval_script_with_cancellation_token(
        &mut self,
        code: &str,
        token: Option<CancellationToken>,
    ) -> RResult {
        self.eval_script_export(code, token, true)
    }

    /// Evaluate for display without cloning the final value into a host object.
    pub fn eval_display(&mut self, code: &str) -> RResult {
        self.eval_script_export(code, None, false)
    }

    fn eval_script_export(
        &mut self,
        code: &str,
        token: Option<CancellationToken>,
        export_value: bool,
    ) -> RResult {
        let limit = self.result_limit;
        let original = self.core.owner_token().and_then(|owner| owner.weak_owner());
        let previous = self.core.replace_cancellation_token(token);
        let result =
            self.core
                .eval_script_with_output_capture_then(
                    code,
                    |result, captured, visible| match result {
                        Ok(result) => result_from_eval(
                            result,
                            captured,
                            visible,
                            limit,
                            export_value,
                            original.as_ref(),
                        ),
                        Err(e) => {
                            error_result_with_captured(e.to_string(), &captured, original.as_ref())
                        }
                    },
                );
        self.core.set_cancellation_token(previous);
        result
    }

    #[cfg(feature = "renderplot-device")]
    /// Evaluate a script with a RenderPlot drawing backend.
    ///
    /// Takes the backend by exclusive reference: no raw pointers cross this
    /// embedding boundary. The backend is installed as the current DrawTarget
    /// for the duration of the evaluation and restored afterward.
    pub fn eval_script_with_renderplot_backend<'session>(
        &'session mut self,
        code: &str,
        backend: &'session mut dyn r_graphics_engine::DrawTarget,
    ) -> RResult {
        // SAFETY: the backend reference outlives this call ('session) and the
        // internal backend slot is restored before returning, so the raw
        // pointer stored by the activation layer never dangles.
        let backend = backend as *mut (dyn r_graphics_engine::DrawTarget + 'session);
        let limit = self.result_limit;
        let original = self.core.owner_token().and_then(|owner| owner.weak_owner());
        self.core.eval_script_with_output_capture_then_renderplot(
            code,
            backend,
            |result, captured, visible| match result {
                Ok(result) => {
                    result_from_eval(result, captured, visible, limit, false, original.as_ref())
                }
                Err(e) => error_result_with_captured(e.to_string(), &captured, original.as_ref()),
            },
        )
    }
}

fn allocation_error() -> RResult {
    error_result("allocation failed")
}

impl Default for RSession {
    fn default() -> Self {
        Self::new()
    }
}
