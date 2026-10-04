//! Canonical built-in integer and real ALTREP sequences.
//!
//! Safe SequenceClass construction owns a `[length, origin, step]` state vector.
//! Private vector metadata retains that state independently of public attributes.
//! Passive reads use the sealed built-in marker and validate the whole formula;
//! materialized buffers take precedence after a numerical write.

use super::ffi::{NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use super::object::{NodeBody, Sexp};
#[cfg(test)]
use std::ffi::CStr;
use std::os::raw::{c_double, c_int};
#[cfg(test)]
pub(crate) const ALTSEQ_TAG_NAME: &CStr = c".InternalAltSeq";

/// Result of reading one element without allocating the payload.
pub(crate) enum LazyRead<T> {
    Ready(T),
    /// Compact sequence, but `i` is outside `0..length`.
    OutOfRange,
    /// Not a compact sequence of this type.
    Absent,
}

#[derive(Clone, Copy, PartialEq)]
enum Formula {
    Int { from: c_int, step: c_int },
    Real { from: c_double, step: c_double },
}

impl Formula {
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

/// Checked callers retain this compatibility root until they install their
/// session-bound lease. An owning heap lease bridges all deferred callbacks;
/// the compatibility guard is captured only after the arena lend ends.
/// # Safety
/// Same owner and payload-loan requirements as `compact_int_seq`; the caller
/// must retain that original owner until the returned guard has been dropped.
pub(crate) unsafe fn compact_int_seq_protected(
    from: c_int,
    step: c_int,
    n: usize,
) -> (SEXP, super::protect::ProtectGuard<'static>) {
    unsafe {
        compact_seq_with_result(SEXPTYPE::INTSXP, n, Formula::Int { from, step }, |raw| {
            (raw, super::protect::protect(raw))
        })
    }
}

/// # Safety
/// Same owner and payload-loan requirements as `compact_real_seq`; the caller
/// must retain that original owner until the returned guard has been dropped.
pub(crate) unsafe fn compact_real_seq_protected(
    from: c_double,
    step: c_double,
    n: usize,
) -> (SEXP, super::protect::ProtectGuard<'static>) {
    unsafe {
        compact_seq_with_result(SEXPTYPE::REALSXP, n, Formula::Real { from, step }, |raw| {
            (raw, super::protect::protect(raw))
        })
    }
}

unsafe fn compact_seq(kind: SEXPTYPE, n: usize, formula: Formula) -> SEXP {
    unsafe { compact_seq_with_result(kind, n, formula, |raw| raw) }
}

unsafe fn compact_seq_with_result<T>(
    kind: SEXPTYPE,
    n: usize,
    formula: Formula,
    finish: impl FnOnce(SEXP) -> T,
) -> T {
    // The native boundary captures original authority once. The safe producer
    // owns initialized state and metadata through every allocating callback.
    let owner = unsafe { super::owner::OwnerToken::current() }
        .unwrap_or_else(|error| super::context::r_error(error.to_string()));
    let capability = super::owner::StoredOwner::from_token(owner);
    let result = capability.with_projection(|pointer| {
        let owner = unsafe { super::owner::OwnerToken::from_raw(pointer) };
        let (origin, step) = match formula {
            Formula::Int { from, step } => (from as f64, step as f64),
            Formula::Real { from, step } => (from, step),
        };
        let value = super::altrep::new_sequence(
            owner,
            kind,
            origin,
            step,
            i64::try_from(n).map_err(|_| super::object::SexpError::Altrep {
                reason: "sequence length",
            })?,
        )?;
        capability.require_active()?;
        Ok(finish(value.as_raw()))
    });
    result.unwrap_or_else(|error| super::context::r_error(error.to_string()))
}

/// Owned copy of a compact sequence. Field arithmetic does not borrow the node.
#[derive(Clone, Copy, PartialEq)]
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
        // This sealed marker is installed only by the actual built-in producer;
        // no public attribute or class name grants this passive formula access.
        let super::ffi::VectorMetadata::BuiltinSequence(link) = vec.metadata else {
            return None;
        };
        let metadata = self.checked_child(link).ok()?;
        if metadata.typeof_() != SEXPTYPE::VECSXP
            || metadata.len() != 5
            || metadata.header().sxpinfo.alt()
        {
            return None;
        }
        let descriptor = metadata.try_vector_elt(0).ok()?;
        let class_node = descriptor.allocation().ok()?;
        if !class_node
            .heap_identity()
            .has_builtin_sequence_permit(class_node, kind)
        {
            return None;
        }
        let cache = metadata.try_vector_elt(3).ok()?;
        if !cache.is_nil()
            && (cache.typeof_() != kind
                || cache.len() != vec.length
                || cache.header().sxpinfo.alt()
                || cache.header().payload.is_empty()
                || cache.header().payload != header.payload)
        {
            return None;
        }
        let state = metadata.try_vector_elt(1).ok()?;
        if state.typeof_() != SEXPTYPE::REALSXP || state.len() != 3 || state.header().sxpinfo.alt()
        {
            return None;
        }
        let length = state.try_real_elt(0).ok()?;
        let from = state.try_real_elt(1).ok()?;
        let step = state.try_real_elt(2).ok()?;
        if !length.is_finite()
            || length < 0.0
            || length > (1u64 << 52) as f64
            || length.fract() != 0.0
            || length as i64 != vec.length
            || !from.is_finite()
            || !step.is_finite()
        {
            return None;
        }
        let formula = if kind == SEXPTYPE::INTSXP {
            let last = from + (length - 1.0).max(0.0) * step;
            if [from, step, last].iter().any(|v| {
                !v.is_finite() || v.fract() != 0.0 || *v < i32::MIN as f64 || *v > i32::MAX as f64
            }) {
                return None;
            }
            Formula::Int {
                from: from as i32,
                step: step as i32,
            }
        } else {
            Formula::Real { from, step }
        };
        Some(CompactSeq {
            formula,
            len: vec.length,
            payload_null: header.payload.is_empty(),
        })
    }

    /// Formula element. `require_unexpanded` matches the C accessors, which
    /// ignore a formula once a buffer pointer is present and read that buffer.
    pub(crate) fn read_compact_int(
        &self,
        i: R_xlen_t,
        _require_unexpanded: bool,
    ) -> LazyRead<c_int> {
        let Some(seq) = self.compact_seq() else {
            return LazyRead::Absent;
        };
        if !seq.payload_null {
            return LazyRead::Absent;
        }
        seq.read_int(i)
    }

    /// Formula element. See [`Self::read_compact_int`].
    pub(crate) fn read_compact_real(
        &self,
        i: R_xlen_t,
        _require_unexpanded: bool,
    ) -> LazyRead<c_double> {
        let Some(seq) = self.compact_seq() else {
            return LazyRead::Absent;
        };
        if !seq.payload_null {
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

/// Compatibility bulk readers use the same checked canonical class path.
/// # Safety
/// The original owner and node are live, with no overlapping payload loan.
pub(crate) unsafe fn materialize(x: SEXP) {
    match unsafe { super::altrep::materialize_raw(x) } {
        Ok(_) | Err(super::object::SexpError::AllocationFailed { .. }) => {}
        Err(error) => super::context::r_error(error.to_string()),
    }
}

#[cfg(test)]
mod tests;
