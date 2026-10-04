#![allow(unused_variables)]
#![allow(unused_assignments)]
//! Object duplication system for R objects.
//!
//! Ported from R's src/main/duplicate.c. Provides deep and shallow duplication
//! of R objects, vector/matrix copy with recycling, and cycle detection for
//! complex assignment operations.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::{Rf_allocVector3, Rf_cons};
use crate::sexp::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::with_arena;

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

unsafe fn DispatchGroup(
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
unsafe fn NO_REFERENCES(x: SEXP) -> c_int {
    unsafe { crate::mainutils::relop::NO_REFERENCES(x) }
}

/// Check if an object is an S4 object.
#[inline]
unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & S4_OBJECT_MASK) != 0) as c_int
    }
}

/// Set the S4 object flag.
#[inline]
unsafe fn SET_S4_OBJECT(x: SEXP) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp() | S4_OBJECT_MASK;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Unset the S4 object flag.
#[inline]
unsafe fn UNSET_S4_OBJECT(x: SEXP) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp() & !S4_OBJECT_MASK;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Check the NOJIT gp bit.
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

/// GNU `duplicate` / `shallow_duplicate`: a traced object reports the copy
/// and the copy stays traced. Closures, builtins, specials, promises, and
/// environments are excluded, matching `duplicate.c`.
unsafe fn trace_duplication(s: SEXP, t: SEXP) {
    unsafe {
        if RTRACE(s) == 0 {
            return;
        }
        let ty = TYPEOF(s);
        if ty == SEXPTYPE::CLOSXP
            || ty == SEXPTYPE::BUILTINSXP
            || ty == SEXPTYPE::SPECIALSXP
            || ty == SEXPTYPE::PROMSXP
            || ty == SEXPTYPE::ENVSXP
        {
            return;
        }
        crate::mainutils::debug::memtrace_report(
            s as *mut std::ffi::c_void,
            t as *mut std::ffi::c_void,
        );
        SET_RTRACE(t, 1);
    }
}

/// Set NAMED to maximum (2).
#[inline]
unsafe fn ENSURE_NAMEDMAX(x: SEXP) {
    unsafe {
        SET_NAMED(x, 2);
    }
}

/// Check if the GROWABLE_BIT is set.
#[inline]
unsafe fn GROWABLE_BIT_SET(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & GROWABLE_BIT_MASK) != 0) as c_int
    }
}

/// Raise a typed error for SEXPTYPEs this port cannot duplicate/copy yet.
unsafe fn UNIMPLEMENTED_TYPE(routine: *const c_char, s: SEXP) -> ! {
    unsafe {
        let routine = if routine.is_null() {
            "duplicate"
        } else {
            CStr::from_ptr(routine).to_str().unwrap_or("duplicate")
        };
        let sexptype = if s.is_null() { -1 } else { TYPEOF(s) };
        std::panic::panic_any(crate::sexp::context::RError {
            message: format!("{routine}: unsupported SEXPTYPE {sexptype}"),
        });
    }
}

/// Set the DDVAL flag on a symbol.
#[inline]
unsafe fn SET_DDVAL(x: SEXP, v: c_int) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if !x.is_null() {
            let gp = if v != 0 {
                (*x).sxpinfo.gp() | DDVAL_MASK
            } else {
                (*x).sxpinfo.gp() & !DDVAL_MASK
            };
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Check if a type is pairlist-like.
#[inline]
unsafe fn isPairList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::DOTSXP) as c_int
    }
}

/// Check if a type is a vector list (VECSXP, EXPRSXP).
#[inline]
unsafe fn isVectorList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP) as c_int
    }
}

/// Get nrows from dim attribute.
unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
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
}

