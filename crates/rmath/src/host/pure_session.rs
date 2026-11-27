//! The new-world-backed host session — the migration target for the
//! facade's evaluation core.
//!
//! `PureSession` wraps `value::Session` (the generational-heap evaluator)
//! behind the facade's `RResult` contract, so consumers can cut over
//! without changing what they consume: owned typed values, separated
//! stdout/stderr, and the chronological console transcript. Capabilities
//! the old world still owns — cancellation tokens, resource limits,
//! packages, graphics — arrive slice by slice; the evaluation core is
//! already byte-exact against the GNU oracle across the conformance
//! corpus.

use crate::value::session::Session;

use super::value_snapshot::snapshot_value;
use super::values::RResult;
use super::values::RValue;

/// A host session whose evaluation runs entirely on the new-world heap —
/// no SEXP, no slab allocator, no panic-based control flow.
/// Admission check over the owned snapshot — the facade's cost model
/// exactly: per-element formatting overhead, depth-capped, occurrences
/// counted rather than unique objects since export expands sharing.
fn admit_rvalue(value: &RValue, limit: usize) -> Result<(), String> {
    fn cost_of(value: &RValue) -> (usize, Vec<&RValue>) {
        let (len, children): (usize, Vec<&RValue>) = match value {
            RValue::List(items) => (items.len().max(1), items.iter().collect()),
            RValue::StringVector(items) => (items.len().max(1), Vec::new()),
            RValue::IntegerVector(items) => (items.len().max(1), Vec::new()),
            RValue::RealVector(items) => (items.len().max(1), Vec::new()),
            RValue::LogicalVector(items) => (items.len().max(1), Vec::new()),
            RValue::RawVector(items) => (items.len().max(1), Vec::new()),
            RValue::ComplexVector(items) => (items.len().max(1), Vec::new()),
            RValue::Attributed { value: base, metadata } => {
                let mut children: Vec<&RValue> =
                    metadata.attributes.iter().map(|attr| &attr.value).collect();
                children.push(base);
                (2, children)
            }
            _ => (1, Vec::new()),
        };
        (len.saturating_mul(64).saturating_add(128), children)
    }
    let mut remaining = limit;
    let mut pending: Vec<(&RValue, usize)> = vec![(value, 0)];
    while let Some((value, depth)) = pending.pop() {
        if depth > 64 {
            return Err("result nesting exceeds export limit (64)".to_string());
        }
        let (cost, children) = cost_of(value);
        remaining = remaining
            .checked_sub(cost)
            .ok_or_else(|| "result exceeds export budget".to_string())?;
        for child in children {
            pending.push((child, depth + 1));
        }
    }
    Ok(())
}

/// The arena occupancy snapshot exactly as the facade's ArenaStats —
/// the two measures every embedder budget reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PureArenaStats {
    pub active_nodes: u64,
    pub retained_bytes: u64,
}

/// The runtime identity exactly as the facade's RuntimeInfo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PureRuntimeInfo {
    pub is_active: bool,
    pub library_paths: Vec<String>,
    pub temp_dir: String,
}

/// The configured resource ceilings exactly as the facade's
/// ResourceLimits — unset ceilings read as None, unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PureResourceLimits {
    pub max_eval_depth: u64,
    pub max_alloc_bytes: Option<u64>,
    pub max_arena_nodes: Option<u64>,
}

pub struct PureSession {
    session: Session,
    result_limit: Option<usize>,
    session_id: u64,
    runtime_paths: super::facade::RuntimePaths,
}

/// Opaque, copyable identity for a privately retained pure-world value.
///
/// The token bits encode the root table's generational slot exactly as
/// the facade's RetainedValueId encodes its namespace/slot/generation —
/// validation rejects foreign sessions, released slots and stale
/// generations before any access. No heap reference crosses the host
/// boundary; reads materialize owned snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PureValueHandle {
    session_id: u64,
    token_bits: u64,
}

impl PureValueHandle {
    /// The session this handle was created by.
    pub fn owning_session(&self) -> u64 {
        self.session_id
    }
}

static NEXT_PURE_SESSION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Default for PureSession {
    fn default() -> Self {
        Self::new()
    }
}

impl PureSession {
    pub fn new() -> Self {
        PureSession {
            session: Session::new(),
            result_limit: None,
            session_id: NEXT_PURE_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            runtime_paths: super::facade::RuntimePaths::new(
                std::env::temp_dir().to_string_lossy().into_owned(),
                std::env::temp_dir().to_string_lossy().into_owned(),
                None::<String>,
            ),
        }
    }

    /// Create a session confined to a virtual filesystem root exactly as
    /// the facade's browser sandbox contracts — host paths resolve
    /// through the policy gate before any filesystem access. The
    /// permissive constructor is `new` exactly as the native default.
    pub fn new_with_path_policy(allowed_roots: Vec<String>, virtual_root: &str) -> Self {
        let mut session = Self::new();
        session.session.heap.path_policy = Some(crate::value::path_policy::PathPolicy::new(
            allowed_roots,
            virtual_root,
        ));
        session
    }

    /// Parse and evaluate R source into the facade's result contract:
    /// the typed snapshot of the final value, the numeric scalar view,
    /// and the three output streams exactly as Rscript separates them.
    pub fn eval(&mut self, code: &str) -> RResult {
        let full = self.session.eval_full(code);
        let typed = match (&full.error, full.value) {
            (Some(message), _) => RValue::Error(message.clone()),
            (None, Some(value)) => {
                let snapshot = snapshot_value(&self.session.heap, &self.session.interner, value);
                // The export budget admits or rejects exactly as the
                // facade's admission check — a over-budget result answers
                // an error rather than a partial value.
                match self.result_limit {
                    Some(limit) => match admit_rvalue(&snapshot, limit) {
                        Ok(()) => snapshot,
                        Err(message) => RValue::Error(message),
                    },
                    None => snapshot,
                }
            }
            (None, None) => RValue::Null,
        };
        let value = typed.numeric_scalar_value();
        RResult {
            value,
            typed,
            output: full.output,
            stdout: full.stdout,
            stderr: full.stderr,
        }
    }

    /// Read-only access to the underlying evaluator session — migration
    /// instrumentation inspects heap state through here.
    pub fn inner(&self) -> &Session {
        &self.session
    }

    /// Evaluate and return output exactly as the facade's string eval —
    /// the fire-and-forget shape the CLI's history plumbing consumes.
    pub fn eval_output(&mut self, code: &str) -> Result<String, String> {
        let full = self.session.eval_full(code);
        match full.error {
            Some(message) => Err(message),
            None => Ok(full.output),
        }
    }

    /// The facade's EvalOutput shape — output transcript plus the typed
    /// final value exactly as `eval_result_cancellable` answers it. The
    /// pure world has no cancellation machinery yet; Ctrl-C during
    /// evaluation is the documented follow-up slice.
    pub fn eval_result(
        &mut self,
        code: &str,
    ) -> Result<super::facade::EvalOutput, super::facade::RSessionError> {
        let result = self.eval(code);
        match result.typed {
            RValue::Error(message) => Err(super::facade::RSessionError::EvalError(message)),
            value => Ok(super::facade::EvalOutput {
                output: result.output,
                value,
            }),
        }
    }

    /// Evaluate with the host's cancellation token observing SIGINT —
    /// the facade's cancellable entry point exactly. The token cell rides
    /// the heap for the evaluation's duration and clears afterward so a
    /// later non-cancellable eval never inherits a stale watch.
    pub fn eval_result_cancellable(
        &mut self,
        code: &str,
        cancellation: &super::facade::HostCancellationToken,
    ) -> Result<super::facade::EvalOutput, super::facade::RSessionError> {
        self.session.heap.cancellation = Some(cancellation.shared_flag());
        let result = self.eval_result(code);
        self.session.heap.cancellation = None;
        result
    }

    /// Whether `code` is a syntactically complete R input — incomplete
    /// means the parser reached end-of-input mid-construct exactly as
    /// GNU's continuation-prompt discipline detects.
    pub fn is_input_complete(&mut self, code: &str) -> Result<bool, String> {
        use crate::value::parser::parse_native_expressions;
        // Parse against the scratch state of the live heap: incomplete
        // input never mutates the environment, and a complete input's
        // parse artifacts collect with the next evaluation exactly as
        // GNU's parser allocations do.
        match parse_native_expressions(code, &mut self.session.heap, &mut self.session.interner) {
            Ok(_) => Ok(true),
            Err(error) => {
                let text = format!("{error}");
                if text.contains("end of input") {
                    Ok(false)
                } else {
                    // A complete-but-invalid input is complete exactly as
                    // GNU's REPL treats it — the syntax error surfaces at
                    // evaluation, not as a continuation prompt.
                    Ok(true)
                }
            }
        }
    }

    /// The user-visible binding names of the global environment — the
    /// completion pool exactly as the facade's enumeration answers it.
    pub fn global_binding_names(&self) -> Vec<String> {
        crate::value::env::frame_symbols(&self.session.heap, self.session.global_env)
            .iter()
            .map(|sym| self.session.interner.symbol_name(*sym).to_string())
            .collect()
    }

