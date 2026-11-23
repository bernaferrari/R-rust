#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R profiling support -- ports profiling functions from eval.c.
//!
//! This module provides the R profiler (Rprof()) and related functions.
//! The profiler samples the call stack at regular intervals and writes
//! results to a file for later analysis.
//!
//! Ported from R's src/main/eval.c profiling sections (~lines 38-935).
//!
//! Key functions:
//! - `do_Rprof` / `R_InitProfiling` / `R_EndProfiling` -- main profiling lifecycle
//! - `doprof` -- render a sample on the owning execution thread
//! - `ProfileSchedule` -- owner-local CPU/elapsed deadlines
//! - `lineprof` / `getFilenum` -- line profiling helpers
//! - `ProfileBuffer` -- checked profiling output buffer
//! - `write_profile_bytes` / `pf_int` -- profiling file writers
//! - `findProfContext` -- context traversal for profiling
//! - `do_bcprofstart` / `do_bcprofstop` / `do_bcprofcounts` -- BC profiling
//! - `dobcprof` / `dobcprof_null` -- BC profiling signal handlers

// wasm32 stubs (below) leave a few native-only imports unused.
#![cfg_attr(target_arch = "wasm32", allow(unused_imports))]

use std::ffi::CStr;
use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::eval::attrib_core::getAttrib;

use crate::sexp::accessors::{
    CADDR, CADR, CAR, CDR, CHAR, LENGTH, PRINTNAME, RAW, STRING_ELT, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::context::RCNTXT;
use crate::sexp::context::ctxt_flags;
use crate::sexp::envir::R_findVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, TRUE};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::{
    NO_PROFILING_OPCODE, PROFILING_OPCODE_COUNT, ProfilingState, RInstance, RuntimeValue,
    with_profiling_state_in, with_required_current_instance,
};
use crate::sexp::owner::{OwnerPin, OwnerToken};

mod schedule;
mod workspace;
pub(crate) use schedule::ProfileSchedule;

/// Read only the caller's thread CPU counter; no runtime or process timer.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn thread_cpu_time_nanos() -> Option<u64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) } != 0 {
        return None;
    }
    u64::try_from(time.tv_sec)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(time.tv_nsec).ok()?)
}
#[cfg(not(all(unix, not(target_arch = "wasm32"))))]
fn thread_cpu_time_nanos() -> Option<u64> {
    None
}

/// Profiling timer type (ITIMER_PROF is not available on Android).
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
const PROF_TIMER: core::ffi::c_int = libc::ITIMER_PROF;
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const PROF_TIMER: core::ffi::c_int = 2; // ITIMER_PROF value on most Unix systems
/// WASM M1: no setitimer; timer fns below are wasm no-op stubs.
#[cfg(target_arch = "wasm32")]
const PROF_TIMER: c_int = 2;
use crate::sexp::symbol::{
    R_Bracket2Symbol, R_DollarSymbol, R_DoubleColonSymbol, R_TripleColonSymbol,
};

// ---------------------------------------------------------------------------
// Active source-reference & bytecode-interpreter location tracking
// ---------------------------------------------------------------------------

fn owned_profile_value(owner: OwnerToken<'_>, raw: SEXP) -> RuntimeValue {
    if raw.is_null() {
        return RuntimeValue::empty();
    }
    let value = owner
        .sexp(raw)
        .and_then(|value| value.into_owned())
        .unwrap_or_else(|error| {
            crate::sexp::context::r_error(format!("invalid context value: {error}"))
        });
    RuntimeValue::from_owned(value)
}

fn owned_srcref(owner: OwnerToken<'_>, raw: SEXP) -> RuntimeValue {
    if raw == R_NilValue() {
        RuntimeValue::empty()
    } else {
        owned_profile_value(owner, raw)
    }
}

fn pin_profiler_owner(owner: OwnerToken<'_>) -> Option<OwnerPin> {
    owner.pin().unwrap_or_else(|error| {
        crate::sexp::context::r_error(format!("unavailable context owner: {error}"))
    })
}

/// Return the per-instance `R_InBCInterpreter` sentinel symbol.
///
/// Mirrors GNU R's `R_InBCInterpreter = mkSymMarker(mkChar("<in-bc-interp>"))`.
/// The symbol is uninterned and rooted in `ProfilingState::in_bc_interpreter`,
/// so no user R value can collide with it by pointer identity.
pub(crate) fn get_R_InBCInterpreter() -> SEXP {
    in_bc_interpreter(current_profiler_owner())
}

fn in_bc_interpreter(owner: OwnerToken<'_>) -> SEXP {
    let existing = owner.with_profiling_state(|state| state.in_bc_interpreter.as_raw());
    if !existing.is_null() && existing != R_NilValue() {
        return existing;
    }
    let _pin = pin_profiler_owner(owner);
    let Ok(marker) = owner.node_factory().allocate(|arena| {
        let node = arena.alloc_node(SEXPTYPE::SYMSXP);
        if node.is_null() {
            return None;
        }
        let name = arena.alloc_charsxp(b"<in-bc-interp>");
        if name.is_null() {
            return None;
        }
        crate::sexp::accessors::SET_PRINTNAME(node, name);
        Some(node)
    }) else {
        return R_NilValue();
    };
    let marker = RuntimeValue::from_owned(
        marker
            .into_owned()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
    );
    let nil = R_NilValue();
    let (raw, displaced) = owner.with_profiling_state(|state| {
        if state.in_bc_interpreter.is_null() || state.in_bc_interpreter.as_raw() == nil {
            let raw = marker.as_raw();
            (raw, std::mem::replace(&mut state.in_bc_interpreter, marker))
        } else {
            (state.in_bc_interpreter.as_raw(), marker)
        }
    });
    drop(displaced);
    raw
}

fn update_srcref(owner: OwnerToken<'_>, raw: SEXP) {
    let value = owned_srcref(owner, raw);
    drop(owner.with_profiling_state(|state| std::mem::replace(&mut state.sref, value)));
}

/// Update the active `R_Srcref` on the current runtime instance.
pub(crate) fn set_current_srcref(srcref: SEXP) {
    update_srcref(current_profiler_owner(), srcref);
}

/// Update the active `R_Srcref` on an explicit runtime instance.
///
/// # Safety
/// `instance` must be a live runtime for the duration of this call.
pub(crate) unsafe fn set_current_srcref_in(instance: *mut RInstance, srcref: SEXP) {
    // SAFETY: the caller's precondition supplies a live runtime owner.
    update_srcref(unsafe { OwnerToken::from_raw(instance) }, srcref);
}

/// RAII guard that saves and restores the owner's profiling `sref` on the
/// original runtime owner, including across panics/unwinds.
pub(crate) struct SrcrefGuard {
    owner: OwnerToken<'static>,
    prev_sref: RuntimeValue,
    _pin: Option<OwnerPin>,
}

impl SrcrefGuard {
    pub(crate) fn push(new_srcref: SEXP) -> Self {
        Self::push_owner(current_profiler_owner(), new_srcref)
    }

    /// # Safety
    /// `instance` must be a live runtime for the duration of this call.
    pub(crate) unsafe fn push_in(instance: *mut RInstance, new_srcref: SEXP) -> Self {
        // SAFETY: the caller's precondition supplies a live runtime owner.
        Self::push_owner(unsafe { OwnerToken::from_raw(instance) }, new_srcref)
    }

    fn push_owner(owner: OwnerToken<'static>, raw: SEXP) -> Self {
        let pin = pin_profiler_owner(owner);
        let value = owned_srcref(owner, raw);
        let prev_sref =
            owner.with_profiling_state(|state| std::mem::replace(&mut state.sref, value));
        Self {
            owner,
            prev_sref,
            _pin: pin,
        }
    }

    pub(crate) fn update(&self, new_srcref: SEXP) {
        if self.owner.is_live() {
            update_srcref(self.owner, new_srcref);
        }
    }
}

impl Drop for SrcrefGuard {
    fn drop(&mut self) {
        if self.owner.is_live() {
            let prev = std::mem::take(&mut self.prev_sref);
            drop(
                self.owner
                    .with_profiling_state(|state| std::mem::replace(&mut state.sref, prev)),
            );
        }
    }
}

/// RAII guard for active bytecode evaluation (`bcEval` / `eval_gnu_adapter`).
///
/// Mirrors GNU R's `save_bcEval_globals` / `restore_bcEval_globals`: saves
/// `(R_Srcref, R_BCbody, bcrelpc)`, sets `R_Srcref = R_InBCInterpreter`,
/// roots `bcbody`, and restores the prior state on exit or unwind if the
/// original runtime is still live.
pub(crate) struct BcEvalSrcrefGuard {
    owner: OwnerToken<'static>,
    prev_sref: RuntimeValue,
    prev_bcbody: RuntimeValue,
    prev_bcrelpc: isize,
    _pin: Option<OwnerPin>,
}

impl BcEvalSrcrefGuard {
    pub(crate) fn enter(body: SEXP, initial_pc: isize) -> Self {
        let owner = current_profiler_owner();
        let pin = pin_profiler_owner(owner);
        // Root the body before the marker allocation so every new owner
        // survives a collection inside `in_bc_interpreter`.
        let body = owned_profile_value(owner, body);
        let in_bc = owned_profile_value(owner, in_bc_interpreter(owner));
        let (prev_sref, prev_bcbody, prev_bcrelpc) = owner.with_profiling_state(|state| {
            (
                std::mem::replace(&mut state.sref, in_bc),
                std::mem::replace(&mut state.bcbody, body),
                std::mem::replace(&mut state.bcrelpc, initial_pc),
            )
        });
        Self {
            owner,
            prev_sref,
            prev_bcbody,
            prev_bcrelpc,
            _pin: pin,
        }
    }

    #[inline]
    pub(crate) fn set_pc(&self, pc: isize) {
        if self.owner.is_live() {
            self.owner
                .with_profiling_state(|state| state.bcrelpc = pc);
        }
    }
}

impl Drop for BcEvalSrcrefGuard {
    fn drop(&mut self) {
        if self.owner.is_live() {
            let prev_sref = std::mem::take(&mut self.prev_sref);
            let prev_bcbody = std::mem::take(&mut self.prev_bcbody);
            drop(self.owner.with_profiling_state(|state| {
                state.bcrelpc = self.prev_bcrelpc;
                (
                    std::mem::replace(&mut state.sref, prev_sref),
                    std::mem::replace(&mut state.bcbody, prev_bcbody),
                )
            }));
        }
    }
}

