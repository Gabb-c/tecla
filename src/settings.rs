//! The rows of the settings screen and how each one edits a [`Config`].

use std::borrow::Cow;
use std::fmt::Display;

use crate::config::{Caret, Config, Mode};
use crate::engine::{Confidence, StopOnError};

const TIME_PRESETS: [u64; 4] = [15, 30, 60, 120];
const WORD_PRESETS: [usize; 4] = [10, 25, 50, 100];

/// Which way to move through a list of values, wrapping around at either end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Prev,
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Mode,
    Time,
    Words,
    Caret,
    CaretBlink,
    LiveProgress,
    LiveWpm,
    LiveAccuracy,
    LiveErrors,
    Confidence,
    StopOnError,
    Blind,
    KeyTips,
}

impl Setting {
    pub const ALL: [Self; 13] = [
        Self::Mode,
        Self::Time,
        Self::Words,
        Self::Caret,
        Self::CaretBlink,
        Self::LiveProgress,
        Self::LiveWpm,
        Self::LiveAccuracy,
        Self::LiveErrors,
        Self::Confidence,
        Self::StopOnError,
        Self::Blind,
        Self::KeyTips,
    ];

    /// The neighbouring row in [`Setting::ALL`].
    #[must_use]
    pub fn step(self, direction: Step) -> Self {
        step(&Self::ALL, self, direction)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mode => "mode",
            Self::Time => "time",
            Self::Words => "words",
            Self::Caret => "caret",
            Self::CaretBlink => "caret blink",
            Self::LiveProgress => "live progress",
            Self::LiveWpm => "live wpm",
            Self::LiveAccuracy => "live accuracy",
            Self::LiveErrors => "live errors",
            Self::Confidence => "confidence",
            Self::StopOnError => "stop on error",
            Self::Blind => "blind mode",
            Self::KeyTips => "key tips",
        }
    }

    pub fn section(self) -> Section {
        match self {
            Self::Mode | Self::Time | Self::Words => Section::Test,
            Self::Caret | Self::CaretBlink => Section::Caret,
            Self::LiveProgress | Self::LiveWpm | Self::LiveAccuracy | Self::LiveErrors => {
                Section::LiveStats
            }
            Self::Confidence | Self::StopOnError | Self::Blind => Section::Difficulty,
            Self::KeyTips => Section::Interface,
        }
    }

    /// What the setting does, for its current value where the values differ in kind.
    pub fn description(self, config: &Config) -> &'static str {
        match self {
            Self::Mode => "race the clock, or type a fixed number of words",
            Self::Time => "length of a time test",
            Self::Words => "length of a words test",
            Self::Caret => "shape of the typing cursor",
            Self::CaretBlink => "whether the typing cursor blinks",
            Self::LiveProgress => "time left, or words typed, while you type",
            Self::LiveWpm => "speed while you type",
            Self::LiveAccuracy => "accuracy while you type",
            Self::LiveErrors => "mistake count while you type",
            Self::Confidence => match config.confidence {
                Confidence::Off => "backspace may return to a previous word with a mistake",
                Confidence::On => "backspace stays within the current word",
                Confidence::Max => "backspace does nothing",
            },
            Self::StopOnError => match config.stop_on_error {
                StopOnError::Off => "mistakes never block input",
                StopOnError::Letter => "wrong letters are rejected; space needs a correct word",
                StopOnError::Word => "wrong letters are kept; space needs a correct word",
            },
            Self::Blind => "hides mistakes while you type",
            Self::KeyTips => "key hints on the typing screen",
        }
    }

    pub fn value(self, config: &Config) -> Cow<'static, str> {
        match self {
            Self::Mode => config.mode.as_str().into(),
            Self::Time => format!("{}s", config.time).into(),
            Self::Words => config.words.to_string().into(),
            Self::Caret => config.caret.as_str().into(),
            Self::Confidence => config.confidence.as_str().into(),
            Self::StopOnError => config.stop_on_error.as_str().into(),
            Self::CaretBlink => on_off(config.caret_blink).into(),
            Self::LiveProgress => on_off(config.live_progress).into(),
            Self::LiveWpm => on_off(config.live_wpm).into(),
            Self::LiveAccuracy => on_off(config.live_accuracy).into(),
            Self::LiveErrors => on_off(config.live_errors).into(),
            Self::Blind => on_off(config.blind).into(),
            Self::KeyTips => on_off(config.key_tips).into(),
        }
    }

    /// Whether the setting has any effect given the rest of `config`, e.g. the other mode's
    /// length does not.
    pub fn is_active(self, config: &Config) -> bool {
        match self {
            Self::Time => config.mode == Mode::Time,
            Self::Words => config.mode == Mode::Words,
            Self::CaretBlink => config.caret != Caret::Off,
            _ => true,
        }
    }

    /// Moves the setting to its next or previous value, wrapping around.
    pub fn cycle(self, config: &mut Config, lengths: &Lengths, direction: Step) {
        match self {
            Self::Mode => config.mode = step(&Mode::ALL, config.mode, direction),
            Self::Time => config.time = step(&lengths.time, config.time, direction),
            Self::Words => config.words = step(&lengths.words, config.words, direction),
            Self::Caret => config.caret = step(&Caret::ALL, config.caret, direction),
            Self::Confidence => {
                config.confidence = step(&Confidence::ALL, config.confidence, direction);
            }
            Self::StopOnError => {
                config.stop_on_error = step(&StopOnError::ALL, config.stop_on_error, direction);
            }
            Self::CaretBlink => config.caret_blink ^= true,
            Self::LiveProgress => config.live_progress ^= true,
            Self::LiveWpm => config.live_wpm ^= true,
            Self::LiveAccuracy => config.live_accuracy ^= true,
            Self::LiveErrors => config.live_errors ^= true,
            Self::Blind => config.blind ^= true,
            Self::KeyTips => config.key_tips ^= true,
        }
    }
}

