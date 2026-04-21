#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Event handling for GraphApp.
//!
//! Ported from events.c - winprocs, timers, and event dispatch.

use std::cell::{Cell, RefCell};
use std::os::raw::{c_int, c_long, c_uint, c_void};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::types::*;

thread_local! { static KEYSTATE: Cell<c_int> = Cell::new(0); }
thread_local! { static TIMER_STATE: RefCell<TimerState> = RefCell::new(TimerState::default()); }

#[derive(Clone, Copy, Default)]
struct TimerState {
    timeout: timerfn,
    data: *mut c_void,
    millisec: c_uint,
    pending: bool,
}

pub unsafe fn init_events() {
    KEYSTATE.with(|state| state.set(0));
    TIMER_STATE.with(|state| *state.borrow_mut() = TimerState::default());
}

pub unsafe fn finish_events() {
    TIMER_STATE.with(|state| *state.borrow_mut() = TimerState::default());
}

pub unsafe fn handle_control(_hwnd: *mut c_void, _message: c_uint) {}

pub unsafe fn getkeystate() -> c_int {
    KEYSTATE.with(|v| v.get())
}

pub unsafe fn drawall() {}

pub unsafe fn peekevent() -> c_int {
    TIMER_STATE.with(|state| i32::from(state.borrow().pending))
}

pub unsafe fn waitevent() {
    let millisec = TIMER_STATE.with(|state| state.borrow().millisec);
    if millisec > 0 {
        unsafe {
            delay(millisec);
        }
    }
    unsafe {
        doevent();
    }
}

pub unsafe fn doevent() -> c_int {
    let (timeout, data) = TIMER_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.pending {
            return (None, std::ptr::null_mut());
        }
        state.pending = false;
        (state.timeout, state.data)
    });

    if let Some(timeout) = timeout {
        unsafe {
            timeout(data);
        }
        1
    } else {
        0
    }
}

pub unsafe fn mainloop() {
    while unsafe { peekevent() } != 0 {
        unsafe {
            waitevent();
        }
    }
}

pub unsafe fn execapp(_cmd: *mut std::os::raw::c_char) -> c_int {
    0
}

pub unsafe fn settimer(millisec: c_uint) -> c_int {
    TIMER_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.millisec = millisec;
        state.pending = millisec > 0 && state.timeout.is_some();
    });
    i32::from(millisec > 0)
}

pub unsafe fn settimerfn(timeout: timerfn, data: *mut c_void) {
    TIMER_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.timeout = timeout;
        state.data = data;
        state.pending = state.millisec > 0 && state.timeout.is_some();
    });
}

pub unsafe fn setmousetimer(millisec: c_uint) -> c_int {
    unsafe { settimer(millisec) }
}

pub unsafe fn delay(millisec: c_uint) {
    if millisec > 0 {
        std::thread::sleep(Duration::from_millis(u64::from(millisec)));
    }
}

pub unsafe fn currenttime() -> c_long {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(c_long::MAX as u128) as c_long
}

pub unsafe fn toolbar_show() {}

pub unsafe fn toolbar_hide() {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! { static TIMER_CALLS: Cell<c_int> = Cell::new(0); }

    unsafe extern "C" fn record_timer(_data: *mut c_void) {
        TIMER_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    #[test]
    fn timer_callback_runs_once_per_scheduled_event() {
        unsafe {
            init_events();
            TIMER_CALLS.with(|calls| calls.set(0));

            settimerfn(Some(record_timer), std::ptr::null_mut());
            assert_eq!(peekevent(), 0);

            assert_eq!(settimer(1), 1);
            assert_eq!(peekevent(), 1);
            assert_eq!(doevent(), 1);
            assert_eq!(peekevent(), 0);

            TIMER_CALLS.with(|calls| assert_eq!(calls.get(), 1));
        }
    }

    #[test]
    fn currenttime_is_nonzero() {
        unsafe {
            assert!(currenttime() > 0);
        }
    }
}