fn get_R_ToplevelContext() -> *mut RCNTXT {
    unsafe {
        let mut context = super::runtime::global_context();
        while !context.is_null() && !(*context).nextcontext.is_null() {
            context = (*context).nextcontext;
        }
        // The Rust evaluator can start with a function as its oldest frame,
        // without a separate GNU CTXT_TOPLEVEL anchor. That function remains
        // profileable; only an actual top-level marker is an exclusion bound.
        if !context.is_null() && (*context).callflag == ctxt_flags::CTXT_TOPLEVEL {
            context
        } else {
            ptr::null_mut()
        }
    }
}

/// Scan a bytecode constant pool from the end for a location table (`INTSXP`
/// whose class inherits `tclass`, e.g. `"srcrefsIndex"`).
///
/// Mirrors GNU R's `findLocTable()` in `src/main/eval.c`.
fn find_loc_table(constants: SEXP, tclass: &CStr) -> SEXP {
    if constants.is_null() || constants == R_NilValue() || TYPEOF(constants) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let n = LENGTH(constants);
    for i in (0..n).rev() {
        let entry = crate::sexp::accessors::VECTOR_ELT(constants, i as i64);
        if !entry.is_null()
            && TYPEOF(entry) == SEXPTYPE::INTSXP
            && crate::mainutils::objects::inherits2(entry, &tclass) != 0
        {
            return entry;
        }
    }
    R_NilValue()
}

/// Resolve the srcref for a snapshotted `(bcbody, relpc)` pair.
fn find_bc_interpreter_srcref_from(body: SEXP, relpc: isize) -> SEXP {
    if body.is_null() || body == R_NilValue() || TYPEOF(body) != SEXPTYPE::BCODESXP || relpc < 0 {
        return R_NilValue();
    }
    let body = crate::sexp::context::own_control_value(body);
    let constants =
        crate::sexp::context::own_control_value(crate::eval::bc_eval::BCODE_CONSTS(body.as_raw()));
    let table = find_loc_table(constants.as_raw(), c"srcrefsIndex");
    if table.is_null() || table == R_NilValue() || TYPEOF(table) != SEXPTYPE::INTSXP {
        return R_NilValue();
    }
    let table_len = LENGTH(table) as isize;
    if relpc >= table_len {
        return R_NilValue();
    }
    let cidx = crate::sexp::accessors::INTEGER_ELT(table, relpc as i32);
    if cidx < 0 || cidx >= LENGTH(constants.as_raw()) {
        return R_NilValue();
    }
    crate::sexp::accessors::VECTOR_ELT(constants.as_raw(), cidx as i64)
}

/// Return the srcref for the current bytecode instruction (when `cptr` is null)
/// or for the bytecode instruction active when `cptr` was pushed.
///
/// Mirrors GNU R's `R_findBCInterpreterSrcref()` in `src/main/eval.c`.
pub(crate) unsafe fn R_findBCInterpreterSrcref(cptr: *mut RCNTXT) -> SEXP {
    unsafe {
        let (body, relpc) = if cptr.is_null() {
            with_profiling_state(|state| (state.bcbody.owned(), state.bcrelpc))
        } else {
            ((*cptr).bcbody.owned(), (*cptr).relpc)
        };
        let body_raw = body
            .as_ref()
            .map_or(ptr::null_mut(), |value| value.as_raw());
        find_bc_interpreter_srcref_from(body_raw, relpc)
    }
}

// ---------------------------------------------------------------------------
// BC profiling constants
// ---------------------------------------------------------------------------

/// Number of bytecode opcodes -- sentinel value in the BC opcode enum.
/// In R, this is the last entry in the bytecode opcode enum (eval.c line ~4732).
/// We use 256 as a reasonable upper bound covering all defined opcodes.
const OPCOUNT: usize = PROFILING_OPCODE_COUNT;

/// Sentinel for "no current opcode" during BC profiling.
const NO_CURRENT_OPCODE: c_int = NO_PROFILING_OPCODE;

/// Buffer size for profiling output.
const PROFBUFSIZ: usize = 10500;

/// Maximum digits for IEEE double integer part printing.
const PB_MAX_DBL_DIGITS: usize = 309;

/// Profiling event type: CPU time or elapsed time.
#[derive(Clone, Copy, PartialEq)]
#[repr(C)]
pub enum rpe_type {
    RPE_CPU = 0,
    RPE_ELAPSED = 1,
}

fn profiling_event_code(event: rpe_type) -> c_int {
    event as c_int
}

fn profiling_event_from_code(event: c_int) -> rpe_type {
    if event == rpe_type::RPE_ELAPSED as c_int {
        rpe_type::RPE_ELAPSED
    } else {
        rpe_type::RPE_CPU
    }
}

fn with_profiling_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut ProfilingState) -> R,
{
    current_profiler_owner().with_profiling_state(f)
}

fn current_profiler_owner() -> OwnerToken<'static> {
    OwnerToken::current()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Retain the physical original owner across profiling callbacks and detect
/// stop/restart without consulting a replacement runtime.
struct ProfileEpoch {
    pin: crate::sexp::owner::OwnerPin,
    generation: u64,
}
impl ProfileEpoch {
    fn capture(owner: crate::sexp::owner::OwnerToken<'_>) -> crate::sexp::object::SexpResult<Self> {
        owner.require_active()?;
        let pin = owner
            .pin()?
            .ok_or(crate::sexp::object::SexpError::RootUnavailable)?;
        let generation = pin.with_profiling_state(|state| state.profile_generation);
        Ok(Self { pin, generation })
    }
    fn unchanged(&self) -> bool {
        let owner = self
            .pin
            .token()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        self.pin
            .with_profiling_state(|state| state.profile_generation == self.generation)
    }
}
struct SamplingGuard(ProfileEpoch);
impl Drop for SamplingGuard {
    fn drop(&mut self) {
        // Physical cleanup must still work when a provider revoked the owner.
        self.0.pin.with_profiling_state(|state| {
            if state.profile_generation == self.0.generation {
                state.sampling_in_progress = false;
            }
        });
    }
}

