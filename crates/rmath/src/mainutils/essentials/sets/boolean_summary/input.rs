use super::fail;
use crate::sexp::{ffi::SEXPTYPE, memory::WorkspaceReservation, object::Sexp, owner::OwnerToken};

pub(super) struct Argument<'s> {
    pub value: Sexp<'s>,
    pub flag: bool,
}
pub(super) struct Arguments<'s> {
    pub values: Vec<Argument<'s>>,
    // Storage and selected roots are released before their original admission.
    _admission: WorkspaceReservation,
}
pub(super) fn bytes(capacity: usize) -> usize {
    capacity
        .checked_mul(std::mem::size_of::<Argument<'_>>())
        .unwrap_or_else(|| fail("Boolean summary workspace size overflow"))
}
pub(super) fn next<'s>(value: &Sexp<'s>) -> Sexp<'s> {
    if value.is_nil() {
        return value.clone();
    }
    if !matches!(
        value.typeof_(),
        SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP
    ) {
        fail("improper pairlist in Boolean summary");
    }
    value
        .try_cdr()
        .expect("Boolean summary checked pairlist tail")
}
pub(super) fn validate(value: &Sexp<'_>) {
    let mut slow = value.clone();
    let mut fast = value.clone();
    loop {
        slow = next(&slow);
        fast = next(&next(&fast));
        if slow.is_nil() || fast.is_nil() {
            return;
        }
        if slow.as_raw() == fast.as_raw() {
            fail("cyclic pairlist in Boolean summary");
        }
    }
}
fn flag(value: &Sexp<'_>) -> bool {
    let tag = value.try_tag().expect("Boolean summary argument tag");
    if tag.is_nil() {
        return false;
    }
    let name = tag.try_printname().expect("Boolean summary tag name");
    unsafe { std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name.as_raw())) }.to_bytes()
        == b"na.rm"
}
impl<'s> Arguments<'s> {
    pub(super) fn new(args: &Sexp<'s>, owner: &OwnerToken<'_>) -> Self {
        validate(args);
        let mut count = 0usize;
        let mut flags = 0;
        let mut cursor = args.clone();
        while !cursor.is_nil() {
            count = count
                .checked_add(1)
                .unwrap_or_else(|| fail("Boolean summary workspace size overflow"));
            if flag(&cursor) {
                flags += 1;
            }
            if flags > 1 {
                fail("formal argument \"na.rm\" matched by multiple actual arguments");
            }
            cursor = next(&cursor);
        }
        let mut admission = owner
            .with_arena(|a| a.reserve_workspace(bytes(count)))
            .expect("Boolean summary original workspace owner")
            .unwrap_or_else(|| fail("Boolean summary arguments exceed session memory limit"));
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .unwrap_or_else(|_| fail("cannot allocate Boolean summary argument roots"));
        admission
            .try_resize(bytes(values.capacity()))
            .unwrap_or_else(|| fail("Boolean summary arguments exceed session memory limit"));
        cursor = args.clone();
        while !cursor.is_nil() {
            values.push(Argument {
                value: cursor.try_car().expect("Boolean summary input root"),
                flag: flag(&cursor),
            });
            cursor = next(&cursor);
        }
        Self {
            values,
            _admission: admission,
        }
    }
}
