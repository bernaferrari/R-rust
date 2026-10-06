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

enum HeaderOrigin {
    Singleton(super::globals::SingletonLease),
    Managed {
        projection: SEXP,
        node: super::heap::CheckedNode,
    },
}

struct HeaderRead {
    snapshot: SexprecCore,
    origin: HeaderOrigin,
}

impl HeaderRead {
    fn projection(&self) -> SEXP {
        match &self.origin {
            HeaderOrigin::Singleton(lease) => lease.projection(),
            HeaderOrigin::Managed { projection, .. } => *projection,
        }
    }
}

fn header_read(pointer: SEXP, failure: &str) -> Option<HeaderRead> {
    if !is_valid_sexp_ptr(pointer) {
        return None;
    }
    if let Some(singleton) = immutable_lease(pointer) {
        return Some(HeaderRead {
            snapshot: singleton.snapshot(),
            origin: HeaderOrigin::Singleton(singleton),
        });
    }
    let (projection, node, snapshot) =
        super::memory::checked_header(pointer).unwrap_or_else(|| super::context::r_error(failure));
    Some(HeaderRead {
        snapshot,
        origin: HeaderOrigin::Managed { projection, node },
    })
}

fn header_snapshot(pointer: SEXP) -> Option<SexprecCore> {
    header_read(pointer, "unowned header read").map(|read| read.snapshot)
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
    let Some(read) = header_read(pointer, "unowned graph parent") else {
        return ptr::null_mut();
    };
    graph_edge_from_read(&read, field)
}

fn graph_edge_from_read(read: &HeaderRead, field: EdgeField) -> SEXP {
    let HeaderOrigin::Managed { node, .. } = &read.origin else {
        return ptr::null_mut();
    };
    let link = read
        .snapshot
        .edge(field)
        .unwrap_or_else(|| super::context::r_error("invalid graph field"));
    node.heap_identity()
        .projection_of_link(link)
        .unwrap_or_else(|| super::context::r_error("stale or foreign graph child"))
}

