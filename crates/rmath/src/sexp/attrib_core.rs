#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Core attribute operations — ports parts of R's attrib.c.
//!
//! Provides the subset of attrib.c needed for method dispatch:
//! - getAttrib: get an attribute from an object
//! - setAttrib: set an attribute on an object
//! - isObject: check if an object has a "class" attribute
//! - R_classgets: set the class of an object

use std::os::raw::c_int;

use super::accessors::{
    ATTRIB, CAR, CDR, SET_ATTRIB, SETCAR, SETCDR, STRING_ELT, TAG, TYPEOF, XLENGTH,
};

use super::constructors::*;
use super::ffi::{SEXP, SEXPTYPE};
use super::globals::R_NilValue;
use super::symbol::Rf_install;

#[cfg(test)]
#[path = "names_atomic_tests.rs"]
mod names_atomic_tests;

#[cfg(test)]
#[path = "tsp_tests.rs"]
mod tsp_tests;

// ---------------------------------------------------------------------------
// Pre-interned attribute name symbols
// ---------------------------------------------------------------------------

/// Get the "class" symbol.
pub unsafe fn R_ClassSymbol() -> SEXP {
    unsafe { Rf_install(c"class".as_ptr()) }
}

/// Get the "names" symbol.
pub unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { Rf_install(c"names".as_ptr()) }
}

/// Get the "dim" symbol.
pub unsafe fn R_DimSymbol() -> SEXP {
    unsafe { Rf_install(c"dim".as_ptr()) }
}

/// Get the "dimnames" symbol.
pub unsafe fn R_DimNamesSymbol() -> SEXP {
    unsafe { Rf_install(c"dimnames".as_ptr()) }
}

/// Get the "levels" symbol.
pub unsafe fn R_LevelsSymbol() -> SEXP {
    unsafe { Rf_install(c"levels".as_ptr()) }
}

/// Get the "tsp" symbol.
pub unsafe fn R_TspSymbol() -> SEXP {
    unsafe { Rf_install(c"tsp".as_ptr()) }
}

/// Get the "srcref" symbol.
pub unsafe fn R_SrcRefSymbol() -> SEXP {
    unsafe { Rf_install(c"srcref".as_ptr()) }
}

/// Get the "srcfile" symbol.
pub unsafe fn R_SrcFileSymbol() -> SEXP {
    unsafe { Rf_install(c"srcfile".as_ptr()) }
}

/// Get the "row.names" symbol.
pub unsafe fn R_RowNamesSymbol() -> SEXP {
    unsafe { Rf_install(c"row.names".as_ptr()) }
}

/// Get the ".Environment" symbol.
pub unsafe fn R_EnvironmentSymbol() -> SEXP {
    unsafe { Rf_install(c".Environment".as_ptr()) }
}

// ---------------------------------------------------------------------------
// getAttrib — get an attribute value
// ---------------------------------------------------------------------------

/// Get the value of an attribute from an object.
///
/// This is the equivalent of R's `getAttrib()` from attrib.c.
/// Searches the attribute pairlist for a matching symbol.
pub unsafe fn getAttrib(x: SEXP, which: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || which.is_null() {
            return R_NilValue();
        }

        let attrib = ATTRIB(x);
        // GNU resolves names of a one-dimensional array through dimnames
        // before consulting pairlist tags or stored attributes.
        if which == R_NamesSymbol() && !attrib.is_null() && attrib != R_NilValue() {
            let dim = getAttrib(x, R_DimSymbol());
            if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 1 {
                let dimnames = getAttrib(x, R_DimNamesSymbol());
                if TYPEOF(dimnames) == SEXPTYPE::VECSXP && XLENGTH(dimnames) > 0 {
                    return super::accessors::VECTOR_ELT(dimnames, 0);
                }
            }
        }
        // Linear search through attribute pairlist
        let mut current = attrib;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == which {
                let value = CAR(current);
                if which == R_RowNamesSymbol() {
                    return expand_compact_row_names(value);
                }
                return value;
            }
            current = CDR(current);
        }
        if which == R_NamesSymbol() {
            let t = TYPEOF(x);
            if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::DOTSXP {
                let mut n = 0i32;
                let mut any = false;
                let mut scan = x;
                while !scan.is_null() && scan != R_NilValue() {
                    let tag = TAG(scan);
                    if !tag.is_null() && tag != R_NilValue() {
                        any = true;
                    }
                    n += 1;
                    scan = CDR(scan);
                }
                if n > 0 && any {
                    let out = Rf_allocVector(SEXPTYPE::STRSXP, n);
                    let _g = super::protect::protect(out);
                    let mut cell = x;
                    let mut i = 0i64;
                    while !cell.is_null() && cell != R_NilValue() && i < n as i64 {
                        let tag = TAG(cell);
                        let s = if tag.is_null() || tag == R_NilValue() {
                            Rf_mkChar(b"\0".as_ptr() as *const std::os::raw::c_char)
                        } else {
                            super::accessors::PRINTNAME(tag)
                        };
                        super::accessors::SET_STRING_ELT(out, i, s);
                        cell = CDR(cell);
                        i += 1;
                    }
                    return out;
                }
            }
        }

        if which == R_NamesSymbol() {
            return names_from_one_dim(x);
        }

        R_NilValue()
    }
}

/// GNU attrib.c getAttrib0: compact `c(NA, ±n)` expands to `1:n`.
unsafe fn expand_compact_row_names(value: SEXP) -> SEXP {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(value) == SEXPTYPE::INTSXP && XLENGTH(value) == 2 {
            let first = *super::accessors::INTEGER(value);
            let second = *super::accessors::INTEGER(value).add(1);
            if first == super::ffi::NA_INTEGER && second != 0 {
                let n = second.unsigned_abs() as usize;
                let expanded = Rf_allocVector(SEXPTYPE::INTSXP, n as i32);
                let _g = super::protect::protect(expanded);
                let dst = super::accessors::INTEGER(expanded);
                for i in 0..n {
                    *dst.add(i) = (i as i32) + 1;
                }
                return expanded;
            }
        }
        value
    }
}
/// GNU `getAttrib`: a length-1 `dim` exposes `dimnames[[1]]` as `names`.
unsafe fn names_from_one_dim(x: SEXP) -> SEXP {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        if TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) != 1 {
            return R_NilValue();
        }
        let dn = getAttrib(x, R_DimNamesSymbol());
        if TYPEOF(dn) != SEXPTYPE::VECSXP || XLENGTH(dn) < 1 {
            return R_NilValue();
        }
        let first = super::accessors::VECTOR_ELT(dn, 0);
        if first.is_null() || TYPEOF(first) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        first
    }
}

// ---------------------------------------------------------------------------
// setAttrib — set an attribute value
// ---------------------------------------------------------------------------

