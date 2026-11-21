#![allow(unused_variables)]
#![allow(unused_assignments)]
//! Object duplication system for R objects.
//!
//! Ported from R's src/main/duplicate.c. Provides deep and shallow duplication
//! of R objects, vector/matrix copy with recycling, and cycle detection for
//! complex assignment operations.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::{c_char, c_double, c_int};
#[cfg(test)]
use std::ptr;
use std::{cell::RefCell, hash::RandomState};

use allocator_api2::vec::Vec as AllocVec;
use hashbrown::HashSet;

mod workspace;
use workspace::TraversalAllocator;

use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_allocVector3;
#[cfg(test)]
use crate::sexp::constructors::Rf_cons;
use crate::sexp::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::{Sexp, SexpError, SexpResult};

fn duplication_failure(error: SexpError) -> ! {
    crate::sexp::context::r_error(error.to_string())
}

// ---------------------------------------------------------------------------
// GP bit constants
// ---------------------------------------------------------------------------

/// DDVAL bit mask (gp bit 10).
const DDVAL_MASK: u16 = 1 << 10;

/// S4 object bit (gp bit 11).
const S4_OBJECT_MASK: u16 = 1 << 4;

/// JIT-related gp bits (bit 0 = NOJIT, bit 1 = MAYBEJIT).
const NOJIT_MASK: u16 = 1 << 0;
const MAYBEJIT_MASK: u16 = 1 << 1;

/// RTRACE bit in sxpinfo (bit 26 in type_and_flags).
const RTRACE_MASK: u32 = 1 << 26;

/// GROWABLE_BIT in gp (bit 5).
const GROWABLE_BIT_MASK: u16 = 1 << 5;

// ---------------------------------------------------------------------------
// Local helpers and entry points
// ---------------------------------------------------------------------------

fn DispatchGroup(
    _s: SEXP,
    _code: *const c_char,
    _call: SEXP,
    _op: *const c_char,
    _args: SEXP,
    _env: SEXP,
) -> c_int {
    0
}

/// Check if an object has no references (NAMED == 0).
#[inline]
fn NO_REFERENCES(x: SEXP) -> c_int {
    crate::mainutils::relop::NO_REFERENCES(x)
}

/// Check if an object is an S4 object.
#[inline]
fn IS_S4_OBJECT(x: SEXP) -> c_int {
    crate::sexp::accessors::checked_s4_object_flag(x)
}

/// Check the NOJIT gp bit.
#[cfg(test)]
#[inline]
unsafe fn NOJIT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & NOJIT_MASK) != 0) as c_int
    }
}

/// Check the MAYBEJIT gp bit.
#[cfg(test)]
#[inline]
unsafe fn MAYBEJIT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & MAYBEJIT_MASK) != 0) as c_int
    }
}

/// Set the NOJIT gp bit.
#[cfg(test)]
#[inline]
unsafe fn SET_NOJIT(x: SEXP) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp() | NOJIT_MASK;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Set the MAYBEJIT gp bit.
#[cfg(test)]
#[inline]
unsafe fn SET_MAYBEJIT(x: SEXP) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp() | MAYBEJIT_MASK;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Check the RTRACE bit.
#[cfg(test)]
#[inline]
unsafe fn RTRACE(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.type_and_flags & RTRACE_MASK) != 0) as c_int
    }
}

/// Set or clear the RTRACE bit (`sxpinfo.trace`, bit 26).
#[inline]
unsafe fn SET_RTRACE(x: SEXP, v: c_int) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_trace(v != 0);
        }
    }
}

/// Set NAMED to maximum (2).
#[inline]
fn ENSURE_NAMEDMAX(x: SEXP) {
    if NAMED(x) < 2 {
        SET_NAMED(x, 2);
    }
}

/// Raise a typed error for SEXPTYPEs this port cannot duplicate/copy yet.
fn UNIMPLEMENTED_TYPE(routine: &str, s: SEXP) -> ! {
    let sexptype = if s.is_null() { -1 } else { TYPEOF(s) };
    std::panic::panic_any(crate::sexp::context::RError {
        message: format!("{routine}: unsupported SEXPTYPE {sexptype}"),
    });
}

/// Checked read handle for a live vector in the active session.
fn source_vector(x: SEXP) -> Sexp<'static> {
    crate::mainutils::essentials::owner_sexp(crate::mainutils::essentials::active_owner(), x)
}

/// Checked write handle for a live vector in the active session.
fn target_vector(x: SEXP) -> crate::sexp::object::SexpMut<'static> {
    crate::mainutils::essentials::owner_sexp_mut(x)
}

fn checked<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|error| duplication_failure(error))
}

/// Visit `(destination, source)` index pairs of `nsrc` values recycled into
/// `n` cells, in GNU `xcopyWithRecycle` order.
fn recycle_pairs(n: R_xlen_t, nsrc: R_xlen_t, mut f: impl FnMut(R_xlen_t, R_xlen_t)) {
    let mut sidx: R_xlen_t = 0;
    for i in 0..n {
        if sidx == nsrc {
            sidx = 0;
        }
        f(i, sidx);
        sidx += 1;
    }
}

/// Check if a type is pairlist-like.
#[inline]
fn isPairList(x: SEXP) -> c_int {
    {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::DOTSXP) as c_int
    }
}

/// Check if a type is a vector list (VECSXP, EXPRSXP).
#[inline]
fn isVectorList(x: SEXP) -> c_int {
    {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP) as c_int
    }
}

/// Get nrows from dim attribute.
fn nrows(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    let dim = ATTRIB(x);
    if dim.is_null() || dim == R_NilValue() {
        return 0;
    }
    // dim should be an integer vector; first element is nrows
    if TYPEOF(dim) != SEXPTYPE::INTSXP {
        return 0;
    }
    let len = LENGTH(dim);
    if len < 2 {
        return if len == 1 { INTEGER_ELT(dim, 0) } else { 0 };
    }
    INTEGER_ELT(dim, 0)
}

/// Get ncols from dim attribute.
fn ncols(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    let dim = ATTRIB(x);
    if dim.is_null() || dim == R_NilValue() {
        return 0;
    }
    if TYPEOF(dim) != SEXPTYPE::INTSXP {
        return 0;
    }
    let len = LENGTH(dim);
    if len < 2 {
        return 0;
    }
    INTEGER_ELT(dim, 1)
}

// ---------------------------------------------------------------------------
// Internal macros / inline helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// FILL_MATRIX_ITERATE macro equivalent
// ---------------------------------------------------------------------------

/// Iterator for filling a matrix from a vector with re-use.
///
/// This is the Rust equivalent of R's `FILL_MATRIX_ITERATE` macro.
/// Calls `f(didx, sidx)` for each destination/source index pair.
///
/// Parameters:
/// - `dstart`: starting destination index
/// - `drows`: number of destination rows
/// - `srows`: number of source rows
/// - `cols`: number of columns
/// - `nsrc`: source length (for recycling)
/// - `f`: callback receiving (didx, sidx)
fn fill_matrix_iterate<F>(
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
    mut f: F,
) where
    F: FnMut(R_xlen_t, R_xlen_t),
{
    let mut i: R_xlen_t = 0;
    let mut sidx: R_xlen_t = 0;
    while i < srows {
        sidx = i;
        let mut j: R_xlen_t = 0;
        let mut didx: R_xlen_t = dstart + i;
        while j < cols {
            if sidx >= nsrc {
                sidx -= nsrc;
            }
            f(didx, sidx);
            j += 1;
            sidx += srows;
            if sidx >= nsrc {
                sidx -= nsrc;
            }
            didx += drows;
        }
        i += 1;
    }
}

/// Iterator for filling a matrix by-row.
///
/// This is the Rust equivalent of R's `FILL_MATRIX_BYROW_ITERATE` macro.
/// Calls `f(didx, sidx)` for each destination/source index pair.
fn fill_matrix_byrow_iterate<F>(
    dstart: R_xlen_t,
    drows: R_xlen_t,
    dcols: R_xlen_t,
    nsrc: R_xlen_t,
    mut f: F,
) where
    F: FnMut(R_xlen_t, R_xlen_t),
{
    let mut i: R_xlen_t = 0;
    let mut sidx: R_xlen_t = 0;
    while i < drows {
        let mut j: R_xlen_t = 0;
        let mut didx: R_xlen_t = dstart + i;
        while j < dcols {
            if sidx >= nsrc {
                sidx -= nsrc;
            }
            f(didx, sidx);
            j += 1;
            sidx += 1;
            if sidx >= nsrc {
                sidx -= nsrc;
            }
            didx += drows;
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// Core duplicate functions
// ---------------------------------------------------------------------------

/// Core duplication function.
///
/// `deep`: if nonzero, performs deep copy; if zero, performs shallow copy
/// (shared subtrees for pairlists/vectors, but new atomic vectors are copied).
///
/// Both native input and result stay owned through result admission.
fn duplicate1(s: SEXP, deep: c_int) -> SEXP {
    if s.is_null() {
        return s;
    }
    let source = unsafe { crate::sexp::altrep::rooted_raw(s) }
        .unwrap_or_else(|error| duplication_failure(error));
    if source.runtime_owner.is_none() {
        return if let Some(singleton) = crate::sexp::globals::immutable_singleton_lease(s) {
            if singleton.snapshot().sxpinfo.type_of() == SEXPTYPE::LGLSXP {
                duplicate_logical_singleton(singleton)
                    .unwrap_or_else(|error| duplication_failure(error))
                    .as_raw()
            } else {
                s
            }
        } else {
            s
        };
    }
    let pin = source
        .pin_runtime()
        .unwrap_or_else(|error| duplication_failure(error));
    let original = source
        .runtime_owner
        .as_ref()
        .expect("managed duplication owner");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::sexp::owner::with_runtime(original, |access| {
            duplicate_iterative(source.clone(), deep, access)
        })
        .unwrap_or_else(|error| duplication_failure(error))
        .unwrap_or_else(|error| duplication_failure(error))
    }));
    if let Some(pin) = &pin {
        pin.require_live()
            .unwrap_or_else(|error| duplication_failure(error));
    }
    match outcome {
        Ok(copy) => copy.as_raw(),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// A copy owns every immediate value selected before an allocating/provider
/// callback. Scalars are copied Rust data; pointer-valued entries are actual
/// independent leases. Repeated children are still duplicated per occurrence.
enum CopyValues<'source> {
    Integers(Vec<c_int>),
    Reals(Vec<c_double>),
    Complex(Vec<Rcomplex>),
    Bytes(Vec<Rbyte>),
    References(Vec<Sexp<'source>>),
    Object,
}

struct AttributeSnapshot<'source> {
    source: Sexp<'source>,
    pairlist: PairlistSnapshot<'source>,
}

fn snapshot_attributes<'source>(
    source: &Sexp<'source>,
    access: &crate::sexp::owner::RuntimeAccess,
) -> SexpResult<Option<AttributeSnapshot<'source>>> {
    let attributes = source.try_attrib()?;
    if attributes.is_nil() {
        return Ok(None);
    }
    let pairlist = snapshot_pairlist(&attributes, access)?;
    Ok(Some(AttributeSnapshot {
        source: attributes,
        pairlist,
    }))
}

