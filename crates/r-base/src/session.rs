//! The productized new-world evaluation session: parse → eval → print as
//! one self-contained pipeline with zero legacy-world involvement.
//!
//! `Session` owns the generational heap, the symbol interner, and the
//! bootstrapped global environment; `eval_script` reproduces GNU's script
//! semantics exactly — every visible top-level result auto-prints, NULL
//! never prints, deferred warnings flush after each expression's output
//! exactly as the REPL end-of-task discipline pins.

use r_core::env;
use r_core::eval::{eval, EvalError, RSignal};
use r_core::heap::Heap;
use r_core::id::ValueId;
use r_core::intern::SymbolInterner;
use r_core::object::{BuiltinId, RObject};
use r_core::parser::{parse_native_expressions, ParseError};

/// A script evaluation failure — parse errors surface upstream-rendered,
/// evaluation errors as the evaluator's structured conditions.
#[derive(Debug)]
pub enum SessionError {
    Parse(ParseError),
    Eval(RSignal),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Parse(error) => write!(f, "{error}"),
            SessionError::Eval(error) => write!(f, "{error:?}"),
        }
    }
}

/// The new-world interpreter session — the unit of runtime embedding the
/// host facade migrates onto.
pub struct Session {
    pub heap: Heap,
    pub interner: SymbolInterner,
    /// The user workspace — evaluation environment for every top-level
    /// expression, enclosed by the primitive-carrying base environment.
    pub global_env: ValueId,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        let mut heap = Heap::new();
        // The dispatch inversion exactly as the DAG requires: eval in
        // r-core applies primitives without a static reference upward.
        heap.primitive_dispatcher = Some(crate::primitives::call_primitive);
        let mut interner = SymbolInterner::new();
        let global_env = bootstrap_env(&mut heap, &mut interner);
        Session {
            heap,
            interner,
            global_env,
        }
    }

    /// Evaluate R source with GNU's exact script contract: expressions
    /// Boundary maintenance after one top-level expression — the safe
    /// point where no evaluator locals exist, so the root set is exactly
    /// the session's reachable state. Collection runs when pressured or
    /// whenever admission caps are configured; the sticky budget flags
    /// then decide whether the expression answered over budget. The
    /// flags clear here so one transgression poisons exactly one
    /// expression, never the session.
    fn boundary_maintenance(
        &mut self,
        value: ValueId,
        pending: &[ValueId],
    ) -> Result<(), String> {
        // An error that reached the top level leaves no raising call
        // behind — the channel belongs to the error in flight only.
        self.heap.error_call = r_core::id::ValueId::NIL;
        let pressured = self.heap.gc_pressure() || self.heap.gc_requested;
        self.heap.gc_requested = false;
        let capped = self.heap.memory_cap_bytes.is_some() || self.heap.node_cap.is_some();
        let memory_tripped = self.heap.memory_cap_tripped;
        let nodes_tripped = self.heap.node_cap_tripped;
        self.heap.memory_cap_tripped = false;
        self.heap.node_cap_tripped = false;
        if pressured || capped {
            let mut extra: Vec<ValueId> = Vec::new();
            extra.push(self.global_env);
            // The value roots only when it survives — a budget-tripped
            // expression discards its result, and rooting it here would
            // defeat the very reclaim recovery depends on.
            if !memory_tripped && !nodes_tripped {
                extra.push(value);
            }
            if let Some(id) = self.heap.global_env_id {
                extra.push(id);
            }
            if let Some(id) = self.heap.base_env_id {
                extra.push(id);
            }
            extra.extend(self.heap.package_registry.values().copied());
            for (key, handlers) in &self.heap.exit_handlers {
                extra.push(*key);
                extra.extend(handlers.iter().copied());
            }
            extra.extend(self.heap.s3_dispatch.iter().map(|(_, method, _)| *method));
            extra.extend(self.heap.debugged.iter().copied());
            extra.extend(self.heap.retained_roots.roots());
            // Parsed-but-unevaluated expressions are live roots held in
            // the caller's Rust local — invisible to the collector, so
            // the boundary must preserve them explicitly or the rest of
            // the script is freed mid-evaluation.
            extra.extend(pending.iter().copied());
            let stack = r_core::roots::RootStack::new();
            let table = r_core::roots::RootTable::default();
            let roots = r_core::gc::RootSet {
                stack: &stack,
                table: &table,
                extra: &extra,
            };
            r_core::gc::collect(&mut self.heap, &roots);
        }
        if memory_tripped {
            return Err("memory limit exceeded".to_string());
        }
        if nodes_tripped {
            return Err("arena node limit exceeded".to_string());
        }
        Ok(())
    }

    /// evaluate in order against the shared global environment, visible
    /// results auto-print, and deferred warnings flush after each
    /// expression's output. The first error aborts the remaining script.
    /// Returns the accumulated standard output.
    pub fn eval_script(&mut self, source: &str) -> Result<String, SessionError> {
        let expressions = parse_native_expressions(source, &mut self.heap, &mut self.interner)
            .map_err(SessionError::Parse)?;
        let tokens: Vec<_> = expressions
            .iter()
            .map(|expr| self.heap.retained_roots.register(*expr))
            .collect();
        let result = self.eval_script_loop(&expressions);
        for token in tokens {
            self.heap.retained_roots.release(token);
        }
        return result;
    }

    fn eval_script_loop(&mut self, expressions: &[ValueId]) -> Result<String, SessionError> {
        for expr in expressions {
            let value = match eval(&mut self.heap, &mut self.interner, *expr, self.global_env) {
                Ok(value) => value,
                Err(signal) => {
                    // The error path still collects — reclaiming the
                    // unwound garbage is what lets the next expression
                    // admit again after a budget failure.
                    let _ = self.boundary_maintenance(ValueId::NIL, expressions);
                    return Err(SessionError::Eval(signal));
                }
            };
            if let Err(text) = self.boundary_maintenance(value, expressions) {
                return Err(SessionError::Eval(RSignal::from(EvalError::Condition(text))));
            }
            // Primitive writes (cat/print/writeLines) and auto-printed
            // values interleave in evaluation order exactly as GNU's
            // stdout does — both append to the one output buffer.
            if self.heap.visible {
                let text = r_core::print::format_value(&self.heap, &self.interner, value);
                self.heap.emit(&text);
                self.heap.emit("\n");
            }
            // Deferred warnings flush at end-of-task exactly as GNU's
            // REPL — after the visible result prints.
            if !self.heap.pending_warnings.is_empty() {
                let pending = std::mem::take(&mut self.heap.pending_warnings);
                if pending.len() == 1 {
                    self.heap.output.push_str("Warning message:\n");
                    self.heap.output.push_str(&pending[0]);
                    self.heap.emit("\n");
                } else {
                    self.heap.output.push_str("Warning messages:\n");
                    for (i, text) in pending.iter().enumerate() {
                        self.heap.output.push_str(&format!("{}: {}\n", i + 1, text));
                    }
                }
            }
        }
        Ok(std::mem::take(&mut self.heap.output))
    }

    /// Evaluate R source and return the final expression's value handle —
    /// the host facade's `eval_result` entry point. Output accumulation
    /// follows the script contract exactly as `eval_script`; the caller
    /// reads the heap state directly for the value snapshot.
    pub fn eval_value(&mut self, source: &str) -> Result<ValueId, SessionError> {
        let expressions = parse_native_expressions(source, &mut self.heap, &mut self.interner)
            .map_err(SessionError::Parse)?;
        let mut last = ValueId::NIL;
        for expr in &expressions {
            last = eval(&mut self.heap, &mut self.interner, *expr, self.global_env)
                .map_err(SessionError::Eval)?;
            if let Err(text) = self.boundary_maintenance(last, &expressions) {
                return Err(SessionError::Eval(RSignal::from(EvalError::Condition(text))));
            }
        }
        Ok(last)
    }

    /// Evaluate R source into the comprehensive result the host facade
    /// consumes: per-expression output chunks join stdout exactly as GNU's
    /// Rscript, deferred warnings and the final error render onto stderr,
    /// and both streams interleave chronologically in `output`. The first
    /// error aborts the remaining script with output-so-far preserved.
    pub fn eval_full(&mut self, source: &str) -> FullEval {
        self.heap.output_bytes_emitted = 0;
        self.heap.output_capture_truncated = false;
        let mut full = FullEval::default();
        let expressions = match parse_native_expressions(source, &mut self.heap, &mut self.interner) {
            Ok(expressions) => expressions,
            Err(error) => {
                let text = format!("{error}");
                full.stderr.push_str(&format!("Error: {text}\n"));
                full.output.push_str(&format!("Error: {text}\n"));
                full.error = Some(text);
                return full;
            }
        };
        // Parsed-but-unevaluated expressions must survive collections
        // triggered mid-evaluation — user-invoked gc() has no knowledge of
        // this loop's Rust locals, so the expressions register as retained
        // roots for the script's duration exactly as host handles do.
        let tokens: Vec<_> = expressions
            .iter()
            .map(|expr| self.heap.retained_roots.register(*expr))
            .collect();
        for expr in &expressions {
            match eval(&mut self.heap, &mut self.interner, *expr, self.global_env) {
                Ok(value) => {
                    if let Err(text) = self.boundary_maintenance(value, &expressions) {
                        let chunk = std::mem::take(&mut self.heap.output);
                        full.stdout.push_str(&chunk);
                        full.output.push_str(&chunk);
                        let line = format!("Error: {text}\n");
                        full.stderr.push_str(&line);
                        full.output.push_str(&line);
                        full.error = Some(text);
                        for token in tokens {
                            self.heap.retained_roots.release(token);
                        }
                        return full;
                    }
                    // Primitive writes accumulate in the output buffer; the
                    // auto-printed value joins them before the chunk is
                    // lifted — the interleave is evaluation order exactly.
                    if self.heap.visible {
                        let text = r_core::print::format_value(&self.heap, &self.interner, value);
                        self.heap.emit(&text);
                        self.heap.emit("\n");
                    }
                    let chunk = std::mem::take(&mut self.heap.output);
                    full.stdout.push_str(&chunk);
                    full.output.push_str(&chunk);
                    if !self.heap.pending_warnings.is_empty() {
                        let pending = std::mem::take(&mut self.heap.pending_warnings);
                        let mut block = String::new();
                        if pending.len() == 1 {
                            block.push_str("Warning message:\n");
                            block.push_str(&pending[0]);
                            block.push('\n');
                        } else {
                            block.push_str("Warning messages:\n");
                            for (i, text) in pending.iter().enumerate() {
                                block.push_str(&format!("{}: {}\n", i + 1, text));
                            }
                        }
                        full.stderr.push_str(&block);
                        full.output.push_str(&block);
                    }
                    full.value = Some(value);
                    full.visible = self.heap.visible;
                }
                Err(signal) => {
                    // Collection still runs — the unwound garbage must
                    // leave the admission measure before the next eval.
                    let _ = self.boundary_maintenance(ValueId::NIL, &expressions);
                    let chunk = std::mem::take(&mut self.heap.output);
                    full.stdout.push_str(&chunk);
                    full.output.push_str(&chunk);
                    let text = render_eval_error(&signal);
                    let line = format!("Error: {text}\n");
                    full.stderr.push_str(&line);
                    full.output.push_str(&line);
                    full.error = Some(text);
                    for token in tokens {
                        self.heap.retained_roots.release(token);
                    }
                    return full;
                }
            }
        }
        for token in tokens {
            self.heap.retained_roots.release(token);
        }
        if self.heap.output_capture_truncated {
            // The capture marker joins stdout and the transcript exactly
            // as the facade appends it — output the runtime dropped on
            // the embedder's behalf is announced, never silent.
            let marker = "\n[captured console output truncated by runtime limit]";
            full.stdout.push_str(marker);
            full.output.push_str(marker);
        }
        full
    }
}

