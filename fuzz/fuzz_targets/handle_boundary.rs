#![no_main]

use libfuzzer_sys::fuzz_target;
use r_embed::RSession;

thread_local! {
    // See eval_boundary.rs: one thread-local session per process.
    static SESSION: std::cell::RefCell<Option<RSession>> =
        const { std::cell::RefCell::new(None) };
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    SESSION.with(|cell| {
        {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                *slot = Some(RSession::new().expect("session init must not fail"));
            }
        }
        let mut slot = cell.borrow_mut();
        let Some(session) = slot.as_mut() else { return };
        let payload = String::from_utf8_lossy(data).into_owned();
        if let Ok(handle) = session.define_handle(&payload) {
            if let Ok(guard) = session.read_handle(&handle) {
                let _ = guard.value();
            }
            if let Ok(mut writer) = session.write_handle(&handle) {
                let half = &data[..data.len() / 2];
                let _ = writer.set(&String::from_utf8_lossy(half).into_owned());
                let _ = writer.update("NULL");
            }
            let _ = session.remove_handle(&handle);
            // Stale use must error, never panic.
            let _ = session.read_handle(&handle);
        }
        let _ = session.eval("gc()");
    });
});