fn collect_copy<T>(
    length: usize,
    mut element: impl FnMut(usize) -> SexpResult<T>,
) -> SexpResult<Vec<T>> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(length)
        .map_err(|_| SexpError::AllocationFailed {
            object: "duplication value snapshot",
        })?;
    for index in 0..length {
        result.push(element(index)?);
    }
    Ok(result)
}

fn prepare_value<'source>(
    source: &Sexp<'source>,
    flags: crate::sexp::ffi::SxpInfo,
    access: &crate::sexp::owner::RuntimeAccess,
) -> SexpResult<VectorContinuation<'source>> {
    prepare_value_attempt(source, flags, access, true)
}

fn prepare_value_attempt<'source>(
    source: &Sexp<'source>,
    flags: crate::sexp::ffi::SxpInfo,
    access: &crate::sexp::owner::RuntimeAccess,
    allow_gc_retry: bool,
) -> SexpResult<VectorContinuation<'source>> {
    let domain: crate::sexp::object::NodeDomain<'source> = source.node_factory()?.into();
    let node = source.allocation()?;
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
    let mut flags = flags;
    let kind = flags.type_of();
    let mut attributes = match snapshot_attributes(source, access) {
        Err(SexpError::AllocationFailed {
            object: "pairlist snapshot workspace",
        }) if allow_gc_retry => {
            // The attribute ticket can hit the limit before the vector
            // snapshot does. Refresh the source after collection either way.
            access.with_native(|owner| owner.full_gc().map(|_| ()))?;
            let refreshed = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
            if refreshed.sxpinfo.type_of() != kind {
                return Err(SexpError::StaleAllocation);
            }
            return prepare_value_attempt(source, refreshed.sxpinfo, access, false);
        }
        outcome => outcome?,
    };
    let (mut length, mut truelength) = match header.data {
        crate::sexp::ffi::NodeBody::Vector(vector) => (
            usize::try_from(vector.length)
                .map_err(|_| SexpError::MissingData { sexptype: kind })?,
            if flags.gp() & GROWABLE_BIT_MASK == 0 {
                vector.truelength
            } else {
                0
            },
        ),
        _ if kind == SEXPTYPE::OBJSXP => (0, 0),
        _ => {
            return Err(SexpError::TypeMismatch {
                expected: "vector storage",
                actual: kind,
            });
        }
    };
    let element_bytes = match kind {
        SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => std::mem::size_of::<c_int>(),
        SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
        SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::STRSXP => {
            std::mem::size_of::<Sexp<'source>>()
        }
        _ => 0,
    };
    let mut bytes = length
        .checked_mul(element_bytes)
        .ok_or(SexpError::AllocationFailed {
            object: "duplication snapshot size",
        })?;
    // This original physical-store admission is passive: it does not lend
    // an arena or flush pending callbacks before the source snapshot exists.
    let reservation = match heap.reserve_payload_bytes(node, bytes) {
        Some(reservation) => reservation,
        None => {
            if !allow_gc_retry {
                return Err(SexpError::AllocationFailed {
                    object: "duplication snapshot budget",
                });
            }
            // A portable lazy-load can reach the arena cap with promoted
            // garbage. Collect only after actual denial, while `source` and
            // the owned attribute snapshot keep the original graph rooted.
            access.with_native(|owner| owner.full_gc().map(|_| ()))?;
            access.require_active()?;

            // A finalizer may have mutated or detached the source during GC.
            // Never continue with stale shape or attribute snapshots.
            let refreshed = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
            if refreshed.sxpinfo.type_of() != kind {
                return Err(SexpError::StaleAllocation);
            }
            if refreshed.attrib != header.attrib
                || refreshed.payload != header.payload
                || refreshed.data != header.data
            {
                drop(attributes);
                return prepare_value_attempt(source, refreshed.sxpinfo, access, false);
            }
            header = refreshed;
            flags = header.sxpinfo;
            attributes = snapshot_attributes(source, access)?;
            (length, truelength) = match header.data {
                crate::sexp::ffi::NodeBody::Vector(vector) => (
                    usize::try_from(vector.length)
                        .map_err(|_| SexpError::MissingData { sexptype: kind })?,
                    if flags.gp() & GROWABLE_BIT_MASK == 0 {
                        vector.truelength
                    } else {
                        0
                    },
                ),
                _ if kind == SEXPTYPE::OBJSXP => (0, 0),
                _ => return Err(SexpError::StaleAllocation),
            };
            bytes = length
                .checked_mul(element_bytes)
                .ok_or(SexpError::AllocationFailed {
                    object: "duplication snapshot size",
                })?;
            heap.reserve_payload_bytes(node, bytes)
                .ok_or(SexpError::AllocationFailed {
                    object: "duplication snapshot budget",
                })?
        }
    };
    let payload = heap.payload_lease(node);
    let missing = || SexpError::MissingData { sexptype: kind };
    let values = match kind {
        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => {
            CopyValues::Integers(collect_copy(length, |index| {
                if let Some(payload) = &payload {
                    return payload.integer_elt(index).ok_or_else(missing);
                }
                if kind == SEXPTYPE::LGLSXP {
                    source.try_logical_elt(index as _)
                } else {
                    source.try_integer_elt(index as _)
                }
            })?)
        }
        SEXPTYPE::REALSXP => CopyValues::Reals(collect_copy(length, |index| {
            if let Some(payload) = &payload {
                return payload.real_elt(index).ok_or_else(missing);
            }
            source.try_real_elt(index as _)
        })?),
        SEXPTYPE::CPLXSXP => CopyValues::Complex(collect_copy(length, |index| {
            if let Some(payload) = &payload {
                return payload.complex_elt(index).ok_or_else(missing);
            }
            source.try_complex_elt(index as _)
        })?),
        SEXPTYPE::RAWSXP => CopyValues::Bytes(collect_copy(length, |index| {
            if let Some(payload) = &payload {
                return payload.byte_elt(index).ok_or_else(missing);
            }
            source.try_raw_elt(index as _)
        })?),
        SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::STRSXP => {
            CopyValues::References(collect_copy(length, |index| {
                let value = if let Some(payload) = &payload {
                    source.checked_child(payload.reference_elt(index).ok_or_else(missing)?)?
                } else if kind == SEXPTYPE::STRSXP {
                    source.try_string_elt(index as _)?
                } else {
                    source.try_vector_elt(index as _)?
                };
                if kind == SEXPTYPE::STRSXP && value.typeof_() != SEXPTYPE::CHARSXP {
                    return Err(SexpError::TypeMismatch {
                        expected: "character element",
                        actual: value.typeof_(),
                    });
                }
                Ok(value)
            })?)
        }
        SEXPTYPE::OBJSXP => CopyValues::Object,
        _ => {
            return Err(SexpError::TypeMismatch {
                expected: "duplicable vector",
                actual: kind,
            });
        }
    };
    access.require_active()?;
    let allocator = access.allocator(&domain)?;
    let copy = allocator.allocate(|arena| {
        let pointer = if kind == SEXPTYPE::OBJSXP {
            arena.alloc_node(kind)
        } else {
            arena
                .alloc_vector_checked(kind, length.try_into().ok()?)
                .ok()?
        };
        let allocation = arena.node_token(pointer)?;
        let copied_heap = allocation.heap_identity();
        if length != 0 {
            let destination = copied_heap.payload_lease(&allocation)?;
            match &values {
                CopyValues::Integers(values) => {
                    for (index, value) in values.iter().enumerate() {
                        destination.set_integer_elt(index, *value)?;
                    }
                }
                CopyValues::Reals(values) => {
                    for (index, value) in values.iter().enumerate() {
                        destination.set_real_elt(index, *value)?;
                    }
                }
                CopyValues::Complex(values) => {
                    for (index, value) in values.iter().enumerate() {
                        destination.set_complex_elt(index, *value)?;
                    }
                }
                CopyValues::Bytes(values) => {
                    for (index, value) in values.iter().enumerate() {
                        destination.set_byte_elt(index, *value)?;
                    }
                }
                // Child values are copied after the initialized parent is
                // published, matching GNU's occurrence/order semantics.
                CopyValues::References(values) if kind == SEXPTYPE::STRSXP => {
                    for (index, value) in values.iter().enumerate() {
                        destination.set_reference_elt(index, domain.link(value).ok()?)?;
                    }
                }
                CopyValues::References(_) | CopyValues::Object => {}
            }
        }
        Some(pointer)
    })?;
    Ok(VectorContinuation {
        copy,
        values,
        attributes,
        flags,
        truelength,
        next_value: 0,
        pending: VectorPending::None,
        _reservation: reservation,
    })
}

fn apply_vector_flags(
    copy: &Sexp<'_>,
    flags: crate::sexp::ffi::SxpInfo,
    truelength: R_xlen_t,
) -> SexpResult<()> {
    let allocation = copy.allocation()?;
    let heap = allocation.heap_identity();
    let mut header = heap
        .node_snapshot(allocation)
        .ok_or(SexpError::StaleAllocation)?;
    header.sxpinfo.set_obj(flags.obj());
    header
        .sxpinfo
        .set_gp((header.sxpinfo.gp() & !S4_OBJECT_MASK) | (flags.gp() & S4_OBJECT_MASK));
    if let crate::sexp::ffi::NodeBody::Vector(vector) = &mut header.data {
        vector.truelength = truelength;
    }
    heap.replace_node(allocation, header)
        .ok_or(SexpError::StaleAllocation)
}

