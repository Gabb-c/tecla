//! User settings persisted as TOML in the XDG config directory.
//!
//! The file lives at `config.toml` in [`paths::config_dir`]. Missing keys take their default,
//! so a partial file is valid; unknown keys are errors. Saving edits the file in place, keeping
//! its comments and layout.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

use crate::engine::{Confidence, Rules, StopOnError};
use crate::paths;

const FILE_NAME: &str = "config.toml";
/// Longest time test, in seconds.
const MAX_TIME: u64 = 3600;
/// Most words in a words test; each one is generated up front.
const MAX_WORDS: usize = 10_000;

/// What ends a test.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Time,
    Words,
}

impl Mode {
    pub const ALL: [Self; 2] = [Self::Time, Self::Words];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Words => "words",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Caret {
    #[default]
    Block,
    Bar,
    Underline,
    Off,
}

impl Caret {
    pub const ALL: [Self; 4] = [Self::Block, Self::Bar, Self::Underline, Self::Off];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Bar => "bar",
            Self::Underline => "underline",
            Self::Off => "off",
        }
    }
}

// the engine stays free of serde, so its enums get their file format here
#[derive(Serialize, Deserialize)]
#[serde(remote = "Confidence", rename_all = "lowercase")]
enum ConfidenceDef {
    Off,
    On,
    Max,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "StopOnError", rename_all = "lowercase")]
enum StopOnErrorDef {
    Off,
    Letter,
    Word,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent on/off settings, one key each in the file"
)]
pub struct Config {
    pub mode: Mode,
    /// Test length in seconds for [`Mode::Time`].
    pub time: u64,
    /// Word count for [`Mode::Words`].
    pub words: usize,
    pub caret: Caret,
    pub caret_blink: bool,
    /// Countdown in time mode, words typed in words mode.
    pub live_progress: bool,
    pub live_wpm: bool,
    pub live_accuracy: bool,
    pub live_errors: bool,
    #[serde(with = "ConfidenceDef")]
    pub confidence: Confidence,
    #[serde(with = "StopOnErrorDef")]
    pub stop_on_error: StopOnError,
    /// Hides mistakes while typing.
    pub blind: bool,
    pub key_tips: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Time,
            time: 30,
            words: 25,
            caret: Caret::Block,
            caret_blink: false,
            live_progress: true,
            live_wpm: false,
            live_accuracy: false,
            live_errors: false,
            confidence: Confidence::Off,
            stop_on_error: StopOnError::Off,
            blind: false,
            key_tips: true,
        }
    }
}

impl Config {
    pub fn rules(&self) -> Rules {
        Rules {
            confidence: self.confidence,
            stop_on_error: self.stop_on_error,
        }
    }

    /// The first out-of-range value, if any.
    fn problem(&self) -> Option<String> {
        if !(1..=MAX_TIME).contains(&self.time) {
            return Some(format!("time must be between 1 and {MAX_TIME}"));
        }
        if !(1..=MAX_WORDS).contains(&self.words) {
            return Some(format!("words must be between 1 and {MAX_WORDS}"));
        }
        None
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("invalid {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("invalid {}: {reason}", .path.display())]
    Invalid { path: PathBuf, reason: String },
}

#[derive(Debug, Clone)]
pub struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The config file in the XDG config directory, or `None` when no home directory is known.
    pub fn in_config_dir() -> Option<Self> {
        Some(Self::new(paths::config_dir()?.join(FILE_NAME)))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the config; a missing file is the default config.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, is not valid TOML for [`Config`], or holds
    /// an out-of-range value.
    pub fn load(&self) -> Result<Config, ConfigError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        let config: Config = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: self.path.clone(),
            source: Box::new(source),
        })?;
        if let Some(reason) = config.problem() {
            return Err(ConfigError::Invalid {
                path: self.path.clone(),
                reason,
            });
        }
        Ok(config)
    }

    /// Writes `config` into the existing file, keeping its comments and layout, and creating
    /// the directory if needed. The file is replaced atomically so an interrupted write never
    /// leaves it truncated.
    ///
    /// # Errors
    ///
    /// Returns an error if the existing file cannot be read or is not valid TOML, or the
    /// directory or file cannot be created or written.
    pub fn save(&self, config: &Config) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut doc = match fs::read_to_string(&self.path) {
            Ok(text) => parse_document(&text)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => DocumentMut::new(),
            Err(e) => return Err(e),
        };
        let fresh = toml::to_string(config).map_err(io::Error::other)?;
        merge(&mut doc, &parse_document(&fresh)?);

        let tmp = self.path.with_extension("toml.tmp");
        let mut file = fs::File::create(&tmp)?;
        file.write_all(doc.to_string().as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, &self.path)
    }
}

