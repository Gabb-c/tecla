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

use std::time::{Duration, Instant};

use crate::engine::{Keystroke, LetterState, Session};

const CHARS_PER_WORD: f64 = 5.0;

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
}

impl Results {
    pub fn from_session(session: &Session) -> Self {
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
            errors: keystrokes
                .iter()
                .filter(|key| matches!(key, Keystroke::Char { correct: false }))
                .count(),
        }
    }
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
        let results = Results::from_session(&session);

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
            }
        );
    }

    #[test]
    fn extra_and_missed_letters_are_counted() {
        let session = finished("abc de fg", &chars("abcx d f"), Duration::from_secs(60));
        let results = Results::from_session(&session);

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
        let results = Results::from_session(&session);

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
        let results = Results::from_session(&session);

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
