//! A single timed typing session.
//!
//! Input is matched word by word, like Monkeytype. Letters are compared positionally within the
//! current word, one grapheme cluster at a time; letters past the end of a word are extra, and
//! space moves on to the next word, leaving any untyped letters missed. The caller supplies
//! every timestamp so the session stays deterministic.

use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;

/// Extra letters accepted past the end of a word.
const MAX_EXTRA: usize = 10;

/// Abstract user input fed into a [`Session`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// A whitespace character moves on to the next word.
    Char(char),
    Backspace,
    DeleteWord,
}

/// One entry of the keystroke log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keystroke {
    Char { correct: bool },
    Backspace,
}

/// How a target letter renders given the current input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LetterState {
    Untyped,
    Correct,
    Incorrect,
    /// Skipped by moving on to the next word.
    Missed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Waiting,
    Running { started_at: Instant },
    Finished { elapsed: Duration },
}

#[derive(Debug, Clone)]
pub struct Session {
    /// Target words, each split into graphemes.
    target: Vec<Vec<String>>,
    /// Input for every word reached so far; the last entry is the current word.
    typed: Vec<Vec<String>>,
    keystrokes: Vec<Keystroke>,
    duration: Duration,
    phase: Phase,
}

impl Session {
    pub fn new(target: &str, duration: Duration) -> Self {
        Self {
            target: words(target),
            typed: vec![Vec::new()],
            keystrokes: Vec::new(),
            duration,
            phase: Phase::Waiting,
        }
    }

    /// Appends more target words, e.g. when the user is about to run out of words.
    pub fn extend(&mut self, text: &str) {
        self.target.extend(words(text));
    }

