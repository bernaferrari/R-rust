// Error handling from R's nmath.h
// ML_WARNING, ML_WARN_return_NAN macros translated to Rust

use std::cell::RefCell;

use crate::constants::*;

/// Host-supplied sink for mathlib warnings.
///
/// Mirrors nmath.h's two `MATHLIB_WARNING` regimes: integrated mode maps it
/// to R's `warning(fmt, x)` so warnings become catchable, deferred R
/// conditions, and the runtime installs a hook bridging to `Rf_warning1`
/// at session creation; standalone mode maps it to `printf`, so the
/// fallback writes the message to stderr.
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
    emit_warning(&format!("non-integer {which} = {value:.6}"));
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
    let msg = match err_code {
        ME_RANGE => "value out of range in '%s'\n",
        ME_NOCONV => "convergence failed in '%s'\n",
        ME_PRECISION => "full precision may not have been achieved in '%s'\n",
        ME_UNDERFLOW => "underflow occurred in '%s'\n",
        _ => "unknown error in '%s'\n",
    };
    emit_warning(&msg.replace("%s", func_name));
}

/// Return NaN after issuing a domain warning.
/// This is a macro in C: { ML_WARNING(ME_DOMAIN, ""); return ML_NAN; }
/// `ME_DOMAIN` is silent (see [`ml_warning`]), matching stock.
#[inline]
pub fn ml_warn_return_nan() -> f64 {
    ML_NAN
}