/// GNU `installAttrib` — store a named attribute without dim/names/class
/// special cases. `R_do_slot_assign` uses this so a slot named `"dim"` can
/// hold a character vector (reg-S4.R reserved slot names).
pub unsafe fn installAttrib(vec: SEXP, name: SEXP, val: SEXP) {
    unsafe {
        if vec.is_null() || name.is_null() {
            return;
        }
        let attrib = ATTRIB(vec);
        let mut previous = R_NilValue();
        let mut current = attrib;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == name {
                SETCAR(current, val);
                return;
            }
            previous = current;
            current = CDR(current);
        }
        let _vec_guard = super::protect::protect(vec);
        let _name_guard = super::protect::protect(name);
        let _val_guard = super::protect::protect(val);
        let new_attr = Rf_cons(val, R_NilValue());
        if new_attr.is_null() {
            return;
        }
        super::accessors::SETTAG(new_attr, name);
        if attrib.is_null() || attrib == R_NilValue() {
            SET_ATTRIB(vec, new_attr);
        } else {
            SETCDR(previous, new_attr);
        }
    }
}

/// Audited raw entry: snapshots checked owners before any provider or GC.
unsafe fn run_tsp<T>(
    x: SEXP,
    value: SEXP,
    preflight: impl FnOnce(&super::object::Sexp<'static>, &super::object::Sexp<'static>) -> Option<T>,
    operation: impl FnOnce(
        &super::owner::RuntimeAccess,
        &super::object::Sexp<'static>,
        &super::object::Sexp<'static>,
    ) -> Result<T, String>,
) -> T {
    unsafe {
        let fail =
            |message: String| -> ! { std::panic::panic_any(super::context::RError { message }) };
        let token =
            super::owner::OwnerToken::current().unwrap_or_else(|error| fail(error.to_string()));
        let authority = super::owner::StoredOwner::from_token(token);
        let original = authority
            .managed()
            .unwrap_or_else(|| fail("runtime owner unavailable".into()));
        let _pin = original
            .pin()
            .unwrap_or_else(|error| fail(error.to_string()));
        // Both roots belong to the same original domain; capture that domain
        // once instead of rebuilding a factory for each input.
        let factory = token.node_factory();
        let object = factory
            .wrap(x)
            .and_then(super::object::Sexp::into_owned)
            .unwrap_or_else(|error| fail(error.to_string()));
        let input = factory
            .wrap(value)
            .and_then(super::object::Sexp::into_owned)
            .unwrap_or_else(|error| fail(error.to_string()));
        // This callback only inspects rooted headers: it cannot allocate or
        // reenter R. Check authority before the fast-path decision.
        authority
            .require_active()
            .unwrap_or_else(|error| fail(error.to_string()));
        if let Some(result) = preflight(&object, &input) {
            return result;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::owner::with_runtime(&original, |access| operation(access, &object, &input))
                .map_err(|error| error.to_string())?
        }));
        authority
            .require_active()
            .unwrap_or_else(|error| fail(error.to_string()));
        match result {
            Ok(result) => result.unwrap_or_else(|error| fail(error)),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

// Dense scalar contents are copied without entering an ALTREP provider. This
// lets publication detect an in-place mutation that preserves the CAR identity.
#[derive(PartialEq)]
enum PairlistNameScalar {
    Opaque,
    Empty,
    Integer(i32),
    Real(u64),
    Complex(u64, u64),
    Raw(u8),
    Reference(super::object::Sexp<'static>),
}

fn pairlist_name_scalar(
    value: &super::object::Sexp<'static>,
) -> Result<PairlistNameScalar, String> {
    use PairlistNameScalar as Snapshot;
    if super::altrep::is_altrep(value) {
        return Ok(Snapshot::Opaque);
    }
    if value.len() == 0 {
        return Ok(Snapshot::Empty);
    }
    let err = |e: super::object::SexpError| e.to_string();
    Ok(match value.typeof_() {
        SEXPTYPE::LGLSXP => Snapshot::Integer(value.try_logical_elt(0).map_err(err)?),
        SEXPTYPE::INTSXP => Snapshot::Integer(value.try_integer_elt(0).map_err(err)?),
        SEXPTYPE::REALSXP => Snapshot::Real(value.try_real_elt(0).map_err(err)?.to_bits()),
        SEXPTYPE::CPLXSXP => {
            let x = value.try_complex_elt(0).map_err(err)?;
            Snapshot::Complex(x.r.to_bits(), x.i.to_bits())
        }
        SEXPTYPE::RAWSXP => Snapshot::Raw(value.try_raw_elt(0).map_err(err)?),
        SEXPTYPE::STRSXP => Snapshot::Reference(
            value
                .try_string_elt(0)
                .and_then(super::object::Sexp::into_owned)
                .map_err(err)?,
        ),
        SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => Snapshot::Reference(
            value
                .try_vector_elt(0)
                .and_then(super::object::Sexp::into_owned)
                .map_err(err)?,
        ),
        _ => Snapshot::Opaque,
    })
}

/// GNU's direct `names` attribute path treats a vectorizable pairlist
/// differently from `names<-`: it builds a target-sized character vector,
/// truncating excess elements and leaving empty strings in unused slots.
pub(crate) unsafe fn try_set_pairlist_names(x: SEXP, which: SEXP, value: SEXP) -> bool {
    unsafe {
        if which != R_NamesSymbol()
            || value.is_null()
            || value == R_NilValue()
            || TYPEOF(value) != SEXPTYPE::LISTSXP
        {
            return false;
        }
        run_tsp(
            x,
            value,
            |object, input| {
                if object.is_s4()
                    || input.typeof_() != SEXPTYPE::LISTSXP
                    || !matches!(
                        object.typeof_(),
                        SEXPTYPE::LGLSXP
                            | SEXPTYPE::INTSXP
                            | SEXPTYPE::REALSXP
                            | SEXPTYPE::CPLXSXP
                            | SEXPTYPE::STRSXP
                            | SEXPTYPE::RAWSXP
                            | SEXPTYPE::VECSXP
                            | SEXPTYPE::EXPRSXP
                            | SEXPTYPE::LISTSXP
                            | SEXPTYPE::LANGSXP
                    )
                {
                    Some(false)
                } else {
                    None
                }
            },
            |access, object, input| {
                let domain = access.domain();
                let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
                let cons_target = matches!(object.typeof_(), SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP);
                let mut target_count = 0usize;
                if cons_target {
                    let mut cell = object.clone();
                    let mut seen = super::tsp::CycleDetector::default();
                    while !cell.is_nil() {
                        if !matches!(cell.typeof_(), SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP) {
                            return Err("improper target pairlist in names assignment".into());
                        }
                        seen.observe(domain.link(&cell).map_err(|e| e.to_string())?)?;
                        target_count = target_count.checked_add(1).ok_or("pairlist is too long")?;
                        cell = cell
                            .try_cdr()
                            .and_then(super::object::Sexp::into_owned)
                            .map_err(|e| e.to_string())?;
                    }
                }
                let target_bytes = target_count
                    .checked_mul(std::mem::size_of::<(
                        super::object::Sexp<'static>,
                        super::object::Sexp<'static>,
                        super::object::Sexp<'static>,
                    )>())
                    .ok_or("R allocation failed: memory or node budget exceeded")?;
                let mut target_workspace = access
                    .with_arena(|arena| arena.reserve_workspace(target_bytes))
                    .map_err(|e| e.to_string())?
                    .ok_or("R allocation failed: memory or node budget exceeded")?;
                let mut target_cells = Vec::new();
                target_cells
                    .try_reserve_exact(target_count)
                    .map_err(|_| "R allocation failed: memory or node budget exceeded")?;
                target_workspace
                    .try_resize(
                        target_cells
                            .capacity()
                            .checked_mul(std::mem::size_of::<(
                                super::object::Sexp<'static>,
                                super::object::Sexp<'static>,
                                super::object::Sexp<'static>,
                            )>())
                            .ok_or("R allocation failed: memory or node budget exceeded")?,
                    )
                    .ok_or("R allocation failed: memory or node budget exceeded")?;
                let mut target_cursor = object.clone();
                for _ in 0..target_count {
                    let tag = target_cursor
                        .try_tag()
                        .and_then(super::object::Sexp::into_owned)
                        .map_err(|e| e.to_string())?;
                    let car = target_cursor
                        .try_car()
                        .and_then(super::object::Sexp::into_owned)
                        .map_err(|e| e.to_string())?;
                    let next = target_cursor
                        .try_cdr()
                        .and_then(super::object::Sexp::into_owned)
                        .map_err(|e| e.to_string())?;
                    target_cells.push((target_cursor, tag, car));
                    target_cursor = next;
                }
                let dimensions = super::tsp::attribute(access, object, b"dim")?;
                let rank = if dimensions.is_nil() {
                    None
                } else {
                    if dimensions.typeof_() != SEXPTYPE::INTSXP {
                        return Ok(false);
                    }
                    Some(dimensions.len())
                };
                let size = if cons_target {
                    i64::try_from(target_count).map_err(|_| "pairlist is too long")?
                } else {
                    object.len()
                };
                let mut cursor = input.clone();
                let mut seen = super::tsp::CycleDetector::default();
                let mut count = 0i64;
                while !cursor.is_nil() {
                    if cursor.typeof_() != SEXPTYPE::LISTSXP {
                        return Err("improper pairlist in names assignment".into());
                    }
                    seen.observe(domain.link(&cursor).map_err(|e| e.to_string())?)?;
                    let child = cursor
                        .try_car()
                        .and_then(super::object::Sexp::into_owned)
                        .map_err(|e| e.to_string())?;
                    if !child.is_vector() || child.len() > 1 {
                        return Err("incompatible 'names' argument".into());
                    }
                    count = count.checked_add(1).ok_or("pairlist is too long")?;
                    cursor = cursor
                        .try_cdr()
                        .and_then(super::object::Sexp::into_owned)
                        .map_err(|e| e.to_string())?;
                }
                let capacity = usize::try_from(count).map_err(|_| "pairlist is too long")?;
                let bytes = capacity
                    .checked_mul(std::mem::size_of::<(
                        super::object::Sexp<'static>,
                        PairlistNameScalar,
                    )>())
                    .ok_or("R allocation failed: memory or node budget exceeded")?;
                let mut workspace = access
                    .with_arena(|arena| arena.reserve_workspace(bytes))
                    .map_err(|e| e.to_string())?
                    .ok_or("R allocation failed: memory or node budget exceeded")?;
                let mut snapshot = Vec::new();
                snapshot
                    .try_reserve_exact(capacity)
                    .map_err(|_| "R allocation failed: memory or node budget exceeded")?;
                let actual = snapshot
                    .capacity()
                    .checked_mul(std::mem::size_of::<(
                        super::object::Sexp<'static>,
                        PairlistNameScalar,
                    )>())
                    .ok_or("R allocation failed: memory or node budget exceeded")?;
                workspace
                    .try_resize(actual)
                    .ok_or("R allocation failed: memory or node budget exceeded")?;
                cursor = input.clone();
                let mut seen = super::tsp::CycleDetector::default();
                for _ in 0..count {
                    if cursor.is_nil() || cursor.typeof_() != SEXPTYPE::LISTSXP {
                        return Err("pairlist changed during names assignment".into());
                    }
                    seen.observe(domain.link(&cursor).map_err(|e| e.to_string())?)?;
                    let child = cursor
                        .try_car()
                        .and_then(super::object::Sexp::into_owned)
                        .map_err(|e| e.to_string())?;
                    if !child.is_vector() || child.len() > 1 {
                        return Err("incompatible 'names' argument".into());
                    }
                    let contents = pairlist_name_scalar(&child)?;
                    snapshot.push((child, contents));
                    cursor = cursor
                        .try_cdr()
                        .and_then(super::object::Sexp::into_owned)
                        .map_err(|e| e.to_string())?;
                }
                if !cursor.is_nil() {
                    return Err("pairlist changed during names assignment".into());
                }
                let validate = || -> Result<(), String> {
                    access.require_active().map_err(|e| e.to_string())?;
                    if (!cons_target && object.len() != size)
                        || super::tsp::attribute(access, object, b"dim")? != dimensions
                        || rank.is_some_and(|n| dimensions.len() != n)
                    {
                        return Err("target changed during names assignment".into());
                    }
                    if cons_target {
                        let mut cell = object.clone();
                        for (saved, tag, car) in &target_cells {
                            if cell != *saved
                                || cell
                                    .try_tag()
                                    .and_then(super::object::Sexp::into_owned)
                                    .map_err(|e| e.to_string())?
                                    != *tag
                                || cell
                                    .try_car()
                                    .and_then(super::object::Sexp::into_owned)
                                    .map_err(|e| e.to_string())?
                                    != *car
                            {
                                return Err("target changed during names assignment".into());
                            }
                            cell = cell
                                .try_cdr()
                                .and_then(super::object::Sexp::into_owned)
                                .map_err(|e| e.to_string())?;
                        }
                        if !cell.is_nil() {
                            return Err("target changed during names assignment".into());
                        }
                    }
                    let mut cell = input.clone();
                    let mut seen = super::tsp::CycleDetector::default();
                    for index in 0..count {
                        if cell.is_nil() || cell.typeof_() != SEXPTYPE::LISTSXP {
                            return Err("pairlist changed during names assignment".into());
                        }
                        seen.observe(domain.link(&cell).map_err(|e| e.to_string())?)?;
                        let current = cell
                            .try_car()
                            .and_then(super::object::Sexp::into_owned)
                            .map_err(|e| e.to_string())?;
                        let (saved, contents) = &snapshot[index as usize];
                        if current != *saved
                            || !current.is_vector()
                            || current.len() > 1
                            || pairlist_name_scalar(&current)? != *contents
                        {
                            return Err("pairlist changed during names assignment".into());
                        }
                        cell = cell
                            .try_cdr()
                            .and_then(super::object::Sexp::into_owned)
                            .map_err(|e| e.to_string())?;
                    }
                    if !cell.is_nil() {
                        return Err("pairlist changed during names assignment".into());
                    }
                    access.require_active().map_err(|e| e.to_string())
                };
                validate()?;
                let output = allocator
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, size)))
                    .map_err(names_allocation_error)?;
                let mut output =
                    super::object::SexpMut::try_from_checked(output).map_err(|e| e.to_string())?;
                let empty = if size > count {
                    Some(allocator.character("").map_err(names_allocation_error)?)
                } else {
                    None
                };
                for index in 0..size {
                    let character = if index >= count {
                        empty.as_ref().expect("padding character").clone()
                    } else {
                        let child = snapshot[index as usize].0.clone();
                        let converted = access
                            .with_native(|owner| {
                                let pointer = crate::mainutils::coerce::coerceVector(
                                    child.as_raw(),
                                    SEXPTYPE::STRSXP.as_c_int(),
                                );
                                owner.sexp(pointer)?.into_owned()
                            })
                            .map_err(|e| e.to_string())?;
                        access.require_active().map_err(|e| e.to_string())?;
                        if converted.len() == 0 {
                            return Err(
                                "Value of SET_STRING_ELT() must be a 'CHARSXP' not a 'NULL'".into(),
                            );
                        }
                        let character = converted
                            .try_string_elt(0)
                            .and_then(super::object::Sexp::into_owned)
                            .map_err(|e| e.to_string())?;
                        access.require_active().map_err(|e| e.to_string())?;
                        character
                    };
                    output
                        .try_set_string_elt(index, character)
                        .map_err(|e| e.to_string())?;
                }
                let output = output.freeze();
                if cons_target && rank != Some(1) {
                    let bytes = target_count
                        .checked_mul(std::mem::size_of::<super::object::Sexp<'static>>())
                        .ok_or("R allocation failed: memory or node budget exceeded")?;
                    let mut tag_workspace = access
                        .with_arena(|arena| arena.reserve_workspace(bytes))
                        .map_err(|e| e.to_string())?
                        .ok_or("R allocation failed: memory or node budget exceeded")?;
                    let mut tags = Vec::new();
                    tags.try_reserve_exact(target_count)
                        .map_err(|_| "R allocation failed: memory or node budget exceeded")?;
                    tag_workspace
                        .try_resize(
                            tags.capacity()
                                .checked_mul(std::mem::size_of::<super::object::Sexp<'static>>())
                                .ok_or("R allocation failed: memory or node budget exceeded")?,
                        )
                        .ok_or("R allocation failed: memory or node budget exceeded")?;
                    for index in 0..size {
                        let character = output
                            .try_string_elt(index)
                            .and_then(super::object::Sexp::into_owned)
                            .map_err(|e| e.to_string())?;
                        let tag = if character.is_na_string()
                            || character.try_char_len().map_err(|e| e.to_string())? == 0
                        {
                            domain.nil()
                        } else {
                            let length = usize::try_from(
                                character.try_char_len().map_err(|e| e.to_string())?,
                            )
                            .map_err(|_| "symbol name is too long")?;
                            let _text_workspace = access
                                .with_arena(|arena| arena.reserve_workspace(length))
                                .map_err(|e| e.to_string())?
                                .ok_or("R allocation failed: memory or node budget exceeded")?;
                            let text = character.try_as_string().map_err(|e| e.to_string())?;
                            access
                                .with_native(|owner| {
                                    let symbol = crate::sexp::symbol::Rf_installChar(
                                        text.as_ptr().cast(),
                                        text.len() as i64,
                                    );
                                    owner.sexp(symbol)?.into_owned()
                                })
                                .map_err(|e| e.to_string())?
                        };
                        tags.push(tag);
                        access.require_active().map_err(|e| e.to_string())?;
                    }
                    validate()?;
                    for ((cell, _, _), tag) in target_cells.iter().zip(&tags) {
                        super::object::SexpMut::try_from_checked(cell.clone())
                            .and_then(|mut cell| cell.try_set_pairlist_tag(tag))
                            .map_err(|e| e.to_string())?;
                    }
                    return Ok(true);
                }
                let one_dimensional = rank == Some(1);
                let name = access
                    .with_native(|owner| {
                        owner
                            .sexp(if one_dimensional {
                                R_DimNamesSymbol()
                            } else {
                                which
                            })?
                            .into_owned()
                    })
                    .map_err(|e| e.to_string())?;
                let published = if one_dimensional {
                    let list = allocator
                        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 1)))
                        .map_err(names_allocation_error)?;
                    let mut list = super::object::SexpMut::try_from_checked(list)
                        .map_err(|e| e.to_string())?;
                    list.try_set_vector_elt(0, if size == 0 { domain.nil() } else { output })
                        .map_err(|e| e.to_string())?;
                    list.freeze()
                } else {
                    output
                };
                super::tsp::install_checked(access, object, &name, &published, validate)?;
                Ok(true)
            },
        )
    }
}

