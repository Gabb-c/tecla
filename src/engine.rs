//! A single timed typing session.
//!
//! Typed input is compared positionally against the target text, one grapheme cluster at a
//! time, spaces included. The caller supplies every timestamp so the session stays
//! deterministic.

use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;

/// Abstract user input fed into a [`Session`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Char(char),
    Backspace,
}

/// One entry of the keystroke log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keystroke {
    Char { correct: bool },
    Backspace,
}

/// How a target grapheme renders given the current input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphemeState {
    Untyped,
    Correct,
    Incorrect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Waiting,
    Running { started_at: Instant },
    Finished { elapsed: Duration },
}

#[derive(Debug, Clone)]
pub struct Session {
    target: Vec<String>,
    typed: Vec<String>,
    keystrokes: Vec<Keystroke>,
    duration: Duration,
    phase: Phase,
}

impl Session {
    pub fn new(target: &str, duration: Duration) -> Self {
        Self {
            target: graphemes(target),
            typed: Vec::new(),
            keystrokes: Vec::new(),
            duration,
            phase: Phase::Waiting,
        }
    }

    /// Appends more target text, e.g. when the user is about to run out of words.
    pub fn extend(&mut self, text: &str) {
        self.target.extend(graphemes(text));
    }

    /// Applies one input at `now`. The first character keypress starts the timer; input after
    /// the time limit is ignored.
    pub fn handle(&mut self, input: Input, now: Instant) {
        self.tick(now);
        match (self.phase, input) {
            (Phase::Finished { .. }, _) | (Phase::Waiting, Input::Backspace) => return,
            (Phase::Waiting, Input::Char(_)) => self.phase = Phase::Running { started_at: now },
            (Phase::Running { .. }, _) => {}
        }
        match input {
            Input::Char(c) => self.type_char(c),
            Input::Backspace => self.backspace(),
        }
    }

    /// Finishes the session once its time limit has passed.
    pub fn tick(&mut self, now: Instant) {
        if let Phase::Running { started_at } = self.phase
            && now.saturating_duration_since(started_at) >= self.duration
        {
            self.phase = Phase::Finished {
                elapsed: self.duration,
            };
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.phase, Phase::Finished { .. })
    }

    pub fn is_started(&self) -> bool {
        !matches!(self.phase, Phase::Waiting)
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        match self.phase {
            Phase::Waiting => self.duration,
            Phase::Running { started_at } => self
                .duration
                .saturating_sub(now.saturating_duration_since(started_at)),
            Phase::Finished { .. } => Duration::ZERO,
        }
    }

