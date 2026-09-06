#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use r_embed::RSession;
use rport_fuzz::Token;

thread_local! {
    // RSession holds raw pointers (not Send); libFuzzer's default mode is
    // single-threaded, so a thread-local session is the right shape. One
    // session per process: engine init under ASan is expensive, and a
    // long-lived console fed many inputs matches the real embedding.
    static SESSION: std::cell::RefCell<Option<RSession>> =
        const { std::cell::RefCell::new(None) };
}

fuzz_target!(|data: &[u8]| {
    SESSION.with(|cell| {
        {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                *slot = Some(RSession::new().expect("session init must not fail"));
            }
        }
        let mut slot = cell.borrow_mut();
        let Some(session) = slot.as_mut() else { return };
        let mut u = Unstructured::new(data);
        let mut script = String::new();
        // Exhausted Unstructured keeps yielding default tokens forever;
        // stop at the data boundary (the earlier unbounded loop here was
        // the source of the giant mallocs under ASan).
        while !u.is_empty() {
            let Ok(token) = Token::arbitrary(&mut u) else { break };
            script.push_str(&rport_fuzz::render_token(&token));
        }
        // Errors are fine; panics escaping eval() are the bug class.
        let _ = session.eval(&script);
        let _ = session.eval("gc()");
    });
});