/// GNU closure duplication shares formals, body, and environment; only
/// attributes follow the requested deep/shallow duplication policy.
fn prepare_closure<'source>(
    source: &Sexp<'source>,
    access: &crate::sexp::owner::RuntimeAccess,
) -> SexpResult<ClosureContinuation<'source>> {
    let domain: crate::sexp::object::NodeDomain<'source> = source.node_factory()?.into();
    let formals = source.try_formals()?;
    let body = source.try_body()?;
    let environment = source.try_cloenv()?;
    let attributes = snapshot_attributes(source, access)?;
    let source_node = source.allocation()?;
    let flags = source_node
        .heap_identity()
        .node_snapshot(source_node)
        .ok_or(SexpError::StaleAllocation)?
        .sxpinfo;
    let formals_link = domain.link(&formals)?;
    let body_link = domain.link(&body)?;
    let environment_link = domain.link(&environment)?;
    let copy = access.allocator(&domain)?.allocate(|arena| {
        let projection = arena.alloc_node(SEXPTYPE::CLOSXP);
        let allocation = arena.node_token(projection)?;
        let heap = arena.heap_identity();
        let mut header = heap.node_snapshot(&allocation)?;
        header.data.closure_mut().formals = formals_link;
        header.data.closure_mut().body = body_link;
        header.data.closure_mut().env = environment_link;
        header.sxpinfo.set_obj(flags.obj());
        header
            .sxpinfo
            .set_gp(flags.gp() & (NOJIT_MASK | MAYBEJIT_MASK | S4_OBJECT_MASK));
        heap.replace_node(&allocation, header)?;
        Some(projection)
    })?;
    Ok(ClosureContinuation {
        copy,
        attributes,
        pending: false,
    })
}

/// Deep duplicate an SEXP.
pub fn duplicate(s: SEXP) -> SEXP {
    duplicate_entry(s, 1)
}

/// Alias for duplicate (R API).
pub fn Rf_duplicate(s: SEXP) -> SEXP {
    duplicate(s)
}

/// Shallow duplicate an SEXP.
pub fn shallow_duplicate(s: SEXP) -> SEXP {
    duplicate_entry(s, 0)
}

/// GNU duplicates immutable logical constants into mutable ordinary vectors.
/// Only this native admission selects the active owner; the allocation scope
/// retains that exact runtime and the actual original singleton bank lease.
fn duplicate_logical_singleton(
    singleton: crate::sexp::globals::SingletonLease,
) -> SexpResult<Sexp<'static>> {
    let owner = crate::sexp::owner::OwnerToken::with_current(|owner| owner.weak_owner())?
        .ok_or(SexpError::RootUnavailable)?;
    crate::sexp::owner::with_runtime(&owner, |access| {
        let value = singleton
            .payload_lease()
            .and_then(|payload| payload.integer_elt(0))
            .ok_or(SexpError::MissingData {
                sexptype: SEXPTYPE::LGLSXP,
            })?;
        let domain = access.domain();
        let allocator = access.allocator(&domain)?;
        let copy = allocator.allocate(|arena| {
            let pointer = arena.alloc_vector_checked(SEXPTYPE::LGLSXP, 1).ok()?;
            let node = arena.node_token(pointer)?;
            let heap = node.heap_identity();
            heap.payload_lease(&node)?.set_integer_elt(0, value)?;
            let mut header = heap.node_snapshot(&node)?;
            header.data.vector_mut().truelength = 0;
            heap.replace_node(&node, header)?;
            Some(pointer)
        })?;
        access.require_active()?;
        Ok(copy)
    })?
}

fn duplicate_entry(s: SEXP, deep: c_int) -> SEXP {
    if s.is_null() {
        return s;
    }
    if let Some(singleton) = crate::sexp::globals::immutable_singleton_lease(s) {
        return if singleton.snapshot().sxpinfo.type_of() == SEXPTYPE::LGLSXP {
            duplicate_logical_singleton(singleton)
                .unwrap_or_else(|error| duplication_failure(error))
                .as_raw()
        } else {
            s
        };
    }
    // SAFETY: `s` arrives from the evaluator graph of the active session;
    // rooted_raw validates its ownership and no payload loan is held here.
    let source = unsafe { crate::sexp::altrep::rooted_raw(s) }
        .unwrap_or_else(|error| duplication_failure(error));
    let pin = source
        .pin_runtime()
        .unwrap_or_else(|error| duplication_failure(error));
    let node = source
        .allocation()
        .unwrap_or_else(|error| duplication_failure(error));
    let flags = node
        .heap_identity()
        .node_snapshot(node)
        .unwrap_or_else(|| duplication_failure(SexpError::StaleAllocation))
        .sxpinfo;
    let trace = flags.trace()
        && !matches!(
            flags.type_of(),
            SEXPTYPE::CLOSXP
                | SEXPTYPE::BUILTINSXP
                | SEXPTYPE::SPECIALSXP
                | SEXPTYPE::PROMSXP
                | SEXPTYPE::ENVSXP
        );
    let factory = source
        .node_factory()
        .unwrap_or_else(|error| duplication_failure(error));
    let original = source
        .runtime_owner
        .as_ref()
        .unwrap_or_else(|| duplication_failure(SexpError::RootUnavailable));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::sexp::owner::with_runtime(original, |access| {
            let copy = access.with_native(|_| factory.wrap(duplicate1(s, deep)))?;
            if trace {
                access.with_native(|_| {
                    crate::mainutils::debug::memtrace_report(
                        source.as_raw().cast(),
                        copy.as_raw().cast(),
                    );
                    Ok(())
                })?;
                // Post-report validation happens before any native result
                // mutation; a new ambient runtime cannot supply authority.
                access.require_active()?;
                // SAFETY: `copy` is the live, freshly duplicated node.
                unsafe { SET_RTRACE(copy.as_raw(), 1) };
            }
            access.require_active()?;
            Ok(copy)
        })
        .unwrap_or_else(|error| duplication_failure(error))
        .unwrap_or_else(|error| duplication_failure(error))
    }));
    if let Some(pin) = &pin {
        pin.require_live()
            .unwrap_or_else(|error| duplication_failure(error));
    }
    match outcome {
        Ok(copy) => copy.as_raw(),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Copy before attribute mutation when the object may be referenced.
///
/// GNU `do_attrgets` uses `MAYBE_SHARED` (`NAMED >= 2`). This port does
/// not yet RAISE_NAMED on formal binding, so a caller-owned value often
/// still has NAMED==1 inside `f(x)`. Use MAYBE_REFERENCED (`NAMED > 0`)
/// until argument matching increments NAMED.
pub fn shallow_duplicate_if_shared(s: SEXP) -> SEXP {
    if s.is_null() || s == R_NilValue() {
        return s;
    }
    if NAMED(s) > 0 {
        shallow_duplicate(s)
    } else {
        s
    }
}

/// Lazy duplicate: just set NAMEDMAX on the input.
/// Returns the input unchanged (no copy is made).
pub fn lazy_duplicate(s: SEXP) -> SEXP {
    if s.is_null() {
        return s;
    }
    match SEXPTYPE(TYPEOF(s)) {
        SEXPTYPE::NILSXP
        | SEXPTYPE::SYMSXP
        | SEXPTYPE::ENVSXP
        | SEXPTYPE::SPECIALSXP
        | SEXPTYPE::BUILTINSXP
        | SEXPTYPE::CHARSXP
        | SEXPTYPE::PROMSXP => {
            // Immutable types - nothing to do
        }
        SEXPTYPE::CLOSXP
        | SEXPTYPE::LISTSXP
        | SEXPTYPE::LANGSXP
        | SEXPTYPE::DOTSXP
        | SEXPTYPE::EXPRSXP
        | SEXPTYPE::VECSXP
        | SEXPTYPE::LGLSXP
        | SEXPTYPE::INTSXP
        | SEXPTYPE::REALSXP
        | SEXPTYPE::CPLXSXP
        | SEXPTYPE::RAWSXP
        | SEXPTYPE::STRSXP
        | SEXPTYPE::OBJSXP => {
            ENSURE_NAMEDMAX(s);
        }
        _ => {} // intentionally unhandled: SEXPTYPE does not require NAMEDMAX enforcement
    }
    s
}

// ---------------------------------------------------------------------------
// Cycle detection
// ---------------------------------------------------------------------------

/// Detect cycles that would be created by assigning `child` as a
/// component of `s` in a complex assignment.
pub fn R_cycle_detected(s: SEXP, child: SEXP) -> c_int {
    if s == child {
        match SEXPTYPE(TYPEOF(child)) {
            SEXPTYPE::NILSXP
            | SEXPTYPE::SYMSXP
            | SEXPTYPE::ENVSXP
            | SEXPTYPE::SPECIALSXP
            | SEXPTYPE::BUILTINSXP => {
                return 0; // OK cycle
            }
            _ => {
                return 1; // Bad cycle
            }
        }
    }

    // Check attributes
    let attr = ATTRIB(child);
    if !attr.is_null() && attr != R_NilValue() && R_cycle_detected(s, attr) != 0 {
        return 1;
    }

    // Check pairlist
    if isPairList(child) != 0 {
        let mut el = child;
        while !el.is_null() && el != R_NilValue() {
            if s == el || R_cycle_detected(s, CAR(el)) != 0 {
                return 1;
            }
            let el_attr = ATTRIB(el);
            if !el_attr.is_null() && el_attr != R_NilValue() && R_cycle_detected(s, el_attr) != 0 {
                return 1;
            }
            el = CDR(el);
        }
    } else if isVectorList(child) != 0 {
        let len = LENGTH(child);
        for i in 0..len {
            if R_cycle_detected(s, VECTOR_ELT(child, i as R_xlen_t)) != 0 {
                return 1;
            }
        }
    }

    0
}

// ---------------------------------------------------------------------------
// Pairlist duplication
// ---------------------------------------------------------------------------

struct PairlistCellSnapshot<'source> {
    value: Sexp<'source>,
    tag: Sexp<'source>,
    attributes: Sexp<'source>,
    flags: crate::sexp::ffi::SxpInfo,
}

struct PairlistSnapshot<'source> {
    cells: Vec<PairlistCellSnapshot<'source>>,
    // Vec drops before its reservation; keep the original owner's transient
    // charge alive for the complete retained-storage lifetime.
    _reservation: crate::sexp::memory::WorkspaceReservation,
}

