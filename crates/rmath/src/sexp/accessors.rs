#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Translated evaluator projections over owned headers and saved graph IDs.
//!
//! Header reads copy checked cells; mutations replace a validated generation.
//! Graph projections resolve each original link in its parent's heap domain.

use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use super::ffi::{
    EdgeField, NA_INTEGER, NA_REAL, NodeBody, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, SexprecCore,
};

#[inline]
fn is_valid_sexp_ptr(x: SEXP) -> bool {
    let addr = x as usize;
    if addr < 0x1000 {
        return false;
    }
    (addr & (std::mem::align_of::<SexprecCore>() - 1)) == 0
}

/// A raw header read recognizes only an owned allocation or an actual
/// immutable lease. No alignment check grants dereference authority.
fn immutable_lease(pointer: SEXP) -> Option<super::globals::SingletonLease> {
    super::globals::immutable_singleton_lease(pointer).or_else(|| {
        let owner = super::instance::current_instance_ptr()?;
        // SAFETY: the installed owner is live at this raw runtime boundary.
        // Copy only its immutable heap identity; no field loan or callback escapes.
        let heap = unsafe { (&*ptr::addr_of!((*owner).heap_identity)).clone() };
        heap.retained_singleton(pointer)
    })
}

fn header_snapshot(pointer: SEXP) -> Option<SexprecCore> {
    if !is_valid_sexp_ptr(pointer) {
        return None;
    }
    if let Some(singleton) = immutable_lease(pointer) {
        return Some(singleton.snapshot());
    }
    let (projection, node) = super::memory::checked_projection(pointer)
        .unwrap_or_else(|| super::context::r_error("unowned header read"));
    Some(
        super::memory::checked_snapshot(projection, &node)
            .unwrap_or_else(|| super::context::r_error("stale header read")),
    )
}

fn mutate_header(pointer: SEXP, update: impl FnOnce(&mut SexprecCore)) {
    if pointer.is_null() || immutable_lease(pointer).is_some() {
        return;
    }
    let (projection, node) = super::memory::checked_projection(pointer)
        .unwrap_or_else(|| super::context::r_error("unowned header write"));
    let mut header = super::memory::checked_snapshot(projection, &node)
        .unwrap_or_else(|| super::context::r_error("stale header write"));
    let original_kind = header.sxpinfo.type_of();
    update(&mut header);
    if header.sxpinfo.type_of() != original_kind {
        super::context::r_error("header mutation cannot change kind");
    }
    node.heap_identity()
        .replace_node(&node, header)
        .unwrap_or_else(|| super::context::r_error("stale header write"));
}

#[inline]
fn debug_assert_sexptype(x: SEXP, expected: &[SEXPTYPE]) {
    debug_assert!(is_valid_sexp_ptr(x), "invalid SEXP pointer");
    debug_assert!(
        expected.contains(
            &header_snapshot(x)
                .expect("checked debug header")
                .sxpinfo
                .type_of()
        ),
        "SEXPTYPE mismatch: expected one of {:?}, got {:?}",
        expected,
        header_snapshot(x)
            .expect("checked debug header")
            .sxpinfo
            .type_of()
    );
}

#[inline]
fn debug_assert_list_like(x: SEXP) {
    debug_assert_sexptype(
        x,
        &[
            SEXPTYPE::LISTSXP,
            SEXPTYPE::LANGSXP,
            SEXPTYPE::DOTSXP,
            SEXPTYPE::NILSXP,
        ],
    );
}

#[inline]
fn debug_assert_vector_type(x: SEXP, expected: SEXPTYPE) {
    debug_assert_sexptype(x, &[expected, SEXPTYPE::LGLSXP, SEXPTYPE::INTSXP]);
}

// ---------------------------------------------------------------------------
// Header accessors
// ---------------------------------------------------------------------------

/// Get the SEXPTYPE tag of an SEXP.
pub unsafe fn TYPEOF(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| header.sxpinfo.type_of().0)
}

/// Change the kind only when the existing canonical body and payload support it.
/// Immutable sentinels and NULL retain their original kind.
///
/// # Safety
/// The projection must refer to the active runtime or a retained live allocation.
/// No payload loan may cross a successful kind change.
pub unsafe fn SET_TYPEOF(x: SEXP, kind: c_int) {
    if x.is_null() || immutable_lease(x).is_some() {
        return;
    }
    let (_, node) = super::memory::checked_projection(x)
        .unwrap_or_else(|| super::context::r_error("unowned kind write"));
    node.heap_identity()
        .retype_node(&node, SEXPTYPE(kind))
        .unwrap_or_else(|| super::context::r_error("incompatible or stale kind write"));
}

/// Get the length of a vector SEXP.
pub unsafe fn LENGTH(x: SEXP) -> c_int {
    unsafe {
        if !is_valid_sexp_ptr(x) {
            return 0;
        }
        let length = XLENGTH(x);
        c_int::try_from(length).unwrap_or_else(|_| {
            std::panic::panic_any(super::context::RError {
                message: "long vectors not supported by LENGTH".into(),
            })
        })
    }
}

/// Get the extended length of a vector SEXP (64-bit).
pub unsafe fn XLENGTH(x: SEXP) -> R_xlen_t {
    unsafe {
        let Some(header) = header_snapshot(x) else {
            return 0;
        };
        match header.sxpinfo.type_of() {
            SEXPTYPE::NILSXP => 0,
            SEXPTYPE::CHARSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::EXPRSXP
            | SEXPTYPE::RAWSXP => header.vecsxp_length(),
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP => {
                let mut n: R_xlen_t = 0;
                let mut p = x;
                while !p.is_null() && TYPEOF(p) != SEXPTYPE::NILSXP {
                    n += 1;
                    p = CDR(p);
                }
                n
            }
            _ => 1,
        }
    }
}

/// Get the true length (allocated capacity) of a vector SEXP.
pub unsafe fn TRUELENGTH(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| header.vecsxp_truelength() as c_int)
}

/// Set the true length of a vector SEXP.
pub unsafe fn SET_TRUELENGTH(x: SEXP, v: c_int) {
    mutate_header(x, |header| header.set_vecsxp_truelength(v as R_xlen_t));
}

/// Resolve a saved edge using the original parent's allocation domain.
/// Raw projections stop at this boundary; canonical headers contain identities.
fn graph_edge(pointer: SEXP, field: EdgeField) -> SEXP {
    if !is_valid_sexp_ptr(pointer) {
        return ptr::null_mut();
    }
    if immutable_lease(pointer).is_some() {
        return ptr::null_mut();
    }
    let (_, node) = super::memory::checked_projection(pointer)
        .unwrap_or_else(|| super::context::r_error("unowned graph parent"));
    let heap = node.heap_identity();
    let link = heap
        .edge(&node, field)
        .unwrap_or_else(|| super::context::r_error("invalid graph field"));
    heap.projection_of_link(link)
        .unwrap_or_else(|| super::context::r_error("stale or foreign graph child"))
}

fn graph_set_edge(pointer: SEXP, field: EdgeField, child: SEXP) {
    if pointer.is_null() || immutable_lease(pointer).is_some() {
        return;
    }
    let (pointer, node) = super::memory::checked_projection(pointer)
        .unwrap_or_else(|| super::context::r_error("unowned graph parent"));
    let value = ReferenceValue::capture_in(&node, child);
    node.heap_identity()
        .set_edge(&node, field, value.capability())
        .unwrap_or_else(|| super::context::r_error("invalid graph edge"));
    super::gengc::write_barrier(pointer, child);
}