/// Caller roots the active execution graph and ends every arena/payload loan
/// before servicing a sample, which may invoke line/source providers.
#[inline]
pub(crate) fn sample_at_safe_point(
    owner: crate::sexp::owner::OwnerToken<'_>,
) -> crate::sexp::object::SexpResult<()> {
    // Each audited caller has just checked its original execution owner. The
    // disabled path must stay a plain flag read, without clocks or Rc traffic.
    if !owner.profiling_scheduled()? {
        return Ok(());
    }
    crate::sexp::instance::check_cancellation();
    write_profile_sample(owner, false)
}
fn write_profile_sample(
    owner: crate::sexp::owner::OwnerToken<'_>,
    forced: bool,
) -> crate::sexp::object::SexpResult<()> {
    owner.require_active()?;
    if owner.with_profiling_state(|state| state.profiling == 0 || state.sampling_in_progress) {
        return Ok(());
    }
    let epoch = ProfileEpoch::capture(owner)?;
    let ready = owner.with_profiling_state(|state| {
        if state.profiling == 0 || state.sampling_in_progress {
            return false;
        }
        if !forced
            && !state
                .profile_schedule
                .as_mut()
                .is_some_and(ProfileSchedule::due)
        {
            return false;
        }
        state.sampling_in_progress = true;
        true
    });
    if ready {
        let _guard = SamplingGuard(epoch);
        let _visibility = super::runtime::VisibilityGuard::new();
        {
            doprof(0);
        }
        owner.require_active()?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MemoryProfileSnapshot {
    current_bytes: u64,
    peak_bytes: u64,
    active_nodes: u64,
    gc_freed_nodes: u64,
}

fn memory_profile_snapshot() -> MemoryProfileSnapshot {
    // SAFETY: `inst` is the live current-instance pointer handed to this accessor closure (or owned by this caller) for the duration of the call.
    with_required_current_instance(|inst| unsafe { memory_profile_snapshot_in(inst) })
}

/// # Safety
///
/// `inst`/`instance` must point to a live `RInstance` (as supplied by the instance accessors) with
/// no conflicting outstanding borrow of the fields touched here.
unsafe fn memory_profile_snapshot_in(instance: *mut RInstance) -> MemoryProfileSnapshot {
    // P1/P2: all borrows are statement-local reads/writes of plain counters;
    // nothing here allocates or reenters the interpreter.
    let (current_bytes, active_nodes, gc_peak, gc_freed_nodes) = unsafe {
        (
            (*instance).arena.total_bytes_allocated(),
            (*instance).arena.node_count() as u64,
            (*instance).gc_state.stats.peak_memory,
            (*instance).gc_state.stats.freed as u64,
        )
    };
    // SAFETY: `instance` is the caller's live fixture; the lend mutates only
    // the local peak counter.
    let peak_bytes = unsafe {
        with_profiling_state_in(instance, |state| {
            state.memory_peak_bytes = state.memory_peak_bytes.max(current_bytes).max(gc_peak);
            state.memory_peak_bytes
        })
    };
    MemoryProfileSnapshot {
        current_bytes: current_bytes as u64,
        peak_bytes: peak_bytes as u64,
        active_nodes,
        gc_freed_nodes,
    }
}

fn write_memory_profile_prefix(pb: &mut ProfileBuffer<'_>, snapshot: MemoryProfileSnapshot) {
    for value in [
        snapshot.current_bytes,
        snapshot.peak_bytes,
        snapshot.active_nodes,
        snapshot.gc_freed_nodes,
    ] {
        pb.append(b":");
        pb.append_integer(value, false);
    }
    pb.append(b":");
}

// ---------------------------------------------------------------------------
// R_Profiling -- check if profiling is active
// ---------------------------------------------------------------------------

/// Check whether R profiling is currently active.
pub fn R_Profiling_active() -> c_int {
    with_profiling_state(|state| state.profiling)
}

// ---------------------------------------------------------------------------
// R_isRprofiling -- check if profiling is enabled
// ---------------------------------------------------------------------------

/// Check if R profiling is enabled (public API).
pub fn R_isRprofiling() -> c_int {
    R_Profiling_active()
}

// ---------------------------------------------------------------------------
// ProfileBuffer -- profiling output buffer
// ---------------------------------------------------------------------------

/// Checked profiling output buffer over a steady-state stack array.
///
/// Appends keep one byte reserved for the caller's terminator, as the old
/// `pb_*` writers did. An append that cannot fit sets a sticky overflow flag;
/// `as_bytes` then yields nothing so the whole sample is discarded.
struct ProfileBuffer<'a> {
    bytes: &'a mut [u8],
    used: usize,
    overflow: bool,
}
impl<'a> ProfileBuffer<'a> {
    fn new(bytes: &'a mut [u8]) -> Self {
        Self {
            bytes,
            used: 0,
            overflow: false,
        }
    }
    fn append(&mut self, bytes: &[u8]) {
        if self.overflow {
            return;
        }
        let Some(end) = self
            .used
            .checked_add(bytes.len())
            .filter(|&end| end < self.bytes.len())
        else {
            self.overflow = true;
            return;
        };
        self.bytes[self.used..end].copy_from_slice(bytes);
        self.used = end;
    }
    fn append_digits(&mut self, digits: &mut [u8], negative: bool) {
        digits.reverse();
        if negative {
            self.append(b"-");
        }
        self.append(digits);
    }
    fn append_integer(&mut self, mut num: u64, negative: bool) {
        let mut digits = [0u8; 20];
        let mut used = 0;
        loop {
            digits[used] = (num % 10) as u8 + b'0';
            used += 1;
            num /= 10;
            if num == 0 {
                break;
            }
        }
        self.append_digits(&mut digits[..used], negative);
    }
    fn append_int(&mut self, num: i64) {
        self.append_integer(num.unsigned_abs(), num.is_negative());
    }
    /// Simplistic printing of the integer parts of doubles (like %0.f) used
    /// only for stack traces in profiling data. Not suitable for general use.
    fn append_double(&mut self, num: f64) {
        if !num.is_finite() {
            // Keep the existing profile spelling for non-finite values.
            self.append(if num.to_bits() == f64::NAN.to_bits() {
                b"NaN"
            } else if num > 0.0 {
                b"Inf"
            } else {
                b"-Inf"
            });
            return;
        }
        let mut digits = [0u8; PB_MAX_DBL_DIGITS];
        let mut n = num.abs();
        let mut used = 0;
        loop {
            digits[used] = (n % 10.0) as u8 + b'0';
            used += 1;
            n /= 10.0;
            if n < 1.0 {
                break;
            }
            if used == digits.len() {
                // Cannot happen with IEEE double
                return;
            }
        }
        self.append_digits(&mut digits[..used], num < 0.0);
    }
    fn as_bytes(&self) -> Option<&[u8]> {
        (!self.overflow).then(|| &self.bytes[..self.used])
    }
}

// ---------------------------------------------------------------------------
// getFilenum -- get/create file number for line profiling
// ---------------------------------------------------------------------------

/// Get a file number for line profiling output.
///
/// Does a linear search through previously recorded filenames.
/// If this one is new, try to add it.
///
/// Ported from R's `getFilenum()` in eval.c.
fn ensure_source_slots(
    owner: crate::sexp::owner::OwnerToken<'_>,
) -> crate::sexp::object::SexpResult<bool> {
    let (length, capacity, maximum, charged) = with_profiling_state(|state| {
        (
            state.srcfiles.len(),
            state.srcfiles.capacity(),
            state.srcfile_bufcount,
            state.srcfiles_reservation.is_some(),
        )
    });
    if capacity > length && charged {
        return Ok(true);
    }
    if length >= maximum || (capacity != 0 && !charged) {
        return Ok(false);
    }
    let Some(mut replacement) = workspace::ProfileBuffer::with_capacity(owner, maximum)? else {
        return Ok(false);
    };
    if !with_profiling_state(|state| replacement.copy_from_slice(&state.srcfiles)) {
        return Ok(false);
    }
    let (values, reservation) = replacement.into_parts();
    // The old reservation remains charged while both actual allocations live.
    let (old_values, old_reservation) = with_profiling_state(|state| {
        (
            std::mem::replace(&mut state.srcfiles, values),
            state.srcfiles_reservation.replace(reservation),
        )
    });
    drop(old_values);
    drop(old_reservation);
    Ok(true)
}

/// The typed filename and buffer owners survive providers. Reborrow character
/// bytes only after callbacks, so no filename or source-offset copy is needed.
fn getFilenum(filename: &crate::sexp::object::Sexp<'_>) -> c_int {
    unsafe {
        if filename.typeof_() != SEXPTYPE::CHARSXP {
            return 0;
        }
        crate::sexp::owner::OwnerToken::with_current(|owner| {
            owner
                .node_factory()
                .link(filename)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let epoch = ProfileEpoch::capture(owner)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let (buffer, used, capacity, line_prof) = with_profiling_state(|state| {
                (
                    state.srcfiles_buffer.owned(),
                    state.srcfile_bytes_used,
                    state.srcfile_bufcount,
                    state.line_profiling,
                )
            });
            let Some(buffer) = buffer else {
                return 0;
            };
            if line_prof <= 0 || buffer.typeof_() != SEXPTYPE::RAWSXP {
                return 0;
            }
            let total = XLENGTH(buffer.as_raw()) as usize;
            if !epoch.unchanged() {
                return 0;
            }
            let bytes = RAW(buffer.as_raw());
            if !epoch.unchanged() {
                return 0;
            }
            if !with_profiling_state(|state| state.srcfiles_buffer.as_raw() == buffer.as_raw()) {
                return 0;
            }
            // This scope performs only bounded byte reads. It ends every payload
            // reference before source-table admission borrows the original arena.
            let (existing, length) = {
                let name = crate::sexp::accessors::charsxp_as_utf8(filename.as_raw());
                let existing = with_profiling_state(|state| {
                    for (index, offset) in state.srcfiles.iter().copied().enumerate() {
                        let Some(remaining) = total.checked_sub(offset) else {
                            return Err(());
                        };
                        if remaining == 0 || bytes.is_null() {
                            return Err(());
                        }
                        let stored = std::slice::from_raw_parts(bytes.add(offset), remaining);
                        let Some(end) = stored.iter().position(|byte| *byte == 0) else {
                            return Err(());
                        };
                        if stored[..end] == *name {
                            return Ok(Some((index + 1) as c_int));
                        }
                    }
                    Ok(None)
                });
                (existing, name.len())
            };
            match existing {
                Ok(Some(number)) => return number,
                Err(()) => {
                    with_profiling_state(|state| state.profiling_error = 2);
                    return 0;
                }
                Ok(None) => {}
            }
            if with_profiling_state(|state| state.srcfiles.len()) >= capacity {
                with_profiling_state(|state| state.profiling_error = 1);
                return 0;
            }
            let Some(next) = used.checked_add(length).and_then(|len| len.checked_add(1)) else {
                with_profiling_state(|state| state.profiling_error = 2);
                return 0;
            };
            if next > total || bytes.is_null() {
                with_profiling_state(|state| state.profiling_error = 2);
                return 0;
            }
            if !ensure_source_slots(owner)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
            {
                with_profiling_state(|state| state.profiling_error = 2);
                return 0;
            }
            if !epoch.unchanged() {
                return 0;
            }
            // Admission above runs no R callback or collection. Both dense payloads
            // remain rooted, and no payload reference spans that arena operation.
            let name = crate::sexp::accessors::charsxp_as_utf8(filename.as_raw());
            if name.len() != length {
                with_profiling_state(|state| state.profiling_error = 2);
                return 0;
            }
            ptr::copy_nonoverlapping(name.as_ptr(), bytes.add(used), length);
            *bytes.add(next - 1) = 0;
            with_profiling_state(|state| {
                if state.srcfiles.len() == state.srcfiles.capacity() {
                    state.profiling_error = 2;
                    return 0;
                }
                state.srcfiles.push(used); // Fixed admitted capacity; cannot allocate.
                state.srcfile_bytes_used = next;
                state.line_profiling = (state.srcfiles.len() as c_int).saturating_add(1);
                state.srcfiles.len() as c_int
            })
        })
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// lineprof -- write line profiling data
// ---------------------------------------------------------------------------

/// Write line profiling data to the profiling output buffer.
///
/// Ported from R's `lineprof()` in eval.c.
fn lineprof(pb: &mut ProfileBuffer<'_>, srcref: SEXP) {
    if srcref.is_null() || srcref == R_NilValue() {
        return;
    }
    let owner = crate::sexp::owner::OwnerToken::current()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let pin = owner
        .pin()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let epoch = ProfileEpoch::capture(owner)
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let srcref = crate::sexp::context::own_control_value(srcref);
    let line = crate::mainutils::coerce::asInteger(srcref.as_raw());
    crate::sexp::context::require_context_owner_live(pin.as_ref());
    if !epoch.unchanged() {
        return;
    }
    if line == NA_INTEGER {
        return;
    }
    let srcfile_sym = crate::sexp::symbol::install_str("srcfile");
    crate::sexp::context::require_context_owner_live(pin.as_ref());
    if !epoch.unchanged() {
        return;
    }
    let srcfile = getAttrib(srcref.as_raw(), srcfile_sym);
    if !epoch.unchanged() {
        return;
    }
    if srcfile.is_null() || srcfile == R_NilValue() || TYPEOF(srcfile) != SEXPTYPE::ENVSXP {
        return;
    }
    let srcfile = crate::sexp::context::own_control_value(srcfile);
    let filename_sym = crate::sexp::symbol::install_str("filename");
    crate::sexp::context::require_context_owner_live(pin.as_ref());
    if !epoch.unchanged() {
        return;
    }
    let filename = R_findVar(filename_sym, srcfile.as_raw());
    crate::sexp::context::require_context_owner_live(pin.as_ref());
    if !epoch.unchanged() {
        return;
    }
    let filename = crate::sexp::context::own_control_value(filename);
    if filename.typeof_() != SEXPTYPE::STRSXP {
        return;
    }
    let length = filename.len();
    crate::sexp::context::require_context_owner_live(pin.as_ref());
    if !epoch.unchanged() {
        return;
    }
    if length == 0 {
        return;
    }
    let chars = crate::sexp::context::own_control_value(STRING_ELT(filename.as_raw(), 0));
    crate::sexp::context::require_context_owner_live(pin.as_ref());
    if !epoch.unchanged() {
        return;
    }
    let number = getFilenum(&chars);
    if number != 0 {
        pb.append_int(number as i64);
        pb.append(b"#");
        pb.append_int(line as i64);
        pb.append(b" ");
    }
}

// ---------------------------------------------------------------------------
// findProfContext -- find next profiling context
// ---------------------------------------------------------------------------

/// Find the next context to include in the profile trace.
///
/// When `R_Filter_Callframes` is enabled, uses a more sophisticated algorithm
/// to skip intermediate frames. Otherwise, simply returns the next context.
///
/// Ported from R's `findProfContext()` in eval.c.
unsafe fn findProfContext(cptr: *mut RCNTXT, eval_internal: SEXP) -> *mut RCNTXT {
    unsafe {
        if with_profiling_state(|state| state.filter_callframes) == 0 {
            return (*cptr).nextcontext;
        }

        let toplevel = get_R_ToplevelContext();
        if cptr == toplevel {
            return ptr::null_mut();
        }

        // Find parent context, same algorithm as in `parent.frame()`
        let parent = super::context::R_findParentContext(cptr, 1);

        // If we're in a frame called by `eval()`, find the evaluation
        // environment higher up the stack, if any.
        let mut result = parent;
        if !parent.is_null() {
            let parent_callfun = (*parent).callfun.as_raw();
            if !parent_callfun.is_null() {
                if parent_callfun == eval_internal {
                    let sysparent = (*cptr).sysparent.as_raw();
                    result = super::context::R_findExecContext((*parent).nextcontext, sysparent);
                }
            }
        }

        if !result.is_null() {
            // Native synthetic contexts can use their caller's environment.
            // Parent lookup may then identify this same frame; every profiler
            // step must still advance along the owning context chain.
            return if result == cptr {
                (*cptr).nextcontext
            } else {
                result
            };
        }

        // Base case: this interrupts the iteration over context frames
        if (*cptr).nextcontext == toplevel {
            return ptr::null_mut();
        }

        // There is no parent frame and we haven't reached the top level
        // context. Find the very first context on the stack which should
        // always be included in the profiles.
        let mut c = cptr;
        while (*c).nextcontext != toplevel && !(*c).nextcontext.is_null() {
            c = (*c).nextcontext;
        }
        c
    }
}

// ---------------------------------------------------------------------------
// write_profile_bytes -- write bytes to profile file
// ---------------------------------------------------------------------------

/// Write a byte slice to the profile output file.
///
/// On Unix, this avoids calling fprintf (signal-safe); EINTR retries.
/// Ported from R's `pf_str()` in eval.c.
#[cfg(not(target_arch = "wasm32"))]
fn write_profile_bytes(mut bytes: &[u8]) -> isize {
    let outfile = with_profiling_state(|state| state.profile_outfile);
    if outfile < 0 {
        return -1;
    }

    let mut written = 0;
    while !bytes.is_empty() {
        // SAFETY: synchronous write borrows this valid readable slice; the
        // current profiling owner supplies the fd. Returned count <= len.
        let count = unsafe { libc::write(outfile, bytes.as_ptr().cast(), bytes.len()) };
        if count < 0 {
            match std::io::Error::last_os_error().kind() {
                std::io::ErrorKind::Interrupted => continue,
                _ => return -1,
            }
        }
        if count == 0 {
            break;
        }
        let count = count as usize;
        written += count;
        bytes = &bytes[count..];
    }
    written as isize
}

/// WASM M1 no-op stub: no file-descriptor profiling output on wasm.
/// Same signature as the native writer; returns the R-level
/// 'not available on this platform' sentinel (-1) used when no
/// profile output file is open.
#[cfg(target_arch = "wasm32")]
fn write_profile_bytes(_bytes: &[u8]) -> isize {
    -1
}

// ---------------------------------------------------------------------------
// pf_int -- write integer to profile file
// ---------------------------------------------------------------------------

/// Write an integer to the profile output file.
///
/// Ported from R's `pf_int()` in eval.c.
fn pf_int(num: c_int) {
    let mut bytes = [0u8; 32];
    let mut pb = ProfileBuffer::new(&mut bytes);
    pb.append_int(num as i64);
    if let Some(bytes) = pb.as_bytes() {
        write_profile_bytes(bytes);
    }
}

// ---------------------------------------------------------------------------
// R_getCurrentSrcref -- get current source reference
// ---------------------------------------------------------------------------

/// Get the current source reference for profiling.
///
/// Ported from R's `R_getCurrentSrcref()` in eval.c.
fn R_getCurrentSrcref() -> SEXP {
    unsafe {
        let srcref = with_profiling_state(|state| state.sref.as_raw());
        let in_bc = get_R_InBCInterpreter();
        if srcref != in_bc {
            srcref
        } else {
            R_findBCInterpreterSrcref(ptr::null_mut())
        }
    }
}

// ---------------------------------------------------------------------------
// doprof -- signal handler for profiling (full implementation)
// ---------------------------------------------------------------------------

/// Render a call-stack sample on its owning execution thread.
///
/// The caller retains roots and ends arena/payload loans before entering.
/// Sampling may invoke source providers, so it is never a worker-thread or
/// signal-handler entry point. Physical pins and generations reject revoked
/// owners and discard samples retained through stop/restart callbacks.
///
/// Ported from R's `doprof()` in eval.c.
fn doprof(_sig: c_int) {
    unsafe {
        crate::sexp::owner::OwnerToken::with_current(|owner| {
            let epoch = ProfileEpoch::capture(owner)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let mut buf = [0u8; PROFBUFSIZ];
            let prevnum = with_profiling_state(|state| state.line_profiling);

            let mut pb = ProfileBuffer::new(&mut buf);

            // Memory profiling: record memory allocation sizes
            if with_profiling_state(|state| state.mem_profiling) != 0 {
                write_memory_profile_prefix(&mut pb, memory_profile_snapshot());
            }

            // GC profiling
            if with_profiling_state(|state| state.gc_profiling) != 0
                && crate::mainutils::memory_main::R_gc_running() != 0
            {
                pb.append(b"\"<GC>\" ");
            }

            let instance = with_required_current_instance(|instance| instance);
            let owner_pin = crate::sexp::context::pin_context_owner_in(instance);
            let eval_internal = if with_profiling_state(|state| state.filter_callframes) != 0 {
                let symbol = crate::sexp::symbol::install_str("eval");
                crate::sexp::context::require_context_owner_live(owner_pin.as_ref());
                if !epoch.unchanged() {
                    return;
                }
                crate::sexp::accessors::INTERNAL(symbol)
            } else {
                ptr::null_mut()
            };
            let current_srcref = with_profiling_state(|state| state.sref.owned());
            let (current_bcbody, current_relpc) =
                with_profiling_state(|state| (state.bcbody.owned(), state.bcrelpc));
            let in_bc_owner = with_profiling_state(|state| state.in_bc_interpreter.owned());
            let mut frame_count = 0usize;
            let mut cptr = super::runtime::global_context();
            while !cptr.is_null() {
                let Some(next) = frame_count.checked_add(1) else {
                    with_profiling_state(|state| state.profiling_error = 3);
                    return;
                };
                frame_count = next;
                cptr = findProfContext(cptr, eval_internal);
            }
            // Even an empty context stack uses one bounded snapshot slot before
            // line/source providers run. Failed admission skips this sample.
            let Some(mut frames) = workspace::ProfileBuffer::with_capacity(owner, frame_count.max(1))
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
            else {
                with_profiling_state(|state| state.profiling_error = 3);
                return;
            };
            let mut cptr = super::runtime::global_context();
            while !cptr.is_null() {
                let cell =
                    crate::sexp::context::retain_context_in(instance, cptr).unwrap_or_else(|| {
                        crate::sexp::context::r_error("unavailable profiling context")
                    });
                if !frames.push((
                    cell,
                    (*cptr).callflag,
                    (*cptr).call.owned(),
                    (*cptr).srcref.owned(),
                    (*cptr).bcbody.owned(),
                    (*cptr).relpc,
                )) {
                    with_profiling_state(|state| state.profiling_error = 3);
                    return;
                }
                cptr = findProfContext(cptr, eval_internal);
            }
            let in_bc = in_bc_owner
                .as_ref()
                .map_or(ptr::null_mut(), |value| value.as_raw());
            if with_profiling_state(|state| state.line_profiling) != 0 {
                let srcref = current_srcref
                    .as_ref()
                    .map_or(ptr::null_mut(), |value| value.as_raw());
                let resolved = if !in_bc.is_null() && srcref == in_bc {
                    let body_raw = current_bcbody
                        .as_ref()
                        .map_or(ptr::null_mut(), |value| value.as_raw());
                    find_bc_interpreter_srcref_from(body_raw, current_relpc)
                } else {
                    srcref
                };
                lineprof(&mut pb, resolved);
            }
            if !epoch.unchanged() {
                return;
            }
            let (frames, frame_reservation) = frames.into_parts();
            for (_cell, callflag, call_owner, srcref_owner, bcbody_owner, relpc) in frames {
                let call = call_owner
                    .as_ref()
                    .map_or(ptr::null_mut(), |value| value.as_raw());
                if (callflag & (ctxt_flags::CTXT_FUNCTION | ctxt_flags::CTXT_BUILTIN)) != 0
                    && !call.is_null()
                    && TYPEOF(call) == SEXPTYPE::LANGSXP
                {
                    let fun_owner = crate::sexp::context::own_control_value(CAR(call));
                    let fun = fun_owner.as_raw();
                    pb.append(b"\"");

                    if TYPEOF(fun) == SEXPTYPE::SYMSXP {
                        // Simple symbol: just print its name
                        pb.append(&crate::sexp::accessors::char_cstr_bytes(PRINTNAME(fun)));
                    } else if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::LANGSXP {
                        let fun_head = CAR(fun);
                        let arg1_owner = crate::sexp::context::own_control_value(CADR(fun));
                        let arg2_owner = crate::sexp::context::own_control_value(CADDR(fun));
                        let arg1 = arg1_owner.as_raw();
                        let arg2 = arg2_owner.as_raw();
                        if (fun_head == R_DoubleColonSymbol()
                            || fun_head == R_TripleColonSymbol()
                            || fun_head == R_DollarSymbol())
                            && !arg1.is_null()
                            && TYPEOF(arg1) == SEXPTYPE::SYMSXP
                            && !arg2.is_null()
                            && TYPEOF(arg2) == SEXPTYPE::SYMSXP
                        {
                            // Function accessed via ::, :::, or $
                            pb.append(&crate::sexp::accessors::char_cstr_bytes(PRINTNAME(arg1)));
                            pb.append(&crate::sexp::accessors::char_cstr_bytes(PRINTNAME(CAR(fun))));
                            pb.append(&crate::sexp::accessors::char_cstr_bytes(PRINTNAME(arg2)));
                        } else if fun_head == R_Bracket2Symbol()
                            && !arg1.is_null()
                            && TYPEOF(arg1) == SEXPTYPE::SYMSXP
                            && !arg2.is_null()
                            && (TYPEOF(arg2) == SEXPTYPE::SYMSXP
                                || TYPEOF(arg2) == SEXPTYPE::STRSXP
                                || TYPEOF(arg2) == SEXPTYPE::INTSXP
                                || TYPEOF(arg2) == SEXPTYPE::REALSXP)
                            && LENGTH(arg2) > 0
                        {
                            if !epoch.unchanged() {
                                return;
                            }
                            // Function accessed via [[
                            pb.append(&crate::sexp::accessors::char_cstr_bytes(PRINTNAME(arg1)));
                            pb.append(b"[[");

                            if TYPEOF(arg2) == SEXPTYPE::SYMSXP {
                                pb.append(&crate::sexp::accessors::char_cstr_bytes(PRINTNAME(arg2)));
                            } else if TYPEOF(arg2) == SEXPTYPE::STRSXP {
                                pb.append(b"\"");
                                let value = STRING_ELT(arg2, 0);
                                if !epoch.unchanged() {
                                    return;
                                }
                                let value = owner.node_factory().wrap(value).unwrap_or_else(|e| {
                                    crate::sexp::context::r_error(e.to_string())
                                });
                                pb.append(&crate::sexp::accessors::char_cstr_bytes(value.as_raw()));
                                pb.append(b"\"");
                            } else if TYPEOF(arg2) == SEXPTYPE::INTSXP {
                                let value = arg2_owner
                                    .try_integer_elt(0)
                                    .unwrap_or_else(|e| {
                                        crate::sexp::context::r_error(e.to_string())
                                    });
                                if !epoch.unchanged() {
                                    return;
                                }
                                pb.append_int(value as i64);
                            } else if TYPEOF(arg2) == SEXPTYPE::REALSXP {
                                let value = arg2_owner.try_real_elt(0).unwrap_or_else(|e| {
                                    crate::sexp::context::r_error(e.to_string())
                                });
                                if !epoch.unchanged() {
                                    return;
                                }
                                pb.append_double(value); // %0.f
                            }

                            pb.append(b"]]");
                        } else {
                            pb.append(b"<Anonymous>");
                        }
                    } else {
                        pb.append(b"<Anonymous>");
                    }

                    pb.append(b"\" ");

                    // Line profiling for this context
                    if with_profiling_state(|state| state.line_profiling) != 0 {
                        let srcref_val = srcref_owner
                            .as_ref()
                            .map_or(ptr::null_mut(), |value| value.as_raw());
                        if !in_bc.is_null() && srcref_val == in_bc {
                            let body_raw = bcbody_owner
                                .as_ref()
                                .map_or(ptr::null_mut(), |value| value.as_raw());
                            lineprof(&mut pb, find_bc_interpreter_srcref_from(body_raw, relpc));
                        } else {
                            lineprof(&mut pb, srcref_val);
                        }
                    }
                }
                crate::sexp::context::require_context_owner_live(owner_pin.as_ref());
                if !epoch.unchanged() {
                    return;
                }
            }

            // IntoIter has released the actual frame backing and its roots.
            drop(frame_reservation);

            // Terminate: an overflowed buffer discards the whole sample.
            let sample = pb.as_bytes();
            if sample.is_none() {
                with_profiling_state(|state| state.profiling_error = 3);
            }

            if !epoch.unchanged() {
                return;
            }
            // Write any new source file references
            let line_prof_val = with_profiling_state(|state| state.line_profiling);
            let source_buffer = with_profiling_state(|state| state.srcfiles_buffer.owned());
            let mut i = prevnum;
            while i < line_prof_val {
                write_profile_bytes(b"#File ");
                pf_int(i);
                write_profile_bytes(b": ");
                let offset =
                    with_profiling_state(|state| state.srcfiles.get((i - 1) as usize).copied());
                if let (Some(buffer), Some(offset)) = (&source_buffer, offset) {
                    let length = XLENGTH(buffer.as_raw()) as usize;
                    if !epoch.unchanged() {
                        return;
                    }
                    if offset < length {
                        let bytes = RAW(buffer.as_raw());
                        crate::sexp::context::require_context_owner_live(owner_pin.as_ref());
                        if !epoch.unchanged() {
                            return;
                        }
                        if !bytes.is_null() {
                            // SAFETY: `tail` is bounded to this RAWSXP
                            // allocation; this region runs no R callback or
                            // collection.
                            let tail =
                                std::slice::from_raw_parts(bytes.add(offset), length - offset);
                            if let Some(end) = tail.iter().position(|&byte| byte == 0) {
                                write_profile_bytes(&tail[..end]);
                            }
                        }
                    }
                }
                write_profile_bytes(b"\n");
                i += 1;
            }

            // Write the profile line
            if let Some(sample) = sample.filter(|bytes| !bytes.is_empty()) {
                write_profile_bytes(sample);
                write_profile_bytes(b"\n");
            }
        })
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// doprof_null -- null signal handler for profiling
// ---------------------------------------------------------------------------

/// Null signal handler for SIGPROF, used when profiling is being stopped.
///
/// Ported from R's `doprof_null()` in eval.c.
fn doprof_null(_sig: c_int) {
    // Just reinstall the handler
    // In C: signal(SIGPROF, doprof_null);
    // In Rust port, signal handling is managed differently
}

// ---------------------------------------------------------------------------
// R_EndProfiling -- stop profiling (full implementation)
// ---------------------------------------------------------------------------

/// Stop profiling and close the output file.
///
/// This follows R's `R_EndProfiling()` state transition, but deliberately does
/// not manipulate process-global profiling timers. Rooted interpreter boundaries
/// and explicit `R_WriteProfile` calls service the owning runtime, including on
/// single-threaded Android hosts.
#[cfg(not(target_arch = "wasm32"))]
fn R_EndProfiling() {
    with_profiling_state(|state| {
        state.profile_generation = state.profile_generation.wrapping_add(1);
        state.profile_schedule = None;
        state.sampling_in_progress = false;
    });
    // Close the output file; `outfile` is the live fd this instance opened.
    let outfile = with_profiling_state(|state| state.profile_outfile);
    if outfile >= 0 {
        // SAFETY: `outfile` is the profiling fd owned by this instance.
        unsafe {
            libc::close(outfile);
        }
        with_profiling_state(|state| state.profile_outfile = -1);
    }

    // Reset state
    with_profiling_state(|state| {
        state.profiling = 0;
        state.mem_profiling = 0;
        state.gc_profiling = 0;
        state.line_profiling = 0;
    });

    // Clear derived offsets before releasing the canonical buffer owner; the
    // displaced reservation and root drop after the lend so provider releases
    // cannot reenter the borrow.
    let displaced = with_profiling_state(|state| {
        state.srcfiles = Vec::new();
        state.srcfile_bytes_used = 0;
        (
            state.srcfiles_reservation.take(),
            std::mem::take(&mut state.srcfiles_buffer),
        )
    });
    // Read any pending error before releasing the displaced roots, so no
    // ambient owner is consulted after provider releases have run.
    let err = with_profiling_state(|state| state.profiling_error);
    drop(displaced);

    if err != 0 {
        if err == 3 {
            // Samples too large for I/O buffer skipped
        } else if err == 1 {
            // Too many source files
        } else if err == 2 {
            // Buffer space exhausted
        }
    }
}

/// WASM M1 no-op stub: no file-descriptor profiling output on wasm.
/// Same signature as the native stop routine; resets the session-local
/// profiling flags (no fd to close since the wasm `R_InitProfiling`
/// stub never opens one).
#[cfg(target_arch = "wasm32")]
fn R_EndProfiling() {
    with_profiling_state(|state| {
        state.profile_generation = state.profile_generation.wrapping_add(1);
        state.profile_schedule = None;
        state.sampling_in_progress = false;
    });
    // Reset state
    with_profiling_state(|state| {
        state.profiling = 0;
        state.mem_profiling = 0;
        state.gc_profiling = 0;
        state.line_profiling = 0;
    });

    // Clear derived offsets before releasing the canonical buffer owner.
    let displaced = with_profiling_state(|state| {
        state.srcfiles = Vec::new();
        state.srcfile_bytes_used = 0;
        (
            state.srcfiles_reservation.take(),
            std::mem::take(&mut state.srcfiles_buffer),
        )
    });
    drop(displaced);
}

// ---------------------------------------------------------------------------
// R_InitProfiling -- initialize profiling (full implementation)
// ---------------------------------------------------------------------------

/// Initialize profiling with the given parameters.
///
/// Opens the output file and enables session-local profiling.
///
/// R's C runtime uses process-global signals/timers for automatic sampling.
/// This Rust/Android port avoids those globals so multiple `RSession`s can run
/// in parallel without stealing each other's profiling timer. CPU/elapsed
/// deadlines are serviced on the owner thread; `R_WriteProfile` also supports
/// an explicit sample without creating a worker or installing a signal.
#[cfg(not(target_arch = "wasm32"))]
fn R_InitProfiling(
    filename: SEXP,
    append: c_int,
    dinterval: c_double,
    mem_profiling: c_int,
    gc_profiling: c_int,
    line_profiling: c_int,
    filter_callframes: c_int,
    numfiles: c_int,
    bufsize: c_int,
    event: rpe_type,
) {
    unsafe {
        // If already profiling, stop first
        if with_profiling_state(|state| state.profile_outfile) >= 0 {
            R_EndProfiling();
        }

        // Open the output file (Unix path)
        if filename.is_null() {
            return;
        }

        let fn_str = CHAR(filename);
        if fn_str.is_null() {
            return;
        }

        let flags = if append != 0 {
            libc::O_CREAT | libc::O_WRONLY | libc::O_APPEND
        } else {
            libc::O_CREAT | libc::O_WRONLY | libc::O_TRUNC
        };
        let mode: u32 = libc::S_IRUSR as u32
            | libc::S_IWUSR as u32
            | libc::S_IRGRP as u32
            | libc::S_IWGRP as u32
            | libc::S_IROTH as u32
            | libc::S_IWOTH as u32;

        let fd = libc::open(fn_str, flags, mode);
        if fd < 0 {
            return;
        }
        with_profiling_state(|state| {
            state.profile_outfile = fd;
            state.profile_generation = state.profile_generation.wrapping_add(1);
            state.profile_schedule = None;
            state.sampling_in_progress = false;
        });
        crate::sexp::owner::OwnerToken::with_current(|owner| {
            let epoch = ProfileEpoch::capture(owner)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));

            let interval: c_int = (1e6 * dinterval + 0.5) as c_int;

            // Write header line
            if mem_profiling != 0 {
                write_profile_bytes(b"memory profiling: ");
            }
            if gc_profiling != 0 {
                write_profile_bytes(b"GC profiling: ");
            }
            if line_profiling != 0 {
                write_profile_bytes(b"line profiling: ");
            }
            write_profile_bytes(b"sample.interval=");
            pf_int(interval);
            write_profile_bytes(b"\n");

            // Set profiling state
            with_profiling_state(|state| {
                state.mem_profiling = mem_profiling;
                state.memory_peak_bytes = 0;
                state.profiling_error = 0;
                state.line_profiling = line_profiling;
                state.gc_profiling = gc_profiling;
                state.filter_callframes = filter_callframes;
            });

            // Filenames use safe byte offsets; the buffer never stores native
            // pointers or relies on alignment suitable for a pointer array.
            if line_profiling != 0 {
                let count = usize::try_from(numfiles)
                    .ok()
                    .filter(|count| *count > 0)
                    .unwrap_or_else(|| {
                        crate::sexp::context::r_error("invalid source file capacity")
                    });
                if bufsize < 0 {
                    crate::sexp::context::r_error("invalid source filename buffer size");
                }
                let instance = with_required_current_instance(|instance| instance);
                let pin = crate::sexp::context::pin_context_owner_in(instance);
                let buffer = crate::sexp::instance::RuntimeValue::from_raw_in(
                    instance,
                    Rf_allocVector(SEXPTYPE::RAWSXP, bufsize),
                );
                crate::sexp::context::require_context_owner_live(pin.as_ref());
                if !epoch.unchanged() {
                    return;
                }
                let displaced = with_profiling_state(|state| {
                    state.srcfiles = Vec::new();
                    state.srcfile_bytes_used = 0;
                    state.srcfile_bufcount = count;
                    (
                        state.srcfiles_reservation.take(),
                        std::mem::replace(&mut state.srcfiles_buffer, buffer),
                    )
                });
                drop(displaced);
            }

            if !epoch.unchanged() {
                return;
            }
            let schedule = ProfileSchedule::new(interval, event);
            with_profiling_state(|state| {
                state.profiling_event = profiling_event_code(event);
                state.profile_schedule = schedule;
                state.profiling = 1;
            });
        })
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    }
}

/// WASM M1 no-op stub: no setitimer/SIGPROF/file-IO profiling on wasm.
/// Same signature as the native initializer; leaves the R-level
/// 'not available on this platform' state (profiling stays off,
/// output fd stays closed) so callers observe the neighboring-stub
/// behavior of returning `R_NilValue()` without starting profiling.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
fn R_InitProfiling(
    _filename: SEXP,
    _append: c_int,
    _dinterval: c_double,
    _mem_profiling: c_int,
    _gc_profiling: c_int,
    _line_profiling: c_int,
    _filter_callframes: c_int,
    _numfiles: c_int,
    _bufsize: c_int,
    _event: rpe_type,
) {
}

// ---------------------------------------------------------------------------
// do_Rprof -- Rprof() builtin (full implementation)
// ---------------------------------------------------------------------------

/// Implement the `Rprof()` function.
///
/// When called with a non-empty filename, starts profiling to that file.
/// When called with an empty filename, stops profiling.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_Rprof(call: SEXP, op: SEXP, mut args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::sexp::owner::OwnerToken::with_current(|owner| {
            let _pin = owner
                .pin()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let factory = owner.node_factory();
            let _arguments = factory
                .wrap(args)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let require_owner = || {
                owner
                    .require_active()
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
            };
            // BC profiling check
            if with_profiling_state(|state| state.bc_profiling) != 0 {
                // Cannot use R profiling while byte code profiling
                return R_NilValue();
            }

            // Parse arguments
            let filename_arg = CAR(args);
            if filename_arg.is_null() {
                return R_NilValue();
            }

            // Get filename string
            if TYPEOF(filename_arg) != SEXPTYPE::STRSXP {
                return R_NilValue();
            }
            let filename_count = LENGTH(filename_arg);
            require_owner();
            if filename_count != 1 {
                return R_NilValue();
            }

            let filename_sexp = STRING_ELT(filename_arg, 0);
            require_owner();
            let _filename = factory
                .wrap(filename_sexp)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            args = CDR(args);

            let append_mode = crate::mainutils::coerce::asLogical(CAR(args));
            require_owner();
            args = CDR(args);

            let dinterval = crate::mainutils::coerce::asReal(CAR(args));
            require_owner();
            args = CDR(args);

            let mem_profiling = crate::mainutils::coerce::asLogical(CAR(args));
            require_owner();
            args = CDR(args);

            let gc_profiling = crate::mainutils::coerce::asLogical(CAR(args));
            require_owner();
            args = CDR(args);

            let line_profiling = crate::mainutils::coerce::asLogical(CAR(args));
            require_owner();
            args = CDR(args);

            let filter_callframes = crate::mainutils::coerce::asLogical(CAR(args));
            require_owner();
            args = CDR(args);

            let numfiles = crate::mainutils::coerce::asInteger(CAR(args));
            require_owner();
            args = CDR(args);

            let bufsize = crate::mainutils::coerce::asInteger(CAR(args));
            require_owner();
            args = CDR(args);

            // Get event type argument
            let event_arg_sexp = CAR(args);
            let event_count =
                if !event_arg_sexp.is_null() && TYPEOF(event_arg_sexp) == SEXPTYPE::STRSXP {
                    let count = LENGTH(event_arg_sexp);
                    require_owner();
                    count
                } else {
                    0
                };
            let event = if event_count == 1 {
                let event_value = STRING_ELT(event_arg_sexp, 0);
                require_owner();
                let event_value = factory
                    .wrap(event_value)
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                let event_str = CHAR(event_value.as_raw());
                if !event_str.is_null() {
                    let bytes = CStr::from_ptr(event_str);
                    match bytes.to_bytes() {
                        b"cpu" | b"default" => rpe_type::RPE_CPU,
                        b"elapsed" => rpe_type::RPE_ELAPSED,
                        _ => rpe_type::RPE_CPU,
                    }
                } else {
                    rpe_type::RPE_CPU
                }
            } else {
                rpe_type::RPE_CPU
            };

            // Check if filename is non-empty
            let filename_len = LENGTH(filename_sexp);
            if filename_len > 0 {
                R_InitProfiling(
                    filename_sexp,
                    append_mode,
                    dinterval,
                    mem_profiling,
                    gc_profiling,
                    line_profiling,
                    filter_callframes,
                    numfiles,
                    bufsize,
                    event,
                );
            } else {
                R_EndProfiling();
            }

            require_owner();
            R_NilValue()
        })
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    }
}
/// WASM M1 stub for `Rprof()`: no timer/file-descriptor profiler on wasm.
/// Starting profiling raises the catchable R platform-unavailable error
/// (matching upstream's `error(_("R profiling is not available on this system"))`
/// fallback). Stopping with an empty filename stays a silent no-op.
#[cfg(any(target_arch = "wasm32", test))]
pub fn do_Rprof_wasm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::sexp::owner::OwnerToken::with_current(|owner| {
            let _pin = owner
                .pin()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let factory = owner.node_factory();
            let arguments = factory
                .wrap(args)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let require_owner = || {
                owner
                    .require_active()
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
            };
            let filename_arg = CAR(arguments.as_raw());
            if filename_arg.is_null()
                || filename_arg == R_NilValue()
                || TYPEOF(filename_arg) != SEXPTYPE::STRSXP
            {
                return R_NilValue();
            }
            let count = LENGTH(filename_arg);
            require_owner();
            if count != 1 {
                return R_NilValue();
            }
            let filename = STRING_ELT(filename_arg, 0);
            require_owner();
            let filename = factory
                .wrap(filename)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let length = LENGTH(filename.as_raw());
            require_owner();
            if length == 0 {
                return R_NilValue();
            }
            crate::sexp::context::r_error("R profiling is not available on this platform")
        })
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// do_Rprof_mem -- Rprofmem() builtin
// ---------------------------------------------------------------------------

