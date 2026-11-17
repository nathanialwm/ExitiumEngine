//! [`Time`], the snapshot of time that systems read, and [`FixedClock`],
//! which turns frame times into whole fixed ticks.
//!
//! Nothing here reads a clock. `std::time::Instant` panics on
//! `wasm32-unknown-unknown`, so the platform layer measures real time and
//! passes it in as seconds. Time in this module is plain data: the same
//! inputs always give the same ticks, which tests, replays and the headless
//! server rely on.
//!
//! # Fixed ticks
//!
//! The simulation runs in fixed ticks of [`FixedClock::step`] seconds,
//! whatever the frame rate. Each frame, [`FixedClock::advance`] adds the
//! frame's real seconds to an accumulator and takes out as many whole steps
//! as it holds. Less than one step is left over, and it carries to the next
//! frame.
//!
//! # Spiral of death
//!
//! If running one tick takes longer in real time than the step it simulates,
//! every frame owes more ticks than the one before, running them makes that
//! frame longer still, and the game locks up. To prevent that, one `advance`
//! call returns at most
//! [`FixedClock::max_ticks`] ticks. The whole steps beyond the cap are
//! dropped, not owed to later frames, so the simulation runs slower than real
//! time instead of freezing. [`Advance::dropped`] reports the dropped
//! seconds, so a caller that must not lose time (offline progress in an
//! incremental game) can account for them some other way.
//!
//! ```
//! use exitium_core::{FixedClock, Time};
//!
//! // Four ticks per second, at most three ticks per frame.
//! let mut clock = FixedClock::new(0.25, 3);
//! let mut time = Time::new(clock.step());
//!
//! // One frame of an app loop. The platform layer measured 0.625 s.
//! let frame_seconds = 0.625;
//! time.delta = frame_seconds;
//! time.elapsed += frame_seconds;
//! let advance = clock.advance(frame_seconds);
//! for _ in 0..advance.ticks {
//!     time.tick_count += 1;
//!     // Run the fixed-rate systems here.
//! }
//! assert_eq!(time.tick_count, 2); // two whole steps
//! assert_eq!(clock.remainder(), 0.125); // half a step carries over
//!
//! // A 2 s hitch owes 8 ticks: 3 run and 5 steps (1.25 s) are dropped.
//! let advance = clock.advance(2.0);
//! assert_eq!((advance.ticks, advance.dropped), (3, 1.25));
//! ```

/// What systems read about time during an update.
///
/// Plain data with public fields: the app loop fills it in, and systems read
/// it. Variable-rate systems use `delta`. Fixed-rate systems use
/// `fixed_step` and `tick_count` and never `delta` or `elapsed`, so the
/// simulation does not depend on the frame rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Time {
    /// Seconds since the previous frame.
    pub delta: f64,
    /// Seconds since the app started: the sum of every frame's `delta`.
    pub elapsed: f64,
    /// Fixed ticks run since the app started.
    pub tick_count: u64,
    /// Seconds per fixed tick, the [`FixedClock::step`] of the clock that
    /// drives the ticks.
    pub fixed_step: f64,
}

impl Time {
    /// The snapshot at startup: no time has passed and no tick has run.
    #[must_use]
    #[inline]
    pub const fn new(fixed_step: f64) -> Self {
        Self {
            delta: 0.0,
            elapsed: 0.0,
            tick_count: 0,
            fixed_step,
        }
    }
}

/// Turns real frame times into whole fixed ticks and carries the rest.
///
/// The [module docs](crate::time) explain the accumulator and why the ticks
/// per call are capped.
// Invariant: `step` is finite and > 0, `max_ticks` >= 1, and
// `0.0 <= accumulator < step` between calls. `new` checks the first two, and
// `advance` restores the third before it returns. Not `Copy`: a silent copy
// of a clock would advance on its own and lose the ticks it accumulates.
#[derive(Clone, Debug, PartialEq)]
pub struct FixedClock {
    step: f64,
    max_ticks: u32,
    accumulator: f64,
}