fn list_edge(pointer: SEXP, field: EdgeField) -> SEXP {
    let Some(read) = header_read(pointer, "unowned header read") else {
        return ptr::null_mut();
    };
    if read.snapshot.sxpinfo.type_of() == SEXPTYPE::NILSXP {
        return read.projection();
    }
    graph_edge_from_read(&read, field)
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
    // Attribute replacement changes only the public graph. Provider state and
    // compact formulas are retained by the canonical private vector edge.
    graph_set_edge(x, EdgeField::Attribute, v);
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

/// GNU scalar admission checks both the requested kind and scalar flag.
/// The supplied address is only a lookup key for a checked owning Cell or
/// immutable lease; reading flags requires no caller dereference authority.
pub fn IS_SCALAR(x: SEXP, requested_type: c_int) -> c_int {
    header_snapshot(x).map_or(0, |header| {
        c_int::from(header.sxpinfo.type_of().0 == requested_type && header.sxpinfo.scalar())
    })
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
    list_edge(x, EdgeField::ListCar)
}

/// Get the CDR of a cons cell.
pub unsafe fn CDR(x: SEXP) -> SEXP {
    list_edge(x, EdgeField::ListCdr)
}

/// Get the TAG of a cons cell.
pub unsafe fn TAG(x: SEXP) -> SEXP {
    list_edge(x, EdgeField::ListTag)
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

/// Retain the actual original allocation before a raw callback boundary.
unsafe fn raw_value<'s>(
    pointer: SEXP,
) -> (
    super::object::Sexp<'s>,
    Option<std::rc::Rc<super::heap::NodeRootLease>>,
) {
    if let Ok(owner) = unsafe { super::owner::OwnerToken::current() }
        && let Ok(value) = owner.sexp(pointer)
    {
        return (value, None);
    }
    let value = unsafe { super::object::Sexp::try_from_raw(pointer) }
        .unwrap_or_else(|error| super::context::r_error(error.to_string()));
    let root = value.allocation().ok().map(|node| {
        node.root_lease()
            .unwrap_or_else(|| super::context::r_error("raw payload parent unavailable"))
    });
    (value, root)
}

/// Project the actual retained payload for translated native operations.
/// # Safety
/// Retain the original parent and exclude conflicting payload loans, mutation
/// and callbacks for the complete use of the returned native projection.
pub unsafe fn DATAPTR(x: SEXP) -> *mut c_void {
    if !is_valid_sexp_ptr(x) {
        return ptr::null_mut();
    }
    let original = header_snapshot(x).expect("native payload parent");
    if !original.has_valid_shape()
        || matches!(original.data, NodeBody::Vector(vector) if !(0..=1_i64 << 52).contains(&vector.length))
    {
        super::context::r_error("invalid native payload shape");
    }
    let (value, _root) = unsafe { raw_value(x) };
    let kind = value.typeof_();
    if kind == SEXPTYPE::CHARSXP {
        super::context::r_error("character scalar payload is immutable");
    }
    if !kind.is_vector_type() && kind != SEXPTYPE::CHARSXP {
        return ptr::null_mut();
    }
    value
        .materialize_compact_payload_for_raw()
        .unwrap_or_else(|error| super::context::r_error(error.to_string()));
    let header = value.header();
    if header.type_of() != kind {
        super::context::r_error("native payload type changed during materialization");
    }
    let Some(lease) = header.payload_lease() else {
        if header.body.vector().length == 0 {
            return ptr::null_mut();
        }
        super::context::r_error("nonempty vector has no committed typed allocation");
    };
    lease.native_projection().cast::<c_void>()
}

/// Get the data pointer, returning a const pointer.
pub unsafe fn ROBJ_DATAPTR(x: SEXP) -> *const c_void {
    if header_snapshot(x).is_some_and(|header| header.sxpinfo.type_of() == SEXPTYPE::CHARSXP) {
        unsafe { CHAR(x).cast::<c_void>() }
    } else {
        unsafe { DATAPTR(x) }
    }
}

/// Get a pointer to the logical vector data.
pub unsafe fn LOGICAL(x: SEXP) -> *mut c_int {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::LGLSXP]);
        let data = DATAPTR(x);
        if ![SEXPTYPE::LGLSXP].contains(
            &header_snapshot(x)
                .expect("typed payload parent")
                .sxpinfo
                .type_of(),
        ) {
            super::context::r_error("native payload type mismatch");
        }
        data as *mut c_int
    }
}

/// Get a pointer to the integer-compatible vector data.
///
/// R stores logical vectors as `c_int` too, and translated C code sometimes
/// uses INTEGER on LGLSXP when it wants the raw storage representation.
pub unsafe fn INTEGER(x: SEXP) -> *mut c_int {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::INTSXP, SEXPTYPE::LGLSXP]);
        let data = DATAPTR(x);
        if ![SEXPTYPE::INTSXP, SEXPTYPE::LGLSXP].contains(
            &header_snapshot(x)
                .expect("typed payload parent")
                .sxpinfo
                .type_of(),
        ) {
            super::context::r_error("native payload type mismatch");
        }
        data as *mut c_int
    }
}

/// Get a pointer to the real (double) vector data.
pub unsafe fn REAL(x: SEXP) -> *mut c_double {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::REALSXP]);
        let data = DATAPTR(x);
        if ![SEXPTYPE::REALSXP].contains(
            &header_snapshot(x)
                .expect("typed payload parent")
                .sxpinfo
                .type_of(),
        ) {
            super::context::r_error("native payload type mismatch");
        }
        data as *mut c_double
    }
}

/// Get a pointer to the complex vector data.
pub unsafe fn COMPLEX(x: SEXP) -> *mut Rcomplex {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::CPLXSXP]);
        let data = DATAPTR(x);
        if ![SEXPTYPE::CPLXSXP].contains(
            &header_snapshot(x)
                .expect("typed payload parent")
                .sxpinfo
                .type_of(),
        ) {
            super::context::r_error("native payload type mismatch");
        }
        data as *mut Rcomplex
    }
}