/// A group of related rows on the settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Test,
    Caret,
    LiveStats,
    Difficulty,
    Interface,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Caret => "caret",
            Self::LiveStats => "live stats",
            Self::Difficulty => "difficulty",
            Self::Interface => "interface",
        }
    }
}

/// The lengths each mode cycles through: the presets, with a custom length the config file
/// started with kept in order among them for the whole session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lengths {
    time: Vec<u64>,
    words: Vec<usize>,
}

impl Lengths {
    pub fn new(config: &Config) -> Self {
        Self {
            time: merged(&TIME_PRESETS, config.time),
            words: merged(&WORD_PRESETS, config.words),
        }
    }

    /// The current mode's lengths, the one in `config` selected.
    pub fn choices(&self, config: &Config) -> Vec<Choice> {
        match config.mode {
            Mode::Time => choices(&self.time, config.time),
            Mode::Words => choices(&self.words, config.words),
        }
    }
}

/// One length of the next test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub selected: bool,
}

/// `presets`, which must be sorted, with `custom` inserted in order.
fn merged<T: Copy + Ord>(presets: &[T], custom: T) -> Vec<T> {
    let mut values = presets.to_vec();
    if let Err(index) = values.binary_search(&custom) {
        values.insert(index, custom);
    }
    values
}

