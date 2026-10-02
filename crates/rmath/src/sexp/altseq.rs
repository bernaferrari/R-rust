//! Compact integer and real sequences for the default build.
//!
//! A compact sequence is an ordinary `INTSXP` or `REALSXP` node. Its logical
//! length lives in the vector header. The element buffer stays null until
//! something asks for `DATAPTR`. The formula (origin and step) is the CAR of
//! an attribute pairlist tagged [`.InternalAltSeq`](ALTSEQ_TAG_NAME). That
//! cell is a real `LISTSXP`, so the collector follows it through the attribute
//! edge it already traces. Nothing in the node is a Rust pointer.
//!
//! Element reads compute `origin + i * step` while the buffer is absent.
//! `INTEGER_ELT`, `REAL_ELT`, and `Sexp::try_integer_elt` / `try_real_elt`
//! share that formula, so arithmetic and formatting stay lazy. `DATAPTR`,
//! `INTEGER`, `REAL`, and any safe pointer or element write allocate a normal
//! arena buffer, fill it, clear the ALT bit, and drop the formula cell. After
//! that the object is a plain vector.

use std::cell::Cell;
use std::ffi::CStr;
use std::os::raw::{c_double, c_int};

use super::accessors::{
    ALTREP, ATTRIB, CAR, CDR, CHAR, PRINTNAME, SET_ALTREP, SETCDR, TAG, TYPEOF,
};
use super::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use super::memory::{self, with_arena};

pub(crate) const ALTSEQ_TAG_NAME: &CStr = c".InternalAltSeq";

/// Result of reading one element without allocating the payload.
pub(crate) enum LazyRead<T> {
    Ready(T),
    /// Compact sequence, but `i` is outside `0..length`.
    OutOfRange,
    /// Not a compact sequence of this type.
    Absent,
}

thread_local! {
    static MATERIALIZING: Cell<usize> = const { Cell::new(0) };
}

struct RestoreMaterializing(usize);
impl Drop for RestoreMaterializing {
    fn drop(&mut self) {
        MATERIALIZING.with(|open| open.set(self.0));
    }
}

#[derive(Clone, Copy)]
enum Formula {
    Int { from: c_int, step: c_int },
    Real { from: c_double, step: c_double },
}

impl Formula {
    fn info_type(self) -> SEXPTYPE {
        match self {
            Formula::Int { .. } => SEXPTYPE::INTSXP,
            Formula::Real { .. } => SEXPTYPE::REALSXP,
        }
    }

    /// Write origin and step into an aligned length-2 buffer of `info_type`.
    unsafe fn write_info(self, ptr: *mut u8) {
        unsafe {
            match self {
                Formula::Int { from, step } => {
                    let p = ptr.cast::<c_int>();
                    *p = from;
                    *p.add(1) = step;
                }
                Formula::Real { from, step } => {
                    let p = ptr.cast::<c_double>();
                    *p = from;
                    *p.add(1) = step;
                }
            }
        }
    }

    fn int_at(self, i: i64) -> c_int {
        match self {
            Formula::Int { from, step } => {
                (from as i64).wrapping_add(i.wrapping_mul(step as i64)) as c_int
            }
            Formula::Real { .. } => unreachable!("integer element of a real sequence"),
        }
    }

    fn real_at(self, i: i64) -> c_double {
        match self {
            Formula::Real { from, step } => from + (i as c_double) * step,
            Formula::Int { .. } => unreachable!("real element of an integer sequence"),
        }
    }
}

/// `tag` is the symbol whose print name is [ALTSEQ_TAG_NAME].
pub(crate) unsafe fn is_formula_tag(tag: SEXP) -> bool {
    unsafe {
        if tag.is_null() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return false;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return false;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            return false;
        }
        CStr::from_ptr(chars).to_bytes() == ALTSEQ_TAG_NAME.to_bytes()
    }
}

/// Lazy integer sequence `from, from+step, ...` of length `n`.
///
/// `n == 0` is a plain empty integer vector. Callers that want a length-1
/// scalar keep using an ordinary allocation.
pub(crate) unsafe fn compact_int_seq(from: c_int, step: c_int, n: usize) -> SEXP {
    unsafe { compact_seq(SEXPTYPE::INTSXP, n, Formula::Int { from, step }) }
}

/// Lazy real sequence `from, from+step, ...` of length `n`.
pub(crate) unsafe fn compact_real_seq(from: c_double, step: c_double, n: usize) -> SEXP {
    unsafe { compact_seq(SEXPTYPE::REALSXP, n, Formula::Real { from, step }) }
}