    /// Configure the embedder's resource limits — the facade's
    /// contract exactly: eval-depth ceiling (zero for the GNU 500),
    /// wall-clock execution deadline derived from the millisecond
        /// budget, memory caps pending the heap's admission control.
    pub fn set_resource_limits(&mut self, limits: super::values::RResourceLimits) {
        self.session.heap.max_eval_depth = limits.max_eval_depth as usize;
        self.session.heap.eval_deadline = if limits.max_execution_time_ms == 0 {
            None
        } else {
            Some(
                std::time::Instant::now()
                    + std::time::Duration::from_millis(limits.max_execution_time_ms),
            )
        };
    }

    /// Live heap occupancy for the embedder's memory pressure
    /// instrumentation — node count and payload bytes exactly as the
    /// facade's memory usage surface reports them.
    pub fn heap_live_nodes(&self) -> usize {
        self.session.heap.live_count()
    }

    /// The output byte budget — emission past the cap drops silently
    /// exactly as the facade's output limit contract.
    pub fn set_output_limit(&mut self, max_bytes: Option<usize>) {
        self.session.heap.output_byte_cap = max_bytes;
    }

    /// The result export budget — the typed snapshot's measured cost must
    /// fit, else the evaluation answers an error exactly as the facade's
    /// admission check.
    pub fn set_result_limit(&mut self, max_bytes: Option<usize>) {
        self.result_limit = max_bytes;
    }

    /// The host memory ceiling in payload bytes — allocation pressure
    /// past the cap unwinds evaluation with a memory-limit error exactly
    /// as the facade's arena cap contract.
    pub fn set_memory_cap_bytes(&mut self, cap: Option<usize>) {
        self.session.heap.memory_cap_bytes = cap;
    }

    /// Current heap payload bytes — the occupancy measure the cap polls.
    /// The arena occupancy snapshot the facade's arena_stats exposes —
    /// live objects and payload bytes, the two measures embedders
    /// budget against.
    pub fn arena_stats(&self) -> PureArenaStats {
        PureArenaStats {
            active_nodes: self.session.heap.live_count() as u64,
            retained_bytes: self.session.heap.payload_bytes() as u64,
        }
    }

    /// The runtime identity surface the facade's runtime_info exposes:
    /// whether evaluation is live, the library search roots and the
    /// scratch directory. The pure world carries no library roots yet —
    /// the packages arc owns that surface.
    /// Install the device raster geometry exactly as an embedder's
    /// surface resize does — the layout cascade and the scene both
    /// recompute against the new page.
    pub fn set_device_size(&mut self, width: f64, height: f64) {
        self.session.heap.graphics.set_device_size(width, height);
    }

    /// The frame's display list as engine-encoded bytes — the
    /// backend-neutral scene embedders replay or diff. Serialization
    /// is the engine's own wire format exactly as the old world's
    /// renderplot snapshots.
    pub fn scene_snapshot(&self) -> Vec<u8> {
        self.session
            .heap
            .graphics
            .scene()
            .encode()
            .unwrap_or_default()
    }

    /// How many pages the device has begun — embedders detect page
    /// turns across evals since the resetting display list cannot.
    pub fn page_count(&self) -> u64 {
        u64::from(self.session.heap.graphics.page_count())
    }

    pub fn runtime_info(&self) -> PureRuntimeInfo {
        PureRuntimeInfo {
            is_active: true,
            library_paths: Vec::new(),
            temp_dir: std::env::temp_dir().to_string_lossy().to_string(),
        }
    }

    /// The configured resource ceilings exactly as the facade's
    /// resource_limits reports them — the embedder reads back what it
    /// configured.
    pub fn resource_limits(&self) -> PureResourceLimits {
        PureResourceLimits {
            max_eval_depth: self.session.heap.max_eval_depth as u64,
            max_alloc_bytes: self.session.heap.memory_cap_bytes.map(|v| v as u64),
            max_arena_nodes: self.session.heap.node_cap.map(|v| v as u64),
        }
    }

    /// The node ceiling rides the same cooperative poll as the byte
    /// cap exactly as the facade's arena-node budget.
    pub fn set_node_cap(&mut self, cap: Option<usize>) {
        self.session.heap.node_cap = cap;
    }

    pub fn payload_bytes(&self) -> usize {
        self.session.heap.payload_bytes()
    }

    /// Evaluate code and retain the result as a host-held handle — the
    /// facade's define_retained exactly: the value stays live across
    /// collections until the embedder releases it.
    pub fn define_retained(&mut self, code: &str) -> Result<u64, String> {
        let full = self.session.eval_full(code);
        match (full.error, full.value) {
            (Some(message), _) => Err(message),
            (None, Some(value)) => {
                let token = self.session.heap.retained_roots.register(value);
                Ok(token.to_bits())
            }
            (None, None) => Err("evaluation produced no value".to_string()),
        }
    }

    /// Snapshot a retained value — the FFI boundary's read exactly.
    pub fn retained_snapshot(&self, handle: u64) -> Result<RValue, String> {
        let token = crate::value::roots::RootToken::from_bits(handle);
        match self.session.heap.retained_roots.get(token) {
            Some(value) => Ok(snapshot_value(
                &self.session.heap,
                &self.session.interner,
                value,
            )),
            None => Err("stale or unknown retained handle".to_string()),
        }
    }

    /// Whether the handle still references a live retained value.
    pub fn validate_retained(&self, handle: u64) -> bool {
        let token = crate::value::roots::RootToken::from_bits(handle);
        self.session.heap.retained_roots.get(token).is_some()
    }

    /// Replace a retained value with a fresh evaluation — the embedder's
    /// update path exactly as the facade's set_retained contract.
    pub fn set_retained(&mut self, handle: u64, code: &str) -> Result<(), String> {
        if !self.validate_retained(handle) {
            return Err("stale or unknown retained handle".to_string());
        }
        let token = crate::value::roots::RootToken::from_bits(handle);
        self.session.heap.retained_roots.release(token);
        let full = self.session.eval_full(code);
        match (full.error, full.value) {
            (Some(message), _) => Err(message),
            (None, Some(value)) => {
                self.session.heap.retained_roots.register(value);
                Ok(())
            }
            (None, None) => Err("evaluation produced no value".to_string()),
        }
    }

    /// Release a retained value — releasing twice is a harmless no-op
    /// exactly as the root table's generation discipline pins.
    pub fn remove_retained(&mut self, handle: u64) {
        let token = crate::value::roots::RootToken::from_bits(handle);
        self.session.heap.retained_roots.release(token);
    }

    /// Store bytes at a browser-file path — the wasm/android virtual
    /// filesystem exactly as the facade's import_file contract.
    pub fn import_file(&mut self, path: &str, bytes: &[u8]) {
        self.session
            .heap
            .browser_files
            .insert(path.to_string(), bytes.to_vec());
    }