fn parse_document(text: &str) -> io::Result<DocumentMut> {
    text.parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Copies every key of `fresh` into `doc`. A changed value keeps the comment and spacing
/// around it, and an unchanged one is left exactly as written.
fn merge(doc: &mut DocumentMut, fresh: &DocumentMut) {
    for (key, item) in fresh.iter() {
        let (Some(old), Some(new)) = (
            doc.get_mut(key).and_then(|i| i.as_value_mut()),
            item.as_value(),
        ) else {
            doc.insert(key, item.clone());
            continue;
        };
        if undecorated(old) != undecorated(new) {
            let decor = old.decor().clone();
            *old = new.clone();
            *old.decor_mut() = decor;
        }
    }
}

fn undecorated(value: &toml_edit::Value) -> String {
    let mut value = value.clone();
    value.decor_mut().clear();
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn file(dir: &TempDir) -> ConfigFile {
        ConfigFile::new(dir.0.join("tecla/config.toml"))
    }

    #[test]
    fn round_trip() {
        let dir = TempDir::new("round-trip");
        let file = file(&dir);
        let config = Config {
            mode: Mode::Words,
            words: 50,
            caret: Caret::Bar,
            confidence: Confidence::Max,
            stop_on_error: StopOnError::Word,
            live_wpm: true,
            ..Config::default()
        };

        file.save(&config).expect("save");
        file.save(&config).expect("overwrite");
        assert_eq!(file.load().expect("load"), config);
    }

    #[test]
    fn missing_file_is_default() {
        let dir = TempDir::new("missing");
        assert_eq!(file(&dir).load().expect("load"), Config::default());
    }

    #[test]
    fn missing_keys_take_defaults() {
        let dir = TempDir::new("partial");
        let file = file(&dir);
        fs::create_dir_all(file.path().parent().unwrap()).expect("create dir");
        fs::write(file.path(), "mode = \"words\"\nwords = 10\n").expect("write");

        let config = file.load().expect("load");
        assert_eq!(config.mode, Mode::Words);
        assert_eq!(config.words, 10);
        assert_eq!(config.time, Config::default().time);
    }

    #[test]
    fn bad_values_are_errors() {
        let dir = TempDir::new("bad");
        let file = file(&dir);
        fs::create_dir_all(file.path().parent().unwrap()).expect("create dir");

        fs::write(file.path(), "caret = \"triangle\"\n").expect("write");
        assert!(matches!(file.load(), Err(ConfigError::Parse { .. })));

        for text in [
            "time = 0\n",
            "time = 3601\n",
            "words = 0\n",
            "words = 10001\n",
        ] {
            fs::write(file.path(), text).expect("write");
            assert!(
                matches!(file.load(), Err(ConfigError::Invalid { .. })),
                "{text:?}"
            );
        }
    }

    #[test]
    fn unknown_key_is_an_error() {
        let dir = TempDir::new("unknown");
        let file = file(&dir);
        fs::create_dir_all(file.path().parent().unwrap()).expect("create dir");
        fs::write(file.path(), "live_wmp = true\n").expect("write");

        assert!(matches!(file.load(), Err(ConfigError::Parse { .. })));
    }

    #[test]
    fn save_keeps_comments_and_layout() {
        let dir = TempDir::new("comments");
        let file = file(&dir);
        fs::create_dir_all(file.path().parent().unwrap()).expect("create dir");
        fs::write(
            file.path(),
            "# my settings\nmode = \"time\"  # time | words\ncaret = \"bar\"\n",
        )
        .expect("write");
        let config = Config {
            mode: Mode::Words,
            ..file.load().expect("load")
        };

        file.save(&config).expect("save");
        let text = fs::read_to_string(file.path()).expect("read");
        assert!(
            text.starts_with("# my settings\nmode = \"words\"  # time | words\ncaret = \"bar\"\n"),
            "{text}"
        );
        assert_eq!(file.load().expect("reload"), config);
    }

    /// Checks that `all` lists every value in declaration order (`ordinal`), and that each
    /// one's `name` is what the file stores under `key`.
    fn assert_names_match<T: Copy + std::fmt::Debug>(
        all: &[T],
        ordinal: fn(T) -> usize,
        key: &str,
        name: fn(T) -> &'static str,
        with: fn(T) -> Config,
    ) {
        for (i, &value) in all.iter().enumerate() {
            assert_eq!(i, ordinal(value), "{value:?} out of place in ALL");
            let text = toml::to_string(&with(value)).expect("serialize");
            let expected = format!("{key} = \"{}\"", name(value));
            assert!(text.contains(&expected), "{text}");
        }
    }

    #[test]
    fn mode_names_match_the_file() {
        assert_names_match(
            &Mode::ALL,
            |mode| match mode {
                Mode::Time => 0,
                Mode::Words => 1,
            },
            "mode",
            Mode::as_str,
            |mode| Config {
                mode,
                ..Config::default()
            },
        );
    }

    #[test]
    fn caret_names_match_the_file() {
        assert_names_match(
            &Caret::ALL,
            |caret| match caret {
                Caret::Block => 0,
                Caret::Bar => 1,
                Caret::Underline => 2,
                Caret::Off => 3,
            },
            "caret",
            Caret::as_str,
            |caret| Config {
                caret,
                ..Config::default()
            },
        );
    }

    #[test]
    fn confidence_names_match_the_file() {
        assert_names_match(
            &Confidence::ALL,
            |confidence| match confidence {
                Confidence::Off => 0,
                Confidence::On => 1,
                Confidence::Max => 2,
            },
            "confidence",
            Confidence::as_str,
            |confidence| Config {
                confidence,
                ..Config::default()
            },
        );
    }

    #[test]
    fn stop_on_error_names_match_the_file() {
        assert_names_match(
            &StopOnError::ALL,
            |stop_on_error| match stop_on_error {
                StopOnError::Off => 0,
                StopOnError::Letter => 1,
                StopOnError::Word => 2,
            },
            "stop_on_error",
            StopOnError::as_str,
            |stop_on_error| Config {
                stop_on_error,
                ..Config::default()
            },
        );
    }
}