unsafe fn compact_seq(kind: SEXPTYPE, n: usize, formula: Formula) -> SEXP {
    unsafe {
        if n == 0 {
            return with_arena(|arena| arena.alloc_vector(kind, 0));
        }
        let Ok(len) = R_xlen_t::try_from(n) else {
            return std::ptr::null_mut();
        };
        let tag = super::symbol::Rf_install(ALTSEQ_TAG_NAME.as_ptr());
        if tag.is_null() {
            return std::ptr::null_mut();
        }
        with_arena(|arena| unsafe {
            let info = arena.alloc_vector(formula.info_type(), 2);
            if info.is_null() || (*info).gengc_next_node.is_null() {
                return std::ptr::null_mut();
            }
            formula.write_info((*info).gengc_next_node as *mut u8);
            let header = arena.alloc_vector(kind, 0);
            if header.is_null() {
                return std::ptr::null_mut();
            }
            (*header).set_vecsxp_length(len);
            (*header).set_vecsxp_truelength(0);
            SET_ALTREP(header, 1);
            let cell = arena.cons(info, super::globals::R_NilValue(), tag);
            if cell.is_null() {
                return std::ptr::null_mut();
            }
            // Header and cell are both young, allocated in this lend. The
            // write barrier would be a no-op, and it must not run while the
            // arena borrow is live.
            (*header).attrib = cell;
            header
        })
    }
}

pub(crate) unsafe fn lazy_int_elt(x: SEXP, i: R_xlen_t) -> LazyRead<c_int> {
    unsafe {
        let Formula::Int { from, step } = (match read_formula(x) {
            Some(formula) => formula,
            None => return LazyRead::Absent,
        }) else {
            return LazyRead::Absent;
        };
        if !index_in_range(x, i) {
            return LazyRead::OutOfRange;
        }
        LazyRead::Ready(Formula::Int { from, step }.int_at(i))
    }
}

pub(crate) unsafe fn lazy_real_elt(x: SEXP, i: R_xlen_t) -> LazyRead<c_double> {
    unsafe {
        let Formula::Real { from, step } = (match read_formula(x) {
            Some(formula) => formula,
            None => return LazyRead::Absent,
        }) else {
            return LazyRead::Absent;
        };
        if !index_in_range(x, i) {
            return LazyRead::OutOfRange;
        }
        LazyRead::Ready(Formula::Real { from, step }.real_at(i))
    }
}

/// Expand a compact sequence into a normal vector buffer.
///
/// A buffer allocated while an arena lend is active is queued and registered
/// when that lend ends, before deferred collection. See
/// [`memory::attach_zeroed_data_buffer`].
pub(crate) unsafe fn materialize(x: SEXP) {
    unsafe {
        if x.is_null() || ALTREP(x) == 0 {
            return;
        }
        let key = x as usize;
        if MATERIALIZING.with(|open| open.get()) == key {
            return;
        }
        if !(*x).gengc_next_node.is_null() {
            // A lend may have filled the buffer before the arena accepted it.
            // Finishing now would drop the formula, and a later budget refusal
            // would then have nothing left to rebuild the values from.
            if !memory::vector_payload_is_pending(x) {
                finish(x);
            }
            return;
        }
        let Some(formula) = read_formula(x) else {
            return;
        };
        let n = (*x).vecsxp_length();
        if n < 0 {
            return;
        }
        let prev = MATERIALIZING.with(|open| open.replace(key));
        let _restore = RestoreMaterializing(prev);
        let _root = super::protect::protect(x);
        if (*x).gengc_next_node.is_null() && n > 0 {
            let elem = memory::sexp_elem_size((*x).sxpinfo.type_of());
            if let Some(bytes) = (n as usize).checked_mul(elem)
                && bytes > 0
                && !memory::attach_zeroed_data_buffer(x, bytes).is_null()
            {
                fill((*x).gengc_next_node as *mut u8, &formula, n as usize);
            }
        }
        let committed = !(*x).gengc_next_node.is_null() && !memory::vector_payload_is_pending(x);
        if committed || n == 0 {
            finish(x);
        }
    }
}

/// The arena accepted `x`'s element buffer. Drop the formula.
pub(crate) unsafe fn commit_expanded_buffer(x: SEXP) {
    unsafe {
        if x.is_null() || (*x).gengc_next_node.is_null() || ALTREP(x) == 0 {
            return;
        }
        finish(x);
    }
}