impl FixedClock {
    /// A clock that ticks every `step` seconds and returns at most
    /// `max_ticks` ticks from each [`advance`](Self::advance) call. The
    /// accumulator starts empty.
    ///
    /// # Panics
    ///
    /// If `step` is not finite and greater than zero, or if `max_ticks` is
    /// zero.
    #[must_use]
    pub const fn new(step: f64, max_ticks: u32) -> Self {
        assert!(
            step.is_finite() && step > 0.0,
            "FixedClock step must be finite and greater than zero"
        );
        assert!(max_ticks > 0, "FixedClock max_ticks must be at least 1");
        Self {
            step,
            max_ticks,
            accumulator: 0.0,
        }
    }

    /// Seconds per fixed tick.
    #[must_use]
    #[inline]
    pub const fn step(&self) -> f64 {
        self.step
    }

    /// The most ticks one [`advance`](Self::advance) call returns.
    #[must_use]
    #[inline]
    pub const fn max_ticks(&self) -> u32 {
        self.max_ticks
    }

    /// Seconds in the accumulator that do not make a whole step yet. Always
    /// at least zero and less than [`step`](Self::step).
    #[must_use]
    #[inline]
    pub const fn remainder(&self) -> f64 {
        self.accumulator
    }

    /// Adds `wall_seconds` of real time and returns the number of fixed ticks
    /// to run now.
    ///
    /// Whole steps are taken out of the accumulator until less than one step
    /// is left or [`max_ticks`](Self::max_ticks) is reached. Whole steps still
    /// left at the cap are dropped and reported in [`Advance::dropped`]. The
    /// part of a step that is left always carries to the next call.
    ///
    /// A negative, NaN or infinite `wall_seconds` counts as zero, so one bad
    /// reading cannot poison the accumulator. A NaN would otherwise stop the
    /// clock for good.
    pub fn advance(&mut self, wall_seconds: f64) -> Advance {
        if wall_seconds.is_finite() && wall_seconds > 0.0 {
            // `min` keeps an absurd sum finite, because `%` below would turn
            // an infinite accumulator into NaN.
            self.accumulator = (self.accumulator + wall_seconds).min(f64::MAX);
        }
        let mut ticks = 0;
        while ticks < self.max_ticks && self.accumulator >= self.step {
            self.accumulator -= self.step;
            ticks += 1;
        }
        let mut dropped = 0.0;
        if self.accumulator >= self.step {
            // The cap was hit. Float `%` is exact, so `kept` is precisely the
            // part of a step that is left.
            let kept = self.accumulator % self.step;
            dropped = self.accumulator - kept;
            self.accumulator = kept;
        }
        Advance { ticks, dropped }
    }
}