fn names_allocation_error(error: super::object::SexpError) -> String {
    match error {
        super::object::SexpError::AllocationFailed { .. } => {
            "R allocation failed: memory or node budget exceeded".into()
        }
        other => other.to_string(),
    }
}

/// Normalize ordinary, attribute-free atomic names in the original runtime.
/// Classed values and arrays retain their distinct names assignment paths.
pub(crate) unsafe fn try_set_atomic_names(x: SEXP, which: SEXP, value: SEXP) -> bool {
    unsafe {
        if which != R_NamesSymbol() || value.is_null() || value == R_NilValue() {
            return false;
        }
        // Preserve the existing no-callback path for already-sized ordinary
        // character vectors. ALTREP lengths require checked dispatch below.
        let token = super::owner::OwnerToken::current()
            .unwrap_or_else(|error| super::context::r_error(error.to_string()));
        let pin = token
            .pin()
            .unwrap_or_else(|error| super::context::r_error(error.to_string()));
        token
            .require_active()
            .unwrap_or_else(|error| super::context::r_error(error.to_string()));
        if pin.is_some()
            && token
                .node_factory()
                .domain()
                .has_equal_plain_character_input(x, value, which)
        {
            return false;
        }
        run_tsp(
            x,
            value,
            |_, _| None,
            |access, object, input| {
                if object.is_s4()
                    || input.is_s4()
                    || !matches!(
                        object.typeof_(),
                        SEXPTYPE::LGLSXP
                            | SEXPTYPE::INTSXP
                            | SEXPTYPE::REALSXP
                            | SEXPTYPE::CPLXSXP
                            | SEXPTYPE::STRSXP
                            | SEXPTYPE::RAWSXP
                            | SEXPTYPE::VECSXP
                            | SEXPTYPE::EXPRSXP
                    )
                    || !matches!(
                        input.typeof_(),
                        SEXPTYPE::LGLSXP
                            | SEXPTYPE::INTSXP
                            | SEXPTYPE::REALSXP
                            | SEXPTYPE::CPLXSXP
                            | SEXPTYPE::RAWSXP
                            | SEXPTYPE::STRSXP
                    )
                {
                    return Ok(false);
                }
                let input_is_object = input.is_object();
                let dimensions = super::tsp::attribute(access, object, b"dim")?;
                let dimension_rank = if dimensions.is_nil() {
                    None
                } else {
                    if dimensions.typeof_() != SEXPTYPE::INTSXP {
                        return Ok(false);
                    }
                    let rank = dimensions.len();
                    access.require_active().map_err(|error| error.to_string())?;
                    if super::tsp::attribute(access, object, b"dim")? != dimensions {
                        return Err("dimensions changed during names assignment".into());
                    }
                    Some(rank)
                };
                let one_dimensional = dimension_rank == Some(1);
                let validate_dimensions = || -> Result<(), String> {
                    if super::tsp::attribute(access, object, b"dim")? != dimensions {
                        return Err("dimensions changed during names assignment".into());
                    }
                    if let Some(expected_rank) = dimension_rank {
                        let rank = dimensions.len();
                        access.require_active().map_err(|error| error.to_string())?;
                        if rank != expected_rank
                            || super::tsp::attribute(access, object, b"dim")? != dimensions
                        {
                            return Err("dimensions changed during names assignment".into());
                        }
                    }
                    Ok(())
                };
                let size = object.len();
                access.require_active().map_err(|error| error.to_string())?;
                let input_size = input.len();
                access.require_active().map_err(|error| error.to_string())?;
                // For other vectors an already-sized character value needs no
                // normalization. Preserve the existing replacement path,
                // which does not allocate an extra attribute cell.
                if !one_dimensional
                    && !input_is_object
                    && input.typeof_() == SEXPTYPE::STRSXP
                    && input_size == size
                {
                    validate_dimensions()?;
                    return Ok(false);
                }
                if input_size > size {
                    return Err(format!(
                        "'names' attribute [{input_size}] must be the same length as the vector [{size}]"
                    ));
                }
                let classed_metadata = if input_is_object {
                    Some(copy_classed_names_attributes(
                        access,
                        input,
                        input_is_object,
                    )?)
                } else {
                    None
                };
                let (name, normalized) = access
                    .with_native(|owner| {
                        let name = owner
                            .sexp(if one_dimensional {
                                R_DimNamesSymbol()
                            } else {
                                which
                            })?
                            .into_owned()?;
                        let pointer = if input.typeof_() == SEXPTYPE::STRSXP {
                            input.as_raw()
                        } else {
                            crate::mainutils::coerce::coerceToString(input.as_raw())
                        };
                        let normalized = owner.sexp(pointer)?.into_owned()?;
                        Ok((name, normalized))
                    })
                    .map_err(|error| error.to_string())?;
                access.require_active().map_err(|error| error.to_string())?;
                if input.is_object() != input_is_object {
                    return Err("names metadata changed during assignment".into());
                }
                if let Some(snapshot) = &classed_metadata {
                    snapshot.validate(access, input)?;
                    if input.typeof_() != SEXPTYPE::STRSXP {
                        snapshot.install(access, &normalized)?;
                    }
                }
                let mut source_metadata = None;
                let normalized = if input_size == size {
                    normalized
                } else {
                    let domain = access.domain();
                    let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
                    let padded = allocator
                        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, size)))
                        .map_err(|error| match error {
                            super::object::SexpError::AllocationFailed { .. } => {
                                "R allocation failed: memory or node budget exceeded".to_owned()
                            }
                            other => other.to_string(),
                        })?;
                    let mut padded = super::object::SexpMut::try_from_checked(padded)
                        .map_err(|e| e.to_string())?;
                    let missing = access
                        .with_native(|owner| owner.sexp(super::globals::R_NaString())?.into_owned())
                        .map_err(|e| e.to_string())?;
                    for index in 0..size {
                        let element = if index < input_size {
                            let element = normalized
                                .try_string_elt(index)
                                .and_then(super::object::Sexp::into_owned)
                                .map_err(|e| e.to_string())?;
                            access.require_active().map_err(|e| e.to_string())?;
                            element
                        } else {
                            missing.clone()
                        };
                        padded
                            .try_set_string_elt(index, element)
                            .map_err(|e| e.to_string())?;
                    }
                    let padded = padded.freeze();
                    let snapshot = snapshot_names_metadata(access, &normalized)?;
                    preserve_padded_names_metadata(access, &normalized, &snapshot, &padded, size)?;
                    source_metadata = Some((normalized, snapshot));
                    padded
                };
                access.require_active().map_err(|error| error.to_string())?;
                let current_size = object.len();
                access.require_active().map_err(|error| error.to_string())?;
                if current_size != size {
                    return Err(format!(
                        "'names' attribute [{size}] must be the same length as the vector [{current_size}]"
                    ));
                }
                validate_dimensions()?;
                let published = if one_dimensional {
                    // GNU normalizes the single axis as dimnames.  A nonempty
                    // factor is converted as a factor at that stage, but names
                    // coercion has already made its storage character.
                    if size != 0 {
                        let class = super::tsp::attribute(access, &normalized, b"class")?;
                        if class.typeof_() == SEXPTYPE::STRSXP {
                            for index in 0..class.len() {
                                let item = class
                                    .try_string_elt(index)
                                    .and_then(super::object::Sexp::into_owned)
                                    .map_err(|error| error.to_string())?;
                                access.require_active().map_err(|error| error.to_string())?;
                                if item
                                    .try_char_eq(b"factor")
                                    .map_err(|error| error.to_string())?
                                {
                                    return Err("malformed factor".into());
                                }
                            }
                        }
                    }
                    let domain = access.domain();
                    let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
                    let list = allocator
                        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 1)))
                        .map_err(|error| match error {
                            super::object::SexpError::AllocationFailed { .. } => {
                                "R allocation failed: memory or node budget exceeded".to_owned()
                            }
                            other => other.to_string(),
                        })?;
                    let mut list = super::object::SexpMut::try_from_checked(list)
                        .map_err(|e| e.to_string())?;
                    let element = if size == 0 { domain.nil() } else { normalized };
                    list.try_set_vector_elt(0, element)
                        .map_err(|e| e.to_string())?;
                    list.freeze()
                } else {
                    normalized
                };
                super::tsp::install_checked(access, object, &name, &published, || {
                    let current_size = object.len();
                    access.require_active().map_err(|error| error.to_string())?;
                    if current_size != size {
                        return Err(format!(
                            "'names' attribute [{size}] must be the same length as the vector [{current_size}]"
                        ));
                    }
                    validate_dimensions()?;
                    if input.is_object() != input_is_object {
                        return Err("names metadata changed during assignment".into());
                    }
                    if let Some(snapshot) = &classed_metadata {
                        snapshot.validate(access, input)?;
                    }
                    if let Some((source, snapshot)) = &source_metadata {
                        snapshot.validate(access, source)?;
                    }
                    Ok(())
                })?;
                Ok(true)
            },
        )
    }
}