/// Keep a still-lazy sequence's formula at the head of `v`.
///
/// Used when expansion did not commit. Replacing the attribute list outright
/// would discard the only copy of the values.
pub(crate) unsafe fn keep_formula_replace_tail(x: SEXP, v: SEXP) {
    unsafe {
        let cell = ATTRIB(x);
        let rest = without_formula_cells(v);
        if is_list(cell) && is_formula_tag(TAG(cell)) {
            SETCDR(cell, rest);
            return;
        }
        super::gengc::attrib_write_barrier(x, rest);
        (*x).attrib = rest;
    }
}

/// Drop every `.InternalAltSeq` cell from an attribute list.
///
/// Leading matches are unlinked by returning the tail. An interior match is
/// unlinked in place. Lists with no such tag are returned unchanged, including
/// a null pointer.
pub(crate) unsafe fn without_formula_cells(list: SEXP) -> SEXP {
    unsafe {
        let nil = super::globals::R_NilValue();
        if list.is_null() || list == nil || TYPEOF(list) != SEXPTYPE::LISTSXP {
            return list;
        }
        let mut head = list;
        while is_list(head) && is_formula_tag(TAG(head)) {
            head = CDR(head);
            if head.is_null() || head == nil || TYPEOF(head) != SEXPTYPE::LISTSXP {
                return if head.is_null() { nil } else { head };
            }
        }
        let mut prev = head;
        let mut cur = CDR(head);
        while is_list(cur) {
            if is_formula_tag(TAG(cur)) {
                let next = CDR(cur);
                super::accessors::SETCDR(prev, next);
                cur = next;
            } else {
                prev = cur;
                cur = CDR(cur);
            }
        }
        head
    }
}

fn is_list(cell: SEXP) -> bool {
    unsafe { !cell.is_null() && TYPEOF(cell) == SEXPTYPE::LISTSXP }
}

unsafe fn index_in_range(x: SEXP, i: R_xlen_t) -> bool {
    unsafe { i >= 0 && i < (*x).vecsxp_length() }
}

unsafe fn read_formula(x: SEXP) -> Option<Formula> {
    unsafe {
        if x.is_null() || ALTREP(x) == 0 {
            return None;
        }
        let kind = (*x).sxpinfo.type_of();
        if kind != SEXPTYPE::INTSXP && kind != SEXPTYPE::REALSXP {
            return None;
        }
        let cell = ATTRIB(x);
        if !is_list(cell) || !is_formula_tag(TAG(cell)) {
            return None;
        }
        let info = CAR(cell);
        if info.is_null() || (*info).gengc_next_node.is_null() {
            return None;
        }
        match kind {
            SEXPTYPE::INTSXP if TYPEOF(info) == SEXPTYPE::INTSXP && (*info).vecsxp_length() >= 2 => {
                let p = (*info).gengc_next_node as *const c_int;
                if (p as usize) % std::mem::align_of::<c_int>() != 0 {
                    return None;
                }
                Some(Formula::Int {
                    from: *p,
                    step: *p.add(1),
                })
            }
            SEXPTYPE::REALSXP
                if TYPEOF(info) == SEXPTYPE::REALSXP && (*info).vecsxp_length() >= 2 =>
            {
                let p = (*info).gengc_next_node as *const c_double;
                if (p as usize) % std::mem::align_of::<c_double>() != 0 {
                    return None;
                }
                Some(Formula::Real {
                    from: *p,
                    step: *p.add(1),
                })
            }
            _ => None,
        }
    }
}

unsafe fn fill(ptr: *mut u8, formula: &Formula, n: usize) {
    unsafe {
        match *formula {
            Formula::Int { .. } => {
                let dest = ptr.cast::<c_int>();
                for i in 0..n {
                    *dest.add(i) = formula.int_at(i as i64);
                }
            }
            Formula::Real { .. } => {
                let dest = ptr.cast::<c_double>();
                for i in 0..n {
                    *dest.add(i) = formula.real_at(i as i64);
                }
            }
        }
    }
}

/// Clear the ALT bit and unlink the formula. The bit is cleared first so a
/// nested `SET_ATTRIB` cannot call back into [`materialize`].
unsafe fn finish(x: SEXP) {
    unsafe {
        SET_ALTREP(x, 0);
        let n = (*x).vecsxp_length();
        if n >= 0 {
            (*x).set_vecsxp_truelength(n);
        }
        let cell = ATTRIB(x);
        if is_list(cell) && is_formula_tag(TAG(cell)) {
            let rest = CDR(cell);
            super::gengc::attrib_write_barrier(x, rest);
            (*x).attrib = rest;
        }
    }
}