/// Get the attributes of an SEXP.
pub unsafe fn ATTRIB(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::Attribute)
}

/// Set the attributes of an SEXP.
pub unsafe fn SET_ATTRIB(x: SEXP, v: SEXP) {
    if x.is_null() {
        return;
    }
    if super::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    let (x, node) = super::memory::checked_projection(x)
        .unwrap_or_else(|| super::context::r_error("unowned attribute parent"));
    let _root = node
        .root_lease()
        .unwrap_or_else(|| super::context::r_error("attribute parent is unavailable"));
    let _value = ReferenceValue::capture_in(&node, v);
    unsafe {
        if is_valid_sexp_ptr(x) {
            // Materialize before replacing the list. The formula cell is what
            // keeps a compact sequence's values alive, and `materialize`
            // clears the ALT bit before it writes the attribute slot. If the
            // buffer was not committed, the formula stays at the head.
            if ALTREP(x) != 0 {
                if super::altrep::materialize_raw(x)
                    .unwrap_or_else(|e| super::context::r_error(e.to_string()))
                {
                    // The buffer now owns the values, so metadata can be removed.
                    mutate_header(x, |header| header.sxpinfo.set_alt(false));
                }
                super::altseq::materialize(x);
            }
            let uncommitted = ALTREP(x) != 0
                && ((*x).gengc_next_node.is_null() || super::memory::vector_payload_is_pending(x));
            if uncommitted {
                super::altseq::keep_formula_replace_tail(x, v);
                return;
            }
            let v = super::altseq::without_formula_cells(v);
            graph_set_edge(x, EdgeField::Attribute, v);
        }
    }
}

/// Check if an SEXP has the OBJECT flag set.
pub unsafe fn OBJECT(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| header.sxpinfo.obj() as c_int)
}

/// Set the OBJECT flag on an SEXP.
pub unsafe fn SET_OBJECT(x: SEXP, v: c_int) {
    mutate_header(x, |header| header.sxpinfo.set_obj(v != 0));
}

/// S4 object bit (gp bit 4), matching Rinternals.h `SET_S4_OBJECT`/`IS_S4_OBJECT`.
pub const S4_OBJECT_MASK: u16 = 1 << 4;

/// Set the S4 object bit (gp bit 4).
pub unsafe fn SET_S4_OBJECT(x: SEXP) {
    mutate_header(x, |header| {
        header.sxpinfo.set_gp(header.sxpinfo.gp() | S4_OBJECT_MASK)
    });
}

/// Unset the S4 object bit (gp bit 4).
pub unsafe fn UNSET_S4_OBJECT(x: SEXP) {
    mutate_header(x, |header| {
        header.sxpinfo.set_gp(header.sxpinfo.gp() & !S4_OBJECT_MASK)
    });
}

/// Get the namedness level (0, 1, or 2).
pub unsafe fn NAMED(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| header.sxpinfo.named() as c_int)
}

/// Set the namedness level.
pub unsafe fn SET_NAMED(x: SEXP, v: c_int) {
    mutate_header(x, |header| header.sxpinfo.set_named(v as u8));
}

/// Get the LEVELS (gp[0..1]) field.
pub unsafe fn LEVELS(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| (header.sxpinfo.gp() & 0x03) as c_int)
}

/// Set the LEVELS field.
pub unsafe fn SETLEVELS(x: SEXP, v: c_int) {
    mutate_header(x, |header| {
        header
            .sxpinfo
            .set_gp((header.sxpinfo.gp() & !0x03) | ((v as u16) & 0x03))
    });
}

/// GNU `MISSING(x)` — gp bit 2 on a pairlist binding cell.
pub unsafe fn MISSING(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| ((header.sxpinfo.gp() & 0x04) != 0) as c_int)
}

/// GNU `SET_MISSING(x, v)` — mark an unmatched formal on its frame cell.
pub unsafe fn SET_MISSING(x: SEXP, v: c_int) {
    mutate_header(x, |header| {
        header.sxpinfo.set_gp(if v != 0 {
            header.sxpinfo.gp() | 0x04
        } else {
            header.sxpinfo.gp() & !0x04
        })
    });
}

/// Get the scalar flag.
pub unsafe fn IS_SCALAR(x: SEXP, _type: c_int) -> c_int {
    header_snapshot(x).map_or(0, |header| header.sxpinfo.scalar() as c_int)
}

/// Set the scalar flag.
pub unsafe fn SET_SCALAR(x: SEXP, v: c_int) {
    mutate_header(x, |header| header.sxpinfo.set_scalar(v != 0));
}

/// Check the ALT flag.
pub unsafe fn ALTREP(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| header.sxpinfo.alt() as c_int)
}

/// Set the ALT flag.
pub unsafe fn SET_ALTREP(x: SEXP, v: c_int) {
    mutate_header(x, |header| header.sxpinfo.set_alt(v != 0));
}

/// Get the mark bit (for GC).
pub unsafe fn MARK(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| header.sxpinfo.mark() as c_int)
}

/// Set the mark bit.
pub unsafe fn SET_MARK(x: SEXP, v: c_int) {
    mutate_header(x, |header| header.sxpinfo.set_mark(v != 0));
}

/// Get the type (same as TYPEOF but as a macro name).
pub unsafe fn Rf_isNull(x: SEXP) -> c_int {
    unsafe { (TYPEOF(x) == SEXPTYPE::NILSXP) as c_int }
}

/// Check the type of a SEXP. Alias for TYPEOF.
pub unsafe fn TYPEOF_CHECK(x: SEXP) -> c_int {
    unsafe { TYPEOF(x) }
}

// ---------------------------------------------------------------------------
// List/cons cell accessors
// ---------------------------------------------------------------------------

/// Get the CAR of a cons cell.
pub unsafe fn CAR(x: SEXP) -> SEXP {
    if !is_valid_sexp_ptr(x) {
        return ptr::null_mut();
    }
    // SAFETY: raw callers retain a live header. Nil has no graph body.
    if unsafe { TYPEOF(x) } == SEXPTYPE::NILSXP {
        return super::globals::immutable_singleton_projection(x).unwrap_or(x);
    }
    graph_edge(x, EdgeField::ListCar)
}

/// Get the CDR of a cons cell.
pub unsafe fn CDR(x: SEXP) -> SEXP {
    if !is_valid_sexp_ptr(x) {
        return ptr::null_mut();
    }
    // SAFETY: raw callers retain a live header. Nil has no graph body.
    if unsafe { TYPEOF(x) } == SEXPTYPE::NILSXP {
        return super::globals::immutable_singleton_projection(x).unwrap_or(x);
    }
    graph_edge(x, EdgeField::ListCdr)
}

/// Get the TAG of a cons cell.
pub unsafe fn TAG(x: SEXP) -> SEXP {
    if !is_valid_sexp_ptr(x) {
        return ptr::null_mut();
    }
    // SAFETY: raw callers retain a live header. Nil has no graph body.
    if unsafe { TYPEOF(x) } == SEXPTYPE::NILSXP {
        return super::globals::immutable_singleton_projection(x).unwrap_or(x);
    }
    graph_edge(x, EdgeField::ListTag)
}

/// Set the CAR of a cons cell.
pub unsafe fn SETCAR(x: SEXP, y: SEXP) {
    graph_set_edge(x, EdgeField::ListCar, y);
}

