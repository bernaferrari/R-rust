#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/print.c -- print.default() and auto-printing.
//!
//! Provides do_printdefault(), PrintValueRec(), PrintValueEnv(), etc.
//! Faithfully follows R's print.c logic with simplifications for features
//! not yet available (S4 dispatch, source references, Win32 UTF8, etc.).

use std::cell::{Cell, RefCell};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::rc::Rc;

use crate::mainutils::r_format::{CArg, r_sprintf};
use crate::sexp::accessors::{
    ATTRIB, BODY, CAR, CDR, CHAR, CLOENV, INTEGER_ELT, LENGTH, PRINTNAME, SET_STRING_ELT, SETCDR,
    STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_LevelsSymbol, R_NamesSymbol, R_RowNamesSymbol,
    getAttrib, isObject as isObject_fn,
};
use crate::sexp::constructors::Rf_cons;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, NA_REAL, R_IsNA, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_NilValue, R_UnboundValue};
use crate::sexp::instance::{
    RuntimeValue, with_current_scalar_print_state, with_print_data_in,
    with_required_current_instance,
};
use crate::sexp::object::SexpResult;
use crate::sexp::owner::OwnerToken;
use crate::sexp::protect::{ProtectGuard, protect};
use crate::sexp::symbol::install_str;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const TAGBUFLEN: usize = 256;
const TAGBUFLEN0: usize = TAGBUFLEN + 6;

const Rprt_adj_left: c_int = 0;
const Rprt_adj_right: c_int = 1;

const SIMPLEDEPARSE: c_int = 0;
const DEFAULTDEPARSE: c_int = 1;
const USESOURCE: c_int = 8;

type PrintTagBytes = [u8; TAGBUFLEN0 * 2];

pub(crate) struct PrintRuntimeState {
    tagbuf: Rc<RefCell<PrintTagBytes>>,
    recursion: PrintRecursionStack,
}

impl Default for PrintRuntimeState {
    fn default() -> Self {
        PrintRuntimeState {
            tagbuf: Rc::new(RefCell::new([0; TAGBUFLEN0 * 2])),
            recursion: Rc::default(),
        }
    }
}

struct PrintFrame {
    value: SEXP,
    suspended: Cell<bool>,
}

type PrintRecursionStack = Rc<RefCell<Vec<Rc<PrintFrame>>>>;

struct PrintRecursionGuard {
    stack: PrintRecursionStack,
    frame: Rc<PrintFrame>,
}

impl Drop for PrintRecursionGuard {
    fn drop(&mut self) {
        let mut frames = self.stack.borrow_mut();
        if let Some(index) = frames
            .iter()
            .rposition(|frame| Rc::ptr_eq(frame, &self.frame))
        {
            frames.remove(index);
        }
    }
}

fn print_recursion_stack() -> PrintRecursionStack {
    with_print_runtime(|state| state.recursion.clone())
}

fn enter_print_value(value: SEXP) -> Option<PrintRecursionGuard> {
    let stack = print_recursion_stack();
    let mut frames = stack.borrow_mut();
    if frames
        .iter()
        .any(|frame| !frame.suspended.get() && frame.value == value)
    {
        return None;
    }
    let frame = Rc::new(PrintFrame {
        value,
        suspended: Cell::new(false),
    });
    frames.push(frame.clone());
    drop(frames);
    Some(PrintRecursionGuard { stack, frame })
}

// showDefault may print the current object's data without discarding its outer frame.
struct SuspendedPrintFrame(Rc<PrintFrame>);

impl Drop for SuspendedPrintFrame {
    fn drop(&mut self) {
        self.0.suspended.set(false);
    }
}

fn suspend_print_value(value: SEXP) -> Option<SuspendedPrintFrame> {
    let stack = print_recursion_stack();
    let frames = stack.borrow();
    let frame = frames
        .iter()
        .rfind(|frame| !frame.suspended.get() && frame.value == value)?;
    frame.suspended.set(true);
    Some(SuspendedPrintFrame(frame.clone()))
}

fn with_print_runtime<R>(f: impl FnOnce(&PrintRuntimeState) -> R) -> R {
    with_required_current_instance(|inst| {
        // SAFETY: the active owner is retained and callers only access the checked cells or clone their handles.
        f(unsafe { &(*inst).eval_state.print })
    })
}

/// Retain exact allocations alongside the raw view used by translated printers.
#[derive(Clone)]
pub(crate) struct PrintDataSnapshot {
    raw: R_PrintData,
    _roots: [RuntimeValue; 4],
}

impl Default for PrintDataSnapshot {
    fn default() -> Self {
        Self {
            raw: R_PRINT_INIT.clone(),
            _roots: Default::default(),
        }
    }
}

impl PrintDataSnapshot {
    fn capture(owner: OwnerToken<'_>, mut raw: R_PrintData) -> SexpResult<Self> {
        owner.require_active()?;
        let retain = |pointer: SEXP| -> SexpResult<RuntimeValue> {
            if pointer.is_null() {
                return Ok(RuntimeValue::empty());
            }
            owner
                .sexp(pointer)
                .and_then(|value| value.into_owned())
                .map(RuntimeValue::from_owned)
        };
        let roots = [
            retain(raw.na_string)?,
            retain(raw.na_string_noquote)?,
            retain(raw.env)?,
            retain(raw.callArgs)?,
        ];
        raw.na_string = roots[0].as_raw();
        raw.na_string_noquote = roots[1].as_raw();
        raw.env = roots[2].as_raw();
        raw.callArgs = roots[3].as_raw();
        Ok(Self {
            raw,
            _roots: roots,
        })
    }

    pub(crate) fn read<R>(&self, f: impl FnOnce(&R_PrintData) -> R) -> R {
        f(&self.raw)
    }

    #[cfg(test)]
    pub(crate) fn clear_roots_for_gc_fixture(&mut self, nil: RuntimeValue) -> Self {
        let mut raw = self.raw.clone();
        let nil_ptr = nil.as_raw();
        raw.na_string = nil_ptr;
        raw.na_string_noquote = nil_ptr;
        raw.env = nil_ptr;
        raw.callArgs = nil_ptr;
        let next = Self {
            raw,
            _roots: std::array::from_fn(|_| nil.clone()),
        };
        std::mem::replace(self, next)
    }
}

fn set_current_print_data(data: &R_PrintData) {
    print_checked(OwnerToken::with_current(|owner| {
        let next = print_checked(PrintDataSnapshot::capture(owner, data.clone()));
        // SAFETY: the token keeps the original owner live for this short swap.
        let old =
            unsafe { with_print_data_in(owner.as_ptr(), |state| std::mem::replace(state, next)) };
        drop(old);
    }));
}

/// Read print fields without copying their roots. The raw values are valid
/// only during the read; use a snapshot when calling code that can reenter R.
pub(crate) fn with_current_print_data<R>(f: impl FnOnce(&R_PrintData) -> R) -> R {
    with_required_current_instance(|inst| {
        // SAFETY: the current owner is pinned for this local checked lend.
        unsafe { with_print_data_in(inst, |state| state.read(f)) }
    })
}

fn print_data_snapshot() -> PrintDataSnapshot {
    with_required_current_instance(|inst| {
        // SAFETY: the owner is pinned and cloning retains the original leases.
        unsafe { with_print_data_in(inst, |state| state.clone()) }
    })
}

/// Length of the NUL-terminated prefix of `buf` (C `strlen`, bounded).
fn nul_len(buf: &[u8]) -> usize {
    buf.iter().position(|&b| b == 0).unwrap_or(buf.len())
}

fn tagbuf_strlen() -> usize {
    with_print_runtime(|state| nul_len(&*state.tagbuf.borrow()))
}

fn tagbuf_set(idx: usize, val: c_char) {
    with_print_runtime(|state| {
        if let Some(byte) = state.tagbuf.borrow_mut().get_mut(idx) {
            *byte = val as u8;
        }
    });
}

fn tagbuf_clear() {
    tagbuf_set(0, 0);
}

fn tagbuf_byte_at(offset: usize) -> u8 {
    with_print_runtime(|state| state.tagbuf.borrow().get(offset).copied().unwrap_or(0))
}

/// Bounded, NUL-terminated write into the tag buffer at byte `offset`,
/// matching C `snprintf` size semantics: at most `sz - 1` payload bytes plus
/// a NUL; `sz == 0` writes nothing. Writes never pass the buffer end.
fn write_tag(offset: usize, sz: usize, s: &str) {
    if sz == 0 {
        return;
    }
    with_print_runtime(|state| {
        let mut buf = state.tagbuf.borrow_mut();
        let Some(tail) = buf.get_mut(offset..) else {
            return;
        };
        let Some(room) = tail.len().checked_sub(1) else {
            return;
        };
        let bytes = s.as_bytes();
        let n = (sz - 1).min(bytes.len()).min(room);
        tail[..n].copy_from_slice(&bytes[..n]);
        tail[n] = 0;
    });
}

fn tagbuf_string() -> String {
    with_print_runtime(|state| {
        let buf = state.tagbuf.borrow();
        String::from_utf8_lossy(&buf[..nul_len(&*buf)]).into_owned()
    })
}

// ---------------------------------------------------------------------------
// R_PrintData struct
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct R_PrintData {
    pub width: c_int,
    pub gap: c_int,
    pub digits: c_int,
    pub scipen: c_int,
    pub max: c_int,
    pub right: c_int,
    pub quote: c_int,
    pub na_width: c_int,
    pub na_width_noquote: c_int,
    pub na_string: SEXP,
    pub na_string_noquote: SEXP,
    pub useSource: c_int,
    pub cutoff: c_int,
    pub env: SEXP,
    pub callArgs: SEXP,
}

/// Const initializer for static R_PRINT.
const R_PRINT_INIT: R_PrintData = R_PrintData {
    width: 80,
    gap: 1,
    digits: 4,
    scipen: 0,
    max: 99999,
    right: 0,
    quote: 1,
    na_width: 2,
    na_width_noquote: 4,
    na_string: ptr::null_mut(),
    na_string_noquote: ptr::null_mut(),
    useSource: 0,
    cutoff: 60,
    env: ptr::null_mut(),
    callArgs: ptr::null_mut(),
};

// ---------------------------------------------------------------------------
// Module-private helpers
// ---------------------------------------------------------------------------

fn isSymbol(x: SEXP) -> c_int {
    {
        if x.is_null() {
            return 0;
        }
        if TYPEOF(x) == SEXPTYPE::SYMSXP { 1 } else { 0 }
    }
}

fn isString(x: SEXP) -> c_int {
    {
        if x.is_null() {
            return 0;
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP { 1 } else { 0 }
    }
}

fn Rf_isNull(x: SEXP) -> c_int {
    {
        if x.is_null() || x == R_NilValue() {
            1
        } else {
            0
        }
    }
}

fn isFunction(x: SEXP) -> c_int {
    {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            1
        } else {
            0
        }
    }
}

fn isList(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LISTSXP {
        return 1;
    }
    if t == SEXPTYPE::VECSXP && getAttrib(x, R_DimSymbol()) == R_NilValue() {
        return 1;
    }
    0
}

fn isArray(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    let dims = getAttrib(x, R_DimSymbol());
    if dims == R_NilValue() {
        return 0;
    }
    if LENGTH(dims) > 1 { 1 } else { 0 }
}