/// Get ncols from dim attribute.
unsafe fn ncols(x: SEXP) -> c_int {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// Internal macros / inline helpers
// ---------------------------------------------------------------------------

/// Copy true length from `from` to `to`, unless GROWABLE_BIT_SET(from).
#[inline]
unsafe fn COPY_TRUELENGTH(to: SEXP, from: SEXP) {
    unsafe {
        if GROWABLE_BIT_SET(from) == 0 {
            SET_TRUELENGTH(to, TRUELENGTH(from));
        }
    }
}

/// Duplicate attributes from `from` to `to`.
/// If `from` has non-nil attributes, they are deep or shallow duplicated
/// based on the `deep` flag.
#[inline]
unsafe fn DUPLICATE_ATTRIB(to: SEXP, from: SEXP, deep: c_int) {
    unsafe {
        let mut a = ATTRIB(from);
        if crate::sexp::altrep::has_extension_raw(from) {
            // Ordinary duplicates copy values and public attributes, not a
            // descriptor/payload belonging to the source's lazy class.
            a = CDR(a);
        }
        if !a.is_null() && a != R_NilValue() {
            SET_ATTRIB(to, duplicate1(a, deep));
            SET_OBJECT(to, OBJECT(from));
            if IS_S4_OBJECT(from) != 0 {
                SET_S4_OBJECT(to);
            } else {
                UNSET_S4_OBJECT(to);
            }
        }
    }
}

/// Copy tag from `from` to `to`, if it is non-nil.
#[inline]
unsafe fn COPY_TAG(to: SEXP, from: SEXP) {
    unsafe {
        let tag = TAG(from);
        if !tag.is_null() && tag != R_NilValue() {
            SETTAG(to, tag);
        }
    }
}

/// Generic function to duplicate an atomic vector.
/// Handles the memcpy for the data and copies attributes.
unsafe fn duplicate_atomic_vector(
    elem_size: usize,
    to: *mut SEXP,
    from: SEXP,
    deep: c_int,
) -> SEXP {
    unsafe {
        let n = XLENGTH(from);
        let new_vec = Rf_allocVector3(TYPEOF(from), n);
        let _guard = crate::sexp::protect::protect(new_vec);
        *to = new_vec;
        if n > 0 {
            let from_data = DATAPTR(from);
            let to_data = DATAPTR(new_vec);
            if !from_data.is_null() && !to_data.is_null() {
                let total_bytes = (n as usize) * elem_size;
                ptr::copy_nonoverlapping(from_data as *const u8, to_data as *mut u8, total_bytes);
            }
        }
        DUPLICATE_ATTRIB(new_vec, from, deep);
        COPY_TRUELENGTH(new_vec, from);
        new_vec
    }
}

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
unsafe fn fill_matrix_iterate<F>(
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
unsafe fn fill_matrix_byrow_iterate<F>(
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

/// Core recursive duplication function.
///
/// `deep`: if nonzero, performs deep copy; if zero, performs shallow copy
/// (shared subtrees for pairlists/vectors, but new atomic vectors are copied).
unsafe fn duplicate1(s: SEXP, deep: c_int) -> SEXP {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }

        // Retain a Rust recursion guard throughout the default copy too.
        // Pointer-valued class elements can legitimately refer to their parent.
        let class_copy_source = if crate::sexp::altrep::has_extension_raw(s) {
            Some(
                crate::sexp::altrep::rooted_raw(s)
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
            )
        } else {
            None
        };
        let _class_copy_guard = class_copy_source.as_ref().map(|source| {
            crate::sexp::altrep::duplication_guard(source)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        });

        let mut t: SEXP = ptr::null_mut();
        // Keep the external source and result owned through flag copying too.
        let mut external_anchors = None;

        match SEXPTYPE(TYPEOF(s)) {
            SEXPTYPE::NILSXP
            | SEXPTYPE::SYMSXP
            | SEXPTYPE::ENVSXP
            | SEXPTYPE::SPECIALSXP
            | SEXPTYPE::BUILTINSXP
            | SEXPTYPE::BCODESXP
            | SEXPTYPE::WEAKREFSXP => {
                return s;
            }
            SEXPTYPE::CLOSXP => {
                let source = crate::sexp::altrep::rooted_raw(s)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                // GNU shares closure syntax, including source bodies. Retain
                // the original edges before an allocating callback can detach
                // them, and publish the initialized copy before callbacks run.
                return duplicate_closure(&source, deep).as_raw();
            }
            SEXPTYPE::LISTSXP => {
                t = duplicate_list(s, deep);
            }
            SEXPTYPE::LANGSXP => {
                t = duplicate_list(s, deep);
                crate::sexp::accessors::SET_TYPEOF(t, SEXPTYPE::LANGSXP.as_c_int());
                DUPLICATE_ATTRIB(t, s, deep);
            }
            SEXPTYPE::DOTSXP => {
                t = duplicate_list(s, deep);
                crate::sexp::accessors::SET_TYPEOF(t, SEXPTYPE::DOTSXP.as_c_int());
                DUPLICATE_ATTRIB(t, s, deep);
            }
            SEXPTYPE::CHARSXP => {
                return s;
            }
            SEXPTYPE::EXPRSXP | SEXPTYPE::VECSXP => {
                let n = XLENGTH(s);
                t = Rf_allocVector3(TYPEOF(s), n);
                let _guard = crate::sexp::protect::protect(t);
                for i in 0..n {
                    SET_VECTOR_ELT(t, i, duplicate_child(VECTOR_ELT(s, i), deep));
                }
                DUPLICATE_ATTRIB(t, s, deep);
                COPY_TRUELENGTH(t, s);
            }
            SEXPTYPE::LGLSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<c_int>(), &mut result, s, deep);
            }
            SEXPTYPE::INTSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<c_int>(), &mut result, s, deep);
            }
            SEXPTYPE::REALSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<c_double>(), &mut result, s, deep);
            }
            SEXPTYPE::CPLXSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<Rcomplex>(), &mut result, s, deep);
            }
            SEXPTYPE::RAWSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<Rbyte>(), &mut result, s, deep);
            }
            SEXPTYPE::STRSXP => {
                let n = XLENGTH(s);
                t = Rf_allocVector3(TYPEOF(s), n);
                let _guard = crate::sexp::protect::protect(t);
                for i in 0..n {
                    SET_STRING_ELT(t, i, STRING_ELT(s, i));
                }
                DUPLICATE_ATTRIB(t, s, deep);
                COPY_TRUELENGTH(t, s);
            }
            SEXPTYPE::PROMSXP => {
                return s;
            }
            SEXPTYPE::EXTPTRSXP => {
                let owner = crate::sexp::owner::OwnerToken::current()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let factory = crate::sexp::object::SessionNodeFactory::new(owner);
                let source = factory
                    .wrap(s)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let copy = duplicate_external_pointer(&source, deep);
                t = copy.as_raw();
                external_anchors = Some((source, copy));
            }
            SEXPTYPE::WEAKREFSXP => {
                return s;
            }
            SEXPTYPE::OBJSXP => {
                t = crate::mainutils::objects::R_allocObject();
                if !t.is_null() {
                    DUPLICATE_ATTRIB(t, s, deep);
                } else {
                    UNIMPLEMENTED_TYPE(b"duplicate\0".as_ptr() as *const c_char, s);
                }
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"duplicate\0".as_ptr() as *const c_char, s);
            }
        }

        // Copy OBJECT and S4 flags if types match
        if TYPEOF(t) == TYPEOF(s) {
            SET_OBJECT(t, OBJECT(s));
            if IS_S4_OBJECT(s) != 0 {
                SET_S4_OBJECT(t);
            } else {
                UNSET_S4_OBJECT(t);
            }
        }

        t
    }
}

