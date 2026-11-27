//! The productized new-world evaluation session: parse → eval → print as
//! one self-contained pipeline with zero legacy-world involvement.
//!
//! `Session` owns the generational heap, the symbol interner, and the
//! bootstrapped global environment; `eval_script` reproduces GNU's script
//! semantics exactly — every visible top-level result auto-prints, NULL
//! never prints, deferred warnings flush after each expression's output
//! exactly as the REPL end-of-task discipline pins.

use super::env;
use super::eval::{eval, RSignal};
use super::heap::Heap;
use super::id::ValueId;
use super::intern::SymbolInterner;
use super::object::{BuiltinId, RObject};
use super::parser::{parse_native_expressions, ParseError};

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
        let mut interner = SymbolInterner::new();
        let global_env = bootstrap_env(&mut heap, &mut interner);
        Session {
            heap,
            interner,
            global_env,
        }
    }

    /// Evaluate R source with GNU's exact script contract: expressions
    /// evaluate in order against the shared global environment, visible
    /// results auto-print, and deferred warnings flush after each
    /// expression's output. The first error aborts the remaining script.
    /// Returns the accumulated standard output.
    pub fn eval_script(&mut self, source: &str) -> Result<String, SessionError> {
        let expressions = parse_native_expressions(source, &mut self.heap, &mut self.interner)
            .map_err(SessionError::Parse)?;
        for expr in &expressions {
            let value = eval(&mut self.heap, &mut self.interner, *expr, self.global_env)
                .map_err(SessionError::Eval)?;
            // Primitive writes (cat/print/writeLines) and auto-printed
            /// values interleave in evaluation order exactly as GNU's
            /// stdout does — both append to the one output buffer.
            if self.heap.visible {
                let text = super::print::format_value(&self.heap, &self.interner, value);
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
}

/// GNU's environment structure: primitives live in a base environment
/// exactly as package:base; user code evaluates in a globalenv whose
/// enclosure chain reaches base — ls() sees only user bindings and
/// definitions never pollute the base frame.
pub fn bootstrap_env(heap: &mut Heap, interner: &mut SymbolInterner) -> ValueId {
    use super::primitives::primitive_id_for_name;
    let base = env::new_environment(heap, ValueId::EMPTY_ENV);
    for &name in super::primitives::primitive_names() {
        let sym = interner.intern_symbol(name);
        let prim = primitive_id_for_name(name).expect("mapped primitive");
        let object = heap.alloc(RObject::Primitive(BuiltinId(prim)));
        env::define(heap, base, sym, object).expect("define primitive");
    }
    // GNU's reserved typed-NA constants — bindings in base exactly as
    // GNU's base namespace carries them.
    let na_int = heap.alloc(RObject::Integer(vec![super::object::NA_INTEGER]));
    let sym = interner.intern_symbol("NA_integer_");
    env::define(heap, base, sym, na_int).expect("NA_integer_");
    let na_real = heap.alloc(RObject::Real(vec![f64::from_bits(super::object::NA_REAL_BITS)]));
    let sym = interner.intern_symbol("NA_real_");
    env::define(heap, base, sym, na_real).expect("NA_real_");
    let pi_val = heap.alloc(RObject::Real(vec![std::f64::consts::PI]));
    let sym = interner.intern_symbol("pi");
    env::define(heap, base, sym, pi_val).expect("pi");
    // .GlobalEnv resolves to the user environment itself — the global
    /// exactly as GNU's top-level reference.
    let user = env::new_environment(heap, base);
    let genv_sym = interner.intern_symbol(".GlobalEnv");
    env::define(heap, base, genv_sym, user).expect(".GlobalEnv");
    // The global environment carries its printed name — GNU's
    /// R_GlobalEnv exactly — through the slot attribute chain.
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
    /// them is TRUE exactly as GNU's aliased constants.
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
    env::define(heap, base, sym, na_str).expect("NA_character_");
    user
}
