//! Results computed from a [`Session`], when finished or live while it runs.
//!
//! A "character" is one grapheme cluster, and a word is five characters.
//!
//! - **Net WPM** = (characters of correct words, plus the space after each, plus the current
//!   word's characters if it is correct so far / 5) / minutes elapsed
//! - **Raw WPM** = (all typed characters, extra ones and spaces included / 5) / minutes elapsed
//! - **Accuracy** = correct keystrokes / total character keystrokes × 100, counted over the
//!   keystroke log. An error corrected later still counts as an error; backspace is not a
//!   character keystroke.
//!
//! Letters are counted as correct, incorrect, extra (typed past the end of a word) or missed
//! (skipped by moving on to the next word).
//!
//! Zero elapsed time yields 0 WPM, and zero character keystrokes yield 0% accuracy.
//!
//! **Consistency** = `kogasa(σ / μ)` over the raw WPM of each roughly one-second interval, with
//! the mean and deviation weighted by interval length. A final interval under [`MIN_TAIL`] is
//! merged into the one before it. It is undefined with fewer than two intervals or no
//! keystrokes.

use std::time::{Duration, Instant};

use crate::engine::{Keystroke, LetterState, Limit, Session};

const CHARS_PER_WORD: f64 = 5.0;
/// Shortest final interval kept on its own; a few keystrokes over less would spike the chart.
pub const MIN_TAIL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Results {
    pub net_wpm: f64,
    pub raw_wpm: f64,
    /// Percentage in `0.0..=100.0`.
    pub accuracy: f64,
    pub correct_chars: usize,
    pub incorrect_chars: usize,
    pub extra_chars: usize,
    pub missed_chars: usize,
    pub duration: Duration,
    /// Percentage in `0.0..=100.0`; `None` when undefined, see [`Timeline::consistency`].
    pub consistency: Option<f64>,
}

impl Results {
    pub fn from_session(session: &Session, timeline: &Timeline) -> Self {
        let tally = Tally::from_session(session);
        let duration = session.elapsed();
        Self {
            net_wpm: wpm(tally.net, duration),
            raw_wpm: wpm(tally.raw, duration),
            accuracy: accuracy(session.keystrokes()),
            correct_chars: tally.correct,
            incorrect_chars: tally.incorrect,
            extra_chars: tally.extra,
            missed_chars: tally.missed,
            duration,
            consistency: timeline.consistency(),
        }
    }
}

/// Stats shown while a session is running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Live {
    pub wpm: f64,
    /// Percentage in `0.0..=100.0`.
    pub accuracy: f64,
    /// Incorrect character keystrokes, including corrected ones.
    pub errors: usize,
}

impl Live {
    pub fn at(session: &Session, now: Instant) -> Self {
        let keystrokes = session.keystrokes();
        Self {
            wpm: wpm(Tally::from_session(session).net, session.elapsed_at(now)),
            accuracy: accuracy(keystrokes),
            errors: count(keystrokes, is_error),
        }
    }
}

fn count(keystrokes: &[Keystroke], matches: fn(Keystroke) -> bool) -> usize {
    keystrokes.iter().filter(|&&key| matches(key)).count()
}

fn is_char(key: Keystroke) -> bool {
    matches!(key, Keystroke::Char { .. })
}

fn is_error(key: Keystroke) -> bool {
    matches!(key, Keystroke::Char { correct: false })
}

/// A running session's stats at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Time since the session started.
    pub at: Duration,
    /// Net WPM so far.
    pub wpm: f64,
    /// Character keystrokes so far; unlike typed letters, backspace never lowers it.
    pub keystrokes: usize,
    /// Incorrect character keystrokes so far.
    pub errors: usize,
}

impl Sample {
    const ORIGIN: Self = Self {
        at: Duration::ZERO,
        wpm: 0.0,
        keystrokes: 0,
        errors: 0,
    };

    fn of(session: &Session, at: Duration) -> Self {
        let keystrokes = session.keystrokes();
        Self {
            at,
            wpm: wpm(Tally::from_session(session).net, at),
            keystrokes: count(keystrokes, is_char),
            errors: count(keystrokes, is_error),
        }
    }
}

/// Typing between two consecutive samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Burst {
    pub end: Duration,
    pub length: Duration,
    /// Every character keystroke in the interval, as WPM.
    pub raw_wpm: f64,
    pub errors: usize,
}

/// Samples of a session taken about once a second, for the results chart and consistency.
///
/// Fed the same timestamps as the session, so it stays deterministic.
#[derive(Debug, Clone, Default)]
pub struct Timeline {
    samples: Vec<Sample>,
}

