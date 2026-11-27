#![forbid(unsafe_code)]
//! Direct O(1) generational heap.

use super::id::ValueId;
use super::intern::SymbolInterner;
use super::object::{RObject, Slot};

/// Slot occupancy: either a live object or a free-list link. Free slots
/// retain their generation counter so reuse bumps it and stale handles fail
/// the generation check.
#[derive(Debug)]
pub(crate) enum SlotState {
    Occupied(Slot),
    Free { generation: u32, next_free: Option<u32> },
}

/// One withRestarts frame entry — the handler closure and its public
/// restart object exactly as GNU's restart record.
#[derive(Debug, Clone, Copy)]
pub struct RestartEntry {
    pub id: u64,
    pub handler: crate::value::id::ValueId,
    pub object: crate::value::id::ValueId,
}

/// One withCallingHandlers entry — a condition-class name and its closure.
#[derive(Debug, Clone, Copy)]
pub struct CallingHandlerEntry {
    pub class_name: crate::value::intern::InternedStr,
    pub handler: crate::value::id::ValueId,
}

/// The object heap: a flat slot vector with a free list. All access goes
/// through generation-checked `ValueId` handles — O(1) array indexing with
/// no reverse lookups, pinning, or unsafe projection.
#[derive(Debug, Default)]
pub struct Heap {
    pub(crate) slots: Vec<SlotState>,
    pub(crate) free_head: Option<u32>,
    live_count: usize,
    /// Bytes of vector payload data, used as a GC pressure heuristic.
    payload_bytes: usize,
    /// Allocation count at which the next collection triggers; grows
    // geometrically after each collection.
    pub(crate) next_gc_threshold: usize,
    allocations_since_gc: usize,
    pub interner: SymbolInterner,
    /// The last error message — GNU's geterrmessage/seterrmessage state.
    pub err_message: String,
    /// Deferred warning texts — GNU's end-of-task warning queue. The harness
    /// drains this into `Warning message:` blocks after each top-level task.
    pub pending_warnings: Vec<String>,
    /// suppressWarnings dynamic state — warnings raised while true are dropped.
    pub warn_suppressed: bool,
    /// gcinfo reporting state — the primitive returns the PRIOR value exactly as GNU.
    pub gcinfo_state: bool,
    /// gctorture state pair — prior-value returns exactly as GNU.
    pub gctorture_state: bool,
    pub gctorture2_step: i32,
    /// The process umask shadow GNU's Sys.umask mutates — applied
    /// explicitly at file/dir creation since the test harness shares the
    /// real process mask.
    pub umask_state: i64,
    /// Library search path — GNU's .libPaths() state, scanned by
    /// find.package and system.file exactly as the loader does.
    pub lib_paths: Vec<String>,
    /// Graphics parameter state — GNU's par() machine, values as numeric
    /// vectors keyed by parameter name exactly as the device layer holds.
    /// The graphics device state exactly as GNU's per-device GPar —
    /// the layout cascade, dumb overrides, and frame progression.
    pub graphics: crate::value::graphics::GraphicsParState,
    /// The session-owned Mersenne stream — see value/rng.rs.
    pub rng: crate::value::rng::PureRng,
    /// GNU's sample.kind state — false is Rejection (the default since
    /// R 3.6.0), true the legacy Rounding branch of walker sampling.
    /// Lives session-locally exactly as the generator state does.
    pub sample_kind_rounding: bool,
    /// Per-session sampler memoization — GNU's static setup state in
    /// rbinom.c/rpois.c/rgamma.c/rbeta.c/rhyper.c and the signrank/wilcox
    /// distribution caches, owned by the session instead of thread-locals.
    pub math: crate::nmath::state::MathState,
    /// The color palette exactly as GNU's per-device state — palette()
    /// reads and replaces it, empty falling back to the R 4.x default.
    pub palette: Vec<String>,
    /// The closure context stack GNU's sys.* family reads — one entry per
    /// active closure application, innermost last.
    pub contexts: Vec<crate::value::eval::ContextFrame>,
    /// on.exit registrations keyed by the closure frame they were
    /// registered in — expressions stored unevaluated, run in order when
    /// the frame exits exactly as GNU.
    pub exit_handlers: std::collections::HashMap<crate::value::id::ValueId, Vec<crate::value::id::ValueId>>,
    /// Active S3 dispatch entries — (generic, object, remaining classes) —
    /// pushed by UseMethod/NextMethod exactly as GNU's dispatch context.
    pub s3_dispatch: Vec<(String, crate::value::id::ValueId, Vec<String>)>,
    /// S4 class registry — name → (ordered slots with types, contains
    /// parent) exactly as GNU's class table.
    pub s4_classes: std::collections::HashMap<String, (Vec<(String, String)>, Option<String>)>,
    /// Debug-marked closures — the .Internal debug machinery's state.
    pub debugged: std::collections::HashSet<crate::value::id::ValueId>,
    /// Interrupt suspension flag — GNU's interruptsSuspended state.
    pub interrupts_suspended: bool,
    /// bindtextdomain registry — domain to translation directory.
    pub text_domains: std::collections::HashMap<String, String>,
    /// Session environment overrides — the pure-safe answer to Sys.setenv
    /// being process-global unsafe mutation in modern Rust. Writes land in
    /// this map; reads check it first and fall back to the genuine process
    /// environment, which is exactly GNU's session-visible behavior for
    /// single-process use. None marks an explicit unset.
    pub env_vars: std::collections::HashMap<String, Option<String>>,
    /// The visibility flag — GNU's R_Visible exactly. Evaluation sets it
    /// true; invisible-returning forms (assignment, loops, print/cat,
    /// invisible()) clear it after their inner evaluations complete, so the
    /// flag left standing after any evaluation is that evaluation's
    /// visibility. Closure application inherits the body's final visibility
    /// with no machinery — exactly as GNU.
    pub visible: bool,
    /// Output capture — print/cat append GNU-formatted text here. Session
    /// output state lives with the runtime state for the migration slice;
    /// the console/connection subsystem migration owns the final shape.
    pub output: String,
    /// Output diversion: while a sink file is active every emitted byte
    /// lands there instead of the capture buffer, exactly as GNU's sink().
    pub sink_file: Option<std::fs::File>,
    /// Sink depth counter backing sink.number().
    pub sink_depth: i32,
    /// The session's global environment identity — serialization
    /// maps it to the T_GLOBALENV pseudo-reference exactly as GNU
    /// writes globalenv, since the environment graph itself never
    /// crosses the wire. Recorded at bootstrap.
    pub global_env_id: Option<crate::value::id::ValueId>,
    /// The base environment identity — same pseudo-reference
    /// treatment for closures serialized inside base.
    pub base_env_id: Option<crate::value::id::ValueId>,
    /// The loaded-package registry — package name to namespace
    /// environment exactly as GNU's loadedNamespaces. Synthetic
    /// registration seeds it today; RBG1 asset loading supplies
    /// real package content when the conversion lands.
    pub package_registry: std::collections::HashMap<String, crate::value::id::ValueId>,
    /// Long-lived host roots — retained FFI values stay live across
    /// collections exactly as the facade's retained surface needs.
    pub retained_roots: crate::value::roots::RootTable,
    /// The embedder's virtual file store — browser files persist
    /// bytes by path exactly as the wasm/android hosts consume them.
    pub browser_files: std::collections::HashMap<String, Vec<u8>>,
    /// When set, the virtual store IS the filesystem exactly as the
    /// browser facade's contract: reads never fall through to the host,
    /// writes never escape — the confined-session semantics wasm needs.
    pub browser_files_enabled: bool,
    /// Host filesystem confinement exactly as the facade's runtime path
    /// policy contracts: when set, absolute paths and paths escaping the
    /// virtual root are refused before any host access. None is the
    /// permissive native default.
    pub path_policy: Option<crate::value::path_policy::PathPolicy>,
    /// The live evaluation recursion depth — the resource limit
    /// embedders configure exactly as the legacy world's guard.
    pub eval_depth: usize,
    /// The configured depth ceiling; zero selects the GNU default.
    pub max_eval_depth: usize,
    /// The host-configured output byte budget — emission past the
    /// cap drops silently, the memory protection exactly as the
    /// facade's output limit. Sink writes bypass: they never
    /// accumulate in memory.
    pub output_byte_cap: Option<usize>,
    /// Whether the cap dropped emission this evaluation — the transcript
    /// marker decision rides this exactly as the facade's truncated flag.
    pub output_capture_truncated: bool,
    /// The call expression of the innermost raising frame while an error
    /// propagates — GNU's context-stack conditionCall discipline exactly:
    /// attached once at the frame that raised, never overwritten as the
    /// error unwinds outward, cleared when the condition is handled or
    /// reaches the top level. NIL means no error in flight.
    pub error_call: crate::value::id::ValueId,
    /// Cumulative bytes emitted this evaluation — the cap measures the
    /// eval's total output, not the drained buffer's momentary length,
    /// exactly as the facade's per-eval limit contract needs. The session
    /// resets the counter at each eval boundary.
    pub output_bytes_emitted: usize,
    /// The host-configured memory ceiling in payload bytes, polled
    /// alongside cancellation and the deadline — allocation pressure
    /// past the cap unwinds evaluation exactly as the facade's arena
        /// limit does.
    pub memory_cap_bytes: Option<usize>,
    /// Sticky admission flags — set in `alloc` when a cap is genuinely
    /// crossed, read and cleared at evaluation boundaries. Collection
    /// reclaims the bytes without clearing the fact of the crossing.
    pub memory_cap_tripped: bool,
    /// The host-configured ceiling on live arena objects, polled
    /// alongside the byte cap exactly as the facade's max_arena_nodes
    /// bounds the old world's arena occupancy.
    pub node_cap: Option<usize>,
    pub node_cap_tripped: bool,
    /// The host-configured execution deadline, polled alongside
    /// cancellation at eval entry exactly as the time limit needs.
    pub eval_deadline: Option<web_time::Instant>,
    /// Cooperative cancellation cell — the host's token observes
    /// SIGINT exactly as the legacy world's does; eval polls it at entry.
    pub cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The dynamic restart registry — withRestarts frames push entries,
    /// invokeRestart unwinds to them. Innermost last exactly as GNU.
    pub restart_stack: Vec<RestartEntry>,
    /// The calling-handler chain — withCallingHandlers frames consult at
    /// signal time without unwinding exactly as GNU.
    pub calling_handlers: Vec<CallingHandlerEntry>,
    /// Fresh restart identity source.
    pub next_restart_id: u64,
    /// Message suppression depth — suppressMessages increments while its
    /// expression evaluates; message() is silent above zero exactly as GNU.
    pub message_suppressed: i32,
    /// Session options as insertion-ordered name/value pairs — GNU's
    /// options list in miniature; reads fall back to width's default.
    pub options: Vec<(String, ValueId)>,
    /// The lazily-allocated width fallback value — 80 exactly as GNU's
    /// default, allocated on first use so the heap owns the object.
    pub options_width_fallback: Option<ValueId>,
}