/// GNU closure duplication shares formals, body, and environment; only
/// attributes follow the requested deep/shallow duplication policy.
unsafe fn duplicate_closure<'s>(
    source: &crate::sexp::object::Sexp<'s>,
    deep: c_int,
) -> crate::sexp::object::Sexp<'s> {
    let fail = |error: crate::sexp::object::SexpError| -> ! {
        crate::sexp::context::r_error(error.to_string())
    };
    let factory = source.node_factory().unwrap_or_else(|error| fail(error));
    let formals = source.try_formals().unwrap_or_else(|error| fail(error));
    let body = source.try_body().unwrap_or_else(|error| fail(error));
    let environment = source.try_cloenv().unwrap_or_else(|error| fail(error));
    let attributes = source.try_attrib().unwrap_or_else(|error| fail(error));
    let source_node = source.allocation().expect("rooted closure allocation");
    let flags = source_node
        .heap_identity()
        .node_snapshot(source_node)
        .expect("rooted closure header")
        .sxpinfo;
    let formals_link = factory.link(&formals).unwrap_or_else(|error| fail(error));
    let body_link = factory.link(&body).unwrap_or_else(|error| fail(error));
    let environment_link = factory
        .link(&environment)
        .unwrap_or_else(|error| fail(error));
    let copy = factory
        .allocate(|arena| {
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
        })
        .unwrap_or_else(|error| fail(error));
    if !attributes.is_nil() {
        let copied_attributes = factory
            .wrap(unsafe { duplicate1(attributes.as_raw(), deep) })
            .unwrap_or_else(|error| fail(error));
        unsafe {
            SET_ATTRIB(copy.as_raw(), copied_attributes.as_raw());
        }
    }
    copy
}

