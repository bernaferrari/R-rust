//! Admitted DFS frames; no Rust recursion and no global visited set.
#![forbid(unsafe_code)]
use crate::sexp::{ffi::SEXPTYPE, memory::WorkspaceReservation, object::Sexp, owner::OwnerToken};

struct Frame<'s> {
    value: Sexp<'s>,
    next: i64,
}
pub(super) struct Stack<'s> {
    // Frames and their roots/storage are destroyed before their admission.
    frames: Vec<Frame<'s>>,
    admission: WorkspaceReservation,
}
fn failure(message: &str) -> ! {
    crate::sexp::context::r_error(message)
}
/// A shallow pairlist fallback needs only constant owning cursors. Checked CDR
/// reads do not enter providers, so Floyd's cycle check needs no scratch Vec.
pub(super) fn validate_pairlist(value: &Sexp<'_>) {
    fn advance<'s>(value: &Sexp<'s>) -> Sexp<'s> {
        if value.is_nil() {
            return value.clone();
        }
        if value.typeof_() != SEXPTYPE::LISTSXP {
            failure("improper pairlist in anyNA");
        }
        value
            .try_cdr()
            .expect("anyNA checked shallow pairlist tail")
    }
    let mut slow = value.clone();
    let mut fast = value.clone();
    loop {
        slow = advance(&slow);
        fast = advance(&advance(&fast));
        if slow.is_nil() || fast.is_nil() {
            return;
        }
        if slow.as_raw() == fast.as_raw() {
            failure("cyclic pairlist in anyNA");
        }
    }
}
pub(super) fn capacity_bytes(capacity: usize) -> usize {
    capacity
        .checked_mul(std::mem::size_of::<Frame<'_>>())
        .unwrap_or_else(|| failure("anyNA traversal workspace size overflow"))
}
impl<'s> Stack<'s> {
    pub(super) fn new(owner: &OwnerToken<'_>) -> Self {
        let admission = owner
            .with_arena(|a| a.reserve_workspace(0))
            .expect("anyNA workspace original owner")
            .unwrap_or_else(|| failure("anyNA traversal exceeds session memory limit"));
        Self {
            frames: Vec::new(),
            admission,
        }
    }
    pub(super) fn push(&mut self, value: Sexp<'s>) {
        // Compare only active ancestors: repeated DAG children are legitimate.
        // Owning frame roots keep their identities alive across callbacks.
        if self
            .frames
            .iter()
            .any(|f| f.value.as_raw() == value.as_raw())
        {
            failure("cyclic list in recursive anyNA");
        }
        if self.frames.len() == self.frames.capacity() {
            let capacity = self
                .frames
                .capacity()
                .max(2)
                .checked_mul(2)
                .unwrap_or_else(|| failure("anyNA traversal workspace size overflow"));
            // Reallocation may hold both buffers until the old frames have
            // moved. Admit that replacement peak before asking the allocator.
            let peak = capacity_bytes(self.frames.capacity())
                .checked_add(capacity_bytes(capacity))
                .unwrap_or_else(|| failure("anyNA traversal workspace size overflow"));
            self.admission
                .try_resize(peak)
                .unwrap_or_else(|| failure("anyNA traversal exceeds session memory limit"));
            self.frames
                .try_reserve_exact(capacity - self.frames.len())
                .unwrap_or_else(|_| failure("cannot allocate anyNA traversal workspace"));
            // The allocator has released the old buffer. Retain the actual new
            // capacity only; on failure, unwind drops storage before admission.
            self.admission
                .try_resize(capacity_bytes(self.frames.capacity()))
                .unwrap_or_else(|| failure("anyNA traversal exceeds session memory limit"));
        }
        self.frames.push(Frame { value, next: 0 });
    }
    pub(super) fn next(&mut self, check: &impl Fn()) -> Option<Sexp<'s>> {
        loop {
            let frame = self.frames.last_mut()?;
            let pairlist = frame.value.typeof_() == SEXPTYPE::LISTSXP;
            let length = if pairlist { 2 } else { frame.value.len() };
            if frame.next >= length {
                self.frames.pop();
                continue;
            }
            let index = frame.next;
            frame.next += 1;
            let value = frame.value.clone();
            // The frame borrow ends before a lazy provider can collect/reenter.
            let child = if pairlist {
                if index == 0 {
                    value.try_car()
                } else {
                    value.try_cdr()
                }
            } else {
                value.try_vector_elt(index)
            }
            .expect("anyNA checked traversal child");
            check();
            if pairlist && index == 1 && !child.is_nil() && child.typeof_() != SEXPTYPE::LISTSXP {
                failure("improper pairlist in recursive anyNA");
            }
            return Some(child);
        }
    }
}
