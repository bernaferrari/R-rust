// Error handling from R's nmath.h
// ML_WARNING, ML_WARN_return_NAN macros translated to Rust

use core::fmt::Write;
use std::cell::RefCell;

use crate::constants::*;
use crate::diagnostics::Text;

/// Host-supplied sink for mathlib warnings.
///
/// Mirrors nmath.h's two `MATHLIB_WARNING` regimes: integrated mode maps it
/// to R's `warning(fmt, x)` so warnings become catchable, deferred R
/// conditions, and the runtime installs a hook bridging to `Rf_warning1`
/// at session creation; standalone mode maps it to `printf`, so the
/// fallback writes the message to stderr.
/// Hooks may allocate, collect, and reenter the host. Supplied text must stay
/// valid for the whole hook call, as required by its Rust shared borrow.
pub type WarningHook = fn(message: &str);

thread_local! {
    static WARNING_HOOK: RefCell<Option<WarningHook>> = const { RefCell::new(None) };
}

/// Install (or clear, with `None`) this thread's mathlib warning hook.
pub fn set_warning_hook(hook: Option<WarningHook>) {
    WARNING_HOOK.with(|slot| *slot.borrow_mut() = hook);
}

/// Deliver a formatted mathlib warning through the installed host hook,
/// falling back to stderr in standalone mode.
fn emit_warning(message: &str) {
    let hook = WARNING_HOOK.with(|slot| *slot.borrow());
    match hook {
        Some(hook) => hook(message),
        None => eprint!("{message}"),
    }
}

/// `MATHLIB_WARNING(_("non-integer %s = %f"), which, value)` — dpq.h's
/// `R_D_nonint_check` warns `x`; pbinom.c warns `n`.
pub fn ml_warn_nonint(which: &str, value: f64) {
    let mut message = Text::<512>::new();
    if write!(&mut message, "non-integer {which} = {value:.6}").is_ok() {
        emit_warning(message.text());
    } else {
        // The public API accepts an arbitrary caller-supplied name.
        emit_warning(&format!("non-integer {which} = {value:.6}"));
    }
}

/// A mathlib warning whose text is the whole message, not a code template.
pub fn ml_warn_message(message: &str) {
    emit_warning(message);
}

/// Print a mathlib warning.
///
/// Translates nmath.h's `ML_WARNING`: `ME_DOMAIN` is deliberately silent
/// upstream — "We don't report ME_DOMAIN errors as the callers collect
/// ML_NANs into a single warning."
#[inline]
pub fn ml_warning(err_code: u32, func_name: &str) {
    if err_code <= ME_DOMAIN {
        return;
    }
    let prefix = match err_code {
        ME_RANGE => "value out of range in '",
        ME_NOCONV => "convergence failed in '",
        ME_PRECISION => "full precision may not have been achieved in '",
        ME_UNDERFLOW => "underflow occurred in '",
        _ => "unknown error in '",
    };
    let mut message = Text::<256>::new();
    if write!(&mut message, "{prefix}{func_name}'\n").is_ok() {
        emit_warning(message.text());
    } else {
        // Preserve unbounded public names rather than truncating diagnostics.
        emit_warning(&format!("{prefix}{func_name}'\n"));
    }
}

/// Return NaN after issuing a domain warning.
/// This is a macro in C: { ML_WARNING(ME_DOMAIN, ""); return ML_NAN; }
/// `ME_DOMAIN` is silent (see [`ml_warning`]), matching stock.
#[inline]
pub fn ml_warn_return_nan() -> f64 {
    ML_NAN
}

#[cfg(test)]
mod warning_tests {
    use super::*;

    thread_local! {
        static MESSAGES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn record(message: &str) {
        MESSAGES.with(|messages| messages.borrow_mut().push(message.to_owned()));
    }

    fn reenter(message: &str) {
        record(message);
        if message.contains("outer") {
            ml_warning(ME_RANGE, "inner");
        }
    }

    fn stop(_: &str) {
        panic!("warning promoted to error");
    }

    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            set_warning_hook(None);
        }
    }

    #[test]
    fn warning_templates_keep_text_for_utf8_and_long_names() {
        MESSAGES.with(|messages| messages.borrow_mut().clear());
        set_warning_hook(Some(record));
        let _reset = Reset;
        ml_warning(ME_DOMAIN, "silent");
        for (code, expected) in [
            (ME_RANGE, "value out of range in 'Rλ%s'\n"),
            (ME_NOCONV, "convergence failed in 'Rλ%s'\n"),
            (
                ME_PRECISION,
                "full precision may not have been achieved in 'Rλ%s'\n",
            ),
            (ME_UNDERFLOW, "underflow occurred in 'Rλ%s'\n"),
            (u32::MAX, "unknown error in 'Rλ%s'\n"),
        ] {
            ml_warning(code, "Rλ%s");
            MESSAGES.with(|messages| assert_eq!(messages.borrow_mut().remove(0), expected));
        }
        let name = "λ".repeat(300);
        ml_warning(ME_RANGE, &name);
        ml_warn_nonint(&name, -0.25);
        ml_warn_nonint("x", f64::MAX);
        MESSAGES.with(|messages| {
            let messages = messages.borrow();
            assert_eq!(messages[0], format!("value out of range in '{name}'\n"));
            assert_eq!(messages[1], format!("non-integer {name} = -0.250000"));
            assert_eq!(messages[2], format!("non-integer x = {:.6}", f64::MAX));
        });
    }

    #[test]
    fn warning_hooks_can_reenter_and_unwind() {
        MESSAGES.with(|messages| messages.borrow_mut().clear());
        set_warning_hook(Some(reenter));
        let _reset = Reset;
        ml_warning(ME_PRECISION, "outer");
        MESSAGES.with(|messages| {
            assert_eq!(
                &*messages.borrow(),
                &[
                    "full precision may not have been achieved in 'outer'\n",
                    "value out of range in 'inner'\n",
                ]
            );
        });
        set_warning_hook(Some(stop));
        assert!(std::panic::catch_unwind(|| ml_warn_nonint("x", 1.5)).is_err());
        set_warning_hook(Some(record));
        ml_warn_nonint("x", 1.5);
        MESSAGES.with(|messages| {
            assert_eq!(
                messages.borrow().last().unwrap(),
                "non-integer x = 1.500000"
            )
        });
    }
}