/// Set the CDR of a cons cell.
pub unsafe fn SETCDR(x: SEXP, y: SEXP) {
    graph_set_edge(x, EdgeField::ListCdr, y);
}

/// Set the TAG of a cons cell.
pub unsafe fn SETTAG(x: SEXP, y: SEXP) {
    graph_set_edge(x, EdgeField::ListTag, y);
}

/// Get the CAR of the CDR (CADR) — second element of a list.
pub unsafe fn CADR(x: SEXP) -> SEXP {
    unsafe { CAR(CDR(x)) }
}

/// Get the CAR of the CDAR (CAAR).
pub unsafe fn CAAR(x: SEXP) -> SEXP {
    unsafe { CAR(CAR(x)) }
}

/// Get the CDR of the CDR (CDDR).
pub unsafe fn CDDR(x: SEXP) -> SEXP {
    unsafe { CDR(CDR(x)) }
}

/// Get the CDR of the CADR (CDAR).
pub unsafe fn CDAR(x: SEXP) -> SEXP {
    unsafe { CDR(CAR(x)) }
}

/// Get the CAR of the CDDR (CADDR).
pub unsafe fn CADDR(x: SEXP) -> SEXP {
    unsafe { CAR(CDR(CDR(x))) }
}

/// Get the CDR of the CDDR (CDDDR).
pub unsafe fn CDDDR(x: SEXP) -> SEXP {
    unsafe { CDR(CDR(CDR(x))) }
}

/// Get the CAR of the CDDDR (CADDDR).
pub unsafe fn CADDDR(x: SEXP) -> SEXP {
    unsafe { CAR(CDR(CDR(CDR(x)))) }
}

/// Get the CAR of the CADDDR (CAD5R).
pub unsafe fn CAD5R(x: SEXP) -> SEXP {
    unsafe { CAR(CDR(CDR(CDR(CDR(x))))) }
}

// ---------------------------------------------------------------------------
// Symbol accessors
// ---------------------------------------------------------------------------

/// Get the print name (CHARSXP) of a symbol.
pub unsafe fn PRINTNAME(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::SymbolName)
}

/// Get the value of a symbol.
pub unsafe fn SYMVALUE(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::SymbolValue)
}

/// Get the internal value of a symbol.
pub unsafe fn INTERNAL(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::SymbolInternal)
}

/// Set the print name of a symbol.
pub unsafe fn SET_PRINTNAME(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::SymbolName, v);
}

/// Set the value of a symbol.
pub unsafe fn SET_SYMVALUE(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::SymbolValue, v);
}

/// Set the internal value of a symbol.
pub unsafe fn SET_INTERNAL(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::SymbolInternal, v);
}

// ---------------------------------------------------------------------------
// Closure accessors
// ---------------------------------------------------------------------------

/// Get the formals of a closure.
pub unsafe fn FORMALS(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::ClosureFormals)
}

/// Get the body of a closure.
pub unsafe fn BODY(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::ClosureBody)
}

/// Get the environment of a closure.
pub unsafe fn CLOENV(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::ClosureEnvironment)
}

/// Set the formals of a closure.
pub unsafe fn SET_FORMALS(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::ClosureFormals, v);
}

/// Set the body of a closure.
pub unsafe fn SET_BODY(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::ClosureBody, v);
}

/// Set the environment of a closure.
pub unsafe fn SET_CLOENV(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::ClosureEnvironment, v);
}

// ---------------------------------------------------------------------------
// Environment accessors
// ---------------------------------------------------------------------------

/// Get the frame of an environment.
pub unsafe fn FRAME(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::EnvironmentFrame)
}

/// Get the enclosing environment.
pub unsafe fn ENCLOS(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::EnvironmentEnclosure)
}

/// Get the hash table of an environment.
pub unsafe fn HASHTAB(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::EnvironmentHashTable)
}

/// Set the frame of an environment.
pub unsafe fn SET_FRAME(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::EnvironmentFrame, v);
}

/// Set the enclosing environment.
pub unsafe fn SET_ENCLOS(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::EnvironmentEnclosure, v);
}

/// Set the hash table of an environment.
pub unsafe fn SET_HASHTAB(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::EnvironmentHashTable, v);
}

// ---------------------------------------------------------------------------
// Promise accessors
// ---------------------------------------------------------------------------

/// Get the value of a promise.
pub unsafe fn PRVALUE(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::PromiseValue)
}

/// Get the expression of a promise.
pub unsafe fn PRCODE(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::PromiseExpression)
}

/// Get the environment of a promise.
pub unsafe fn PRENV(x: SEXP) -> SEXP {
    graph_edge(x, EdgeField::PromiseEnvironment)
}

/// Set the value of a promise.
pub unsafe fn SET_PRVALUE(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::PromiseValue, v);
}

/// Set the expression of a promise.
pub unsafe fn SET_PRCODE(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::PromiseExpression, v);
}

/// Set the environment of a promise.
pub unsafe fn SET_PRENV(x: SEXP, v: SEXP) {
    graph_set_edge(x, EdgeField::PromiseEnvironment, v);
}

// ---------------------------------------------------------------------------
// Primitive function accessors
// ---------------------------------------------------------------------------

/// Get the offset of a primitive function.
pub unsafe fn PRIMOFFSET(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| header.data.primitive().offset)
}

/// Set the offset of a primitive function.
pub unsafe fn SET_PRIMOFFSET(x: SEXP, v: c_int) {
    mutate_header(x, |header| header.data.primitive_mut().offset = v);
}

// ---------------------------------------------------------------------------
// Vector data accessors
// ---------------------------------------------------------------------------

/// Get a pointer to the data region of a vector SEXP.
///
/// For vector types, the data is stored in a separate allocation
/// tracked by the arena allocator. The data pointer is stored in
/// the gengc_next_node field for vector types.
///
/// # Safety
/// `x` must be live and belong to the active instance. No Rust payload borrow
/// may overlap materialization. Nonempty compact expansion returns usable
/// storage or raises `RError`; checked owner-bound access uses `Sexp` instead.
pub unsafe fn DATAPTR(x: SEXP) -> *mut c_void {
    unsafe {
        if !is_valid_sexp_ptr(x) {
            return ptr::null_mut();
        }
        // For vector types, data pointer is stored in gengc_next_node.
        // A compact sequence keeps that pointer null until the first request.
        let t = header_snapshot(x)
            .expect("checked vector header")
            .sxpinfo
            .type_of();
        if t.is_vector_type() || t == SEXPTYPE::CHARSXP {
            if ALTREP(x) != 0 && (*x).gengc_next_node.is_null() {
                let extension = super::altrep::materialize_raw(x)
                    .unwrap_or_else(|e| super::context::r_error(e.to_string()));
                if !extension {
                    super::altseq::materialize(x);
                }
                // Ported callers expect usable storage or an R error. A
                // nonempty lazy vector must never yield a null data pointer.
                if (*x).vecsxp_length() != 0 && (*x).gengc_next_node.is_null() {
                    super::context::r_error(
                        "cannot materialize compact vector: invalid size, memory budget or allocation failure",
                    );
                }
            }
            (*x).gengc_next_node as *mut c_void
        } else {
            ptr::null_mut()
        }
    }
}

/// Set the data pointer for a vector SEXP.
pub unsafe fn SET_DATAPTR(x: SEXP, v: *mut c_void) {
    if super::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if is_valid_sexp_ptr(x) {
            (*x).gengc_next_node = v as SEXP;
        }
    }
}