/// What one [`FixedClock::advance`] call produced.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub struct Advance {
    /// Fixed ticks to run now, from zero up to [`FixedClock::max_ticks`].
    pub ticks: u32,
    /// Seconds dropped because the cap was hit. Only whole steps are
    /// dropped; the part of a step that is left carries to the next call.
    /// Zero when the cap was not hit.
    pub dropped: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1/64 s, exact in binary, so these tests can compare floats exactly.
    const STEP: f64 = 1.0 / 64.0;

    fn adv(ticks: u32, dropped: f64) -> Advance {
        Advance { ticks, dropped }
    }

    #[test]
    fn time_starts_at_zero() {
        let expected = Time {
            delta: 0.0,
            elapsed: 0.0,
            tick_count: 0,
            fixed_step: STEP,
        };
        assert_eq!(Time::new(STEP), expected);
    }

    #[test]
    fn new_clock_is_empty() {
        // `new` is a `const fn`, so a clock can be a constant.
        const CLOCK: FixedClock = FixedClock::new(STEP, 5);
        assert_eq!(CLOCK.step(), STEP);
        assert_eq!(CLOCK.max_ticks(), 5);
        assert_eq!(CLOCK.remainder(), 0.0);
    }

    #[test]
    fn advance_takes_whole_steps_and_carries_the_rest() {
        let mut clock = FixedClock::new(0.25, 10);
        // (seconds fed, ticks expected, remainder expected afterwards)
        let frames = [
            (0.0, 0, 0.0),
            (0.125, 0, 0.125),   // half a step: nothing yet
            (0.125, 1, 0.0),     // completes the step
            (0.25, 1, 0.0),      // exactly one step
            (0.5625, 2, 0.0625), // 2.25 steps
            (0.1875, 1, 0.0),    // completes the quarter step left over
            (1.0, 4, 0.0),
        ];
        for (frame, (wall, ticks, remainder)) in frames.into_iter().enumerate() {
            assert_eq!(clock.advance(wall), adv(ticks, 0.0), "frame {frame}");
            assert_eq!(clock.remainder(), remainder, "frame {frame}");
        }
    }

    #[test]
    fn short_frames_accumulate() {
        let mut clock = FixedClock::new(STEP, 4);
        let ticks: Vec<u32> = (0..8).map(|_| clock.advance(STEP / 4.0).ticks).collect();
        assert_eq!(ticks, [0, 0, 0, 1, 0, 0, 0, 1]);
    }

    #[test]
    fn cap_limits_ticks_and_drops_only_whole_steps() {
        let mut clock = FixedClock::new(0.25, 3);
        // Exactly three steps reach the cap but leave nothing to drop.
        assert_eq!(clock.advance(0.75), adv(3, 0.0));
        // 3.5 steps: the half step carries and nothing is dropped.
        assert_eq!(clock.advance(0.875), adv(3, 0.0));
        assert_eq!(clock.remainder(), 0.125);
        // 0.125 + 1.0 is 4.5 steps: one whole step over the cap is dropped,
        // and the half step still carries.
        assert_eq!(clock.advance(1.0), adv(3, 0.25));
        assert_eq!(clock.remainder(), 0.125);
        // The dropped step is not owed later: this frame only completes the
        // half step that carried.
        assert_eq!(clock.advance(0.125), adv(1, 0.0));
        assert_eq!(clock.remainder(), 0.0);
    }

    #[test]
    fn a_long_stall_does_not_spiral() {
        // An hour at once, as after waking a laptop from sleep.
        let mut clock = FixedClock::new(STEP, 8);
        assert_eq!(clock.advance(3600.0), adv(8, 3600.0 - 8.0 * STEP));
        assert_eq!(clock.remainder(), 0.0);
        // The next normal frame runs one tick, not a backlog.
        assert_eq!(clock.advance(STEP), adv(1, 0.0));
    }

    #[test]
    fn bad_wall_seconds_count_as_zero() {
        let mut clock = FixedClock::new(0.25, 4);
        assert_eq!(clock.advance(0.125).ticks, 0);
        let bad = [
            f64::NAN,
            -f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN,
            -1.0,
            -0.0,
        ];
        for wall in bad {
            assert_eq!(clock.advance(wall), adv(0, 0.0), "{wall}");
            assert_eq!(clock.remainder(), 0.125, "{wall} changed the accumulator");
        }
        assert_eq!(clock.advance(0.125).ticks, 1, "the clock stopped working");
    }

    #[test]
    fn extreme_values_stay_finite() {
        // (step, max ticks, seconds fed by each of two calls)
        let cases = [
            (STEP, 2, f64::MAX),
            (f64::MAX, 2, f64::MAX),
            // Without the `min` in `advance`, the second call would overflow
            // the accumulator to infinity and make it NaN.
            (f64::MAX, 2, 1.5e308),
            (5e-324, 2, 1.0),
        ];
        for (step, max_ticks, wall) in cases {
            let mut clock = FixedClock::new(step, max_ticks);
            for _ in 0..2 {
                let advance = clock.advance(wall);
                let case = format!("step {step:e}, fed {wall:e}: {advance:?}");
                assert!(advance.ticks <= max_ticks, "{case}");
                assert!(
                    advance.dropped.is_finite() && advance.dropped >= 0.0,
                    "{case}"
                );
                assert!((0.0..step).contains(&clock.remainder()), "{case}");
            }
        }
    }

    #[test]
    fn time_is_conserved() {
        // Every second fed in is run as a tick, dropped by the cap or still in
        // the accumulator. The frames are multiples of 1/1024 s and STEP is
        // 1/64 s, so all of this arithmetic is exact.
        let mut clock = FixedClock::new(STEP, 4);
        let (mut fed, mut ticks, mut dropped) = (0.0, 0_u32, 0.0);
        for frame in 0..1000_u32 {
            // Irregular frames from 0 to 6.25 steps long.
            let wall = f64::from(frame * 37 % 101) / 1024.0;
            let advance = clock.advance(wall);
            fed += wall;
            ticks += advance.ticks;
            dropped += advance.dropped;
            assert!(advance.ticks <= 4, "frame {frame}");
            assert_eq!((advance.dropped / STEP).fract(), 0.0, "frame {frame}");
            assert!((0.0..STEP).contains(&clock.remainder()), "frame {frame}");
            let accounted = f64::from(ticks) * STEP + dropped + clock.remainder();
            assert_eq!(fed, accounted, "frame {frame}");
        }
        assert!(dropped > 0.0, "the frames never hit the cap");
    }

    #[test]
    fn non_exact_steps_do_not_drift() {
        // 1/60 s and 1 ms are not exact in binary, so each subtraction rounds.
        // Over a minute of 1 ms frames the ticks still match the time fed.
        let step = 1.0 / 60.0;
        let mut clock = FixedClock::new(step, u32::MAX);
        let (mut fed, mut ticks) = (0.0_f64, 0_u64);
        for frame in 0..60_000 {
            let advance = clock.advance(0.001);
            assert_eq!(advance.dropped, 0.0, "frame {frame}");
            ticks += u64::from(advance.ticks);
            fed += 0.001;
            assert!((0.0..step).contains(&clock.remainder()), "frame {frame}");
        }
        // `fed / step` is about 3600, so the cast is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let expected = (fed / step).floor() as u64;
        assert!(
            ticks.abs_diff(expected) <= 1,
            "{ticks} ticks, {expected} expected"
        );
    }

    #[test]
    fn a_cap_of_one_runs_one_tick_per_call() {
        let mut clock = FixedClock::new(0.25, 1);
        assert_eq!(clock.advance(0.25), adv(1, 0.0));
        assert_eq!(clock.advance(0.125), adv(0, 0.0));
        // 0.125 + 0.5 is 2.5 steps: one runs, one is dropped, half carries.
        assert_eq!(clock.advance(0.5), adv(1, 0.25));
        assert_eq!(clock.remainder(), 0.125);
        // A bad reading right after a capped call drops nothing more.
        assert_eq!(clock.advance(f64::NAN), adv(0, 0.0));
        assert_eq!(clock.remainder(), 0.125);
        assert_eq!(clock.advance(0.125), adv(1, 0.0));
        assert_eq!(clock.remainder(), 0.0);
    }

    #[test]
    fn tiny_wall_seconds_are_absorbed() {
        let mut clock = FixedClock::new(0.25, 4);
        assert_eq!(clock.advance(0.125).ticks, 0);
        for wall in [f64::MIN_POSITIVE, 5e-324] {
            assert_eq!(clock.advance(wall), adv(0, 0.0), "{wall:e}");
            // Too small to change 0.125 at all.
            assert_eq!(clock.remainder(), 0.125, "{wall:e}");
        }
    }

    macro_rules! rejects_step {
        ($($name:ident: $step:expr,)*) => {$(
            #[test]
            #[should_panic(expected = "step must be finite and greater than zero")]
            fn $name() {
                let _ = FixedClock::new($step, 1);
            }
        )*};
    }

    rejects_step! {
        new_rejects_zero_step: 0.0,
        new_rejects_negative_zero_step: -0.0,
        new_rejects_negative_step: -STEP,
        new_rejects_nan_step: f64::NAN,
        new_rejects_infinite_step: f64::INFINITY,
        new_rejects_negative_infinite_step: f64::NEG_INFINITY,
    }

    #[test]
    #[should_panic(expected = "max_ticks must be at least 1")]
    fn new_rejects_zero_max_ticks() {
        let _ = FixedClock::new(STEP, 0);
    }
}