    /// Time spent typing; zero until the session has finished.
    pub fn elapsed(&self) -> Duration {
        match self.phase {
            Phase::Finished { elapsed } => elapsed,
            _ => Duration::ZERO,
        }
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn target(&self) -> &[String] {
        &self.target
    }

    pub fn typed(&self) -> &[String] {
        &self.typed
    }

    pub fn keystrokes(&self) -> &[Keystroke] {
        &self.keystrokes
    }

    /// Index of the next target grapheme to type.
    pub fn cursor(&self) -> usize {
        self.typed.len()
    }

    pub fn state_at(&self, index: usize) -> GraphemeState {
        match self.typed.get(index) {
            None => GraphemeState::Untyped,
            Some(typed) if self.target.get(index) == Some(typed) => GraphemeState::Correct,
            Some(_) => GraphemeState::Incorrect,
        }
    }

    fn type_char(&mut self, c: char) {
        let joins_last = self.typed.last().is_some_and(|last| joins(last, c));
        if joins_last {
            if let Some(last) = self.typed.last_mut() {
                last.push(c);
            }
        } else if self.typed.len() < self.target.len() {
            self.typed.push(c.to_string());
        } else {
            return;
        }
        let index = self.typed.len() - 1;
        // a partial cluster such as "e" before its combining accent is still on track
        let correct = self.target[index].starts_with(self.typed[index].as_str());
        self.keystrokes.push(Keystroke::Char { correct });
    }

    fn backspace(&mut self) {
        if self.typed.pop().is_some() {
            self.keystrokes.push(Keystroke::Backspace);
        }
    }
}

fn graphemes(text: &str) -> Vec<String> {
    text.graphemes(true).map(str::to_owned).collect()
}

/// Whether `c` extends the grapheme cluster `last` instead of starting a new one.
fn joins(last: &str, c: char) -> bool {
    let mut merged = last.to_owned();
    merged.push(c);
    merged.graphemes(true).count() == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Duration = Duration::from_secs(30);

    fn type_str(session: &mut Session, text: &str, now: Instant) {
        for c in text.chars() {
            session.handle(Input::Char(c), now);
        }
    }

    fn states(session: &Session) -> Vec<GraphemeState> {
        (0..session.target().len())
            .map(|i| session.state_at(i))
            .collect()
    }

    #[test]
    fn correct_typing() {
        let now = Instant::now();
        let mut session = Session::new("hi you", LIMIT);
        type_str(&mut session, "hi you", now);

        assert_eq!(session.typed().concat(), "hi you");
        assert!(
            states(&session)
                .iter()
                .all(|s| *s == GraphemeState::Correct)
        );
        assert!(
            session
                .keystrokes()
                .iter()
                .all(|k| *k == Keystroke::Char { correct: true })
        );
    }

    #[test]
    fn mistakes_are_marked_incorrect() {
        use GraphemeState::*;

        let now = Instant::now();
        let mut session = Session::new("abc", LIMIT);
        type_str(&mut session, "axc", now);

        assert_eq!(states(&session), [Correct, Incorrect, Correct]);
        assert_eq!(session.keystrokes()[1], Keystroke::Char { correct: false });
    }

    #[test]
    fn space_is_compared_positionally() {
        let now = Instant::now();
        let mut session = Session::new("ab cd", LIMIT);
        type_str(&mut session, "abc", now);

        assert_eq!(session.state_at(2), GraphemeState::Incorrect);
        assert_eq!(session.cursor(), 3);
    }

    #[test]
    fn backspace_removes_last_grapheme() {
        let now = Instant::now();
        let mut session = Session::new("abc", LIMIT);
        type_str(&mut session, "ax", now);
        session.handle(Input::Backspace, now);

        assert_eq!(session.typed().concat(), "a");
        assert_eq!(session.state_at(1), GraphemeState::Untyped);
        assert_eq!(session.keystrokes().last(), Some(&Keystroke::Backspace));
    }

    #[test]
    fn backspace_crosses_word_boundary() {
        let now = Instant::now();
        let mut session = Session::new("ab cd", LIMIT);
        type_str(&mut session, "ab c", now);
        for _ in 0..3 {
            session.handle(Input::Backspace, now);
        }

        assert_eq!(session.typed().concat(), "a");
        assert_eq!(session.cursor(), 1);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let now = Instant::now();
        let mut session = Session::new("abc", LIMIT);
        session.handle(Input::Backspace, now);
        assert_eq!(session.typed(), Vec::<String>::new());
        assert_eq!(session.keystrokes(), Vec::<Keystroke>::new());
        assert!(!session.is_started());

        type_str(&mut session, "a", now);
        session.handle(Input::Backspace, now);
        session.handle(Input::Backspace, now);
        assert_eq!(session.typed(), Vec::<String>::new());
        assert_eq!(session.keystrokes().len(), 2);
    }

    #[test]
    fn typing_past_end_is_ignored() {
        let now = Instant::now();
        let mut session = Session::new("ab", LIMIT);
        type_str(&mut session, "abcd", now);

        assert_eq!(session.typed().concat(), "ab");
        assert_eq!(session.keystrokes().len(), 2);
    }

    #[test]
    fn extend_allows_typing_further() {
        let now = Instant::now();
        let mut session = Session::new("ab", LIMIT);
        session.extend(" cd");
        type_str(&mut session, "ab c", now);

        assert_eq!(session.target().concat(), "ab cd");
        assert_eq!(session.cursor(), 4);
    }

    #[test]
    fn precomposed_accents_are_single_graphemes() {
        use GraphemeState::*;

        let now = Instant::now();
        let mut session = Session::new("ação", LIMIT);
        assert_eq!(session.target().len(), 4);

        type_str(&mut session, "acão", now);
        assert_eq!(states(&session), [Correct, Incorrect, Correct, Correct]);
    }

    #[test]
    fn combining_mark_joins_previous_grapheme() {
        let now = Instant::now();
        let mut session = Session::new("e\u{301}x", LIMIT);
        assert_eq!(session.target().len(), 2);

        type_str(&mut session, "e\u{301}", now);
        assert_eq!(session.typed(), ["e\u{301}"]);
        assert_eq!(session.state_at(0), GraphemeState::Correct);
        assert_eq!(session.keystrokes(), [Keystroke::Char { correct: true }; 2]);

        session.handle(Input::Backspace, now);
        assert_eq!(session.typed(), Vec::<String>::new());
    }

    #[test]
    fn combining_mark_on_wrong_base_is_incorrect() {
        let now = Instant::now();
        let mut session = Session::new("e\u{301}", LIMIT);
        type_str(&mut session, "a\u{301}", now);

        assert_eq!(session.state_at(0), GraphemeState::Incorrect);
        assert_eq!(
            session.keystrokes(),
            [Keystroke::Char { correct: false }; 2]
        );
    }

    #[test]
    fn timer_starts_on_first_key() {
        let t0 = Instant::now();
        let mut session = Session::new("abc", LIMIT);

        session.tick(t0 + LIMIT * 2);
        assert!(!session.is_started());
        assert_eq!(session.remaining(t0 + LIMIT * 2), LIMIT);

        let start = t0 + Duration::from_secs(100);
        session.handle(Input::Char('a'), start);
        assert!(session.is_started());
        assert_eq!(
            session.remaining(start + Duration::from_secs(10)),
            Duration::from_secs(20)
        );

        session.tick(start + LIMIT.saturating_sub(Duration::from_millis(1)));
        assert!(!session.is_finished());
        session.tick(start + LIMIT);
        assert!(session.is_finished());
        assert_eq!(session.elapsed(), LIMIT);
        assert_eq!(session.remaining(start + LIMIT), Duration::ZERO);
    }

    #[test]
    fn input_after_end_is_ignored() {
        let start = Instant::now();
        let mut session = Session::new("abc", LIMIT);
        session.handle(Input::Char('a'), start);

        // the expired timer is noticed even without a tick in between
        session.handle(Input::Char('b'), start + LIMIT);
        assert!(session.is_finished());
        session.handle(Input::Backspace, start + LIMIT);

        assert_eq!(session.typed().concat(), "a");
        assert_eq!(session.keystrokes().len(), 1);
    }
}