/// The comprehensive evaluation result the host facade consumes —
/// separated stdout/stderr streams, the chronological interleave, the
/// final value handle, and the rendered error when evaluation failed.
#[derive(Debug, Default)]
pub struct FullEval {
    /// print/cat/auto-print output — Rscript's stdout exactly.
    pub stdout: String,
    /// Warning and error rendering — Rscript's stderr exactly.
    pub stderr: String,
    /// Every emission in chronological order — the console transcript.
    pub output: String,
    /// The final expression's value when evaluation completed.
    pub value: Option<ValueId>,
    /// Whether the final expression answered visibly — export admission
    /// applies to visible results only, since an invisible value crosses
    /// the boundary as nothing the host was meant to inspect.
    pub visible: bool,
    /// GNU-rendered error text when evaluation aborted.
    pub error: Option<String>,
}

/// GNU's error text for one evaluation failure — the message body without
/// the "Error" prefix, exactly as `stop()` users and the runtime produce it.
pub fn render_eval_error(signal: &RSignal) -> String {
    match signal {
        RSignal::Error(r_core::eval::EvalError::UnboundVariable(name)) => {
            format!("object '{name}' not found")
        }
        RSignal::Error(r_core::eval::EvalError::NotCallable) => {
            "attempt to apply non-function".to_string()
        }
        RSignal::Error(r_core::eval::EvalError::MissingArgument(name)) => {
            format!("argument \"{name}\" is missing, with no default")
        }
        RSignal::Error(r_core::eval::EvalError::RecursiveForcing) => {
            "promise already under evaluation: recursive default argument reference or earlier problems?"
                .to_string()
        }
        RSignal::Error(r_core::eval::EvalError::UnsupportedForm(form)) => {
            format!("unsupported form: {form}")
        }
        RSignal::Error(r_core::eval::EvalError::TypeError(message)) => {
            message.to_string()
        }
        RSignal::Error(r_core::eval::EvalError::Condition(message)) => message.clone(),
        // Control-flow signals reaching top level are evaluator bugs in
        // disguise; surface them readably rather than panicking.
        other => format!("{other:?}"),
    }
}