    /// Applies one input at `now`. The first non-space character starts the timer; input after
    /// the time limit is ignored.
    pub fn handle(&mut self, input: Input, now: Instant) {
        self.tick(now);
        match (self.phase, input) {
            (Phase::Running { .. }, _) => {}
            (Phase::Waiting, Input::Char(c)) if !c.is_whitespace() => {
                self.phase = Phase::Running { started_at: now };
            }
            (Phase::Waiting | Phase::Finished { .. }, _) => return,
        }
        match input {
            Input::Char(c) if c.is_whitespace() => self.space(),
            Input::Char(c) => self.type_char(c),
            Input::Backspace => self.backspace(),
            Input::DeleteWord => self.delete_word(),
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

    pub fn words(&self) -> &[Vec<String>] {
        &self.target
    }

    pub fn typed(&self) -> &[Vec<String>] {
        &self.typed
    }

    pub fn keystrokes(&self) -> &[Keystroke] {
        &self.keystrokes
    }

    /// Index of the word being typed.
    pub fn current_word(&self) -> usize {
        self.typed.len() - 1
    }

    /// Target words after the current one.
    pub fn remaining_words(&self) -> usize {
        self.target.len().saturating_sub(self.typed.len())
    }

    pub fn letter_state(&self, word: usize, letter: usize) -> LetterState {
        match self.typed.get(word).and_then(|typed| typed.get(letter)) {
            Some(typed) if self.target[word].get(letter) == Some(typed) => LetterState::Correct,
            Some(_) => LetterState::Incorrect,
            None if word < self.current_word() => LetterState::Missed,
            None => LetterState::Untyped,
        }
    }

    /// Letters typed past the end of `word`.
    pub fn extra(&self, word: usize) -> &[String] {
        let len = self.target.get(word).map_or(0, Vec::len);
        self.typed
            .get(word)
            .and_then(|typed| typed.get(len..))
            .unwrap_or(&[])
    }

    pub fn is_word_correct(&self, word: usize) -> bool {
        self.typed.get(word) == self.target.get(word)
    }

    fn type_char(&mut self, c: char) {
        let word = self.current_word();
        let Some(target) = self.target.get(word) else {
            return;
        };
        let typed = &mut self.typed[word];
        if let Some(last) = typed.last_mut().filter(|last| joins(last, c)) {
            last.push(c);
        } else if typed.len() < target.len() + MAX_EXTRA {
            typed.push(c.to_string());
        } else {
            return;
        }
        let index = typed.len() - 1;
        // a partial cluster such as "e" before its combining accent is still on track
        let correct = target
            .get(index)
            .is_some_and(|letter| letter.starts_with(typed[index].as_str()));
        self.keystrokes.push(Keystroke::Char { correct });
    }

    fn space(&mut self) {
        let word = self.current_word();
        if self.typed[word].is_empty() || word + 1 >= self.target.len() {
            return;
        }
        let correct = self.is_word_correct(word);
        self.keystrokes.push(Keystroke::Char { correct });
        self.typed.push(Vec::new());
    }

    fn backspace(&mut self) {
        let word = self.current_word();
        if self.typed[word].pop().is_some() || self.step_back() {
            self.keystrokes.push(Keystroke::Backspace);
        }
    }

    fn delete_word(&mut self) {
        let word = self.current_word();
        if self.typed[word].is_empty() && !self.step_back() {
            return;
        }
        let word = self.current_word();
        self.typed[word].clear();
        self.keystrokes.push(Keystroke::Backspace);
    }

    /// Returns to the end of the previous word, which is only allowed when it has a mistake.
    fn step_back(&mut self) -> bool {
        let word = self.current_word();
        if word == 0 || self.is_word_correct(word - 1) {
            return false;
        }
        self.typed.pop();
        true
    }
}

fn words(text: &str) -> Vec<Vec<String>> {
    text.split_whitespace()
        .map(|word| word.graphemes(true).map(str::to_owned).collect())
        .collect()
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

    fn typed(session: &Session) -> Vec<String> {
        session.typed().iter().map(|word| word.concat()).collect()
    }

    fn states(session: &Session, word: usize) -> Vec<LetterState> {
        (0..session.words()[word].len())
            .map(|i| session.letter_state(word, i))
            .collect()
    }

    #[test]
    fn correct_typing() {
        let now = Instant::now();
        let mut session = Session::new("hi you", LIMIT);
        type_str(&mut session, "hi you", now);

        assert_eq!(typed(&session), ["hi", "you"]);
        assert!(session.is_word_correct(0) && session.is_word_correct(1));
        assert!(
            session
                .keystrokes()
                .iter()
                .all(|k| *k == Keystroke::Char { correct: true })
        );
    }

    #[test]
    fn mistakes_are_marked_incorrect() {
        use LetterState::*;

        let now = Instant::now();
        let mut session = Session::new("abc", LIMIT);
        type_str(&mut session, "axc", now);

        assert_eq!(states(&session, 0), [Correct, Incorrect, Correct]);
        assert_eq!(session.keystrokes()[1], Keystroke::Char { correct: false });
    }

    #[test]
    fn extra_letters_stay_in_their_word() {
        let now = Instant::now();
        let mut session = Session::new("ab cd", LIMIT);
        type_str(&mut session, "abx cd", now);

        assert_eq!(session.extra(0), ["x"]);
        assert!(!session.is_word_correct(0));
        assert!(session.is_word_correct(1));
        assert_eq!(
            session.keystrokes()[2..4],
            [Keystroke::Char { correct: false }; 2]
        );
    }

    #[test]
    fn extra_letters_are_capped() {
        let now = Instant::now();
        let mut session = Session::new("ab", LIMIT);
        type_str(&mut session, &format!("ab{}", "x".repeat(15)), now);

        assert_eq!(session.extra(0).len(), MAX_EXTRA);
        assert_eq!(session.keystrokes().len(), 2 + MAX_EXTRA);
    }

    #[test]
    fn space_skips_the_rest_of_the_word() {
        use LetterState::*;

        let now = Instant::now();
        let mut session = Session::new("abc de", LIMIT);
        type_str(&mut session, "a d", now);

        assert_eq!(states(&session, 0), [Correct, Missed, Missed]);
        assert_eq!(states(&session, 1), [Correct, Untyped]);
        assert_eq!(session.current_word(), 1);
    }

    #[test]
    fn space_on_empty_word_is_ignored() {
        let now = Instant::now();
        let mut session = Session::new("ab cd", LIMIT);
        type_str(&mut session, " ", now);
        assert!(!session.is_started());

        type_str(&mut session, "ab  ", now);
        assert_eq!(session.current_word(), 1);
        assert_eq!(session.keystrokes().len(), 3);
    }

    #[test]
    fn space_on_last_word_is_ignored() {
        let now = Instant::now();
        let mut session = Session::new("ab", LIMIT);
        type_str(&mut session, "ab ", now);

        assert_eq!(session.current_word(), 0);
        assert_eq!(session.keystrokes().len(), 2);
    }

    #[test]
    fn backspace_removes_last_grapheme() {
        let now = Instant::now();
        let mut session = Session::new("abc", LIMIT);
        type_str(&mut session, "ax", now);
        session.handle(Input::Backspace, now);

        assert_eq!(typed(&session), ["a"]);
        assert_eq!(session.letter_state(0, 1), LetterState::Untyped);
        assert_eq!(session.keystrokes().last(), Some(&Keystroke::Backspace));
    }

    #[test]
    fn backspace_returns_to_wrong_previous_word() {
        let now = Instant::now();
        let mut session = Session::new("ab cd", LIMIT);
        type_str(&mut session, "ax ", now);

        session.handle(Input::Backspace, now);
        assert_eq!(session.current_word(), 0);
        assert_eq!(typed(&session), ["ax"]);

        session.handle(Input::Backspace, now);
        assert_eq!(typed(&session), ["a"]);
    }

    #[test]
    fn backspace_does_not_enter_correct_previous_word() {
        let now = Instant::now();
        let mut session = Session::new("ab cd", LIMIT);
        type_str(&mut session, "ab ", now);
        session.handle(Input::Backspace, now);

        assert_eq!(session.current_word(), 1);
        assert_eq!(session.keystrokes().len(), 3);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let now = Instant::now();
        let mut session = Session::new("abc", LIMIT);
        session.handle(Input::Backspace, now);
        assert_eq!(typed(&session), [""]);
        assert_eq!(session.keystrokes(), Vec::<Keystroke>::new());
        assert!(!session.is_started());

        type_str(&mut session, "a", now);
        session.handle(Input::Backspace, now);
        session.handle(Input::Backspace, now);
        assert_eq!(typed(&session), [""]);
        assert_eq!(session.keystrokes().len(), 2);
    }

    #[test]
    fn delete_word_clears_current_word() {
        let now = Instant::now();
        let mut session = Session::new("abc de", LIMIT);
        type_str(&mut session, "ab", now);
        session.handle(Input::DeleteWord, now);

        assert_eq!(typed(&session), [""]);
        assert_eq!(session.keystrokes().last(), Some(&Keystroke::Backspace));
    }

    #[test]
    fn delete_word_returns_to_wrong_previous_word() {
        let now = Instant::now();
        let mut session = Session::new("ab cd", LIMIT);
        type_str(&mut session, "ax ", now);
        session.handle(Input::DeleteWord, now);

        assert_eq!(session.current_word(), 0);
        assert_eq!(typed(&session), [""]);
    }

    #[test]
    fn delete_word_does_not_enter_correct_previous_word() {
        let now = Instant::now();
        let mut session = Session::new("ab cd", LIMIT);
        type_str(&mut session, "ab ", now);
        session.handle(Input::DeleteWord, now);

        assert_eq!(typed(&session), ["ab", ""]);
        assert_eq!(session.keystrokes().len(), 3);
    }

    #[test]
    fn extend_allows_typing_further() {
        let now = Instant::now();
        let mut session = Session::new("ab", LIMIT);
        session.extend("cd");
        type_str(&mut session, "ab c", now);

        assert_eq!(session.words().len(), 2);
        assert_eq!(session.remaining_words(), 0);
        assert_eq!(session.current_word(), 1);
    }

    #[test]
    fn precomposed_accents_are_single_graphemes() {
        use LetterState::*;

        let now = Instant::now();
        let mut session = Session::new("ação", LIMIT);
        assert_eq!(session.words()[0].len(), 4);

        type_str(&mut session, "acão", now);
        assert_eq!(states(&session, 0), [Correct, Incorrect, Correct, Correct]);
    }

    #[test]
    fn combining_mark_joins_previous_grapheme() {
        let now = Instant::now();
        let mut session = Session::new("e\u{301}x", LIMIT);
        assert_eq!(session.words()[0].len(), 2);

        type_str(&mut session, "e\u{301}", now);
        assert_eq!(session.typed()[0], ["e\u{301}"]);
        assert_eq!(session.letter_state(0, 0), LetterState::Correct);
        assert_eq!(session.keystrokes(), [Keystroke::Char { correct: true }; 2]);

        session.handle(Input::Backspace, now);
        assert_eq!(typed(&session), [""]);
    }

    #[test]
    fn combining_mark_on_wrong_base_is_incorrect() {
        let now = Instant::now();
        let mut session = Session::new("e\u{301}", LIMIT);
        type_str(&mut session, "a\u{301}", now);

        assert_eq!(session.letter_state(0, 0), LetterState::Incorrect);
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

        assert_eq!(typed(&session), ["a"]);
        assert_eq!(session.keystrokes().len(), 1);
    }
}