impl Heap {
    pub fn new() -> Heap {
        Heap {
            next_gc_threshold: 1024,
            visible: true,
            umask_state: 0o022,
            palette: vec![
                "black".to_string(),
                "#DF536B".to_string(),
                "#61D04F".to_string(),
                "#2297E6".to_string(),
                "#28E2E5".to_string(),
                "#CD0BBC".to_string(),
                "#F5C710".to_string(),
                "gray62".to_string(),
            ],
            ..Heap::default()
        }
    }

    /// Allocate a new object and return its handle.
    /// Emit output bytes: diverted to the sink file while one is active,
    /// the capture buffer otherwise — GNU's sink() diversion point.
    pub fn emit(&mut self, text: &str) {
        if let Some(file) = &mut self.sink_file {
            use std::io::Write;
            let _ = file.write_all(text.as_bytes());
            return;
        }
        // The output cap drops emission past the budget — evaluation
        // continues exactly as the facade's limit contract; the buffer
        // simply stops growing, the memory protection the embedder needs.
        if let Some(cap) = self.output_byte_cap {
            if self.output_bytes_emitted >= cap {
                self.output_capture_truncated = true;
                return;
            }
            let remaining = cap - self.output_bytes_emitted;
            if text.len() > remaining {
                self.output_capture_truncated = true;
                // Truncate at a character boundary — the budget is bytes,
                // the buffer is UTF-8.
                let mut end = remaining.min(text.len());
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                self.output.push_str(&text[..end]);
                self.output_bytes_emitted += end;
                return;
            }
        }
        self.output_bytes_emitted += text.len();
        self.output.push_str(text);
    }

