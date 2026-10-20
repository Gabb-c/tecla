//! The rows of the settings screen and how each one edits a [`Config`].

use std::borrow::Cow;

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
    pub fn cycle(self, config: &mut Config, direction: Step) {
        match self {
            Self::Mode => config.mode = step(&Mode::ALL, config.mode, direction),
            Self::Time => config.time = step_preset(&TIME_PRESETS, config.time, direction),
            Self::Words => config.words = step_preset(&WORD_PRESETS, config.words, direction),
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

/// Like [`step`], but a custom value from the config file moves to the nearest preset in the
/// chosen direction.
fn step_preset<T: Copy + Ord>(presets: &[T], current: T, direction: Step) -> T {
    let (found, wrapped) = match direction {
        Step::Next => (presets.iter().find(|&&p| p > current), presets.first()),
        Step::Prev => (presets.iter().rev().find(|&&p| p < current), presets.last()),
    };
    found.or(wrapped).copied().unwrap_or(current)
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
        Setting::Caret.cycle(&mut config, Step::Prev);
        assert_eq!(config.caret, Caret::Off);
        Setting::Caret.cycle(&mut config, Step::Next);
        assert_eq!(config.caret, Caret::Block);

        config.time = 120;
        Setting::Time.cycle(&mut config, Step::Next);
        assert_eq!(config.time, 15);
        Setting::Time.cycle(&mut config, Step::Prev);
        assert_eq!(config.time, 120);
    }

    #[test]
    fn custom_length_moves_to_nearest_preset() {
        let mut config = Config {
            words: 40,
            ..Config::default()
        };
        Setting::Words.cycle(&mut config, Step::Next);
        assert_eq!(config.words, 50);

        config.words = 40;
        Setting::Words.cycle(&mut config, Step::Prev);
        assert_eq!(config.words, 25);
    }

    #[test]
    fn toggles_flip_and_show_on_off() {
        let mut config = Config::default();
        assert_eq!(Setting::LiveWpm.value(&config), "off");
        Setting::LiveWpm.cycle(&mut config, Step::Next);
        assert!(config.live_wpm);
        assert_eq!(Setting::LiveWpm.value(&config), "on");
    }

    #[test]
    fn only_the_current_modes_length_is_active() {
        let mut config = Config::default();
        assert!(Setting::Time.is_active(&config));
        assert!(!Setting::Words.is_active(&config));

        Setting::Mode.cycle(&mut config, Step::Next);
        assert!(!Setting::Time.is_active(&config));
        assert!(Setting::Words.is_active(&config));
    }
}