/// Get the data pointer, returning a const pointer.
pub unsafe fn ROBJ_DATAPTR(x: SEXP) -> *const c_void {
    unsafe { DATAPTR(x) }
}

/// Get a pointer to the logical vector data.
pub unsafe fn LOGICAL(x: SEXP) -> *mut c_int {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::LGLSXP]);
        DATAPTR(x) as *mut c_int
    }
}

/// Get a pointer to the integer-compatible vector data.
///
/// R stores logical vectors as `c_int` too, and translated C code sometimes
/// uses INTEGER on LGLSXP when it wants the raw storage representation.
pub unsafe fn INTEGER(x: SEXP) -> *mut c_int {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::INTSXP, SEXPTYPE::LGLSXP]);
        DATAPTR(x) as *mut c_int
    }
}

/// Get a pointer to the real (double) vector data.
pub unsafe fn REAL(x: SEXP) -> *mut c_double {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::REALSXP]);
        DATAPTR(x) as *mut c_double
    }
}

/// Get a pointer to the complex vector data.
pub unsafe fn COMPLEX(x: SEXP) -> *mut Rcomplex {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::CPLXSXP]);
        DATAPTR(x) as *mut Rcomplex
    }
}

/// Get a pointer to the raw byte vector data.
pub unsafe fn RAW(x: SEXP) -> *mut super::ffi::Rbyte {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::RAWSXP]);
        DATAPTR(x) as *mut super::ffi::Rbyte
    }
}

/// Get the character data of a CHARSXP.
pub unsafe fn CHAR(x: SEXP) -> *const c_char {
    unsafe { DATAPTR(x) as *const c_char }
}

/// Get a mutable pointer to the character data of a CHARSXP.
pub unsafe fn CHAR_RW(x: SEXP) -> *mut c_char {
    unsafe { DATAPTR(x) as *mut c_char }
}

// ---------------------------------------------------------------------------
// String/list element accessors
// ---------------------------------------------------------------------------

/// Why [`element_slot_decision`] refused a string or list element access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementSlotRejectKind {
    BadTag,
    BadIndex,
}

/// Tag or index/buffer failure from [`element_slot_decision`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ElementSlotReject {
    pub kind: ElementSlotRejectKind,
}

/// Pure accept/reject decision shared by [`checked_reference_slot`].
///
/// `Ok(())` means the tag is legal for `string_only`, the index is in range,
/// and the data pointer is non-null. A bad tag wins when the index is also
/// illegal, matching the historical check order. This does not prove that
/// `length` describes the real allocation.
pub(crate) fn element_slot_decision(
    tag: SEXPTYPE,
    string_only: bool,
    length: R_xlen_t,
    index: R_xlen_t,
    data_is_null: bool,
) -> Result<(), ElementSlotReject> {
    let valid_tag = if string_only {
        tag == SEXPTYPE::STRSXP
    } else {
        matches!(
            tag,
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::STRSXP | SEXPTYPE::BCODESXP
        )
    };
    if !valid_tag {
        return Err(ElementSlotReject {
            kind: ElementSlotRejectKind::BadTag,
        });
    }
    if index < 0 || index >= length || data_is_null {
        return Err(ElementSlotReject {
            kind: ElementSlotRejectKind::BadIndex,
        });
    }
    Ok(())
}

/// An exact live allocation retained through materialization and element access.
struct ReferenceSlot {
    pointer: SEXP,
    node: super::heap::CheckedNode,
    index: usize,
    _root: std::rc::Rc<super::heap::NodeRootLease>,
}

fn checked_reference_slot(x: SEXP, i: R_xlen_t, string_only: bool) -> ReferenceSlot {
    let (pointer, node) = super::memory::checked_projection(x)
        .unwrap_or_else(|| super::context::r_error("unowned reference-vector allocation"));
    let root = node
        .root_lease()
        .unwrap_or_else(|| super::context::r_error("reference-vector allocation is unavailable"));
    let header = super::memory::checked_snapshot(pointer, &node)
        .unwrap_or_else(|| super::context::r_error("stale reference-vector allocation"));
    let length = match header.data {
        NodeBody::Vector(vector) => vector.length,
        _ => 0,
    };
    // A lazy element has no dense buffer yet. Bounds and kind are checked
    // here; the actual typed allocation is checked after materialization.
    if let Err(reject) =
        element_slot_decision(header.sxpinfo.type_of(), string_only, length, i, false)
    {
        match reject.kind {
            ElementSlotRejectKind::BadTag => super::context::r_error(format!(
                "invalid {} element access for type {}",
                if string_only { "string" } else { "vector" },
                header.sxpinfo.type_of().0
            )),
            ElementSlotRejectKind::BadIndex => {
                super::context::r_error(format!("invalid vector index: length {length} index {i}"))
            }
        }
    }
    ReferenceSlot {
        pointer,
        node,
        index: i as usize,
        _root: root,
    }
}

impl ReferenceSlot {
    fn read(&self) -> SEXP {
        self.node
            .heap_identity()
            .reference_elt(&self.node, self.index)
            .unwrap_or_else(|| super::context::r_error("invalid typed reference-vector buffer"))
    }
}

/// Own both inputs before any provider callback. No supplied pointer is read.
enum ReferenceValue {
    Null,
    Node(
        super::heap::CheckedNode,
        std::rc::Rc<super::heap::NodeRootLease>,
    ),
    Singleton(super::globals::SingletonLease),
}
impl ReferenceValue {
    fn capture(slot: &ReferenceSlot, pointer: SEXP) -> Self {
        if pointer.is_null() {
            return Self::Null;
        }
        Self::capture_in(&slot.node, pointer)
    }

    fn capture_in(parent: &super::heap::CheckedNode, pointer: SEXP) -> Self {
        if pointer.is_null() {
            return Self::Null;
        }
        let heap = parent.heap_identity();
        if let Some(singleton) = heap
            .retained_singleton(pointer)
            .or_else(|| super::globals::immutable_singleton_lease(pointer))
        {
            return Self::Singleton(singleton);
        }
        let (_, node) = super::memory::checked_projection(pointer)
            .filter(|(_, node)| parent.same_heap(node))
            .unwrap_or_else(|| super::context::r_error("unowned reference-vector child"));
        let root = node
            .root_lease()
            .unwrap_or_else(|| super::context::r_error("reference-vector child is unavailable"));
        Self::Node(node, root)
    }

    fn capability(&self) -> super::heap::ReferenceChild<'_> {
        match self {
            Self::Null => super::heap::ReferenceChild::Null,
            Self::Node(node, _root) => super::heap::ReferenceChild::Node(node),
            Self::Singleton(singleton) => super::heap::ReferenceChild::Singleton(singleton),
        }
    }
}

#[cfg(test)]
mod element_slot_decision_tests {
    use super::{ElementSlotRejectKind, element_slot_decision};
    use crate::sexp::ffi::SEXPTYPE;

    #[test]
    fn extremes_follow_the_index_rule() {
        assert!(
            element_slot_decision(SEXPTYPE::STRSXP, true, i64::MAX, i64::MAX - 1, false).is_ok()
        );
        let low =
            element_slot_decision(SEXPTYPE::STRSXP, true, i64::MAX, i64::MIN, false).unwrap_err();
        assert_eq!(low.kind, ElementSlotRejectKind::BadIndex);
        let empty = element_slot_decision(SEXPTYPE::STRSXP, true, i64::MIN, 0, false).unwrap_err();
        assert_eq!(empty.kind, ElementSlotRejectKind::BadIndex);
    }
}