/// GNU's environment structure: primitives live in a base environment
/// exactly as package:base; user code evaluates in a globalenv whose
/// enclosure chain reaches base — ls() sees only user bindings and
/// definitions never pollute the base frame.
pub fn bootstrap_env(heap: &mut Heap, interner: &mut SymbolInterner) -> ValueId {
    use crate::primitives::primitive_id_for_name;
    let base = env::new_environment(heap, ValueId::EMPTY_ENV);
    for &name in crate::primitives::primitive_names() {
        let sym = interner.intern_symbol(name);
        let prim = primitive_id_for_name(name).expect("mapped primitive");
        let object = heap.alloc(RObject::Primitive(BuiltinId(prim)));
        env::define(heap, base, sym, object).expect("define primitive");
    }
    // GNU's reserved typed-NA constants — bindings in base exactly as
    // GNU's base namespace carries them.
    let na_int = heap.alloc(RObject::Integer(vec![r_core::object::NA_INTEGER]));
    let sym = interner.intern_symbol("NA_integer_");
    env::define(heap, base, sym, na_int).expect("NA_integer_");
    let na_real = heap.alloc(RObject::Real(vec![f64::from_bits(r_core::object::NA_REAL_BITS)]));
    let sym = interner.intern_symbol("NA_real_");
    env::define(heap, base, sym, na_real).expect("NA_real_");
    let pi_val = heap.alloc(RObject::Real(vec![std::f64::consts::PI]));
    let sym = interner.intern_symbol("pi");
    env::define(heap, base, sym, pi_val).expect("pi");
    // letters and LETTERS — GNU's base constants exactly.
    let lower: Vec<_> = (b'a'..=b'z')
        .map(|c| interner.intern_string(&(c as char).to_string()))
        .collect();
    let letters_val = heap.alloc(RObject::String(lower));
    let sym = interner.intern_symbol("letters");
    env::define(heap, base, sym, letters_val).expect("letters");
    let upper: Vec<_> = (b'A'..=b'Z')
        .map(|c| interner.intern_string(&(c as char).to_string()))
        .collect();
    let upper_val = heap.alloc(RObject::String(upper));
    let sym = interner.intern_symbol("LETTERS");
    env::define(heap, base, sym, upper_val).expect("LETTERS");
    // .GlobalEnv resolves to the user environment itself — the global
    // exactly as GNU's top-level reference.
    let user = env::new_environment(heap, base);
    // The serialization codec recognizes both identities — closures
    // capture them by value and the wire carries them as pseudo-
    // references exactly as GNU's globalenv/baseenv items.
    heap.global_env_id = Some(user);
    heap.base_env_id = Some(base);
    let genv_sym = interner.intern_symbol(".GlobalEnv");
    env::define(heap, base, genv_sym, user).expect(".GlobalEnv");
    // The global environment carries its printed name — GNU's
    // R_GlobalEnv exactly — through the slot attribute chain.
    {
        let name_sym = interner.intern_symbol("name");
        let name_val = heap.alloc(RObject::String(vec![interner.intern_string("R_GlobalEnv")]));
        let tag_obj = heap.alloc(RObject::Symbol {
            name: name_sym,
            value: ValueId::UNBOUND,
            internal: ValueId::NIL,
        });
        let chain = heap.alloc(RObject::Pairlist {
            car: name_val,
            cdr: ValueId::NIL,
            tag: tag_obj,
        });
        if let Some(slot) = heap.get_mut(user) {
            slot.attrib = chain;
        }
    }
    // version and R.version share one list object — identical() over
    // them is TRUE exactly as GNU's aliased constants.
    {
        let major = heap.alloc(RObject::String(vec![interner.intern_string("4")]));
        let minor = heap.alloc(RObject::String(vec![interner.intern_string("4.0")]));
        let version_list = heap.alloc(RObject::List(vec![major, minor]));
        let vsym = interner.intern_symbol("version");
        env::define(heap, base, vsym, version_list).expect("version");
        let rsym = interner.intern_symbol("R.version");
        env::define(heap, base, rsym, version_list).expect("R.version");
        let version_string = heap.alloc(RObject::String(vec![interner.intern_string(
            "R version 4.4.0 (rport)",
        )]));
        let vssym = interner.intern_symbol("R.version.string");
        env::define(heap, base, vssym, version_string).expect("R.version.string");
    }
    let na_str = heap.alloc(RObject::String(vec![interner.intern_string("\0NA")]));
    let sym = interner.intern_symbol("NA_character_");
    env::define(heap, base, sym, na_str).expect("NA_character_");    register_baked_packages(heap, interner, base);
    attach_default_packages(heap, interner, base, user);
    populate_rlevel_namespaces(heap, interner);
    user
}

