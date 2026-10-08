//! Keep fixed simulation steps on a wall-clock cadence through short stalls.

use std::time::Duration;

use crate::TICK;

const MAX_CATCH_UP: Duration = Duration::from_millis(500);

pub(crate) struct TickSchedule {
    next: Duration,
}

impl TickSchedule {
    pub fn new() -> Self {
        Self { next: TICK }
    }

    /// Returns sleep and discarded lag. A long outage rebases the cadence;
    /// ordinary overruns retain their deadline and recover with fixed steps.
    pub fn finish_tick(&mut self, elapsed: Duration) -> (Duration, Duration) {
        let lag = elapsed.saturating_sub(self.next);
        let discarded = if lag > MAX_CATCH_UP {
            self.next = elapsed;
            lag
        } else {
            Duration::ZERO
        };
        let sleep = self.next.saturating_sub(elapsed);
        self.next += TICK;
        (sleep, discarded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_stall_and_sleep_overshoot_do_not_permanently_slow_simulation() {
        let mut schedule = TickSchedule::new();
        let mut wall = Duration::ZERO;
        for tick in 0..20 {
            wall += Duration::from_millis(if tick == 3 { 203 } else { 7 });
            let (sleep, discarded) = schedule.finish_tick(wall);
            assert_eq!(discarded, Duration::ZERO);
            wall += sleep;
            // Model the OS waking one millisecond after a requested deadline.
            if !sleep.is_zero() && tick < 19 {
                wall += Duration::from_millis(1);
            }
        }
        assert_eq!(wall, Duration::from_secs(1));
    }

    #[test]
    fn a_long_outage_drops_old_debt_and_resumes_the_normal_cadence() {
        let mut schedule = TickSchedule::new();
        let wall = Duration::from_secs(5);
        let (sleep, discarded) = schedule.finish_tick(wall);
        assert_eq!(sleep, Duration::ZERO);
        assert_eq!(discarded, wall - TICK);
        assert_eq!(
            schedule.finish_tick(wall + Duration::from_millis(7)),
            (Duration::from_millis(43), Duration::ZERO)
        );
    }

    #[test]
    fn sustained_overload_cannot_accumulate_unbounded_catch_up_debt() {
        let mut schedule = TickSchedule::new();
        let mut discarded = Duration::ZERO;
        for tick in 1..=100 {
            let wall = Duration::from_millis(tick * 80);
            let (sleep, dropped) = schedule.finish_tick(wall);
            assert_eq!(sleep, Duration::ZERO);
            assert!(wall.saturating_sub(schedule.next) <= MAX_CATCH_UP);
            discarded += dropped;
        }
        assert!(discarded > Duration::from_secs(2));
    }
}
