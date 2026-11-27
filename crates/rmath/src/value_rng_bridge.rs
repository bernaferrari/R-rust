//! Transitional bridge routing nmath's distribution samplers through the
//! pure world's heap-owned RNG stream.
//!
//! The nmath kernels draw uniforms through the crate-global `unif_rand`,
//! which delegates to a thread-local hook when installed. The pure world
//! owns its GNU-MT stream inside `heap.rng` rather than any global, so a
//! primitive that needs a sampler installs the heap stream as the hook for
//! exactly the duration of the draw loop. The unsafe dereference lives
//! here — outside the `#![forbid(unsafe_code)]` value module — and is
//! sound by construction: the pointer comes from a live `&mut PureRng`,
//! the guard restores the hook before the primitive regains control, and
//! samplers never reenter the evaluator. This machinery retires when the
//! runtime threads state explicitly (rport-c2v5f.13).

use std::cell::Cell;

use crate::value::rng::PureRng;

thread_local! {
    static ACTIVE_RNG: Cell<*mut PureRng> = const { Cell::new(std::ptr::null_mut()) };
}

/// The installed hook: draws one uniform from the active heap stream.
fn pure_unif_hook() -> f64 {
    let ptr = ACTIVE_RNG.with(Cell::get);
    // SAFETY: `enter` installs the pointer from a live &mut PureRng and the
    // guard clears it before the borrow's owner regains control; samplers
    // cannot outlive the guard because they run inside the draw loop.
    unsafe { (*ptr).unif() }
}

/// RAII guard restoring hook and pointer on drop.
pub struct RngBridge {
    previous: *mut PureRng,
}

/// Install `rng` as the nmath uniform source for the guard's lifetime.
pub fn enter(rng: &mut PureRng) -> RngBridge {
    let ptr = rng as *mut PureRng;
    let previous = ACTIVE_RNG.with(|slot| slot.replace(ptr));
    crate::nmath::rng::set_unif_hook(Some(pure_unif_hook));
    RngBridge { previous }
}

impl Drop for RngBridge {
    fn drop(&mut self) {
        crate::nmath::rng::set_unif_hook(None);
        ACTIVE_RNG.with(|slot| slot.set(self.previous));
    }
}