/// GNU populates package namespaces with R-level closures defined in the
/// package's own sources — modifyList, most of stats modeling, the print
/// methods. The pure world evaluates the pinned per-package R sources into
/// their namespace environments exactly as GNU's lazy-load installation
/// does: each definition binds in the namespace and closures capture it as
/// their enclosure, so namespace-internal references resolve namespace-first
/// exactly as GNU. Evaluation is best-effort at bootstrap — a definition
/// whose primitives are unimplemented stays absent, mirroring the
/// unimplemented-export surface exactly as the primitive remainder does.
fn populate_rlevel_namespaces(heap: &mut Heap, interner: &mut SymbolInterner) {
    use r_core::eval::eval;
    use r_core::parser::parse_native_expressions;
    // Base functions install into the base environment itself exactly as
    // GNU's base R sources do; package functions install into their
    // namespace environments.
    for &(package, source) in crate::assets::RLEVEL_SOURCES {
        let target = if package == "base" {
            heap.base_env_id
        } else {
            heap.package_registry.get(package).copied()
        };
        let Some(namespace) = target else {
            continue;
        };
        let Ok(expressions) = parse_native_expressions(source, heap, interner) else {
            continue;
        };
        for expr in &expressions {
            let _ = eval(heap, interner, *expr, namespace);
        }
    }
}