#[cfg(kani)]
mod element_slot_kani {
    use super::{ElementSlotRejectKind, element_slot_decision};
    use crate::sexp::ffi::SEXPTYPE;

    #[kani::proof]
    fn element_slot_decision_complete() {
        let raw: i32 = kani::any();
        kani::assume((0..=32).contains(&raw));
        let tag = SEXPTYPE(raw);
        let string_only: bool = kani::any();
        let length: i64 = kani::any();
        let index: i64 = kani::any();
        kani::assume((-2..=4).contains(&length) && (-2..=4).contains(&index));
        let data_is_null: bool = kani::any();
        let decision = element_slot_decision(tag, string_only, length, index, data_is_null);
        let valid_tag = if string_only {
            tag == SEXPTYPE::STRSXP
        } else {
            matches!(
                tag,
                SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::STRSXP | SEXPTYPE::BCODESXP
            )
        };
        match decision {
            Ok(()) => {
                assert!(valid_tag);
                assert!(index >= 0 && index < length && !data_is_null);
            }
            Err(reject) if !valid_tag => {
                assert_eq!(reject.kind, ElementSlotRejectKind::BadTag);
            }
            Err(reject) => {
                assert_eq!(reject.kind, ElementSlotRejectKind::BadIndex);
                assert!(index < 0 || index >= length || data_is_null);
            }
        }
        kani::cover(
            decision.is_ok() && tag == SEXPTYPE::STRSXP && string_only,
            "reachable",
        );
        kani::cover(
            decision.is_ok() && tag == SEXPTYPE::VECSXP && !string_only,
            "reachable",
        );
        kani::cover(
            decision.is_ok() && tag == SEXPTYPE::BCODESXP && !string_only,
            "reachable",
        );
        kani::cover(!string_only && tag == SEXPTYPE::BCODESXP, "reachable");
        kani::cover(
            string_only && tag == SEXPTYPE::BCODESXP && decision.is_err(),
            "reachable",
        );
        kani::cover(index == -1 && decision.is_err(), "reachable");
        kani::cover(data_is_null && decision.is_err(), "reachable");
        kani::cover(tag == SEXPTYPE::INTSXP && decision.is_err(), "reachable");
    }
}

/// Get the i-th element from checked canonical string storage.
pub unsafe fn STRING_ELT(x: SEXP, i: R_xlen_t) -> SEXP {
    let slot = checked_reference_slot(x, i, true);
    if let Some(value) = unsafe { super::altrep::lazy_raw(slot.pointer, i) } {
        if let super::altrep::AltrepElement::String(value) =
            value.unwrap_or_else(|e| super::context::r_error(e.to_string()))
        {
            return value.as_raw();
        }
        super::context::r_error("ALTREP element type mismatch");
    }
    // SAFETY: the exact parent lease retains the node through materialization.
    let _ = unsafe { DATAPTR(slot.pointer) };
    slot.read()
}

/// Store a string child through typed cells and the collector barrier.
pub unsafe fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    let slot = checked_reference_slot(x, i, true);
    let value = ReferenceValue::capture(&slot, val);
    let _ = unsafe { DATAPTR(slot.pointer) };
    let _ = slot.read();
    unsafe { super::gengc::vector_write_barrier(slot.pointer, slot.index, val) };
    slot.node
        .heap_identity()
        .set_reference_elt(&slot.node, slot.index, value.capability())
        .unwrap_or_else(|| super::context::r_error("invalid typed reference-vector write"));
}

/// Get a checked reference-array element, including bytecode constants.
pub unsafe fn VECTOR_ELT(x: SEXP, i: R_xlen_t) -> SEXP {
    let slot = checked_reference_slot(x, i, false);
    if let Some(value) = unsafe { super::altrep::lazy_raw(slot.pointer, i) } {
        if let super::altrep::AltrepElement::List(value) =
            value.unwrap_or_else(|e| super::context::r_error(e.to_string()))
        {
            return value.as_raw();
        }
        super::context::r_error("ALTREP element type mismatch");
    }
    let _ = unsafe { DATAPTR(slot.pointer) };
    slot.read()
}

/// Store a checked reference-array child and record old-to-young edges.
pub unsafe fn SET_VECTOR_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    let slot = checked_reference_slot(x, i, false);
    let value = ReferenceValue::capture(&slot, val);
    let _ = unsafe { DATAPTR(slot.pointer) };
    let _ = slot.read();
    unsafe { super::gengc::vector_write_barrier(slot.pointer, slot.index, val) };
    slot.node
        .heap_identity()
        .set_reference_elt(&slot.node, slot.index, value.capability())
        .unwrap_or_else(|| super::context::r_error("invalid typed reference-vector write"));
}

// ---------------------------------------------------------------------------
// Element-level accessors
// ---------------------------------------------------------------------------

/// Get the i-th logical value.
pub unsafe fn LOGICAL_ELT(x: SEXP, i: c_int) -> c_int {
    unsafe {
        if let Some(value) = super::altrep::lazy_raw(x, i as R_xlen_t) {
            if let super::altrep::AltrepElement::Logical(v) =
                value.unwrap_or_else(|e| super::context::r_error(e.to_string()))
            {
                return v;
            }
            super::context::r_error("ALTREP element type mismatch");
        }
        if !is_valid_sexp_ptr(x) {
            return NA_INTEGER;
        }
        let data = LOGICAL(x);
        if data.is_null() || (data as usize) % std::mem::align_of::<c_int>() != 0 {
            return NA_INTEGER;
        }
        debug_assert_sexptype(x, &[SEXPTYPE::LGLSXP]);
        *data.add(i as usize)
    }
}

/// Set the i-th logical value.
pub unsafe fn SET_LOGICAL_ELT(x: SEXP, i: c_int, v: c_int) {
    if super::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if is_valid_sexp_ptr(x) && !LOGICAL(x).is_null() {
            *LOGICAL(x).add(i as usize) = v;
        }
    }
}

/// Get the i-th integer value.
/// GNU `INTEGER_ELT` shares storage with `LGLSXP`.
///
/// This helper does not check the index, and the type test is a
/// `debug_assert` in release builds. Callers must already hold a legal
/// `INTSXP` or `LGLSXP` index. Checked list and string access goes through
/// `checked_element_slot`. The safe object path is `Sexp::try_integer_elt`.
pub unsafe fn INTEGER_ELT(x: SEXP, i: c_int) -> c_int {
    unsafe {
        if let Some(value) = super::altrep::lazy_raw(x, i as R_xlen_t) {
            if let super::altrep::AltrepElement::Integer(v)
            | super::altrep::AltrepElement::Logical(v) =
                value.unwrap_or_else(|e| super::context::r_error(e.to_string()))
            {
                return v;
            }
            super::context::r_error("ALTREP element type mismatch");
        }
        if !is_valid_sexp_ptr(x) {
            return NA_INTEGER;
        }
        // Resolve a compact sequence before `INTEGER`, which materializes.
        // One handle at this FFI edge; the formula walk itself is safe.
        if let Some(sx) = super::object::Sexp::from_raw(x) {
            match sx.read_compact_int(i as R_xlen_t, true) {
                super::altseq::LazyRead::Ready(value) => return value,
                super::altseq::LazyRead::OutOfRange => return NA_INTEGER,
                super::altseq::LazyRead::Absent => {}
            }
        }
        let data = INTEGER(x);
        if data.is_null() || (data as usize) % std::mem::align_of::<c_int>() != 0 {
            return NA_INTEGER;
        }
        debug_assert_sexptype(x, &[SEXPTYPE::INTSXP, SEXPTYPE::LGLSXP]);
        *data.add(i as usize)
    }
}