struct ClassedNamesSnapshot {
    original: super::object::Sexp<'static>,
    copied: super::object::Sexp<'static>,
    is_object: bool,
}

impl ClassedNamesSnapshot {
    fn validate(
        &self,
        access: &super::owner::RuntimeAccess,
        source: &super::object::Sexp<'static>,
    ) -> Result<(), String> {
        let domain = access.domain();
        if source.is_object() != self.is_object {
            return Err("names metadata changed during assignment".into());
        }
        let current = source
            .try_attrib()
            .and_then(super::object::Sexp::into_owned)
            .map_err(|e| e.to_string())?;
        if current != self.original {
            return Err("names metadata changed during assignment".into());
        }
        let mut source_cell = current;
        let mut copied = self.copied.clone();
        let mut verify = super::tsp::CycleDetector::default();
        while !source_cell.is_nil() && !copied.is_nil() {
            verify.observe(domain.link(&source_cell).map_err(|e| e.to_string())?)?;
            if source_cell.try_tag().map_err(|e| e.to_string())?
                != copied.try_tag().map_err(|e| e.to_string())?
                || source_cell.try_car().map_err(|e| e.to_string())?
                    != copied.try_car().map_err(|e| e.to_string())?
            {
                return Err("names metadata changed during assignment".into());
            }
            source_cell = source_cell
                .try_cdr()
                .and_then(super::object::Sexp::into_owned)
                .map_err(|e| e.to_string())?;
            copied = copied
                .try_cdr()
                .and_then(super::object::Sexp::into_owned)
                .map_err(|e| e.to_string())?;
        }
        if !source_cell.is_nil() || !copied.is_nil() {
            return Err("names metadata changed during assignment".into());
        }
        access.require_active().map_err(|e| e.to_string())
    }