/// Get a pointer to the raw byte vector data.
pub unsafe fn RAW(x: SEXP) -> *mut super::ffi::Rbyte {
    unsafe {
        debug_assert_sexptype(x, &[SEXPTYPE::RAWSXP]);
        let data = DATAPTR(x);
        if ![SEXPTYPE::RAWSXP].contains(
            &header_snapshot(x)
                .expect("typed payload parent")
                .sxpinfo
                .type_of(),
        ) {
            super::context::r_error("native payload type mismatch");
        }
        data as *mut super::ffi::Rbyte
    }
}

/// Get the sealed character bytes without executing R or creating an owning value.
/// # Safety
/// Retain the original character parent for the entire use of this const projection.
pub unsafe fn CHAR(x: SEXP) -> *const c_char {
    let Some(read) = header_read(x, "unowned header read") else {
        return ptr::null();
    };
    if read.snapshot.sxpinfo.type_of() != SEXPTYPE::CHARSXP {
        super::context::r_error("CHAR requires a character scalar");
    }
    if !read.snapshot.has_valid_shape() {
        super::context::r_error("invalid character shape");
    }
    // The canonical missing-string lease supplies immutable GNU "NA" bytes.
    // Its identity, rather than a caller address or a replacement runtime,
    // authorizes this special read.
    let lease = match &read.origin {
        HeaderOrigin::Singleton(singleton) if singleton.is_na_string() => return c"NA".as_ptr(),
        HeaderOrigin::Singleton(singleton) => singleton.payload_lease(),
        HeaderOrigin::Managed { node, .. } => node.heap_identity().payload_lease(node),
    }
    .unwrap_or_else(|| {
        super::context::r_error("character scalar has no committed byte allocation")
    });
    let length = usize::try_from(read.snapshot.data.vector().length)
        .unwrap_or_else(|_| super::context::r_error("invalid character length"));
    if !lease.matches_header(&read.snapshot)
        || !lease.is_immutable()
        || lease.byte_elt(length) != Some(0)
    {
        super::context::r_error("character payload must be sealed with a trailing NUL");
    }
    lease.native_projection().cast::<c_char>().cast_const()
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

/// Copy the bounded logical element from its original retained allocation.
pub unsafe fn LOGICAL_ELT(x: SEXP, i: c_int) -> c_int {
    if !is_valid_sexp_ptr(x) {
        return NA_INTEGER;
    }
    let (value, _root) = unsafe { raw_value(x) };
    let result = value.try_logical_elt(i as R_xlen_t);
    result.unwrap_or_else(|error| super::context::r_error(error.to_string()))
}

/// Update a bounded canonical logical cell; immutable sentinels stay unchanged.
pub unsafe fn SET_LOGICAL_ELT(x: SEXP, i: c_int, v: c_int) {
    if x.is_null() || immutable_lease(x).is_some() {
        return;
    }
    let (value, _root) = unsafe { raw_value(x) };
    let mut value = unsafe { super::object::SexpMut::from_owned(value) };
    value
        .try_set_logical_elt(i as R_xlen_t, v)
        .unwrap_or_else(|error| super::context::r_error(error.to_string()));
}

/// Copy the bounded integer element from its original retained allocation.
pub unsafe fn INTEGER_ELT(x: SEXP, i: c_int) -> c_int {
    if !is_valid_sexp_ptr(x) {
        return NA_INTEGER;
    }
    let (value, _root) = unsafe { raw_value(x) };
    let result = if value.typeof_() == SEXPTYPE::LGLSXP {
        value.try_logical_elt(i as R_xlen_t)
    } else {
        value.try_integer_elt(i as R_xlen_t)
    };
    result.unwrap_or_else(|error| super::context::r_error(error.to_string()))
}

/// Update a bounded canonical integer cell; immutable sentinels stay unchanged.
pub unsafe fn SET_INTEGER_ELT(x: SEXP, i: c_int, v: c_int) {
    if x.is_null() || immutable_lease(x).is_some() {
        return;
    }
    let (value, _root) = unsafe { raw_value(x) };
    let mut value = unsafe { super::object::SexpMut::from_owned(value) };
    let result = if value.typeof_() == SEXPTYPE::LGLSXP {
        value.try_set_logical_elt(i as R_xlen_t, v)
    } else {
        value.try_set_integer_elt(i as R_xlen_t, v)
    };
    result.unwrap_or_else(|error| super::context::r_error(error.to_string()));
}

/// Copy the bounded real element from its original retained allocation.
pub unsafe fn REAL_ELT(x: SEXP, i: c_int) -> c_double {
    if !is_valid_sexp_ptr(x) {
        return NA_REAL;
    }
    let (value, _root) = unsafe { raw_value(x) };
    let result = value.try_real_elt(i as R_xlen_t);
    result.unwrap_or_else(|error| super::context::r_error(error.to_string()))
}

/// Update a bounded canonical real cell; immutable sentinels stay unchanged.
pub unsafe fn SET_REAL_ELT(x: SEXP, i: c_int, v: c_double) {
    if x.is_null() || immutable_lease(x).is_some() {
        return;
    }
    let (value, _root) = unsafe { raw_value(x) };
    let mut value = unsafe { super::object::SexpMut::from_owned(value) };
    value
        .try_set_real_elt(i as R_xlen_t, v)
        .unwrap_or_else(|error| super::context::r_error(error.to_string()));
}

/// Copy the bounded complex element from its original retained allocation.
pub unsafe fn COMPLEX_ELT(x: SEXP, i: c_int) -> Rcomplex {
    if !is_valid_sexp_ptr(x) {
        return Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        };
    }
    let (value, _root) = unsafe { raw_value(x) };
    let result = value.try_complex_elt(i as R_xlen_t);
    result.unwrap_or_else(|error| super::context::r_error(error.to_string()))
}