/// Implement the `Rprofmem()` function for memory profiling.
#[cfg(not(target_arch = "wasm32"))]
pub fn do_Rprofmem(_call: SEXP, _op: SEXP, mut args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::owner::OwnerToken::with_current(|owner| {
        let _pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let factory = owner.node_factory();
        let _arguments = factory
            .wrap(args)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let require_owner = || {
            owner
                .require_active()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        };
        let filename_arg = if args.is_null() || args == R_NilValue() {
            crate::sexp::constructors::mk_string_str("Rprofmem.out")
        } else {
            let arg = CAR(args);
            args = CDR(args);
            arg
        };

        require_owner();
        let _filename_arg = factory
            .wrap(filename_arg)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        if filename_arg.is_null() || TYPEOF(filename_arg) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let count = LENGTH(filename_arg);
        require_owner();
        if count != 1 {
            return R_NilValue();
        }

        let append_mode = if args.is_null() || args == R_NilValue() {
            FALSE
        } else {
            crate::mainutils::coerce::asLogical(CAR(args))
        };
        require_owner();
        let filename_sexp = STRING_ELT(filename_arg, 0);
        require_owner();
        let _filename = factory
            .wrap(filename_sexp)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));

        if LENGTH(filename_sexp) > 0 {
            R_InitProfiling(
                filename_sexp,
                append_mode,
                0.02,
                TRUE,
                FALSE,
                FALSE,
                FALSE,
                100,
                PROFBUFSIZ as c_int,
                rpe_type::RPE_ELAPSED,
            );
        } else {
            R_EndProfiling();
        }

        require_owner();
        // Retain the existing explicit memory-sample boundary; Rprofmem is not
        // a periodic CPU/elapsed Rprof session.
        with_profiling_state(|state| state.profile_schedule = None);
        R_NilValue()
    })
    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}