/// Validate one pairlist spine node without retaining an arena or payload
/// borrow across the caller. Nil can be the process singleton outside the
/// source heap; non-nil foreign/stale nodes remain typed errors.
fn pairlist_snapshot_node(
    current: &Sexp<'_>,
) -> SexpResult<Option<(crate::sexp::heap::NodeLink, crate::sexp::ffi::SexprecCore)>> {
    let node = match current.allocation() {
        Ok(node) => node,
        Err(SexpError::UnownedPointer { .. }) if current.is_nil() => return Ok(None),
        Err(SexpError::UnownedPointer { .. }) => {
            return Err(SexpError::TypeMismatch {
                expected: "pairlist tail",
                actual: current.typeof_(),
            });
        }
        Err(error) => return Err(error),
    };
    let key = node.link().ok_or(SexpError::StaleAllocation)?;
    let header = node
        .heap_identity()
        .node_snapshot(&node)
        .ok_or(SexpError::StaleAllocation)?;
    let kind = header.sxpinfo.type_of();
    if kind == SEXPTYPE::NILSXP {
        return Ok(None);
    }
    if !matches!(
        kind,
        SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP
    ) {
        return Err(SexpError::TypeMismatch {
            expected: "pairlist tail",
            actual: kind,
        });
    }
    if !matches!(header.data, crate::sexp::ffi::NodeBody::List(_)) {
        return Err(SexpError::TypeMismatch {
            expected: "pairlist storage",
            actual: kind,
        });
    }
    Ok(Some((key, header)))
}

fn count_pairlist_cells_reject_cycle(source: &Sexp<'_>) -> SexpResult<usize> {
    let mut current = source.clone();
    let mut _checkpoint: Option<Sexp<'_>> = None;
    let mut checkpoint_key = None;
    let mut power = 1usize;
    let mut span = 0usize;
    let mut count = 0usize;
    loop {
        // Brent's checkpoint cycle detector needs one cursor: every checked
        // header both validates and counts a cell. The checkpoint owns its
        // Sexp root, while its cached NodeLink avoids rereading that header.
        let Some((key, header)) = pairlist_snapshot_node(&current)? else {
            return Ok(count);
        };
        if checkpoint_key == Some(key) {
            return Err(SexpError::EvaluationFailed {
                message: "cyclic pairlist tail cannot be duplicated".into(),
            });
        }

        count = count.checked_add(1).ok_or(SexpError::AllocationFailed {
            object: "pairlist snapshot size",
        })?;
        if checkpoint_key.is_none() {
            _checkpoint = Some(current.clone());
            checkpoint_key = Some(key);
            power = 2;
            span = 0;
        } else {
            span = span.checked_add(1).ok_or(SexpError::AllocationFailed {
                object: "pairlist snapshot size",
            })?;
            if span == power {
                _checkpoint = Some(current.clone());
                checkpoint_key = Some(key);
                power = power.saturating_mul(2);
                span = 0;
            }
        }
        let crate::sexp::ffi::NodeBody::List(list) = header.data else {
            unreachable!("pairlist node validation checked list storage")
        };
        current = current.checked_child(list.cdrval)?;
    }
}

/// Resolve each original edge once before allocations or provider callbacks.
/// Only child values are retained; detached source tail cells may be collected.
fn snapshot_pairlist<'source>(
    source: &Sexp<'source>,
    access: &crate::sexp::owner::RuntimeAccess,
) -> SexpResult<PairlistSnapshot<'source>> {
    // Validate and count in one Brent pass before allocating, avoiding both a
    // dynamic cycle set and a separate length traversal.
    let count = count_pairlist_cells_reject_cycle(source)?;
    let bytes = count
        .checked_mul(std::mem::size_of::<PairlistCellSnapshot<'source>>())
        .ok_or(SexpError::AllocationFailed {
            object: "pairlist snapshot size",
        })?;
    let mut reservation = access
        .with_arena(|arena| arena.reserve_workspace(bytes))?
        .ok_or(SexpError::AllocationFailed {
            object: "pairlist snapshot workspace",
        })?;
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(count)
        .map_err(|_| SexpError::AllocationFailed {
            object: "pairlist snapshot",
        })?;
    // `try_reserve_exact` has no R callback/finalizer path. Reconcile the
    // admitted logical capacity before retaining or publishing any snapshot
    // edges, including allocator-reported excess capacity.
    let actual_bytes = cells
        .capacity()
        .checked_mul(std::mem::size_of::<PairlistCellSnapshot<'source>>())
        .ok_or(SexpError::AllocationFailed {
            object: "pairlist snapshot size",
        })?;
    reservation
        .try_resize(actual_bytes)
        .ok_or(SexpError::AllocationFailed {
            object: "pairlist snapshot workspace",
        })?;
    // From this point through the final edge capture, the code only reads
    // checked headers and creates owner leases. It does not evaluate R, call
    // ALTREP providers, collect, or run finalizers, so Brent validation and
    // the count cannot be invalidated between passes.
    let mut current = source.clone();
    for _ in 0..count {
        let (_, header) = pairlist_snapshot_node(&current)?.ok_or(SexpError::StaleAllocation)?;
        let kind = header.sxpinfo.type_of();
        let crate::sexp::ffi::NodeBody::List(list) = header.data else {
            return Err(SexpError::TypeMismatch {
                expected: "pairlist storage",
                actual: kind,
            });
        };
        cells.push(PairlistCellSnapshot {
            value: current.checked_child(list.carval)?,
            tag: current.checked_child(list.tagval)?,
            attributes: current.checked_child(header.attrib)?,
            flags: header.sxpinfo,
        });
        current = current.checked_child(list.cdrval)?;
    }
    Ok(PairlistSnapshot {
        cells,
        _reservation: reservation,
    })
}

fn apply_pairlist_flags(copy: &Sexp<'_>, flags: crate::sexp::ffi::SxpInfo) -> SexpResult<()> {
    let node = copy.allocation()?;
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
    header.sxpinfo.set_obj(flags.obj());
    let gp = (header.sxpinfo.gp() & !S4_OBJECT_MASK) | (flags.gp() & S4_OBJECT_MASK);
    header.sxpinfo.set_gp(gp);
    heap.replace_node(node, header)
        .ok_or(SexpError::StaleAllocation)
}

/// A suspended copy retains its actual selected values, unfinished output,
/// checked path identity and provider guard. Moving a frame never moves a heap
/// node, and popping it releases only that occurrence's active-path identity.
struct OwnedCopyFrame<'source> {
    _source: Sexp<'source>,
    key: crate::sexp::heap::NodeLink,
    _provider: Option<crate::sexp::altrep::OperationGuard>,
    body: CopyContinuation<'source>,
}

enum CopyContinuation<'source> {
    Vector(VectorContinuation<'source>),
    Pairlist(PairlistContinuation<'source>),
    Closure(ClosureContinuation<'source>),
}

enum VectorPending {
    None,
    Value(usize),
    Attributes,
}

struct VectorContinuation<'source> {
    copy: Sexp<'source>,
    values: CopyValues<'source>,
    attributes: Option<AttributeSnapshot<'source>>,
    flags: crate::sexp::ffi::SxpInfo,
    truelength: R_xlen_t,
    next_value: usize,
    pending: VectorPending,
    _reservation: crate::sexp::memory::TransientReservation,
}

enum PairlistPending {
    None,
    Value,
    Attributes,
}

struct PairlistContinuation<'source> {
    domain: crate::sexp::object::NodeDomain<'source>,
    snapshot: PairlistSnapshot<'source>,
    head: Option<Sexp<'source>>,
    tail: Option<Sexp<'source>>,
    next_cell: usize,
    pending: PairlistPending,
}

struct ClosureContinuation<'source> {
    copy: Sexp<'source>,
    attributes: Option<AttributeSnapshot<'source>>,
    pending: bool,
}

struct CopyRequest<'source> {
    source: Sexp<'source>,
    /// Attribute spines were selected before any provider/allocation callback.
    /// They must not be traversed again when the parent later suspends.
    cells: Option<PairlistSnapshot<'source>>,
}

impl<'source> CopyRequest<'source> {
    fn value(source: Sexp<'source>) -> Self {
        Self {
            source,
            cells: None,
        }
    }

    fn attributes(snapshot: AttributeSnapshot<'source>) -> Self {
        Self {
            source: snapshot.source,
            cells: Some(snapshot.pairlist),
        }
    }
}

// Both containers are private to this copy. A reentrant copy gets its own
// traversal and is still charged to its original owner's ledger.
struct CopyTraversal<'workspace, 'source> {
    frames: AllocVec<OwnedCopyFrame<'source>, TraversalAllocator<'workspace>>,
    active: HashSet<crate::sexp::heap::NodeLink, RandomState, TraversalAllocator<'workspace>>,
}

