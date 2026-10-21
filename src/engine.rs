//! A single typing session, limited by time or by its word count.
//!
//! Input is matched word by word. Letters are compared positionally within the current word,
//! one grapheme cluster at a time; letters past the end of a word are extra, and space moves on
//! to the next word, leaving any untyped letters missed. The caller supplies every timestamp so
//! the session stays deterministic.

use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;

/// Extra letters accepted past the end of a word.
const MAX_EXTRA: usize = 10;

/// What ends a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// Ends when the time runs out; the caller keeps the words coming with [`Session::extend`].
    Time(Duration),
    /// Ends once the last target word is typed correctly, or space is pressed on it.
    Words,
}

/// How far backspace may reach.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Confidence {
    /// Backspace may return to a previous word that has a mistake.
    #[default]
    Off,
    /// Backspace stays within the current word.
    On,
    /// Backspace does nothing.
    Max,
}

impl Confidence {
    pub const ALL: [Self; 3] = [Self::Off, Self::On, Self::Max];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Max => "max",
        }
    }
}

/// Whether mistakes block further input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StopOnError {
    #[default]
    Off,
    /// Wrong letters are rejected, and space only moves on from a correct word.
    Letter,
    /// Wrong letters are accepted, but space only moves on from a correct word.
    Word,
}

impl StopOnError {
    pub const ALL: [Self; 3] = [Self::Off, Self::Letter, Self::Word];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Letter => "letter",
            Self::Word => "word",
        }
    }
}

/// Optional input restrictions; a rejected keystroke still counts against accuracy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rules {
    pub confidence: Confidence,
    pub stop_on_error: StopOnError,
}

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
    limit: Limit,
    rules: Rules,
    phase: Phase,
}