/// WASM M1 stub for `Rprofmem()`: no memory profiler on wasm.
/// Always raises the catchable R platform-unavailable error (matching
/// upstream's `error(_("memory profiling is not available on this system"))`
/// fallback) instead of silently returning success.
#[cfg(target_arch = "wasm32")]
pub fn do_Rprofmem_wasm(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("memory profiling is not available on this platform")
}

// ---------------------------------------------------------------------------
// do_Rprofaddr -- Rprofaddr() builtin
// ---------------------------------------------------------------------------

/// Implement the `Rprofaddr()` function for address profiling.
#[cfg(not(target_arch = "wasm32"))]
pub fn do_Rprofaddr(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    R_NilValue()
}
/// WASM M1 stub for `Rprofaddr()`: no address profiler on wasm.
/// Raises the catchable R platform-unavailable error instead of silently
/// returning success.
#[cfg(target_arch = "wasm32")]
pub fn do_Rprofaddr(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("address profiling is not available on this platform")
}
/// Implement the `gcprof()` function for GC profiling.
#[cfg(not(target_arch = "wasm32"))]
pub fn do_gcprof(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    R_NilValue()
}
/// WASM M1 stub for `gcprof()`: no GC profiler on wasm.
/// Raises the catchable R platform-unavailable error instead of silently
/// returning success.
#[cfg(target_arch = "wasm32")]
pub fn do_gcprof(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("GC profiling is not available on this platform")
}