    fn install(
        &self,
        access: &super::owner::RuntimeAccess,
        normalized: &super::object::Sexp<'static>,
    ) -> Result<(), String> {
        super::object::SexpMut::try_from_checked(normalized.clone())
            .and_then(|mut value| value.try_set_attribute(&self.copied))
            .map_err(|e| e.to_string())?;
        access
            .with_native(|owner| unsafe {
                super::accessors::SET_OBJECT(normalized.as_raw(), i32::from(self.is_object));
                owner.require_active()
            })
            .map_err(|e| e.to_string())
    }
}

/// Copy attribute cells in source order without sharing their mutable links.
/// Allocation can reenter R, so validate the whole captured chain.  The
/// caller captures it before coercion and validates it again before publication.
fn copy_classed_names_attributes(
    access: &super::owner::RuntimeAccess,
    source: &super::object::Sexp<'static>,
    is_object: bool,
) -> Result<ClassedNamesSnapshot, String> {
    let domain = access.domain();
    let allocator = access.allocator(&domain).map_err(|e| e.to_string())?;
    let original = source
        .try_attrib()
        .and_then(super::object::Sexp::into_owned)
        .map_err(|e| e.to_string())?;
    let mut cursor = original.clone();
    let mut head = domain.nil();
    let mut tail: Option<super::object::Sexp<'static>> = None;
    let mut seen = super::tsp::CycleDetector::default();
    while !cursor.is_nil() {
        seen.observe(domain.link(&cursor).map_err(|e| e.to_string())?)?;
        let tag = cursor
            .try_tag()
            .and_then(super::object::Sexp::into_owned)
            .map_err(|e| e.to_string())?;
        let value = cursor
            .try_car()
            .and_then(super::object::Sexp::into_owned)
            .map_err(|e| e.to_string())?;
        let next = cursor
            .try_cdr()
            .and_then(super::object::Sexp::into_owned)
            .map_err(|e| e.to_string())?;
        let new = allocator
            .pairlist_cell(&value, &domain.nil(), &tag)
            .map_err(|error| match error {
                super::object::SexpError::AllocationFailed { .. } => {
                    "R allocation failed: memory or node budget exceeded".to_owned()
                }
                other => other.to_string(),
            })?;
        access.require_active().map_err(|e| e.to_string())?;
        if let Some(previous) = tail {
            super::object::SexpMut::try_from_checked(previous)
                .and_then(|mut node| node.try_set_pairlist_cdr(&new))
                .map_err(|e| e.to_string())?;
        } else {
            head = new.clone();
        }
        tail = Some(new);
        cursor = next;
    }
    let snapshot = ClassedNamesSnapshot {
        original,
        copied: head,
        is_object,
    };
    snapshot.validate(access, source)?;
    Ok(snapshot)
}

