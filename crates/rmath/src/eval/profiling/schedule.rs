//! Cooperative profiling deadlines; clocks never inspect a runtime or heap.
use super::rpe_type;
use std::time::Instant;

pub(crate) struct ProfileSchedule {
    clock: Clock,
    interval_ns: u64,
    next_ns: u64,
}

enum Clock {
    Elapsed(Instant),
    ThreadCpu,
}

impl Clock {
    fn now(&self) -> Option<u64> {
        match self {
            Self::Elapsed(start) => u64::try_from(start.elapsed().as_nanos()).ok(),
            Self::ThreadCpu => super::thread_cpu_time_nanos(),
        }
    }
}

impl ProfileSchedule {
    pub(super) fn new(interval_us: i32, event: rpe_type) -> Option<Self> {
        let interval_ns = u64::try_from(interval_us).ok()?.checked_mul(1_000)?;
        if interval_ns == 0 {
            return None;
        }
        let clock = match event {
            rpe_type::RPE_ELAPSED => Clock::Elapsed(Instant::now()),
            rpe_type::RPE_CPU => Clock::ThreadCpu,
        };
        let next_ns = clock.now()?.checked_add(interval_ns)?;
        Some(Self {
            clock,
            interval_ns,
            next_ns,
        })
    }

    #[cfg(test)]
    pub(super) fn make_due(&mut self) {
        self.next_ns = 0;
    }

    pub(super) fn due(&mut self) -> bool {
        self.clock.now().is_some_and(|now| self.due_at(now))
    }

    fn due_at(&mut self, now: u64) -> bool {
        if now < self.next_ns {
            return false;
        }
        // Coalesce missed deadlines, preserving the configured cadence instead
        // of fabricating a backlog of samples for a stack that no longer exists.
        let periods = (now - self.next_ns) / self.interval_ns + 1;
        self.next_ns = self
            .next_ns
            .saturating_add(periods.saturating_mul(self.interval_ns));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiling_deadlines_preserve_cadence_and_coalesce_missed_samples() {
        let mut schedule = ProfileSchedule {
            clock: Clock::Elapsed(Instant::now()),
            interval_ns: 10,
            next_ns: 10,
        };
        assert!(!schedule.due_at(9));
        assert!(schedule.due_at(10));
        assert!(!schedule.due_at(10));
        assert!(schedule.due_at(39));
        assert_eq!(schedule.next_ns, 40);
        assert!(!schedule.due_at(39));
        assert!(schedule.due_at(40));
    }

    #[cfg(all(not(miri), unix, not(target_arch = "wasm32")))]
    #[test]
    fn profiling_clocks_require_no_runtime_and_distinguish_sleep_from_cpu_time() {
        std::thread::spawn(|| {
            assert!(unsafe { crate::sexp::owner::OwnerToken::current() }.is_err());
            let elapsed = Clock::Elapsed(Instant::now());
            let wall_before = elapsed.now().unwrap();
            let cpu_before = Clock::ThreadCpu.now().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            let cpu_used = Clock::ThreadCpu.now().unwrap() - cpu_before;
            let elapsed_used = elapsed.now().unwrap() - wall_before;
            assert!(elapsed_used >= 10_000_000);
            assert!(
                cpu_used < elapsed_used / 2,
                "CPU={cpu_used}; elapsed={elapsed_used}"
            );
        })
        .join()
        .unwrap();
    }
}
