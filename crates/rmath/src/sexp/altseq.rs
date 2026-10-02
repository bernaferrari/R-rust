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
//! [`Sexp::try_integer_elt`] / [`Sexp::try_real_elt`](crate::sexp::object::Sexp::try_real_elt)
//! and the vector and matrix printers share that formula, copied out through
//! the safe handle, so arithmetic and formatting stay lazy. `DATAPTR`,
//! `INTEGER`, `REAL`, and any pointer or element write allocate a normal arena
//! buffer, fill it, clear the ALT bit, and drop the formula cell. After that
//! the object is a plain vector.

use std::cell::Cell;
use std::ffi::CStr;
use std::os::raw::{c_double, c_int};

use super::accessors::{
    ALTREP, ATTRIB, CDR, CHAR, PRINTNAME, SET_ALTREP, SETCDR, TAG, TYPEOF,
};
use super::ffi::{NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use super::memory::{self, with_arena};
use super::object::{LeadingScalars, NodeBody, Sexp, copy_leading_scalars};

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
    /// Address of the sequence currently being expanded. Zero means idle.
    static MATERIALIZE_ADDR: Cell<usize> = const { Cell::new(0) };
}

struct RestoreMaterializing(usize);
impl Drop for RestoreMaterializing {
    fn drop(&mut self) {
        MATERIALIZE_ADDR.with(|open| open.set(self.0));
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

/// Owned copy of a compact sequence. Field arithmetic does not borrow the node.
#[derive(Clone, Copy)]
pub(crate) struct CompactSeq {
    formula: Formula,
    len: R_xlen_t,
    payload_null: bool,
}

impl CompactSeq {
    #[inline]
    pub(crate) fn payload_is_null(self) -> bool {
        self.payload_null
    }

    #[inline]
    pub(crate) fn is_int(self) -> bool {
        matches!(self.formula, Formula::Int { .. })
    }

    #[inline]
    pub(crate) fn is_real(self) -> bool {
        matches!(self.formula, Formula::Real { .. })
    }

    #[inline]
    pub(crate) fn len(self) -> R_xlen_t {
        self.len
    }

    /// Origin and step of an integer formula. `None` for a real sequence.
    #[inline]
    pub(crate) fn int_origin_step(self) -> Option<(c_int, c_int)> {
        match self.formula {
            Formula::Int { from, step } => Some((from, step)),
            Formula::Real { .. } => None,
        }
    }

    /// Origin and step of a real formula. `None` for an integer sequence.
    #[inline]
    pub(crate) fn real_origin_step(self) -> Option<(c_double, c_double)> {
        match self.formula {
            Formula::Real { from, step } => Some((from, step)),
            Formula::Int { .. } => None,
        }
    }

    pub(crate) fn int_or_na(self, i: R_xlen_t) -> c_int {
        match self.read_int(i) {
            LazyRead::Ready(value) => value,
            LazyRead::OutOfRange | LazyRead::Absent => NA_INTEGER,
        }
    }

    pub(crate) fn real_or_na(self, i: R_xlen_t) -> c_double {
        match self.read_real(i) {
            LazyRead::Ready(value) => value,
            LazyRead::OutOfRange | LazyRead::Absent => NA_REAL,
        }
    }

    fn read_int(self, i: R_xlen_t) -> LazyRead<c_int> {
        let Formula::Int { from, step } = self.formula else {
            return LazyRead::Absent;
        };
        if i < 0 || i >= self.len {
            return LazyRead::OutOfRange;
        }
        LazyRead::Ready(Formula::Int { from, step }.int_at(i))
    }

    fn read_real(self, i: R_xlen_t) -> LazyRead<c_double> {
        let Formula::Real { from, step } = self.formula else {
            return LazyRead::Absent;
        };
        if i < 0 || i >= self.len {
            return LazyRead::OutOfRange;
        }
        LazyRead::Ready(Formula::Real { from, step }.real_at(i))
    }
}

impl Sexp<'_> {
    /// Copy the formula when this node is still a compact int or real sequence.
    ///
    /// The walk copies headers and the two scalars. It does not allocate, protect,
    /// or expand the payload.
    pub(crate) fn compact_seq(&self) -> Option<CompactSeq> {
        let header = self.header();
        if !header.sxpinfo.alt() {
            return None;
        }
        let kind = header.sxpinfo.type_of();
        if kind != SEXPTYPE::INTSXP && kind != SEXPTYPE::REALSXP {
            return None;
        }
        let NodeBody::Vector(vec) = header.body else {
            return None;
        };
        let cell = self.copied_header(header.attrib)?;
        if cell.sxpinfo.type_of() != SEXPTYPE::LISTSXP {
            return None;
        }
        let NodeBody::List(list) = cell.body else {
            return None;
        };
        let tag = self.copied_header(list.tagval)?;
        if tag.sxpinfo.type_of() != SEXPTYPE::SYMSXP {
            return None;
        }
        let NodeBody::Symbol(sym) = tag.body else {
            return None;
        };
        let pname = self.copied_header(sym.pname)?;
        if !pname.char_eq(ALTSEQ_TAG_NAME.to_bytes()) {
            return None;
        }
        let info = self.copied_header(list.carval)?;
        if info.sxpinfo.type_of() != kind {
            return None;
        }
        let formula = match copy_leading_scalars(info) {
            Some(LeadingScalars::Int(from, step)) if kind == SEXPTYPE::INTSXP => {
                Formula::Int { from, step }
            }
            Some(LeadingScalars::Real(from, step)) if kind == SEXPTYPE::REALSXP => {
                Formula::Real { from, step }
            }
            _ => return None,
        };
        Some(CompactSeq {
            formula,
            len: vec.length,
            payload_null: header.payload.is_null(),
        })
    }

    /// Formula element. `require_unexpanded` matches the C accessors, which
    /// ignore a formula once a buffer pointer is present and read that buffer.
    pub(crate) fn read_compact_int(&self, i: R_xlen_t, require_unexpanded: bool) -> LazyRead<c_int> {
        let Some(seq) = self.compact_seq() else {
            return LazyRead::Absent;
        };
        if require_unexpanded && !seq.payload_null {
            return LazyRead::Absent;
        }
        seq.read_int(i)
    }

    /// Formula element. See [`Self::read_compact_int`].
    pub(crate) fn read_compact_real(
        &self,
        i: R_xlen_t,
        require_unexpanded: bool,
    ) -> LazyRead<c_double> {
        let Some(seq) = self.compact_seq() else {
            return LazyRead::Absent;
        };
        if require_unexpanded && !seq.payload_null {
            return LazyRead::Absent;
        }
        seq.read_real(i)
    }
}

/// Compact integer sequence whose element buffer is still null.
///
/// A null `x` is [`None`]. Wrapping a live node copies its header and does not allocate.
pub(crate) fn unexpanded_int(x: SEXP) -> Option<CompactSeq> {
    unexpanded(x).filter(|seq| seq.is_int())
}

/// Compact real sequence whose element buffer is still null.
///
/// See [`unexpanded_int`].
pub(crate) fn unexpanded_real(x: SEXP) -> Option<CompactSeq> {
    unexpanded(x).filter(|seq| seq.is_real())
}

fn unexpanded(x: SEXP) -> Option<CompactSeq> {
    let sx = unsafe { Sexp::from_raw(x) }?;
    sx.compact_seq().filter(|seq| seq.payload_is_null())
}

/// Expand a compact sequence into a normal vector buffer.
///
/// A buffer allocated while an arena lend is active is queued and registered
/// when that lend ends, before deferred collection. See
/// [`memory::attach_zeroed_data_buffer`].
///
/// # Safety
/// `x` is a live node owned by the active instance, with no overlapping Rust
/// payload borrow. The caller keeps it rooted while using its returned storage.
pub(crate) unsafe fn materialize(x: SEXP) {
    unsafe {
        if x.is_null() || ALTREP(x) == 0 {
            return;
        }
        let key = x as usize;
        if MATERIALIZE_ADDR.with(|open| open.get()) == key {
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
        let Ok(n) = usize::try_from((*x).vecsxp_length()) else {
            return;
        };
        let prev = MATERIALIZE_ADDR.with(|open| open.replace(key));
        let _restore = RestoreMaterializing(prev);
        let _root = super::protect::protect(x);
        if (*x).gengc_next_node.is_null() && n > 0 {
            let elem = memory::sexp_elem_size((*x).sxpinfo.type_of());
            if let Some(bytes) = n.checked_mul(elem)
                && bytes > 0
                && !memory::attach_zeroed_data_buffer(x, bytes).is_null()
            {
                fill((*x).gengc_next_node as *mut u8, &formula, n);
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

unsafe fn read_formula(x: SEXP) -> Option<Formula> {
    unsafe { Sexp::from_raw(x) }.and_then(|sx| sx.compact_seq().map(|seq| seq.formula))
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

#[cfg(test)]
mod tests;