/// GNU resizing retains the names of a names value and fills their new slots
/// with empty strings, while dropping the value's other attributes.
fn preserve_padded_names_metadata(
    access: &super::owner::RuntimeAccess,
    original: &super::object::Sexp<'static>,
    snapshot: &NamesMetadataSnapshot,
    padded: &super::object::Sexp<'static>,
    size: i64,
) -> Result<(), String> {
    let names = &snapshot.names;
    if names.is_nil() {
        return Ok(());
    }
    if names.typeof_() != SEXPTYPE::STRSXP {
        return Err("invalid names attribute on names value".into());
    }
    let length = names.len();
    access.require_active().map_err(|error| error.to_string())?;
    let domain = access.domain();
    let allocator = access
        .allocator(&domain)
        .map_err(|error| error.to_string())?;
    let result = allocator
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, size)))
        .map_err(|error| match error {
            super::object::SexpError::AllocationFailed { .. } => {
                "R allocation failed: memory or node budget exceeded".to_owned()
            }
            other => other.to_string(),
        })?;
    let mut result =
        super::object::SexpMut::try_from_checked(result).map_err(|error| error.to_string())?;
    let blank = allocator.character("").map_err(|error| match error {
        super::object::SexpError::AllocationFailed { .. } => {
            "R allocation failed: memory or node budget exceeded".to_owned()
        }
        other => other.to_string(),
    })?;
    for index in 0..size {
        let value = if index < length {
            let value = names
                .try_string_elt(index)
                .and_then(super::object::Sexp::into_owned)
                .map_err(|error| error.to_string())?;
            access.require_active().map_err(|error| error.to_string())?;
            value
        } else {
            blank.clone()
        };
        result
            .try_set_string_elt(index, value)
            .map_err(|error| error.to_string())?;
    }
    let result = result.freeze();
    let tag = access
        .with_native(|owner| unsafe { owner.sexp(R_NamesSymbol())?.into_owned() })
        .map_err(|error| error.to_string())?;
    super::tsp::install_checked(access, padded, &tag, &result, || {
        snapshot.validate(access, original)?;
        if names.len() != length {
            return Err("names metadata changed during assignment".into());
        }
        Ok(())
    })
}

/// Retain the metadata links used by getAttrib(names) without reading an
/// ALTREP dimnames element a second time during validation.
struct NamesMetadataSnapshot {
    direct: super::object::Sexp<'static>,
    dimensions: Option<super::object::Sexp<'static>>,
    dimension_rank: Option<i64>,
    dimnames: Option<super::object::Sexp<'static>>,
    dimnames_length: Option<i64>,
    names: super::object::Sexp<'static>,
    names_length: i64,
}