enum CopyStep<'source> {
    Child(CopyRequest<'source>),
    Done(Sexp<'source>),
}

fn identity_kind(kind: SEXPTYPE) -> bool {
    matches!(
        kind,
        SEXPTYPE::NILSXP
            | SEXPTYPE::SYMSXP
            | SEXPTYPE::ENVSXP
            | SEXPTYPE::SPECIALSXP
            | SEXPTYPE::BUILTINSXP
            | SEXPTYPE::BCODESXP
            | SEXPTYPE::WEAKREFSXP
            | SEXPTYPE::EXTPTRSXP
            | SEXPTYPE::CHARSXP
            | SEXPTYPE::PROMSXP
    )
}

fn retry_traversal_admission<T>(
    access: &crate::sexp::owner::RuntimeAccess,
    mut reserve: impl FnMut() -> SexpResult<T>,
) -> SexpResult<T> {
    match reserve() {
        Err(SexpError::AllocationFailed { .. }) => {
            // The allocator itself never enters the interpreter. All source
            // and continuation roots remain owned across this bounded retry.
            access.with_native(|owner| owner.full_gc().map(|_| ()))?;
            reserve()
        }
        outcome => outcome,
    }
}

fn start_copy<'source>(
    request: CopyRequest<'source>,
    traversal: &mut CopyTraversal<'_, 'source>,
    access: &crate::sexp::owner::RuntimeAccess,
) -> SexpResult<Option<Sexp<'source>>> {
    access.require_active()?;
    let source = request.source;
    let kind = source.typeof_();
    if request.cells.is_none() && identity_kind(kind) {
        return Ok(Some(source));
    }
    if let Some(singleton) = crate::sexp::globals::immutable_singleton_lease(source.as_raw()) {
        let copy = if kind == SEXPTYPE::LGLSXP {
            access.with_native(|_| duplicate_logical_singleton(singleton))?
        } else {
            source
        };
        return Ok(Some(copy));
    }
    let node = source.allocation()?;
    let key = node.link().ok_or(SexpError::StaleAllocation)?;
    if traversal.active.contains(&key) {
        return Err(SexpError::EvaluationFailed {
            message: "cyclic object graph cannot be duplicated".into(),
        });
    }
    // Reserve both containers before copy preparation can allocate R objects
    // or call a provider. Only requests that suspend need storage.
    retry_traversal_admission(access, || {
        traversal
            .frames
            .try_reserve(1)
            .map_err(|_| SexpError::AllocationFailed {
                object: "duplication continuations",
            })?;
        traversal
            .active
            .try_reserve(1)
            .map_err(|_| SexpError::AllocationFailed {
                object: "duplication active path",
            })?;
        Ok(())
    })?;
    traversal.active.insert(key);
    let flags = match &request.cells {
        Some(cells) => {
            cells
                .cells
                .first()
                .ok_or(SexpError::MissingData { sexptype: kind })?
                .flags
        }
        None => {
            node.heap_identity()
                .node_snapshot(node)
                .ok_or(SexpError::StaleAllocation)?
                .sxpinfo
        }
    };
    let extension = unsafe { crate::sexp::altrep::has_extension_raw(source.as_raw()) };
    let provider = if extension {
        Some(crate::sexp::altrep::duplication_guard(&source)?)
    } else {
        None
    };
    let body = match flags.type_of() {
        SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP => {
            let snapshot = match request.cells {
                Some(snapshot) => snapshot,
                None => snapshot_pairlist(&source, access)?,
            };
            CopyContinuation::Pairlist(PairlistContinuation {
                domain: source.node_factory()?.into(),
                snapshot,
                head: None,
                tail: None,
                next_cell: 0,
                pending: PairlistPending::None,
            })
        }
        SEXPTYPE::CLOSXP => CopyContinuation::Closure(prepare_closure(&source, access)?),
        SEXPTYPE::EXPRSXP
        | SEXPTYPE::VECSXP
        | SEXPTYPE::STRSXP
        | SEXPTYPE::LGLSXP
        | SEXPTYPE::INTSXP
        | SEXPTYPE::REALSXP
        | SEXPTYPE::CPLXSXP
        | SEXPTYPE::RAWSXP
        | SEXPTYPE::OBJSXP => CopyContinuation::Vector(prepare_value(&source, flags, access)?),
        actual => {
            return Err(SexpError::TypeMismatch {
                expected: "duplicable object",
                actual,
            });
        }
    };
    traversal.frames.push(OwnedCopyFrame {
        _source: source,
        key,
        _provider: provider,
        body,
    });
    Ok(None)
}

fn publish_attributes(
    parent: &Sexp<'_>,
    child: &Sexp<'_>,
    access: &crate::sexp::owner::RuntimeAccess,
) -> SexpResult<()> {
    access.with_native(|_| {
        SET_ATTRIB(parent.as_raw(), child.as_raw());
        Ok(())
    })
}

impl<'source> VectorContinuation<'source> {
    fn step(
        &mut self,
        result: Option<Sexp<'source>>,
        deep: c_int,
        access: &crate::sexp::owner::RuntimeAccess,
    ) -> SexpResult<CopyStep<'source>> {
        if let Some(result) = result {
            match std::mem::replace(&mut self.pending, VectorPending::None) {
                VectorPending::Value(index) => access.with_native(|_| {
                    SET_VECTOR_ELT(self.copy.as_raw(), index as _, result.as_raw());
                    Ok(())
                })?,
                VectorPending::Attributes => publish_attributes(&self.copy, &result, access)?,
                VectorPending::None => unreachable!("no suspended vector child"),
            }
        }
        if let CopyValues::References(values) = &self.values
            && self.flags.type_of() != SEXPTYPE::STRSXP
        {
            while let Some(value) = values.get(self.next_value) {
                let index = self.next_value;
                self.next_value += 1;
                if deep != 0 {
                    self.pending = VectorPending::Value(index);
                    return Ok(CopyStep::Child(CopyRequest::value(value.clone())));
                }
                access.with_native(|_| {
                    lazy_duplicate(value.as_raw());
                    SET_VECTOR_ELT(self.copy.as_raw(), index as _, value.as_raw());
                    Ok(())
                })?;
            }
        }
        if let Some(attributes) = self.attributes.take() {
            self.pending = VectorPending::Attributes;
            return Ok(CopyStep::Child(CopyRequest::attributes(attributes)));
        }
        apply_vector_flags(&self.copy, self.flags, self.truelength)?;
        access.require_active()?;
        Ok(CopyStep::Done(self.copy.clone()))
    }
}

impl<'source> PairlistContinuation<'source> {
    fn step(
        &mut self,
        mut result: Option<Sexp<'source>>,
        deep: c_int,
        access: &crate::sexp::owner::RuntimeAccess,
    ) -> SexpResult<CopyStep<'source>> {
        let domain = &self.domain;
        loop {
            if let Some(value) = result.take() {
                let saved = &self.snapshot.cells[self.next_cell];
                match std::mem::replace(&mut self.pending, PairlistPending::None) {
                    PairlistPending::Value => {
                        let cell = access.allocator(domain)?.pairlist_cell(
                            &value,
                            &domain.nil(),
                            &saved.tag,
                        )?;
                        if let Some(previous) = &self.tail {
                            access.with_native(|_| {
                                SETCDR(previous.as_raw(), cell.as_raw());
                                Ok(())
                            })?;
                        } else {
                            self.head = Some(cell.clone());
                        }
                        self.tail = Some(cell);
                        if !saved.attributes.is_nil() {
                            self.pending = PairlistPending::Attributes;
                            return Ok(CopyStep::Child(CopyRequest::value(
                                saved.attributes.clone(),
                            )));
                        }
                        self.next_cell += 1;
                    }
                    PairlistPending::Attributes => {
                        let cell = self.tail.as_ref().expect("allocated pairlist cell");
                        publish_attributes(cell, &value, access)?;
                        apply_pairlist_flags(cell, saved.flags)?;
                        self.next_cell += 1;
                    }
                    PairlistPending::None => unreachable!("no suspended pairlist child"),
                }
            }
            let Some(saved) = self.snapshot.cells.get(self.next_cell) else {
                let copy = self.head.as_ref().expect("nonempty pairlist snapshot");
                let node = copy.allocation()?;
                node.heap_identity()
                    .retype_node(node, self.snapshot.cells[0].flags.type_of())
                    .ok_or(SexpError::StaleAllocation)?;
                apply_pairlist_flags(copy, self.snapshot.cells[0].flags)?;
                access.require_active()?;
                return Ok(CopyStep::Done(copy.clone()));
            };
            self.pending = PairlistPending::Value;
            if deep != 0 {
                return Ok(CopyStep::Child(CopyRequest::value(saved.value.clone())));
            }
            access.with_native(|_| {
                lazy_duplicate(saved.value.as_raw());
                Ok(())
            })?;
            result = Some(saved.value.clone());
        }
    }
}

impl<'source> ClosureContinuation<'source> {
    fn step(
        &mut self,
        result: Option<Sexp<'source>>,
        access: &crate::sexp::owner::RuntimeAccess,
    ) -> SexpResult<CopyStep<'source>> {
        if let Some(result) = result {
            assert!(self.pending, "suspended closure attributes");
            self.pending = false;
            publish_attributes(&self.copy, &result, access)?;
        }
        if let Some(attributes) = self.attributes.take() {
            self.pending = true;
            return Ok(CopyStep::Child(CopyRequest::attributes(attributes)));
        }
        access.require_active()?;
        Ok(CopyStep::Done(self.copy.clone()))
    }
}

