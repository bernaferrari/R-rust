#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Event handling for GraphApp.
//!
//! Ported from events.c - winprocs, timers, and event dispatch.

use std::os::raw::{c_int, c_long, c_uint, c_void};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::runtime::{with_graphapp_runtime, TimerState};
use super::types::*;

pub fn init_events() {
    with_graphapp_runtime(|runtime| {
        runtime.events.keystate = 0;
        runtime.events.timer = TimerState::default();
    });
}

pub fn finish_events() {
    with_graphapp_runtime(|runtime| runtime.events.timer = TimerState::default());
}

pub unsafe fn handle_control(_hwnd: *mut c_void, _message: c_uint) {}

pub fn getkeystate() -> c_int {
    with_graphapp_runtime(|runtime| runtime.events.keystate)
}

pub fn drawall() {}

pub fn peekevent() -> c_int {
    with_graphapp_runtime(|runtime| i32::from(runtime.events.timer.pending))
}

pub unsafe fn waitevent() {
    let millisec = with_graphapp_runtime(|runtime| runtime.events.timer.millisec);
    if millisec > 0 {
        delay(millisec);
    }
    doevent();
}

pub unsafe fn doevent() -> c_int {
    let (timeout, data) = with_graphapp_runtime(|runtime| {
        let timer = &mut runtime.events.timer;
        if !timer.pending {
            return (None, std::ptr::null_mut());
        }
        timer.pending = false;
        (timer.timeout, timer.data)
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
    while peekevent() != 0 {
        waitevent();
    }
}

pub unsafe fn execapp(_cmd: *mut std::os::raw::c_char) -> c_int {
    0
}

pub fn settimer(millisec: c_uint) -> c_int {
    with_graphapp_runtime(|runtime| {
        let timer = &mut runtime.events.timer;
        timer.millisec = millisec;
        timer.pending = millisec > 0 && timer.timeout.is_some();
    });
    i32::from(millisec > 0)
}

pub unsafe fn settimerfn(timeout: timerfn, data: *mut c_void) {
    with_graphapp_runtime(|runtime| {
        let timer = &mut runtime.events.timer;
        timer.timeout = timeout;
        timer.data = data;
        timer.pending = timer.millisec > 0 && timer.timeout.is_some();
    });
}

pub fn setmousetimer(millisec: c_uint) -> c_int {
    settimer(millisec)
}

pub fn delay(millisec: c_uint) {
    if millisec > 0 {
        std::thread::sleep(Duration::from_millis(u64::from(millisec)));
    }
}

pub fn currenttime() -> c_long {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(c_long::MAX as u128) as c_long
}

pub fn toolbar_show() {}

pub fn toolbar_hide() {}

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
        init_events();
        TIMER_CALLS.with(|calls| calls.set(0));

        unsafe { settimerfn(Some(record_timer), std::ptr::null_mut()); }
        assert_eq!(peekevent(), 0);

        assert_eq!(settimer(1), 1);
        assert_eq!(peekevent(), 1);
        assert_eq!(doevent(), 1);
        assert_eq!(peekevent(), 0);

        TIMER_CALLS.with(|calls| assert_eq!(calls.get(), 1));
    }

    #[test]
    fn currenttime_is_nonzero() {
        assert!(currenttime() > 0);
    }
}
