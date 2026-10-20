//! User settings persisted as TOML in the XDG config directory.
//!
//! The file lives at `$XDG_CONFIG_HOME/tecla/config.toml` on every platform. Without
//! `XDG_CONFIG_HOME` it falls back to `~/.config` on Linux and macOS, and to `%APPDATA%` on
//! Windows. Missing keys take their default, so a partial file is valid.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::engine::{Confidence, Rules, StopOnError};

const APP_DIR: &str = "tecla";
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
#[serde(default)]
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
    fn problem(&self) -> Option<&'static str> {
        if self.time == 0 {
            return Some("time must be greater than 0");
        }
        if self.time > MAX_TIME {
            return Some("time must be at most 3600");
        }
        if self.words == 0 {
            return Some("words must be greater than 0");
        }
        if self.words > MAX_WORDS {
            return Some("words must be at most 10000");
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
    Invalid { path: PathBuf, reason: &'static str },
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
        let base = BaseDirs::new();
        let fallback = base.as_ref().map(|dirs| {
            if cfg!(windows) {
                dirs.config_dir().to_owned()
            } else {
                dirs.home_dir().join(".config")
            }
        });
        let dir = config_home(std::env::var_os("XDG_CONFIG_HOME"), fallback)?;
        Some(Self::new(dir.join(APP_DIR).join(FILE_NAME)))
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

    /// Writes `config`, creating the directory if needed. The file is replaced atomically so an
    /// interrupted write never leaves it truncated.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory or file cannot be created or written.
    pub fn save(&self, config: &Config) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string(config).map_err(io::Error::other)?;
        let tmp = self.path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &self.path)
    }
}

/// `XDG_CONFIG_HOME` when set to an absolute path, as the XDG spec requires, else `fallback`.
fn config_home(xdg: Option<OsString>, fallback: Option<PathBuf>) -> Option<PathBuf> {
    xdg.map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("tecla-config-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

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

    fn serialized(config: &Config) -> String {
        toml::to_string(config).expect("serialize")
    }

    #[test]
    fn mode_names_match_the_file() {
        for (i, mode) in Mode::ALL.into_iter().enumerate() {
            let ordinal = match mode {
                Mode::Time => 0,
                Mode::Words => 1,
            };
            assert_eq!(i, ordinal, "{mode:?} out of place in ALL");
            let text = serialized(&Config {
                mode,
                ..Config::default()
            });
            assert!(
                text.contains(&format!("mode = \"{}\"", mode.as_str())),
                "{text}"
            );
        }
    }

    #[test]
    fn caret_names_match_the_file() {
        for (i, caret) in Caret::ALL.into_iter().enumerate() {
            let ordinal = match caret {
                Caret::Block => 0,
                Caret::Bar => 1,
                Caret::Underline => 2,
                Caret::Off => 3,
            };
            assert_eq!(i, ordinal, "{caret:?} out of place in ALL");
            let text = serialized(&Config {
                caret,
                ..Config::default()
            });
            assert!(
                text.contains(&format!("caret = \"{}\"", caret.as_str())),
                "{text}"
            );
        }
    }

    #[test]
    fn confidence_names_match_the_file() {
        for (i, confidence) in Confidence::ALL.into_iter().enumerate() {
            let ordinal = match confidence {
                Confidence::Off => 0,
                Confidence::On => 1,
                Confidence::Max => 2,
            };
            assert_eq!(i, ordinal, "{confidence:?} out of place in ALL");
            let text = serialized(&Config {
                confidence,
                ..Config::default()
            });
            let expected = format!("confidence = \"{}\"", confidence.as_str());
            assert!(text.contains(&expected), "{text}");
        }
    }

    #[test]
    fn stop_on_error_names_match_the_file() {
        for (i, stop_on_error) in StopOnError::ALL.into_iter().enumerate() {
            let ordinal = match stop_on_error {
                StopOnError::Off => 0,
                StopOnError::Letter => 1,
                StopOnError::Word => 2,
            };
            assert_eq!(i, ordinal, "{stop_on_error:?} out of place in ALL");
            let text = serialized(&Config {
                stop_on_error,
                ..Config::default()
            });
            let expected = format!("stop_on_error = \"{}\"", stop_on_error.as_str());
            assert!(text.contains(&expected), "{text}");
        }
    }

    #[test]
    fn xdg_config_home_must_be_absolute() {
        let fallback = Some(PathBuf::from("fallback"));
        let absolute = std::env::temp_dir();

        assert_eq!(
            config_home(Some(absolute.clone().into()), fallback.clone()),
            Some(absolute)
        );
        assert_eq!(
            config_home(Some("relative".into()), fallback.clone()),
            fallback
        );
        assert_eq!(config_home(None, fallback.clone()), fallback);
        assert_eq!(config_home(None, None), None);
    }
}