// ---------------------------------------------------------------------------
// dobcprof -- BC profiling signal handler
// ---------------------------------------------------------------------------

/// Bytecode profiling signal handler.
///
/// Records the current bytecode opcode when the profiling timer fires.
///
/// Ported from R's `dobcprof()` in eval.c.
fn dobcprof(_sig: c_int) {
    let op = with_profiling_state(|state| state.current_opcode);
    if op >= 0 && (op as usize) < OPCOUNT {
        with_profiling_state(|state| state.opcode_counts[op as usize] += 1);
    }
    // Reinstall handler: signal(SIGPROF, dobcprof);
}

// ---------------------------------------------------------------------------
// dobcprof_null -- null BC profiling signal handler
// ---------------------------------------------------------------------------

/// Null signal handler for BC profiling, used when BC profiling stops.
///
/// Ported from R's `dobcprof_null()` in eval.c.
fn dobcprof_null(_sig: c_int) {
    // Just reinstall: signal(SIGPROF, dobcprof_null);
}

// ---------------------------------------------------------------------------
// do_bcprofstart -- start bytecode profiling
// ---------------------------------------------------------------------------

/// Start bytecode profiling.
///
/// Sets up the profiling timer and initializes opcode counts.
///
/// Ported from R's `do_bcprofstart()` in eval.c.
#[cfg(not(target_arch = "wasm32"))]
pub fn do_bcprofstart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let dinterval: c_double = 0.02;
        let interval: c_int = (1e6 * dinterval + 0.5) as c_int;

        if with_profiling_state(|state| state.profiling) != 0 {
            // Profile timer in use
            return R_NilValue();
        }
        if with_profiling_state(|state| state.bc_profiling) != 0 {
            // Already byte code profiling
            return R_NilValue();
        }

        // Initialize the profile data
        with_profiling_state(|state| {
            state.current_opcode = NO_CURRENT_OPCODE;
            state.opcode_counts.fill(0);
        });

        // Set up the timer
        let it_interval = libc::timeval {
            tv_sec: (interval as i64 / 1000000) as libc::time_t,
            tv_usec: (interval - (interval / 1000000) * 1000000) as libc::suseconds_t,
        };
        let itv = libc::itimerval {
            it_interval,
            it_value: it_interval,
        };
        if libc::setitimer(PROF_TIMER, &itv, ptr::null_mut()) == -1 {
            return R_NilValue();
        }

        with_profiling_state(|state| state.bc_profiling = 1);

        R_NilValue()
    }
}