impl Timeline {
    /// Samples `session` when it has run into a new whole second since the last sample. Several
    /// skipped seconds give one longer interval.
    pub fn observe(&mut self, session: &Session, now: Instant) {
        if session.is_finished() {
            return;
        }
        let at = session.elapsed_at(now);
        // past the limit but not yet ticked; `finish` samples the end
        if let Limit::Time(limit) = session.limit()
            && at >= limit
        {
            return;
        }
        let last = self.samples.last().map_or(0, |s| s.at.as_secs());
        if at.as_secs() > last {
            self.samples.push(Sample::of(session, at));
        }
    }

    /// Samples the end of a finished `session`. A final interval shorter than [`MIN_TAIL`]
    /// replaces the last sample, extending the interval before it.
    pub fn finish(&mut self, session: &Session) {
        let end = session.elapsed();
        let last = self.samples.last().map_or(Duration::ZERO, |s| s.at);
        if end <= last {
            return;
        }
        if end.saturating_sub(last) < MIN_TAIL {
            self.samples.pop();
        }
        self.samples.push(Sample::of(session, end));
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    pub fn bursts(&self) -> impl Iterator<Item = Burst> + '_ {
        self.samples.iter().scan(Sample::ORIGIN, |prev, &sample| {
            let length = sample.at.saturating_sub(prev.at);
            let burst = Burst {
                end: sample.at,
                length,
                raw_wpm: wpm(sample.keystrokes - prev.keystrokes, length),
                errors: sample.errors - prev.errors,
            };
            *prev = sample;
            Some(burst)
        })
    }

    /// How steady the raw WPM stayed, from 0 to 100; `None` with fewer than two intervals or
    /// no keystrokes.
    pub fn consistency(&self) -> Option<f64> {
        let bursts: Vec<(f64, f64)> = self
            .bursts()
            .map(|b| (b.length.as_secs_f64(), b.raw_wpm))
            .collect();
        weighted_consistency(&bursts)
    }
}

/// Consistency of `(seconds, raw wpm)` intervals, each weighted by its length.
fn weighted_consistency(bursts: &[(f64, f64)]) -> Option<f64> {
    if bursts.len() < 2 {
        return None;
    }
    let total: f64 = bursts.iter().map(|(secs, _)| secs).sum();
    let mean = bursts.iter().map(|(secs, raw)| secs * raw).sum::<f64>() / total;
    if mean == 0.0 {
        return None;
    }
    let variance = bursts
        .iter()
        .map(|(secs, raw)| secs * (raw - mean).powi(2))
        .sum::<f64>()
        / total;
    Some(kogasa(variance.sqrt() / mean))
}

/// Maps a coefficient of variation to a 0–100 score.
fn kogasa(cov: f64) -> f64 {
    100.0 * (1.0 - (cov + cov.powi(3) / 3.0 + cov.powi(5) / 5.0).tanh())
}

/// Character counts over every word reached in a session.
#[derive(Debug, Default)]
struct Tally {
    correct: usize,
    incorrect: usize,
    extra: usize,
    missed: usize,
    /// Characters counted towards net WPM.
    net: usize,
    /// Characters counted towards raw WPM.
    raw: usize,
}

impl Tally {
    fn from_session(session: &Session) -> Self {
        let mut tally = Self::default();
        for (word, typed) in session.typed().iter().enumerate() {
            let target = &session.words()[word];
            for letter in 0..target.len() {
                match session.letter_state(word, letter) {
                    LetterState::Correct => tally.correct += 1,
                    LetterState::Incorrect => tally.incorrect += 1,
                    LetterState::Missed => tally.missed += 1,
                    LetterState::Untyped => {}
                }
            }
            tally.extra += session.extra(word).len();

            let committed = word < session.current_word();
            let space = usize::from(committed);
            tally.raw += typed.len() + space;
            let on_track = if committed {
                session.is_word_correct(word)
            } else {
                session.is_on_track(word)
            };
            if on_track {
                tally.net += typed.len() + space;
            }
        }
        tally
    }
}

pub fn wpm(chars: usize, elapsed: Duration) -> f64 {
    let minutes = elapsed.as_secs_f64() / 60.0;
    if minutes == 0.0 {
        return 0.0;
    }
    chars as f64 / CHARS_PER_WORD / minutes
}