/// Set the i-th integer value.
pub unsafe fn SET_INTEGER_ELT(x: SEXP, i: c_int, v: c_int) {
    if super::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if is_valid_sexp_ptr(x) && !INTEGER(x).is_null() {
            *INTEGER(x).add(i as usize) = v;
        }
    }
}
pub unsafe fn REAL_ELT(x: SEXP, i: c_int) -> c_double {
    unsafe {
        if let Some(value) = super::altrep::lazy_raw(x, i as R_xlen_t) {
            if let super::altrep::AltrepElement::Real(v) =
                value.unwrap_or_else(|e| super::context::r_error(e.to_string()))
            {
                return v;
            }
            super::context::r_error("ALTREP element type mismatch");
        }
        if !is_valid_sexp_ptr(x) {
            return NA_REAL;
        }
        if let Some(sx) = super::object::Sexp::from_raw(x) {
            match sx.read_compact_real(i as R_xlen_t, true) {
                super::altseq::LazyRead::Ready(value) => return value,
                super::altseq::LazyRead::OutOfRange => return NA_REAL,
                super::altseq::LazyRead::Absent => {}
            }
        }
        let data = REAL(x);
        if data.is_null() || (data as usize) % std::mem::align_of::<c_double>() != 0 {
            return NA_REAL;
        }
        debug_assert_sexptype(x, &[SEXPTYPE::REALSXP]);
        *data.add(i as usize)
    }
}

/// Set the i-th real value.
pub unsafe fn SET_REAL_ELT(x: SEXP, i: c_int, v: c_double) {
    if super::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if is_valid_sexp_ptr(x) && !REAL(x).is_null() {
            *REAL(x).add(i as usize) = v;
        }
    }
}

/// Get the i-th complex value.
pub unsafe fn COMPLEX_ELT(x: SEXP, i: c_int) -> Rcomplex {
    unsafe {
        if let Some(value) = super::altrep::lazy_raw(x, i as R_xlen_t) {
            if let super::altrep::AltrepElement::Complex(v) =
                value.unwrap_or_else(|e| super::context::r_error(e.to_string()))
            {
                return v;
            }
            super::context::r_error("ALTREP element type mismatch");
        }
        if !is_valid_sexp_ptr(x) || COMPLEX(x).is_null() {
            return Rcomplex {
                r: NA_REAL,
                i: NA_REAL,
            };
        }
        *COMPLEX(x).add(i as usize)
    }
}

/// Set the i-th complex value.
pub unsafe fn SET_COMPLEX_ELT(x: SEXP, i: c_int, v: Rcomplex) {
    if super::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if is_valid_sexp_ptr(x) && !COMPLEX(x).is_null() {
            *COMPLEX(x).add(i as usize) = v;
        }
    }
}

/// Get the i-th raw byte value.
pub unsafe fn RAW_ELT(x: SEXP, i: c_int) -> super::ffi::Rbyte {
    unsafe {
        if let Some(value) = super::altrep::lazy_raw(x, i as R_xlen_t) {
            if let super::altrep::AltrepElement::Raw(v) =
                value.unwrap_or_else(|e| super::context::r_error(e.to_string()))
            {
                return v;
            }
            super::context::r_error("ALTREP element type mismatch");
        }
        if !is_valid_sexp_ptr(x) || RAW(x).is_null() {
            return 0;
        }
        *RAW(x).add(i as usize)
    }
}

/// Set the i-th raw byte value.
pub unsafe fn SET_RAW_ELT(x: SEXP, i: c_int, v: super::ffi::Rbyte) {
    if super::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if is_valid_sexp_ptr(x) && !RAW(x).is_null() {
            *RAW(x).add(i as usize) = v;
        }
    }
}

// ---------------------------------------------------------------------------
// Scalar getters (for length-1 vectors)
// ---------------------------------------------------------------------------

/// Get the scalar logical value.
pub unsafe fn SCALAR_LVAL(x: SEXP) -> c_int {
    unsafe { LOGICAL_ELT(x, 0) }
}

/// Get the scalar integer value.
pub unsafe fn SCALAR_IVAL(x: SEXP) -> c_int {
    unsafe { INTEGER_ELT(x, 0) }
}

/// Get the scalar real value.
pub unsafe fn SCALAR_DVAL(x: SEXP) -> c_double {
    unsafe { REAL_ELT(x, 0) }
}

// ---------------------------------------------------------------------------
// Helper methods on SexprecCore
// ---------------------------------------------------------------------------

impl SexprecCore {
    fn require_vector_header(&self) {
        if !matches!(
            self.sxpinfo.type_of(),
            SEXPTYPE::CHARSXP
                | SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::VECSXP
                | SEXPTYPE::EXPRSXP
                | SEXPTYPE::BCODESXP // This runtime stores bytecode in a vector payload.
                | SEXPTYPE::RAWSXP
        ) {
            std::panic::panic_any(super::context::RError {
                message: "internal vector header requested for non-vector".into(),
            });
        }
    }
    /// Get the length from the checked vector body.
    #[inline]
    pub fn vecsxp_length(&self) -> R_xlen_t {
        self.require_vector_header();
        self.data.vector().length
    }

    /// Get the capacity from the checked vector body.
    #[inline]
    pub fn vecsxp_truelength(&self) -> R_xlen_t {
        self.require_vector_header();
        self.data.vector().truelength
    }

    /// Set the vector true length.
    #[inline]
    pub fn set_vecsxp_truelength(&mut self, v: R_xlen_t) {
        self.require_vector_header();
        self.data.vector_mut().truelength = v;
    }