/// Own every external-pointer edge across allocating recursive duplication.
unsafe fn duplicate_external_pointer<'s>(
    source: &crate::sexp::object::Sexp<'s>,
    deep: c_int,
) -> crate::sexp::object::Sexp<'s> {
    unsafe {
        let factory = source
            .node_factory()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let protected_is_null =
            crate::mainutils::memory_main::R_ExternalPtrProtected(source.as_raw()).is_null();
        let tag_is_null =
            crate::mainutils::memory_main::R_ExternalPtrTag(source.as_raw()).is_null();
        let protected = source
            .try_extprot()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let tag = source
            .try_extptr_tag()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let attributes = source
            .try_attrib()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let address = source
            .try_extptr_ptr()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let source_node = source.allocation()
            .expect("owning external pointer retains its original allocation");
        let resource = source_node.heap_identity().resource_erased(source_node);
        let copy = factory
            .allocate(|arena| {
                let projection = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
                let allocation = arena.node_token(projection)?;
                let heap = arena.heap_identity();
                let mut header = heap.node_snapshot(&allocation)?;
                header.data.extptr_mut().address = address;
                heap.replace_node(&allocation, header)?;
                // Opaque addresses alias under GNU duplication. Attached Rust
                // state shares ownership across the independent node lifetimes
                // and is authenticated before allocation callbacks can run.
                if let Some(resource) = &resource {
                    heap.attach_resource(&allocation, resource.clone())?;
                }
                Some(projection)
            })
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let copied_protected = if protected_is_null {
            None
        } else {
            Some(
                factory
                    .wrap(duplicate_child(protected.as_raw(), deep))
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            )
        };
        if let Some(value) = &copied_protected {
            crate::mainutils::memory_main::R_SetExternalPtrProtected(copy.as_raw(), value.as_raw());
        }
        let copied_tag = if tag_is_null {
            None
        } else {
            Some(
                factory
                    .wrap(duplicate_child(tag.as_raw(), deep))
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            )
        };
        if let Some(value) = &copied_tag {
            crate::mainutils::memory_main::R_SetExternalPtrTag(copy.as_raw(), value.as_raw());
        }
        if !attributes.is_nil() {
            let copied_attributes = factory
                .wrap(duplicate1(attributes.as_raw(), deep))
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            SET_ATTRIB(copy.as_raw(), copied_attributes.as_raw());
        }
        copy
    }
}

/// Deep duplicate an SEXP.
pub unsafe fn duplicate(s: SEXP) -> SEXP {
    unsafe {
        let t = duplicate1(s, 1);
        trace_duplication(s, t);
        t
    }
}

/// Alias for duplicate (R API).
pub unsafe fn Rf_duplicate(s: SEXP) -> SEXP {
    unsafe { duplicate(s) }
}

/// Shallow duplicate an SEXP.
pub unsafe fn shallow_duplicate(s: SEXP) -> SEXP {
    unsafe {
        let t = duplicate1(s, 0);
        trace_duplication(s, t);
        t
    }
}

/// Copy before attribute mutation when the object may be referenced.
///
/// GNU `do_attrgets` uses `MAYBE_SHARED` (`NAMED >= 2`). This port does
/// not yet RAISE_NAMED on formal binding, so a caller-owned value often
/// still has NAMED==1 inside `f(x)`. Use MAYBE_REFERENCED (`NAMED > 0`)
/// until argument matching increments NAMED.
pub unsafe fn shallow_duplicate_if_shared(s: SEXP) -> SEXP {
    unsafe {
        if s.is_null() || s == R_NilValue() {
            return s;
        }
        if NAMED(s) > 0 {
            shallow_duplicate(s)
        } else {
            s
        }
    }
}