/// Update a bounded canonical complex cell; immutable sentinels stay unchanged.
pub unsafe fn SET_COMPLEX_ELT(x: SEXP, i: c_int, v: Rcomplex) {
    if x.is_null() || immutable_lease(x).is_some() {
        return;
    }
    let (value, _root) = unsafe { raw_value(x) };
    let mut value = unsafe { super::object::SexpMut::from_owned(value) };
    value
        .try_set_complex_elt(i as R_xlen_t, v)
        .unwrap_or_else(|error| super::context::r_error(error.to_string()));
}

/// Copy the bounded raw element from its original retained allocation.
pub unsafe fn RAW_ELT(x: SEXP, i: c_int) -> super::ffi::Rbyte {
    if !is_valid_sexp_ptr(x) {
        return 0;
    }
    let (value, _root) = unsafe { raw_value(x) };
    let result = value.try_raw_elt(i as R_xlen_t);
    result.unwrap_or_else(|error| super::context::r_error(error.to_string()))
}

/// Update a bounded canonical raw cell; immutable sentinels stay unchanged.
pub unsafe fn SET_RAW_ELT(x: SEXP, i: c_int, v: super::ffi::Rbyte) {
    if x.is_null() || immutable_lease(x).is_some() {
        return;
    }
    let (value, _root) = unsafe { raw_value(x) };
    let mut value = unsafe { super::object::SexpMut::from_owned(value) };
    value
        .try_set_raw_elt(i as R_xlen_t, v)
        .unwrap_or_else(|error| super::context::r_error(error.to_string()));
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
    fn character_native_reads_are_const_and_mutable_projection_is_rejected() {
        let mut arena = super::super::memory::RArena::new();
        let character = arena.alloc_charsxp("café".as_bytes());
        let (_, node) = super::super::memory::checked_projection(character).unwrap();
        let heap = node.heap_identity();
        let original = heap.node_snapshot(&node).unwrap();
        let lease = heap.payload_lease(&node).unwrap();
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            DATAPTR(character)
        }));
        assert!(rejected.is_err());
        assert!(lease.is_immutable());
        assert_eq!(heap.node_snapshot(&node).unwrap().payload, original.payload);
        unsafe {
            assert_eq!(
                std::ffi::CStr::from_ptr(CHAR(character)).to_bytes(),
                "café".as_bytes()
            );
            assert_eq!(ROBJ_DATAPTR(character), CHAR(character).cast::<c_void>());
        }
        let generic = arena.alloc_node(SEXPTYPE::CHARSXP);
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(CHAR(generic)).to_bytes() },
            b""
        );

        let mut permanent = super::super::instance::persistent::PersistentHeap::new(heap);
        let character = permanent.allocate_chars(b"permanent").unwrap();
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(CHAR(character)).to_bytes() },
            b"permanent"
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                DATAPTR(character)
            }))
            .is_err()
        );

        // Writable raw and numeric vectors keep their ordinary native path.
        let raw = arena.alloc_vector(SEXPTYPE::RAWSXP, 1);
        unsafe { RAW(raw).write(b'x') };
        assert_eq!(unsafe { RAW_ELT(raw, 0) }, b'x');
        let integer = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        unsafe { INTEGER(integer).write(23) };
        assert_eq!(unsafe { INTEGER_ELT(integer, 0) }, 23);
    }

    #[test]
    fn immutable_na_character_keeps_its_const_sentinel_read() {
        let session = super::super::session::RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let sentinel = super::super::globals::R_NaString();
            assert_eq!(std::ffi::CStr::from_ptr(CHAR(sentinel)).to_bytes(), b"NA");
            assert_eq!(
                std::ffi::CStr::from_ptr(ROBJ_DATAPTR(sentinel).cast()).to_bytes(),
                b"NA"
            );
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| DATAPTR(sentinel)))
                    .is_err()
            );
            assert_eq!(TYPEOF(sentinel), SEXPTYPE::CHARSXP.0);
        });
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
fn reference_elements_reject_foreign_payloads_and_oversized_headers() {
    let mut arena = super::memory::RArena::new();
    let parent = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
    let node = arena.node_token(parent).unwrap();
    let heap = node.heap_identity();
    let original = heap.node_snapshot(&node).unwrap();
    let numeric = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
    let numeric_node = arena.node_token(numeric).unwrap();
    let numeric_payload = heap.payload_lease(&numeric_node).unwrap();
    let mut oversized = original;
    oversized.data.vector_mut().length = 3;
    assert!(heap.replace_node(&node, oversized).is_none());
    assert!(
        heap.publish_payload(&node, original.payload, &numeric_payload)
            .is_none()
    );
    let mut foreign_arena = super::memory::RArena::new();
    let foreign = foreign_arena.alloc_vector(SEXPTYPE::VECSXP, 3);
    let foreign_node = foreign_arena.node_token(foreign).unwrap();
    let foreign_payload = foreign_node
        .heap_identity()
        .payload_lease(&foreign_node)
        .unwrap();
    assert!(
        heap.publish_payload(&node, original.payload, &foreign_payload)
            .is_none()
    );
    let unchanged = heap.node_snapshot(&node).unwrap();
    assert_eq!(unchanged.payload, original.payload);
    assert_eq!(unchanged.data, original.data);
    assert_eq!(
        heap.reference_links(&node),
        Some(vec![super::heap::NodeLink::NULL])
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            VECTOR_ELT(parent, 2);
        }))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            SET_VECTOR_ELT(parent, 2, ptr::null_mut());
        }))
        .is_err()
    );
}

#[cfg(test)]
#[path = "accessors/projection_tests.rs"]
mod projection_tests;

#[cfg(test)]
#[path = "accessors/scalar_admission_tests.rs"]
mod scalar_admission_tests;

#[cfg(test)]
#[path = "accessors/na_character_tests.rs"]
mod na_character_tests;