    /// Set the logical vector length without touching the element buffer.
    #[inline]
    pub fn set_vecsxp_length(&mut self, v: R_xlen_t) {
        self.require_vector_header();
        self.data.vector_mut().length = v;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::ffi::*;
    use super::*;

    #[test]
    fn checked_kind_setter_rejects_incompatible_changes_without_modifying_values() {
        let mut arena = super::super::memory::RArena::new();
        let vector = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        unsafe { SET_INTEGER_ELT(vector, 0, 42) };
        let (_, allocation) = super::super::memory::checked_projection(vector).unwrap();
        let heap = allocation.heap_identity();
        let before = heap.node_snapshot(&allocation).unwrap();
        for kind in [SEXPTYPE::REALSXP, SEXPTYPE::SYMSXP, SEXPTYPE::LISTSXP] {
            let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                SET_TYPEOF(vector, kind.0);
            }));
            assert!(rejected.is_err());
            let after = heap.node_snapshot(&allocation).unwrap();
            assert_eq!(after.sxpinfo.type_of(), before.sxpinfo.type_of());
            assert_eq!(after.data, before.data);
            assert_eq!(unsafe { INTEGER_ELT(vector, 0) }, 42);
        }
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            mutate_header(vector, |header| header.sxpinfo.set_type(SEXPTYPE::REALSXP));
        }));
        assert!(rejected.is_err());
        assert_eq!(unsafe { TYPEOF(vector) }, SEXPTYPE::INTSXP.0);
        assert_eq!(unsafe { INTEGER_ELT(vector, 0) }, 42);
    }

    #[test]
    fn checked_kind_setter_preserves_compatible_payloads_edges_and_immutable_nil() {
        let mut arena = super::super::memory::RArena::new();
        let logical = arena.alloc_vector(SEXPTYPE::LGLSXP, 2);
        unsafe {
            SET_LOGICAL_ELT(logical, 0, 1);
            SET_LOGICAL_ELT(logical, 1, NA_INTEGER);
            SET_TYPEOF(logical, SEXPTYPE::INTSXP.0);
            assert_eq!(INTEGER_ELT(logical, 0), 1);
            assert_eq!(INTEGER_ELT(logical, 1), NA_INTEGER);
            SET_TYPEOF(logical, SEXPTYPE::LGLSXP.0);
            assert_eq!(LOGICAL_ELT(logical, 0), 1);
            assert_eq!(LOGICAL_ELT(logical, 1), NA_INTEGER);
        }
        let list = arena.alloc_list_chain(2);
        unsafe { SETCAR(list, logical) };
        let (_, allocation) = super::super::memory::checked_projection(list).unwrap();
        let heap = allocation.heap_identity();
        let original_body = heap.node_snapshot(&allocation).unwrap().data;
        for kind in [SEXPTYPE::LANGSXP, SEXPTYPE::DOTSXP, SEXPTYPE::LISTSXP] {
            unsafe {
                SET_TYPEOF(list, kind.0);
                assert_eq!(TYPEOF(list), kind.0);
                assert_eq!(CAR(list), logical);
                assert_eq!(XLENGTH(list), 2);
            }
            assert_eq!(heap.node_snapshot(&allocation).unwrap().data, original_body);
        }
        let nil = super::super::object::Sexp::nil();
        unsafe { SET_TYPEOF(nil.as_raw(), SEXPTYPE::LISTSXP.0) };
        assert!(nil.is_nil());
        assert_eq!(unsafe { TYPEOF(nil.as_raw()) }, SEXPTYPE::NILSXP.0);
    }

    #[test]
    fn nil_list_accessors_use_the_nil_value_without_a_list_body() {
        let value = super::super::object::Sexp::nil();
        let canonical = value.as_raw();
        let address_only = std::ptr::without_provenance_mut(canonical.addr());
        unsafe {
            for input in [canonical, address_only] {
                for output in [CAR(input), CDR(input), TAG(input)] {
                    assert_eq!(output, canonical);
                    assert_eq!((*output).sxpinfo.type_of(), SEXPTYPE::NILSXP);
                    assert!(matches!((*output).data, NodeBody::Other));
                }
            }
        }
    }

    #[test]
    fn string_setter_rejects_out_of_bounds_before_writing() {
        let mut arena = super::super::memory::RArena::new();
        let node = arena.alloc_vector(SEXPTYPE::STRSXP, 1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            SET_STRING_ELT(node, 1, ptr::null_mut());
        }));
        assert!(
            result.is_err(),
            "out-of-range string write must be rejected"
        );
        assert!(unsafe { STRING_ELT(node, 0) }.is_null());
    }

    #[test]
    fn string_setter_remembers_old_to_young_edges() {
        let mut session = super::super::session::RSession::new_for_gc_tests();
        let (parent, child) = session
            .with_arena(|arena| {
                (
                    arena.alloc_vector(SEXPTYPE::STRSXP, 1),
                    arena.alloc_charsxp(b"young"),
                )
            })
            .unwrap();
        session.with_active(|| unsafe {
            super::super::gengc::promote_to_old(parent);
            SET_STRING_ELT(parent, 0, child);
            super::super::instance::with_required_current_instance(|instance| {
                assert!(
                    (*instance)
                        .gc_state
                        .remembered_set
                        .iter()
                        .any(|p| p == parent)
                );
            });
            assert_eq!(STRING_ELT(parent, 0), child);
        });
    }

    #[test]
    fn element_access_checks_indices_and_tags_before_reading_or_writing() {
        let mut arena = super::super::memory::RArena::new();
        let node = arena.alloc_vector(SEXPTYPE::STRSXP, 1);
        for index in [-1, 1, i64::MAX] {
            for operation in 0..4 {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    match operation {
                        0 => {
                            STRING_ELT(node, index);
                        }
                        1 => {
                            VECTOR_ELT(node, index);
                        }
                        2 => SET_STRING_ELT(node, index, ptr::null_mut()),
                        _ => SET_VECTOR_ELT(node, index, ptr::null_mut()),
                    }
                }));
                assert!(result.is_err());
            }
        }
        for tag in [SEXPTYPE::REALSXP, SEXPTYPE::LISTSXP] {
            let node = arena.alloc_node(tag);
            for operation in 0..4 {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    match operation {
                        0 => {
                            STRING_ELT(node, 0);
                        }
                        1 => {
                            VECTOR_ELT(node, 0);
                        }
                        2 => SET_STRING_ELT(node, 0, ptr::null_mut()),
                        _ => SET_VECTOR_ELT(node, 0, ptr::null_mut()),
                    }
                }));
                assert!(result.is_err());
            }
        }
        assert!(unsafe { STRING_ELT(node, 0) }.is_null());
    }

    fn make_test_vector() -> (super::super::memory::RArena, SEXP) {
        let mut arena = super::super::memory::RArena::new();
        let node = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        (arena, node)
    }

    #[test]
    fn test_typeof_null() {
        unsafe {
            assert_eq!(TYPEOF(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_typeof_vector() {
        let (_arena, node) = make_test_vector();
        unsafe {
            assert_eq!(TYPEOF(node), 14); // REALSXP
        }
    }

    #[test]
    fn test_length_vector() {
        let (_arena, node) = make_test_vector();
        unsafe {
            assert_eq!(LENGTH(node), 3);
            assert_eq!(XLENGTH(node), 3);
        }
    }

    #[test]
    fn test_length_null() {
        unsafe {
            assert_eq!(LENGTH(ptr::null_mut()), 0);
            assert_eq!(XLENGTH(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_attrib_null() {
        unsafe {
            assert!(ATTRIB(ptr::null_mut()).is_null());
        }
    }

    #[test]
    fn test_isnull() {
        let (_arena, node) = make_test_vector();
        unsafe {
            assert_eq!(Rf_isNull(ptr::null_mut()), 1);
            assert_eq!(Rf_isNull(node), 0);
        }
    }

    #[test]
    fn test_set_attrib() {
        let (_arena, node) = make_test_vector();
        unsafe {
            let ptr = node;
            assert!(ATTRIB(ptr).is_null());
            SET_ATTRIB(ptr, ptr); // self-referential for test
            assert_eq!(ATTRIB(ptr), ptr);
            SET_ATTRIB(ptr, ptr::null_mut());
        }
    }

    #[test]
    fn test_named() {
        let (_arena, node) = make_test_vector();
        unsafe {
            let ptr = node;
            assert_eq!(NAMED(ptr), 0);
            SET_NAMED(ptr, 2);
            assert_eq!(NAMED(ptr), 2);
        }
    }

    #[test]
    fn test_object_flag() {
        let (_arena, node) = make_test_vector();
        unsafe {
            let ptr = node;
            assert_eq!(OBJECT(ptr), 0);
            SET_OBJECT(ptr, 1);
            assert_eq!(OBJECT(ptr), 1);
        }
    }

    #[test]
    fn test_set_truelength() {
        let (_arena, node) = make_test_vector();
        unsafe {
            let ptr = node;
            assert_eq!(TRUELENGTH(ptr), 3);
            SET_TRUELENGTH(ptr, 10);
            assert_eq!(TRUELENGTH(ptr), 10);
        }
    }

    #[test]
    fn test_elt_null_returns_na() {
        unsafe {
            assert_eq!(LOGICAL_ELT(ptr::null_mut(), 0), NA_INTEGER);
            assert_eq!(INTEGER_ELT(ptr::null_mut(), 0), NA_INTEGER);
            assert!(REAL_ELT(ptr::null_mut(), 0).is_nan());
        }
    }

    #[test]
    fn test_integer_accepts_logical_storage() {
        let mut arena = super::super::memory::RArena::new();
        let node = arena.alloc_vector(SEXPTYPE::LGLSXP, 1);
        unsafe {
            INTEGER(node).write(1);
            assert_eq!(*INTEGER(node), 1);
        }
    }

    #[test]
    fn test_dataptr_null() {
        unsafe {
            assert!(DATAPTR(ptr::null_mut()).is_null());
        }
    }

    #[test]
    fn test_invalid_pointer_guards() {
        unsafe {
            let bad = 0x1 as SEXP;
            assert_eq!(TYPEOF(bad), 0);
            assert_eq!(LENGTH(bad), 0);
            assert_eq!(XLENGTH(bad), 0);
            assert_eq!(TRUELENGTH(bad), 0);
            assert!(CAR(bad).is_null());
            assert!(CDR(bad).is_null());
            assert!(DATAPTR(bad).is_null());
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding accessors (GP bit checks)
// ---------------------------------------------------------------------------

const BYTES_MASK: u16 = 1 << 1; // 0x02
const LATIN1_MASK: u16 = 1 << 2; // 0x04
const UTF8_MASK: u16 = 1 << 3; // 0x08
const IS_ASCII_MASK: u16 = 1 << 6; // 0x40

/// IS_ASCII: check if CHARSXP has ASCII encoding marker.
pub unsafe fn IS_ASCII(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| {
        ((header.sxpinfo.gp() & IS_ASCII_MASK) != 0) as c_int
    })
}

/// IS_UTF8: check if CHARSXP has UTF-8 encoding marker.
pub unsafe fn IS_UTF8(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| {
        ((header.sxpinfo.gp() & UTF8_MASK) != 0) as c_int
    })
}

/// IS_BYTES: check if CHARSXP has bytes encoding marker.
pub unsafe fn IS_BYTES(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| {
        ((header.sxpinfo.gp() & BYTES_MASK) != 0) as c_int
    })
}

/// IS_LATIN1: check if CHARSXP has Latin-1 encoding marker.
pub unsafe fn IS_LATIN1(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| {
        ((header.sxpinfo.gp() & LATIN1_MASK) != 0) as c_int
    })
}

/// Set exactly one of UTF-8 / latin1 / bytes, or clear all (native/unknown).
pub unsafe fn mark_charsxp_encoding(x: SEXP, kind: &str) {
    mutate_header(x, |header| {
        let mut gp = header.sxpinfo.gp() & !(UTF8_MASK | LATIN1_MASK | BYTES_MASK);
        match kind {
            "UTF-8" => gp |= UTF8_MASK,
            "latin1" => gp |= LATIN1_MASK,
            "bytes" => gp |= BYTES_MASK,
            _ => {}
        }
        header.sxpinfo.set_gp(gp);
    });
}

/// ENC_KNOWN: check if CHARSXP has a known encoding.
/// Returns the OR of LATIN1_MASK, UTF8_MASK, and BYTES_MASK bits.
pub unsafe fn ENC_KNOWN(x: SEXP) -> c_int {
    header_snapshot(x).map_or(0, |header| {
        (header.sxpinfo.gp() & (LATIN1_MASK | UTF8_MASK | BYTES_MASK)) as c_int
    })
}

/// translateChar: return the CHAR pointer for a CHARSXP.
/// In the full R implementation, this re-encodes to native encoding.
/// Since we operate in UTF-8 mode, return CHAR(s) directly for UTF-8/ASCII,
/// and fall back to CHAR(s) for other encodings (best-effort).
pub unsafe fn translateChar(x: SEXP) -> *const c_char {
    unsafe {
        if !is_valid_sexp_ptr(x) {
            return std::ptr::null();
        }
        CHAR(x)
    }
}

/// Bytes of a CHARSXP translated to UTF-8, like GNU `translateCharUTF8`.
pub unsafe fn charsxp_as_utf8(x: SEXP) -> Vec<u8> {
    unsafe {
        if !is_valid_sexp_ptr(x) || x == crate::sexp::globals::R_NaString() {
            return Vec::new();
        }
        let n = if TYPEOF(x) == SEXPTYPE::CHARSXP {
            LENGTH(x) as usize
        } else {
            0
        };
        let bytes = if n == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(CHAR(x) as *const u8, n)
        };
        if IS_LATIN1(x) != 0 {
            let mut out = Vec::new();
            for &b in bytes {
                let mut buf = [0u8; 4];
                out.extend_from_slice(char::from(b).encode_utf8(&mut buf).as_bytes());
            }
            out
        } else {
            bytes.to_vec()
        }
    }
}

/// translateCharUTF8: return the CHAR pointer for a CHARSXP as UTF-8.
/// For UTF-8 or ASCII strings, return directly. For others, return as-is (best-effort).
pub unsafe fn translateCharUTF8(x: SEXP) -> *const c_char {
    unsafe {
        if !is_valid_sexp_ptr(x) {
            return std::ptr::null();
        }
        CHAR(x)
    }
}

/// getCharCE: return the character encoding of a CHARSXP.
/// Returns CE_NATIVE (0) for native, CE_UTF8 (2) for UTF-8, etc.
pub unsafe fn getCharCE(x: SEXP) -> c_int {
    let gp = header_snapshot(x).map_or(0, |header| header.sxpinfo.gp());
    if gp & UTF8_MASK != 0 {
        2
    } else if gp & LATIN1_MASK != 0 {
        3
    } else if gp & BYTES_MASK != 0 {
        4
    } else {
        0
    }
}
#[test]
fn reference_elements_reject_forged_lengths_and_payloads_without_reading_them() {
    let mut arena = super::memory::RArena::new();
    let parent = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
    let node = arena.node_token(parent).unwrap();
    let original = super::memory::checked_snapshot(parent, &node).unwrap();
    let numeric = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
    let numeric_node = arena.node_token(numeric).unwrap();
    let numeric_payload = super::memory::checked_snapshot(numeric, &numeric_node)
        .unwrap()
        .gengc_next_node;
    for payload in [
        original.gengc_next_node,
        numeric_payload,
        ptr::dangling_mut(),
    ] {
        let mut forged = original;
        forged.data.vector_mut().length = 3;
        forged.gengc_next_node = payload;
        // SAFETY: this fixture exclusively modifies its own initialized
        // header. Every element operation must reject the forged shape.
        unsafe {
            parent.write(forged);
        }
        for index in [0, 2] {
            for write in [false, true] {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    if write {
                        SET_VECTOR_ELT(parent, index, ptr::null_mut());
                    } else {
                        VECTOR_ELT(parent, index);
                    }
                }));
                assert!(result.is_err());
            }
        }
    }
    unsafe {
        parent.write(original);
    }
    assert_eq!(
        node.heap_identity().reference_elements(&node),
        Some(vec![ptr::null_mut()])
    );
}
