//! Scoped, per-thread allocator faults for production TRE cleanup tests.
#![forbid(unsafe_code)]

use std::{cell::RefCell, collections::HashSet};

#[derive(Default)]
struct State {
    fail_at: Option<usize>,
    attempts: usize,
    live: HashSet<usize>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub(super) fn deny() -> bool {
    STATE.with_borrow_mut(|state| {
        let Some(state) = state else { return false };
        let index = state.attempts;
        state.attempts += 1;
        state.fail_at == Some(index)
    })
}

pub(super) fn allocated(pointer: *mut u8) {
    if !pointer.is_null() {
        STATE.with_borrow_mut(|state| {
            if let Some(state) = state {
                assert!(state.live.insert(pointer.addr()));
            }
        });
    }
}

pub(super) fn deallocated(pointer: *mut u8) {
    STATE.with_borrow_mut(|state| {
        if let Some(state) = state {
            assert!(
                state.live.remove(&pointer.addr()),
                "untracked TRE allocation freed"
            );
        }
    });
}

pub(crate) struct Report {
    pub attempts: usize,
    pub remaining: usize,
}

pub(crate) fn observe<T>(fail_at: Option<usize>, action: impl FnOnce() -> T) -> (T, Report) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            STATE.with_borrow_mut(|state| *state = None);
        }
    }
    STATE.with_borrow_mut(|state| {
        assert!(state.is_none(), "nested TRE allocator observation");
        *state = Some(State {
            fail_at,
            ..State::default()
        });
    });
    let reset = Reset;
    let result = action();
    let report = STATE.with_borrow(|state| {
        let state = state.as_ref().unwrap();
        Report {
            attempts: state.attempts,
            remaining: state.live.len(),
        }
    });
    drop(reset);
    (result, report)
}
