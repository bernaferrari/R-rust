//! Copied Boolean coercion; no raw output span survives provider callbacks.
use super::{fail, input};
use crate::sexp::{
    ffi::{NA_INTEGER, SEXPTYPE},
    object::{SessionNodeFactory, Sexp, SexpMut},
};
use std::ffi::CStr;

fn atomic(kind: SEXPTYPE) -> bool {
    matches!(
        kind,
        SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::RAWSXP
    )
}
fn pair(kind: SEXPTYPE) -> bool {
    matches!(
        kind,
        SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP
    )
}
fn cells(value: &Sexp<'_>, bindings: bool) -> i64 {
    input::validate(value);
    let mut cursor = value.clone();
    let mut count = 0i64;
    while !cursor.is_nil() {
        if !bindings
            || !cursor
                .try_tag()
                .expect("Boolean summary binding tag")
                .is_nil()
        {
            count = count
                .checked_add(1)
                .unwrap_or_else(|| fail("Boolean summary length overflow"));
        }
        cursor = input::next(&cursor);
    }
    count
}
pub(super) fn length(value: &Sexp<'_>) -> i64 {
    if value.is_nil() {
        return 0;
    }
    if value.is_vector() || value.typeof_() == SEXPTYPE::CHARSXP {
        return value.len();
    }
    if pair(value.typeof_()) {
        return cells(value, false);
    }
    if value.typeof_() == SEXPTYPE::ENVSXP {
        let mut n = cells(
            &value
                .try_frame()
                .expect("Boolean summary environment frame"),
            true,
        );
        let buckets = value
            .try_hashtab()
            .expect("Boolean summary environment buckets");
        if !buckets.is_nil() {
            for i in 0..buckets.len() {
                n = n
                    .checked_add(cells(
                        &buckets.try_vector_elt(i).expect("Boolean summary bucket"),
                        true,
                    ))
                    .unwrap_or_else(|| fail("Boolean summary length overflow"));
            }
        }
        return n;
    }
    1
}
pub(super) fn warning(kind: SEXPTYPE) -> &'static CStr {
    match kind {
        SEXPTYPE::REALSXP => c"coercing argument of type 'double' to logical",
        SEXPTYPE::CPLXSXP => c"coercing argument of type 'complex' to logical",
        SEXPTYPE::STRSXP => c"coercing argument of type 'character' to logical",
        SEXPTYPE::RAWSXP => c"coercing argument of type 'raw' to logical",
        SEXPTYPE::VECSXP => c"coercing argument of type 'list' to logical",
        SEXPTYPE::EXPRSXP => c"coercing argument of type 'expression' to logical",
        SEXPTYPE::LISTSXP => c"coercing argument of type 'pairlist' to logical",
        SEXPTYPE::LANGSXP => c"coercing argument of type 'language' to logical",
        SEXPTYPE::SYMSXP => c"coercing argument of type 'symbol' to logical",
        SEXPTYPE::CLOSXP => c"coercing argument of type 'closure' to logical",
        SEXPTYPE::ENVSXP => c"coercing argument of type 'environment' to logical",
        SEXPTYPE::BUILTINSXP => c"coercing argument of type 'builtin' to logical",
        SEXPTYPE::SPECIALSXP => c"coercing argument of type 'special' to logical",
        _ => c"coercing argument to logical",
    }
}
fn string(value: &Sexp<'_>) -> i32 {
    if value.as_raw() == unsafe { crate::sexp::globals::R_NaString() } {
        return NA_INTEGER;
    }
    let text = unsafe { CStr::from_ptr(crate::sexp::accessors::CHAR(value.as_raw())) }.to_bytes();
    match text {
        b"T" | b"TRUE" | b"True" | b"true" => 1,
        b"F" | b"FALSE" | b"False" | b"false" => 0,
        _ => NA_INTEGER,
    }
}
fn element(value: &Sexp<'_>, i: i64, check: &impl Fn()) -> i32 {
    let result = match value.typeof_() {
        SEXPTYPE::LGLSXP => value
            .try_logical_elt(i)
            .expect("Boolean summary logical coercion"),
        SEXPTYPE::INTSXP => {
            let v = value
                .try_integer_elt(i)
                .expect("Boolean summary integer coercion");
            if v == NA_INTEGER { v } else { (v != 0) as i32 }
        }
        SEXPTYPE::REALSXP => {
            let v = value
                .try_real_elt(i)
                .expect("Boolean summary real coercion");
            if v.is_nan() {
                NA_INTEGER
            } else {
                (v != 0.) as i32
            }
        }
        SEXPTYPE::CPLXSXP => {
            let v = value
                .try_complex_elt(i)
                .expect("Boolean summary complex coercion");
            if v.r.is_nan() || v.i.is_nan() {
                NA_INTEGER
            } else {
                (v.r != 0. || v.i != 0.) as i32
            }
        }
        SEXPTYPE::STRSXP => {
            let v = value
                .try_string_elt(i)
                .expect("Boolean summary string coercion");
            check();
            string(&v)
        }
        SEXPTYPE::RAWSXP => {
            (value.try_raw_elt(i).expect("Boolean summary raw coercion") != 0) as i32
        }
        _ => NA_INTEGER,
    };
    check();
    result
}
pub(super) fn scalar(value: &Sexp<'_>, strict: bool, check: &impl Fn()) -> i32 {
    if value.typeof_() == SEXPTYPE::CHARSXP {
        return string(value);
    }
    if !atomic(value.typeof_()) || value.len() == 0 {
        return NA_INTEGER;
    }
    if strict && value.len() > 1 {
        fail(&format!(
            "'length = {}' in coercion to 'logical(1)'",
            value.len()
        ));
    }
    element(value, 0, check)
}
fn invalid(kind: SEXPTYPE) -> ! {
    fail(match kind {
        SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => "'list' object cannot be coerced to type 'logical'",
        SEXPTYPE::LISTSXP => "'pairlist' object cannot be coerced to type 'logical'",
        SEXPTYPE::LANGSXP => "'language' object cannot be coerced to type 'logical'",
        SEXPTYPE::SYMSXP => "'symbol' object cannot be coerced to type 'logical'",
        SEXPTYPE::CLOSXP => "cannot coerce type 'closure' to vector of type 'logical'",
        SEXPTYPE::ENVSXP => "environments cannot be coerced to other types",
        _ => "cannot coerce argument to logical",
    })
}
pub(super) fn logical<'s>(
    value: &Sexp<'_>,
    f: &SessionNodeFactory<'s>,
    check: &impl Fn(),
) -> Sexp<'s> {
    let kind = value.typeof_();
    let list = kind == SEXPTYPE::VECSXP;
    let pairlist = kind == SEXPTYPE::LISTSXP;
    if !atomic(kind) && !list && !pairlist {
        invalid(kind);
    }
    let n = length(value);
    if list || pairlist {
        let mut cursor = value.clone();
        for i in 0..n {
            let child = if list {
                value.try_vector_elt(i)
            } else {
                cursor.try_car()
            }
            .expect("Boolean summary scalar-list preflight");
            check();
            if !child.is_vector() || child.len() > 1 {
                invalid(kind);
            }
            if pairlist {
                cursor = input::next(&cursor);
            }
        }
    }
    let raw = unsafe { crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::LGLSXP, n) };
    check();
    let mut output = SexpMut::try_from_checked(
        f.wrap(raw)
            .expect("Boolean summary admitted coercion output"),
    )
    .expect("Boolean summary fresh logical output");
    let mut cursor = value.clone();
    for i in 0..n {
        let result = if list || pairlist {
            let child = if list {
                value.try_vector_elt(i)
            } else {
                cursor.try_car()
            }
            .expect("Boolean summary scalar-list element");
            check();
            if pairlist {
                cursor = input::next(&cursor);
            }
            scalar(&child, false, check)
        } else {
            element(value, i, check)
        };
        output
            .try_set_logical_elt(i, result)
            .expect("Boolean summary logical output write");
    }
    output.freeze()
}