/// WASM M1 stub: no setitimer/SIGPROF bytecode profiling on wasm.
/// Same signature as the native starter; raises the catchable R
/// platform-unavailable error (matching upstream's
/// `error(_("byte code profiling is not supported in this build"))`
/// fallback) instead of silently returning success.
#[cfg(target_arch = "wasm32")]
pub fn do_bcprofstart(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("byte code profiling is not supported on this platform")
}

// ---------------------------------------------------------------------------
// do_bcprofstop -- stop bytecode profiling
// ---------------------------------------------------------------------------

/// Stop bytecode profiling.
///
/// Disables the profiling timer.
///
/// Ported from R's `do_bcprofstop()` in eval.c.
#[cfg(not(target_arch = "wasm32"))]
pub fn do_bcprofstop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if with_profiling_state(|state| state.bc_profiling) == 0 {
            // Not byte code profiling
            return R_NilValue();
        }

        let zero_val = libc::itimerval {
            it_interval: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        };
        libc::setitimer(PROF_TIMER, &zero_val, ptr::null_mut());

        with_profiling_state(|state| state.bc_profiling = 0);

        R_NilValue()
    }
}

/// WASM M1 stub: no setitimer/SIGPROF bytecode profiling on wasm.
/// Same signature as the native stopper; raises the catchable R
/// platform-unavailable error (matching upstream's
/// `error(_("byte code profiling is not supported in this build"))`
/// fallback) instead of silently returning success.
#[cfg(target_arch = "wasm32")]
pub fn do_bcprofstop(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("byte code profiling is not supported on this platform")
}

// ---------------------------------------------------------------------------
// do_bcprofcounts -- get bytecode profiling counts
// ---------------------------------------------------------------------------

/// Get bytecode profiling opcode counts.
///
/// Returns an integer vector of opcode counts.
///
/// Ported from R's `do_bcprofcounts()` in eval.c.
#[cfg(not(target_arch = "wasm32"))]
pub fn do_bcprofcounts(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let owner = current_profiler_owner();
    let val = Rf_allocVector(SEXPTYPE::INTSXP, OPCOUNT as c_int);
    if val.is_null() {
        return R_NilValue();
    }
    let _root = crate::sexp::protect::protect(val);
    let counts = owner.with_profiling_state(|state| state.opcode_counts);
    for (i, count) in counts.into_iter().enumerate() {
        crate::sexp::accessors::SET_INTEGER_ELT(val, i as c_int, count);
    }
    val
}
/// WASM M1 stub for `bcprofcounts()`: no bytecode profiler state on wasm.
/// Raises the catchable R platform-unavailable error (matching upstream's
/// `error(_("byte code profiling is not supported in this build"))`
/// fallback) instead of returning a silent zero vector.
#[cfg(target_arch = "wasm32")]
pub fn do_bcprofcounts(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("byte code profiling is not supported on this platform")
}

// ---------------------------------------------------------------------------
// R_WriteProfile -- write profiling output
// ---------------------------------------------------------------------------

/// Write current profiling sample to the output file.
///
/// Ported from R's `R_WriteProfile()` in eval.c.
pub fn R_WriteProfile(_out: c_int) {
    crate::sexp::owner::OwnerToken::with_current(|owner| {
        write_profile_sample(owner, true)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    })
    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}