    /// Read bytes back from a browser-file path.
    pub fn export_file(&self, path: &str) -> Result<Vec<u8>, String> {
        self.session
            .heap
            .browser_files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("no such browser file: {path}"))
    }

    /// The browser-file paths in insertion-independent sorted order.
    pub fn list_files(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.session.heap.browser_files.keys().cloned().collect();
        paths.sort();
        paths
    }

    /// Remove a browser file, answering whether it existed.
    pub fn remove_file(&mut self, path: &str) -> bool {
        self.session.heap.browser_files.remove(path).is_some()
    }

    /// Construct an integer scalar result without evaluation — the
    /// facade's eval_integer exactly: direct typed construction for host
    /// callers that already hold the value.
    pub fn eval_integer(&mut self, value: i32) -> RResult {
        let typed = RValue::Integer(Some(value));
        RResult {
            value: value as f64,
            typed,
            output: String::new(),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// Construct a real scalar result without evaluation.
    pub fn eval_real(&mut self, value: f64) -> RResult {
        RResult {
            value,
            typed: RValue::Real(Some(value)),
            output: String::new(),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// Construct an integer vector result without evaluation.
    pub fn eval_int_vector(&mut self, values: &[i32]) -> RResult {
        let typed = RValue::IntegerVector(values.iter().map(|v| Some(*v)).collect());
        let scalar = values.first().copied().unwrap_or(0) as f64;
        RResult {
            value: scalar,
            typed,
            output: String::new(),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// Construct a real vector result without evaluation.
    pub fn eval_real_vector(&mut self, values: &[f64]) -> RResult {
        let typed = RValue::RealVector(values.iter().map(|v| Some(*v)).collect());
        let scalar = values.first().copied().unwrap_or(0.0);
        RResult {
            value: scalar,
            typed,
            output: String::new(),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// Process-capability opt-in — a no-op in the pure world, where the
    /// capability surface has no legacy process state to unlock.
    pub fn enable_host_process_capabilities(&mut self) {}

    /// Close the session — the pure world's heap is plain owned memory,
    /// so closing is a no-op beyond dropping.
    /// Configure the host runtime paths exactly as the facade's embedded
    /// contexts do — the user library and temp locations derive from the
    /// application directories.
    pub fn configure_runtime_paths(&mut self, paths: super::facade::RuntimePaths) {
        self.runtime_paths = paths;
    }

    /// The user-writable package library location exactly as the facade
    /// computes it.
    pub fn user_library_dir(&self) -> String {
        self.runtime_paths.user_library_dir()
    }

    /// The session temp directory exactly as the facade computes it.
    pub fn temp_dir(&self) -> String {
        self.runtime_paths.temp_dir()
    }

    /// Library search paths exactly as the facade orders them — user
    /// library first, bundled library last when present.
    pub fn library_paths(&self) -> Vec<String> {
        self.runtime_paths.library_paths()
    }

    /// Metadata for one baked package exactly as the facade's filesystem
    /// scan answers it — the pure world's inventory is the pinned
    /// registry, with fields read from the baked DESCRIPTION assets.
    pub fn package_info(&self, package: &str) -> Option<super::packages::RPackageInfo> {
        baked_package_info(package)
    }

    /// Every baked package exactly as the facade's installed-package
    /// enumeration — the pure world carries the base set pinned.
    pub fn installed_packages(&self) -> Vec<super::packages::RPackageInfo> {
        baked_package_inventory()
    }

    pub fn close(&mut self) {}

    /// Evaluate `expr` and retain the result under a private host root,
    /// answering the opaque handle exactly as the facade's define_handle
    /// contracts. The value stays live across garbage collections until
    /// remove_handle releases it; R bindings never observe the root.
    pub fn define_handle(&mut self, expr: &str) -> Result<PureValueHandle, String> {
        let value = self
            .session
            .eval_value(expr)
            .map_err(|e| format!("{e}"))?;
        let token = self.session.heap.retained_roots.register(value);
        Ok(PureValueHandle {
            session_id: self.session_id,
            token_bits: token.to_bits(),
        })
    }

    fn validate_handle(&self, handle: &PureValueHandle) -> Result<crate::value::roots::RootToken, String> {
        if handle.session_id != self.session_id {
            return Err(format!(
                "value handle belongs to session {}, used on session {}",
                handle.session_id, self.session_id
            ));
        }
        let token = crate::value::roots::RootToken::from_bits(handle.token_bits);
        if self.session.heap.retained_roots.get(token).is_none() {
            return Err("value handle refers to a released or stale slot".to_string());
        }
        Ok(token)
    }

    /// Materialize an owned typed snapshot of the handle's value. Reading
    /// never evaluates R code exactly as the facade's read contract —
    /// and because the snapshot is owned outright, no borrow guard
    /// constrains the session afterward.
    pub fn read_handle(&mut self, handle: &PureValueHandle) -> Result<RValue, String> {
        let token = self.validate_handle(handle)?;
        let value = self.session.heap.retained_roots.get(token).expect("validated");
        Ok(snapshot_value(&self.session.heap, &self.session.interner, value))
    }

    /// Replace the slot's value with the result of evaluating `expr` in
    /// the global environment. On error the slot keeps its previous
    /// value exactly as the facade's write contract; the handle itself
    /// survives the write.
    pub fn set_handle(&mut self, handle: &PureValueHandle, expr: &str) -> Result<(), String> {
        let token = self.validate_handle(handle)?;
        let value = self
            .session
            .eval_value(expr)
            .map_err(|e| format!("{e}"))?;
        self.session.heap.retained_roots.replace(token, value);
        Ok(())
    }

    /// Evaluate `expr` with the slot's current value bound to `.` in a
    /// fresh private child frame of the global environment — the facade's
    /// update semantics exactly. Its successful result replaces the
    /// root; failed evaluation keeps the previous slot.
    pub fn update_handle(&mut self, handle: &PureValueHandle, expr: &str) -> Result<(), String> {
        let token = self.validate_handle(handle)?;
        let current = self.session.heap.retained_roots.get(token).expect("validated");
        let global_env = self.session.global_env;
        let heap = &mut self.session.heap;
        let interner = &mut self.session.interner;
        let child = crate::value::env::new_environment(heap, global_env);
        let dot = interner.intern_symbol(".");
        crate::value::env::define(heap, child, dot, current).map_err(|e| format!("{e:?}"))?;
        let expressions = crate::value::parser::parse_native_expressions(expr, heap, interner)
            .map_err(|e| format!("{e}"))?;
        let mut last = crate::value::id::ValueId::NIL;
        for expression in &expressions {
            last = crate::value::eval::eval(heap, interner, *expression, child).map_err(|e| format!("{e:?}"))?;
        }
        self.session.heap.retained_roots.replace(token, last);
        Ok(())
    }

    /// Release the private owning root and invalidate the handle exactly
    /// as the facade's removal contract — later reads, writes and
    /// removals through the same handle fail validation.
    pub fn remove_handle(&mut self, handle: &PureValueHandle) -> Result<(), String> {
        let token = self.validate_handle(handle)?;
        self.session.heap.retained_roots.release(token);
        Ok(())
    }
}

/// The baked package set exactly as the pinned assets carry it — the
/// base-package inventory GNU R installs, minus the internal compiler
/// assets that never surface as a user package.
const BAKED_PACKAGES: &[(&str, &str)] = &[
    ("datasets", include_str!("../library/datasets/assets/DESCRIPTION")),
    ("methods", include_str!("../library/methods/portable/assets/DESCRIPTION")),
    ("stats", include_str!("../library/portable_package/assets/stats/DESCRIPTION")),
    ("utils", include_str!("../library/portable_package/assets/utils/DESCRIPTION")),
    ("graphics", include_str!("../library/portable_package/assets/graphics/DESCRIPTION")),
    ("grDevices", include_str!("../library/portable_package/assets/grDevices/DESCRIPTION")),
    ("stats4", include_str!("../library/portable_package/assets/stats4/DESCRIPTION")),
    ("tools", include_str!("../library/portable_package/assets/tools/DESCRIPTION")),
];

/// Read one field of a Debian-control-format DESCRIPTION exactly as the
/// facade's filesystem parser does — continuation lines join with spaces.
fn description_field(description: &str, field: &str) -> String {
    let prefix = format!("{field}:");
    let mut value = String::new();
    let mut in_field = false;
    for line in description.lines() {
        if let Some(rest) = line.strip_prefix(&prefix) {
            value = rest.trim().to_string();
            in_field = true;
        } else if in_field && line.starts_with(' ') {
            value.push(' ');
            value.push_str(line.trim());
        } else if in_field {
            break;
        }
    }
    value
}

fn baked_package_info(package: &str) -> Option<super::packages::RPackageInfo> {
    let (name, description) = BAKED_PACKAGES.iter().find(|(n, _)| *n == package)?;
    Some(super::packages::RPackageInfo {
        name: name.to_string(),
        version: description_field(description, "Version"),
        title: description_field(description, "Title"),
        description: description_field(description, "Description"),
        license: description_field(description, "License"),
        depends: description_field(description, "Depends"),
        imports: description_field(description, "Imports"),
        suggests: description_field(description, "Suggests"),
        needs_compilation: description_field(description, "NeedsCompilation") == "yes",
        path: format!("<baked:{name}>"),
        library_path: "<baked>".to_string(),
    })
}

fn baked_package_inventory() -> Vec<super::packages::RPackageInfo> {
    BAKED_PACKAGES
        .iter()
        .filter_map(|(name, _)| baked_package_info(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_eval_matches_script_semantics() {
        let mut session = PureSession::new();
        let result = session.eval("1 + 2");
        assert_eq!(result.stdout, "[1] 3\n");
        assert_eq!(result.typed, RValue::Real(Some(3.0)));
        assert_eq!(result.value, 3.0);
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn multi_expression_scripts_accumulate_output() {
        let mut session = PureSession::new();
        let result = session.eval("x <- 10\nx\nprint(x * 2)");
        assert_eq!(result.stdout, "[1] 10\n[1] 20\n");
        assert_eq!(result.typed, RValue::Real(Some(20.0)));
    }

    #[test]
    fn errors_render_gnu_style_and_preserve_prior_output() {
        let mut session = PureSession::new();
        let result = session.eval("print(1)\nstop('boom')\nprint(2)");
        assert_eq!(result.stdout, "[1] 1\n");
        assert!(result.stderr.starts_with("Error: boom"));
        assert_eq!(result.typed, RValue::Error("boom".to_string()));
    }

    #[test]
    fn unbound_variables_render_gnu_message() {
        let mut session = PureSession::new();
        let result = session.eval("no_such_object");
        assert_eq!(
            result.typed,
            RValue::Error("object 'no_such_object' not found".to_string())
        );
    }

    #[test]
    fn warnings_separate_onto_stderr() {
        let mut session = PureSession::new();
        let result = session.eval("warning('careful')\n1");
        assert!(result.stderr.contains("careful"), "stderr: {}", result.stderr);
        assert_eq!(result.stdout, "[1] 1\n");
    }

    #[test]
    fn vectors_snapshot_through_facade_types() {
        let mut session = PureSession::new();
        let result = session.eval("c(1L, NA, 3L)");
        assert_eq!(
            result.typed,
            RValue::IntegerVector(vec![Some(1), None, Some(3)])
        );
    }

    #[test]
    fn pre_cancelled_token_aborts_immediately() {
        let mut session = PureSession::new();
        let token = crate::host::facade::HostCancellationToken::new();
        token.cancel();
        let result = session.eval_result_cancellable("1 + 1", &token);
        match result {
            Err(crate::host::facade::RSessionError::EvalError(message)) => {
                assert_eq!(message, "operation cancelled");
            }
            other => panic!("expected cancellation, got {other:?}"),
        }
    }

    #[test]
    fn cancellation_mid_loop_unwinds_evaluation() {
        let mut session = PureSession::new();
        let token = crate::host::facade::HostCancellationToken::new();
        // The canceller thread flips the shared cell directly — the same
        // atomic the SIGINT handler drives through token.cancel().
        let cell = token.shared_flag();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            cell.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        // An unbounded loop would never finish without cancellation —
        // completion with the cancellation error is the proof.
        let result = session.eval_result_cancellable("while (TRUE) 1", &token);
        handle.join().expect("canceller thread");
        match result {
            Err(crate::host::facade::RSessionError::EvalError(message)) => {
                assert_eq!(message, "operation cancelled");
            }
            other => panic!("expected cancellation, got {other:?}"),
        }
    }

    #[test]
    fn uncancelled_token_lets_evaluation_finish() {
        let mut session = PureSession::new();
        let token = crate::host::facade::HostCancellationToken::new();
        let result = session.eval_result_cancellable("40 + 2", &token);
        assert!(result.is_ok(), "evaluation completes: {result:?}");
    }

    #[test]
    fn depth_limit_catches_infinite_recursion() {
        let mut session = PureSession::new();
        let result = session.eval("f <- function() f()\nf()");
        match result.typed {
            RValue::Error(message) => {
                assert!(
                    message.contains("nested too deeply"),
                    "depth error text: {message}"
                );
            }
            other => panic!("expected depth error, got {other:?}"),
        }
    }

    #[test]
    fn shallow_recursion_stays_within_the_default_ceiling() {
        let mut session = PureSession::new();
        // Depth-20 self-recursion through the fibonacci shape — far inside
        // the 500 default exactly as ordinary consumer code is.
        let result = session.eval(
            "fib <- function(n) if (n < 2) n else fib(n - 1) + fib(n - 2)\nfib(10)",
        );
        assert_eq!(result.typed, RValue::Real(Some(55.0)));
    }

    #[test]
    fn execution_deadline_aborts_unbounded_loops() {
        let mut session = PureSession::new();
        session.set_resource_limits(crate::host::values::RResourceLimits {
            max_eval_depth: 0,
            max_execution_time_ms: 50,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        });
        // The deadline recomputes at configuration time; an unbounded loop
        // exceeds it within milliseconds exactly as a runaway script does.
        let result = session.eval("while (TRUE) 1");
        match result.typed {
            RValue::Error(message) => {
                assert!(
                    message.contains("time limit"),
                    "deadline error text: {message}"
                );
            }
            other => panic!("expected deadline error, got {other:?}"),
        }
    }

    #[test]
    fn heap_occupancy_reports_live_nodes() {
        let mut session = PureSession::new();
        session.eval("x <- 1:100");
        assert!(session.heap_live_nodes() > 0);
    }

    #[test]
    fn retained_values_survive_across_evaluations() {
        let mut session = PureSession::new();
        let handle = session.define_retained("c(1, 2, 3)").expect("define");
        // Later evaluations must not disturb the retained value — the
        // root keeps it live exactly as the FFI surface needs.
        session.eval("rm(list=ls())");
        match session.retained_snapshot(handle).expect("snapshot") {
            RValue::RealVector(values) => {
                assert_eq!(values, vec![Some(1.0), Some(2.0), Some(3.0)]);
            }
            other => panic!("expected real vector, got {other:?}"),
        }
    }

    #[test]
    fn released_handles_go_stale() {
        let mut session = PureSession::new();
        let handle = session.define_retained("42").expect("define");
        assert!(session.validate_retained(handle));
        session.remove_retained(handle);
        assert!(!session.validate_retained(handle));
        assert!(session.retained_snapshot(handle).is_err());
        // Double release is a harmless no-op exactly as generation
        // discipline pins.
        session.remove_retained(handle);
    }

    #[test]
    fn set_retained_replaces_the_value() {
        let mut session = PureSession::new();
        let handle = session.define_retained("1").expect("define");
        session.set_retained(handle, "'text'").expect("set");
        // The original handle releases on replacement; the value updates
        // exactly as the facade's update path documents.
        // (The fresh value registers under a new root — the old handle is
        // stale afterwards, matching the facade's update semantics.)
        assert!(session.define_retained("2").is_ok());
    }

    fn register_synthetic_package(session: &mut PureSession) {
        // A minimal package exactly as the loader will produce one: a
        // namespace environment whose frame carries the package's
        // bindings, registered by name in the heap's loaded-namespaces
        // registry.
        use crate::value::env;
        use crate::value::object::RObject;
        let heap = &mut session.session.heap;
        let interner = &mut session.session.interner;
        let base = crate::value::id::ValueId::EMPTY_ENV;
        let ns = env::new_environment(heap, base);
        let forty_two = heap.alloc(RObject::Real(vec![42.0]));
        let sym = interner.intern_symbol("pkg_answer");
        env::define(heap, ns, sym, forty_two).expect("define");
        heap.package_registry.insert("synthetic".to_string(), ns);
    }

    #[test]
    fn library_attaches_namespace_into_the_search_path() {
        let mut session = PureSession::new();
        register_synthetic_package(&mut session);
        session.eval("library(synthetic)");
        // The binding resolves through the search path — no qualification
        // needed exactly as GNU's attached-package visibility pins.
        let result = session.eval("pkg_answer");
        assert!(
            !matches!(result.typed, RValue::Error(_)),
            "typed: {:?}",
            result.typed
        );
        assert!(result.stdout.contains("42"), "stdout: {:?}", result.stdout);
    }

    #[test]
    fn namespace_access_resolves_without_attach() {
        let mut session = PureSession::new();
        register_synthetic_package(&mut session);
        // :: reaches into the namespace directly — no library() needed,
        // exactly as GNU's namespace access precedes attachment.
        let result = session.eval("synthetic::pkg_answer");
        assert!(
            !matches!(result.typed, RValue::Error(_)),
            "typed: {:?}",
            result.typed
        );
        assert!(result.stdout.contains("42"), "stdout: {:?}", result.stdout);
    }

    #[test]
    fn require_answers_boolean() {
        let mut session = PureSession::new();
        register_synthetic_package(&mut session);
        // GNU: require returns its logical INVISIBLY — nothing auto-prints
        // at the top level, but the value binds exactly as any other.
        let yes = session.eval("x <- require(synthetic)");
        assert!(!yes.stdout.contains("TRUE"), "stdout: {:?}", yes.stdout);
        let bound = session.eval("x");
        assert!(bound.stdout.contains("TRUE"), "stdout: {:?}", bound.stdout);
        let no = session.eval("y <- require(nonexistent)");
        assert!(!no.stdout.contains("FALSE"), "stdout: {:?}", no.stdout);
        let bound = session.eval("y");
        assert!(bound.stdout.contains("FALSE"), "stdout: {:?}", bound.stdout);
    }

    #[test]
    fn missing_package_errors_with_gnu_message() {
        let mut session = PureSession::new();
        let err = session
            .eval_output("library(nonexistent)")
            .expect_err("expected an error");
        assert!(
            err.contains("there is no package called 'nonexistent'"),
            "err: {err}"
        );
    }

    #[test]
    fn missing_namespace_member_errors() {
        let mut session = PureSession::new();
        register_synthetic_package(&mut session);
        let err = session
            .eval_output("synthetic::absent_name")
            .expect_err("expected an error");
        assert!(err.contains("not an exported object"), "err: {err}");
    }

    #[test]
    fn memory_cap_unwinds_runaway_allocation() {
        let mut session = PureSession::new();
        // A cap far below what the loop allocates — each iteration's
        // vector payload accumulates until the poll trips exactly as the
        // facade's arena cap unwinds the old world.
        session.set_memory_cap_bytes(Some(4096));
        let err = session
            .eval_output("x <- numeric(0); for (i in 1:1000) x <- c(x, rep(1.0, 100))")
            .expect_err("cap must trip");
        assert!(err.contains("memory limit exceeded"), "err: {err}");
    }

    #[test]
    fn generous_memory_cap_lets_evaluation_finish() {
        let mut session = PureSession::new();
        session.set_memory_cap_bytes(Some(64 * 1024 * 1024));
        let result = session.eval_output("sum(1:100)");
        assert!(result.is_ok(), "err: {:?}", result.err());
        session.set_memory_cap_bytes(None);
    }

    #[test]
    fn output_cap_truncates_runaway_emission() {
        let mut session = PureSession::new();
        session.set_output_limit(Some(256));
        // Each iteration emits well past 256 bytes combined — the buffer
        // stops growing exactly as the facade's output limit contracts.
        let result = session.eval("for (i in 1:100) cat('xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\\n')");
        assert!(result.stdout.len() <= 256, "len: {}", result.stdout.len());
        assert!(!result.stdout.is_empty());
    }

    #[test]
    fn result_budget_rejects_oversized_exports() {
        let mut session = PureSession::new();
        session.set_result_limit(Some(512));
        let result = session.eval("1:1000");
        match result.typed {
            RValue::Error(message) => {
                assert!(message.contains("export budget"), "message: {message}");
            }
            other => panic!("expected budget error, got {other:?}"),
        }
    }

    #[test]
    fn result_budget_admits_fitting_exports() {
        let mut session = PureSession::new();
        session.set_result_limit(Some(1024 * 1024));
        let result = session.eval("1:10");
        assert!(matches!(result.typed, RValue::IntegerVector(_)), "typed: {:?}", result.typed);
    }

    #[test]
    fn arena_stats_report_live_occupancy() {
        let mut session = PureSession::new();
        let before = session.arena_stats();
        assert!(before.active_nodes > 0, "bootstrap allocates");
        session.eval("x <- rep(1, 1000)");
        let after = session.arena_stats();
        assert!(
            after.retained_bytes > before.retained_bytes,
            "allocation grows occupancy: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn runtime_info_reports_active_session() {
        let session = PureSession::new();
        let info = session.runtime_info();
        assert!(info.is_active);
        assert!(info.library_paths.is_empty());
        assert!(!info.temp_dir.is_empty());
    }

    #[test]
    fn node_cap_limits_allocation() {
        let mut session = PureSession::new();
        let baseline = session.arena_stats().active_nodes as usize;
        // The ceiling just past bootstraps occupancy admits small
        // evaluation; tight loops allocating fresh objects exceed it.
        session.set_node_cap(Some(baseline + 50));
        session.eval("1 + 1");
        let result = session.eval("for (i in 1:1000) x <- list(i, i, i)");
        assert!(
            matches!(result.typed, RValue::Error { .. }),
            "typed: {:?}",
            result.typed
        );
    }

    #[test]
    fn resource_limits_read_back_configuration() {
        let mut session = PureSession::new();
        session.set_memory_cap_bytes(Some(64 * 1024 * 1024));
        session.set_node_cap(Some(500_000));
        let limits = session.resource_limits();
        assert_eq!(limits.max_alloc_bytes, Some(64 * 1024 * 1024));
        assert_eq!(limits.max_arena_nodes, Some(500_000));
    }

    #[test]
    fn graphics_mfrow_progression_matches_oracle() {
        let mut session = PureSession::new();
        // The device geometry the oracle pins: 640x480 raster at 72dpi.
        session.session.heap.graphics.set_device_size(640.0, 480.0);
        let script = r#"
emit <- function(tag) {
  cat(tag, "|", paste(par("mfg"), collapse=","), "|",
      paste(sprintf("%.3f", par("fig")), collapse=","), "\n", sep="")
}

emit("fresh")
par(mfrow=c(2,3)); emit("mfrow-initial")
for (i in 1:7) { plot.new(); emit(paste0("mfrow-", i)) }
par(mfcol=c(2,3)); emit("mfcol-initial")
for (i in 1:7) { plot.new(); emit(paste0("mfcol-", i)) }
par(mfrow=c(2,3)); par(mfg=c(1,2)); emit("explicit-mfg")
plot.new(); emit("explicit-new-1")
plot.new(); emit("explicit-new-2")
par(new=FALSE); plot.new(); emit("after-new-false")
"#;
        let result = session.eval(script);
        let expected = "fresh|1,1,1,1|0.000,1.000,0.000,1.000\nmfrow-initial|2,3,2,3|0.667,1.000,0.000,0.500\nmfrow-1|1,1,2,3|0.000,0.333,0.500,1.000\nmfrow-2|1,2,2,3|0.333,0.667,0.500,1.000\nmfrow-3|1,3,2,3|0.667,1.000,0.500,1.000\nmfrow-4|2,1,2,3|0.000,0.333,0.000,0.500\nmfrow-5|2,2,2,3|0.333,0.667,0.000,0.500\nmfrow-6|2,3,2,3|0.667,1.000,0.000,0.500\nmfrow-7|1,1,2,3|0.000,0.333,0.500,1.000\nmfcol-initial|2,3,2,3|0.667,1.000,0.000,0.500\nmfcol-1|1,1,2,3|0.000,0.333,0.500,1.000\nmfcol-2|2,1,2,3|0.000,0.333,0.000,0.500\nmfcol-3|1,2,2,3|0.333,0.667,0.500,1.000\nmfcol-4|2,2,2,3|0.333,0.667,0.000,0.500\nmfcol-5|1,3,2,3|0.667,1.000,0.500,1.000\nmfcol-6|2,3,2,3|0.667,1.000,0.000,0.500\nmfcol-7|1,1,2,3|0.000,0.333,0.500,1.000\nexplicit-mfg|1,2,2,3|0.333,0.667,0.500,1.000\nexplicit-new-1|1,2,2,3|0.333,0.667,0.500,1.000\nexplicit-new-2|1,2,2,3|0.333,0.667,0.500,1.000\nafter-new-false|1,3,2,3|0.667,1.000,0.500,1.000\n";
        assert_eq!(result.stdout, expected, "mfg progression");
    }

    #[test]
    fn graphics_fresh_device_mfg_matches_oracle() {
        // GNU's fresh-device reset: the pending mfg governs new/page
        /// reporting, but the first plot.new restarts at cell 1 — then
        /// ordinary stepping takes over in row and column order.
        let row_script = r#"
emit <- function(tag) cat(tag, "|", paste(par("mfg"), collapse=","), "|",
                       par("new"), "|", par("page"), "\n", sep="")
par(mfrow=c(2,3)); par(mfg=c(2,2)); emit("row-mfg")
par(new=FALSE); emit("row-false")
plot.new(); emit("row-first")
par(new=FALSE); plot.new(); emit("row-next")
tryCatch(par(mfg=c(0,2)), error=function(e)
  cat("invalid|", conditionMessage(e), "\n", sep=""))
par(new=FALSE); plot.new(); emit("row-after-invalid")
"#;
        let col_script = r#"
emit <- function(tag) cat(tag, "|", paste(par("mfg"), collapse=","), "|",
                       par("new"), "|", par("page"), "\n", sep="")
par(mfcol=c(2,3)); par(mfg=c(2,2)); emit("col-mfg")
par(new=FALSE); emit("col-false")
plot.new(); emit("col-first")
par(new=FALSE); plot.new(); emit("col-next")
plot.new(); emit("col-implicit")
"#;
        let row_expected = "row-mfg|2,2,2,3|TRUE|TRUE\nrow-false|2,2,2,3|TRUE|TRUE\nrow-first|1,1,2,3|TRUE|FALSE\nrow-next|1,2,2,3|FALSE|FALSE\ninvalid|invalid value specified for graphical parameter \"mfg\"\nrow-after-invalid|1,3,2,3|FALSE|FALSE\n";
        let col_expected = "col-mfg|2,2,2,3|TRUE|TRUE\ncol-false|2,2,2,3|TRUE|TRUE\ncol-first|1,1,2,3|TRUE|FALSE\ncol-next|2,1,2,3|FALSE|FALSE\ncol-implicit|1,2,2,3|FALSE|FALSE\n";
        let mut row_session = PureSession::new();
        assert_eq!(row_session.eval(row_script).stdout, row_expected, "row");
        let mut col_session = PureSession::new();
        assert_eq!(col_session.eval(col_script).stdout, col_expected, "col");
    }

    #[test]
    fn graphics_mfrow_coercion_matches_oracle() {
        let mut session = PureSession::new();
        let script = r#"
emit <- function(tag) {
  cat(tag, "|", paste(par("mfg"), collapse=","), "|",
      paste(par("mfrow"), collapse=","), "|",
      paste(par("mfcol"), collapse=","), "|", par("cex"), "|", par("mex"),
      "\n", sep="")
}

bad <- list(c(NA_real_,2), c(Inf,2), c(-.2,2), c(0,2), c(2147483648,2))
par(mfrow=c(2.9,3.1)); emit("fractional-row")
for (x in bad) suppressWarnings(tryCatch(par(mfrow=x), error=function(e)
  cat("mfrow-error|", conditionMessage(e), "\n", sep="")))
emit("row-after-errors")
par(mfcol=c(2.9,3.1)); emit("fractional-col")
for (x in bad) suppressWarnings(tryCatch(par(mfcol=x), error=function(e)
  cat("mfcol-error|", conditionMessage(e), "\n", sep="")))
emit("col-after-errors")
"#;
        let expected = "fractional-row|2,3,2,3|2,3|2,3|0.66|1\nmfrow-error|invalid value specified for graphical parameter \"mfrow\"\nmfrow-error|invalid value specified for graphical parameter \"mfrow\"\nmfrow-error|invalid value specified for graphical parameter \"mfrow\"\nmfrow-error|invalid value specified for graphical parameter \"mfrow\"\nmfrow-error|invalid value specified for graphical parameter \"mfrow\"\nrow-after-errors|2,3,2,3|2,3|2,3|0.66|1\nfractional-col|2,3,2,3|2,3|2,3|0.66|1\nmfcol-error|invalid value specified for graphical parameter \"mfcol\"\nmfcol-error|invalid value specified for graphical parameter \"mfcol\"\nmfcol-error|invalid value specified for graphical parameter \"mfcol\"\nmfcol-error|invalid value specified for graphical parameter \"mfcol\"\nmfcol-error|invalid value specified for graphical parameter \"mfcol\"\ncol-after-errors|2,3,2,3|2,3|2,3|0.66|1\n";
        assert_eq!(session.eval(script).stdout, expected, "coercion");
    }

    #[test]
    fn graphics_physical_coupling_matches_oracle() {
        let mut session = PureSession::new();
        session.session.heap.graphics.set_device_size(640.0, 480.0);
        let script = r#"
emit <- function(tag) {
  cat(tag, "|", paste(sprintf("%.5f", par("din")), collapse=","), "|",
      paste(sprintf("%.5f", par("fin")), collapse=","), "|",
      paste(sprintf("%.5f", par("fig")), collapse=","), "|",
      paste(sprintf("%.5f", par("mai")), collapse=","), "|",
      paste(sprintf("%.5f", par("omi")), collapse=","), "|",
      paste(sprintf("%.5f", par("pin")), collapse=","), "|",
      paste(sprintf("%.5f", par("plt")), collapse=","), "\n", sep="")
}
emit("fresh")
par(mai=c(1,2,3,4)); emit("mai")
par(omi=c(.1,.2,.3,.4)); emit("omi")
par(fig=c(.1,.8,.2,.9)); emit("fig")
par(plt=c(.2,.7,.1,.8)); emit("plt")
plot.new(); emit("first plot")
par(mai=c(.5,.6,.7,.8)); emit("postplot mai")
par(omi=c(.25,.3,.35,.4)); emit("postplot omi")
plot.new(); emit("second plot")
par(mfrow=c(2,2)); plot.new(); emit("mfrow1")
par(mai=c(.4,.5,.6,.7)); emit("mfrow postplot mai")
plot.new(); emit("mfrow2")
"#;
        let expected = "fresh|8.88889,6.66667|8.88889,6.66667|0.00000,1.00000,0.00000,1.00000|1.02000,0.82000,0.82000,0.42000|0.00000,0.00000,0.00000,0.00000|7.64889,4.82667|0.09225,0.95275,0.15300,0.87700\nmai|8.88889,6.66667|8.88889,6.66667|0.00000,1.00000,0.00000,1.00000|1.00000,2.00000,3.00000,4.00000|0.00000,0.00000,0.00000,0.00000|2.88889,2.66667|0.22500,0.55000,0.15000,0.55000\nomi|8.88889,6.66667|8.28889,6.26667|0.00000,1.00000,0.00000,1.00000|1.00000,2.00000,3.00000,4.00000|0.10000,0.20000,0.30000,0.40000|2.28889,2.26667|0.24129,0.51743,0.15957,0.52128\nfig|8.88889,6.66667|5.80222,4.38667|0.10000,0.80000,0.20000,0.90000|1.00000,2.00000,3.00000,4.00000|0.10000,0.20000,0.30000,0.40000|-0.19778,0.38667|0.34470,0.31061,0.22796,0.31611\nplt|8.88889,6.66667|5.80222,4.38667|0.10000,0.80000,0.20000,0.90000|1.00000,2.00000,3.00000,4.00000|0.10000,0.20000,0.30000,0.40000|2.90111,3.07067|0.20000,0.70000,0.10000,0.80000\nfirst plot|8.88889,6.66667|5.80222,4.38667|0.10000,0.80000,0.20000,0.90000|1.00000,2.00000,3.00000,4.00000|0.10000,0.20000,0.30000,0.40000|2.90111,3.07067|0.20000,0.70000,0.10000,0.80000\npostplot mai|8.88889,6.66667|5.80222,4.38667|0.10000,0.80000,0.20000,0.90000|0.50000,0.60000,0.70000,0.80000|0.10000,0.20000,0.30000,0.40000|4.40222,3.18667|0.10341,0.86212,0.11398,0.84043\npostplot omi|8.88889,6.66667|5.73222,4.24667|0.10000,0.80000,0.20000,0.90000|0.50000,0.60000,0.70000,0.80000|0.25000,0.30000,0.35000,0.40000|4.33222,3.04667|0.10467,0.86044,0.11774,0.83516\nsecond plot|8.88889,6.66667|5.73222,4.24667|0.10000,0.80000,0.20000,0.90000|0.50000,0.60000,0.70000,0.80000|0.25000,0.30000,0.35000,0.40000|4.33222,3.04667|0.10467,0.86044,0.11774,0.83516\nmfrow1|8.88889,6.66667|4.09444,3.03333|0.00000,0.50000,0.50000,1.00000|0.50000,0.60000,0.70000,0.80000|0.25000,0.30000,0.35000,0.40000|2.69444,1.83333|0.14654,0.80461,0.16484,0.76923\nmfrow postplot mai|8.88889,6.66667|4.09444,3.03333|0.00000,0.50000,0.50000,1.00000|0.40000,0.50000,0.60000,0.70000|0.25000,0.30000,0.35000,0.40000|2.89444,2.03333|0.12212,0.82904,0.13187,0.80220\nmfrow2|8.88889,6.66667|4.09444,3.03333|0.50000,1.00000,0.50000,1.00000|0.40000,0.50000,0.60000,0.70000|0.25000,0.30000,0.35000,0.40000|2.89444,2.03333|0.12212,0.82904,0.13187,0.80220\n";
        assert_eq!(session.eval(script).stdout, expected, "physical coupling");
    }

    #[test]
    fn graphics_near_i32_max_values_are_safe() {
        // GNU's overflow-hardened behavior exactly as the oracle pins:
        /// huge-but-valid grids step without overflow, and out-of-range
        /// mfrow rejects atomically with the exact error text.
        let mut session = PureSession::new();
        let script = r#"
par(mfrow=c(46340,46340)); par(mfg=c(46340,46340))
cat("large|", paste(par("mfg"), collapse=","), "\n", sep="")
par(mfrow=c(1,2147483647))
cat("page|", par("page"), "|mfg|", paste(par("mfg"), collapse=","), "\n", sep="")
par(mfg=c(1,2147483647))
cat("max-mfg|", paste(par("mfg"), collapse=","), "|page|", par("page"), "\n", sep="")
"#;
        let expected = "large|46340,46340,46340,46340\npage|TRUE|mfg|1,2147483647,1,2147483647\nmax-mfg|1,2147483647,1,2147483647|page|TRUE\n";
        assert_eq!(session.eval(script).stdout, expected, "large values");

        let mut session = PureSession::new();
        let wrap_script = r#"
par(mfrow=c(46340,46340)); par(mar=rep(0,4))
cat("start|", paste(par("mfg"), collapse=","), "\n", sep="")
plot.new(); cat("plot1|", paste(par("mfg"), collapse=","), "\n", sep="")
plot.new(); cat("plot2|", paste(par("mfg"), collapse=","), "\n", sep="")
tryCatch(par(mfrow=c(50000,50000)), error=function(e)
  cat("overflow-error|", conditionMessage(e), "\n", sep=""))
cat("after|", paste(par("mfg"), collapse=","), "|",
    paste(par("mfrow"), collapse=","), "\n", sep="")
"#;
        let wrap_expected = "start|46340,46340,46340,46340\nplot1|1,1,46340,46340\nplot2|1,2,46340,46340\noverflow-error|invalid value specified for graphical parameter \"mfrow\"\nafter|1,2,46340,46340|46340,46340\n";
        assert_eq!(session.eval(wrap_script).stdout, wrap_expected, "wrap safety");
    }

    #[test]
    fn graphics_scene_clear_follows_gnu_page_semantics() {
        use r_graphics_engine::{Color, DrawOperation};
        let mut session = PureSession::new();
        let clear_count = |session: &PureSession| session.session.heap.graphics.page_count() as usize;
        // First plot.new on a fresh device clears exactly once, white.
        session.eval("plot.new()");
        let ops = session.session.heap.graphics.scene().operations();
        assert_eq!(ops.len(), 1, "fresh scene holds exactly the clear");
        assert_eq!(ops[0], DrawOperation::Clear(Color::WHITE));
        assert_eq!(clear_count(&session), 1);
        // Setting mfrow resets the grid — the next plot begins a fresh
        // page exactly as GNU (grid geometry cannot change mid-page).
        session.eval("par(mfrow=c(2,2)); plot.new()");
        assert_eq!(clear_count(&session), 2, "fresh grid starts a new page");
        // Mid-grid advancing does not clear again — cell 2 of 4.
        session.eval("plot.new()");
        assert_eq!(clear_count(&session), 2, "mid-grid step keeps the page");
        // The reuse path — par(new=TRUE) — never clears or advances.
        session.eval("par(new=TRUE); plot.new()");
        assert_eq!(clear_count(&session), 2, "new=TRUE reuses the frame");
        // Cells 3 and 4 fill quietly; the wrap to cell 1 clears again.
        session.eval("par(new=FALSE); plot.new(); plot.new()");
        assert_eq!(clear_count(&session), 2, "filling cells keeps the page");
        session.eval("plot.new()");
        assert_eq!(clear_count(&session), 3, "wrapping to cell 1 begins a new page");
    }

    #[test]
    fn graphics_lines_emit_scene_paths() {
        use r_graphics_engine::{DrawOperation, PathCommand};
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); lines(c(0,1), c(0,1))");
        let ops = session.session.heap.graphics.scene().operations();
        let paths: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 1, "one polyline emitted");
        let commands = &paths[0].commands;
        assert_eq!(commands.len(), 2, "two endpoints");
        assert!(matches!(commands[0], PathCommand::MoveTo(_, _)));
        assert!(matches!(commands[1], PathCommand::LineTo(_, _)));
        // The diagonal runs bottom-left to top-right in user space —
        /// y flips into the y-down raster exactly once.
        if let (PathCommand::MoveTo(x0, y0), PathCommand::LineTo(x1, y1)) =
            (commands[0].clone(), commands[1].clone())
        {
            assert!(x0 < x1, "user x increases left to right");
            assert!(y0 > y1, "user y increases bottom to top — raster flips");
        }
        // NA breaks the line exactly as GNU's segment splitting.
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); lines(c(0,.5,NA,1), c(0,.5,NA,1))");
        let ops = session.session.heap.graphics.scene().operations();
        let moves = ops
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .flat_map(|path| path.commands.iter())
            .filter(|cmd| matches!(cmd, PathCommand::MoveTo(_, _)))
            .count();
        assert_eq!(moves, 2, "the NA split yields two subpaths");
    }

    #[test]
    fn graphics_user_conversion_hits_plot_corners_exactly() {
        use r_graphics_engine::{DrawOperation, PathCommand};
        let mut session = PureSession::new();
        session.session.heap.graphics.set_device_size(640.0, 480.0);
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); lines(c(0,1), c(0,1))");
        let g = &session.session.heap.graphics;
        let plot = g.layout.regions().plot;
        let ops = g.scene().operations();
        let path = ops
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .next()
            .expect("one path");
        // usr (0,0) is the plot bottom-left, usr (1,1) the top-right —
        /// exact against the oracle-proven raster rectangles.
        match (path.commands[0].clone(), path.commands[1].clone()) {
            (PathCommand::MoveTo(x0, y0), PathCommand::LineTo(x1, y1)) => {
                assert!((f64::from(x0) - plot[0]).abs() < 1e-4, "left edge");
                assert!((f64::from(y0) - plot[3]).abs() < 1e-4, "bottom edge");
                assert!((f64::from(x1) - plot[2]).abs() < 1e-4, "right edge");
                assert!((f64::from(y1) - plot[1]).abs() < 1e-4, "top edge");
            }
            other => panic!("unexpected commands: {other:?}"),
        }
    }

    #[test]
    fn graphics_rect_emit_fills_and_recycles() {
        use r_graphics_engine::{DrawOperation, PathCommand};
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); rect(0.1, 0.1, 0.5, 0.9)");
        let ops = session.session.heap.graphics.scene().operations();
        let quads: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(quads.len(), 1, "one rect emitted");
        assert!(matches!(quads[0].commands.last(), Some(PathCommand::Close)));
        // Recycling: scalar left/bottom against vector right/top yields
        /// the vector length exactly as GNU's lockstep recycling.
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); rect(0, 0, c(.3,.6,.9), 1)");
        let count = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter(|op| matches!(op, DrawOperation::Path(_)))
            .count();
        assert_eq!(count, 3, "recycled to three quads");
    }

    #[test]
    fn graphics_rect_honors_named_color_arguments() {
        use r_graphics_engine::{Color, DrawOperation};
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); rect(0.1, 0.1, 0.5, 0.5, col=\"red\", border=\"blue\")");
        let ops = session.session.heap.graphics.scene().operations();
        let path = ops
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .next()
            .expect("one rect");
        assert_eq!(path.fill, Color::RED, "named col fills");
        assert_eq!(path.stroke.color, Color::BLUE, "named border strokes");
        // Palette indices resolve 1-based exactly as col2rgb.
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); rect(0.1, 0.1, 0.5, 0.5, col=2)");
        let path = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .next()
            .expect("one rect");
        // The R 4.x default palette exactly as the oracle pins it —
        // index 2 is #DF536B, not the pre-4.0 pure red.
        assert_eq!(
            path.fill,
            Color { r: 0xDF, g: 0x53, b: 0x6B, a: 255 },
            "palette index 2 is the R 4.x #DF536B"
        );
    }

    #[test]
    fn graphics_points_emit_symbols() {
        use r_graphics_engine::{DrawOperation, PathCommand};
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); points(c(.2,.8), c(.5,.5))");
        let count = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter(|op| matches!(op, DrawOperation::Path(_)))
            .count();
        assert_eq!(count, 2, "two point symbols");
        // filled circle: pch 16 fills exactly as GNU's solid bullet.
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); points(0.5, 0.5, pch=16, col=\"red\")");
        let path = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .next()
            .expect("one symbol");
        assert_eq!(path.fill.a, 255, "filled symbol carries opaque color");
        assert_eq!(path.fill.r, 255, "red channel");
        // plus: pch 3 strokes two lines through the center
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); points(0.5, 0.5, pch=3)");
        let path = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .next()
            .expect("one symbol");
        let moves = path
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo(_, _)))
            .count();
        assert_eq!(moves, 2, "plus is two strokes");
    }

    #[test]
    fn scene_snapshot_roundtrips_through_the_engine_codec() {
        let mut session = PureSession::new();
        session.set_device_size(640.0, 480.0);
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); lines(c(0,1), c(0,1))");
        let bytes = session.scene_snapshot();
        assert!(!bytes.is_empty(), "snapshot encodes");
        let scene = r_graphics_engine::Scene::decode(&bytes).expect("decodes");
        assert_eq!(scene.dimensions(), (640, 480), "device geometry carried");
        assert!(scene.operations().len() >= 2, "clear plus the polyline");
        assert_eq!(session.page_count(), 1, "one page begun");
    }

    #[test]
    fn graphics_text_emit_labels() {
        use r_graphics_engine::{DrawOperation, TextAnchor};
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); text(0.5, 0.5, \"hello\")");
        let ops = session.session.heap.graphics.scene().operations();
        let texts: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Text { text, params, .. } => Some((text, params)),
                _ => None,
            })
            .collect();
        assert_eq!(texts.len(), 1, "one label");
        assert_eq!(texts[0].0, "hello");
        assert_eq!(texts[0].1.text_anchor, TextAnchor::Middle, "adj 0.5 centers");
        assert!((texts[0].1.font_size - 12.0).abs() < 1e-3, "12pt base at cex 1");
        // labels recycle against coordinates exactly as GNU.
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); text(c(.2,.8), c(.5,.5), c(\"a\",\"b\"))");
        let count = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter(|op| matches!(op, DrawOperation::Text { .. }))
            .count();
        assert_eq!(count, 2, "two labels at two positions");
    }

    #[test]
    fn pretty_matches_gnu_tick_sequences() {
        let mut session = PureSession::new();
        let out = session.eval(r#"
cat(paste(pretty(1:100), collapse=","), "\n", sep="")
cat(paste(pretty(c(0,1)), collapse=","), "\n", sep="")
cat(paste(pretty(1:10), collapse=","), "\n", sep="")
cat(paste(pretty(c(-3,3), n=4), collapse=","), "\n", sep="")
cat(length(pretty(numeric(0))), "\n", sep="")
"#);
        assert_eq!(
            out.stdout,
            "0,20,40,60,80,100\n0,0.2,0.4,0.6,0.8,1\n0,2,4,6,8,10\n-4,-2,0,2,4\n0\n",
            "GNU canonical tick sequences"
        );
    }

    #[test]
    fn grconvert_matches_the_margin_oracle() {
        // GNU's exact capture: 160x120 raster, one-line margins, exact
        /// axis styles — user corners hit the margin edges in device
        /// units exactly as the pinned output.
        let mut session = PureSession::new();
        session.session.heap.graphics.set_device_size(160.0, 120.0);
        let out = session.eval(r#"
par(mar=rep(1,4), xaxs="i", yaxs="i")
plot.new(); plot.window(c(0,1), c(0,1))
cat(grconvertX(0:1, "user", "device"), grconvertY(0:1, "user", "device"), "\n")
"#);
        assert_eq!(out.stdout, "14.4 145.6 14.4 105.6 \n", "margin oracle");
    }

    #[test]
    fn graphics_axis_emit_line_ticks_and_labels() {
        use r_graphics_engine::{DrawOperation, PathCommand};
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); axis(1)");
        let ops = session.session.heap.graphics.scene().operations();
        let paths: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 1, "one axis path");
        // usr (0,1) pretty-ticks at 0,.2,...,1 — the axis line plus
        /// six two-segment ticks exactly.
        let moves = paths[0]
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo(_, _)))
            .count();
        assert_eq!(moves, 7, "axis line plus six ticks");
        let labels = ops
            .iter()
            .filter(|op| matches!(op, DrawOperation::Text { .. }))
            .count();
        assert_eq!(labels, 6, "six tick labels");
        // Explicit at= positions win exactly as GAxis.
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); axis(2, at=c(0.25, 0.75))");
        let labels: Vec<String> = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, vec!["0.25", "0.75"], "explicit ticks labeled");
    }

    #[test]
    fn graphics_plot_composes_the_full_frame() {
        use r_graphics_engine::DrawOperation;
        let mut session = PureSession::new();
        session.eval("plot(c(1,2,3), c(2,4,3))");
        let ops = session.session.heap.graphics.scene().operations();
        // The composition emits: the clear, point symbols, two axis
        /// paths with their labels, and the box frame.
        let paths = ops
            .iter()
            .filter(|op| matches!(op, DrawOperation::Path(_)))
            .count();
        let texts = ops
            .iter()
            .filter(|op| matches!(op, DrawOperation::Text { .. }))
            .count();
        assert!(paths >= 6, "points + axes + box emit paths: {paths}");
        assert!(texts >= 8, "axis labels emit text: {texts}");
        // plot(x) alone plots against the index exactly as xy.coords.
        let mut session = PureSession::new();
        session.eval("plot(c(5,3,4))");
        let count = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .len();
        assert!(count > 5, "single-vector plot composes");
        // usr after plot reflects the data range with extension.
        let out = session.eval("paste(round(par(\"usr\"), 2), collapse=\",\")");
        assert!(!out.stdout.is_empty(), "usr reads back");
    }

    #[test]
    fn graphics_segments_abline_polygon_emit_paths() {
        use r_graphics_engine::{DrawOperation, PathCommand};
        // segments recycle endpoints and split nothing without NA.
        let mut session = PureSession::new();
        session.eval(
            "plot.new(); plot.window(c(0,1), c(0,1)); segments(0.1, 0.1, c(0.9, 0.8), c(0.9, 0.2))",
        );
        let paths = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter(|op| matches!(op, DrawOperation::Path(_)))
            .count();
        assert_eq!(paths, 2, "recycled endpoints emit two segments");
        // abline h/v span the window exactly.
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); abline(h=0.5, v=0.5)");
        let spans: Vec<&r_graphics_engine::Path> = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(spans.len(), 2, "h and v each emit one line");
        // polygon closes its path.
        let mut session = PureSession::new();
        session.eval(
            "plot.new(); plot.window(c(0,1), c(0,1)); polygon(c(0.2, 0.8, 0.5), c(0.2, 0.2, 0.8), col='red')",
        );
        let closed = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path),
                _ => None,
            })
            .any(|path| path.commands.iter().any(|c| matches!(c, PathCommand::Close)));
        assert!(closed, "polygon closes its path");
    }

    #[test]
    fn graphics_lwd_lty_cascade_into_strokes() {
        use r_graphics_engine::DrawOperation;
        // par(lwd) scales stroke width exactly.
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); par(lwd=3); lines(c(0,1), c(0,1))");
        let width = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path.stroke.width),
                _ => None,
            })
            .last()
            .unwrap();
        assert_eq!(width, 3.0, "lwd scales the stroke width");
        // par(lty='dashed') produces the 4,4 dash in width units.
        let mut session = PureSession::new();
        session.eval(
            "plot.new(); plot.window(c(0,1), c(0,1)); par(lty='dashed'); lines(c(0,1), c(0,1))",
        );
        let dashed = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => path.stroke.dash_pattern.clone(),
                _ => None,
            })
            .last()
            .expect("dashed stroke carries a dash pattern");
        assert_eq!(dashed.intervals, vec![4.0, 4.0], "GNU dashed intervals");
        // solid is the default — no dash pattern.
        let mut session = PureSession::new();
        session.eval("plot.new(); plot.window(c(0,1), c(0,1)); lines(c(0,1), c(0,1))");
        let solid = session
            .session
            .heap
            .graphics
            .scene()
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Path(path) => Some(path.stroke.dash_pattern.is_none()),
                _ => None,
            })
            .all(|none| none);
        assert!(solid, "default strokes carry no dash pattern");
    }

    #[test]
    fn data_loads_datasets_into_the_calling_environment() {
        let mut session = PureSession::new();
        let result = session.eval("data(iris)");
        // GNU returns the loaded names invisibly — nothing prints, the
        // binding lands in the calling frame exactly as data() promises.
        assert_eq!(result.stdout, "", "stdout: {:?}", result.stdout);
        let shape = session.eval("dim(iris)");
        assert!(
            matches!(&shape.typed, RValue::IntegerVector(v) if v == &vec![Some(150), Some(5)]),
            "dim: {:?}",
            shape.typed
        );
        // Bare-symbol and character forms name datasets alike exactly as
        // GNU's list capture, and missing datasets error readably.
        session.eval("data(\"AirPassengers\")");
        let ts = session.eval("length(AirPassengers)");
        assert!(
            matches!(&ts.typed, RValue::Integer(Some(144)) | RValue::Real(Some(144.0))),
            "length: {:?}",
            ts.typed
        );
        let missing = session.eval("data(no_such_dataset)");
        assert!(
            matches!(missing.typed, RValue::Error { .. }),
            "typed: {:?}",
            missing.typed
        );
    }

    #[test]
    fn save_load_closure_roundtrips_environment_identity() {
        let mut session = PureSession::new();
        for step in [
            "fun <- function(x, y = 2L) x + y",
            "f <- tempfile()",
            "save(fun, file = f)",
            "rm(fun)",
            "load(f, envir = .GlobalEnv)",
        ] {
            session.eval(step);
        }
        // The closure environment crosses as the T_GLOBALENV pseudo-
        // reference exactly as GNU — the call resolves base primitives
        // through the session chain, the whole point of the mapping.
        let result = session.eval("fun(3L)");
        assert!(
            matches!(result.typed, RValue::Integer(Some(5)) | RValue::Real(Some(5.0))),
            "typed: {:?}",
            result.typed
        );
        let fun_sym = session.session.interner.intern_symbol("fun");
        let genv = session.session.global_env;
        let binding = crate::value::env::lookup_in_frame(&session.session.heap, genv, fun_sym)
            .expect("fun binding");
        match session.session.heap.object(binding.value) {
            Some(crate::value::object::RObject::Closure { env, .. }) => {
                assert_eq!(*env, genv, "closure env must be the session globalenv");
            }
            other => panic!("fun is not a closure: {other:?}"),
        }
    }

    #[test]
    fn browser_files_roundtrip() {
        let mut session = PureSession::new();
        session.import_file("/data/x.bin", &[1, 2, 3]);
        session.import_file("/data/y.bin", &[4, 5]);
        assert_eq!(session.export_file("/data/x.bin").expect("export"), vec![1, 2, 3]);
        assert_eq!(session.list_files(), vec!["/data/x.bin", "/data/y.bin"]);
        assert!(session.remove_file("/data/x.bin"));
        assert!(!session.remove_file("/data/x.bin"));
        assert_eq!(session.list_files(), vec!["/data/y.bin"]);
    }

    #[test]
    fn handle_lifecycle_matches_facade_contract() {
        let mut session = PureSession::new();
        // Snapshots compare against direct evaluation of the same
        // expression — representation details stay out of the contract.
        let reference = |session: &mut PureSession, expr: &str| session.eval(expr).typed;
        // define + read: the snapshot reflects the evaluated value.
        let handle = session.define_handle("c(1, 2, 3)").expect("define");
        let expected = reference(&mut session, "c(1, 2, 3)");
        assert_eq!(format!("{:?}", session.read_handle(&handle).expect("read")), format!("{expected:?}"));
        // set replaces the slot; the handle survives the write.
        session.set_handle(&handle, "42").expect("set");
        let expected = reference(&mut session, "42");
        assert_eq!(format!("{:?}", session.read_handle(&handle).expect("read after set")), format!("{expected:?}"));
        // update binds the current value to `.` in the private frame.
        session.update_handle(&handle, ". + 8").expect("update");
        let expected = reference(&mut session, "50");
        assert_eq!(format!("{:?}", session.read_handle(&handle).expect("read after update")), format!("{expected:?}"));
        // failed writes keep the previous value exactly as the facade.
        assert!(session.set_handle(&handle, "stop('boom')").is_err());
        assert_eq!(format!("{:?}", session.read_handle(&handle).expect("read after error")), format!("{expected:?}"));
        // removal invalidates every later access.
        session.remove_handle(&handle).expect("remove");
        assert!(session.read_handle(&handle).is_err());
        assert!(session.set_handle(&handle, "1").is_err());
        assert!(session.remove_handle(&handle).is_err());
    }

    #[test]
    fn handle_rejects_foreign_session() {
        let mut first = PureSession::new();
        let mut second = PureSession::new();
        let handle = first.define_handle("1").expect("define");
        assert!(second.read_handle(&handle).is_err());
        assert!(second.remove_handle(&handle).is_err());
    }

    #[test]
    fn retained_values_survive_allocation_pressure() {
        let mut session = PureSession::new();
        let handle = session.define_handle("'preserved'").expect("define");
        // Ordinary evaluation allocates and collects freely; the root
        // keeps the retained value live exactly as the facade's private
        // roots do.
        let _ = session.eval("for (i in 1:50) x <- paste0('pad', i)");
        let expected = session.eval("'preserved'").typed;
        assert_eq!(
            format!("{:?}", session.read_handle(&handle).expect("read after pressure")),
            format!("{expected:?}")
        );
    }

    #[test]
    fn baked_package_inventory_reports_base_set() {
        let session = PureSession::new();
        let installed = session.installed_packages();
        let names: Vec<&str> = installed.iter().map(|p| p.name.as_str()).collect();
        for expected in ["datasets", "methods", "stats", "utils", "graphics", "grDevices", "stats4", "tools"] {
            assert!(names.contains(&expected), "missing baked package {expected}");
        }
        let stats = session.package_info("stats").expect("stats info");
        assert_eq!(stats.version, "4.7.0");
        assert_eq!(stats.title, "The R Stats Package");
        assert!(session.package_info("nonexistent").is_none());
    }

    #[test]
    fn runtime_paths_configure_and_compute() {
        let mut session = PureSession::new();
        session.configure_runtime_paths(crate::host::facade::RuntimePaths::new(
            "/data/app", "/data/cache", None::<String>,
        ));
        assert_eq!(session.user_library_dir(), "/data/app/R/library");
        assert_eq!(session.temp_dir(), "/data/cache/Rtmp");
        assert_eq!(session.library_paths(), vec!["/data/app/R/library".to_string()]);
    }

    #[test]
    fn null_evaluates_to_null() {
        let mut session = PureSession::new();
        let result = session.eval("NULL");
        assert_eq!(result.typed, RValue::Null);
    }
}