pub fn accuracy(keystrokes: &[Keystroke]) -> f64 {
    let (correct, total) = keystrokes
        .iter()
        .fold((0usize, 0usize), |(correct, total), key| match key {
            Keystroke::Char { correct: true } => (correct + 1, total + 1),
            Keystroke::Char { correct: false } => (correct, total + 1),
            Keystroke::Backspace => (correct, total),
        });
    if total == 0 {
        return 0.0;
    }
    correct as f64 / total as f64 * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Input, Limit};
    use crate::test_util::steady_timeline;

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    /// Samples, then types `text`, all at `start + at`.
    fn type_at(
        session: &mut Session,
        timeline: &mut Timeline,
        text: &str,
        start: Instant,
        at: f64,
    ) {
        let now = start + secs(at);
        timeline.observe(session, now);
        for input in chars(text) {
            session.handle(input, now);
        }
    }

    fn sample_times(timeline: &Timeline) -> Vec<Duration> {
        timeline.samples().iter().map(|s| s.at).collect()
    }

    /// A words test over "ab cd" with "ab " typed at the start and "cd" at `end`.
    fn words_test(end: f64) -> Timeline {
        let start = Instant::now();
        let mut session = Session::new(["ab", "cd"], Limit::Words);
        let mut timeline = Timeline::default();
        type_at(&mut session, &mut timeline, "ab ", start, 0.0);
        type_at(&mut session, &mut timeline, "", start, 1.0);
        type_at(&mut session, &mut timeline, "cd", start, end);
        timeline.finish(&session);
        timeline
    }

    #[test]
    fn timeline_samples_once_per_whole_second() {
        assert_eq!(
            sample_times(&steady_timeline(3)),
            [secs(1.0), secs(2.0), secs(3.0)]
        );
    }

    #[test]
    fn skipped_seconds_give_one_longer_interval() {
        let start = Instant::now();
        let mut session = Session::new(["a"; 8], Limit::Time(secs(10.0)));
        let mut timeline = Timeline::default();
        type_at(&mut session, &mut timeline, "a", start, 0.0);
        type_at(&mut session, &mut timeline, "", start, 3.2);

        assert_eq!(sample_times(&timeline), [secs(3.2)]);
    }

    #[test]
    fn short_tail_merges_into_previous_interval() {
        assert_eq!(sample_times(&words_test(1.2)), [secs(1.2)]);
    }

    #[test]
    fn long_tail_is_its_own_interval() {
        assert_eq!(sample_times(&words_test(1.7)), [secs(1.0), secs(1.7)]);
    }

    #[test]
    fn bursts_count_every_char_keystroke() {
        let start = Instant::now();
        let mut session = Session::new(["ab"; 8], Limit::Time(secs(10.0)));
        let mut timeline = Timeline::default();
        type_at(&mut session, &mut timeline, "ax", start, 0.0);
        session.handle(Input::Backspace, start);
        type_at(&mut session, &mut timeline, "b", start, 0.5);
        type_at(&mut session, &mut timeline, "", start, 1.0);

        let bursts: Vec<Burst> = timeline.bursts().collect();
        assert_eq!(
            bursts,
            [Burst {
                end: secs(1.0),
                length: secs(1.0),
                // 3 keystrokes / 5 = 0.6 words in a sixtieth of a minute
                raw_wpm: 36.0,
                errors: 1,
            }]
        );
    }

    #[test]
    fn steady_typing_is_fully_consistent() {
        assert_eq!(steady_timeline(5).consistency(), Some(100.0));
    }

    #[test]
    fn consistency_needs_two_intervals() {
        assert_eq!(steady_timeline(1).consistency(), None);
    }

    #[test]
    fn consistency_weights_intervals_by_length() {
        // mean (60 + 2 × 30) / 3 = 40, variance (400 + 2 × 100) / 3 = 200
        let expected = kogasa(200.0_f64.sqrt() / 40.0);
        assert_eq!(
            weighted_consistency(&[(1.0, 60.0), (2.0, 30.0)]),
            Some(expected)
        );
    }

    #[test]
    fn kogasa_of_no_variation_is_full_score() {
        assert_eq!(kogasa(0.0), 100.0);
    }

    #[test]
    fn kogasa_of_half_variation() {
        let score = kogasa(0.5);
        assert!((score - 50.104_283).abs() < 1e-5, "{score}");
    }

    #[test]
    fn kogasa_of_full_variation() {
        let score = kogasa(1.0);
        assert!((score - 8.900_671).abs() < 1e-5, "{score}");
    }

    #[test]
    fn observe_past_time_limit_takes_no_sample() {
        let start = Instant::now();
        let mut session = Session::new(["a"; 8], Limit::Time(secs(2.0)));
        let mut timeline = Timeline::default();
        type_at(&mut session, &mut timeline, "a", start, 0.0);
        type_at(&mut session, &mut timeline, "", start, 2.1);

        assert_eq!(sample_times(&timeline), Vec::<Duration>::new());
    }

    fn finished(target: &str, inputs: &[Input], limit: Duration) -> Session {
        let start = Instant::now();
        let mut session = Session::new(target.split_whitespace(), Limit::Time(limit));
        for &input in inputs {
            session.handle(input, start);
        }
        session.tick(start + limit);
        session
    }

    fn chars(text: &str) -> Vec<Input> {
        text.chars().map(Input::Char).collect()
    }

    #[test]
    fn known_input_gives_exact_results() {
        let session = finished(
            "hello world",
            &chars("hello wrrld"),
            Duration::from_secs(30),
        );
        let results = Results::from_session(&session, &Timeline::default());

        assert_eq!(
            results,
            Results {
                // "hello " = 6 chars / 5 = 1.2 words in half a minute; "wrrld" is off track
                net_wpm: 2.4,
                // 11 typed / 5 = 2.2 words in half a minute
                raw_wpm: 4.4,
                accuracy: 10.0 / 11.0 * 100.0,
                correct_chars: 9,
                incorrect_chars: 1,
                extra_chars: 0,
                missed_chars: 0,
                duration: Duration::from_secs(30),
                consistency: None,
            }
        );
    }

    #[test]
    fn extra_and_missed_letters_are_counted() {
        let session = finished("abc de fg", &chars("abcx d f"), Duration::from_secs(60));
        let results = Results::from_session(&session, &Timeline::default());

        assert_eq!(results.correct_chars, 5);
        assert_eq!(results.incorrect_chars, 0);
        assert_eq!(results.extra_chars, 1);
        assert_eq!(results.missed_chars, 1);
        // only "f" of the current word is on track
        assert_eq!(results.net_wpm, 0.2);
        // "abcx d f" = 8 chars
        assert_eq!(results.raw_wpm, 1.6);
        // x and both spaces after wrong words are errors
        assert_eq!(results.accuracy, 62.5);
    }

    #[test]
    fn corrected_errors_still_lower_accuracy() {
        let mut inputs = chars("ax");
        inputs.push(Input::Backspace);
        inputs.extend(chars("bcd"));
        let session = finished("abcd", &inputs, Duration::from_secs(60));
        let results = Results::from_session(&session, &Timeline::default());

        assert_eq!(results.correct_chars, 4);
        assert_eq!(results.incorrect_chars, 0);
        assert_eq!(results.net_wpm, 0.8);
        assert_eq!(results.accuracy, 80.0);
    }

    #[test]
    fn partial_cluster_counts_towards_live_wpm() {
        let start = Instant::now();
        let mut session = Session::new(
            "e\u{301}".split_whitespace(),
            Limit::Time(Duration::from_secs(60)),
        );
        session.handle(Input::Char('e'), start);
        let live = Live::at(&session, start + Duration::from_secs(12));

        // 1 char / 5 = 0.2 words in a fifth of a minute
        assert_eq!(live.wpm, 1.0);
    }

    #[test]
    fn zero_elapsed_time_gives_zero_wpm() {
        assert_eq!(wpm(50, Duration::ZERO), 0.0);
    }

    #[test]
    fn no_keystrokes_gives_zero_accuracy() {
        assert_eq!(accuracy(&[]), 0.0);
        assert_eq!(accuracy(&[Keystroke::Backspace]), 0.0);
    }

    #[test]
    fn unstarted_session_gives_zero_results() {
        let session = Session::new(
            "abc".split_whitespace(),
            Limit::Time(Duration::from_secs(30)),
        );
        let results = Results::from_session(&session, &Timeline::default());

        assert_eq!(results.net_wpm, 0.0);
        assert_eq!(results.raw_wpm, 0.0);
        assert_eq!(results.accuracy, 0.0);
        assert!(results.net_wpm.is_finite() && results.accuracy.is_finite());
    }

    #[test]
    fn live_stats_use_elapsed_time_so_far() {
        let start = Instant::now();
        let mut session = Session::new(
            "hello world".split_whitespace(),
            Limit::Time(Duration::from_secs(60)),
        );
        for input in chars("hello wx") {
            session.handle(input, start);
        }
        let live = Live::at(&session, start + Duration::from_secs(30));

        // "hello " = 6 chars / 5 = 1.2 words in half a minute; "wx" is off track
        assert_eq!(live.wpm, 2.4);
        assert_eq!(live.errors, 1);
        assert_eq!(live.accuracy, 87.5);
    }
}