/// Lazy duplicate: just set NAMEDMAX on the input.
/// Returns the input unchanged (no copy is made).
pub unsafe fn lazy_duplicate(s: SEXP) -> SEXP {
    unsafe {
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
}

/// Helper: call duplicate1 or lazy_duplicate based on deep flag.
unsafe fn duplicate_child(s: SEXP, deep: c_int) -> SEXP {
    unsafe {
        if deep != 0 {
            duplicate1(s, 1)
        } else {
            lazy_duplicate(s)
        }
    }
}

// ---------------------------------------------------------------------------
// Cycle detection
// ---------------------------------------------------------------------------

/// Detect cycles that would be created by assigning `child` as a
/// component of `s` in a complex assignment.
pub unsafe fn R_cycle_detected(s: SEXP, child: SEXP) -> c_int {
    unsafe {
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
                if !el_attr.is_null()
                    && el_attr != R_NilValue()
                    && R_cycle_detected(s, el_attr) != 0
                {
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
}

// ---------------------------------------------------------------------------
// Pairlist duplication
// ---------------------------------------------------------------------------

/// Duplicate a pairlist (LISTSXP/LANGSXP/DOTSXP).
unsafe fn duplicate_list(s: SEXP, deep: c_int) -> SEXP {
    unsafe {
        let mut val: SEXP = R_NilValue();
        let mut root = crate::sexp::protect::protect(val);

        // First pass: build the skeleton list
        let mut sp = s;
        while !sp.is_null() && sp != R_NilValue() {
            val = Rf_cons(R_NilValue(), val);
            root = crate::sexp::protect::protect(val);
            sp = CDR(sp);
        }

        // Second pass: fill in CAR, TAG, and ATTRIB
        sp = s;
        let mut vp = val;
        while !sp.is_null() && sp != R_NilValue() {
            SETCAR(vp, duplicate_child(CAR(sp), deep));
            COPY_TAG(vp, sp);
            DUPLICATE_ATTRIB(vp, sp, deep);
            sp = CDR(sp);
            vp = CDR(vp);
        }

        drop(root);
        val
    }
}

// ---------------------------------------------------------------------------
// copyVector
// ---------------------------------------------------------------------------

/// Copy the contents of vector `t` into vector `s`.
///
/// Both vectors must have the same type. The source `t` is recycled
/// into the destination `s` if it is shorter.
pub unsafe fn copyVector(s: SEXP, t: SEXP) {
    unsafe {
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
                xcopyLogicalWithRecycle(LOGICAL(s), LOGICAL(t), 0, ns, nt);
            }
            SEXPTYPE::INTSXP => {
                xcopyIntegerWithRecycle(INTEGER(s), INTEGER(t), 0, ns, nt);
            }
            SEXPTYPE::REALSXP => {
                xcopyRealWithRecycle(REAL(s), REAL(t), 0, ns, nt);
            }
            SEXPTYPE::CPLXSXP => {
                xcopyComplexWithRecycle(COMPLEX(s), COMPLEX(t), 0, ns, nt);
            }
            SEXPTYPE::EXPRSXP | SEXPTYPE::VECSXP => {
                xcopyVectorWithRecycle(s, t, 0, ns, nt);
            }
            SEXPTYPE::RAWSXP => {
                xcopyRawWithRecycle(RAW(s), RAW(t), 0, ns, nt);
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"copyVector\0".as_ptr() as *const c_char, s);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// copyListMatrix (legacy, no longer used by R)
// ---------------------------------------------------------------------------

/// Copy list matrix contents (legacy function).
#[allow(clippy::never_loop)]
pub unsafe fn copyListMatrix(s: SEXP, t: SEXP, byrow: c_int) {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// copyMatrix
// ---------------------------------------------------------------------------

/// Helper for VECTOR_ELT_LD: returns lazy_duplicate of the element.
unsafe fn VECTOR_ELT_LD(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe { lazy_duplicate(VECTOR_ELT(x, i)) }
}

/// Copy matrix contents from `t` into `s`.
///
/// If `byrow` is nonzero, fills by rows; otherwise fills by columns
/// (which is equivalent to copyVector).
pub unsafe fn copyMatrix(s: SEXP, t: SEXP, byrow: c_int) {
    unsafe {
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
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *LOGICAL(s).add(didx as usize) = *LOGICAL(t).add(sidx as usize);
                    });
                }
                SEXPTYPE::INTSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *INTEGER(s).add(didx as usize) = *INTEGER(t).add(sidx as usize);
                    });
                }
                SEXPTYPE::REALSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *REAL(s).add(didx as usize) = *REAL(t).add(sidx as usize);
                    });
                }
                SEXPTYPE::CPLXSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *COMPLEX(s).add(didx as usize) = *COMPLEX(t).add(sidx as usize);
                    });
                }
                SEXPTYPE::EXPRSXP | SEXPTYPE::VECSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        SET_VECTOR_ELT(s, didx, VECTOR_ELT_LD(t, sidx));
                    });
                }
                SEXPTYPE::RAWSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *RAW(s).add(didx as usize) = *RAW(t).add(sidx as usize);
                    });
                }
                _ => {
                    UNIMPLEMENTED_TYPE(b"copyMatrix\0".as_ptr() as *const c_char, s);
                }
            }
        } else {
            copyVector(s, t);
        }
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
pub unsafe fn xcopyStringWithRecycle(
    dst: SEXP,
    src: SEXP,
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
}