fn duplicate_iterative<'source>(
    source: Sexp<'source>,
    deep: c_int,
    access: &crate::sexp::owner::RuntimeAccess,
) -> SexpResult<Sexp<'source>> {
    access.require_active()?;
    if identity_kind(source.typeof_()) {
        return Ok(source);
    }
    let workspace = RefCell::new(retry_traversal_admission(access, || {
        access
            .with_arena(|arena| arena.reserve_workspace(0))?
            .ok_or(SexpError::AllocationFailed {
                object: "duplication traversal workspace",
            })
    })?);
    let allocator = TraversalAllocator(&workspace);
    let mut traversal = CopyTraversal {
        frames: AllocVec::new_in(allocator),
        active: HashSet::with_hasher_in(RandomState::new(), allocator),
    };
    let mut next = Some(CopyRequest::value(source));
    let mut result = None;
    loop {
        access.require_active()?;
        if let Some(request) = next.take() {
            result = start_copy(request, &mut traversal, access)?;
        }
        let Some(frame) = traversal.frames.last_mut() else {
            return result.ok_or(SexpError::MissingData {
                sexptype: SEXPTYPE::NILSXP,
            });
        };
        let step = match &mut frame.body {
            CopyContinuation::Vector(body) => body.step(result.take(), deep, access)?,
            CopyContinuation::Pairlist(body) => body.step(result.take(), deep, access)?,
            CopyContinuation::Closure(body) => body.step(result.take(), access)?,
        };
        match step {
            CopyStep::Child(request) => next = Some(request),
            CopyStep::Done(value) => {
                traversal.active.remove(&frame.key);
                traversal.frames.pop();
                result = Some(value);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// copyVector
// ---------------------------------------------------------------------------

/// Copy the contents of vector `t` into vector `s`.
///
/// Both vectors must have the same type. The source `t` is recycled
/// into the destination `s` if it is shorter.
pub fn copyVector(s: SEXP, t: SEXP) {
    let sT = TYPEOF(s);
    let tT = TYPEOF(t);
    if sT != tT {
        return; // In real R this would error
    }
    let ns = XLENGTH(s);
    let nt = XLENGTH(t);

    match SEXPTYPE(sT) {
        SEXPTYPE::STRSXP => {
            xcopyStringWithRecycle(s, t, 0, ns, nt);
        }
        SEXPTYPE::LGLSXP => {
            let (src, mut dst) = (source_vector(t), target_vector(s));
            recycle_pairs(ns, nt, |i, j| {
                checked(dst.try_set_logical_elt(i, checked(src.try_logical_elt(j))))
            });
        }
        SEXPTYPE::INTSXP => {
            let (src, mut dst) = (source_vector(t), target_vector(s));
            recycle_pairs(ns, nt, |i, j| {
                checked(dst.try_set_integer_elt(i, checked(src.try_integer_elt(j))))
            });
        }
        SEXPTYPE::REALSXP => {
            let (src, mut dst) = (source_vector(t), target_vector(s));
            recycle_pairs(ns, nt, |i, j| {
                checked(dst.try_set_real_elt(i, checked(src.try_real_elt(j))))
            });
        }
        SEXPTYPE::CPLXSXP => {
            let (src, mut dst) = (source_vector(t), target_vector(s));
            recycle_pairs(ns, nt, |i, j| {
                checked(dst.try_set_complex_elt(i, checked(src.try_complex_elt(j))))
            });
        }
        SEXPTYPE::EXPRSXP | SEXPTYPE::VECSXP => {
            xcopyVectorWithRecycle(s, t, 0, ns, nt);
        }
        SEXPTYPE::RAWSXP => {
            let (src, mut dst) = (source_vector(t), target_vector(s));
            recycle_pairs(ns, nt, |i, j| {
                checked(dst.try_set_raw_elt(i, checked(src.try_raw_elt(j))))
            });
        }
        _ => {
            UNIMPLEMENTED_TYPE("copyVector", s);
        }
    }
}

// ---------------------------------------------------------------------------
// copyListMatrix (legacy, no longer used by R)
// ---------------------------------------------------------------------------

/// Copy list matrix contents (legacy function).
#[allow(clippy::never_loop)]
pub fn copyListMatrix(s: SEXP, t: SEXP, byrow: c_int) {
    let nr = nrows(s);
    let nc = ncols(s);
    let ns = (nr as R_xlen_t) * (nc as R_xlen_t);

    let mut pt = t;
    if byrow != 0 {
        let nR = nr as R_xlen_t;
        let tmp = Rf_allocVector3(SEXPTYPE::VECSXP, ns);
        for i in 0..nr {
            for j in 0..nc {
                let idx = (i as R_xlen_t) + (j as R_xlen_t) * nR;
                SET_VECTOR_ELT(tmp, idx, duplicate(CAR(pt)));
                pt = CDR(pt);
                if pt.is_null() || pt == R_NilValue() {
                    pt = t;
                }
            }
        }
        let mut sp = s;
        for i in 0..ns {
            SETCAR(sp, VECTOR_ELT(tmp, i));
            sp = CDR(sp);
        }
    } else {
        let mut sp = s;
        for _ in 0..ns {
            SETCAR(sp, duplicate(CAR(pt)));
            sp = CDR(sp);
            pt = CDR(pt);
            if pt.is_null() || pt == R_NilValue() {
                pt = t;
            }
            if sp.is_null() || sp == R_NilValue() {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// copyMatrix
// ---------------------------------------------------------------------------

/// Helper for VECTOR_ELT_LD: returns lazy_duplicate of the element.
fn VECTOR_ELT_LD(x: SEXP, i: R_xlen_t) -> SEXP {
    lazy_duplicate(VECTOR_ELT(x, i))
}

/// Copy matrix contents from `t` into `s`.
///
/// If `byrow` is nonzero, fills by rows; otherwise fills by columns
/// (which is equivalent to copyVector).
pub fn copyMatrix(s: SEXP, t: SEXP, byrow: c_int) {
    let nr = nrows(s) as R_xlen_t;
    let nc = ncols(s) as R_xlen_t;
    let nt = XLENGTH(t);

    if byrow != 0 {
        match SEXPTYPE(TYPEOF(s)) {
            SEXPTYPE::STRSXP => {
                fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                    SET_STRING_ELT(s, didx, STRING_ELT(t, sidx));
                });
            }
            SEXPTYPE::LGLSXP => {
                let (src, mut dst) = (source_vector(t), target_vector(s));
                fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                    checked(dst.try_set_logical_elt(didx, checked(src.try_logical_elt(sidx))))
                });
            }
            SEXPTYPE::INTSXP => {
                let (src, mut dst) = (source_vector(t), target_vector(s));
                fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                    checked(dst.try_set_integer_elt(didx, checked(src.try_integer_elt(sidx))))
                });
            }
            SEXPTYPE::REALSXP => {
                let (src, mut dst) = (source_vector(t), target_vector(s));
                fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                    checked(dst.try_set_real_elt(didx, checked(src.try_real_elt(sidx))))
                });
            }
            SEXPTYPE::CPLXSXP => {
                let (src, mut dst) = (source_vector(t), target_vector(s));
                fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                    checked(dst.try_set_complex_elt(didx, checked(src.try_complex_elt(sidx))))
                });
            }
            SEXPTYPE::EXPRSXP | SEXPTYPE::VECSXP => {
                fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                    SET_VECTOR_ELT(s, didx, VECTOR_ELT_LD(t, sidx));
                });
            }
            SEXPTYPE::RAWSXP => {
                let (src, mut dst) = (source_vector(t), target_vector(s));
                fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                    checked(dst.try_set_raw_elt(didx, checked(src.try_raw_elt(sidx))))
                });
            }
            _ => {
                UNIMPLEMENTED_TYPE("copyMatrix", s);
            }
        }
    } else {
        copyVector(s, t);
    }
}

// ---------------------------------------------------------------------------
// xcopy*WithRecycle functions
// ---------------------------------------------------------------------------

/// Copy complex data with recycling.
pub unsafe fn xcopyComplexWithRecycle(
    dst: *mut Rcomplex,
    src: *const Rcomplex,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                *dst.add((dstart + i) as usize) = *src.add(i as usize);
            }
            return;
        }
        if nsrc == 1 {
            let val = *src;
            for i in 0..n {
                *dst.add((dstart + i) as usize) = val;
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            *dst.add((dstart + i) as usize) = *src.add(sidx as usize);
            sidx += 1;
        }
    }
}

/// Copy integer data with recycling.
pub unsafe fn xcopyIntegerWithRecycle(
    dst: *mut c_int,
    src: *const c_int,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                *dst.add((dstart + i) as usize) = *src.add(i as usize);
            }
            return;
        }
        if nsrc == 1 {
            let val = *src;
            for i in 0..n {
                *dst.add((dstart + i) as usize) = val;
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            *dst.add((dstart + i) as usize) = *src.add(sidx as usize);
            sidx += 1;
        }
    }
}

/// Copy logical data with recycling.
pub unsafe fn xcopyLogicalWithRecycle(
    dst: *mut c_int,
    src: *const c_int,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        xcopyIntegerWithRecycle(dst, src, dstart, n, nsrc);
    }
}

/// Copy raw data with recycling.
pub unsafe fn xcopyRawWithRecycle(
    dst: *mut Rbyte,
    src: *const Rbyte,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                *dst.add((dstart + i) as usize) = *src.add(i as usize);
            }
            return;
        }
        if nsrc == 1 {
            let val = *src;
            for i in 0..n {
                *dst.add((dstart + i) as usize) = val;
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            *dst.add((dstart + i) as usize) = *src.add(sidx as usize);
            sidx += 1;
        }
    }
}

/// Copy real (double) data with recycling.
pub unsafe fn xcopyRealWithRecycle(
    dst: *mut c_double,
    src: *const c_double,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                *dst.add((dstart + i) as usize) = *src.add(i as usize);
            }
            return;
        }
        if nsrc == 1 {
            let val = *src;
            for i in 0..n {
                *dst.add((dstart + i) as usize) = val;
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            *dst.add((dstart + i) as usize) = *src.add(sidx as usize);
            sidx += 1;
        }
    }
}

/// Copy string vector elements with recycling.
pub fn xcopyStringWithRecycle(dst: SEXP, src: SEXP, dstart: R_xlen_t, n: R_xlen_t, nsrc: R_xlen_t) {
    if dst.is_null() || src.is_null() || n == 0 {
        return;
    }
    if nsrc >= n {
        for i in 0..n {
            SET_STRING_ELT(dst, dstart + i, STRING_ELT(src, i));
        }
        return;
    }
    if nsrc == 1 {
        let val = STRING_ELT(src, 0);
        for i in 0..n {
            SET_STRING_ELT(dst, dstart + i, val);
        }
        return;
    }
    let mut sidx: R_xlen_t = 0;
    for i in 0..n {
        if sidx == nsrc {
            sidx = 0;
        }
        SET_STRING_ELT(dst, dstart + i, STRING_ELT(src, sidx));
        sidx += 1;
    }
}

/// Copy generic vector elements with recycling.
pub fn xcopyVectorWithRecycle(dst: SEXP, src: SEXP, dstart: R_xlen_t, n: R_xlen_t, nsrc: R_xlen_t) {
    if dst.is_null() || src.is_null() || n == 0 {
        return;
    }
    if nsrc >= n {
        for i in 0..n {
            SET_VECTOR_ELT(dst, dstart + i, VECTOR_ELT_LD(src, i));
        }
        return;
    }
    if nsrc == 1 {
        let val = VECTOR_ELT_LD(src, 0);
        for i in 0..n {
            SET_VECTOR_ELT(dst, dstart + i, val);
        }
        return;
    }
    let mut sidx: R_xlen_t = 0;
    for i in 0..n {
        if sidx == nsrc {
            sidx = 0;
        }
        SET_VECTOR_ELT(dst, dstart + i, VECTOR_ELT_LD(src, sidx));
        sidx += 1;
    }
}

// ---------------------------------------------------------------------------
// xfill*MatrixWithRecycle functions
// ---------------------------------------------------------------------------

/// Fill complex matrix with recycling.
pub unsafe fn xfillComplexMatrixWithRecycle(
    dst: *mut Rcomplex,
    src: *mut Rcomplex,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            *dst.add(didx as usize) = *src.add(sidx as usize);
        });
    }
}

/// Fill integer matrix with recycling.
pub unsafe fn xfillIntegerMatrixWithRecycle(
    dst: *mut c_int,
    src: *mut c_int,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            *dst.add(didx as usize) = *src.add(sidx as usize);
        });
    }
}