fn isDataFrame(_x: SEXP) -> c_int {
    0
}

fn IS_S4_OBJECT(x: SEXP) -> c_int {
    crate::mainutils::objects::IS_S4_OBJECT(x)
}

fn isMethodsDispatchOn() -> c_int {
    crate::mainutils::objects::isMethodsDispatchOn()
}

fn isByteCode(_x: SEXP) -> c_int {
    0
}

/// inherits using a class name.
fn inherits_cstr(x: SEXP, cn: &str) -> c_int {
    if x.is_null() {
        return 0;
    }
    let klass = getAttrib(x, R_ClassSymbol());
    if klass == R_NilValue() || TYPEOF(klass) != SEXPTYPE::STRSXP {
        return 0;
    }
    let n = LENGTH(klass);
    for i in 0..n {
        let elt = STRING_ELT(klass, i as R_xlen_t);
        if elt.is_null() || elt == R_NilValue() {
            continue;
        }
        if char_bytes(elt) == cn.as_bytes() {
            return 1;
        }
    }
    0
}

/// Copy `bytes` into a NUL-terminated cell buffer, truncated to `cap` bytes.
fn copy_pbuf(pbuf: &mut [u8], bytes: &[u8], cap: usize) {
    let copy_len = bytes.len().min(cap);
    pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
    pbuf[copy_len] = 0;
}

/// `mkChar` of the NUL-terminated prefix of `buf`.
fn mk_char_nul(buf: &[u8]) -> SEXP {
    let len = nul_len(buf);
    // SAFETY: `buf` is a live buffer of at least `len` bytes; mkCharLen copies them.
    unsafe { crate::sexp::constructors::Rf_mkCharLen(buf.as_ptr() as *const c_char, len as c_int) }
}

/// Raw CHARSXP contents (GNU `CHAR(x)`).
fn char_bytes(x: SEXP) -> Vec<u8> {
    crate::mainutils::printutils::charsxp_bytes(x)
}

/// GNU `isValidName` (gram.y): PrintGenericVector / printList tag quoting.
fn isValidName(s: &[u8]) -> bool {
    crate::mainutils::deparse::local_parse_data::is_valid_r_name_bytes(s)
}

/// NA_STRING accessor.
fn NA_STRING_local() -> SEXP {
    crate::mainutils::relop::NA_STRING()
}

/// GNU `printMatrix(t, 0, dims, quote, right, rl, cl, rn, cn)` labelled with
/// `matrix_dimnames(s, ...)`.
fn print_matrix_dimnamed(t: SEXP, s: SEXP, dims: SEXP, quote: c_int, right: c_int) {
    let labels = crate::mainutils::printvector::matrix_dimnames(s);
    crate::mainutils::printarray::printMatrix(
        t,
        0,
        dims,
        quote,
        right,
        labels.rows,
        labels.cols,
        labels.row_title.as_deref(),
        labels.col_title.as_deref(),
    );
}

/// Single-frame variable lookup (GNU `findVarInFrame`).
fn R_findVarInFrame(rho: SEXP, symbol: SEXP) -> SEXP {
    crate::sexp::envir::R_findVarInFrame(rho, symbol)
}

/// GNU `defineVar` in a live, rooted environment.
fn define_var(symbol: SEXP, value: SEXP, rho: SEXP) {
    let _ = crate::sexp::envir::define_var_updates(symbol, value, rho);
}

/// GNU `findFun` from a live environment.
fn find_fun(symbol: SEXP, rho: SEXP) -> SEXP {
    crate::sexp::envir::findFun(symbol, rho)
}

/// GNU `forcePromise` on a live promise.
fn force_promise(prom: SEXP) -> SEXP {
    crate::sexp::envir::forcePromise(prom)
}

/// GNU `eval(call, rho)` of a protected call.
fn eval_in(call: SEXP, rho: SEXP) -> SEXP {
    crate::eval::eval::Rf_eval(call, rho)
}

/// GNU `duplicate`.
fn duplicate_value(x: SEXP) -> SEXP {
    crate::mainutils::duplicate::Rf_duplicate(x)
}

/// Checked handle for a live value in the active session.
fn wrap_sexp(x: SEXP) -> Option<crate::sexp::object::Sexp<'static>> {
    crate::mainutils::essentials::active_owner().sexp(x).ok()
}

/// GetArrayDimnames -- local implementation.
fn GetArrayDimnames(x: SEXP) -> SEXP {
    getAttrib(x, R_DimNamesSymbol())
}

/// allocArray (delegate to array.rs).
fn allocArray(mode: c_int, dims: SEXP) -> SEXP {
    crate::mainutils::array::allocArray(mode, dims)
}

/// Use the shared GNU scalar coercion, including string and complex admission.
fn asInteger(x: SEXP) -> c_int {
    if x.is_null() {
        NA_INTEGER
    } else {
        crate::mainutils::coerce::asInteger(x)
    }
}