fn choices<T: Copy + PartialEq + Display>(values: &[T], current: T) -> Vec<Choice> {
    values
        .iter()
        .map(|&value| Choice {
            label: value.to_string(),
            selected: value == current,
        })
        .collect()
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn step<T: Copy + PartialEq>(all: &[T], current: T, direction: Step) -> T {
    let index = all.iter().position(|&v| v == current).unwrap_or(0);
    let next = match direction {
        Step::Next => (index + 1) % all.len(),
        Step::Prev => (index + all.len() - 1) % all.len(),
    };
    all[next]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_setting_once_in_order() {
        let ordinals: Vec<usize> = Setting::ALL
            .iter()
            .map(|setting| match setting {
                Setting::Mode => 0,
                Setting::Time => 1,
                Setting::Words => 2,
                Setting::Caret => 3,
                Setting::CaretBlink => 4,
                Setting::LiveProgress => 5,
                Setting::LiveWpm => 6,
                Setting::LiveAccuracy => 7,
                Setting::LiveErrors => 8,
                Setting::Confidence => 9,
                Setting::StopOnError => 10,
                Setting::Blind => 11,
                Setting::KeyTips => 12,
            })
            .collect();
        assert_eq!(ordinals, (0..13).collect::<Vec<_>>());
    }

    #[test]
    fn rows_wrap_both_ways() {
        assert_eq!(Setting::Mode.step(Step::Prev), Setting::KeyTips);
        assert_eq!(Setting::KeyTips.step(Step::Next), Setting::Mode);
    }

    #[test]
    fn cycles_wrap_both_ways() {
        let mut config = Config::default();
        let lengths = Lengths::new(&config);
        Setting::Caret.cycle(&mut config, &lengths, Step::Prev);
        assert_eq!(config.caret, Caret::Off);
        Setting::Caret.cycle(&mut config, &lengths, Step::Next);
        assert_eq!(config.caret, Caret::Block);

        config.time = 120;
        Setting::Time.cycle(&mut config, &lengths, Step::Next);
        assert_eq!(config.time, 15);
        Setting::Time.cycle(&mut config, &lengths, Step::Prev);
        assert_eq!(config.time, 120);
    }

    #[test]
    fn presets_are_sorted() {
        assert!(TIME_PRESETS.is_sorted());
        assert!(WORD_PRESETS.is_sorted());
    }

    #[test]
    fn custom_length_stays_in_the_cycle() {
        let mut config = Config {
            words: 40,
            ..Config::default()
        };
        let lengths = Lengths::new(&config);
        Setting::Words.cycle(&mut config, &lengths, Step::Next);
        assert_eq!(config.words, 50);

        Setting::Words.cycle(&mut config, &lengths, Step::Prev);
        assert_eq!(config.words, 40);
    }

    #[test]
    fn toggles_flip_and_show_on_off() {
        let mut config = Config::default();
        let lengths = Lengths::new(&config);
        assert_eq!(Setting::LiveWpm.value(&config), "off");
        Setting::LiveWpm.cycle(&mut config, &lengths, Step::Next);
        assert!(config.live_wpm);
        assert_eq!(Setting::LiveWpm.value(&config), "on");
    }

    #[test]
    fn every_setting_has_a_description() {
        let config = Config::default();
        for setting in Setting::ALL {
            assert!(!setting.description(&config).is_empty(), "{setting:?}");
        }
    }

    #[test]
    fn confidence_description_follows_value() {
        let mut config = Config::default();
        let lengths = Lengths::new(&config);
        Setting::Confidence.cycle(&mut config, &lengths, Step::Next);
        assert_eq!(
            Setting::Confidence.description(&config),
            "backspace stays within the current word"
        );
    }

    #[test]
    fn length_choices_flag_the_selected_preset() {
        let config = Config::default();
        let selected: Vec<String> = Lengths::new(&config)
            .choices(&config)
            .into_iter()
            .filter_map(|choice| choice.selected.then_some(choice.label))
            .collect();
        assert_eq!(selected, ["30"]);
    }

    #[test]
    fn length_choices_merge_a_custom_length_in_order() {
        let config = Config {
            mode: Mode::Words,
            words: 40,
            ..Config::default()
        };
        let labels: Vec<String> = Lengths::new(&config)
            .choices(&config)
            .into_iter()
            .map(|choice| choice.label)
            .collect();
        assert_eq!(labels, ["10", "25", "40", "50", "100"]);
    }

    #[test]
    fn only_the_current_modes_length_is_active() {
        let mut config = Config::default();
        let lengths = Lengths::new(&config);
        assert!(Setting::Time.is_active(&config));
        assert!(!Setting::Words.is_active(&config));

        Setting::Mode.cycle(&mut config, &lengths, Step::Next);
        assert!(!Setting::Time.is_active(&config));
        assert!(Setting::Words.is_active(&config));
    }
}