impl NamesMetadataSnapshot {
    fn validate(
        &self,
        access: &super::owner::RuntimeAccess,
        value: &super::object::Sexp<'static>,
    ) -> Result<(), String> {
        if super::tsp::attribute(access, value, b"names")? != self.direct {
            return Err("names metadata changed during assignment".into());
        }
        if self.names.len() != self.names_length {
            return Err("names metadata changed during assignment".into());
        }
        if let Some(dimensions) = &self.dimensions {
            if super::tsp::attribute(access, value, b"dim")? != *dimensions
                || Some(dimensions.len()) != self.dimension_rank
            {
                return Err("names metadata changed during assignment".into());
            }
        }
        if let Some(dimnames) = &self.dimnames {
            if super::tsp::attribute(access, value, b"dimnames")? != *dimnames
                || Some(dimnames.len()) != self.dimnames_length
            {
                return Err("names metadata changed during assignment".into());
            }
        }
        access.require_active().map_err(|error| error.to_string())
    }
}

fn snapshot_names_metadata(
    access: &super::owner::RuntimeAccess,
    value: &super::object::Sexp<'static>,
) -> Result<NamesMetadataSnapshot, String> {
    let direct = super::tsp::attribute(access, value, b"names")?;
    let mut snapshot = NamesMetadataSnapshot {
        names_length: direct.len(),
        names: direct.clone(),
        direct,
        dimensions: None,
        dimension_rank: None,
        dimnames: None,
        dimnames_length: None,
    };
    if !snapshot.direct.is_nil() {
        return Ok(snapshot);
    }
    let dimensions = super::tsp::attribute(access, value, b"dim")?;
    let rank = dimensions.len();
    snapshot.dimensions = Some(dimensions.clone());
    snapshot.dimension_rank = Some(rank);
    if dimensions.typeof_() != SEXPTYPE::INTSXP || rank != 1 {
        return Ok(snapshot);
    }
    let dimnames = super::tsp::attribute(access, value, b"dimnames")?;
    let length = dimnames.len();
    snapshot.dimnames = Some(dimnames.clone());
    snapshot.dimnames_length = Some(length);
    if dimnames.typeof_() == SEXPTYPE::VECSXP && length != 0 {
        let names = dimnames
            .try_vector_elt(0)
            .and_then(super::object::Sexp::into_owned)
            .map_err(|error| error.to_string())?;
        access.require_active().map_err(|error| error.to_string())?;
        if names.typeof_() == SEXPTYPE::STRSXP {
            snapshot.names_length = names.len();
            snapshot.names = names;
        }
    }
    snapshot.validate(access, value)?;
    Ok(snapshot)
}

/// `names(x) <- NULL` removes dimnames for a one-dimensional array, while
/// `attr(x, "names") <- NULL` leaves that attribute alone.
pub(crate) unsafe fn try_clear_one_dimensional_names(x: SEXP) -> bool {
    unsafe {
        run_tsp(
            x,
            R_NilValue(),
            |_, _| None,
            |access, object, _| {
                if object.is_s4()
                    || !matches!(
                        object.typeof_(),
                        SEXPTYPE::LGLSXP
                            | SEXPTYPE::INTSXP
                            | SEXPTYPE::REALSXP
                            | SEXPTYPE::CPLXSXP
                            | SEXPTYPE::STRSXP
                            | SEXPTYPE::RAWSXP
                            | SEXPTYPE::VECSXP
                            | SEXPTYPE::EXPRSXP
                    )
                {
                    return Ok(false);
                }
                let dimensions = super::tsp::attribute(access, object, b"dim")?;
                if dimensions.is_nil() || dimensions.typeof_() != SEXPTYPE::INTSXP {
                    return Ok(false);
                }
                let rank = dimensions.len();
                access.require_active().map_err(|error| error.to_string())?;
                if super::tsp::attribute(access, object, b"dim")? != dimensions {
                    return Err("dimensions changed during names assignment".into());
                }
                if rank != 1 {
                    return Ok(false);
                }
                let name = access
                    .with_native(|owner| owner.sexp(R_DimNamesSymbol())?.into_owned())
                    .map_err(|error| error.to_string())?;
                let nil = access.domain().nil();
                super::tsp::install_checked(access, object, &name, &nil, || {
                    if super::tsp::attribute(access, object, b"dim")? != dimensions {
                        return Err("dimensions changed during names assignment".into());
                    }
                    let current_rank = dimensions.len();
                    access.require_active().map_err(|error| error.to_string())?;
                    if current_rank != 1
                        || super::tsp::attribute(access, object, b"dim")? != dimensions
                    {
                        return Err("dimensions changed during names assignment".into());
                    }
                    Ok(())
                })?;
                Ok(true)
            },
        )
    }
}

/// The names replacement primitive drops attributes on its character value;
/// attr replacement deliberately retains them. Return an owned plain copy
/// only when the replacement value actually has attributes.
pub(crate) unsafe fn plain_names_replacement(
    x: SEXP,
    value: SEXP,
) -> Option<super::object::Sexp<'static>> {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return None;
        }
        let token = super::owner::OwnerToken::current()
            .unwrap_or_else(|error| super::context::r_error(error.to_string()));
        let pin = token
            .pin()
            .unwrap_or_else(|error| super::context::r_error(error.to_string()));
        token
            .require_active()
            .unwrap_or_else(|error| super::context::r_error(error.to_string()));
        if pin.is_some() && token.node_factory().domain().has_no_attributes(value) {
            return None;
        }
        run_tsp(
            x,
            value,
            |_, _| None,
            |access, _object, input| {
                if input.typeof_() != SEXPTYPE::STRSXP
                    || input
                        .try_attrib()
                        .map_err(|error| error.to_string())?
                        .is_nil()
                {
                    return Ok(None);
                }
                let size = input.len();
                access.require_active().map_err(|error| error.to_string())?;
                let domain = access.domain();
                let allocator = access
                    .allocator(&domain)
                    .map_err(|error| error.to_string())?;
                let output = allocator
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, size)))
                    .map_err(|error| match error {
                        super::object::SexpError::AllocationFailed { .. } => {
                            "R allocation failed: memory or node budget exceeded".to_owned()
                        }
                        other => other.to_string(),
                    })?;
                let mut output = super::object::SexpMut::try_from_checked(output)
                    .map_err(|error| error.to_string())?;
                for index in 0..size {
                    let value = input
                        .try_string_elt(index)
                        .and_then(super::object::Sexp::into_owned)
                        .map_err(|error| error.to_string())?;
                    access.require_active().map_err(|error| error.to_string())?;
                    output
                        .try_set_string_elt(index, value)
                        .map_err(|error| error.to_string())?;
                }
                Ok(Some(output.freeze()))
            },
        )
    }
}

fn tsp_parameters(
    access: &super::owner::RuntimeAccess,
    object: &super::object::Sexp<'static>,
    input: &super::object::Sexp<'static>,
) -> Result<super::object::Sexp<'static>, String> {
    super::tsp::normalize(access, object, input, |access| {
        // SAFETY: the adapter pins the original owner and owns the option
        // before scalar dispatch; no payload loan crosses that callback.
        access
            .with_native(|owner| unsafe {
                let option = crate::mainutils::options::GetOption1(Rf_install(c"ts.eps".as_ptr()));
                let option = owner.sexp(option)?.into_owned()?;
                Ok(crate::mainutils::coerce::asReal(option.as_raw()))
            })
            .map_err(|error| error.to_string())
    })
}