/// Fill logical matrix with recycling.
pub unsafe fn xfillLogicalMatrixWithRecycle(
    dst: *mut c_int,
    src: *mut c_int,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        xfillIntegerMatrixWithRecycle(dst, src, dstart, drows, srows, cols, nsrc);
    }
}

/// Fill raw matrix with recycling.
pub unsafe fn xfillRawMatrixWithRecycle(
    dst: *mut Rbyte,
    src: *mut Rbyte,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            *dst.add(didx as usize) = *src.add(sidx as usize);
        });
    }
}

/// Fill real matrix with recycling.
pub unsafe fn xfillRealMatrixWithRecycle(
    dst: *mut c_double,
    src: *mut c_double,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            *dst.add(didx as usize) = *src.add(sidx as usize);
        });
    }
}

/// Fill string matrix with recycling.
pub fn xfillStringMatrixWithRecycle(
    dst: SEXP,
    src: SEXP,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
        SET_STRING_ELT(dst, didx, STRING_ELT(src, sidx));
    });
}

/// Fill generic vector matrix with recycling.
pub fn xfillVectorMatrixWithRecycle(
    dst: SEXP,
    src: SEXP,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
        SET_VECTOR_ELT(dst, didx, VECTOR_ELT(src, sidx));
    });
}

// ---------------------------------------------------------------------------
// duplicate_attr: duplicate before attribute modification
// ---------------------------------------------------------------------------

/// Duplicate before attribute modification using ordinary deep or shallow
/// value and public-attribute copying.
fn duplicate_attr(x: SEXP, deep: c_int) -> SEXP {
    if x.is_null() {
        return x;
    }
    if deep != 0 {
        duplicate(x)
    } else {
        shallow_duplicate(x)
    }
}

/// Shallow duplicate before attribute modification.
pub fn R_shallow_duplicate_attr(x: SEXP) -> SEXP {
    duplicate_attr(x, 0)
}

/// Deep duplicate before attribute modification.
pub fn R_duplicate_attr(x: SEXP) -> SEXP {
    duplicate_attr(x, 1)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "duplicate/altrep_tests.rs"]
mod altrep_tests;

#[cfg(test)]
#[path = "duplicate/extptr_identity_tests.rs"]
mod extptr_identity_tests;