    pub fn alloc(&mut self, body: RObject) -> ValueId {
        self.allocations_since_gc += 1;
        self.payload_bytes += payload_estimate(&body);
        // Admission accounting — the cap crossing sticks because the
        // allocation genuinely happened over budget; boundary collection
        // can reclaim the object afterward without erasing that fact.
        if let Some(cap) = self.memory_cap_bytes {
            if self.payload_bytes > cap {
                self.memory_cap_tripped = true;
            }
        }
        if let Some(cap) = self.node_cap {
            if self.live_count + 1 > cap {
                self.node_cap_tripped = true;
            }
        }
        match self.free_head {
            Some(free_index) => {
                let slot_entry = &mut self.slots[free_index as usize];
                let (generation, next_free) = match slot_entry {
                    SlotState::Free {
                        generation,
                        next_free,
                    } => (*generation, *next_free),
                    SlotState::Occupied(_) => unreachable!("free list points at occupied slot"),
                };
                // Reuse with a bumped generation so prior handles to this
                // slot are invalidated.
                let generation = generation.wrapping_add(1).max(1);
                *slot_entry = SlotState::Occupied(Slot::new(generation, body));
                self.free_head = next_free;
                self.live_count += 1;
                ValueId::new(free_index, generation)
            }
            None => {
                debug_assert!(self.slots.len() < u32::MAX as usize);
                let slot_index = self.slots.len() as u32;
                let generation = 1;
                self.slots
                    .push(SlotState::Occupied(Slot::new(generation, body)));
                self.live_count += 1;
                ValueId::new(slot_index, generation)
            }
        }
    }