impl Session {
    /// Starts a session over `words`, each a single word without whitespace.
    pub fn new<'a>(words: impl IntoIterator<Item = &'a str>, limit: Limit) -> Self {
        Self {
            target: split_graphemes(words),
            typed: vec![Vec::new()],
            keystrokes: Vec::new(),
            limit,
            rules: Rules::default(),
            phase: Phase::Waiting,
        }
    }

    #[must_use]
    pub fn with_rules(mut self, rules: Rules) -> Self {
        self.rules = rules;
        self
    }

    /// Appends more target words, e.g. when the user is about to run out of words.
    pub fn extend<'a>(&mut self, words: impl IntoIterator<Item = &'a str>) {
        self.target.extend(split_graphemes(words));
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
            Input::Char(c) if c.is_whitespace() => self.space(now),
            Input::Char(c) => {
                self.type_char(c);
                self.finish_if_last_word_done(now);
            }
            Input::Backspace => self.backspace(),
            Input::DeleteWord => self.delete_word(),
        }
    }

    /// Finishes the session once its time limit has passed.
    pub fn tick(&mut self, now: Instant) {
        if let (Phase::Running { started_at }, Limit::Time(duration)) = (self.phase, self.limit)
            && now.saturating_duration_since(started_at) >= duration
        {
            self.phase = Phase::Finished { elapsed: duration };
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.phase, Phase::Finished { .. })
    }

    pub fn is_started(&self) -> bool {
        !matches!(self.phase, Phase::Waiting)
    }

    /// Time left at `now`, or `None` when the session is not limited by time.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        let Limit::Time(duration) = self.limit else {
            return None;
        };
        Some(duration.saturating_sub(self.elapsed_at(now)))
    }

    /// Time spent typing; zero until the session has finished.
    pub fn elapsed(&self) -> Duration {
        match self.phase {
            Phase::Finished { elapsed } => elapsed,
            _ => Duration::ZERO,
        }
    }

    /// Time spent typing as of `now`.
    pub fn elapsed_at(&self, now: Instant) -> Duration {
        match self.phase {
            Phase::Waiting => Duration::ZERO,
            Phase::Running { started_at } => now.saturating_duration_since(started_at),
            Phase::Finished { elapsed } => elapsed,
        }
    }

    pub fn limit(&self) -> Limit {
        self.limit
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
            Some(typed) if self.letter_matches(word, letter, typed) => LetterState::Correct,
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

    /// Whether `word` is correct so far: every typed letter matches and none are extra.
    pub fn is_on_track(&self, word: usize) -> bool {
        self.typed.get(word).is_some_and(|typed| {
            typed
                .iter()
                .enumerate()
                .all(|(letter, grapheme)| self.letter_matches(word, letter, grapheme))
        })
    }

    /// Whether `typed` matches the target letter. The letter at the caret may be a cluster
    /// still waiting for its combining marks.
    fn letter_matches(&self, word: usize, letter: usize, typed: &str) -> bool {
        let Some(expected) = self.target.get(word).and_then(|target| target.get(letter)) else {
            return false;
        };
        let at_caret = word == self.current_word() && letter + 1 == self.typed[word].len();
        expected == typed || (at_caret && expected.starts_with(typed))
    }

    fn type_char(&mut self, c: char) {
        let word = self.current_word();
        let Some(target) = self.target.get(word) else {
            return;
        };
        let typed = &mut self.typed[word];
        let (index, letter) = match typed.last().filter(|last| joins(last, c)) {
            Some(last) => (typed.len() - 1, format!("{last}{c}")),
            None if typed.len() < target.len() + MAX_EXTRA => (typed.len(), c.to_string()),
            None => return,
        };
        // a partial cluster such as "e" before its combining accent is still on track
        let correct = target
            .get(index)
            .is_some_and(|expected| expected.starts_with(letter.as_str()));
        self.keystrokes.push(Keystroke::Char { correct });
        if !correct && self.rules.stop_on_error == StopOnError::Letter {
            return;
        }
        if index < typed.len() {
            typed[index] = letter;
        } else {
            typed.push(letter);
        }
    }

    fn space(&mut self, now: Instant) {
        let word = self.current_word();
        let last = word + 1 >= self.target.len();
        if self.typed[word].is_empty() || (last && self.limit != Limit::Words) {
            return;
        }
        let correct = self.is_word_correct(word);
        self.keystrokes.push(Keystroke::Char { correct });
        if !correct && self.rules.stop_on_error != StopOnError::Off {
            return;
        }
        if last {
            self.finish(now);
        } else {
            self.typed.push(Vec::new());
        }
    }

    fn finish_if_last_word_done(&mut self, now: Instant) {
        let word = self.current_word();
        if self.limit == Limit::Words && word + 1 == self.target.len() && self.is_word_correct(word)
        {
            self.finish(now);
        }
    }

    fn finish(&mut self, now: Instant) {
        if let Phase::Running { started_at } = self.phase {
            self.phase = Phase::Finished {
                elapsed: now.saturating_duration_since(started_at),
            };
        }
    }

    fn backspace(&mut self) {
        if self.rules.confidence == Confidence::Max {
            return;
        }
        let word = self.current_word();
        if self.typed[word].pop().is_some() || self.step_back() {
            self.keystrokes.push(Keystroke::Backspace);
        }
    }

    fn delete_word(&mut self) {
        if self.rules.confidence == Confidence::Max {
            return;
        }
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
        if word == 0 || self.is_word_correct(word - 1) || self.rules.confidence != Confidence::Off {
            return false;
        }
        self.typed.pop();
        true
    }
}