/// asLogical -- local implementation.
fn asLogical(x: SEXP) -> c_int {
    if x.is_null() || x == R_NilValue() {
        return NA_LOGICAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP {
        if LENGTH(x) >= 1 {
            return crate::mainutils::essentials::logical_elt_checked(x, 0);
        }
    } else if t == SEXPTYPE::INTSXP && LENGTH(x) >= 1 {
        return crate::mainutils::essentials::integer_elt_checked(x, 0);
    }
    NA_LOGICAL
}

/// FixupDigits -- local implementation.
fn FixupDigits(digits: SEXP, _warn: c_int) -> c_int {
    let d = asInteger(digits);
    if d == NA_INTEGER {
        return 0;
    }
    if d < 1 {
        return 1;
    }
    if d > 22 {
        return 22;
    }
    d
}

/// FixupWidth -- local implementation.
fn FixupWidth(width: SEXP, _warn: c_int) -> c_int {
    let w = asInteger(width);
    if w == NA_INTEGER {
        return 80;
    }
    if w < 10 {
        return 10;
    }
    if w > 10000 {
        return 10000;
    }
    w
}

// ---------------------------------------------------------------------------
// PrintInit
// ---------------------------------------------------------------------------

pub unsafe fn PrintInit(data: *mut std::ffi::c_void, env: SEXP) {
    let d = data as *mut R_PrintData;
    if d.is_null() {
        return;
    }
    // SAFETY: the caller passes a live, exclusively borrowed R_PrintData.
    drop(print_init(unsafe { &mut *d }, env));
}

struct PrintInitRoots {
    _environment: ProtectGuard<'static>,
    _na_string_noquote: ProtectGuard<'static>,
}

fn print_checked<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Initialize a local print snapshot and keep its heap values live for the caller.
fn print_init(d: &mut R_PrintData, env: SEXP) -> PrintInitRoots {
    print_checked(OwnerToken::with_current(|owner| {
        let environment = protect(env);
        d.na_string = NA_STRING_local();
        d.na_string_noquote = mk_char_nul(b"<NA>\0");
        print_checked(owner.require_active());
        let na_string_noquote = protect(d.na_string_noquote);
        d.na_width = crate::mainutils::printutils::Rstrlen(d.na_string, 0);
        d.na_width_noquote = crate::mainutils::printutils::Rstrlen(d.na_string_noquote, 0);
        d.quote = 1;
        d.right = Rprt_adj_left;
        d.digits = crate::mainutils::options::GetOptionDigits();
        print_checked(owner.require_active());

        let scipen_sym = install_str("scipen");
        print_checked(owner.require_active());
        d.scipen = asInteger(crate::mainutils::options::GetOption1(scipen_sym));
        print_checked(owner.require_active());
        if d.scipen == NA_INTEGER {
            d.scipen = 0;
        }

        let maxprint_sym = install_str("max.print");
        print_checked(owner.require_active());
        d.max = asInteger(crate::mainutils::options::GetOption1(maxprint_sym));
        print_checked(owner.require_active());
        if d.max == NA_INTEGER || d.max < 0 {
            d.max = 99999;
        } else if d.max == c_int::MAX {
            d.max -= 1;
        }
        d.gap = 1;
        d.width = crate::mainutils::options::GetOptionWidth();
        print_checked(owner.require_active());
        d.useSource = USESOURCE;
        d.cutoff = crate::mainutils::options::GetOptionCutoff();
        print_checked(owner.require_active());
        d.env = env;
        d.callArgs = R_NilValue();
        PrintInitRoots {
            _environment: environment,
            _na_string_noquote: na_string_noquote,
        }
    }))
}

// ---------------------------------------------------------------------------
// PrintDefaults
// ---------------------------------------------------------------------------

pub fn PrintDefaults() {
    let mut data = R_PRINT_INIT.clone();
    // Option coercion can collect or run callbacks before defaults are complete.
    let _roots = print_init(&mut data, R_GlobalEnv());
    let defaults = crate::mainutils::format::RPrint {
        digits: data.digits,
        scipen: data.scipen,
        na_width: data.na_width,
        na_width_noquote: data.na_width_noquote,
    };
    set_current_print_data(&data);
    // GNU nested printing resets only the active conversion's defaults.
    with_current_scalar_print_state(|cell| {
        let mut state = cell.get();
        if state.character_active {
            state.print = defaults;
            cell.set(state);
        }
    });
}

// ---------------------------------------------------------------------------
// Internal: advancePrintArgs
// ---------------------------------------------------------------------------

fn advancePrintArgs(
    args: &mut SEXP,
    prev: &mut SEXP,
    missings: SEXP,
    missing_idx: &mut R_xlen_t,
    allMissing: &mut c_int,
) {
    *args = CDR(*args);
    let missing = *missing_idx < XLENGTH(missings)
        && crate::mainutils::essentials::logical_elt_checked(missings, *missing_idx) != 0;
    if missing {
        SETCDR(*prev, *args);
    } else {
        *allMissing = 0;
        *prev = CDR(*prev);
    }
    *missing_idx += 1;
}

// ---------------------------------------------------------------------------
// Internal: save/restore tagbuf
// ---------------------------------------------------------------------------

struct PrintTagGuard {
    buffer: Rc<RefCell<PrintTagBytes>>,
    saved: PrintTagBytes,
    length: usize,
}

impl PrintTagGuard {
    fn save() -> Self {
        let buffer = with_print_runtime(|state| state.tagbuf.clone());
        let mut saved = [0; TAGBUFLEN0 * 2];
        let length = {
            let current = buffer.borrow();
            let length = nul_len(&*current);
            if length < saved.len() {
                saved[..=length].copy_from_slice(&current[..=length]);
                length + 1
            } else {
                1
            }
        };
        Self {
            buffer,
            saved,
            length,
        }
    }
}

impl Drop for PrintTagGuard {
    fn drop(&mut self) {
        self.buffer.borrow_mut()[..self.length].copy_from_slice(&self.saved[..self.length]);
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintLanguage
// ---------------------------------------------------------------------------

fn PrintLanguage(s: SEXP, data: &R_PrintData) {
    let t = crate::mainutils::deparse::deparse1w(s, false, data.useSource | DEFAULTDEPARSE);
    let _t_guard = protect(t);
    set_current_print_data(data);

    let n = LENGTH(t);
    for i in 0..n {
        let elt = STRING_ELT(t, i as R_xlen_t);
        if !elt.is_null() && elt != R_NilValue() {
            crate::mainutils::printutils::console_emit(format_args!(
                "{}\n",
                std::str::from_utf8(&char_bytes(elt)).unwrap_or("?")
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintClosure
// ---------------------------------------------------------------------------

fn PrintClosure(s: SEXP, data: &R_PrintData) {
    PrintLanguage(s, data);

    if isByteCode(BODY(s)) != 0 {
        crate::mainutils::printutils::console_emit(format_args!("<bytecode: {:?}>\n", BODY(s)));
    }
    let t = CLOENV(s);
    if t != R_GlobalEnv() {
        if let Some(env_str) = crate::mainutils::printutils::encode_environment_bytes(t) {
            crate::mainutils::printutils::console_emit(format_args!(
                "{}\n",
                std::str::from_utf8(&env_str).unwrap_or("<environment>")
            ));
        } else {
            crate::mainutils::printutils::console_emit(format_args!("<environment>\n"));
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintSpecial
// ---------------------------------------------------------------------------

fn PrintSpecial(s: SEXP, data: &R_PrintData) {
    let nm = crate::eval::builtin::PRIMNAME(s);
    let nm_sym = install_str(nm);

    let env = R_findVarInFrame(R_BaseEnv(), install_str(".ArgsEnv"));

    let mut s2 = R_findVarInFrame(env, nm_sym);
    if s2 == R_UnboundValue() {
        let env2 = R_findVarInFrame(R_BaseEnv(), install_str(".GenericArgsEnv"));
        s2 = R_findVarInFrame(env2, nm_sym);
    }

    if s2 != R_UnboundValue() {
        let t = crate::mainutils::deparse::deparse1m(s2, false, DEFAULTDEPARSE);
        let _t_guard = protect(t);
        set_current_print_data(data);

        let line = STRING_ELT(t, 0);
        if !line.is_null() && line != R_NilValue() {
            print!("{} ", std::str::from_utf8(&char_bytes(line)).unwrap_or(""));
        }
        crate::mainutils::printutils::console_emit(format_args!(".Primitive(\"{}\")\n", nm));
    } else {
        crate::mainutils::printutils::console_emit(format_args!(".Primitive(\"{}\")\n", nm));
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintExpression
// ---------------------------------------------------------------------------

fn PrintExpression(s: SEXP, data: &R_PrintData) {
    let u = crate::mainutils::deparse::deparse1w(s, false, data.useSource | DEFAULTDEPARSE);
    let _u_guard = protect(u);
    set_current_print_data(data);

    let n = LENGTH(u);
    for i in 0..n {
        let elt = STRING_ELT(u, i as R_xlen_t);
        if !elt.is_null() && elt != R_NilValue() {
            crate::mainutils::printutils::console_emit(format_args!(
                "{}\n",
                std::str::from_utf8(&char_bytes(elt)).unwrap_or("?")
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: PrintDispatch
// ---------------------------------------------------------------------------

fn PrintDispatch(s: SEXP, data: &R_PrintData) {
    validate_print_chain(ATTRIB(s));
    if sexp_class_is(s, "srcref") {
        print_srcref_value(s);
    } else if isObject_fn(s) != 0 {
        PrintObject(s, data);
    } else {
        PrintValueRec_inner(s, data);
    }
}

/// GNU `print.srcref`: `cat(as.character(x), sep="\n")`.
fn print_srcref_value(s: SEXP) {
    let srcfile = getAttrib(s, install_str("srcfile"));
    let lines =
        if !srcfile.is_null() && srcfile != R_NilValue() && TYPEOF(srcfile) == SEXPTYPE::ENVSXP {
            R_findVarInFrame(srcfile, install_str("lines"))
        } else {
            R_NilValue()
        };
    if lines.is_null()
        || lines == R_NilValue()
        || lines == R_UnboundValue()
        || TYPEOF(lines) != SEXPTYPE::STRSXP
        || XLENGTH(s) < 4
    {
        PrintValueRec_inner(s, &R_PRINT_INIT);
        return;
    }
    let first = INTEGER_ELT(s, 0);
    let mut last = INTEGER_ELT(s, 2);
    let n = XLENGTH(lines);
    if first < 1 || last < first {
        return;
    }
    last = last.min(n as i32);
    let first_byte = INTEGER_ELT(s, 1).max(1) as usize;
    let last_byte = INTEGER_ELT(s, 3).max(1) as usize;
    for i in first..=last {
        let elt = STRING_ELT(lines, (i - 1) as i64);
        if elt.is_null() {
            continue;
        }
        let mut text = String::from_utf8_lossy(&char_bytes(elt)).into_owned();
        if i == last {
            let take = last_byte.min(text.len());
            text.truncate(take);
        }
        if i == first {
            let skip = first_byte.saturating_sub(1).min(text.len());
            text = text[skip..].to_string();
        }
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&format!("{text}\n"));
        } else {
            println!("{text}");
        }
    }
}

fn sexp_class_is(s: SEXP, name: &str) -> bool {
    let class = getAttrib(s, R_ClassSymbol());
    if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
        return false;
    }
    let n = XLENGTH(class);
    for i in 0..n {
        let elt = STRING_ELT(class, i);
        if elt.is_null() {
            continue;
        }
        if char_bytes(elt) == name.as_bytes() {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Internal: PrintObjectS3
// ---------------------------------------------------------------------------

fn PrintObjectS3(s: SEXP, data: &R_PrintData) {
    let xsym = install_str("x");
    let parent = if data.env.is_null() || data.env == R_NilValue() {
        R_GlobalEnv()
    } else {
        data.env
    };
    let mask = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue());
    let _mask_guard = protect(mask);
    define_var(xsym, s, mask);
    let fun = find_fun(install_str("print"), R_BaseEnv());
    if fun.is_null() || fun == R_UnboundValue() {
        crate::mainutils::essentials::do_print(
            R_NilValue(),
            R_NilValue(),
            crate::sexp::output::cons_print_args(s),
            parent,
        );
        define_var(xsym, R_NilValue(), mask);
        return;
    }
    let call = crate::sexp::constructors::Rf_lang2(fun, xsym);
    let extra = if data.callArgs.is_null() {
        R_NilValue()
    } else {
        data.callArgs
    };
    if extra != R_NilValue() {
        SETCDR(CDR(call), extra);
    }
    let _call_guard = protect(call);
    let _ = eval_in(call, mask);
    define_var(xsym, R_NilValue(), mask);
}

/// GNU `print(object, useS4=FALSE)` / `PrintValueRec` on an S4 basic object
/// prints the data part. `format_matrix` / `print_value` go through the
/// capture sink (stdout); `printMatrix` currently writes stderr.
fn print_s4_data_part(s: SEXP, _data: &R_PrintData) {
    let copy = duplicate_value(s);
    let _copy = protect(copy);
    crate::sexp::accessors::UNSET_S4_OBJECT(copy);
    crate::sexp::attrib_core::setAttrib(copy, R_ClassSymbol(), R_NilValue());
    if let Some(sexp) = wrap_sexp(copy) {
        crate::sexp::output::print_value(sexp);
    }
}

struct PrintFormatGuard {
    owner: crate::sexp::owner::OwnerPin,
    previous: crate::mainutils::format::RPrint,
}

impl PrintFormatGuard {
    fn new(data: &R_PrintData) -> Self {
        let owner = crate::sexp::owner::OwnerToken::with_current(|owner| owner.weak_owner())
            .expect("print formatting requires a current runtime")
            .expect("print formatting requires a managed runtime")
            .pin()
            .expect("print formatting requires an active runtime");
        let previous = owner.replace_format_print(crate::mainutils::format::RPrint {
            digits: data.digits,
            scipen: data.scipen,
            na_width: data.na_width,
            na_width_noquote: data.na_width_noquote,
        });
        Self { owner, previous }
    }
}

impl Drop for PrintFormatGuard {
    fn drop(&mut self) {
        self.owner.replace_format_print(self.previous);
    }
}

/// `do_printdefault` body: S4 with no extra args uses `show()`; extra args
/// (GNU `useS4=FALSE`) print the data part; non-S4 uses `print_value` so
/// `capture.output(print(m))` sees the same matrix layout as auto-print.
fn emit_print_default(x: SEXP, data: &R_PrintData, show_s4: bool) {
    // The shared numeric formatter otherwise reads options("digits").
    // Scope the admitted print.default parameters across recursive calls,
    // restoring the previous formatter even when a print method errors.
    let _format = PrintFormatGuard::new(data);
    if show_s4 && IS_S4_OBJECT(x) != 0 && isMethodsDispatchOn() != 0 {
        PrintObject(x, data);
    } else if IS_S4_OBJECT(x) != 0 {
        // GNU do_printdefault has no separate S4-with-arguments branch:
        // PrintValueRec prints the data part by type and then the
        // attributes (including `attr(,"class")` and its own package
        // attribute).
        PrintValueRec_reroot(x, data);
    } else if let Some(sexp) = wrap_sexp(x) {
        let extras = if data.callArgs.is_null() {
            R_NilValue()
        } else {
            data.callArgs
        };
        let _extras = protect(extras);
        let _guard = crate::sexp::output::push_print_dispatch_extras(extras);
        crate::sexp::output::print_value(sexp);
    } else {
        PrintValueRec_inner(x, data);
    }
}

fn PrintObjectS4(s: SEXP, data: &R_PrintData) {
    let depth = print_recursion_stack()
        .borrow()
        .iter()
        .filter(|frame| !frame.suspended.get())
        .count();
    if depth > 8 {
        print_s4_data_part(s, data);
        return;
    }
    let Some(methods) = crate::mainutils::essentials::cached_namespace_by_name("methods") else {
        PrintObjectS3(s, data);
        return;
    };
    let mut fun = R_findVarInFrame(methods, install_str("show"));
    if fun.is_null() || fun == R_UnboundValue() {
        PrintObjectS3(s, data);
        return;
    }
    if TYPEOF(fun) == SEXPTYPE::PROMSXP {
        fun = force_promise(fun);
    }
    let env = if data.env.is_null() || data.env == R_NilValue() {
        R_GlobalEnv()
    } else {
        data.env
    };
    let call = crate::sexp::constructors::Rf_lang2(fun, s);
    let _call_guard = protect(call);
    // GNU PrintObjectS4 eval()s show with no local handler.
    let _ = eval_in(call, env);
}

fn PrintObject(s: SEXP, data: &R_PrintData) {
    let _tag_guard = PrintTagGuard::save();
    if isMethodsDispatchOn() != 0 && IS_S4_OBJECT(s) != 0 {
        match enter_print_value(s) {
            Some(_guard) => PrintObjectS4(s, data),
            None => {
                // An explicit print(object) from a custom show method
                // dispatches again in GNU R; options(expressions) owns
                // the recursion error. showDefault's useS4=FALSE takes
                // PrintValueRec_reroot and never enters this branch.
                PrintObjectS4(s, data);
            }
        }
    } else {
        PrintObjectS3(s, data);
    }
    set_current_print_data(data);
}

// ---------------------------------------------------------------------------
// Internal: PrintGenericVector
// ---------------------------------------------------------------------------

fn PrintGenericVector(s: SEXP, data: &R_PrintData) {
    let ns = XLENGTH(s);
    let dims = getAttrib(s, R_DimSymbol());

    if dims != R_NilValue() && LENGTH(dims) > 1 {
        // Array-like list
        let _dims_guard = protect(dims);
        let t = allocArray(SEXPTYPE::STRSXP.as_c_int(), dims);
        let _t_guard = protect(t);

        let limit = if ns <= data.max as i64 + 1 {
            ns
        } else {
            data.max as i64
        };
        for i in 0..limit {
            let s_i = VECTOR_ELT(s, i);
            let mut pbuf = [0u8; 115];

            if isObject_fn(s_i) != 0 {
                let snip = match TYPEOF(s_i) {
                    x if x == SEXPTYPE::NILSXP => "NULL",
                    x if x == SEXPTYPE::LGLSXP => "logical",
                    x if x == SEXPTYPE::INTSXP => {
                        if inherits_cstr(s_i, "factor") != 0 {
                            "factor"
                        } else {
                            "integer"
                        }
                    }
                    x if x == SEXPTYPE::REALSXP => "numeric",
                    x if x == SEXPTYPE::CPLXSXP => "complex",
                    x if x == SEXPTYPE::STRSXP => "character",
                    x if x == SEXPTYPE::RAWSXP => "raw",
                    x if x == SEXPTYPE::LISTSXP || x == SEXPTYPE::VECSXP => "list",
                    x if x == SEXPTYPE::LANGSXP => "expression",
                    _ => "?",
                };
                let full = format!("{},{}", snip, LENGTH(s_i));
                let bytes = full.as_bytes();
                let copy_len = bytes.len().min(114);
                pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                pbuf[copy_len] = 0;
            } else {
                match TYPEOF(s_i) {
                    _t if _t == SEXPTYPE::NILSXP => {
                        let src = b"NULL\0";
                        pbuf[..src.len()].copy_from_slice(src);
                    }
                    _t if _t == SEXPTYPE::LGLSXP => {
                        if LENGTH(s_i) == 1 {
                            let x = crate::mainutils::essentials::logical_elt_checked(s_i, 0);
                            let w = crate::mainutils::format::logical_field_width([x]);
                            if let Some(bytes) =
                                crate::mainutils::printutils::encode_logical_bytes(x, w)
                            {
                                copy_pbuf(&mut pbuf, &bytes, 114);
                            }
                        } else {
                            let full = format!("logical,{}", LENGTH(s_i));
                            let bytes = full.as_bytes();
                            let copy_len = bytes.len().min(114);
                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                            pbuf[copy_len] = 0;
                        }
                    }
                    _t if _t == SEXPTYPE::INTSXP => {
                        if inherits_cstr(s_i, "factor") != 0 {
                            let full = format!("factor,{}", LENGTH(s_i));
                            let bytes = full.as_bytes();
                            let copy_len = bytes.len().min(114);
                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                            pbuf[copy_len] = 0;
                        } else if LENGTH(s_i) == 1 {
                            let x = crate::mainutils::essentials::integer_elt_checked(s_i, 0);
                            let w = crate::mainutils::format::integer_field_width(1, |_| x);
                            if let Some(bytes) =
                                crate::mainutils::printutils::encode_integer_bytes(x, w)
                            {
                                copy_pbuf(&mut pbuf, &bytes, 114);
                            }
                        } else {
                            let full = format!("integer,{}", LENGTH(s_i));
                            let bytes = full.as_bytes();
                            let copy_len = bytes.len().min(114);
                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                            pbuf[copy_len] = 0;
                        }
                    }
                    _t if _t == SEXPTYPE::REALSXP => {
                        if LENGTH(s_i) == 1 {
                            let x = crate::mainutils::essentials::real_elt_checked(s_i, 0);
                            let f = crate::mainutils::format::real_field(1, 0, |_| x);
                            if let Some(bytes) = crate::mainutils::printutils::encode_real0_bytes(
                                x, f.w, f.d, f.e, c".",
                            ) {
                                copy_pbuf(&mut pbuf, &bytes, 114);
                            }
                        } else {
                            let full = format!("numeric,{}", LENGTH(s_i));
                            let bytes = full.as_bytes();
                            let copy_len = bytes.len().min(114);
                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                            pbuf[copy_len] = 0;
                        }
                    }
                    _t if _t == SEXPTYPE::CPLXSXP => {
                        if LENGTH(s_i) == 1 {
                            let cx = crate::mainutils::essentials::complex_elt_checked(s_i, 0);
                            let enc = if R_IsNA(cx.r) || R_IsNA(cx.i) {
                                crate::mainutils::printutils::encode_real0_bytes(
                                    NA_REAL,
                                    data.na_width,
                                    0,
                                    0,
                                    c".",
                                )
                            } else {
                                let f = crate::mainutils::format::complex_field(&[cx], 0);
                                crate::mainutils::printutils::encode_complex_bytes(
                                    cx, f.wr, f.dr, f.er, f.wi, f.di, f.ei, c".",
                                )
                            };
                            if let Some(bytes) = enc {
                                copy_pbuf(&mut pbuf, &bytes, 114);
                            }
                        } else {
                            let full = format!("complex,{}", LENGTH(s_i));
                            let bytes = full.as_bytes();
                            let copy_len = bytes.len().min(114);
                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                            pbuf[copy_len] = 0;
                        }
                    }
                    _t if _t == SEXPTYPE::STRSXP => {
                        if LENGTH(s_i) == 1 {
                            let elt0 = STRING_ELT(s_i, 0);
                            if !elt0.is_null() {
                                let ctmp = char_bytes(elt0);
                                let s_str = std::str::from_utf8(&ctmp).unwrap_or("");
                                let quoted = if s_str.len() < 100 {
                                    format!("\"{}\"", s_str)
                                } else {
                                    format!("\"{}\" [truncated]", &s_str[..99])
                                };
                                let bytes = quoted.as_bytes();
                                let copy_len = bytes.len().min(114);
                                pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                                pbuf[copy_len] = 0;
                            }
                        } else {
                            let full = format!("character,{}", LENGTH(s_i));
                            let bytes = full.as_bytes();
                            let copy_len = bytes.len().min(114);
                            pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                            pbuf[copy_len] = 0;
                        }
                    }
                    _t if _t == SEXPTYPE::RAWSXP => {
                        let full = format!("raw,{}", LENGTH(s_i));
                        let bytes = full.as_bytes();
                        let copy_len = bytes.len().min(114);
                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        pbuf[copy_len] = 0;
                    }
                    _t if _t == SEXPTYPE::LISTSXP || _t == SEXPTYPE::VECSXP => {
                        let full = format!("list,{}", LENGTH(s_i));
                        let bytes = full.as_bytes();
                        let copy_len = bytes.len().min(114);
                        pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        pbuf[copy_len] = 0;
                    }
                    _t if _t == SEXPTYPE::LANGSXP => {
                        let src = b"expression\0";
                        pbuf[..src.len()].copy_from_slice(src);
                    }
                    _ => {
                        let src = b"?\0";
                        pbuf[..src.len()].copy_from_slice(src);
                    }
                }
            }
            pbuf[114] = 0;
            SET_STRING_ELT(t, i, mk_char_nul(&pbuf));
        }

        if LENGTH(dims) == 2 {
            print_matrix_dimnamed(t, s, dims, 0, data.right);
        } else {
            let names = GetArrayDimnames(s);
            let _names_guard = protect(names);
            crate::mainutils::printarray::printArray(t, dims, 0, Rprt_adj_left, names);
        }
    } else {
        // No dim
        let names = getAttrib(s, R_NamesSymbol());
        let _names_guard = protect(names);

        let taglen = tagbuf_strlen();
        let ptag = taglen;
        let sz = TAGBUFLEN0 * 2 - taglen;

        if ns > 0 {
            let n_pr = if ns <= data.max as i64 + 1 {
                ns
            } else {
                data.max as i64
            };
            for i in 0..n_pr {
                if i > 0 {
                    println!();
                }

                if names != R_NilValue() && i < XLENGTH(names) {
                    let name_elt = STRING_ELT(names, i);
                    if !name_elt.is_null() && name_elt != R_NilValue() {
                        let name_chars = char_bytes(name_elt);
                        if !name_chars.is_empty() {
                            let name_len = name_chars.len();
                            if taglen + name_len > TAGBUFLEN {
                                if taglen <= TAGBUFLEN {
                                    write_tag(ptag, sz, "$...");
                                }
                            } else {
                                let na_str = NA_STRING_local();
                                if name_elt == na_str {
                                    write_tag(ptag, sz, "$<NA>");
                                } else if isValidName(&name_chars) {
                                    let out = r_sprintf("$%s", &[CArg::Str(&name_chars)]);
                                    write_tag(ptag, sz, &out);
                                } else {
                                    let enc =
                                        crate::mainutils::printutils::encode_char_bytes(name_elt);
                                    match enc {
                                        Some(enc) if isValidName(&enc) => {
                                            let out = r_sprintf("$%s", &[CArg::Str(&enc)]);
                                            write_tag(ptag, sz, &out);
                                        }
                                        Some(enc) => {
                                            let out = r_sprintf("$`%s`", &[CArg::Str(&enc)]);
                                            write_tag(ptag, sz, &out);
                                        }
                                        None => write_tag(ptag, sz, "$..."),
                                    }
                                }
                            }
                        } else {
                            let iw = crate::mainutils::printutils::IndexWidth_xlen(i);
                            if taglen + iw as usize > TAGBUFLEN {
                                if taglen <= TAGBUFLEN {
                                    write_tag(ptag, sz, "$...");
                                }
                            } else {
                                let out = r_sprintf("[[%lld]]", &[(i + 1).into()]);
                                write_tag(ptag, sz, &out);
                            }
                        }
                    } else {
                        let iw = crate::mainutils::printutils::IndexWidth_xlen(i);
                        if taglen + iw as usize > TAGBUFLEN {
                            if taglen <= TAGBUFLEN {
                                write_tag(ptag, sz, "$...");
                            }
                        } else {
                            let out = r_sprintf("[[%lld]]", &[(i + 1).into()]);
                            write_tag(ptag, sz, &out);
                        }
                    }
                } else {
                    let iw = crate::mainutils::printutils::IndexWidth_xlen(i);
                    if taglen + iw as usize > TAGBUFLEN {
                        if taglen <= TAGBUFLEN {
                            write_tag(ptag, sz, "$...");
                        }
                    } else {
                        let out = r_sprintf("[[%lld]]", &[(i + 1).into()]);
                        write_tag(ptag, sz, &out);
                    }
                }

                // Print tag line
                println!("{}", tagbuf_string());

                PrintDispatch(VECTOR_ELT(s, i), data);

                tagbuf_set(taglen, 0);
            }
            println!();
            if (n_pr as i64) < ns {
                println!(
                    " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]",
                    ns - n_pr as i64
                );
            }
        } else {
            let named = names != R_NilValue() || {
                let mut cell = crate::sexp::accessors::ATTRIB(s);
                let mut found = false;
                while !cell.is_null() && cell != R_NilValue() {
                    let tag = crate::sexp::accessors::TAG(cell);
                    if !tag.is_null() && tag != R_NilValue() {
                        let pn = crate::sexp::accessors::PRINTNAME(tag);
                        if !pn.is_null() {
                            if char_bytes(pn) == b"names" {
                                found = true;
                                break;
                            }
                        }
                    }
                    cell = crate::sexp::accessors::CDR(cell);
                }
                found
            };
            if named {
                print!("named ");
            }
            println!("list()");
        }
    }
    printAttributes(s, data, false);
}

// ---------------------------------------------------------------------------
// Internal: printList
// ---------------------------------------------------------------------------

// Detect malformed CDR cycles without allocating or truncating valid long lists.
fn validate_print_chain(head: SEXP) {
    {
        let next = |cell: SEXP| {
            if cell.is_null() || cell == R_NilValue() || TYPEOF(cell) != SEXPTYPE::LISTSXP {
                R_NilValue()
            } else {
                CDR(cell)
            }
        };
        let mut slow = head;
        let mut fast = head;
        loop {
            crate::eval::limits::poll_computation();
            slow = next(slow);
            fast = next(next(fast));
            if fast.is_null() || fast == R_NilValue() {
                return;
            }
            if slow == fast {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "cyclic pairlist in legacy printing".into(),
                });
            }
        }
    }
}

fn printList(s: SEXP, data: &R_PrintData) {
    validate_print_chain(s);
    let dims = getAttrib(s, R_DimSymbol());

    if dims != R_NilValue() && LENGTH(dims) > 1 {
        let _dims_guard = protect(dims);
        let t = allocArray(SEXPTYPE::STRSXP.as_c_int(), dims);
        let _t_guard = protect(t);
        let mut i: i64 = 0;
        let mut cur = s;

        while cur != R_NilValue() && TYPEOF(cur) == SEXPTYPE::LISTSXP {
            let mut pbuf = [0u8; 101];
            match TYPEOF(CAR(cur)) {
                _t if _t == SEXPTYPE::NILSXP => {
                    let src = b"NULL\0";
                    pbuf[..src.len()].copy_from_slice(src);
                }
                _t if _t == SEXPTYPE::LGLSXP => {
                    let full = format!("logical,{}", LENGTH(CAR(cur)));
                    let bytes = full.as_bytes();
                    let copy_len = bytes.len().min(100);
                    pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    pbuf[copy_len] = 0;
                }
                _t if _t == SEXPTYPE::INTSXP || _t == SEXPTYPE::REALSXP => {
                    let full = format!("numeric,{}", LENGTH(CAR(cur)));
                    let bytes = full.as_bytes();
                    let copy_len = bytes.len().min(100);
                    pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    pbuf[copy_len] = 0;
                }
                _t if _t == SEXPTYPE::CPLXSXP => {
                    let full = format!("complex,{}", LENGTH(CAR(cur)));
                    let bytes = full.as_bytes();
                    let copy_len = bytes.len().min(100);
                    pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    pbuf[copy_len] = 0;
                }
                _t if _t == SEXPTYPE::STRSXP => {
                    let full = format!("character,{}", LENGTH(CAR(cur)));
                    let bytes = full.as_bytes();
                    let copy_len = bytes.len().min(100);
                    pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    pbuf[copy_len] = 0;
                }
                _t if _t == SEXPTYPE::RAWSXP => {
                    let full = format!("raw,{}", LENGTH(CAR(cur)));
                    let bytes = full.as_bytes();
                    let copy_len = bytes.len().min(100);
                    pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    pbuf[copy_len] = 0;
                }
                _t if _t == SEXPTYPE::LISTSXP => {
                    let full = format!("list,{}", LENGTH(CAR(cur)));
                    let bytes = full.as_bytes();
                    let copy_len = bytes.len().min(100);
                    pbuf[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    pbuf[copy_len] = 0;
                }
                _t if _t == SEXPTYPE::LANGSXP => {
                    let src = b"expression\0";
                    pbuf[..src.len()].copy_from_slice(src);
                }
                _ => {
                    let src = b"?\0";
                    pbuf[..src.len()].copy_from_slice(src);
                }
            }
            pbuf[100] = 0;
            SET_STRING_ELT(t, i, mk_char_nul(&pbuf));
            cur = CDR(cur);
            i += 1;
        }

        if LENGTH(dims) == 2 {
            print_matrix_dimnamed(t, s, dims, data.quote, data.right);
        } else {
            let dimnames = getAttrib(s, R_DimNamesSymbol());
            let _dimnames_guard = protect(dimnames);
            crate::mainutils::printarray::printArray(t, dims, 0, Rprt_adj_left, dimnames);
        }
    } else {
        let mut i: c_int = 1;
        let mut cur = s;

        let taglen = tagbuf_strlen();
        let ptag = taglen;
        let sz = TAGBUFLEN0 * 2 - taglen;

        while TYPEOF(cur) == SEXPTYPE::LISTSXP {
            if i > 1 {
                println!();
            }

            let tag = TAG(cur);
            if tag != R_NilValue() && isSymbol(tag) != 0 {
                let pname = PRINTNAME(tag);
                let name_chars = char_bytes(pname);
                let name_len = name_chars.len();
                if taglen + name_len > TAGBUFLEN {
                    if taglen <= TAGBUFLEN {
                        write_tag(ptag, sz, "$...");
                    }
                } else {
                    let na_str = NA_STRING_local();
                    if pname == na_str {
                        write_tag(ptag, sz, "$<NA>");
                    } else if isValidName(&name_chars) {
                        let out = r_sprintf("$%s", &[CArg::Str(&name_chars)]);
                        write_tag(ptag, sz, &out);
                    } else {
                        match crate::mainutils::printutils::encode_char_bytes(pname) {
                            Some(enc) if isValidName(&enc) => {
                                let out = r_sprintf("$%s", &[CArg::Str(&enc)]);
                                write_tag(ptag, sz, &out);
                            }
                            Some(enc) => {
                                let out = r_sprintf("$`%s`", &[CArg::Str(&enc)]);
                                write_tag(ptag, sz, &out);
                            }
                            None => write_tag(ptag, sz, "$..."),
                        }
                    }
                }
            } else {
                let iw = crate::mainutils::printutils::IndexWidth_xlen(i as i64);
                if taglen + iw as usize > TAGBUFLEN {
                    if taglen <= TAGBUFLEN {
                        write_tag(ptag, sz, "$...");
                    }
                } else {
                    let out = r_sprintf("[[%d]]", &[i.into()]);
                    write_tag(ptag, sz, &out);
                }
            }

            println!("{}", tagbuf_string());

            PrintDispatch(CAR(cur), data);

            tagbuf_set(taglen, 0);
            cur = CDR(cur);
            i += 1;
        }

        if cur != R_NilValue() {
            print!("\n. \n\n");
            PrintValueRec_inner(cur, data);
        }
        println!();
    }
    printAttributes(s, data, false);
}

// ---------------------------------------------------------------------------
// Internal: printAttributes
// ---------------------------------------------------------------------------

fn printAttributes(s: SEXP, data: &R_PrintData, useSlots: bool) {
    let mut a = ATTRIB(s);
    if a.is_null() || a == R_NilValue() {
        return;
    }

    let current_len = tagbuf_strlen();
    if current_len > TAGBUFLEN0 {
        return;
    }

    let _tag_guard = PrintTagGuard::save();

    // Remove list tag if it looks like a list, not an attribute
    if current_len > 0 {
        if tagbuf_byte_at(current_len - 1) != b')' {
            tagbuf_clear();
        }
    }

    let comment_sym = install_str("comment");
    let srcref_sym = install_str("srcref");
    let whole_srcref_sym = install_str("wholeSrcref");
    let srcfile_sym = install_str("srcfile");
    while !a.is_null() && a != R_NilValue() {
        let tag = TAG(a);

        // Skip certain attributes
        if useSlots && tag == R_ClassSymbol() {
            a = CDR(a);
            continue;
        }
        if isArray(s) != 0 && (tag == R_DimSymbol() || tag == R_DimNamesSymbol()) {
            a = CDR(a);
            continue;
        }
        if inherits_cstr(s, "factor") != 0 && (tag == R_LevelsSymbol() || tag == R_ClassSymbol()) {
            a = CDR(a);
            continue;
        }
        if isDataFrame(s) != 0 && tag == R_RowNamesSymbol() {
            a = CDR(a);
            continue;
        }
        if isArray(s) == 0 && tag == R_NamesSymbol() {
            a = CDR(a);
            continue;
        }
        if tag == comment_sym || tag == srcref_sym || tag == whole_srcref_sym || tag == srcfile_sym
        {
            a = CDR(a);
            continue;
        }

        // Build the attribute tag. GNU keeps `ptag` pointing at the
        // start of this tag and truncates back to it (`*ptag = '\0'`)
        // after the value prints, so nested attribute values render
        // `attr(,"outer")attr(,"inner")` without accumulating stale
        // tags from earlier siblings.
        let tag_start = tagbuf_strlen();
        let space = TAGBUFLEN0 - tag_start;
        let ptag_start = tag_start;

        if !tag.is_null() && isSymbol(tag) != 0 {
            let pname = PRINTNAME(tag);
            if let Some(enc) = crate::mainutils::printutils::encode_char_bytes(pname) {
                if useSlots {
                    let out = r_sprintf("Slot \"%s\":", &[CArg::Str(&enc)]);
                    write_tag(ptag_start, space, &out);
                } else {
                    let out = r_sprintf("attr(,\"%s\")", &[CArg::Str(&enc)]);
                    write_tag(ptag_start, space, &out);
                }
            }
        }

        // Route the header through the console sink so captured
        // sessions see it in order (std println! bypasses capture).
        crate::mainutils::printutils::console_emit(format_args!("{}\n", tagbuf_string()));

        if tag == R_RowNamesSymbol() {
            let val = getAttrib(s, R_RowNamesSymbol());
            let _val_guard = protect(val);
            PrintValueRec_inner(val, data);
            tagbuf_set(tag_start, 0);
            a = CDR(a);
            continue;
        }

        PrintDispatch(CAR(a), data);
        tagbuf_set(tag_start, 0);
        a = CDR(a);
    }
}

// ---------------------------------------------------------------------------
// PrintValueRec
// ---------------------------------------------------------------------------

pub unsafe fn PrintValueRec(s: SEXP, _data: *mut std::ffi::c_void) {
    unsafe {
        let data = _data as *const R_PrintData;
        if data.is_null() {
            PrintDefaults();
            let data = print_data_snapshot();
            PrintValueRec_inner(s, &data.raw);
        } else {
            PrintValueRec_inner(s, &*data);
        }
    }
}

/// GNU `do_printdefault`'s else-branch — `PrintValueRec` called on an object
/// whose `show()` wrapper is still active (methods' showDefault re-enters via
/// `print(object, useS4 = FALSE)`). GNU has no cycle guard on this path; only
/// true nested-element cycles (re-entry from *within* element printing) must
/// stay recoverable errors, so temporarily lift the outer guard.
fn PrintValueRec_reroot(s: SEXP, data: &R_PrintData) {
    let _suspended = suspend_print_value(s);
    PrintValueRec_inner(s, data);
}

fn PrintValueRec_inner(s: SEXP, data: &R_PrintData) {
    if s.is_null() {
        return;
    }

    // GNU showDefault's `print(object, useS4 = FALSE)` lands here.
    // Printing the data part must not re-enter show() (the recursion stack)
    // and must use the capture-aware printer. With methods dispatch
    // on, GNU PrintValueRec prints S4 objects by TYPEOF (deparse
    // renders `new("Class", ...)` for closures) followed by
    // printAttributes — so only the dispatch-off case keeps the
    // stripped data-part shortcut.
    if IS_S4_OBJECT(s) != 0 && isMethodsDispatchOn() == 0 {
        print_s4_data_part(s, data);
        return;
    }

    let _recursion_guard = match enter_print_value(s) {
        Some(guard) => guard,
        None => {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "cyclic object encountered while printing".into(),
            });
        }
    };

    if isMethodsDispatchOn() == 0 && TYPEOF(s) == SEXPTYPE(25).0 {
        crate::mainutils::printutils::console_emit(format_args!("<object>\n"));
        return;
    }

    match TYPEOF(s) {
        t if t == SEXPTYPE::NILSXP => {
            crate::mainutils::printutils::console_emit(format_args!("NULL\n"));
        }
        t if t == SEXPTYPE::SYMSXP => {
            let t = crate::mainutils::deparse::deparse1(s, false, SIMPLEDEPARSE);
            let _t_guard = protect(t);
            set_current_print_data(data);
            let line = STRING_ELT(t, 0);
            if !line.is_null() && line != R_NilValue() {
                crate::mainutils::printutils::console_emit(format_args!(
                    "{}\n",
                    std::str::from_utf8(&char_bytes(line)).unwrap_or("?")
                ));
            }
        }
        t if t == SEXPTYPE::SPECIALSXP || t == SEXPTYPE::BUILTINSXP => {
            PrintSpecial(s, data);
        }
        t if t == SEXPTYPE::CHARSXP => {
            crate::mainutils::printutils::console_emit(format_args!("<CHARSXP: "));
            if let Some(enc) = crate::mainutils::printutils::encode_string_bytes(
                s,
                0,
                b'"' as c_int,
                crate::mainutils::printutils::Rprt_adj::none,
            ) {
                crate::mainutils::printutils::console_emit(format_args!(
                    "{}",
                    std::str::from_utf8(&enc).unwrap_or("")
                ));
            }
            crate::mainutils::printutils::console_emit(format_args!(">\n"));
            return; // skip attributes for CHARSXP
        }
        t if t == SEXPTYPE::EXPRSXP => {
            PrintExpression(s, data);
        }
        t if t == SEXPTYPE::LANGSXP => {
            PrintLanguage(s, data);
        }
        t if t == SEXPTYPE::CLOSXP => {
            PrintClosure(s, data);
        }
        t if t == SEXPTYPE::ENVSXP => {
            if let Some(env_str) = crate::mainutils::printutils::encode_environment_bytes(s) {
                crate::mainutils::printutils::console_emit(format_args!(
                    "{}\n",
                    std::str::from_utf8(&env_str).unwrap_or("<environment>")
                ));
            } else {
                crate::mainutils::printutils::console_emit(format_args!("<environment>\n"));
            }
        }
        t if t == SEXPTYPE::PROMSXP => {
            crate::mainutils::printutils::console_emit(format_args!("<promise: {:?}>\n", s));
        }
        t if t == SEXPTYPE::DOTSXP => {
            crate::mainutils::printutils::console_emit(format_args!("<...>\n"));
        }
        t if t == SEXPTYPE::VECSXP => {
            PrintGenericVector(s, data);
            return; // handles attributes
        }
        t if t == SEXPTYPE::LISTSXP => {
            printList(s, data);
        }
        t if t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::RAWSXP =>
        {
            let dim = getAttrib(s, R_DimSymbol());
            let _dim_guard = protect(dim);
            if TYPEOF(dim) == SEXPTYPE::INTSXP {
                if LENGTH(dim) == 1 {
                    let dnames = getAttrib(s, R_DimNamesSymbol());
                    if dnames != R_NilValue() && VECTOR_ELT(dnames, 0) != R_NilValue() {
                        let nn = getAttrib(dnames, R_NamesSymbol());
                        let title = if nn != R_NilValue() && LENGTH(nn) > 0 {
                            let title_elt = STRING_ELT(nn, 0);
                            if !title_elt.is_null() && title_elt != R_NilValue() {
                                // SAFETY: `title_elt` is a live CHARSXP of the
                                // rooted dimnames, alive across the print call.
                                unsafe { CHAR(title_elt) }
                            } else {
                                ptr::null()
                            }
                        } else {
                            ptr::null()
                        };
                        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                        unsafe {
                            crate::mainutils::printvector::printNamedVector(
                                s,
                                VECTOR_ELT(dnames, 0),
                                data.quote,
                                title,
                            )
                        };
                    } else {
                        crate::mainutils::printvector::printVector(s, 1, data.quote);
                    }
                } else if LENGTH(dim) == 2 {
                    print_matrix_dimnamed(s, s, dim, data.quote, data.right);
                } else {
                    let dimnames = GetArrayDimnames(s);
                    let _dimnames_guard = protect(dimnames);
                    crate::mainutils::printarray::printArray(
                        s, dim, data.quote, data.right, dimnames,
                    );
                }
            } else {
                let names = getAttrib(s, R_NamesSymbol());
                let _names_guard = protect(names);
                if names != R_NilValue() {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    unsafe {
                        crate::mainutils::printvector::printNamedVector(
                            s,
                            names,
                            data.quote,
                            ptr::null(),
                        )
                    };
                } else {
                    crate::mainutils::printvector::printVector(s, 1, data.quote);
                }
                printAttributes(s, data, false);
                return;
            }
        }
        t if t == SEXPTYPE::EXTPTRSXP => {
            crate::mainutils::printutils::console_emit(format_args!("<pointer: {:?}>", s));
        }
        t if t == SEXPTYPE::BCODESXP => {
            crate::mainutils::printutils::console_emit(format_args!("<bytecode: {:?}>", s));
        }
        t if t == SEXPTYPE::WEAKREFSXP => {
            crate::mainutils::printutils::console_emit(format_args!("<weak reference>\n"));
        }
        t if t == SEXPTYPE(25).0 => {
            // OBJSXP
            if IS_S4_OBJECT(s) != 0 {
                crate::mainutils::printutils::console_emit(format_args!("<S4 Type Object>\n"));
            } else {
                crate::mainutils::printutils::console_emit(format_args!("<object>\n"));
            }
        }
        _ => {
            crate::mainutils::printutils::console_emit(format_args!(
                "<unknown type {}>",
                TYPEOF(s)
            ));
        }
    }

    printAttributes(s, data, false);
}

// ---------------------------------------------------------------------------
// PrintValue
// ---------------------------------------------------------------------------

pub fn PrintValue(s: SEXP) {
    PrintValueEnv(s, R_GlobalEnv());
}

// ---------------------------------------------------------------------------
// PrintValueEnv
// ---------------------------------------------------------------------------

pub fn PrintValueEnv(s: SEXP, env: SEXP) {
    if s.is_null() {
        return;
    }

    validate_print_chain(ATTRIB(s));
    let _s_guard = protect(s);
    let _env_guard = protect(env);
    PrintDefaults();
    tagbuf_clear();

    let mut data = R_PRINT_INIT.clone();
    let _roots = print_init(&mut data, env);

    if isFunction(s) != 0 {
        PrintObject(s, &data);
    } else {
        PrintDispatch(s, &data);
    }
}

// ---------------------------------------------------------------------------
// R_PV
// ---------------------------------------------------------------------------

pub fn R_PV(s: SEXP) {
    if !s.is_null() && isObject_fn(s) != 0 {
        PrintValueEnv(s, R_GlobalEnv());
    }
}

// ---------------------------------------------------------------------------
// CustomPrintValue
// ---------------------------------------------------------------------------

pub fn CustomPrintValue(s: SEXP, env: SEXP) {
    tagbuf_clear();

    let mut data = R_PRINT_INIT.clone();
    let _roots = print_init(&mut data, env);
    PrintValueRec_inner(s, &data);
}

// ---------------------------------------------------------------------------
// do_printdefault
// ---------------------------------------------------------------------------

pub fn do_printdefault(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    if Rf_isNull(args) != 0 {
        return ptr::null_mut();
    }

    let x = CAR(args);
    let args_rest = CDR(args);

    let mut data = R_PRINT_INIT.clone();
    let _roots = print_init(&mut data, rho);

    // GNU: x = CAR(args); args = CDR(args);
    // missingArg = LOGICAL(CADR(args)); args = CAR(args);
    // .Internal(print.default(x, wrapped_args, missings))
    if args_rest == R_NilValue() || CDR(args_rest) == R_NilValue() {
        set_current_print_data(&data);
        tagbuf_clear();
        emit_print_default(x, &data, true);
        PrintDefaults();
        return x;
    }

    let wrapped_args = CAR(args_rest);
    let missings_vec = CAR(CDR(args_rest));
    if missings_vec.is_null() || TYPEOF(missings_vec) != SEXPTYPE::LGLSXP {
        set_current_print_data(&data);
        tagbuf_clear();
        emit_print_default(x, &data, true);
        PrintDefaults();
        return x;
    }
    let mut missing_idx: R_xlen_t = 0;
    let mut all_missing: c_int = 1;

    let orig = Rf_cons(R_NilValue(), wrapped_args);
    let _orig_guard = protect(orig);
    let mut prev = orig;
    let mut cur_args = wrapped_args;

    // digits
    if cur_args != R_NilValue() && Rf_isNull(CAR(cur_args)) == 0 {
        data.digits = FixupDigits(CAR(cur_args), 2);
    }
    if cur_args != R_NilValue() {
        advancePrintArgs(
            &mut cur_args,
            &mut prev,
            missings_vec,
            &mut missing_idx,
            &mut all_missing,
        );
    }

    // quote
    if cur_args != R_NilValue() {
        data.quote = asLogical(CAR(cur_args));
        if data.quote == NA_LOGICAL {
            data.quote = 1;
        }
        advancePrintArgs(
            &mut cur_args,
            &mut prev,
            missings_vec,
            &mut missing_idx,
            &mut all_missing,
        );
    }

    // na.print
    if cur_args != R_NilValue() {
        let naprint = CAR(cur_args);
        if Rf_isNull(naprint) == 0 && isString(naprint) != 0 && LENGTH(naprint) >= 1 {
            let na_str = STRING_ELT(naprint, 0);
            data.na_string = na_str;
            data.na_string_noquote = na_str;
            data.na_width = crate::mainutils::printutils::Rstrlen(data.na_string, 0);
            data.na_width_noquote = data.na_width;
        }
        advancePrintArgs(
            &mut cur_args,
            &mut prev,
            missings_vec,
            &mut missing_idx,
            &mut all_missing,
        );
    }

    // gap
    if cur_args != R_NilValue() {
        let gap = CAR(cur_args);
        if Rf_isNull(gap) == 0 {
            data.gap = asInteger(gap);
            if data.gap == NA_INTEGER || data.gap < 0 {
                crate::sexp::context::r_error("'gap' must be non-negative integer");
            }
            if data.gap > 1024 {
                crate::sexp::context::r_error("'print.gap' must be less than 1024");
            }
        }
        advancePrintArgs(
            &mut cur_args,
            &mut prev,
            missings_vec,
            &mut missing_idx,
            &mut all_missing,
        );
    }

    // right
    if cur_args != R_NilValue() {
        let right_val = asLogical(CAR(cur_args));
        if right_val != NA_LOGICAL {
            data.right = right_val;
        }
        advancePrintArgs(
            &mut cur_args,
            &mut prev,
            missings_vec,
            &mut missing_idx,
            &mut all_missing,
        );
    }

    // max
    if cur_args != R_NilValue() {
        let max_val = CAR(cur_args);
        if Rf_isNull(max_val) == 0 {
            data.max = asInteger(max_val);
            if data.max == NA_INTEGER || data.max < 0 {
                data.max = 99999;
            } else if data.max == c_int::MAX {
                data.max -= 1;
            }
        }
        advancePrintArgs(
            &mut cur_args,
            &mut prev,
            missings_vec,
            &mut missing_idx,
            &mut all_missing,
        );
    }

    // width
    if cur_args != R_NilValue() {
        let width_val = CAR(cur_args);
        if Rf_isNull(width_val) == 0 {
            data.width = FixupWidth(width_val, 2);
        }
        advancePrintArgs(
            &mut cur_args,
            &mut prev,
            missings_vec,
            &mut missing_idx,
            &mut all_missing,
        );
    }

    // useSource
    if cur_args != R_NilValue() {
        let use_source = asLogical(CAR(cur_args));
        if use_source != NA_LOGICAL {
            data.useSource = if use_source != 0 { USESOURCE } else { 0 };
        }
        advancePrintArgs(
            &mut cur_args,
            &mut prev,
            missings_vec,
            &mut missing_idx,
            &mut all_missing,
        );
    }

    let no_params = if all_missing != 0 && cur_args == R_NilValue() {
        1
    } else {
        0
    };
    data.callArgs = CDR(orig);

    set_current_print_data(&data);

    tagbuf_clear();

    emit_print_default(x, &data, no_params != 0);

    PrintDefaults();
    x
}

// ---------------------------------------------------------------------------
// do_prmatrix
// ---------------------------------------------------------------------------

pub fn do_prmatrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let count = LENGTH(args);
    if count != 6 {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::condition_caller_call(),
            &format!("{count} arguments passed to .Internal(prmatrix) which requires 6"),
        );
    }
    let mut a = args;
    let x = CAR(a);
    a = CDR(a);
    let rowlab = CAR(a);
    a = CDR(a);
    let collab = CAR(a);
    a = CDR(a);

    let quote = asInteger(CAR(a));
    a = CDR(a);
    let right = asInteger(CAR(a));
    a = CDR(a);
    let naprint = CAR(a);

    let caller = crate::mainutils::errors::condition_caller_call();
    let dim = getAttrib(x, R_DimSymbol());
    if dim.is_null()
        || TYPEOF(dim) != SEXPTYPE::INTSXP
        || LENGTH(dim) != 2
        || INTEGER_ELT(dim, 0) < 0
        || INTEGER_ELT(dim, 1) < 0
    {
        crate::mainutils::errors::errorcall_str(caller, "invalid matrix argument");
    }
    if Rf_isNull(naprint) == 0 && (isString(naprint) == 0 || LENGTH(naprint) < 1) {
        crate::mainutils::errors::errorcall_str(caller, "invalid 'na.print' specification");
    }
    for (labels, extent, axis) in [
        (rowlab, INTEGER_ELT(dim, 0), "row"),
        (collab, INTEGER_ELT(dim, 1), "column"),
    ] {
        if Rf_isNull(labels) == 0 && isString(labels) == 0 {
            crate::mainutils::errors::errorcall_str(caller, &format!("invalid {axis} labels"));
        }
        if LENGTH(labels) != 0 && LENGTH(labels) < extent {
            crate::mainutils::errors::errorcall_str(caller, &format!("too few {axis} labels"));
        }
    }

    with_required_current_instance(|inst| {
        // SAFETY: instance is pinned and this updates only a scalar.
        unsafe { with_print_data_in(inst, |state| state.raw.right = right) }
    });

    if Rf_isNull(naprint) == 0 && isString(naprint) != 0 && LENGTH(naprint) >= 1 {
        let na_str = STRING_ELT(naprint, 0);
        let na_width = crate::mainutils::printutils::Rstrlen(na_str, 0);
        let mut data = with_current_print_data(Clone::clone);
        data.na_string = na_str;
        data.na_string_noquote = na_str;
        data.na_width = na_width;
        data.na_width_noquote = na_width;
        set_current_print_data(&data);
    }

    let mut rowlab_use = rowlab;
    let mut collab_use = collab;
    if LENGTH(rowlab) == 0 {
        rowlab_use = R_NilValue();
    }
    if LENGTH(collab) == 0 {
        collab_use = R_NilValue();
    }

    let right_val = with_current_print_data(|data| data.right);
    crate::mainutils::printarray::printMatrix(
        x,
        0,
        dim,
        quote,
        right_val,
        rowlab_use,
        collab_use,
        None,
        None,
    );
    PrintDefaults();
    crate::sexp::globals::set_R_Visible(0);
    x
}

// ---------------------------------------------------------------------------
// do_unclass
// ---------------------------------------------------------------------------

pub fn do_unclass(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = CAR(args);
    if !x.is_null() {
        crate::sexp::attrib_core::setAttrib(x, R_ClassSymbol(), R_NilValue());
    }
    x
}

// ---------------------------------------------------------------------------
// isEnvironment (public, #[unsafe(no_mangle)])
// ---------------------------------------------------------------------------

pub fn isEnvironment(x: SEXP) -> c_int {
    {
        if x.is_null() {
            return 0;
        }
        if TYPEOF(x) == SEXPTYPE::ENVSXP { 1 } else { 0 }
    }
}

// ---------------------------------------------------------------------------
// inherits (public, #[unsafe(no_mangle)])
// ---------------------------------------------------------------------------

pub fn inherits(x: SEXP, class: SEXP, _pkg: SEXP) -> c_int {
    if class.is_null() || TYPEOF(class) != SEXPTYPE::STRSXP || LENGTH(class) < 1 {
        return 0;
    }
    let class_name = char_bytes(STRING_ELT(class, 0));
    match std::str::from_utf8(&class_name) {
        Ok(class_name) => inherits_cstr(x, class_name),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Module-private helpers
// ---------------------------------------------------------------------------

fn GetOptionDigits() -> c_int {
    crate::mainutils::options::GetOptionDigits()
}

fn GetOption1(sym: SEXP) -> SEXP {
    crate::mainutils::options::GetOption1(sym)
}

fn do_names(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue()
}

fn do_dim(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue()
}

fn do_str(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::with_scalar_print_state_in;
    use crate::sexp::session::RSession;
    use std::ptr;

    #[test]
    fn print_format_cleanup_restores_revoked_original_owner_without_touching_foreign_owner() {
        let mut original = RSession::new_for_gc_tests();
        let (guard, observer, weak, previous) = original.with_active(|| {
            let owner = original.owner_token().unwrap().weak_owner().unwrap();
            let observer = owner.pin().unwrap();
            let previous = unsafe {
                with_scalar_print_state_in(observer.as_ptr(), |cell| cell.get().print)
            };
            let mut data = R_PRINT_INIT.clone();
            data.digits = 2;
            (PrintFormatGuard::new(&data), observer, owner, previous)
        });
        original.close();
        drop(original);
        assert_eq!(weak.allocation_strong_count(), 2);
        assert!(matches!(
            weak.pin(),
            Err(crate::sexp::object::SexpError::RootUnavailable)
        ));
        let foreign = RSession::new_for_gc_tests();
        foreign.with_active(|| {
            with_current_scalar_print_state(|cell| {
                let mut state = cell.get();
                state.print.digits = 11;
                cell.set(state);
            });
            drop(guard);
            assert_eq!(
                with_current_scalar_print_state(|cell| cell.get().print.digits),
                Some(11)
            );
            assert_eq!(
                unsafe { with_scalar_print_state_in(observer.as_ptr(), |cell| cell.get().print.digits) },
                previous.digits
            );
        });
        assert_eq!(weak.allocation_strong_count(), 1);
        drop(observer);
        assert_eq!(weak.allocation_strong_count(), 0);
    }

    #[test]
    fn print_recursion_guard_cleans_up_after_original_session_is_destroyed() {
        let mut original = RSession::new_for_gc_tests();
        let (guard, original_stack) = original.with_active(|| {
            let value = R_NilValue();
            let guard = enter_print_value(value).unwrap();
            assert!(enter_print_value(value).is_none());
            (guard, print_recursion_stack())
        });
        original.close();
        drop(original);

        let foreign = RSession::new_for_gc_tests();
        foreign.with_active(|| {
            let value = R_NilValue();
            let _foreign_guard = enter_print_value(value).unwrap();
            drop(guard);
            assert!(original_stack.borrow().is_empty());
            assert!(enter_print_value(value).is_none());
        });
    }

    #[test]
    fn print_recursion_suspension_restores_original_frame_on_unwind() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let original = RSession::new_for_gc_tests();
        let foreign = RSession::new_for_gc_tests();
        original.with_active(|| {
            let value = R_NilValue();
            let outer = enter_print_value(value).unwrap();
            let result = catch_unwind(AssertUnwindSafe(|| {
                let _suspended = suspend_print_value(value).unwrap();
                {
                    let _inner = enter_print_value(value).unwrap();
                    assert!(enter_print_value(value).is_none());
                }
                foreign.with_active(|| {
                    let _foreign_guard = enter_print_value(value).unwrap();
                    panic!("forced print unwind");
                });
            }));
            assert!(result.is_err());
            assert!(enter_print_value(value).is_none());
            assert_eq!(print_recursion_stack().borrow().len(), 1);
            drop(outer);
            assert!(print_recursion_stack().borrow().is_empty());
        });
        foreign.with_active(|| assert!(print_recursion_stack().borrow().is_empty()));
    }

    #[test]
    fn test_do_printdefault_null() {
        unsafe {
            let result = do_printdefault(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_get_option_digits() {
        let _session = RSession::new();
        unsafe {
            assert!(GetOptionDigits() > 0);
        }
    }

    #[test]
    fn test_is_symbol_null() {
        {
            assert_eq!(isSymbol(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_is_environment_null() {
        {
            assert_eq!(isEnvironment(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_is_string_null() {
        {
            assert_eq!(isString(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_is_null_helper() {
        {
            assert_eq!(Rf_isNull(ptr::null_mut()), 1);
            assert_eq!(Rf_isNull(R_NilValue()), 1);
        }
    }

    #[test]
    fn test_print_data_init() {
        assert_eq!(R_PRINT_INIT.width, 80);
        assert_eq!(R_PRINT_INIT.gap, 1);
        assert_eq!(R_PRINT_INIT.digits, 4);
        assert_eq!(R_PRINT_INIT.scipen, 0);
        assert_eq!(R_PRINT_INIT.max, 99999);
        assert_eq!(R_PRINT_INIT.right, 0);
        assert_eq!(R_PRINT_INIT.quote, 1);
    }

    #[test]
    fn test_constants() {
        assert_eq!(Rprt_adj_left, 0);
        assert_eq!(Rprt_adj_right, 1);
        assert_eq!(TAGBUFLEN, 256);
        assert_eq!(TAGBUFLEN0, 262);
    }

    #[test]
    fn test_tagbuf_session_owned() {
        let original = RSession::new_for_gc_tests();
        let other = RSession::new_for_gc_tests();
        original.with_active(|| {
            write_tag(0, 6, "hello");
            other.with_active(|| {
                assert_eq!(tagbuf_string(), "");
                write_tag(0, 6, "other");
            });
            assert_eq!(tagbuf_string(), "hello");
        });
    }

    #[test]
    fn test_print_defaults() {
        let _session = RSession::new();
        unsafe {
            PrintDefaults();
            with_current_print_data(|pd| {
                assert!(pd.digits > 0);
                assert!(pd.width > 0);
            });
        }
    }

    #[test]
    fn print_defaults_publishes_after_collecting_option_read() {
        use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
        use std::{cell::Cell, rc::Rc};

        struct Digits(Rc<Cell<usize>>);
        impl AltrepClass for Digits {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::INTSXP
            }

            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
                Ok(1)
            }

            fn element<'s>(&self, cx: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
                self.0.set(self.0.get() + 1);
                cx.gc()?;
                with_current_print_data(|current| {
                    assert_eq!(current.digits, R_PRINT_INIT.digits);
                    assert!(current.na_string_noquote.is_null());
                });
                Ok(AltrepElement::Integer(11))
            }
        }

        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let reads = Rc::new(Cell::new(0));
            let class = session
                .register_altrep_class("print-default-digits", Digits(reads.clone()))
                .unwrap();
            let option = AltrepBuilder::new(class).build().unwrap();
            crate::mainutils::options::SetOptionByName("digits", option.as_raw());
            assert_eq!(reads.get(), 0);
            PrintDefaults();
            assert_eq!(reads.get(), 1);
            let current = print_data_snapshot();
            assert_eq!(current.raw.digits, 11);
            assert_eq!(char_bytes(current.raw.na_string_noquote), b"<NA>");
        });
    }

    #[test]
    fn owned_print_data_survives_gc_and_replacement() {
        use crate::sexp::memory::checked_projection;

        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let pointers = {
                let quoted = factory.character("custom quoted NA label").unwrap();
                let unquoted = factory.character("custom unquoted NA label").unwrap();
                let env = factory
                    .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::ENVSXP)))
                    .unwrap();
                let args = factory
                    .pairlist_cell(&factory.nil(), &factory.nil(), &factory.nil())
                    .unwrap();
                let raw = R_PrintData {
                    na_string: quoted.as_raw(),
                    na_string_noquote: unquoted.as_raw(),
                    env: env.as_raw(),
                    callArgs: args.as_raw(),
                    ..R_PRINT_INIT
                };
                set_current_print_data(&raw);
                [
                    raw.na_string,
                    raw.na_string_noquote,
                    raw.env,
                    raw.callArgs,
                ]
            };
            owner.full_gc().unwrap();
            assert!(pointers.iter().all(|&ptr| checked_projection(ptr).is_some()));
            let retained = print_data_snapshot();
            set_current_print_data(&R_PRINT_INIT);
            owner.full_gc().unwrap();
            assert!(pointers.iter().all(|&ptr| checked_projection(ptr).is_some()));
            assert_eq!(char_bytes(retained.raw.na_string), b"custom quoted NA label");
            assert_eq!(
                char_bytes(retained.raw.na_string_noquote),
                b"custom unquoted NA label"
            );
            assert_eq!(TYPEOF(retained.raw.env), SEXPTYPE::ENVSXP);
            assert_eq!(TYPEOF(retained.raw.callArgs), SEXPTYPE::LISTSXP);
            drop(retained);
            owner.full_gc().unwrap();
            assert!(pointers.iter().all(|&ptr| checked_projection(ptr).is_none()));
        });
    }

    #[test]
    fn test_print_value_null() {
        let _session = RSession::new();
        unsafe {
            PrintValue(ptr::null_mut());
        }
    }

    #[test]
    fn test_print_value_rec_null() {
        unsafe {
            let mut data = R_PRINT_INIT.clone();
            PrintValueRec(
                ptr::null_mut(),
                &mut data as *mut R_PrintData as *mut std::ffi::c_void,
            );
        }
    }

    #[test]
    fn legacy_s3_print_dispatches_and_recovers_after_method_error() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_code_with_output_capture(
            "print.foo <- function(x, ...) cat('custom', x$value, '\n'); x <- structure(list(value=7), class='foo'); x"
        );
        let value = result.unwrap().as_raw();
        session.with_active(|| unsafe {
            let capture = crate::sexp::output::OutputCaptureGuard::start();
            PrintValueEnv(value, R_GlobalEnv());
            assert_eq!(capture.finish().stdout.trim(), "custom 7");
        });
        let (result, _, _) = session
            .eval_code_with_output_capture("print.foo <- function(x, ...) stop('method failed')");
        result.unwrap();
        session.with_active(|| {
            let capture = crate::sexp::output::OutputCaptureGuard::start();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                PrintValueEnv(value, R_GlobalEnv());
            }));
            assert!(result.is_err());
            drop(capture);
            assert!(!crate::sexp::output::is_capturing());
        });
        let (result, _, _) = session.eval_code_with_output_capture("1+1");
        assert_eq!(result.unwrap().real_elt(0), Some(2.0));
    }

    #[test]
    fn legacy_print_scalar_without_attributes_terminates() {
        const CHILD: &str = "RPORT_LEGACY_PRINT_PROBE_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let session = RSession::new_for_gc_tests();
            session.with_active(|| unsafe {
                let value = crate::sexp::constructors::Rf_ScalarReal(1.0);
                let _value_root = protect(value);
                assert!(ATTRIB(value).is_null());
                PrintValueEnv(value, R_GlobalEnv());
            });
            return;
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "mainutils::print::tests::legacy_print_scalar_without_attributes_terminates",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Keep a deadline in case printing fails to terminate.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "legacy printing child failed: {status}");
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("legacy scalar printing did not terminate within 20 seconds");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn legacy_print_cyclic_pairlists_terminate() {
        const CHILD: &str = "RPORT_LEGACY_PRINT_CYCLE_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let session = RSession::new_for_gc_tests();
            session.with_active(|| unsafe {
                let cell = Rf_cons(crate::sexp::constructors::Rf_ScalarInteger(1), R_NilValue());
                let _root = protect(cell);
                SETCDR(cell, cell);
                let cycle = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    PrintValueEnv(cell, R_GlobalEnv())
                }));
                assert!(cycle.is_err());
                let value = crate::sexp::constructors::Rf_ScalarInteger(1);
                let _value_root = protect(value);
                crate::sexp::accessors::SET_ATTRIB(value, cell);
                let cycle = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    PrintValueEnv(value, R_GlobalEnv())
                }));
                assert!(cycle.is_err());
                crate::sexp::accessors::SET_ATTRIB(value, R_NilValue());
                PrintValueEnv(value, R_GlobalEnv());
            });
            return;
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "mainutils::print::tests::legacy_print_cyclic_pairlists_terminate",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Keep a deadline in case cyclic traversal fails to terminate.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "cyclic printing child failed: {status}");
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("cyclic legacy printing did not terminate within 20 seconds");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn legacy_print_cyclic_nested_vectors_terminate() {
        const CHILD: &str = "RPORT_LEGACY_PRINT_NESTED_CYCLE_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let session = RSession::new_for_gc_tests();
            session.with_active(|| unsafe {
                let value = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::VECSXP, 1);
                let _root = protect(value);
                crate::sexp::accessors::SET_VECTOR_ELT(value, 0, value);
                let cycle = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    PrintValueEnv(value, R_GlobalEnv());
                }));
                let error = cycle.expect_err("cycle should return a recoverable error");
                assert!(
                    error
                        .downcast_ref::<crate::sexp::context::RError>()
                        .is_some_and(|error| error.message.contains("cyclic object"))
                );
                let ordinary = crate::sexp::constructors::Rf_ScalarInteger(2);
                let _ordinary_root = protect(ordinary);
                PrintValueEnv(ordinary, R_GlobalEnv());
            });
            return;
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "mainutils::print::tests::legacy_print_cyclic_nested_vectors_terminate",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Keep a deadline in case cyclic traversal fails to terminate.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "nested cycle child failed: {status}");
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("nested cyclic printing did not terminate within 20 seconds");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn test_print_value_env_null() {
        let _session = RSession::new();
        unsafe {
            PrintValueEnv(ptr::null_mut(), R_GlobalEnv());
        }
    }

    #[test]
    fn test_r_pv_null() {
        unsafe {
            R_PV(ptr::null_mut());
        }
    }

    #[test]
    fn test_custom_print_value_null() {
        let _session = RSession::new();
        unsafe {
            CustomPrintValue(ptr::null_mut(), ptr::null_mut());
        }
    }

    #[test]
    fn test_is_valid_name() {
        unsafe {
            assert!(isValidName(b"foo"));
            assert!(isValidName(b".foo"));
            assert!(isValidName(b"foo_bar"));
            assert!(isValidName(b"."));
            assert!(isValidName(b"..."));
            assert!(!isValidName(b"123"));
            assert!(!isValidName(b".1"));
            assert!(!isValidName(b"if"));
            assert!(!isValidName(b"TRUE"));
            assert!(!isValidName(b""));
        }
    }

    #[test]
    fn test_inherits_null() {
        unsafe {
            assert_eq!(
                inherits(ptr::null_mut(), ptr::null_mut(), ptr::null_mut()),
                0
            );
        }
    }

    #[test]
    fn test_save_restore_tagbuf() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            write_tag(0, 13, "test_prefix$");
            {
                let _saved = PrintTagGuard::save();
                write_tag(0, 9, "modified");
                assert_eq!(tagbuf_string(), "modified");
            }
            assert_eq!(tagbuf_string(), "test_prefix$");
        });
    }

    #[test]
    fn tagbuf_unwind_restores_destroyed_original_only() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let mut original = RSession::new_for_gc_tests();
        let (guard, buffer) = original.with_active(|| {
            write_tag(0, 7, "prefix");
            let guard = PrintTagGuard::save();
            write_tag(0, 9, "modified");
            (guard, with_print_runtime(|state| state.tagbuf.clone()))
        });
        original.close();
        drop(original);

        let other = RSession::new_for_gc_tests();
        other.with_active(|| {
            write_tag(0, 6, "other");
            let result = catch_unwind(AssertUnwindSafe(move || {
                let _saved = guard;
                panic!("forced print unwind");
            }));
            assert!(result.is_err());
            assert_eq!(&buffer.borrow()[..7], b"prefix\0");
            assert_eq!(tagbuf_string(), "other");
        });
    }

    #[test]
    fn test_do_prmatrix_admitted_matrix() {
        let mut session = RSession::new();
        let (arguments, _, _) = session.eval_code_with_output_capture(
            "as.pairlist(list(matrix(1:4, 2), NULL, NULL, FALSE, TRUE, NULL))",
        );
        let arguments = arguments.unwrap().into_owned().unwrap();
        session.with_active(|| unsafe {
            let matrix = CAR(arguments.as_raw());
            let result = do_prmatrix(
                ptr::null_mut(),
                ptr::null_mut(),
                arguments.as_raw(),
                R_GlobalEnv(),
            );
            assert_eq!(result, matrix);
            assert_eq!(INTEGER_ELT(result, 3), 4);
        });
    }

    #[test]
    fn test_unclass_public_call_preserves_original_class_and_names() {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_code_with_output_capture(
            "local({ x <- structure(1:3, class='foo', names=c('a','b','c')); identical(unclass(x), setNames(1:3,c('a','b','c'))) && identical(class(x),'foo') })",
        );
        assert_eq!(result.unwrap().logical_elt(0), Some(crate::sexp::TRUE));
    }
}
