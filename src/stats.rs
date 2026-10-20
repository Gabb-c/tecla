//! Results computed from a finished [`Session`].
//!
//! A "character" is one grapheme cluster, and a word is five characters.
//!
//! - **Net WPM** = (correctly typed graphemes in the final input / 5) / minutes elapsed
//! - **Raw WPM** = (all typed graphemes in the final input / 5) / minutes elapsed
//! - **Accuracy** = correct keystrokes / total character keystrokes × 100, counted over the
//!   keystroke log. An error corrected later still counts as an error; backspace is not a
//!   character keystroke.
//!
//! Zero elapsed time yields 0 WPM, and zero character keystrokes yield 0% accuracy.

use std::time::Duration;

use crate::engine::{GraphemeState, Keystroke, Session};

const CHARS_PER_WORD: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Results {
    pub net_wpm: f64,
    pub raw_wpm: f64,
    /// Percentage in `0.0..=100.0`.
    pub accuracy: f64,
    pub correct_chars: usize,
    pub incorrect_chars: usize,
    pub duration: Duration,
}

impl Results {
    pub fn from_session(session: &Session) -> Self {
        let correct_chars = (0..session.cursor())
            .filter(|&i| session.state_at(i) == GraphemeState::Correct)
            .count();
        let incorrect_chars = session.cursor() - correct_chars;
        let duration = session.elapsed();
        Self {
            net_wpm: wpm(correct_chars, duration),
            raw_wpm: wpm(session.cursor(), duration),
            accuracy: accuracy(session.keystrokes()),
            correct_chars,
            incorrect_chars,
            duration,
        }
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
    use std::time::Instant;

    use super::*;
    use crate::engine::Input;

    fn finished(target: &str, inputs: &[Input], limit: Duration) -> Session {
        let start = Instant::now();
        let mut session = Session::new(target, limit);
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
                // 10 correct / 5 = 2 words in half a minute
                net_wpm: 4.0,
                // 11 typed / 5 = 2.2 words in half a minute
                raw_wpm: 4.4,
                accuracy: 10.0 / 11.0 * 100.0,
                correct_chars: 10,
                incorrect_chars: 1,
                duration: Duration::from_secs(30),
            }
        );
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
        let session = Session::new("abc", Duration::from_secs(30));
        let results = Results::from_session(&session);

        assert_eq!(results.net_wpm, 0.0);
        assert_eq!(results.raw_wpm, 0.0);
        assert_eq!(results.accuracy, 0.0);
        assert!(results.net_wpm.is_finite() && results.accuracy.is_finite());
    }
}