fn split_graphemes<'a>(words: impl IntoIterator<Item = &'a str>) -> Vec<Vec<String>> {
    words
        .into_iter()
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

    const SECS: Duration = Duration::from_secs(30);
    const LIMIT: Limit = Limit::Time(SECS);

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
        let mut session = Session::new("hi you".split_whitespace(), LIMIT);
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
        let mut session = Session::new("abc".split_whitespace(), LIMIT);
        type_str(&mut session, "axc", now);

        assert_eq!(states(&session, 0), [Correct, Incorrect, Correct]);
        assert_eq!(session.keystrokes()[1], Keystroke::Char { correct: false });
    }

    #[test]
    fn extra_letters_stay_in_their_word() {
        let now = Instant::now();
        let mut session = Session::new("ab cd".split_whitespace(), LIMIT);
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
        let mut session = Session::new("ab".split_whitespace(), LIMIT);
        type_str(&mut session, &format!("ab{}", "x".repeat(15)), now);

        assert_eq!(session.extra(0).len(), MAX_EXTRA);
        assert_eq!(session.keystrokes().len(), 2 + MAX_EXTRA);
    }

    #[test]
    fn space_skips_the_rest_of_the_word() {
        use LetterState::*;

        let now = Instant::now();
        let mut session = Session::new("abc de".split_whitespace(), LIMIT);
        type_str(&mut session, "a d", now);

        assert_eq!(states(&session, 0), [Correct, Missed, Missed]);
        assert_eq!(states(&session, 1), [Correct, Untyped]);
        assert_eq!(session.current_word(), 1);
    }

    #[test]
    fn space_on_empty_word_is_ignored() {
        let now = Instant::now();
        let mut session = Session::new("ab cd".split_whitespace(), LIMIT);
        type_str(&mut session, " ", now);
        assert!(!session.is_started());

        type_str(&mut session, "ab  ", now);
        assert_eq!(session.current_word(), 1);
        assert_eq!(session.keystrokes().len(), 3);
    }

    #[test]
    fn space_on_last_word_is_ignored() {
        let now = Instant::now();
        let mut session = Session::new("ab".split_whitespace(), LIMIT);
        type_str(&mut session, "ab ", now);

        assert_eq!(session.current_word(), 0);
        assert_eq!(session.keystrokes().len(), 2);
    }

    #[test]
    fn backspace_removes_last_grapheme() {
        let now = Instant::now();
        let mut session = Session::new("abc".split_whitespace(), LIMIT);
        type_str(&mut session, "ax", now);
        session.handle(Input::Backspace, now);

        assert_eq!(typed(&session), ["a"]);
        assert_eq!(session.letter_state(0, 1), LetterState::Untyped);
        assert_eq!(session.keystrokes().last(), Some(&Keystroke::Backspace));
    }

    #[test]
    fn backspace_returns_to_wrong_previous_word() {
        let now = Instant::now();
        let mut session = Session::new("ab cd".split_whitespace(), LIMIT);
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
        let mut session = Session::new("ab cd".split_whitespace(), LIMIT);
        type_str(&mut session, "ab ", now);
        session.handle(Input::Backspace, now);

        assert_eq!(session.current_word(), 1);
        assert_eq!(session.keystrokes().len(), 3);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let now = Instant::now();
        let mut session = Session::new("abc".split_whitespace(), LIMIT);
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
        let mut session = Session::new("abc de".split_whitespace(), LIMIT);
        type_str(&mut session, "ab", now);
        session.handle(Input::DeleteWord, now);

        assert_eq!(typed(&session), [""]);
        assert_eq!(session.keystrokes().last(), Some(&Keystroke::Backspace));
    }

    #[test]
    fn delete_word_returns_to_wrong_previous_word() {
        let now = Instant::now();
        let mut session = Session::new("ab cd".split_whitespace(), LIMIT);
        type_str(&mut session, "ax ", now);
        session.handle(Input::DeleteWord, now);

        assert_eq!(session.current_word(), 0);
        assert_eq!(typed(&session), [""]);
    }

    #[test]
    fn delete_word_does_not_enter_correct_previous_word() {
        let now = Instant::now();
        let mut session = Session::new("ab cd".split_whitespace(), LIMIT);
        type_str(&mut session, "ab ", now);
        session.handle(Input::DeleteWord, now);

        assert_eq!(typed(&session), ["ab", ""]);
        assert_eq!(session.keystrokes().len(), 3);
    }

    #[test]
    fn extend_allows_typing_further() {
        let now = Instant::now();
        let mut session = Session::new("ab".split_whitespace(), LIMIT);
        session.extend(["cd"]);
        type_str(&mut session, "ab c", now);

        assert_eq!(session.words().len(), 2);
        assert_eq!(session.remaining_words(), 0);
        assert_eq!(session.current_word(), 1);
    }

    #[test]
    fn precomposed_accents_are_single_graphemes() {
        use LetterState::*;

        let now = Instant::now();
        let mut session = Session::new("ação".split_whitespace(), LIMIT);
        assert_eq!(session.words()[0].len(), 4);

        type_str(&mut session, "acão", now);
        assert_eq!(states(&session, 0), [Correct, Incorrect, Correct, Correct]);
    }

    #[test]
    fn combining_mark_joins_previous_grapheme() {
        let now = Instant::now();
        let mut session = Session::new("e\u{301}x".split_whitespace(), LIMIT);
        assert_eq!(session.words()[0].len(), 2);

        type_str(&mut session, "e\u{301}", now);
        assert_eq!(session.typed()[0], ["e\u{301}"]);
        assert_eq!(session.letter_state(0, 0), LetterState::Correct);
        assert_eq!(session.keystrokes(), [Keystroke::Char { correct: true }; 2]);

        session.handle(Input::Backspace, now);
        assert_eq!(typed(&session), [""]);
    }

    #[test]
    fn partial_cluster_at_caret_is_correct() {
        let now = Instant::now();
        let mut session = Session::new("e\u{301}x".split_whitespace(), LIMIT);
        type_str(&mut session, "e", now);

        assert_eq!(session.letter_state(0, 0), LetterState::Correct);
        assert!(session.is_on_track(0));
    }

    #[test]
    fn partial_cluster_left_behind_is_incorrect() {
        let now = Instant::now();
        let mut session = Session::new("e\u{301}x".split_whitespace(), LIMIT);
        type_str(&mut session, "ex", now);

        assert_eq!(session.letter_state(0, 0), LetterState::Incorrect);
        assert!(!session.is_on_track(0));
    }

    #[test]
    fn combining_mark_on_wrong_base_is_incorrect() {
        let now = Instant::now();
        let mut session = Session::new("e\u{301}".split_whitespace(), LIMIT);
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
        let mut session = Session::new("abc".split_whitespace(), LIMIT);

        session.tick(t0 + SECS * 2);
        assert!(!session.is_started());
        assert_eq!(session.remaining(t0 + SECS * 2), Some(SECS));

        let start = t0 + Duration::from_secs(100);
        session.handle(Input::Char('a'), start);
        assert!(session.is_started());
        assert_eq!(
            session.remaining(start + Duration::from_secs(10)),
            Some(Duration::from_secs(20))
        );

        session.tick(start + SECS.saturating_sub(Duration::from_millis(1)));
        assert!(!session.is_finished());
        session.tick(start + SECS);
        assert!(session.is_finished());
        assert_eq!(session.elapsed(), SECS);
        assert_eq!(session.remaining(start + SECS), Some(Duration::ZERO));
    }

    #[test]
    fn input_after_end_is_ignored() {
        let start = Instant::now();
        let mut session = Session::new("abc".split_whitespace(), LIMIT);
        session.handle(Input::Char('a'), start);

        // the expired timer is noticed even without a tick in between
        session.handle(Input::Char('b'), start + SECS);
        assert!(session.is_finished());
        session.handle(Input::Backspace, start + SECS);

        assert_eq!(typed(&session), ["a"]);
        assert_eq!(session.keystrokes().len(), 1);
    }

    fn with_rules(target: &str, rules: Rules) -> Session {
        Session::new(target.split_whitespace(), LIMIT).with_rules(rules)
    }

    #[test]
    fn words_mode_ends_when_last_word_is_correct() {
        let start = Instant::now();
        let mut session = Session::new("ab cd".split_whitespace(), Limit::Words);
        session.handle(Input::Char('a'), start);
        type_str(&mut session, "b c", start);
        assert!(!session.is_finished());
        assert_eq!(session.remaining(start), None);

        let end = start + Duration::from_secs(7);
        session.handle(Input::Char('d'), end);
        assert!(session.is_finished());
        assert_eq!(session.elapsed(), Duration::from_secs(7));
    }

    #[test]
    fn words_mode_ends_on_space_after_last_word() {
        let start = Instant::now();
        let mut session = Session::new("ab cd".split_whitespace(), Limit::Words);
        type_str(&mut session, "ab cx", start);
        assert!(!session.is_finished());

        session.handle(Input::Char(' '), start + Duration::from_secs(3));
        assert!(session.is_finished());
        assert_eq!(session.elapsed(), Duration::from_secs(3));
        assert_eq!(
            session.keystrokes().last(),
            Some(&Keystroke::Char { correct: false })
        );
    }

    #[test]
    fn words_mode_never_times_out() {
        let start = Instant::now();
        let mut session = Session::new("ab cd".split_whitespace(), Limit::Words);
        session.handle(Input::Char('a'), start);
        session.tick(start + Duration::from_secs(3600));

        assert!(!session.is_finished());
        assert_eq!(
            session.elapsed_at(start + Duration::from_secs(5)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn confidence_on_keeps_backspace_in_current_word() {
        let now = Instant::now();
        let rules = Rules {
            confidence: Confidence::On,
            ..Rules::default()
        };
        let mut session = with_rules("ab cd", rules);
        type_str(&mut session, "ax c", now);

        session.handle(Input::Backspace, now);
        session.handle(Input::Backspace, now);
        session.handle(Input::DeleteWord, now);
        assert_eq!(typed(&session), ["ax", ""]);
        assert_eq!(session.current_word(), 1);
    }

    #[test]
    fn confidence_max_disables_backspace() {
        let now = Instant::now();
        let rules = Rules {
            confidence: Confidence::Max,
            ..Rules::default()
        };
        let mut session = with_rules("abc", rules);
        type_str(&mut session, "ax", now);

        session.handle(Input::Backspace, now);
        session.handle(Input::DeleteWord, now);
        assert_eq!(typed(&session), ["ax"]);
        assert!(!session.keystrokes().contains(&Keystroke::Backspace));
    }

    #[test]
    fn stop_on_letter_rejects_wrong_letters() {
        let now = Instant::now();
        let rules = Rules {
            stop_on_error: StopOnError::Letter,
            ..Rules::default()
        };
        let mut session = with_rules("ab cd", rules);
        type_str(&mut session, "axb", now);
        assert_eq!(typed(&session), ["ab"]);
    }

    #[test]
    fn stop_on_letter_blocks_space_after_wrong_word() {
        let now = Instant::now();
        let rules = Rules {
            stop_on_error: StopOnError::Letter,
            ..Rules::default()
        };
        let mut session = with_rules("ab cd", rules);
        type_str(&mut session, "a ", now);
        assert_eq!(typed(&session), ["a"]);
        assert_eq!(
            session.keystrokes(),
            [
                Keystroke::Char { correct: true },
                Keystroke::Char { correct: false }
            ]
        );
    }

    #[test]
    fn stop_on_word_keeps_letters_but_blocks_space() {
        let now = Instant::now();
        let rules = Rules {
            stop_on_error: StopOnError::Word,
            ..Rules::default()
        };
        let mut session = with_rules("ab cd", rules);
        type_str(&mut session, "ax ", now);
        assert_eq!(typed(&session), ["ax"]);

        session.handle(Input::Backspace, now);
        type_str(&mut session, "b c", now);
        assert_eq!(typed(&session), ["ab", "c"]);
    }
}