/// The pinned baked-package export manifests, decoded and resolved at
/// bootstrap: every package namespace binds the exports the pinned
/// metadata declares, resolved against the base environment's bindings
/// exactly as GNU's namespace population links package code — the pure
/// world carries the implementations as base primitives, so population
/// is a name-linking pass with no code loading. Names without a base
/// binding belong to the unimplemented remainder and stay absent until
/// the primitive lands, exactly mirroring GNU's undefined-export error
/// surface at use time rather than at attach time.
fn register_baked_packages(heap: &mut Heap, interner: &mut SymbolInterner, base: ValueId) {
    use std::io::Read;
    for &(package, gzipped) in crate::assets::PACKAGE_EXPORTS {
        let namespace = env::new_environment(heap, base);
        // The attached-package name exactly as GNU's search() reports —
        // library() splices this very environment into the chain.
        {
            let name_sym = interner.intern_symbol("name");
            let name_val = heap.alloc(RObject::String(vec![
                interner.intern_string(&format!("package:{package}")),
            ]));
            let tag_obj = heap.alloc(RObject::Symbol {
                name: name_sym,
                value: ValueId::UNBOUND,
                internal: ValueId::NIL,
            });
            let chain = heap.alloc(RObject::Pairlist {
                car: name_val,
                cdr: ValueId::NIL,
                tag: tag_obj,
            });
            if let Some(slot) = heap.get_mut(namespace) {
                slot.attrib = chain;
            }
        }
        let mut plain = Vec::new();
        if flate2::read::GzDecoder::new(gzipped).read_to_end(&mut plain).is_ok() {
            if let Some(manifest) = r_core::rbg1::decode_rds(heap, interner, &plain, base) {
                let names: Vec<String> = match heap.object(manifest) {
                    Some(RObject::String(entries)) => entries
                        .iter()
                        .map(|entry| interner.string_text(*entry).to_string())
                        .collect(),
                    _ => Vec::new(),
                };
                for export in names {
                    let sym = interner.intern_symbol(&export);
                    if let Some(binding) = env::lookup_in_frame(heap, base, sym) {
                        let _ = env::define(heap, namespace, sym, binding.value);
                    }
                }
            }
        }
        heap.package_registry.insert(package.to_string(), namespace);
    }
    // base resolves through itself — every primitive is already there —
    // and datasets carries its contents through data() rather than
    // exports, so an empty namespace satisfies library(datasets).
    heap.package_registry.insert("base".to_string(), base);
    let datasets = env::new_environment(heap, base);
    name_environment(heap, interner, datasets, "package:datasets");
    heap.package_registry.insert("datasets".to_string(), datasets);
}