#[cfg(test)]
#[path = "duplicate/owned_pairlist_tests.rs"]
mod owned_pairlist_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::constructors::Rf_ScalarInteger;
    use crate::sexp::memory::RArena;

    fn owning_closure_fixture(
        session: &crate::sexp::session::RSession,
        terms: usize,
    ) -> crate::sexp::object::Sexp<'_> {
        let factory = session.owner_token().unwrap().node_factory();
        let scalar = factory.wrap(unsafe { Rf_ScalarInteger(42) }).unwrap();
        let mut formals = factory.nil();
        for _ in 0..terms {
            formals = factory
                .pairlist_cell(&scalar, &formals, &factory.nil())
                .unwrap();
        }
        let head = factory
            .wrap(unsafe { crate::sexp::symbol::install_str("+") })
            .unwrap();
        let body = factory
            .allocate(|arena| {
                let call = arena.alloc_node(SEXPTYPE::LANGSXP);
                let node = arena.node_token(call)?;
                let heap = arena.heap_identity();
                let mut header = heap.node_snapshot(&node)?;
                header.data.list_mut().carval = factory.link(&head).ok()?;
                header.data.list_mut().cdrval = factory.link(&formals).ok()?;
                heap.replace_node(&node, header)?;
                Some(call)
            })
            .unwrap();
        let environment = factory
            .wrap(unsafe { crate::sexp::globals::R_GlobalEnv() })
            .unwrap();
        let attributes = factory
            .pairlist_cell(&scalar, &factory.nil(), &head)
            .unwrap();
        factory
            .allocate(|arena| {
                let closure = arena.alloc_node(SEXPTYPE::CLOSXP);
                let node = arena.node_token(closure)?;
                let heap = arena.heap_identity();
                let mut header = heap.node_snapshot(&node)?;
                header.data.closure_mut().formals = factory.link(&formals).ok()?;
                header.data.closure_mut().body = factory.link(&body).ok()?;
                header.data.closure_mut().env = factory.link(&environment).ok()?;
                header.attrib = factory.link(&attributes).ok()?;
                heap.replace_node(&node, header)?;
                Some(closure)
            })
            .unwrap()
    }

    #[test]
    fn owning_closure_duplicate_shares_syntax_with_constant_allocation_cost() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        for terms in [1, 32] {
            for deep in [false, true] {
                let source = owning_closure_fixture(&session, terms);
                let factory = source.node_factory().unwrap();
                let owner = session.owner_token().unwrap();
                let before = owner.with_arena(|arena| arena.node_count()).unwrap();
                let pointer = unsafe {
                    if deep {
                        duplicate(source.as_raw())
                    } else {
                        shallow_duplicate(source.as_raw())
                    }
                };
                let copy = factory.wrap(pointer).unwrap();
                let after = owner.with_arena(|arena| arena.node_count()).unwrap();
                assert_ne!(copy, source);
                assert_eq!(
                    copy.try_formals().unwrap(),
                    source.try_formals().unwrap(),
                    "GNU closure formals retain original identity"
                );
                assert_eq!(
                    copy.try_body().unwrap(),
                    source.try_body().unwrap(),
                    "GNU closure body retains original identity"
                );
                assert_eq!(copy.try_cloenv().unwrap(), source.try_cloenv().unwrap());
                assert_ne!(copy.try_attrib().unwrap(), source.try_attrib().unwrap());
                let original_attribute = source.try_attrib().unwrap().try_car().unwrap();
                let copied_attribute = copy.try_attrib().unwrap().try_car().unwrap();
                if deep {
                    assert_ne!(copied_attribute, original_attribute);
                } else {
                    assert_eq!(copied_attribute, original_attribute);
                }
                assert_eq!(
                    after - before,
                    if deep { 3 } else { 2 },
                    "syntax size must not add closure-copy allocations"
                );
            }
        }
    }

    #[test]
    fn owning_closure_duplicate_retains_original_edges_across_detachment_and_gc() {
        use std::{cell::Cell, rc::Rc};
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let source = owning_closure_fixture(&session, 32);
        let factory = source.node_factory().unwrap();
        let formals = source.try_formals().unwrap();
        let body = source.try_body().unwrap();
        let environment = source.try_cloenv().unwrap();
        let attributes = source.try_attrib().unwrap();
        let attributes_node = attributes.allocation().unwrap().clone();
        let expected_attributes = factory.link(&attributes).unwrap();
        let source_node = source.allocation().unwrap().clone();
        let expected_formals = factory.link(&formals).unwrap();
        let expected_body = factory.link(&body).unwrap();
        let expected_environment = factory.link(&environment).unwrap();
        let source_link = factory.link(&source).unwrap();
        drop((formals, body, environment, attributes));
        unsafe {
            SET_NOJIT(source.as_raw());
            SET_MAYBEJIT(source.as_raw());
            SET_OBJECT(source.as_raw(), 1);
            SET_S4_OBJECT(source.as_raw());
        }
        let nil_link = factory.link(&factory.nil()).unwrap();
        let heap = source_node.heap_identity();
        let seen = Rc::new(Cell::new(0));
        let observed = seen.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if observed.replace(observed.get() + 1) != 0 {
                return;
            }
            let (_, copy_node) = crate::sexp::memory::automatic_roots(&heap)
                .into_iter()
                .find(|(_, node)| {
                    node.link() != Some(source_link)
                        && heap
                            .node_snapshot(node)
                            .is_some_and(|header| header.sxpinfo.type_of() == SEXPTYPE::CLOSXP)
                })
                .expect("closure copy is rooted and initialized before callbacks");
            let header = heap.node_snapshot(&copy_node).unwrap();
            assert_eq!(header.data.closure().formals, expected_formals);
            assert_eq!(header.data.closure().body, expected_body);
            unsafe {
                crate::sexp::owner::OwnerToken::with_current(|owner| {
                    (*owner.as_ptr()).memory_state.gc_force_gap = 0;
                })
                .unwrap()
            }
            let mut source_header = heap.node_snapshot(&source_node).unwrap();
            source_header.data.closure_mut().formals = nil_link;
            source_header.data.closure_mut().body = nil_link;
            source_header.attrib = nil_link;
            source_header.sxpinfo.set_obj(false);
            source_header.sxpinfo.set_gp(0);
            heap.replace_node(&source_node, source_header).unwrap();
            crate::sexp::gengc::full_gc();
            assert!(copy_node.is_live());
            assert!(
                attributes_node.is_live(),
                "original attribute snapshot survives source detachment"
            );
        }));
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
        let copy = factory.wrap(duplicate(source.as_raw())).unwrap();
        assert!(seen.get() > 0);
        assert_eq!(
            factory.link(&copy.try_formals().unwrap()).unwrap(),
            expected_formals
        );
        assert_eq!(
            factory.link(&copy.try_body().unwrap()).unwrap(),
            expected_body
        );
        assert_eq!(
            factory.link(&copy.try_cloenv().unwrap()).unwrap(),
            expected_environment
        );
        unsafe {
            assert_ne!(NOJIT(copy.as_raw()), 0);
            assert_ne!(MAYBEJIT(copy.as_raw()), 0);
            assert_eq!(OBJECT(copy.as_raw()), 1);
            assert_ne!(IS_S4_OBJECT(copy.as_raw()), 0);
        }
        let copied_attributes = copy.try_attrib().unwrap();
        assert_ne!(
            factory.link(&copied_attributes).unwrap(),
            expected_attributes
        );
        assert_eq!(
            copied_attributes
                .try_car()
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            42
        );
    }

    /// Helper to create an integer vector with values.
    unsafe fn make_int_vector(values: &[c_int]) -> SEXP {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::INTSXP, values.len() as R_xlen_t);
            for (i, &val) in values.iter().enumerate() {
                *INTEGER(v).add(i) = val;
            }
            v
        }
    }

    /// Helper to create a real vector with values.
    unsafe fn make_real_vector(values: &[c_double]) -> SEXP {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
            for (i, &val) in values.iter().enumerate() {
                *REAL(v).add(i) = val;
            }
            v
        }
    }

    #[test]
    fn test_duplicate_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let d = duplicate(R_NilValue());
            assert_eq!(d, R_NilValue());
        }
    }

    #[test]
    fn test_duplicate_integer_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[1, 2, 3]);
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(d), 3);
            assert_ne!(d, v); // Should be a new allocation
            assert_eq!(*INTEGER(d).add(0), 1);
            assert_eq!(*INTEGER(d).add(1), 2);
            assert_eq!(*INTEGER(d).add(2), 3);
        }
    }

    #[test]
    fn test_duplicate_real_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_real_vector(&[1.5, 2.5, 3.5]);
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(d), 3);
            assert_ne!(d, v);
            assert!((*REAL(d).add(0) - 1.5).abs() < 1e-10);
            assert!((*REAL(d).add(1) - 2.5).abs() < 1e-10);
            assert!((*REAL(d).add(2) - 3.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_shallow_duplicate_integer() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[10, 20]);
            let d = shallow_duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(d).add(0), 10);
            assert_eq!(*INTEGER(d).add(1), 20);
        }
    }

    #[test]
    fn test_lazy_duplicate() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[5, 6, 7]);
            let d = lazy_duplicate(v);
            assert_eq!(d, v); // Same pointer - no copy
            assert_eq!(NAMED(d), 2); // NAMEDMAX
        }
    }

    #[test]
    fn test_duplicate_pairlist() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let car = Rf_ScalarInteger(1);
            // Create a proper nil-terminated pairlist: (1)
            let list = Rf_cons(car, R_NilValue());

            let d = duplicate(list);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::LISTSXP);
            assert_ne!(d, list); // New allocation
            // CAR should be a duplicate of the original scalar
            let d_car = CAR(d);
            assert_eq!(TYPEOF(d_car), SEXPTYPE::INTSXP);
            // CDR should be nil
            assert_eq!(CDR(d), R_NilValue());
        }
    }

    #[test]
    fn test_duplicate_closure() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Fixture values must live in the session's graph; a detached
            // RArena crosses the ownership boundary enforced at duplicate().
            let formals = R_NilValue();
            let body = Rf_ScalarInteger(42);
            let _body_guard = crate::sexp::protect::protect(body);
            let env = crate::sexp::memory_ext::allocSExp(SEXPTYPE::ENVSXP);

            // Create closure using mkCLOSXP
            let c = crate::mainutils::dstruct::mkCLOSXP(formals, body, env);
            let _c_guard = crate::sexp::protect::protect(c);
            let d = duplicate(c);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::CLOSXP);
            assert_ne!(d, c);
            // Formals, body, env are shared (not deep-copied)
            assert_eq!(FORMALS(d), formals);
            assert_eq!(BODY(d), body);
            assert_eq!(CLOENV(d), env);
        }
    }

    #[test]
    fn test_cycle_detected_self() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(1);
            // Self-reference is a cycle for most types
            assert_ne!(R_cycle_detected(v, v), 0);
        }
    }

    #[test]
    fn test_cycle_detected_nil_ok() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // NILSXP self-reference is OK
            assert_eq!(R_cycle_detected(R_NilValue(), R_NilValue()), 0);
        }
    }

    #[test]
    fn test_cycle_detected_sym_ok() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut arena = RArena::new();
            let sym = arena.alloc_node(SEXPTYPE::SYMSXP);
            // SYMSXP self-reference is OK
            assert_eq!(R_cycle_detected(sym, sym), 0);
        }
    }

    #[test]
    fn test_cycle_detected_no_cycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            assert_eq!(R_cycle_detected(a, b), 0);
        }
    }

    #[test]
    fn test_copy_vector_int() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            let src = make_int_vector(&[10, 20, 30]);
            copyVector(dst, src);
            assert_eq!(*INTEGER(dst).add(0), 10);
            assert_eq!(*INTEGER(dst).add(1), 20);
            assert_eq!(*INTEGER(dst).add(2), 30);
        }
    }

    #[test]
    fn test_copy_vector_real() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
            let src = make_real_vector(&[1.0, 2.0, 3.0]);
            copyVector(dst, src);
            assert!((*REAL(dst).add(0) - 1.0).abs() < 1e-10);
            assert!((*REAL(dst).add(1) - 2.0).abs() < 1e-10);
            assert!((*REAL(dst).add(2) - 3.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_copy_vector_with_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::INTSXP, 6);
            let src = make_int_vector(&[1, 2]);
            copyVector(dst, src);
            assert_eq!(*INTEGER(dst).add(0), 1);
            assert_eq!(*INTEGER(dst).add(1), 2);
            assert_eq!(*INTEGER(dst).add(2), 1);
            assert_eq!(*INTEGER(dst).add(3), 2);
            assert_eq!(*INTEGER(dst).add(4), 1);
            assert_eq!(*INTEGER(dst).add(5), 2);
        }
    }

    #[test]
    fn test_xcopy_real_no_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
            let src = make_real_vector(&[1.0, 2.0, 3.0]);
            xcopyRealWithRecycle(REAL(dst), REAL(src), 0, 3, 3);
            assert!((*REAL(dst).add(0) - 1.0).abs() < 1e-10);
            assert!((*REAL(dst).add(1) - 2.0).abs() < 1e-10);
            assert!((*REAL(dst).add(2) - 3.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_xcopy_real_with_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
            let src = make_real_vector(&[10.0, 20.0]);
            xcopyRealWithRecycle(REAL(dst), REAL(src), 0, 5, 2);
            assert!((*REAL(dst).add(0) - 10.0).abs() < 1e-10);
            assert!((*REAL(dst).add(1) - 20.0).abs() < 1e-10);
            assert!((*REAL(dst).add(2) - 10.0).abs() < 1e-10);
            assert!((*REAL(dst).add(3) - 20.0).abs() < 1e-10);
            assert!((*REAL(dst).add(4) - 10.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_xcopy_real_scalar_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::REALSXP, 4);
            let src = make_real_vector(&[42.0]);
            xcopyRealWithRecycle(REAL(dst), REAL(src), 0, 4, 1);
            for i in 0..4 {
                assert!((*REAL(dst).add(i) - 42.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_xcopy_int_with_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::INTSXP, 5);
            let src = make_int_vector(&[7, 8, 9]);
            xcopyIntegerWithRecycle(INTEGER(dst), INTEGER(src), 0, 5, 3);
            assert_eq!(*INTEGER(dst).add(0), 7);
            assert_eq!(*INTEGER(dst).add(1), 8);
            assert_eq!(*INTEGER(dst).add(2), 9);
            assert_eq!(*INTEGER(dst).add(3), 7);
            assert_eq!(*INTEGER(dst).add(4), 8);
        }
    }

    #[test]
    fn test_xcopy_null_pointers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Should not crash
            xcopyRealWithRecycle(ptr::null_mut(), ptr::null(), 0, 0, 0);
            xcopyIntegerWithRecycle(ptr::null_mut(), ptr::null(), 0, 0, 0);
        }
    }

    #[test]
    fn test_shallow_duplicate_attr() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[1, 2, 3]);
            let d = R_shallow_duplicate_attr(v);
            assert!(!d.is_null());
        }
    }

    #[test]
    fn test_deep_duplicate_attr() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[1, 2, 3]);
            let d = R_duplicate_attr(v);
            assert!(!d.is_null());
        }
    }

    #[test]
    fn test_duplicate_raw_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::RAWSXP, 4);
            *RAW(v).add(0) = 0xAA;
            *RAW(v).add(1) = 0xBB;
            *RAW(v).add(2) = 0xCC;
            *RAW(v).add(3) = 0xDD;
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::RAWSXP);
            assert_eq!(LENGTH(d), 4);
            assert_eq!(*RAW(d).add(0), 0xAA);
            assert_eq!(*RAW(d).add(1), 0xBB);
            assert_eq!(*RAW(d).add(2), 0xCC);
            assert_eq!(*RAW(d).add(3), 0xDD);
        }
    }

    #[test]
    fn test_duplicate_logical_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::LGLSXP, 2);
            *LOGICAL(v).add(0) = 1;
            *LOGICAL(v).add(1) = 0;
            let d = duplicate(v);
            assert_eq!(*LOGICAL(d).add(0), 1);
            assert_eq!(*LOGICAL(d).add(1), 0);
        }
    }

    #[test]
    fn test_duplicate_complex_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::CPLXSXP, 2);
            *COMPLEX(v).add(0) = Rcomplex { r: 1.0, i: 2.0 };
            *COMPLEX(v).add(1) = Rcomplex { r: 3.0, i: 4.0 };
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::CPLXSXP);
            assert!(((*COMPLEX(d).add(0)).r - 1.0).abs() < 1e-10);
            assert!(((*COMPLEX(d).add(0)).i - 2.0).abs() < 1e-10);
            assert!(((*COMPLEX(d).add(1)).r - 3.0).abs() < 1e-10);
            assert!(((*COMPLEX(d).add(1)).i - 4.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_duplicate_string_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s1 = crate::sexp::constructors::mk_char_str("a");
            let s2 = crate::sexp::constructors::mk_char_str("b");
            let v = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            let _v_guard = crate::sexp::protect::protect(v);
            SET_STRING_ELT(v, 0, s1);
            SET_STRING_ELT(v, 1, s2);
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(d), 2);
            assert_eq!(STRING_ELT(d, 0), s1); // CHARSXP is shared, not copied
            assert_eq!(STRING_ELT(d, 1), s2);
        }
    }

    #[test]
    fn test_duplicate_vecsxp() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let elem1 = Rf_ScalarInteger(10);
            let elem2 = Rf_ScalarInteger(20);
            let v = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(v, 0, elem1);
            SET_VECTOR_ELT(v, 1, elem2);
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(d), 2);
        }
    }

    #[test]
    fn test_duplicate_extptrsxp() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let extptr = crate::sexp::memory_ext::allocSExp(SEXPTYPE::EXTPTRSXP);
            let d = duplicate(extptr);
            // GNU duplicate.c returns external pointers unchanged (shared
            // external identity, one collectible graph).
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::EXTPTRSXP);
            assert_eq!(d, extptr);
        }
    }
}

#[cfg(test)]
#[path = "duplicate/owned_graph_tests.rs"]
mod owned_graph_tests;

#[cfg(test)]
#[path = "duplicate/owned_depth_tests.rs"]
mod owned_depth_tests;