/// Copy generic vector elements with recycling.
pub unsafe fn xcopyVectorWithRecycle(
    dst: SEXP,
    src: SEXP,
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
pub unsafe fn xfillStringMatrixWithRecycle(
    dst: SEXP,
    src: SEXP,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            SET_STRING_ELT(dst, didx, STRING_ELT(src, sidx));
        });
    }
}

/// Fill generic vector matrix with recycling.
pub unsafe fn xfillVectorMatrixWithRecycle(
    dst: SEXP,
    src: SEXP,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            SET_VECTOR_ELT(dst, didx, VECTOR_ELT(src, sidx));
        });
    }
}

// ---------------------------------------------------------------------------
// duplicate_attr: duplicate before attribute modification
// ---------------------------------------------------------------------------

/// Duplicate before attribute modification using ordinary deep or shallow
/// value and public-attribute copying.
unsafe fn duplicate_attr(x: SEXP, deep: c_int) -> SEXP {
    unsafe {
        if x.is_null() {
            return x;
        }
        if deep != 0 {
            duplicate(x)
        } else {
            shallow_duplicate(x)
        }
    }
}

/// Shallow duplicate before attribute modification.
pub unsafe fn R_shallow_duplicate_attr(x: SEXP) -> SEXP {
    unsafe { duplicate_attr(x, 0) }
}