// ---------------------------------------------------------------------------
// bc_check_sigint -- check for user interrupts in bytecode loop
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::sexp::RSession;
    use crate::sexp::constructors::{
        Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_cons, Rf_mkString,
    };
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::instance::RInstance;
    use crate::sexp::memory::with_arena;

    fn r_string(text: &str) -> SEXP {
        let c_text = std::ffi::CString::new(text).expect("test string without interior nul");
        unsafe { crate::sexp::constructors::mk_string_bytes(c_text.to_bytes()) }
    }

    fn pairlist(values: &[SEXP]) -> SEXP {
        values
            .iter()
            .rev()
            .fold(R_NilValue(), |tail, value| Rf_cons(*value, tail))
    }

    fn unique_profile_path(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("{prefix}-{}-{nanos}.out", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn owned_profile_filename_offsets_respect_exact_capacity_and_release_buffer() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let buffer = owner
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 6)))
                .unwrap();
            let node = crate::sexp::memory::checked_projection(buffer.as_raw())
                .unwrap()
                .1;
            let value = crate::sexp::instance::RuntimeValue::from_raw_in(instance, buffer.as_raw());
            with_profiling_state(|state| {
                state.srcfiles_buffer = value;
                state.line_profiling = 1;
                state.srcfile_bufcount = 1;
            });
            drop(buffer);
            owner.full_gc().unwrap();
            assert!(node.is_live());
            let abcde = owner.node_factory().character("abcde").unwrap();
            assert_eq!(getFilenum(&abcde), 1);
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                std::mem::size_of::<usize>()
            );
            assert_eq!(getFilenum(&abcde), 1);
            assert_eq!(with_profiling_state(|state| state.srcfiles.clone()), [0]);
            assert_eq!(with_profiling_state(|state| state.srcfile_bytes_used), 6);
            assert_eq!(getFilenum(&owner.node_factory().character("x").unwrap()), 0);
            assert_eq!(with_profiling_state(|state| state.profiling_error), 1);
            with_profiling_state(|state| {
                state.srcfile_bufcount = 2;
                state.profiling_error = 0;
            });
            assert_eq!(getFilenum(&owner.node_factory().character("x").unwrap()), 0);
            assert_eq!(with_profiling_state(|state| state.profiling_error), 2);
            assert_eq!(getFilenum(&abcde), 1);
            R_EndProfiling();
            owner.full_gc().unwrap();
            assert!(!node.is_live());
            with_profiling_state(|state| {
                assert!(state.srcfiles_buffer.is_null());
                assert!(state.srcfiles.is_empty());
                assert_eq!(state.srcfiles.capacity(), 0);
                assert!(state.srcfiles_reservation.is_none());
                assert_eq!(state.srcfile_bytes_used, 0);
            });
        });
    }

    #[test]
    fn owned_line_profile_survives_collecting_active_filename_binding() {
        use std::{cell::Cell, rc::Rc};
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let factory = crate::sexp::object::SessionNodeFactory::new(owner);
            let nil = R_NilValue();
            crate::eval::jit::set_R_jit_enabled(0);
            let srcfile = factory
                .wrap(crate::sexp::memory_ext::NewEnvironment(
                    nil,
                    nil,
                    crate::sexp::globals::R_GlobalEnv(),
                ))
                .unwrap();
            let function_expression = owner
                .with_arena(|arena| {
                    crate::eval::parser::parse(
                        "function() { 1L + 2L; \"script.R\" }",
                        arena,
                        factory.clone(),
                    )
                })
                .unwrap()
                .unwrap();
            let function = factory
                .wrap(crate::eval::eval::Rf_eval(
                    function_expression.as_raw(),
                    crate::sexp::globals::R_GlobalEnv(),
                ))
                .unwrap();
            let filename_symbol = crate::sexp::symbol::install_str("filename");
            let srcfile_symbol = crate::sexp::symbol::install_str("srcfile");
            crate::sexp::envir::make_active_binding_raw(
                srcfile.as_raw(),
                filename_symbol,
                function.as_raw(),
            );
            let srcref = factory.wrap(Rf_ScalarInteger(71)).unwrap();
            let pointer = srcref.as_raw();
            let node = crate::sexp::memory::checked_projection(pointer).unwrap().1;
            crate::sexp::attrib_core::setAttrib(pointer, srcfile_symbol, srcfile.as_raw());
            let owned_srcref =
                crate::sexp::instance::RuntimeValue::from_raw_in(instance, pointer);
            with_profiling_state_in(instance, |state| state.sref = owned_srcref);
            let buffer = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 64)))
                .unwrap();
            let buffer =
                crate::sexp::instance::RuntimeValue::from_raw_in(instance, buffer.as_raw());
            with_profiling_state(|state| {
                state.srcfiles_buffer = buffer;
                state.line_profiling = 1;
                state.srcfile_bufcount = 4;
            });
            drop(srcref);
            drop(srcfile);
            drop(function);
            drop(function_expression);
            let observed = Rc::new(Cell::new(false));
            let called = observed.clone();
            let node_observed = node.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if called.replace(true) {
                    return;
                }
                drop(with_profiling_state_in(instance, |state| {
                    std::mem::take(&mut state.sref)
                }));
                crate::sexp::attrib_core::setAttrib(pointer, srcfile_symbol, R_NilValue());
                (*instance).context_stack.clear();
                crate::sexp::gengc::full_gc_in(instance);
                assert!(node_observed.is_live());
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let mut bytes = [0_u8; 32];
            let mut output = ProfileBuffer::new(&mut bytes);
            lineprof(&mut output, pointer);
            (*instance).memory_state.gc_force_gap = 0;
            assert!(observed.get());
            assert_eq!(
                std::str::from_utf8(output.as_bytes().unwrap()).unwrap(),
                "1#71 "
            );
            assert_eq!(with_profiling_state(|state| state.srcfiles.len()), 1);
            owner.full_gc().unwrap();
            assert!(!node.is_live());
            R_EndProfiling();
        });
    }

    #[test]
    fn profiling_flags_are_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| {
            with_profiling_state(|state| {
                state.profiling = 1;
                state.bc_profiling = 1;
                state.current_opcode = 7;
                state.opcode_counts[7] = 11;
            });
            assert_eq!(R_Profiling_active(), 1);
            assert_eq!(R_isRprofiling(), 1);
        });

        right.with_protected(|| {
            assert_eq!(R_Profiling_active(), 0);
            assert_eq!(R_isRprofiling(), 0);
            with_profiling_state(|state| {
                assert_eq!(state.bc_profiling, 0);
                assert_eq!(state.current_opcode, NO_CURRENT_OPCODE);
                assert_eq!(state.opcode_counts[7], 0);
            });
        });

        left.with_protected(|| {
            with_profiling_state(|state| {
                assert_eq!(state.bc_profiling, 1);
                assert_eq!(state.current_opcode, 7);
                assert_eq!(state.opcode_counts[7], 11);
            });
        });
    }

    #[test]
    fn profiling_flags_can_target_instance_explicitly() {
        let left = RSession::new_for_gc_tests();
        let right = RSession::new_for_gc_tests();
        let left_owner = left.owner_token().unwrap();
        let right_owner = right.owner_token().unwrap();

        left_owner.with_profiling_state(|state| {
            state.profiling = 1;
            state.bc_profiling = 1;
            state.current_opcode = 9;
            state.opcode_counts[9] = 17;
        });
        assert_eq!(left_owner.with_profiling_state(|state| state.profiling), 1);
        assert_eq!(right_owner.with_profiling_state(|state| state.profiling), 0);
        right_owner.with_profiling_state(|state| {
            assert_eq!(state.bc_profiling, 0);
            assert_eq!(state.current_opcode, NO_CURRENT_OPCODE);
            assert_eq!(state.opcode_counts[9], 0);
        });
    }

    #[test]
    fn profiling_state_reentrant_borrow_panics_and_recovers() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owner.with_profiling_state(|_| owner.with_profiling_state(|_| ()))
        }));
        assert!(denied.is_err());
        owner.with_profiling_state(|state| state.line_profiling = 7);
        assert_eq!(owner.with_profiling_state(|state| state.line_profiling), 7);
    }

    #[test]
    fn profiling_lend_keeps_owner_alive_through_revocation() {
        let session = RSession::new_for_gc_tests();
        let owner = session.with_active(|| OwnerToken::current().unwrap());
        let weak = owner.weak_owner().unwrap();
        owner.with_profiling_state(|state| {
            drop(session);
            assert!(!owner.is_live());
            assert_eq!(weak.allocation_strong_count(), 1);
            state.profiling = 1;
            assert_eq!(state.profiling, 1);
        });
        assert_eq!(weak.allocation_strong_count(), 0);
        assert!(owner.profiling_scheduled().is_err());
    }

    #[test]
    fn bytecode_profiler_samples_current_session_only() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| {
            with_profiling_state(|state| {
                state.current_opcode = 3;
                state.opcode_counts.fill(0);
            });
            dobcprof(0);
            with_profiling_state(|state| assert_eq!(state.opcode_counts[3], 1));
        });

        right.with_protected(|| {
            with_profiling_state(|state| {
                assert_eq!(state.opcode_counts[3], 0);
                state.current_opcode = 3;
            });
            dobcprof(0);
            with_profiling_state(|state| assert_eq!(state.opcode_counts[3], 1));
        });

        left.with_protected(|| {
            with_profiling_state(|state| assert_eq!(state.opcode_counts[3], 1));
        });
    }

    #[test]
    fn memory_profile_snapshot_uses_current_session_arena() {
        let left = RSession::new();
        let right = RSession::new();

        let left_after = left.with_protected(|| {
            let before = memory_profile_snapshot();
            unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ with_arena(|arena| {
                arena.alloc_vector(SEXPTYPE::REALSXP, 128);
                arena.alloc_charsxp(b"profile-left");
            }) };
            let after = memory_profile_snapshot();
            assert!(after.current_bytes > before.current_bytes);
            assert!(after.peak_bytes >= after.current_bytes);
            assert!(after.active_nodes > before.active_nodes);
            after
        });

        right.with_protected(|| {
            let right_snapshot = memory_profile_snapshot();
            assert!(right_snapshot.current_bytes < left_after.current_bytes);
            assert!(right_snapshot.active_nodes < left_after.active_nodes);
        });
    }

    #[test]
    fn memory_profile_snapshot_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        // SAFETY: `inst` is the live current-instance pointer handed to this accessor closure (or owned by this caller) for the duration of the call.
        let before = unsafe { memory_profile_snapshot_in(&mut left as *mut RInstance) };
        left.arena.alloc_vector(SEXPTYPE::REALSXP, 64);
        left.arena.alloc_charsxp(b"profile-explicit-left");
        // SAFETY: `inst` is the live current-instance pointer handed to this accessor closure (or owned by this caller) for the duration of the call.
        let after = unsafe { memory_profile_snapshot_in(&mut left as *mut RInstance) };
        // SAFETY: `inst` is the live current-instance pointer handed to this accessor closure (or owned by this caller) for the duration of the call.
        let right_snapshot = unsafe { memory_profile_snapshot_in(&mut right as *mut RInstance) };

        assert!(after.current_bytes > before.current_bytes);
        assert!(after.active_nodes > before.active_nodes);
        assert!(after.peak_bytes >= after.current_bytes);
        assert!(right_snapshot.current_bytes < after.current_bytes);
        assert!(right_snapshot.active_nodes < after.active_nodes);
    }

    #[test]
    fn memory_profile_prefix_writes_real_snapshot_values() {
        let _session = RSession::new();
        unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                arena.alloc_vector(SEXPTYPE::INTSXP, 64);
            })
        };
        let snapshot = memory_profile_snapshot();

        let mut buf = [0u8; 128];
        let mut pb = ProfileBuffer::new(&mut buf);
        write_memory_profile_prefix(&mut pb, snapshot);

        let text = std::str::from_utf8(pb.as_bytes().unwrap())
            .expect("profile prefix should be utf8");
        let fields: Vec<u64> = text
            .trim_matches(':')
            .split(':')
            .map(|field| field.parse::<u64>().expect("numeric profile field"))
            .collect();

        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], snapshot.current_bytes);
        assert_eq!(fields[1], snapshot.peak_bytes);
        assert_eq!(fields[2], snapshot.active_nodes);
        assert_eq!(fields[3], snapshot.gc_freed_nodes);
        assert!(fields[0] > 0);
        assert!(fields[1] >= fields[0]);
        assert!(fields[2] > 0);
    }

    #[test]
    fn profile_buffer_keeps_exact_bytes_and_discards_overflow() {
        let mut storage = [0u8; 64];
        let mut p = ProfileBuffer::new(&mut storage);
        p.append_int(i64::MIN);
        p.append(b" ");
        p.append_integer(u64::MAX, false);
        p.append(b" ");
        p.append_double(-73.9);
        assert_eq!(
            p.as_bytes(),
            Some(b"-9223372036854775808 18446744073709551615 -73".as_slice())
        );

        let mut storage = [0u8; 4];
        let mut p = ProfileBuffer::new(&mut storage);
        p.append(b"abc");
        assert_eq!(p.as_bytes(), Some(b"abc".as_slice()));
        p.append(b"d");
        p.append(b"");
        assert!(p.as_bytes().is_none());
    }

    #[test]
    fn public_rprof_wrapper_uses_session_profiler() {
        let session = RSession::new();
        session.with_protected(|| {
            let path = unique_profile_path("rport-rprof");
            let args = pairlist(&[
                r_string(&path),
                Rf_ScalarLogical(FALSE),
                Rf_ScalarReal(0.02),
                Rf_ScalarLogical(FALSE),
                Rf_ScalarLogical(FALSE),
                Rf_ScalarLogical(FALSE),
                Rf_ScalarLogical(FALSE),
                Rf_ScalarInteger(100),
                Rf_ScalarInteger(PROFBUFSIZ as c_int),
                r_string("elapsed"),
            ]);

            crate::mainutils::essentials::do_Rprof(
                ptr::null_mut(),
                ptr::null_mut(),
                args,
                R_NilValue(),
            );

            with_profiling_state(|state| {
                assert_eq!(state.profiling, 1);
                assert_eq!(state.mem_profiling, 0);
                assert!(state.profile_outfile >= 0);
            });

            let stop_args = pairlist(&[r_string("")]);
            crate::mainutils::essentials::do_Rprof(
                ptr::null_mut(),
                ptr::null_mut(),
                stop_args,
                R_NilValue(),
            );

            with_profiling_state(|state| {
                assert_eq!(state.profiling, 0);
                assert_eq!(state.profile_outfile, -1);
            });

            let contents = fs::read_to_string(&path).expect("profile file");
            assert!(contents.contains("sample.interval=20000"));
            let _ = fs::remove_file(path);
        });
    }

    #[test]
    fn public_rprofmem_wrapper_writes_session_memory_sample() {
        let session = RSession::new();
        session.with_protected(|| {
            let path = unique_profile_path("rport-rprofmem");
            let args =  pairlist(&[r_string(&path), Rf_ScalarLogical(FALSE)]) ;

            
                crate::mainutils::essentials::do_Rprofmem(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    args,
                    R_NilValue(),
                );
            
            with_profiling_state(|state| {
                assert_eq!(state.profiling, 1);
                assert_eq!(state.mem_profiling, 1);
                assert!(state.profile_outfile >= 0);
            });

            unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ with_arena(|arena| {
                arena.alloc_vector(SEXPTYPE::INTSXP, 128);
            }) };
            assert!(with_profiling_state(|state| state.profile_schedule.is_none()));
            R_WriteProfile(0);

            
                let stop_args = pairlist(&[r_string("")]);
                crate::mainutils::essentials::do_Rprofmem(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    stop_args,
                    R_NilValue(),
                );
            
            with_profiling_state(|state| {
                assert_eq!(state.profiling, 0);
                assert_eq!(state.mem_profiling, 0);
                assert_eq!(state.profile_outfile, -1);
            });

            let contents = fs::read_to_string(&path).expect("memory profile file");
            assert!(contents.contains("memory profiling: sample.interval=20000"));
            let sample_line = contents
                .lines()
                .find(|line| line.starts_with(':'))
                .expect("memory profile sample line");
            let fields: Vec<u64> = sample_line
                .trim_matches(':')
                .split(':')
                .take(4)
                .map(|field| field.parse::<u64>().expect("numeric memory field"))
                .collect();
            assert_eq!(fields.len(), 4);
            assert!(fields[0] > 0);
            assert!(fields[1] >= fields[0]);
            assert!(fields[2] > 0);
            let _ = fs::remove_file(path);
        });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod owner_tests;