    /// Generation-checked immutable access. Returns `None` for stale or
    /// sentinel handles.
    pub fn get(&self, id: ValueId) -> Option<&Slot> {
        let index = id.slot_index()?;
        match self.slots.get(index)? {
            SlotState::Occupied(slot) if slot.generation == id.generation() => Some(slot),
            _ => None,
        }
    }

    /// Generation-checked mutable access.
    pub fn get_mut(&mut self, id: ValueId) -> Option<&mut Slot> {
        let index = id.slot_index()?;
        match self.slots.get_mut(index)? {
            SlotState::Occupied(slot) if slot.generation == id.generation() => Some(slot),
            _ => None,
        }
    }

    /// Immutable access to just the payload.
    pub fn object(&self, id: ValueId) -> Option<&RObject> {
        self.get(id).map(|slot| &slot.body)
    }

    /// Mutable access to just the payload.
    pub fn object_mut(&mut self, id: ValueId) -> Option<&mut RObject> {
        self.get_mut(id).map(|slot| &mut slot.body)
    }

    /// True when the handle currently addresses a live object.
    pub fn is_live(&self, id: ValueId) -> bool {
        if id.is_sentinel() {
            return true;
        }
        self.get(id).is_some()
    }

    pub fn live_count(&self) -> usize {
        self.live_count
    }

    /// Sweep-side accounting — a freed object's payload leaves the
    /// occupancy measure exactly as its allocation entered it.
    pub(crate) fn dec_payload(&mut self, bytes: usize) {
        self.payload_bytes = self.payload_bytes.saturating_sub(bytes);
    }

    pub fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    /// Whether allocation pressure suggests a collection. The collector
    /// itself decides; this is the cheap admission probe.
    pub fn gc_pressure(&self) -> bool {
        self.allocations_since_gc >= self.next_gc_threshold
    }

    pub(crate) fn note_collection(&mut self, surviving: usize) {
        self.allocations_since_gc = 0;
        // Grow the threshold geometrically relative to the surviving set so
        // collection frequency stays proportional to live data.
        self.next_gc_threshold = (surviving.max(256) * 2).max(1024);
    }

    pub(crate) fn dec_live(&mut self) {
        self.live_count = self.live_count.saturating_sub(1);
    }
}

pub(crate) fn payload_estimate(body: &RObject) -> usize {
    match body {
        RObject::Logical(v) | RObject::Integer(v) => v.len() * 4,
        RObject::Real(v) => v.len() * 8,
        RObject::Complex(v) => v.len() * 16,
        RObject::Raw(v) => v.len(),
        RObject::String(v) => v.len() * 8,
        RObject::List(v) | RObject::Expression(v) => v.len() * 8,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_get_roundtrip() {
        let mut heap = Heap::new();
        let id = heap.alloc(RObject::Real(vec![1.0, 2.0, 3.0]));
        assert!(heap.is_live(id));
        match heap.object(id) {
            Some(RObject::Real(v)) => assert_eq!(v, &[1.0, 2.0, 3.0]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn sentinel_handles_are_not_heap_objects() {
        let heap = Heap::new();
        assert!(heap.object(ValueId::NIL).is_none());
        assert!(heap.is_live(ValueId::NIL));
    }
}