/// Deep duplicate before attribute modification.
pub unsafe fn R_duplicate_attr(x: SEXP) -> SEXP {
    unsafe { duplicate_attr(x, 1) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "duplicate/altrep_tests.rs"]
mod altrep_tests;

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
            .wrap(unsafe { crate::sexp::symbol::Rf_install(c"+".as_ptr()) })
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
                let owner = crate::sexp::owner::OwnerToken::current().unwrap();
                (*owner.as_ptr()).memory_state.gc_force_gap = 0;
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
        let copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
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

    struct CollectingExternalChild {
        value: i32,
        reads: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl crate::sexp::altrep::AltrepClass for CollectingExternalChild {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::INTSXP
        }

        fn length(
            &self,
            _: &crate::sexp::altrep::AltrepContext<'_>,
        ) -> crate::sexp::object::SexpResult<i64> {
            Ok(1)
        }

        fn element<'s>(
            &self,
            context: &crate::sexp::altrep::AltrepContext<'s>,
            _: i64,
        ) -> crate::sexp::object::SexpResult<crate::sexp::altrep::AltrepElement<'s>> {
            self.reads.set(self.reads.get() + 1);
            context.gc()?;
            Ok(crate::sexp::altrep::AltrepElement::Integer(self.value))
        }
    }

    fn external_pointer_duplicate_survives_collecting_children(deep: bool) {
        use crate::sexp::{
            altrep::{AltrepBuilder, is_altrep},
            object::Sexp,
            session::RSession,
        };
        use std::{cell::Cell, rc::Rc};

        let session = RSession::new_for_gc_tests();
        let reads = [
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
        ];
        let children: Vec<_> = reads
            .iter()
            .enumerate()
            .map(|(index, reads)| {
                let class = session
                    .register_altrep_class(
                        &format!("external-child-{index}"),
                        CollectingExternalChild {
                            value: 41 + index as i32,
                            reads: reads.clone(),
                        },
                    )
                    .unwrap();
                AltrepBuilder::new(class).build().unwrap()
            })
            .collect();
        let factory = children[0].node_factory().unwrap();
        let mut attribute_builder = crate::sexp::object::PairlistBuilder::from_factory(factory);
        attribute_builder.push(children[2].clone(), None).unwrap();
        let attributes = attribute_builder.finish().unwrap();
        let attribute_tag = unsafe { crate::sexp::symbol::Rf_install(c"payload".as_ptr()) };
        let mut opaque = 17_i32;
        let address = std::ptr::from_mut(&mut opaque).cast();
        let source = session
            .sexp(unsafe {
                crate::mainutils::memory_main::R_MakeExternalPtr(
                    address,
                    children[1].as_raw(),
                    children[0].as_raw(),
                )
            })
            .unwrap();
        unsafe {
            SETTAG(attributes.as_raw(), attribute_tag);
            SET_ATTRIB(source.as_raw(), attributes.as_raw());
            SET_OBJECT(source.as_raw(), 1);
            SET_S4_OBJECT(source.as_raw());
        }
        let attributes_raw = attributes.as_raw();
        let source_raw = source.as_raw();
        let source_token = crate::sexp::memory::checked_projection(source_raw)
            .unwrap()
            .1;
        let original_children: Vec<_> = children
            .iter()
            .map(|child| {
                let projection = child.as_raw();
                (
                    projection,
                    crate::sexp::memory::checked_projection(projection)
                        .unwrap()
                        .1,
                )
            })
            .collect();
        let active = Rc::new(Cell::new(true));
        let callback_active = active.clone();
        let notifications = Rc::new(Cell::new(0));
        let callback_notifications = notifications.clone();
        let callback_children = original_children.clone();
        let rejected_reads = Rc::new(Cell::new(0));
        let callback_rejected_reads = rejected_reads.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if !callback_active.get() {
                return;
            }
            callback_notifications.set(callback_notifications.get() + 1);
            crate::sexp::gengc::full_gc();
            for (index, (projection, token)) in callback_children.iter().enumerate() {
                assert!(
                    token.is_live(),
                    "original edge must retain its saved allocation"
                );
                // Query through the checked interface: a provider currently
                // being duplicated must reject recursive entry before it can
                // lend its state twice. Other reads still collect for real.
                // SAFETY: the saved allocation is live and rooted by the
                // source graph in this notification's original owner.
                let value = unsafe { crate::sexp::altrep::rooted_raw(*projection) }.unwrap();
                match value.try_integer_elt(0) {
                    Ok(actual) => assert_eq!(actual, 41 + index as i32),
                    Err(crate::sexp::object::SexpError::Altrep {
                        reason: "recursive ALTREP operation",
                    }) if deep => {
                        callback_rejected_reads.set(callback_rejected_reads.get() + 1);
                    }
                    Err(error) => panic!("unexpected callback read failure: {error}"),
                }
            }
        }));
        // Only the canonical source graph retains the input children now; the
        // implementation must root that source before its first allocation.
        drop(source);
        drop(attributes);
        drop(children);
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let protected_count = crate::sexp::protect::R_ProtectCount();
        let copied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                if deep {
                    duplicate(source_raw)
                } else {
                    shallow_duplicate(source_raw)
                }
            })).unwrap_or_else(|error| {
                if let Some(error) = error.downcast_ref::<crate::sexp::context::RError>() {
                    panic!("external pointer copy failed: {}", error.message);
                }
                std::panic::resume_unwind(error)
            });
        let output: Sexp<'_> = session
            .sexp(copied)
            .unwrap();
        active.set(false);
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 0;
        });
        assert_eq!(crate::sexp::protect::R_ProtectCount(), protected_count);
        assert!(notifications.get() > 0);
        assert_eq!(rejected_reads.get() > 0, deep);
        assert_ne!(output.as_raw(), source_raw);
        assert_eq!(output.try_extptr_ptr().unwrap(), address);
        assert_eq!(opaque, 17);
        crate::sexp::gengc::full_gc();
        assert!(!source_token.is_live());
        let copied_children = [
            output.try_extprot().unwrap(),
            output.try_extptr_tag().unwrap(),
            output.try_attrib().unwrap().try_car().unwrap(),
        ];
        for (index, child) in copied_children.iter().enumerate() {
            assert_eq!(child.try_integer_elt(0).unwrap(), 41 + index as i32);
            assert!(reads[index].get() > 0);
            if deep {
                assert_ne!(child.as_raw(), original_children[index].0);
                assert!(!is_altrep(child));
            } else {
                assert_eq!(child.as_raw(), original_children[index].0);
                assert!(is_altrep(child));
            }
        }
        let copied_attributes = output.try_attrib().unwrap();
        assert_ne!(copied_attributes.as_raw(), attributes_raw);
        assert_eq!(copied_attributes.try_tag().unwrap().as_raw(), attribute_tag);
        unsafe {
            assert_eq!(OBJECT(output.as_raw()), 1);
            assert_ne!(IS_S4_OBJECT(output.as_raw()), 0);
        }
    }

    #[test]
    fn deep_external_pointer_duplicate_roots_result_through_collecting_providers() {
        external_pointer_duplicate_survives_collecting_children(true);
    }

    #[test]
    fn shallow_external_pointer_duplicate_roots_shared_children_through_reentrant_gc() {
        external_pointer_duplicate_survives_collecting_children(false);
    }

    #[test]
    fn external_pointer_duplicates_share_native_resource_before_callbacks() {
        use crate::sexp::{memory, session::RSession};
        use std::{cell::Cell, rc::Rc};
        struct Resource(Rc<Cell<usize>>);
        impl Drop for Resource {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        for deep in [false, true] {
            let session = RSession::new_for_gc_tests();
            let factory = session.owner_token().unwrap().node_factory();
            let protected_count = crate::sexp::protect::R_ProtectCount();
            let drops = Rc::new(Cell::new(0));
            let resource = Rc::new(Resource(drops.clone()));
            let weak = Rc::downgrade(&resource);
            let address = Rc::as_ptr(&resource).cast_mut().cast();
            let source = factory
                .allocate(|arena| {
                    let pointer = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
                    let node = arena.node_token(pointer)?;
                    let heap = arena.heap_identity();
                    let mut header = heap.node_snapshot(&node)?;
                    header.data.extptr_mut().address = address;
                    heap.replace_node(&node, header)?;
                    heap.attach_resource(&node, resource.clone())?;
                    Some(pointer)
                })
                .unwrap();
            drop(resource);
            let (_, source_node) = memory::checked_projection(source.as_raw()).unwrap();
            let heap = source_node.heap_identity();
            let source_link = source_node.link().unwrap();
            let notifications = Rc::new(Cell::new(0));
            let observed = notifications.clone();
            let active = Rc::new(Cell::new(true));
            let callback_active = active.clone();
            let callback_heap = heap.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !callback_active.get() {
                    return;
                }
                let (_, copy_node) = memory::automatic_roots(&callback_heap)
                    .into_iter()
                    .find(|(_, node)| {
                        node.link() != Some(source_link)
                            && callback_heap.node_snapshot(node).is_some_and(|header| {
                                header.sxpinfo.type_of() == SEXPTYPE::EXTPTRSXP
                            })
                    })
                    .expect("duplicate is rooted before allocation callbacks");
                assert!(callback_heap.resource::<Resource>(&copy_node).is_some());
                crate::sexp::gengc::full_gc();
                assert!(copy_node.is_live());
                observed.set(observed.get() + 1);
            }));
            session.with_active_in(|owner| unsafe {
                (*owner).memory_state.gc_force_gap = 1;
                (*owner).memory_state.gc_force_wait = 1;
            });
            let pointer = unsafe {
                if deep {
                    duplicate(source.as_raw())
                } else {
                    shallow_duplicate(source.as_raw())
                }
            };
            let copy = factory.wrap(pointer).unwrap();
            active.set(false);
            session.with_active_in(|owner| unsafe {
                (*owner).memory_state.gc_force_gap = 0;
            });
            assert!(notifications.get() > 0);
            assert_eq!(copy.try_extptr_ptr().unwrap(), address);
            // Explicitly closing one attachment does not revoke its duplicate.
            drop(heap.take_resource(&source_node).unwrap());
            drop(source);
            crate::sexp::gengc::full_gc();
            assert!(!source_node.is_live());
            assert_eq!(drops.get(), 0);
            let (_, copy_node) = memory::checked_projection(copy.as_raw()).unwrap();
            assert!(heap.resource::<Resource>(&copy_node).is_some());
            drop(copy);
            crate::sexp::gengc::full_gc();
            assert!(!copy_node.is_live());
            assert_eq!(drops.get(), 1);
            assert!(weak.upgrade().is_none());
            assert_eq!(crate::sexp::protect::R_ProtectCount(), protected_count);
        }
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
            let mut arena = RArena::new();
            let formals = arena.alloc_node(SEXPTYPE::NILSXP);
            let body = Rf_ScalarInteger(42);
            let env = arena.alloc_node(SEXPTYPE::ENVSXP);

            // Create closure using mkCLOSXP
            let c = crate::mainutils::dstruct::mkCLOSXP(formals, body, env);
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
            let s1 = Rf_allocVector3(SEXPTYPE::CHARSXP, 0); // placeholder CHARSXP
            let s2 = Rf_allocVector3(SEXPTYPE::CHARSXP, 0);
            let v = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
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
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::EXTPTRSXP);
            assert_ne!(d, extptr);
        }
    }
}