/// Attach the printed name exactly as GNU's environment naming — the
/// attribute chain search() reads when reporting the path.
fn name_environment(heap: &mut Heap, interner: &mut SymbolInterner, target: ValueId, label: &str) {
    let name_sym = interner.intern_symbol("name");
    let name_val = heap.alloc(RObject::String(vec![interner.intern_string(label)]));
    let tag_obj = heap.alloc(RObject::Symbol {
        name: name_sym,
        value: ValueId::UNBOUND,
        internal: ValueId::NIL,
    });
    let chain = heap.alloc(RObject::Pairlist {
        car: name_val,
        cdr: ValueId::NIL,
        tag: tag_obj,
    });
    if let Some(slot) = heap.get_mut(target) {
        slot.attrib = chain;
    }
}

/// GNU's default search path exactly as Rscript assembles it: the six
/// standard packages pre-attached between .GlobalEnv and base, with the
/// Autoloads environment ahead of base. Namespaces chain through their
/// enclosure slots in attachment order — the pure world's namespaces are
/// the attached environments directly, since implementations live in
/// base and attachment is a name-resolution shell either way.
fn attach_default_packages(heap: &mut Heap, interner: &mut SymbolInterner, base: ValueId, user: ValueId) {
    let order = ["stats", "graphics", "grDevices", "utils", "datasets", "methods"];
    let autoloads = env::new_environment(heap, base);
    name_environment(heap, interner, autoloads, "Autoloads");
    // Link base-ward: each package's enclosure is the next environment
    // down the chain exactly as GNU's attach ordering arranges.
    let mut chain_envs: Vec<ValueId> = Vec::new();
    for package in order {
        if let Some(ns) = heap.package_registry.get(package).copied() {
            chain_envs.push(ns);
        }
    }
    chain_envs.push(autoloads);
    chain_envs.push(base);
    // user -> stats -> graphics -> ... -> methods -> Autoloads -> base
    let mut head = user;
    for window in chain_envs.windows(2) {
        let (env, next) = (window[0], window[1]);
        if let Some(RObject::Environment { enclos, .. }) = heap.object_mut(env) {
            *enclos = next;
        }
        let _ = head;
        head = env;
    }
    if let Some(first) = chain_envs.first() {
        if let Some(RObject::Environment { enclos, .. }) = heap.object_mut(user) {
            *enclos = *first;
        }
    }
}
