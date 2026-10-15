#![forbid(unsafe_code)]
//! Checked relaxed counter updates shared by the pinned and Wasm toolchains.

// Rust 1.96 calls this operation fetch_update; the Wasm toolchain deprecates
// that name. Spell out the same compare/exchange loop without version gates.
macro_rules! update {
    ($counter:expr, $next:expr) => {{
        let counter = $counter;
        let mut current = counter.load(std::sync::atomic::Ordering::Relaxed);
        loop {
            let Some(next) = ($next)(current) else {
                break Err(current);
            };
            match counter.compare_exchange_weak(
                current,
                next,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(previous) => break Ok(previous),
                Err(observed) => current = observed,
            }
        }
    }};
}
pub(crate) use update;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

    #[test]
    fn checked_atomic_counters_preserve_original_and_reject_exhaustion() {
        let wide = AtomicU64::new(u64::MAX - 1);
        assert_eq!(
            super::update!(&wide, |n: u64| n.checked_add(1)),
            Ok(u64::MAX - 1)
        );
        assert_eq!(
            super::update!(&wide, |n: u64| n.checked_add(1)),
            Err(u64::MAX)
        );
        assert_eq!(wide.load(Ordering::Relaxed), u64::MAX);
        let cookie = AtomicU32::new(0);
        assert_eq!(
            super::update!(&cookie, |n: u32| (n != 0).then_some(n + 1)),
            Err(0)
        );
        assert_eq!(cookie.load(Ordering::Relaxed), 0);
        let serial = AtomicUsize::new(0);
        assert_eq!(super::update!(&serial, |n: usize| n.checked_add(1)), Ok(0));
    }

    #[test]
    fn checked_atomic_counter_updates_are_unique_across_threads() {
        let counter = AtomicU64::new(1);
        let mut values = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        (0..128)
                            .map(|_| super::update!(&counter, |n: u64| n.checked_add(1)).unwrap())
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        values.sort_unstable();
        assert_eq!(values, (1..=512).collect::<Vec<_>>());
    }
}