#[cfg(test)]
unsafe fn normalize_tsp(x: SEXP, value: SEXP) -> super::object::Sexp<'static> {
    unsafe { run_tsp(x, value, |_, _| None, tsp_parameters) }
}

/// Set an attribute on an object.
///
/// This is the equivalent of R's `setAttrib()` from attrib.c.
pub unsafe fn setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        if x.is_null() || which.is_null() {
            return;
        }
        if try_set_pairlist_names(x, which, value) || try_set_atomic_names(x, which, value) {
            return;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            let kind = if t == SEXPTYPE::BUILTINSXP {
                "builtin"
            } else {
                "special"
            };
            std::panic::panic_any(crate::sexp::context::RError {
                message: format!("cannot set an attribute on a '{kind}'"),
            });
        }
        // Keep the target, parameters and original owner through checked
        // publication. The attribute chain is selected after allocation.
        if which == R_TspSymbol() && !value.is_null() {
            run_tsp(
                x,
                value,
                |_, _| None,
                |access, object, input| {
                    let name = access
                        .with_native(|owner| owner.sexp(which)?.into_owned())
                        .map_err(|error| error.to_string())?;
                    let normalized = if input.is_nil() {
                        input.clone()
                    } else {
                        tsp_parameters(access, object, input)?
                    };
                    super::tsp::install(access, object, &name, &normalized)
                },
            );
            return;
        }
        // GNU classgets: empty/NULL class strips; non-string errors.
        // GNU dimgets: coerce dims to INTSXP first.
        let value = if which == R_ClassSymbol() {
            classgets_normalize(x, value)
        } else if which == R_DimSymbol()
            && !value.is_null()
            && value != R_NilValue()
            && TYPEOF(value) != SEXPTYPE::INTSXP
        {
            crate::mainutils::coerce::coerceVector(value, SEXPTYPE::INTSXP.into())
        } else {
            value
        };

        let attrib = ATTRIB(x);

        // Search for existing attribute
        let mut previous = R_NilValue();
        let mut current = attrib;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == which {
                if value.is_null() || value == R_NilValue() {
                    let next = CDR(current);
                    if previous.is_null() || previous == R_NilValue() {
                        SET_ATTRIB(x, next);
                    } else {
                        SETCDR(previous, next);
                    }
                    if which == R_ClassSymbol() {
                        super::accessors::SET_OBJECT(x, 0);
                    }
                    return;
                }
                // Found — replace value
                SETCAR(current, value);
                // Update OBJECT flag for "class" attribute
                if which == R_ClassSymbol() {
                    if value.is_null() || value == R_NilValue() {
                        super::accessors::SET_OBJECT(x, 0);
                    } else {
                        super::accessors::SET_OBJECT(x, 1);
                    }
                }
                return;
            }
            previous = current;
            current = CDR(current);
        }

        if value.is_null() || value == R_NilValue() {
            return;
        }

        // Not found — append the new attribute. GNU R preserves attribute
        // assignment order: replacing an existing value leaves it in place,
        // while a new name is linked after the current tail.
        let _x_guard = super::protect::protect(x);
        let _which_guard = super::protect::protect(which);
        let _value_guard = super::protect::protect(value);
        let new_attr = Rf_cons(value, R_NilValue());
        if !new_attr.is_null() {
            super::accessors::SETTAG(new_attr, which);
            if attrib.is_null() || attrib == R_NilValue() {
                SET_ATTRIB(x, new_attr);
            } else {
                SETCDR(previous, new_attr);
            }
        }

        // Set OBJECT flag if setting "class" to non-nil
        if which == R_ClassSymbol() && !value.is_null() && value != R_NilValue() {
            super::accessors::SET_OBJECT(x, 1);
        }
    }
}

/// GNU `classgets`: NULL or length-0 STRSXP unclasses; other non-strings error.
pub(crate) unsafe fn classgets_normalize(vec: SEXP, klass: SEXP) -> SEXP {
    unsafe {
        if klass.is_null() || klass == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(klass) != SEXPTYPE::STRSXP {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "attempt to set invalid 'class' attribute".to_string(),
            });
        }
        if XLENGTH(klass) <= 0 {
            return R_NilValue();
        }
        if vec.is_null() || vec == R_NilValue() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "attempt to set an attribute on NULL".to_string(),
            });
        }
        let n = XLENGTH(klass);
        for i in 0..n {
            let elt = STRING_ELT(klass, i);
            if elt.is_null() {
                continue;
            }
            let cs = super::accessors::CHAR(elt);
            if !cs.is_null()
                && std::ffi::CStr::from_ptr(cs).to_bytes() == b"factor"
                && TYPEOF(vec) != SEXPTYPE::INTSXP
            {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "adding class \"factor\" to an invalid object".to_string(),
                });
            }
        }
        klass
    }
}

// ---------------------------------------------------------------------------
// isObject — check if an object has a class attribute
// ---------------------------------------------------------------------------

/// Check if an object is an S4 object (has a "class" attribute).
///
/// This is the equivalent of R's `isObject()` macro.
pub unsafe fn isObject(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        super::accessors::OBJECT(x)
    }
}

// ---------------------------------------------------------------------------
// R_classgets — set the class of an object
// ---------------------------------------------------------------------------

/// Set the class attribute of an object.
///
/// This is the equivalent of R's `R_classgets()`.
pub unsafe fn R_classgets(x: SEXP, klass: SEXP) -> SEXP {
    unsafe {
        if klass.is_null() || klass == R_NilValue() {
            return x;
        }

        let class_sym = R_ClassSymbol();
        setAttrib(x, class_sym, klass);
        x
    }
}

// Implicit class lives in eval/attrib_core.rs::R_data_class (GNU lang2str /
// type2str). Do not add a second table here.

// ---------------------------------------------------------------------------
// R_length_gets — get the length attribute
// ---------------------------------------------------------------------------

/// Get the length of an object via the "length" attribute.
pub unsafe fn R_length_gets(x: SEXP) -> c_int {
    unsafe {
        let len_sym = Rf_install(c"length".as_ptr());
        let val = getAttrib(x, len_sym);
        if !val.is_null() && TYPEOF(val) == SEXPTYPE::INTSXP {
            let data = super::accessors::INTEGER(val);
            if !data.is_null() {
                return *data;
            }
        }
        // Default: use the actual length
        super::constructors::Rf_length(x)
    }
}

// ---------------------------------------------------------------------------
// Rf_getAttrib — FFI-compatible getAttrib
// ---------------------------------------------------------------------------

/// FFI-compatible version of getAttrib.
pub unsafe fn Rf_getAttrib(x: SEXP, which: SEXP) -> SEXP {
    unsafe { getAttrib(x, which) }
}

/// FFI-compatible version of setAttrib.
pub unsafe fn Rf_setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        setAttrib(x, which, value);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn test_get_attrib_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(getAttrib(ptr::null_mut(), ptr::null_mut()), R_NilValue());
        }
    }

    #[test]
    fn test_is_object_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(isObject(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_data_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let class = crate::eval::attrib_core::R_data_class(v);
            assert!(!class.is_null());
        }
    }
}
